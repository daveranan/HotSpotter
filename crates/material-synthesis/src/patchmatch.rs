//! Stage 8C bounded deterministic registered PatchMatch synthesis.

use std::{collections::BTreeMap, sync::Arc};

use hot_trimmer_domain::{
    AlgorithmProvenance, ContentDigest, MaterialBehaviorClass, MaterialChannelRole, StageResult,
};
use hot_trimmer_image_io::{ImagePlane, LinearScalar, MaskValue, TangentNormal};
use hot_trimmer_render_core::{PreparedExemplarChannel, RenderCancellationToken};

use super::*;

pub const STAGE_08C_PATCHMATCH_ALGORITHM_ID: &str = "hot_trimmer.registered_patchmatch";
pub const STAGE_08C_ALGORITHM_VERSION: &str = "8.3.0";

/// A value of one in `completion_mask` requests synthesis. Zero preserves the
/// registered source pixel. A value of one in `source_exclusion_mask` forbids
/// that source pixel from every synthesized patch.
#[derive(Clone, Debug, PartialEq)]
pub struct PatchMatchSettings {
    pub output_width: u32,
    pub output_height: u32,
    pub completion_mask: Option<Arc<ImagePlane<MaskValue>>>,
    pub source_exclusion_mask: Option<Arc<ImagePlane<MaskValue>>>,
    pub seamless_x: bool,
    pub seamless_y: bool,
    pub patch_radius: u16,
    pub pyramid_levels: u8,
    pub iterations_per_level: u16,
    pub random_search_radius: u16,
    pub random_candidates_per_radius: u8,
    pub minimum_usable_confidence_milli: u16,
    pub convergence_change_threshold_milli: u16,
    /// Publication gate for each requested seamless output boundary.
    pub max_boundary_error_milli: u16,
    pub weights: PatchMatchWeights,
    pub semantics: PatchMatchSemanticConstraint,
    pub max_output_dimension: u32,
    pub max_output_pixels: u64,
    pub max_patch_radius: u16,
    pub max_search_radius: u16,
    pub max_iterations: u16,
    pub max_working_bytes: u64,
    pub max_operations: u64,
}

impl Default for PatchMatchSettings {
    fn default() -> Self {
        Self {
            output_width: 1024,
            output_height: 1024,
            completion_mask: None,
            source_exclusion_mask: None,
            seamless_x: true,
            seamless_y: true,
            patch_radius: 3,
            pyramid_levels: 3,
            iterations_per_level: 6,
            random_search_radius: 128,
            random_candidates_per_radius: 2,
            minimum_usable_confidence_milli: 650,
            convergence_change_threshold_milli: 20,
            max_boundary_error_milli: 80,
            weights: PatchMatchWeights::default(),
            semantics: PatchMatchSemanticConstraint::StochasticIsotropic,
            max_output_dimension: 16_384,
            max_output_pixels: 67_108_864,
            max_patch_radius: 32,
            max_search_radius: 4096,
            max_iterations: 64,
            max_working_bytes: 2_147_483_648,
            max_operations: 4_000_000_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PatchMatchWeights {
    pub color: u16,
    pub gradient: u16,
    pub height: u16,
    pub vector_normal: u16,
    pub roughness: u16,
    pub structure: u16,
    pub coherence: u16,
    pub duplicate_use: u16,
    pub saliency_repeat: u16,
}

impl Default for PatchMatchWeights {
    fn default() -> Self {
        Self {
            color: 220,
            gradient: 100,
            height: 70,
            vector_normal: 90,
            roughness: 60,
            structure: 130,
            coherence: 220,
            duplicate_use: 70,
            saliency_repeat: 40,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PatchMatchSemanticConstraint {
    StochasticIsotropic,
    Directional {
        behavior: MaterialBehaviorClass,
        requested_angle_millidegrees: i32,
        tolerance_millidegrees: u32,
    },
    /// Explicit permission for completion around protected unique content. This
    /// is legal only with a completion mask; preserved pixels are never moved.
    ProtectedUniqueCompletion,
    /// Explicit period compatibility for manufactured content.
    ManufacturedPeriod {
        period_x: u16,
        period_y: u16,
    },
    UniqueDetail,
    ManufacturedPattern,
    MixedUnknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatchMatchSourceUsage {
    pub source_x: u32,
    pub source_y: u32,
    pub use_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatchMatchPassDiagnostics {
    pub pyramid_level: u8,
    pub iteration: u16,
    pub direction_forward: bool,
    pub changed_pixels: u64,
    pub accepted_propagation: u64,
    pub accepted_random_search: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatchMatchDiagnostics {
    pub algorithm_version: String,
    pub seed: u64,
    pub patch_radius: u16,
    pub pyramid_levels: u8,
    pub passes: Vec<PatchMatchPassDiagnostics>,
    pub source_usage: Vec<PatchMatchSourceUsage>,
    pub synthesized_pixels: u64,
    pub preserved_pixels: u64,
    pub rejected_unusable_candidates: u64,
    pub rejected_excluded_candidates: u64,
    pub operation_count: u64,
    pub converged: bool,
    pub convergence_iteration: u16,
    pub final_changed_pixels: u64,
    pub mean_confidence_milli: u16,
    pub minimum_confidence_milli: u16,
    pub incomplete_pixels: u64,
    pub boundary_error_milli: (u16, u16),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Match {
    coordinate: SourceCoordinate,
    cost: u64,
}

#[derive(Clone, Copy, Default)]
struct Descriptor {
    rgb: [f64; 3],
    gradient: f64,
    height: f64,
    roughness: f64,
    normal: [f64; 3],
    structure: f64,
    saliency: f64,
    usability: f64,
    excluded: bool,
}

struct DescriptorLevel {
    width: u32,
    height: u32,
    pixels: Vec<Descriptor>,
}
struct RegisteredDescriptorPyramid {
    levels: Vec<DescriptorLevel>,
}

#[derive(Default)]
struct Counters {
    operations: u64,
    rejected_unusable: u64,
    rejected_excluded: u64,
}

pub(super) fn patchmatch_cache_fragment(settings: &PatchMatchSettings) -> String {
    format!(
        "{}x{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{:?}|{:?}|{}|{}",
        settings.output_width,
        settings.output_height,
        settings.seamless_x,
        settings.seamless_y,
        settings.patch_radius,
        settings.pyramid_levels,
        settings.iterations_per_level,
        settings.random_search_radius,
        settings.random_candidates_per_radius,
        settings.minimum_usable_confidence_milli,
        settings.convergence_change_threshold_milli,
        settings.max_boundary_error_milli,
        settings.weights,
        settings.semantics,
        mask_digest(settings.completion_mask.as_deref()).0,
        mask_digest(settings.source_exclusion_mask.as_deref()).0
    )
}

fn mask_digest(mask: Option<&ImagePlane<MaskValue>>) -> ContentDigest {
    let Some(mask) = mask else {
        return ContentDigest::sha256(b"none");
    };
    let mut bytes = Vec::with_capacity(
        (mask.width() as usize)
            .saturating_mul(mask.height() as usize)
            .saturating_mul(4)
            + 16,
    );
    bytes.extend_from_slice(&mask.width().to_le_bytes());
    bytes.extend_from_slice(&mask.height().to_le_bytes());
    for y in 0..mask.height() {
        for x in 0..mask.width() {
            bytes.extend_from_slice(&mask.pixel(x, y).0.to_bits().to_le_bytes());
        }
    }
    ContentDigest::sha256(&bytes)
}

pub(super) fn patchmatch_domain(
    r: &DomainRequest,
    key: ContentDigest,
    cancel: &RenderCancellationToken,
) -> Result<PreparedMaterialDomain, DomainError> {
    validate(r)?;
    preflight(r)?;
    check_cancel(cancel)?;
    let s = &r.patch_match;
    let (ow, oh) = (s.output_width, s.output_height);
    let count = pixel_count(ow, oh)?;
    let mut counters = Counters::default();
    let descriptors = build_descriptor_pyramid(r, cancel)?;
    let synthesized_pixels = synthesized_count(s);
    let preserved_pixels = count as u64 - synthesized_pixels;
    if synthesized_pixels == 0 {
        return Err(DomainError::IncompletePatchMatch {
            incomplete_pixels: 0,
        });
    }
    let mut passes = Vec::new();
    let mut converged = false;
    let mut convergence_iteration = 0;
    let mut final_changed = synthesized_pixels;
    let mut total_iteration = 0_u16;
    let mut field = Vec::new();
    let mut previous_dimensions = (0, 0);
    for level in (0..descriptors.levels.len()).rev() {
        let scale = 1_u32 << level.min(15);
        let level_width = ow.div_ceil(scale);
        let level_height = oh.div_ceil(scale);
        let descriptor = &descriptors.levels[level];
        field = if field.is_empty() {
            initialize_level(
                r,
                descriptor,
                level_width,
                level_height,
                level as u8,
                &mut counters,
                cancel,
            )?
        } else {
            upscale_level(
                r,
                &field,
                previous_dimensions,
                descriptor,
                level_width,
                level_height,
                level as u8,
                &mut counters,
                cancel,
            )?
        };
        previous_dimensions = (level_width, level_height);
        let level_synthesized = synthesized_count_level(s, level_width, level_height, scale);
        let threshold = level_synthesized
            .saturating_mul(u64::from(s.convergence_change_threshold_milli))
            .div_ceil(1000);
        let radius = u32::from(s.patch_radius) / scale;
        for iteration in 0..s.iterations_per_level {
            check_cancel(cancel)?;
            total_iteration = total_iteration.saturating_add(1);
            let forward = iteration % 2 == 0;
            let (changed, propagation, random) = optimize_pass_level(
                r,
                descriptor,
                &mut field,
                level_width,
                level_height,
                scale,
                radius,
                level as u8,
                iteration,
                forward,
                &mut counters,
                cancel,
            )?;
            passes.push(PatchMatchPassDiagnostics {
                pyramid_level: level as u8,
                iteration,
                direction_forward: forward,
                changed_pixels: changed,
                accepted_propagation: propagation,
                accepted_random_search: random,
            });
            final_changed = changed;
            if changed <= threshold {
                if level == 0 {
                    converged = true;
                    convergence_iteration = total_iteration;
                }
                break;
            }
        }
    }
    if !converged {
        return Err(DomainError::PatchMatchNonConverged {
            iterations: total_iteration,
            changed_pixels: final_changed,
        });
    }

    let incomplete = field
        .iter()
        .zip(target_flags(s))
        .filter(|(entry, synth)| *synth && entry.cost == u64::MAX)
        .count() as u64;
    if incomplete != 0 {
        return Err(DomainError::IncompletePatchMatch {
            incomplete_pixels: incomplete,
        });
    }
    enforce_periodic_correspondence(s, &mut field, &mut counters)?;
    let samples: Vec<_> = field
        .iter()
        .map(|entry| CorrespondenceSample {
            sources: [
                Some(WeightedSource {
                    coordinate: entry.coordinate,
                    weight: 1.0,
                }),
                None,
                None,
                None,
            ],
        })
        .collect();
    let channels = compose_channels(r, &samples, ow, oh, cancel)?;
    let validity = source_validity(r, &samples, ow, oh, cancel)?;
    let minimum = f32::from(s.minimum_usable_confidence_milli) / 1000.0;
    if validity
        .tiles()
        .iter()
        .flat_map(|tile| &tile.pixels)
        .zip(target_flags(s))
        .any(|(value, synth)| synth && value.0 + 1.0e-6 < minimum)
    {
        return Err(DomainError::UnusablePatchMatchSource {
            rejected_candidates: counters.rejected_unusable,
        });
    }
    let mut usage = BTreeMap::<(u32, u32), u32>::new();
    for (entry, synth) in field.iter().zip(target_flags(s)) {
        if synth {
            *usage
                .entry((entry.coordinate.x, entry.coordinate.y))
                .or_default() += 1;
        }
    }
    let source_usage = usage
        .into_iter()
        .map(|((source_x, source_y), use_count)| PatchMatchSourceUsage {
            source_x,
            source_y,
            use_count,
        })
        .collect();
    let confidences: Vec<u16> = field
        .iter()
        .zip(target_flags(s))
        .filter_map(|(entry, synth)| synth.then_some(cost_confidence(entry.cost)))
        .collect();
    let mean_confidence = if confidences.is_empty() {
        1000
    } else {
        (confidences.iter().map(|v| u64::from(*v)).sum::<u64>() / confidences.len() as u64) as u16
    };
    let minimum_confidence = confidences.iter().copied().min().unwrap_or(1000);
    let boundary = measured_patchmatch_boundary(&channels, ow, oh);
    if (s.seamless_x && boundary.0 > s.max_boundary_error_milli)
        || (s.seamless_y && boundary.1 > s.max_boundary_error_milli)
    {
        return Err(DomainError::UnacceptablePatchMatchBoundary {
            horizontal_milli: boundary.0,
            vertical_milli: boundary.1,
            maximum_milli: s.max_boundary_error_milli,
        });
    }
    let diagnostics_pm = PatchMatchDiagnostics {
        algorithm_version: STAGE_08C_ALGORITHM_VERSION.into(),
        seed: r.seed,
        patch_radius: s.patch_radius,
        pyramid_levels: s.pyramid_levels,
        passes,
        source_usage,
        synthesized_pixels,
        preserved_pixels,
        rejected_unusable_candidates: counters.rejected_unusable,
        rejected_excluded_candidates: counters.rejected_excluded,
        operation_count: counters.operations,
        converged,
        convergence_iteration,
        final_changed_pixels: final_changed,
        mean_confidence_milli: mean_confidence,
        minimum_confidence_milli: minimum_confidence,
        incomplete_pixels: incomplete,
        boundary_error_milli: boundary,
    };
    let edge = r.source.base_color().tile_edge();
    let correspondence = CorrespondenceField::Registered(plane(ow, oh, edge, samples)?);
    let operations = OperationField::Registered(plane(
        ow,
        oh,
        edge,
        target_flags(s)
            .map(|synth| DomainOperation::PatchMatch { preserved: !synth })
            .collect(),
    )?);
    let provenance = plane(
        ow,
        oh,
        edge,
        target_flags(s)
            .map(|synth| {
                if synth {
                    ProvenanceValue::SeamComposed
                } else {
                    ProvenanceValue::Original
                }
            })
            .collect(),
    )?;
    let domain_diagnostics = DomainDiagnostics {
        selected_route: DomainRoute::PatchMatch,
        cache_key: key.clone(),
        available_seam_terms: r.analysis.seamability.available_terms.clone(),
        normalized_weight_milli: normalized_weights(r),
        pass_through: None,
        seams: Vec::new(),
        boundary_cost_before_milli: (
            r.analysis.seamability.horizontal_cost_milli,
            r.analysis.seamability.vertical_cost_milli,
        ),
        boundary_cost_after_milli: boundary,
        messages: vec![format!(
            "converged registered NNF in {} passes; {} synthesized and {} preserved pixels",
            diagnostics_pm.passes.len(),
            synthesized_pixels,
            preserved_pixels
        )],
    };
    Ok(PreparedMaterialDomain {
        cache_key: key,
        prepared_source_digest: r.prepared_source_digest.clone(),
        analysis_digest: r.analysis.cache_key.clone(),
        route: DomainRoute::PatchMatch,
        width: ow,
        height: oh,
        channels: DomainChannelStorage::Generated(channels),
        correspondence,
        operations,
        validity,
        provenance,
        seams: Vec::new(),
        quilting: None,
        patch_match: Some(diagnostics_pm),
        diagnostics: domain_diagnostics,
        qa_views: vec![
            DomainQaView::RegisteredChannels,
            DomainQaView::NearestNeighborField,
            DomainQaView::Coherence,
            DomainQaView::SourceUsage,
            DomainQaView::Correspondence,
            DomainQaView::Operations,
            DomainQaView::Validity,
            DomainQaView::Provenance,
        ],
        stage_result: StageResult::Executed {
            algorithm: AlgorithmProvenance {
                algorithm_id: STAGE_08C_PATCHMATCH_ALGORITHM_ID.into(),
                version: STAGE_08C_ALGORITHM_VERSION.into(),
            },
            settings_hash: ContentDigest::sha256(
                format!("{}|{}", patchmatch_cache_fragment(s), r.seed).as_bytes(),
            ),
            diagnostics: Vec::new(),
        },
    })
}

fn validate(r: &DomainRequest) -> Result<(), DomainError> {
    let s = &r.patch_match;
    let b = r.source.base_color();
    let weights = [
        s.weights.color,
        s.weights.gradient,
        s.weights.height,
        s.weights.vector_normal,
        s.weights.roughness,
        s.weights.structure,
        s.weights.coherence,
        s.weights.duplicate_use,
        s.weights.saliency_repeat,
    ]
    .into_iter()
    .map(u64::from)
    .sum::<u64>();
    if s.output_width == 0
        || s.output_height == 0
        || s.output_width > s.max_output_dimension
        || s.output_height > s.max_output_dimension
        || s.patch_radius == 0
        || s.patch_radius > s.max_patch_radius
        || u32::from(s.patch_radius) * 2 + 1 > b.width().min(b.height())
        || s.pyramid_levels == 0
        || s.pyramid_levels > 8
        || s.iterations_per_level == 0
        || s.iterations_per_level > s.max_iterations
        || s.random_search_radius == 0
        || s.random_search_radius > s.max_search_radius
        || s.random_candidates_per_radius == 0
        || s.random_candidates_per_radius > 16
        || s.minimum_usable_confidence_milli > 1000
        || s.convergence_change_threshold_milli > 1000
        || s.max_boundary_error_milli > 1000
        || weights == 0
        || s.max_working_bytes == 0
        || s.max_operations == 0
    {
        return Err(DomainError::InvalidSettings);
    }
    if let Some(mask) = &s.completion_mask {
        if (mask.width(), mask.height()) != (s.output_width, s.output_height)
            || (s.output_width, s.output_height) != (b.width(), b.height())
        {
            return Err(DomainError::RegistrationDrift);
        }
    }
    if s.source_exclusion_mask
        .as_ref()
        .is_some_and(|mask| (mask.width(), mask.height()) != (b.width(), b.height()))
    {
        return Err(DomainError::RegistrationDrift);
    }
    match s.semantics {
        PatchMatchSemanticConstraint::UniqueDetail => {
            return incompatible("unique detail requires an explicit protected completion mask");
        }
        PatchMatchSemanticConstraint::ManufacturedPattern => {
            return incompatible("manufactured content requires an explicit compatible period");
        }
        PatchMatchSemanticConstraint::MixedUnknown => {
            return incompatible("mixed/unknown semantics do not authorize PatchMatch");
        }
        PatchMatchSemanticConstraint::ProtectedUniqueCompletion if s.completion_mask.is_none() => {
            return incompatible(
                "protected unique content is legal only for constrained completion",
            );
        }
        PatchMatchSemanticConstraint::ManufacturedPeriod { period_x, period_y }
            if period_x == 0
                || period_y == 0
                || s.output_width % u32::from(period_x) != 0
                || s.output_height % u32::from(period_y) != 0 =>
        {
            return incompatible("manufactured period is not compatible with the output extent");
        }
        PatchMatchSemanticConstraint::Directional {
            behavior,
            requested_angle_millidegrees,
            tolerance_millidegrees,
        } => {
            if !matches!(
                behavior,
                MaterialBehaviorClass::StochasticDirectional
                    | MaterialBehaviorClass::OrganicDirectional
            ) {
                return incompatible("invalid directional behavior class");
            }
            let Some(axis) = r.scale_orientation.global_orientation.axis_millidegrees else {
                return incompatible(
                    "directional PatchMatch requires Stage 6 orientation authority",
                );
            };
            if r.scale_orientation.global_orientation.confidence_milli == 0
                || angular_delta(axis as i32, requested_angle_millidegrees) > tolerance_millidegrees
            {
                return incompatible(
                    "requested direction exceeds the Stage 6 orientation constraint",
                );
            }
        }
        _ => {}
    }
    Ok(())
}

fn incompatible<T>(reason: &str) -> Result<T, DomainError> {
    Err(DomainError::IncompatiblePatchMatch {
        reason: reason.into(),
    })
}
fn angular_delta(a: i32, b: i32) -> u32 {
    let d = (i64::from(a) - i64::from(b)).unsigned_abs() % 180_000;
    d.min(180_000 - d) as u32
}

fn preflight(r: &DomainRequest) -> Result<(), DomainError> {
    let s = &r.patch_match;
    let pixels = u64::from(s.output_width)
        .checked_mul(u64::from(s.output_height))
        .ok_or(DomainError::ResourceLimitExceeded)?;
    if pixels > s.max_output_pixels {
        return Err(DomainError::ResourceLimitExceeded);
    }
    let bytes = patchmatch_required_working_bytes(r)?;
    let radii = u64::from(s.random_search_radius).ilog2() as u64 + 1;
    let candidates = 3 + radii * u64::from(s.random_candidates_per_radius);
    let patch_samples = u64::from(s.patch_radius) * 2 + 1;
    let operations = pixels
        .checked_mul(u64::from(s.pyramid_levels))
        .and_then(|v| v.checked_mul(u64::from(s.iterations_per_level)))
        .and_then(|v| v.checked_mul(candidates))
        .and_then(|v| v.checked_mul(patch_samples.min(9)))
        .ok_or(DomainError::ResourceLimitExceeded)?;
    if bytes > s.max_working_bytes || operations > s.max_operations {
        Err(DomainError::ResourceLimitExceeded)
    } else {
        Ok(())
    }
}

pub(super) fn patchmatch_required_working_bytes(r: &DomainRequest) -> Result<u64, DomainError> {
    let s = &r.patch_match;
    let output_pixels = u64::from(s.output_width)
        .checked_mul(u64::from(s.output_height))
        .ok_or(DomainError::ResourceLimitExceeded)?;
    // Covers the full-resolution NNF plus pass snapshot/source-usage nodes,
    // correspondence/operation/validity/provenance planes, and generated channels.
    let output_bytes = output_pixels
        .checked_mul(96 + r.source.channels.len() as u64 * 24)
        .ok_or(DomainError::ResourceLimitExceeded)?;
    let descriptor_bytes = descriptor_pyramid_allocation_bytes(
        r.source.base_color().width(),
        r.source.base_color().height(),
        s.pyramid_levels,
    )?;
    output_bytes
        .checked_add(descriptor_bytes)
        .ok_or(DomainError::ResourceLimitExceeded)
}

fn descriptor_pyramid_allocation_bytes(
    mut width: u32,
    mut height: u32,
    requested_levels: u8,
) -> Result<u64, DomainError> {
    let mut descriptor_count = 0_u64;
    let mut level_count = 0_u64;
    while level_count < u64::from(requested_levels) {
        descriptor_count = descriptor_count
            .checked_add(
                u64::from(width)
                    .checked_mul(u64::from(height))
                    .ok_or(DomainError::ResourceLimitExceeded)?,
            )
            .ok_or(DomainError::ResourceLimitExceeded)?;
        level_count += 1;
        if width == 1 && height == 1 {
            break;
        }
        width = width.div_ceil(2);
        height = height.div_ceil(2);
    }
    let descriptors = descriptor_count
        .checked_mul(std::mem::size_of::<Descriptor>() as u64)
        .ok_or(DomainError::ResourceLimitExceeded)?;
    // `levels` grows geometrically from capacity one; account for its retained
    // allocation rather than only its initialized entries.
    let level_capacity = level_count
        .checked_next_power_of_two()
        .ok_or(DomainError::ResourceLimitExceeded)?;
    let level_storage = level_capacity
        .checked_mul(std::mem::size_of::<DescriptorLevel>() as u64)
        .and_then(|v| v.checked_add(std::mem::size_of::<RegisteredDescriptorPyramid>() as u64))
        .ok_or(DomainError::ResourceLimitExceeded)?;
    descriptors
        .checked_add(level_storage)
        .ok_or(DomainError::ResourceLimitExceeded)
}

fn target_flags(s: &PatchMatchSettings) -> impl Iterator<Item = bool> + '_ {
    (0..s.output_height).flat_map(move |y| {
        (0..s.output_width).map(move |x| {
            s.completion_mask
                .as_ref()
                .is_none_or(|mask| mask.pixel(x, y).0 >= 0.5)
        })
    })
}
fn synthesized_count(s: &PatchMatchSettings) -> u64 {
    target_flags(s).map(u64::from).sum()
}

fn build_descriptor_pyramid(
    r: &DomainRequest,
    cancel: &RenderCancellationToken,
) -> Result<RegisteredDescriptorPyramid, DomainError> {
    let s = &r.patch_match;
    let base = r.source.base_color();
    let usability = r
        .analysis
        .usability
        .confidence
        .level(0)
        .ok_or(DomainError::RegistrationDrift)?;
    let mut pixels = Vec::with_capacity(pixel_count(base.width(), base.height())?);
    for y in 0..base.height() {
        if y % 16 == 0 {
            check_cancel(cancel)?;
        }
        for x in 0..base.width() {
            let c = base.pixel(x, y);
            let coordinate = SourceCoordinate { x, y };
            let normal =
                normal_channel(r).map_or([0.0, 0.0, 1.0], |p| p.pixel(x, y).xyz.map(f64::from));
            pixels.push(Descriptor {
                rgb: c.rgb.map(f64::from),
                gradient: f64::from(luminance_gradient(base, x, y)),
                height: scalar_channel(r, MaterialChannelRole::Height)
                    .map_or(0.0, |p| f64::from(p.pixel(x, y).0)),
                roughness: scalar_channel(r, MaterialChannelRole::Roughness)
                    .map_or(0.0, |p| f64::from(p.pixel(x, y).0)),
                normal,
                structure: structure_value(r, coordinate),
                saliency: f64::from(
                    r.analysis
                        .saliency
                        .level(0)
                        .ok_or(DomainError::RegistrationDrift)?
                        .pixel(x, y)
                        .0,
                ),
                usability: f64::from(
                    usability.pixel(x, y).0
                        * r.source.coverage.as_ref().map_or(1.0, |p| p.pixel(x, y).0),
                ),
                excluded: s
                    .source_exclusion_mask
                    .as_ref()
                    .is_some_and(|p| p.pixel(x, y).0 >= 0.5),
            });
        }
    }
    let mut levels = vec![DescriptorLevel {
        width: base.width(),
        height: base.height(),
        pixels,
    }];
    while levels.len() < usize::from(s.pyramid_levels) {
        let prior = levels.last().expect("level zero");
        if prior.width == 1 && prior.height == 1 {
            break;
        }
        let (width, height) = (prior.width.div_ceil(2), prior.height.div_ceil(2));
        let mut next = Vec::with_capacity(pixel_count(width, height)?);
        for y in 0..height {
            if y % 16 == 0 {
                check_cancel(cancel)?;
            }
            for x in 0..width {
                let mut d = Descriptor {
                    normal: [0.0; 3],
                    usability: 1.0,
                    ..Descriptor::default()
                };
                let mut n = 0.0;
                for py in y * 2..(y * 2 + 2).min(prior.height) {
                    for px in x * 2..(x * 2 + 2).min(prior.width) {
                        let q = prior.pixels[(py * prior.width + px) as usize];
                        n += 1.0;
                        for i in 0..3 {
                            d.rgb[i] += q.rgb[i];
                            d.normal[i] += q.normal[i];
                        }
                        d.gradient += q.gradient;
                        d.height += q.height;
                        d.roughness += q.roughness;
                        d.structure += q.structure;
                        d.saliency += q.saliency;
                        d.usability = d.usability.min(q.usability);
                        d.excluded |= q.excluded;
                    }
                }
                for i in 0..3 {
                    d.rgb[i] /= n;
                    d.normal[i] /= n;
                }
                d.normal = normalize64(d.normal);
                d.gradient /= n;
                d.height /= n;
                d.roughness /= n;
                d.structure /= n;
                d.saliency /= n;
                next.push(d);
            }
        }
        levels.push(DescriptorLevel {
            width,
            height,
            pixels: next,
        });
    }
    Ok(RegisteredDescriptorPyramid { levels })
}

fn synthesized_count_level(s: &PatchMatchSettings, width: u32, height: u32, scale: u32) -> u64 {
    (0..height)
        .flat_map(|y| (0..width).map(move |x| synth_at_level(s, x, y, scale)))
        .map(u64::from)
        .sum()
}

fn synth_at_level(s: &PatchMatchSettings, x: u32, y: u32, scale: u32) -> bool {
    let Some(mask) = &s.completion_mask else {
        return true;
    };
    for py in y * scale..(y * scale + scale).min(s.output_height) {
        for px in x * scale..(x * scale + scale).min(s.output_width) {
            if mask.pixel(px, py).0 >= 0.5 {
                return true;
            }
        }
    }
    false
}

fn initialize_level(
    r: &DomainRequest,
    level: &DescriptorLevel,
    ow: u32,
    oh: u32,
    level_index: u8,
    counters: &mut Counters,
    cancel: &RenderCancellationToken,
) -> Result<Vec<Match>, DomainError> {
    let s = &r.patch_match;
    let scale = 1_u32 << level_index.min(15);
    let mut out = Vec::with_capacity(pixel_count(ow, oh)?);
    for y in 0..oh {
        if y % 16 == 0 {
            check_cancel(cancel)?;
        }
        for x in 0..ow {
            if !synth_at_level(s, x, y, scale) {
                out.push(Match {
                    coordinate: SourceCoordinate {
                        x: x.min(level.width - 1),
                        y: y.min(level.height - 1),
                    },
                    cost: 0,
                });
                continue;
            }
            let start = splitmix(
                r.seed
                    ^ version_salt()
                    ^ u64::from(level_index).rotate_left(9)
                    ^ u64::from(x).rotate_left(17)
                    ^ u64::from(y).rotate_left(39),
            ) % (u64::from(level.width) * u64::from(level.height));
            let mut selected = None;
            for offset in 0..u64::from(level.width) * u64::from(level.height) {
                count_op(counters, s, 1)?;
                let index = (start + offset) % (u64::from(level.width) * u64::from(level.height));
                let candidate = SourceCoordinate {
                    x: (index % u64::from(level.width)) as u32,
                    y: (index / u64::from(level.width)) as u32,
                };
                if valid_level_candidate(
                    s,
                    level,
                    candidate,
                    u32::from(s.patch_radius) / scale,
                    counters,
                ) {
                    selected = Some(candidate);
                    break;
                }
            }
            let Some(coordinate) = selected else {
                return Err(DomainError::UnusablePatchMatchSource {
                    rejected_candidates: counters.rejected_unusable,
                });
            };
            out.push(Match {
                coordinate,
                cost: u64::MAX - 1,
            });
        }
    }
    Ok(out)
}

fn upscale_level(
    r: &DomainRequest,
    prior: &[Match],
    prior_dimensions: (u32, u32),
    level: &DescriptorLevel,
    ow: u32,
    oh: u32,
    level_index: u8,
    counters: &mut Counters,
    cancel: &RenderCancellationToken,
) -> Result<Vec<Match>, DomainError> {
    let s = &r.patch_match;
    let scale = 1_u32 << level_index.min(15);
    let mut out = Vec::with_capacity(pixel_count(ow, oh)?);
    for y in 0..oh {
        if y % 16 == 0 {
            check_cancel(cancel)?;
        }
        for x in 0..ow {
            if !synth_at_level(s, x, y, scale) {
                out.push(Match {
                    coordinate: SourceCoordinate {
                        x: x.min(level.width - 1),
                        y: y.min(level.height - 1),
                    },
                    cost: 0,
                });
                continue;
            }
            let parent = prior[((y / 2).min(prior_dimensions.1 - 1) * prior_dimensions.0
                + (x / 2).min(prior_dimensions.0 - 1)) as usize]
                .coordinate;
            let preferred = SourceCoordinate {
                x: (parent.x * 2 + x % 2).min(level.width - 1),
                y: (parent.y * 2 + y % 2).min(level.height - 1),
            };
            let level_radius = u32::from(s.patch_radius) / scale;
            let coordinate = if valid_level_candidate(s, level, preferred, level_radius, counters) {
                preferred
            } else {
                let start = splitmix(
                    r.seed
                        ^ version_salt()
                        ^ u64::from(level_index).rotate_left(9)
                        ^ u64::from(x).rotate_left(17)
                        ^ u64::from(y).rotate_left(39),
                ) % (u64::from(level.width) * u64::from(level.height));
                let mut found = None;
                for offset in 0..u64::from(level.width) * u64::from(level.height) {
                    let index =
                        (start + offset) % (u64::from(level.width) * u64::from(level.height));
                    let candidate = SourceCoordinate {
                        x: (index % u64::from(level.width)) as u32,
                        y: (index / u64::from(level.width)) as u32,
                    };
                    if valid_level_candidate(s, level, candidate, level_radius, counters) {
                        found = Some(candidate);
                        break;
                    }
                }
                found.ok_or(DomainError::UnusablePatchMatchSource {
                    rejected_candidates: counters.rejected_unusable,
                })?
            };
            out.push(Match {
                coordinate,
                cost: u64::MAX - 1,
            });
            count_op(counters, s, 1)?;
        }
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn optimize_pass_level(
    r: &DomainRequest,
    level: &DescriptorLevel,
    field: &mut [Match],
    ow: u32,
    oh: u32,
    scale: u32,
    radius: u32,
    level_index: u8,
    iteration: u16,
    forward: bool,
    counters: &mut Counters,
    cancel: &RenderCancellationToken,
) -> Result<(u64, u64, u64), DomainError> {
    let s = &r.patch_match;
    let snapshot = field.to_vec();
    let usage = usage_map_plain(&snapshot);
    let mut changed = 0;
    let mut propagation = 0;
    let mut random = 0;
    for order_y in 0..oh {
        let y = if forward { order_y } else { oh - 1 - order_y };
        if order_y % 16 == 0 {
            check_cancel(cancel)?;
        }
        for order_x in 0..ow {
            let x = if forward { order_x } else { ow - 1 - order_x };
            if !synth_at_level(s, x, y, scale) {
                continue;
            }
            let index = (y * ow + x) as usize;
            let original = field[index];
            let mut best = Match {
                coordinate: original.coordinate,
                cost: level_patch_cost(
                    s,
                    level,
                    &snapshot,
                    &usage,
                    ow,
                    oh,
                    x,
                    y,
                    original.coordinate,
                    radius,
                    counters,
                )?,
            };
            let directions = if forward {
                [(-1, 0), (0, -1)]
            } else {
                [(1, 0), (0, 1)]
            };
            for (dx, dy) in directions {
                if let Some((nx, ny)) = output_neighbor_dimensions(s, ow, oh, x, y, dx, dy) {
                    let neighbor = field[(ny * ow + nx) as usize].coordinate;
                    if let Some(candidate) =
                        shifted_source(neighbor, -dx, -dy, level.width, level.height)
                        && consider_level(
                            s, level, &snapshot, &usage, ow, oh, x, y, candidate, radius,
                            &mut best, counters,
                        )?
                    {
                        propagation += 1;
                    }
                }
            }
            let mut window = u32::from(s.random_search_radius)
                .div_ceil(scale)
                .max(1)
                .min(level.width.max(level.height));
            let mut round = 0_u64;
            while window > 0 {
                for candidate_index in 0..s.random_candidates_per_radius {
                    let hash = splitmix(
                        r.seed
                            ^ version_salt()
                            ^ u64::from(level_index).rotate_left(7)
                            ^ u64::from(iteration).rotate_left(13)
                            ^ u64::from(index as u32).rotate_left(29)
                            ^ round.rotate_left(41)
                            ^ u64::from(candidate_index),
                    );
                    let span = u64::from(window) * 2 + 1;
                    let dx = (hash % span) as i64 - i64::from(window);
                    let dy = ((hash >> 32) % span) as i64 - i64::from(window);
                    if let Some(candidate) =
                        shifted_source(best.coordinate, dx, dy, level.width, level.height)
                        && consider_level(
                            s, level, &snapshot, &usage, ow, oh, x, y, candidate, radius,
                            &mut best, counters,
                        )?
                    {
                        random += 1;
                    }
                }
                window /= 2;
                round += 1;
            }
            if best.coordinate != original.coordinate {
                changed += 1;
            }
            field[index] = best;
        }
    }
    Ok((changed, propagation, random))
}

#[allow(clippy::too_many_arguments)]
fn consider_level(
    s: &PatchMatchSettings,
    level: &DescriptorLevel,
    field: &[Match],
    usage: &BTreeMap<(u32, u32), u32>,
    ow: u32,
    oh: u32,
    x: u32,
    y: u32,
    candidate: SourceCoordinate,
    radius: u32,
    best: &mut Match,
    counters: &mut Counters,
) -> Result<bool, DomainError> {
    if !valid_level_candidate(s, level, candidate, radius, counters) {
        return Ok(false);
    }
    let cost = level_patch_cost(
        s, level, field, usage, ow, oh, x, y, candidate, radius, counters,
    )?;
    if (cost, candidate.y, candidate.x) < (best.cost, best.coordinate.y, best.coordinate.x) {
        *best = Match {
            coordinate: candidate,
            cost,
        };
        Ok(true)
    } else {
        Ok(false)
    }
}

fn valid_level_candidate(
    s: &PatchMatchSettings,
    level: &DescriptorLevel,
    center: SourceCoordinate,
    radius: u32,
    counters: &mut Counters,
) -> bool {
    for (dx, dy) in patch_offsets(radius) {
        let Some(c) = shifted_source(center, dx, dy, level.width, level.height) else {
            counters.rejected_unusable += 1;
            return false;
        };
        let d = level.pixels[(c.y * level.width + c.x) as usize];
        if d.excluded {
            counters.rejected_excluded += 1;
            return false;
        }
        if d.usability + 1.0e-6 < f64::from(s.minimum_usable_confidence_milli) / 1000.0 {
            counters.rejected_unusable += 1;
            return false;
        }
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn level_patch_cost(
    s: &PatchMatchSettings,
    level: &DescriptorLevel,
    field: &[Match],
    usage: &BTreeMap<(u32, u32), u32>,
    ow: u32,
    oh: u32,
    ox: u32,
    oy: u32,
    candidate: SourceCoordinate,
    radius: u32,
    counters: &mut Counters,
) -> Result<u64, DomainError> {
    let mut appearance = 0.0;
    let mut n = 0_u64;
    for (dx, dy) in patch_offsets(radius) {
        let Some((tx, ty)) = output_neighbor_dimensions(s, ow, oh, ox, oy, dx, dy) else {
            continue;
        };
        let mapped = field[(ty * ow + tx) as usize].coordinate;
        let Some(source) = shifted_source(candidate, dx, dy, level.width, level.height) else {
            appearance += 1.0;
            n += 1;
            continue;
        };
        appearance += descriptor_level_distance(level, mapped, source, s.weights);
        n += 1;
        count_op(counters, s, 1)?;
    }
    appearance /= n.max(1) as f64;
    let mut coherence = 0.0;
    let mut cn = 0.0;
    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
        if let Some((nx, ny)) = output_neighbor_dimensions(s, ow, oh, ox, oy, dx, dy) {
            let neighbor = field[(ny * ow + nx) as usize].coordinate;
            coherence += ((i64::from(neighbor.x) - (i64::from(candidate.x) + dx)).unsigned_abs()
                as f64
                / f64::from(level.width.max(1))
                + (i64::from(neighbor.y) - (i64::from(candidate.y) + dy)).unsigned_abs() as f64
                    / f64::from(level.height.max(1)))
                * 0.5;
            cn += 1.0;
        }
    }
    coherence /= if cn < 1.0 { 1.0 } else { cn };
    let descriptor = level.pixels[(candidate.y * level.width + candidate.x) as usize];
    let duplicate = f64::from(*usage.get(&(candidate.x, candidate.y)).unwrap_or(&0))
        / (1.0 + field.len() as f64);
    let sum = weight_sum_patchmatch(s.weights);
    let total = (appearance
        + f64::from(s.weights.coherence) * coherence
        + f64::from(s.weights.duplicate_use) * duplicate
        + f64::from(s.weights.saliency_repeat) * duplicate * descriptor.saliency)
        / sum;
    Ok((total.clamp(0.0, 65_535.0) * 1_000_000.0).round() as u64)
}

fn descriptor_level_distance(
    level: &DescriptorLevel,
    a: SourceCoordinate,
    b: SourceCoordinate,
    w: PatchMatchWeights,
) -> f64 {
    let a = level.pixels[(a.y * level.width + a.x) as usize];
    let b = level.pixels[(b.y * level.width + b.x) as usize];
    let color = a
        .rgb
        .into_iter()
        .zip(b.rgb)
        .map(|(x, y)| (x - y).abs())
        .sum::<f64>()
        / 3.0;
    let normal = (1.0
        - a.normal
            .into_iter()
            .zip(b.normal)
            .map(|(x, y)| x * y)
            .sum::<f64>())
    .clamp(0.0, 2.0)
        * 0.5;
    f64::from(w.color) * color
        + f64::from(w.gradient) * (a.gradient - b.gradient).abs()
        + f64::from(w.height) * (a.height - b.height).abs()
        + f64::from(w.vector_normal) * normal
        + f64::from(w.roughness) * (a.roughness - b.roughness).abs()
        + f64::from(w.structure) * (a.structure - b.structure).abs()
}

fn weight_sum_patchmatch(w: PatchMatchWeights) -> f64 {
    [
        w.color,
        w.gradient,
        w.height,
        w.vector_normal,
        w.roughness,
        w.structure,
        w.coherence,
        w.duplicate_use,
        w.saliency_repeat,
    ]
    .into_iter()
    .map(f64::from)
    .sum::<f64>()
    .max(1.0)
}
fn usage_map_plain(field: &[Match]) -> BTreeMap<(u32, u32), u32> {
    let mut usage = BTreeMap::new();
    for entry in field {
        *usage
            .entry((entry.coordinate.x, entry.coordinate.y))
            .or_default() += 1;
    }
    usage
}
fn output_neighbor_dimensions(
    s: &PatchMatchSettings,
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    dx: i64,
    dy: i64,
) -> Option<(u32, u32)> {
    let mut nx = i64::from(x) + dx;
    let mut ny = i64::from(y) + dy;
    if s.seamless_x {
        nx = nx.rem_euclid(i64::from(width));
    }
    if s.seamless_y {
        ny = ny.rem_euclid(i64::from(height));
    }
    (nx >= 0 && ny >= 0 && nx < i64::from(width) && ny < i64::from(height))
        .then_some((nx as u32, ny as u32))
}
fn normalize64(v: [f64; 3]) -> [f64; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if length > 1.0e-12 {
        [v[0] / length, v[1] / length, v[2] / length]
    } else {
        [0.0, 0.0, 1.0]
    }
}

fn enforce_periodic_correspondence(
    s: &PatchMatchSettings,
    field: &mut [Match],
    counters: &mut Counters,
) -> Result<(), DomainError> {
    if s.completion_mask.is_some() {
        return Ok(());
    }
    if s.seamless_x {
        for y in 0..s.output_height {
            let first = field[(y * s.output_width) as usize];
            field[(y * s.output_width + s.output_width - 1) as usize] = first;
            count_op(counters, s, 1)?;
        }
    }
    if s.seamless_y {
        for x in 0..s.output_width {
            let first = field[x as usize];
            field[((s.output_height - 1) * s.output_width + x) as usize] = first;
            count_op(counters, s, 1)?;
        }
    }
    Ok(())
}

fn measured_patchmatch_boundary(
    channels: &[PreparedExemplarChannel],
    width: u32,
    height: u32,
) -> (u16, u16) {
    let mut horizontal = 0.0_f64;
    let mut vertical = 0.0_f64;
    let mut terms = 0.0_f64;
    for channel in channels {
        match channel {
            PreparedExemplarChannel::BaseColor { plane, .. } => {
                horizontal += (0..height)
                    .map(|y| {
                        f64::from(color_difference(
                            plane.pixel(0, y),
                            plane.pixel(width - 1, y),
                        ))
                    })
                    .sum::<f64>()
                    / f64::from(height);
                vertical += (0..width)
                    .map(|x| {
                        f64::from(color_difference(
                            plane.pixel(x, 0),
                            plane.pixel(x, height - 1),
                        ))
                    })
                    .sum::<f64>()
                    / f64::from(width);
                terms += 1.0;
            }
            PreparedExemplarChannel::Scalar { plane, .. } => {
                horizontal += (0..height)
                    .map(|y| f64::from((plane.pixel(0, y).0 - plane.pixel(width - 1, y).0).abs()))
                    .sum::<f64>()
                    / f64::from(height);
                vertical += (0..width)
                    .map(|x| f64::from((plane.pixel(x, 0).0 - plane.pixel(x, height - 1).0).abs()))
                    .sum::<f64>()
                    / f64::from(width);
                terms += 1.0;
            }
            PreparedExemplarChannel::Normal { plane, .. } => {
                horizontal += (0..height)
                    .map(|y| {
                        f64::from(
                            (1.0 - dot(plane.pixel(0, y).xyz, plane.pixel(width - 1, y).xyz))
                                .clamp(0.0, 2.0),
                        ) * 0.5
                    })
                    .sum::<f64>()
                    / f64::from(height);
                vertical += (0..width)
                    .map(|x| {
                        f64::from(
                            (1.0 - dot(plane.pixel(x, 0).xyz, plane.pixel(x, height - 1).xyz))
                                .clamp(0.0, 2.0),
                        ) * 0.5
                    })
                    .sum::<f64>()
                    / f64::from(width);
                terms += 1.0;
            }
            PreparedExemplarChannel::MaterialId { plane } => {
                horizontal += (0..height)
                    .map(|y| {
                        if plane.pixel(0, y) != plane.pixel(width - 1, y) {
                            1.0
                        } else {
                            0.0
                        }
                    })
                    .sum::<f64>()
                    / f64::from(height);
                vertical += (0..width)
                    .map(|x| {
                        if plane.pixel(x, 0) != plane.pixel(x, height - 1) {
                            1.0
                        } else {
                            0.0
                        }
                    })
                    .sum::<f64>()
                    / f64::from(width);
                terms += 1.0;
            }
            PreparedExemplarChannel::Mask { plane, .. } => {
                horizontal += (0..height)
                    .map(|y| f64::from((plane.pixel(0, y).0 - plane.pixel(width - 1, y).0).abs()))
                    .sum::<f64>()
                    / f64::from(height);
                vertical += (0..width)
                    .map(|x| f64::from((plane.pixel(x, 0).0 - plane.pixel(x, height - 1).0).abs()))
                    .sum::<f64>()
                    / f64::from(width);
                terms += 1.0;
            }
        }
    }
    (
        score((horizontal / terms.max(1.0)) as f32),
        score((vertical / terms.max(1.0)) as f32),
    )
}

fn patch_offsets(radius: u32) -> Vec<(i64, i64)> {
    if radius == 0 {
        return vec![(0, 0)];
    }
    let r = i64::from(radius);
    let half = (r / 2).max(1);
    vec![
        (0, 0),
        (-r, 0),
        (r, 0),
        (0, -r),
        (0, r),
        (-half, -half),
        (half, -half),
        (-half, half),
        (half, half),
    ]
}

fn scalar_channel(
    r: &DomainRequest,
    role: MaterialChannelRole,
) -> Option<&ImagePlane<LinearScalar>> {
    r.source.channels.iter().find_map(|c| match c {
        PreparedExemplarChannel::Scalar { role: found, plane } if *found == role => Some(plane),
        _ => None,
    })
}
fn normal_channel(r: &DomainRequest) -> Option<&ImagePlane<TangentNormal>> {
    r.source.channels.iter().find_map(|c| match c {
        PreparedExemplarChannel::Normal { plane, .. } => Some(plane),
        _ => None,
    })
}
fn structure_value(r: &DomainRequest, c: SourceCoordinate) -> f64 {
    let p = &r.analysis.structure;
    [
        &p.edge,
        &p.line,
        &p.boundary,
        &p.grid,
        &p.fiber,
        &p.intersection,
    ]
    .into_iter()
    .filter_map(|v| v.level(0))
    .map(|v| f64::from(v.pixel(c.x, c.y).0))
    .fold(0.0, f64::max)
}

fn shifted_source(
    c: SourceCoordinate,
    dx: i64,
    dy: i64,
    width: u32,
    height: u32,
) -> Option<SourceCoordinate> {
    let x = i64::from(c.x) + dx;
    let y = i64::from(c.y) + dy;
    (x >= 0 && y >= 0 && x < i64::from(width) && y < i64::from(height)).then_some(
        SourceCoordinate {
            x: x as u32,
            y: y as u32,
        },
    )
}
fn cost_confidence(cost: u64) -> u16 {
    1000_u16.saturating_sub((cost / 1_000).min(1000) as u16)
}
fn count_op(c: &mut Counters, s: &PatchMatchSettings, amount: u64) -> Result<(), DomainError> {
    c.operations = c.operations.saturating_add(amount);
    if c.operations > s.max_operations {
        Err(DomainError::ResourceLimitExceeded)
    } else {
        Ok(())
    }
}
fn version_salt() -> u64 {
    0x08c0_0000_0000_0003
}
fn splitmix(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}
