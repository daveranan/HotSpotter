use std::collections::BTreeSet;

use hot_trimmer_domain::{
    ErrorCode, FillBehavior, IdColor, Layout, LayoutContractError, LayoutItem, LayoutOrder,
    LayoutPreset, LayoutRegion, LayoutRequest, NormalizedBounds, PackPriority, PixelBounds,
    PixelSize, RegionId, RegionLocks, TrimAxis, UserFacingError,
};
use thiserror::Error;

/// Produces a deterministic layout from immutable source/patch descriptors.
///
/// Existing compatible regions keep their IDs and colors. Dimension locks keep the corresponding size,
/// position locks keep the position, and automatic placement never writes back into patch definitions.
///
/// # Errors
///
/// Returns a typed diagnostic without a partial layout when contracts, locks, padding/bleed, or requested
/// dimensions cannot fit without overlap.
#[allow(clippy::too_many_lines)]
pub fn solve_layout(request: &LayoutRequest) -> Result<Layout, LayoutSolveError> {
    request.validate().map_err(LayoutSolveError::Contract)?;
    let active: Vec<(usize, &LayoutItem)> = request
        .items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.enabled && item.participates)
        .collect();
    if active.is_empty() {
        return Err(LayoutSolveError::NoParticipatingRegions);
    }

    if request.preset != LayoutPreset::Atlas
        && active
            .iter()
            .all(|(_, item)| item.constraints.template_bounds.is_some())
    {
        return solve_template_layout(request, &active);
    }
    CustomAtlasLayoutEngine::solve(request, active)
}

/// The existing deterministic packer behind explicit Custom Atlas mode.
struct CustomAtlasLayoutEngine;

impl CustomAtlasLayoutEngine {
    #[allow(clippy::too_many_lines)]
    fn solve(request: &LayoutRequest, active: Vec<(usize, &LayoutItem)>) -> Result<Layout, LayoutSolveError> {
    let mut used_ids = BTreeSet::new();
    let mut prepared = Vec::with_capacity(active.len());
    let grid = grid_dimensions(request.preset, active.len(), request.settings.output);
    let maximum_clearance = active
        .iter()
        .map(|(_, item)| {
            item.padding_px.unwrap_or(request.settings.padding_px)
                + item.bleed_px.unwrap_or(request.settings.bleed_px)
        })
        .max()
        .unwrap_or(0);
    let cell = grid_cell(request.settings.output, grid, maximum_clearance).ok_or_else(|| {
        LayoutSolveError::ImpossibleFit {
            item_key: active[0].1.key.clone(),
            requested: PixelSize {
                width: 1,
                height: 1,
            },
            output: request.settings.output,
            reason: "padding and bleed consume the available output".into(),
        }
    })?;

    for (input_index, item) in active {
        let existing = compatible_existing(request, item);
        let id = choose_region_id(
            item,
            existing,
            request.settings.auto_pack.seed,
            &mut used_ids,
        );
        let locks = existing.map_or_else(RegionLocks::default, |region| region.locks);
        let mut size = target_size(request.preset, item, cell);
        if let Some(region) = existing {
            if locks.width {
                size.width = region.bounds.width;
            }
            if locks.height {
                size.height = region.bounds.height;
            }
        }
        if let Some(width) = item.constraints.fixed_width_px {
            size.width = width;
        }
        if let Some(height) = item.constraints.fixed_height_px {
            size.height = height;
        }
        if let Some(fixed) = request.settings.fixed_selected_size
            && fixed.region_id == id
        {
            size = fixed.size;
        }
        if !size.is_nonzero() {
            return Err(LayoutSolveError::ImpossibleFit {
                item_key: item.key.clone(),
                requested: size,
                output: request.settings.output,
                reason: "the requested content size is zero".into(),
            });
        }
        let padding_px = item.padding_px.unwrap_or(request.settings.padding_px);
        let bleed_px = item.bleed_px.unwrap_or(request.settings.bleed_px);
        let clearance =
            padding_px
                .checked_add(bleed_px)
                .ok_or_else(|| LayoutSolveError::ImpossibleFit {
                    item_key: item.key.clone(),
                    requested: size,
                    output: request.settings.output,
                    reason: "padding plus bleed overflows the supported integer range".into(),
                })?;
        let position = existing
            .filter(|_| locks.position)
            .map(|region| (region.bounds.x, region.bounds.y));
        prepared.push(PreparedRegion {
            input_index,
            item,
            existing,
            id,
            id_color: existing
                .filter(|region| region.id == id)
                .map_or_else(|| IdColor::for_region(id), |region| region.id_color),
            locks,
            size,
            padding_px,
            bleed_px,
            clearance,
            locked_position: position,
        });
    }

    sort_prepared(&mut prepared, request.settings.order);
    resolve_id_colors(&mut prepared);
    let use_grid_fast_path = request.settings.auto_pack.enabled
        && prepared.iter().all(|region| {
            region.existing.is_none()
                && region.locked_position.is_none()
                && region.clearance == maximum_clearance
                && region.size.width <= cell.width
                && region.size.height <= cell.height
        });
    let mut occupied: Vec<Placed> = Vec::with_capacity(prepared.len());
    let mut results = Vec::with_capacity(prepared.len());

    // Position-locked regions are authoritative obstacles irrespective of display order.
    for prepared_region in prepared
        .iter()
        .filter(|region| region.locked_position.is_some())
    {
        let Some((x, y)) = prepared_region.locked_position else {
            continue;
        };
        let bounds = PixelBounds {
            x,
            y,
            width: prepared_region.size.width,
            height: prepared_region.size.height,
        };
        if !fits_output(bounds, prepared_region.clearance, request.settings.output) {
            return Err(LayoutSolveError::LockedRegionOutOfBounds {
                region_id: prepared_region.id,
            });
        }
        if let Some(other) = occupied.iter().find(|placed| {
            !separated(
                bounds,
                prepared_region.clearance,
                placed.bounds,
                placed.clearance,
            )
        }) {
            return Err(LayoutSolveError::LockedRegionsOverlap {
                first: prepared_region.id,
                second: other.id,
            });
        }
        validate_caps_fit(prepared_region, bounds)?;
        occupied.push(Placed {
            id: prepared_region.id,
            bounds,
            clearance: prepared_region.clearance,
        });
    }

    for (order_index, prepared_region) in prepared.iter().enumerate() {
        let bounds = if let Some((x, y)) = prepared_region.locked_position {
            PixelBounds {
                x,
                y,
                width: prepared_region.size.width,
                height: prepared_region.size.height,
            }
        } else if !request.settings.auto_pack.enabled {
            let Some(existing) = prepared_region.existing else {
                return Err(LayoutSolveError::AutoPackDisabledMissingPlacement {
                    item_key: prepared_region.item.key.clone(),
                });
            };
            let bounds = PixelBounds {
                x: existing.bounds.x,
                y: existing.bounds.y,
                width: prepared_region.size.width,
                height: prepared_region.size.height,
            };
            if !fits_output(bounds, prepared_region.clearance, request.settings.output)
                || occupied.iter().any(|placed| {
                    !separated(
                        bounds,
                        prepared_region.clearance,
                        placed.bounds,
                        placed.clearance,
                    )
                })
            {
                return Err(LayoutSolveError::ImpossibleFit {
                    item_key: prepared_region.item.key.clone(),
                    requested: prepared_region.size,
                    output: request.settings.output,
                    reason:
                        "the retained manual position is out of bounds or overlaps another region"
                            .into(),
                });
            }
            bounds
        } else if use_grid_fast_path {
            grid_position(
                order_index,
                grid,
                cell,
                maximum_clearance,
                prepared_region.size,
                request.settings.auto_pack.priority,
            )
        } else {
            find_position(
                prepared_region,
                &occupied,
                request.settings.output,
                request.settings.auto_pack.priority,
            )
            .ok_or_else(|| LayoutSolveError::ImpossibleFit {
                item_key: prepared_region.item.key.clone(),
                requested: prepared_region.size,
                output: request.settings.output,
                reason: format!(
                    "no non-overlapping position remains after reserving {} px of padding plus bleed",
                    prepared_region.clearance
                ),
            })?
        };
        validate_caps_fit(prepared_region, bounds)?;
        if prepared_region.locked_position.is_none() {
            occupied.push(Placed {
                id: prepared_region.id,
                bounds,
                clearance: prepared_region.clearance,
            });
        }
        results.push(LayoutRegion {
            id: prepared_region.id,
            item_key: prepared_region.item.key.clone(),
            fill: prepared_region.item.fill.clone(),
            behavior: prepared_region.item.behavior,
            trim_caps: prepared_region.item.trim_caps,
            bounds,
            padding_px: prepared_region.padding_px,
            bleed_px: prepared_region.bleed_px,
            order_index: u32::try_from(order_index).map_err(|_| {
                LayoutSolveError::Contract(LayoutContractError::TooManyRegions {
                    found: prepared.len(),
                    maximum: hot_trimmer_domain::MAX_LAYOUT_REGIONS,
                })
            })?,
            locks: prepared_region.locks,
            id_color: prepared_region.id_color,
        });
    }

    let layout = Layout {
        id: request.layout_id,
        preset: request.preset,
        settings: request.settings.clone(),
        regions: results,
    };
    validate_layout(&layout)?;
    Ok(layout)
    }
}

/// Solves a versioned Hotspot or Trim template directly from normalized slot bounds.
///
/// Template topology is authoritative: slots never enter the generic atlas packer, never move
/// because another slot changes content, and carry zero external clearance so shared edges remain exact.
fn solve_template_layout(
    request: &LayoutRequest,
    active: &[(usize, &LayoutItem)],
) -> Result<Layout, LayoutSolveError> {
    let mut used_ids = BTreeSet::new();
    let mut used_colors = BTreeSet::new();
    let mut regions = Vec::with_capacity(active.len());

    for (order_index, (_, item)) in active.iter().enumerate() {
        let normalized = item.constraints.template_bounds.ok_or_else(|| {
            LayoutSolveError::Contract(LayoutContractError::InvalidItemSize(item.key.clone()))
        })?;
        let bounds = template_pixel_bounds(normalized, request.settings.output);
        if !bounds.size().is_nonzero() {
            return Err(LayoutSolveError::ImpossibleFit {
                item_key: item.key.clone(),
                requested: bounds.size(),
                output: request.settings.output,
                reason: "the template slot rounds to zero pixels at this output resolution".into(),
            });
        }

        let existing = compatible_existing(request, item);
        let id = choose_region_id(
            item,
            existing,
            request.settings.auto_pack.seed,
            &mut used_ids,
        );
        let mut salt = 0_u32;
        let mut id_color = existing
            .filter(|region| region.id == id)
            .map_or_else(|| IdColor::for_region(id), |region| region.id_color);
        while !used_colors.insert(id_color) {
            salt = salt.saturating_add(1);
            id_color = IdColor::for_region_with_salt(id, salt);
        }

        regions.push(LayoutRegion {
            id,
            item_key: item.key.clone(),
            fill: item.fill.clone(),
            behavior: item.behavior,
            trim_caps: item.trim_caps,
            bounds,
            padding_px: 0,
            bleed_px: 0,
            order_index: u32::try_from(order_index).map_err(|_| {
                LayoutSolveError::Contract(LayoutContractError::TooManyRegions {
                    found: active.len(),
                    maximum: hot_trimmer_domain::MAX_LAYOUT_REGIONS,
                })
            })?,
            locks: RegionLocks::default(),
            id_color,
        });
    }

    let layout = Layout {
        id: request.layout_id,
        preset: request.preset,
        settings: request.settings.clone(),
        regions,
    };
    validate_layout(&layout)?;
    Ok(layout)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn template_pixel_bounds(bounds: NormalizedBounds, output: PixelSize) -> PixelBounds {
    let pixel = |value: f64, edge: u32| {
        (value * f64::from(edge))
            .round()
            .clamp(0.0, f64::from(edge)) as u32
    };
    let x = pixel(bounds.x.get(), output.width);
    let y = pixel(bounds.y.get(), output.height);
    let right = pixel(bounds.x.get() + bounds.width.get(), output.width);
    let bottom = pixel(bounds.y.get() + bounds.height.get(), output.height);
    PixelBounds {
        x,
        y,
        width: right.saturating_sub(x),
        height: bottom.saturating_sub(y),
    }
}

/// Validates authoritative bounds, stable order/IDs, cap spans, and the padding-plus-bleed exclusion policy.
///
/// # Errors
///
/// Returns the first deterministic diagnostic for a malformed persisted or manually edited layout.
pub fn validate_layout(layout: &Layout) -> Result<(), LayoutSolveError> {
    let shell = LayoutRequest {
        layout_id: layout.id,
        preset: layout.preset,
        settings: layout.settings.clone(),
        items: Vec::new(),
        existing_regions: layout.regions.clone(),
    };
    shell.validate().map_err(LayoutSolveError::Contract)?;
    let mut ids = BTreeSet::new();
    let mut keys = BTreeSet::new();
    let mut orders = BTreeSet::new();
    for region in &layout.regions {
        if !ids.insert(region.id) {
            return Err(LayoutSolveError::DuplicateRegionId(region.id));
        }
        if !keys.insert(region.item_key.as_str()) {
            return Err(LayoutSolveError::DuplicateItemKey(region.item_key.clone()));
        }
        if !orders.insert(region.order_index) {
            return Err(LayoutSolveError::DuplicateOrder(region.order_index));
        }
        let clearance = region.padding_px.checked_add(region.bleed_px).ok_or(
            LayoutSolveError::InsetOverflow {
                region_id: region.id,
            },
        )?;
        if !fits_output(region.bounds, clearance, layout.settings.output) {
            return Err(LayoutSolveError::RegionOutOfBounds {
                region_id: region.id,
            });
        }
        if let Some(caps) = region.trim_caps {
            let span = match caps.axis {
                TrimAxis::Horizontal => region.bounds.width,
                TrimAxis::Vertical => region.bounds.height,
            };
            if caps.leading_px.saturating_add(caps.trailing_px) >= span {
                return Err(LayoutSolveError::TrimCapsDoNotFit {
                    region_id: region.id,
                });
            }
        }
    }
    for (index, first) in layout.regions.iter().enumerate() {
        let first_clearance = first.padding_px + first.bleed_px;
        for second in layout.regions.iter().skip(index + 1) {
            let second_clearance = second.padding_px + second.bleed_px;
            if !separated(
                first.bounds,
                first_clearance,
                second.bounds,
                second_clearance,
            ) {
                return Err(LayoutSolveError::RegionsOverlap {
                    first: first.id,
                    second: second.id,
                });
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Grid {
    columns: u32,
    rows: u32,
}

fn grid_dimensions(preset: LayoutPreset, count: usize, output: PixelSize) -> Grid {
    let count = u32::try_from(count).expect("bounded region count");
    match preset {
        LayoutPreset::HorizontalTrims => Grid {
            columns: 1,
            rows: count,
        },
        LayoutPreset::VerticalTrims => Grid {
            columns: count,
            rows: 1,
        },
        LayoutPreset::Balanced | LayoutPreset::ModularKit | LayoutPreset::Atlas => {
            let target = (f64::from(count) * f64::from(output.width) / f64::from(output.height))
                .sqrt()
                .ceil();
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let columns = (target as u32).clamp(1, count);
            Grid {
                columns,
                rows: count.div_ceil(columns),
            }
        }
    }
}

fn grid_cell(output: PixelSize, grid: Grid, clearance: u32) -> Option<PixelSize> {
    let horizontal_insets = clearance.checked_mul(grid.columns.checked_mul(2)?)?;
    let vertical_insets = clearance.checked_mul(grid.rows.checked_mul(2)?)?;
    Some(PixelSize {
        width: output.width.checked_sub(horizontal_insets)? / grid.columns,
        height: output.height.checked_sub(vertical_insets)? / grid.rows,
    })
    .filter(|size| size.is_nonzero())
}

fn grid_position(
    order_index: usize,
    grid: Grid,
    cell: PixelSize,
    clearance: u32,
    size: PixelSize,
    priority: PackPriority,
) -> PixelBounds {
    let index = u32::try_from(order_index).expect("bounded region count");
    let (column, row) = match priority {
        PackPriority::VerticalStrips => (index / grid.rows, index % grid.rows),
        PackPriority::Balanced | PackPriority::HorizontalStrips => {
            (index % grid.columns, index / grid.columns)
        }
    };
    PixelBounds {
        x: clearance + column * (cell.width + clearance * 2),
        y: clearance + row * (cell.height + clearance * 2),
        width: size.width,
        height: size.height,
    }
}

fn target_size(preset: LayoutPreset, item: &LayoutItem, cell: PixelSize) -> PixelSize {
    if matches!(
        preset,
        LayoutPreset::HorizontalTrims | LayoutPreset::VerticalTrims | LayoutPreset::ModularKit
    ) {
        return cell;
    }
    let aspect = f64::from(item.natural_size.width) / f64::from(item.natural_size.height);
    let cell_aspect = f64::from(cell.width) / f64::from(cell.height);
    let mut size = if aspect >= cell_aspect {
        PixelSize {
            width: cell.width,
            height: rounded_positive(f64::from(cell.width) / aspect).min(cell.height),
        }
    } else {
        PixelSize {
            width: rounded_positive(f64::from(cell.height) * aspect).min(cell.width),
            height: cell.height,
        }
    };
    match item.behavior {
        FillBehavior::HorizontalLoop => size.width = cell.width,
        FillBehavior::VerticalLoop => size.height = cell.height,
        FillBehavior::TrimCap => {
            if item
                .trim_caps
                .is_some_and(|caps| caps.axis == TrimAxis::Horizontal)
            {
                size.width = cell.width;
            } else {
                size.height = cell.height;
            }
        }
        FillBehavior::Tile | FillBehavior::Stretch | FillBehavior::UniqueDetail => {}
    }
    size
}

fn rounded_positive(value: f64) -> u32 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let result = value.round().max(1.0) as u32;
    result
}

struct PreparedRegion<'a> {
    input_index: usize,
    item: &'a LayoutItem,
    existing: Option<&'a LayoutRegion>,
    id: RegionId,
    id_color: IdColor,
    locks: RegionLocks,
    size: PixelSize,
    padding_px: u32,
    bleed_px: u32,
    clearance: u32,
    locked_position: Option<(u32, u32)>,
}

#[derive(Clone, Copy)]
struct Placed {
    id: RegionId,
    bounds: PixelBounds,
    clearance: u32,
}

fn compatible_existing<'a>(
    request: &'a LayoutRequest,
    item: &LayoutItem,
) -> Option<&'a LayoutRegion> {
    item.region_id
        .and_then(|id| {
            request
                .existing_regions
                .iter()
                .find(|region| region.id == id)
        })
        .filter(|region| region.item_key == item.key && region.fill == item.fill)
        .or_else(|| {
            request
                .existing_regions
                .iter()
                .find(|region| region.item_key == item.key && region.fill == item.fill)
        })
}

fn choose_region_id(
    item: &LayoutItem,
    existing: Option<&LayoutRegion>,
    seed: u64,
    used: &mut BTreeSet<RegionId>,
) -> RegionId {
    if let Some(id) = item.region_id.or_else(|| existing.map(|region| region.id))
        && used.insert(id)
    {
        return id;
    }
    for collision in 0_u64.. {
        let id = deterministic_region_id(seed, &item.key, collision);
        if used.insert(id) {
            return id;
        }
    }
    unreachable!("u64 collision space is finite but cannot be exhausted in a bounded request")
}

fn deterministic_region_id(seed: u64, key: &str, collision: u64) -> RegionId {
    let mut first = 0xcbf2_9ce4_8422_2325_u64 ^ seed;
    let mut second = 0x8422_2325_cbf2_9ce4_u64 ^ collision.rotate_left(23);
    for byte in key.bytes().chain(collision.to_le_bytes()) {
        first ^= u64::from(byte);
        first = first.wrapping_mul(0x0000_0100_0000_01b3);
        second ^= u64::from(byte).wrapping_add(first.rotate_left(17));
        second = second.wrapping_mul(0x9e37_79b1_85eb_ca87);
    }
    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(&first.to_be_bytes());
    bytes[8..].copy_from_slice(&second.to_be_bytes());
    // RFC 4122 variant/version bits make the deterministic value production-shaped as a UUID.
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    RegionId::from_bytes(bytes)
}

fn sort_prepared(regions: &mut [PreparedRegion<'_>], order: LayoutOrder) {
    regions.sort_by(|left, right| {
        let primary = match order {
            LayoutOrder::Input => left.input_index.cmp(&right.input_index),
            LayoutOrder::LargestFirst => area(right.size).cmp(&area(left.size)),
            LayoutOrder::HorizontalFirst => behavior_rank(left.item.behavior, true)
                .cmp(&behavior_rank(right.item.behavior, true)),
            LayoutOrder::VerticalFirst => behavior_rank(left.item.behavior, false)
                .cmp(&behavior_rank(right.item.behavior, false)),
        };
        primary
            .then_with(|| left.item.key.cmp(&right.item.key))
            .then_with(|| left.input_index.cmp(&right.input_index))
    });
}

fn resolve_id_colors(regions: &mut [PreparedRegion<'_>]) {
    // Reserve every compatible persisted color first so newly introduced regions cannot steal a color
    // from a later persisted region in the stable ordering pass.
    let mut used: BTreeSet<IdColor> = regions
        .iter()
        .filter(|region| {
            region
                .existing
                .is_some_and(|existing| existing.id == region.id)
        })
        .map(|region| region.id_color)
        .collect();
    for region in regions {
        if region
            .existing
            .is_some_and(|existing| existing.id == region.id)
        {
            continue;
        }
        for salt in 0.. {
            let candidate = IdColor::for_region_with_salt(region.id, salt);
            if used.insert(candidate) {
                region.id_color = candidate;
                break;
            }
        }
    }
}

fn area(size: PixelSize) -> u64 {
    u64::from(size.width) * u64::from(size.height)
}

fn behavior_rank(behavior: FillBehavior, horizontal: bool) -> u8 {
    match (behavior, horizontal) {
        (FillBehavior::HorizontalLoop, true) | (FillBehavior::VerticalLoop, false) => 0,
        (FillBehavior::TrimCap, _) => 1,
        (FillBehavior::Tile, _) => 2,
        (FillBehavior::Stretch, _) => 3,
        (FillBehavior::UniqueDetail, _) => 4,
        _ => 5,
    }
}

fn find_position(
    region: &PreparedRegion<'_>,
    occupied: &[Placed],
    output: PixelSize,
    priority: PackPriority,
) -> Option<PixelBounds> {
    let mut candidates = vec![(region.clearance, region.clearance)];
    for placed in occupied {
        if let Some(value) = placed
            .bounds
            .x
            .checked_add(placed.bounds.width)?
            .checked_add(placed.clearance)?
            .checked_add(region.clearance)
        {
            candidates.push((value, placed.bounds.y));
        }
        if let Some(value) = placed.bounds.x.checked_sub(
            region
                .size
                .width
                .checked_add(placed.clearance)?
                .checked_add(region.clearance)?,
        ) {
            candidates.push((value, placed.bounds.y));
        }
        if let Some(value) = placed
            .bounds
            .y
            .checked_add(placed.bounds.height)?
            .checked_add(placed.clearance)?
            .checked_add(region.clearance)
        {
            candidates.push((region.clearance, value));
        }
        if let Some(value) = placed.bounds.y.checked_sub(
            region
                .size
                .height
                .checked_add(placed.clearance)?
                .checked_add(region.clearance)?,
        ) {
            candidates.push((placed.bounds.x, value));
        }
    }
    candidates.sort_unstable();
    candidates.dedup();
    candidates.sort_by(|left, right| match priority {
        PackPriority::VerticalStrips => left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)),
        PackPriority::Balanced | PackPriority::HorizontalStrips => {
            left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0))
        }
    });
    candidates.into_iter().find_map(|(x, y)| {
        let bounds = PixelBounds {
            x,
            y,
            width: region.size.width,
            height: region.size.height,
        };
        (fits_output(bounds, region.clearance, output)
            && occupied
                .iter()
                .all(|placed| separated(bounds, region.clearance, placed.bounds, placed.clearance)))
        .then_some(bounds)
    })
}

fn fits_output(bounds: PixelBounds, clearance: u32, output: PixelSize) -> bool {
    bounds.width > 0
        && bounds.height > 0
        && bounds.x >= clearance
        && bounds.y >= clearance
        && bounds
            .x
            .checked_add(bounds.width)
            .and_then(|right| right.checked_add(clearance))
            .is_some_and(|right| right <= output.width)
        && bounds
            .y
            .checked_add(bounds.height)
            .and_then(|bottom| bottom.checked_add(clearance))
            .is_some_and(|bottom| bottom <= output.height)
}

fn separated(
    first: PixelBounds,
    first_clearance: u32,
    second: PixelBounds,
    second_clearance: u32,
) -> bool {
    let gap = first_clearance.saturating_add(second_clearance);
    first
        .x
        .checked_add(first.width)
        .and_then(|right| right.checked_add(gap))
        .is_some_and(|right| right <= second.x)
        || second
            .x
            .checked_add(second.width)
            .and_then(|right| right.checked_add(gap))
            .is_some_and(|right| right <= first.x)
        || first
            .y
            .checked_add(first.height)
            .and_then(|bottom| bottom.checked_add(gap))
            .is_some_and(|bottom| bottom <= second.y)
        || second
            .y
            .checked_add(second.height)
            .and_then(|bottom| bottom.checked_add(gap))
            .is_some_and(|bottom| bottom <= first.y)
}

fn validate_caps_fit(
    region: &PreparedRegion<'_>,
    bounds: PixelBounds,
) -> Result<(), LayoutSolveError> {
    if let Some(caps) = region.item.trim_caps {
        let span = match caps.axis {
            TrimAxis::Horizontal => bounds.width,
            TrimAxis::Vertical => bounds.height,
        };
        if caps.leading_px.saturating_add(caps.trailing_px) >= span {
            return Err(LayoutSolveError::TrimCapsDoNotFit {
                region_id: region.id,
            });
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum LayoutSolveError {
    #[error("invalid layout contract: {0}")]
    Contract(LayoutContractError),
    #[error("the layout contains no enabled participating source or patch")]
    NoParticipatingRegions,
    #[error("region {item_key} ({requested:?}) cannot fit output {output:?}: {reason}")]
    ImpossibleFit {
        item_key: String,
        requested: PixelSize,
        output: PixelSize,
        reason: String,
    },
    #[error(
        "position-locked region {region_id} leaves the output bounds including padding and bleed"
    )]
    LockedRegionOutOfBounds { region_id: RegionId },
    #[error("position-locked regions {first} and {second} overlap including padding and bleed")]
    LockedRegionsOverlap { first: RegionId, second: RegionId },
    #[error("auto-pack is disabled and new region {item_key} has no manual position")]
    AutoPackDisabledMissingPlacement { item_key: String },
    #[error("region {region_id} leaves the output bounds including padding and bleed")]
    RegionOutOfBounds { region_id: RegionId },
    #[error("regions {first} and {second} overlap or violate padding/bleed separation")]
    RegionsOverlap { first: RegionId, second: RegionId },
    #[error("trim caps do not fit inside region {region_id}")]
    TrimCapsDoNotFit { region_id: RegionId },
    #[error("padding plus bleed overflows for region {region_id}")]
    InsetOverflow { region_id: RegionId },
    #[error("region ID is repeated: {0}")]
    DuplicateRegionId(RegionId),
    #[error("region item key is repeated: {0}")]
    DuplicateItemKey(String),
    #[error("region order index is repeated: {0}")]
    DuplicateOrder(u32),
}

impl From<LayoutSolveError> for UserFacingError {
    fn from(error: LayoutSolveError) -> Self {
        let (message, recovery) = match &error {
            LayoutSolveError::Contract(contract) => return contract.clone().into(),
            LayoutSolveError::NoParticipatingRegions => (
                "There is nothing enabled for this trim sheet.",
                "Enable a material source or patch and choose Create Trim Sheet again.",
            ),
            LayoutSolveError::ImpossibleFit { .. }
            | LayoutSolveError::LockedRegionOutOfBounds { .. }
            | LayoutSolveError::LockedRegionsOverlap { .. }
            | LayoutSolveError::RegionOutOfBounds { .. }
            | LayoutSolveError::RegionsOverlap { .. } => (
                "The trim regions cannot fit without overlap.",
                "Increase output resolution, reduce region sizes/padding/bleed, or unlock a conflicting region.",
            ),
            LayoutSolveError::AutoPackDisabledMissingPlacement { .. } => (
                "A new region has no manual placement.",
                "Enable auto-pack or place the region before validating the layout.",
            ),
            LayoutSolveError::TrimCapsDoNotFit { .. } => (
                "The trim caps are larger than their region.",
                "Reduce the cap spans or increase the region's locked dimension.",
            ),
            LayoutSolveError::InsetOverflow { .. } => (
                "Padding and bleed cannot be represented safely.",
                "Reduce padding and bleed, then regenerate the layout.",
            ),
            LayoutSolveError::DuplicateRegionId(_)
            | LayoutSolveError::DuplicateItemKey(_)
            | LayoutSolveError::DuplicateOrder(_) => (
                "The persisted layout contains conflicting stable references.",
                "Regenerate the layout from its source sets before export.",
            ),
        };
        Self {
            code: ErrorCode::LayoutInvalid,
            message: message.into(),
            recovery: recovery.into(),
            detail: Some(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use hot_trimmer_domain::{
        AutoPackSettings, FixedRegionSize, LayoutId, LayoutSettings, NormalizedScalar, PatchId,
        RegionConstraints, RegionFill, SourceSetId, TrimCaps,
    };
    use serde::Deserialize;

    use super::*;

    fn item(index: usize, behavior: FillBehavior, size: PixelSize) -> LayoutItem {
        LayoutItem {
            key: format!("patch:{index:04}"),
            fill: RegionFill::RectifiedPatch {
                source_set_id: SourceSetId::from_bytes([1; 16]),
                patch_id: PatchId::from_bytes({
                    let mut bytes = [0_u8; 16];
                    bytes[8..].copy_from_slice(&(index as u64).to_be_bytes());
                    bytes
                }),
            },
            behavior,
            trim_caps: None,
            natural_size: size,
            enabled: true,
            participates: true,
            constraints: RegionConstraints::default(),
            padding_px: None,
            bleed_px: None,
            region_id: None,
        }
    }

    fn request(preset: LayoutPreset, items: Vec<LayoutItem>) -> LayoutRequest {
        LayoutRequest {
            layout_id: LayoutId::from_bytes([9; 16]),
            preset,
            settings: LayoutSettings {
                output: PixelSize {
                    width: 1_024,
                    height: 1_024,
                },
                padding_px: 3,
                bleed_px: 5,
                order: LayoutOrder::Input,
                auto_pack: AutoPackSettings {
                    enabled: true,
                    priority: PackPriority::Balanced,
                    seed: 42,
                },
                fixed_selected_size: None,
            },
            items,
            existing_regions: Vec::new(),
        }
    }

    #[test]
    fn template_slots_use_exact_stable_bounds_without_atlas_clearance() {
        let scalar = |value| NormalizedScalar::new(value).expect("normalized");
        let mut upper = item(
            1,
            FillBehavior::VerticalLoop,
            PixelSize {
                width: 512,
                height: 512,
            },
        );
        upper.key = "template:architectural:v1:upper".into();
        upper.constraints.template_bounds = Some(NormalizedBounds {
            x: scalar(0.0),
            y: scalar(0.0),
            width: scalar(1.0),
            height: scalar(0.5),
        });
        let mut lower = item(
            2,
            FillBehavior::Tile,
            PixelSize {
                width: 512,
                height: 512,
            },
        );
        lower.key = "template:architectural:v1:lower".into();
        lower.constraints.template_bounds = Some(NormalizedBounds {
            x: scalar(0.0),
            y: scalar(0.5),
            width: scalar(1.0),
            height: scalar(0.5),
        });

        let solved = solve_layout(&request(LayoutPreset::Balanced, vec![upper, lower]))
            .expect("template solve");
        assert_eq!(
            solved
                .regions
                .iter()
                .map(|region| region.bounds)
                .collect::<Vec<_>>(),
            vec![
                PixelBounds {
                    x: 0,
                    y: 0,
                    width: 1_024,
                    height: 512,
                },
                PixelBounds {
                    x: 0,
                    y: 512,
                    width: 1_024,
                    height: 512,
                },
            ]
        );
        assert!(
            solved
                .regions
                .iter()
                .all(|region| region.padding_px == 0 && region.bleed_px == 0)
        );
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct LayoutGoldenFixture {
        version: u32,
        coordinate_policy: String,
        cases: Vec<LayoutGoldenCase>,
    }

    #[derive(Deserialize)]
    struct LayoutGoldenCase {
        name: String,
        request: LayoutRequest,
        expect: LayoutGoldenExpectation,
    }

    #[derive(Deserialize)]
    #[serde(
        tag = "result",
        rename_all = "snake_case",
        rename_all_fields = "camelCase"
    )]
    enum LayoutGoldenExpectation {
        Success { regions: Vec<LayoutRegion> },
        Failure { error: LayoutGoldenError },
    }

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(
        tag = "kind",
        rename_all = "snake_case",
        rename_all_fields = "camelCase"
    )]
    enum LayoutGoldenError {
        ImpossibleFit {
            item_key: String,
            requested: PixelSize,
            output: PixelSize,
            reason: String,
        },
    }

    #[test]
    fn phase_3_layout_fixture_executes_exact_solver_goldens() {
        let fixture: LayoutGoldenFixture = serde_json::from_str(include_str!(
            "../../../fixtures/renders/phase-3-layouts.json"
        ))
        .expect("phase 3 layout fixture parses");
        assert_eq!(fixture.version, 2);
        assert_eq!(
            fixture.coordinate_policy,
            "integer_content_bounds_with_external_padding_and_bleed"
        );
        assert_eq!(
            fixture
                .cases
                .iter()
                .map(|case| case.name.as_str())
                .collect::<Vec<_>>(),
            vec![
                "mixed-behavior-order-and-colors",
                "locked-region-and-trim-caps",
                "extreme-aspect-ratios",
                "typed-impossible-padding-fit",
            ]
        );

        for case in fixture.cases {
            match case.expect {
                LayoutGoldenExpectation::Success { regions } => {
                    let solved = solve_layout(&case.request).unwrap_or_else(|error| {
                        panic!("golden case {} failed: {error}", case.name)
                    });
                    assert_eq!(solved.regions, regions, "golden case {} drifted", case.name);
                    validate_layout(&solved).unwrap_or_else(|error| {
                        panic!("golden case {} invalid: {error}", case.name)
                    });
                }
                LayoutGoldenExpectation::Failure { error } => {
                    let actual_error = match solve_layout(&case.request) {
                        Ok(_) => panic!("golden case {} unexpectedly succeeded", case.name),
                        Err(error) => error,
                    };
                    let actual = match actual_error {
                        LayoutSolveError::ImpossibleFit {
                            item_key,
                            requested,
                            output,
                            reason,
                        } => LayoutGoldenError::ImpossibleFit {
                            item_key,
                            requested,
                            output,
                            reason,
                        },
                        other => panic!("golden case {} returned wrong error: {other}", case.name),
                    };
                    assert_eq!(
                        actual, error,
                        "golden case {} diagnostic drifted",
                        case.name
                    );
                }
            }
        }
    }

    #[test]
    fn property_results_are_in_bounds_non_overlapping_and_respect_insets() {
        for count in 1..=64 {
            let items = (0..count)
                .map(|index| {
                    let index_u32 = u32::try_from(index).expect("property count fits u32");
                    let behavior = match index % 5 {
                        0 => FillBehavior::HorizontalLoop,
                        1 => FillBehavior::VerticalLoop,
                        2 => FillBehavior::Tile,
                        3 => FillBehavior::Stretch,
                        _ => FillBehavior::UniqueDetail,
                    };
                    item(
                        index,
                        behavior,
                        PixelSize {
                            width: 40 + (index_u32 * 37) % 900,
                            height: 30 + (index_u32 * 53) % 700,
                        },
                    )
                })
                .collect();
            let layout =
                solve_layout(&request(LayoutPreset::Atlas, items)).expect("property layout");
            assert_eq!(layout.regions.len(), count);
            validate_layout(&layout).expect("valid property layout");
        }
    }

    #[test]
    fn fixed_seed_and_inputs_are_deterministic_with_stable_order() {
        let request = request(
            LayoutPreset::Balanced,
            vec![
                item(
                    2,
                    FillBehavior::Tile,
                    PixelSize {
                        width: 20,
                        height: 900,
                    },
                ),
                item(
                    1,
                    FillBehavior::HorizontalLoop,
                    PixelSize {
                        width: 900,
                        height: 20,
                    },
                ),
                item(
                    3,
                    FillBehavior::UniqueDetail,
                    PixelSize {
                        width: 200,
                        height: 200,
                    },
                ),
            ],
        );
        let first = solve_layout(&request).expect("first");
        let second = solve_layout(&request).expect("second");
        assert_eq!(first, second);
        assert_eq!(
            first
                .regions
                .iter()
                .map(|region| region.order_index)
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn regeneration_preserves_patch_inputs_ids_colors_and_locks() {
        let items = vec![
            item(
                0,
                FillBehavior::HorizontalLoop,
                PixelSize {
                    width: 900,
                    height: 80,
                },
            ),
            item(
                1,
                FillBehavior::Tile,
                PixelSize {
                    width: 300,
                    height: 300,
                },
            ),
        ];
        let original_items = items.clone();
        let mut initial_request = request(LayoutPreset::Balanced, items.clone());
        let mut initial = solve_layout(&initial_request).expect("initial");
        initial.regions[0].locks = RegionLocks {
            position: true,
            width: true,
            height: true,
        };
        initial_request.existing_regions = initial.regions.clone();
        initial_request.items = items;
        let regenerated = solve_layout(&initial_request).expect("regenerated");
        assert_eq!(initial_request.items, original_items);
        assert_eq!(regenerated.regions[0].id, initial.regions[0].id);
        assert_eq!(regenerated.regions[0].id_color, initial.regions[0].id_color);
        assert_eq!(regenerated.regions[0].bounds, initial.regions[0].bounds);
    }

    #[test]
    fn deterministic_color_collisions_resolve_and_persist_without_changing_ids() {
        let first_id = RegionId::from_bytes([0; 16]);
        let mut second_bytes = [0; 16];
        second_bytes[1] = 1;
        let second_id = RegionId::from_bytes(second_bytes);
        assert_eq!(
            IdColor::for_region(first_id),
            IdColor::for_region(second_id),
            "fixture IDs must exercise the collision path"
        );

        let mut first = item(
            0,
            FillBehavior::Tile,
            PixelSize {
                width: 100,
                height: 100,
            },
        );
        first.region_id = Some(first_id);
        let mut second = item(
            1,
            FillBehavior::Tile,
            PixelSize {
                width: 100,
                height: 100,
            },
        );
        second.region_id = Some(second_id);
        let mut request = request(LayoutPreset::Atlas, vec![first, second]);
        let solved = solve_layout(&request).expect("collision-resolved layout");
        assert_eq!(solved.regions[0].id, first_id);
        assert_eq!(solved.regions[1].id, second_id);
        assert_ne!(solved.regions[0].id_color, solved.regions[1].id_color);
        assert!(
            solved
                .regions
                .iter()
                .all(|region| region.id_color.is_valid())
        );

        let colors: Vec<_> = solved
            .regions
            .iter()
            .map(|region| region.id_color)
            .collect();
        request.existing_regions = solved.regions;
        let regenerated = solve_layout(&request).expect("preserved resolved colors");
        assert_eq!(
            regenerated
                .regions
                .iter()
                .map(|region| region.id_color)
                .collect::<Vec<_>>(),
            colors
        );
    }

    #[test]
    fn selected_exact_size_and_locked_dimension_are_authoritative() {
        let mut request = request(
            LayoutPreset::Atlas,
            vec![item(
                0,
                FillBehavior::Stretch,
                PixelSize {
                    width: 500,
                    height: 200,
                },
            )],
        );
        let initial = solve_layout(&request).expect("initial");
        let region_id = initial.regions[0].id;
        let mut existing = initial.regions[0].clone();
        existing.bounds.width = 311;
        existing.locks.width = true;
        request.existing_regions = vec![existing];
        request.settings.fixed_selected_size = Some(FixedRegionSize {
            region_id,
            size: PixelSize {
                width: 257,
                height: 129,
            },
        });
        let solved = solve_layout(&request).expect("fixed selection");
        assert_eq!(
            solved.regions[0].bounds.size(),
            PixelSize {
                width: 257,
                height: 129
            }
        );
    }

    #[test]
    fn golden_mixed_modes_caps_extreme_aspects_and_presets() {
        for preset in [
            LayoutPreset::Balanced,
            LayoutPreset::HorizontalTrims,
            LayoutPreset::VerticalTrims,
            LayoutPreset::ModularKit,
            LayoutPreset::Atlas,
        ] {
            let mut cap = item(
                0,
                FillBehavior::TrimCap,
                PixelSize {
                    width: 4_000,
                    height: 100,
                },
            );
            cap.trim_caps = Some(TrimCaps {
                axis: TrimAxis::Horizontal,
                leading_px: 12,
                trailing_px: 20,
            });
            let source = LayoutItem {
                key: "source-only:no-patches".into(),
                fill: RegionFill::WholeSourceSet {
                    source_set_id: SourceSetId::from_bytes([8; 16]),
                },
                behavior: FillBehavior::Stretch,
                trim_caps: None,
                natural_size: PixelSize {
                    width: 4_096,
                    height: 4_096,
                },
                enabled: true,
                participates: true,
                constraints: RegionConstraints::default(),
                padding_px: Some(1),
                bleed_px: Some(2),
                region_id: None,
            };
            let solved = solve_layout(&request(
                preset,
                vec![
                    cap,
                    item(
                        1,
                        FillBehavior::VerticalLoop,
                        PixelSize {
                            width: 1,
                            height: 16_000,
                        },
                    ),
                    item(
                        2,
                        FillBehavior::Tile,
                        PixelSize {
                            width: 16_000,
                            height: 1,
                        },
                    ),
                    source,
                ],
            ))
            .expect("golden preset");
            assert_eq!(solved.regions.len(), 4);
            validate_layout(&solved).expect("golden valid");
        }
    }

    #[test]
    fn golden_impossible_fit_explains_recovery() {
        let mut huge = item(
            0,
            FillBehavior::UniqueDetail,
            PixelSize {
                width: 1,
                height: 1,
            },
        );
        huge.constraints.fixed_width_px = Some(2_000);
        let error =
            solve_layout(&request(LayoutPreset::Balanced, vec![huge])).expect_err("cannot fit");
        assert!(matches!(error, LayoutSolveError::ImpossibleFit { .. }));
        let user: UserFacingError = error.into();
        assert!(user.recovery.contains("Increase output resolution"));
    }

    #[test]
    fn debug_budget_large_representative_512_region_atlas_under_one_second() {
        let items = (0..512)
            .map(|index| {
                let index_u32 = u32::try_from(index).expect("representative count fits u32");
                item(
                    index,
                    FillBehavior::Tile,
                    PixelSize {
                        width: 32 + (index_u32 % 19) * 17,
                        height: 32 + (index_u32 % 23) * 13,
                    },
                )
            })
            .collect();
        let mut request = request(LayoutPreset::Atlas, items);
        request.settings.output = PixelSize {
            width: 4_096,
            height: 4_096,
        };
        let started = Instant::now();
        let solved = solve_layout(&request).expect("large representative atlas");
        let elapsed = started.elapsed();
        assert_eq!(solved.regions.len(), 512);
        assert!(
            elapsed < Duration::from_secs(1),
            "debug solve budget exceeded: {elapsed:?}"
        );
    }
}
