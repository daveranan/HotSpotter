//! Non-exportable composition of authoritative Stage 14 slot results into Stage 9 topology.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};

use crate::ResolvedRegion;
use hot_trimmer_domain::{
    AlgorithmProvenance, CompilationDiagnostic, CompiledTemplateTopology, ContentDigest,
    EdgeEligibility, GridRect, ManualRegionRole, MaterialChannelRole, MaterialMapKind,
    RegionContinuity, RegionId, RegionSampling, SamplingMode,
};
use hot_trimmer_material_synthesis::PreparedMaterialDomain;
use hot_trimmer_placement_solver::{CandidateTransform, PlacementPlan, SamplingPlan, SourceCrop};
use hot_trimmer_render_core::PreparedExemplarChannel;

use crate::SynthesizedSlotMaterial;

pub const INTERMEDIATE_ATLAS_LABEL: &str = "Intermediate Stage 14 material-placement preview";
pub const INCOMPLETE_AFTER_STAGE: u8 = 14;

#[derive(Clone, Debug)]
pub struct IntermediateAtlasRequest<'a> {
    pub topology: &'a CompiledTemplateTopology,
    pub placement_plan: &'a PlacementPlan,
    pub slots: Vec<IntermediateSlotInput<'a>>,
    pub revision: u64,
    pub algorithm_versions: BTreeMap<u8, AlgorithmProvenance>,
    pub diagnostics: Vec<CompilationDiagnostic>,
    pub regions: Vec<ResolvedRegion>,
}

#[derive(Clone, Debug)]
pub struct IntermediateSlotInput<'a> {
    pub region_id: RegionId,
    pub slot_key: &'a str,
    pub display_name: &'a str,
    pub required: bool,
    pub patch_id: Option<String>,
    pub domain: &'a PreparedMaterialDomain,
    pub plan: &'a SamplingPlan,
    pub result: &'a SynthesizedSlotMaterial,
    pub grid_rect: Option<GridRect>,
    pub behavior: hot_trimmer_domain::RegionBehavior,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntermediateAtlasChannel {
    pub role: MaterialChannelRole,
    /// Display-ready RGBA8. Linear scalar and vector values retain their registered semantics.
    pub rgba8: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IntermediateSlotInspection {
    pub region_id: RegionId,
    pub slot_key: String,
    pub display_name: String,
    pub allocation: hot_trimmer_domain::CanonicalRect,
    pub hotspot: hot_trimmer_domain::CanonicalRect,
    pub semantic_rect: hot_trimmer_domain::CanonicalRect,
    pub padded_rect: hot_trimmer_domain::CanonicalRect,
    pub atlas_destination: hot_trimmer_domain::CanonicalRect,
    pub preview_padding_px: u32,
    pub mapping_mode: SamplingMode,
    pub source_transform: CandidateTransform,
    pub isotropic_scale: f64,
    pub sampling_scale: f64,
    pub valid_pixel_count: u64,
    pub source_id: ContentDigest,
    pub patch_id: Option<String>,
    pub domain_id: ContentDigest,
    pub candidate_id: ContentDigest,
    pub sampling_plan_id: ContentDigest,
    pub stage_14_result_id: ContentDigest,
    pub source_crop: Option<SourceCrop>,
    pub grid_rect: Option<GridRect>,
    pub behavior_version: u16,
    pub role: ManualRegionRole,
    pub continuity: RegionContinuity,
    pub requested_sampling: RegionSampling,
    pub executed_mode: SamplingMode,
    pub edge_eligibility: EdgeEligibility,
    pub period_pixels: Option<[u32; 2]>,
    pub address_mode: &'static str,
    /// Authoritative Stage 15 plan compiled on the persisted spine. This is
    /// metadata for inspection; pixels continue to come from GPU tile publications.
    pub compiled_profile: Option<hot_trimmer_effect_compiler::CompiledProfile>,
    /// Authoritative Stage 16 plan compiled on the persisted spine.
    pub compiled_details: Option<hot_trimmer_effect_compiler::CompiledDetailSet>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IntermediateAtlasArtifact {
    pub label: &'static str,
    pub non_exportable: bool,
    pub incomplete_after_stage: u8,
    pub revision: u64,
    /// Exact Stage 9 value, retained rather than reconstructed from slot pixels.
    pub topology: CompiledTemplateTopology,
    pub placement_plan_id: ContentDigest,
    pub channels: Vec<IntermediateAtlasChannel>,
    pub unavailable_channels: Vec<MaterialChannelRole>,
    pub correspondence: Vec<[f32; 2]>,
    pub validity: Vec<u8>,
    /// Per-pixel RegionId ownership, including padding. This categorical authority is
    /// retained without encoding unrelated material channels for a Base Color preview.
    pub region_ownership: Vec<RegionId>,
    /// Stable compact-index lookup for GPU Region ID tiles. GPU artifacts include
    /// authored role/continuity/sampling/edge semantics for external consumers.
    pub region_id_lookup: Vec<crate::CompiledCompactRegionIdLookup>,
    pub slots: Vec<IntermediateSlotInspection>,
    pub algorithm_versions: BTreeMap<u8, AlgorithmProvenance>,
    pub diagnostics: Vec<CompilationDiagnostic>,
    /// Profile-local overlay records produced beside the pixels and slots.
    pub regions: Vec<ResolvedRegion>,
    /// Measured persisted-spine facts.  These describe the exact artifact, never a
    /// separately reconstructed preview.
    pub telemetry: Vec<String>,
    /// Bounded raw GPU tile returned by the persisted Stage 14 compile, when the
    /// GPU executor is selected for interactive preview publication.
    pub rendered_tile: Option<Arc<crate::GpuAtlasRenderedTile>>,
    /// All bounded raw GPU material-map tiles returned by the persisted Stage 14
    /// compile. CPU artifacts leave this empty.
    pub rendered_tiles: BTreeMap<MaterialMapKind, Arc<crate::GpuAtlasRenderedTile>>,
    pub rendered_tile_timings: BTreeMap<MaterialMapKind, crate::GpuAtlasTileTiming>,
    /// Paintable GPU display publications for material maps. Typed working
    /// resources remain in `rendered_tiles`.
    pub rendered_display_tiles: BTreeMap<MaterialMapKind, Arc<crate::GpuAtlasRenderedTile>>,
    pub pending: Vec<&'static str>,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum IntermediateAtlasError {
    #[error("Stage 9 topology has invalid or unbounded dimensions")]
    InvalidTopology,
    #[error("Stage 13 placement plan is incomplete")]
    IncompletePlacementPlan,
    #[error("required Stage 9 slot '{0}' has no successful Stage 14 result")]
    MissingRequiredSlot(String),
    #[error("Stage 9, Stage 13, and Stage 14 disagree for slot '{0}'")]
    SlotArtifactMismatch(String),
    #[error("intermediate preview was cancelled")]
    Cancelled,
    #[error("intermediate preview revision was superseded")]
    RevisionSuperseded,
    #[error("authoritative artifact identity could not be serialized: {0}")]
    Identity(String),
}

pub(crate) fn compose_intermediate_atlas(
    request: &IntermediateAtlasRequest<'_>,
    cancelled: impl Fn() -> bool,
    current_revision: impl Fn() -> u64,
) -> Result<IntermediateAtlasArtifact, IntermediateAtlasError> {
    if cancelled() {
        return Err(IntermediateAtlasError::Cancelled);
    }
    if current_revision() != request.revision {
        return Err(IntermediateAtlasError::RevisionSuperseded);
    }
    if !placement_plan_has_complete_required_assignments(request.placement_plan, &request.slots)
        || !placement_plan_uses_stage14_executable_mappings(request.placement_plan)
    {
        return Err(IntermediateAtlasError::IncompletePlacementPlan);
    }
    let width = request.topology.output_size.width;
    let height = request.topology.output_size.height;
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|n| usize::try_from(n).ok())
        .ok_or(IntermediateAtlasError::InvalidTopology)?;
    if width == 0 || height == 0 {
        return Err(IntermediateAtlasError::InvalidTopology);
    }

    let placement_plan_id = ContentDigest::sha256(
        &request
            .placement_plan
            .deterministic_bytes()
            .map_err(|error| IntermediateAtlasError::Identity(error.to_string()))?,
    );
    let required_keys = request
        .slots
        .iter()
        .filter(|slot| slot.required)
        .map(|slot| slot.slot_key)
        .collect::<BTreeSet<_>>();
    for topology_slot in &request.topology.slots {
        if !request
            .slots
            .iter()
            .any(|slot| slot.slot_key == topology_slot.slot_key)
        {
            return Err(IntermediateAtlasError::MissingRequiredSlot(
                topology_slot.slot_key.clone(),
            ));
        }
    }
    if required_keys.len() != request.slots.iter().filter(|slot| slot.required).count() {
        return Err(IntermediateAtlasError::IncompletePlacementPlan);
    }

    let roles = request
        .slots
        .iter()
        .filter(|slot| slot.required)
        .map(|slot| {
            slot.result
                .channels
                .iter()
                .map(PreparedExemplarChannel::role)
                .collect::<BTreeSet<_>>()
        })
        .reduce(|left, right| left.intersection(&right).copied().collect())
        .unwrap_or_default();
    if !roles.contains(&MaterialChannelRole::BaseColor) {
        return Err(IntermediateAtlasError::MissingRequiredSlot(
            "base_color".into(),
        ));
    }
    let mut channel_pixels = roles
        .iter()
        .map(|role| (*role, vec![0_u8; pixels * 4]))
        .collect::<BTreeMap<_, _>>();
    let mut correspondence = vec![[f32::NAN; 2]; pixels];
    let mut validity = vec![0_u8; pixels];
    let mut region_ownership = vec![None; pixels];
    let mut inspections = Vec::with_capacity(request.slots.len());

    for input in &request.slots {
        if cancelled() {
            return Err(IntermediateAtlasError::Cancelled);
        }
        if current_revision() != request.revision {
            return Err(IntermediateAtlasError::RevisionSuperseded);
        }
        let topology_slot = request
            .topology
            .slots
            .iter()
            .find(|slot| slot.slot_key == input.slot_key)
            .ok_or_else(|| IntermediateAtlasError::SlotArtifactMismatch(input.slot_key.into()))?;
        let allocation = topology_slot.allocation;
        let semantic = topology_slot.hotspot;
        if request
            .placement_plan
            .placements
            .iter()
            .find(|plan| plan.slot_id == input.plan.slot_id)
            != Some(input.plan)
            || input.plan.slot_id != input.region_id
            || input.result.width != semantic.width
            || input.result.height != semantic.height
            || allocation
                .x
                .checked_add(allocation.width)
                .is_none_or(|end| end > width)
            || allocation
                .y
                .checked_add(allocation.height)
                .is_none_or(|end| end > height)
            || semantic.x < allocation.x
            || semantic.y < allocation.y
            || semantic
                .x
                .checked_add(semantic.width)
                .is_none_or(|end| end > allocation.x + allocation.width)
            || semantic
                .y
                .checked_add(semantic.height)
                .is_none_or(|end| end > allocation.y + allocation.height)
            || semantic.width == 0
            || semantic.height == 0
            || input.result.diagnostics.executed_mode != input.plan.candidate.mapping_mode
            || input.plan.candidate.domain_id != input.domain.cache_key
        {
            return Err(IntermediateAtlasError::SlotArtifactMismatch(
                input.slot_key.into(),
            ));
        }
        let plan_id = ContentDigest::sha256(
            &serde_json::to_vec(input.plan)
                .map_err(|error| IntermediateAtlasError::Identity(error.to_string()))?,
        );
        let result_id = slot_result_id(input.result, &plan_id);
        for y in 0..semantic.height {
            for x in 0..semantic.width {
                let atlas = usize::try_from(
                    u64::from(semantic.y + y) * u64::from(width) + u64::from(semantic.x + x),
                )
                .map_err(|_| IntermediateAtlasError::InvalidTopology)?;
                correspondence[atlas] = *input.result.correspondence.pixel(x, y);
                let is_valid = input.result.validity.pixel(x, y).0 >= 0.5;
                validity[atlas] = u8::from(is_valid) * 255;
                if is_valid {
                    for role in &roles {
                        let channel = input
                            .result
                            .channels
                            .iter()
                            .find(|channel| channel.role() == *role)
                            .ok_or_else(|| {
                                IntermediateAtlasError::SlotArtifactMismatch(input.slot_key.into())
                            })?;
                        write_rgba(
                            channel_pixels.get_mut(role).expect("role buffer exists"),
                            atlas,
                            channel,
                            x,
                            y,
                        );
                    }
                }
            }
        }
        dilate_invalid_semantic(
            semantic,
            width,
            &mut correspondence,
            &mut validity,
            &mut channel_pixels,
        )?;
        for y in allocation.y..allocation.y + allocation.height {
            for x in allocation.x..allocation.x + allocation.width {
                let atlas = usize::try_from(u64::from(y) * u64::from(width) + u64::from(x))
                    .map_err(|_| IntermediateAtlasError::InvalidTopology)?;
                if region_ownership[atlas].replace(input.region_id).is_some() {
                    return Err(IntermediateAtlasError::SlotArtifactMismatch(
                        input.slot_key.into(),
                    ));
                }
                if x < semantic.x
                    || x >= semantic.x + semantic.width
                    || y < semantic.y
                    || y >= semantic.y + semantic.height
                {
                    let owner_x = x.clamp(semantic.x, semantic.x + semantic.width - 1);
                    let owner_y = y.clamp(semantic.y, semantic.y + semantic.height - 1);
                    let owner =
                        usize::try_from(u64::from(owner_y) * u64::from(width) + u64::from(owner_x))
                            .map_err(|_| IntermediateAtlasError::InvalidTopology)?;
                    correspondence[atlas] = correspondence[owner];
                    validity[atlas] = validity[owner];
                    for pixels in channel_pixels.values_mut() {
                        let source = pixels[owner * 4..owner * 4 + 4].to_vec();
                        pixels[atlas * 4..atlas * 4 + 4].copy_from_slice(&source);
                    }
                }
            }
        }
        let valid_count = (allocation.y..allocation.y + allocation.height)
            .flat_map(|y| (allocation.x..allocation.x + allocation.width).map(move |x| (x, y)))
            .filter(|(x, y)| validity[(*y * width + *x) as usize] > 0)
            .count() as u64;
        let preview_padding_px = semantic
            .x
            .saturating_sub(allocation.x)
            .max(semantic.y.saturating_sub(allocation.y))
            .max((allocation.x + allocation.width).saturating_sub(semantic.x + semantic.width))
            .max((allocation.y + allocation.height).saturating_sub(semantic.y + semantic.height));
        inspections.push(IntermediateSlotInspection {
            region_id: input.region_id,
            slot_key: input.slot_key.into(),
            display_name: input.display_name.into(),
            allocation,
            hotspot: semantic,
            semantic_rect: semantic,
            padded_rect: allocation,
            atlas_destination: allocation,
            preview_padding_px,
            mapping_mode: input.plan.candidate.mapping_mode,
            source_transform: input.plan.candidate.transform,
            isotropic_scale: input.plan.candidate.isotropic_scale,
            sampling_scale: input.plan.sampling_policy.scale,
            valid_pixel_count: valid_count,
            source_id: input.plan.candidate.source_id.clone(),
            patch_id: input.patch_id.clone(),
            domain_id: input.domain.cache_key.clone(),
            candidate_id: input.plan.candidate.candidate_id.clone(),
            sampling_plan_id: plan_id,
            stage_14_result_id: result_id,
            source_crop: input.plan.candidate.crop,
            grid_rect: input.grid_rect,
            behavior_version: input.behavior.version,
            role: input.behavior.role,
            continuity: input.behavior.continuity,
            requested_sampling: input.behavior.sampling,
            executed_mode: input.result.diagnostics.executed_mode,
            edge_eligibility: input.behavior.edge_eligibility,
            period_pixels: input.plan.candidate.period_pixels,
            address_mode: match input.behavior.sampling {
                RegionSampling::OneShot => "clamp",
                RegionSampling::LoopX => "repeat_x",
                RegionSampling::LoopY => "repeat_y",
                RegionSampling::LoopXy => "repeat_xy",
            },
            compiled_profile: None,
            compiled_details: None,
        });
    }
    if cancelled() {
        return Err(IntermediateAtlasError::Cancelled);
    }
    if current_revision() != request.revision {
        return Err(IntermediateAtlasError::RevisionSuperseded);
    }

    let all_importable = [
        MaterialChannelRole::Normal,
        MaterialChannelRole::Height,
        MaterialChannelRole::Roughness,
        MaterialChannelRole::Metallic,
        MaterialChannelRole::AmbientOcclusion,
        MaterialChannelRole::Specular,
        MaterialChannelRole::Opacity,
        MaterialChannelRole::EdgeMask,
        MaterialChannelRole::RegionId,
        MaterialChannelRole::MaterialId,
    ];
    fill_unowned_transparent_pixels(&mut region_ownership, request.topology, &request.slots);
    Ok(IntermediateAtlasArtifact {
        label: INTERMEDIATE_ATLAS_LABEL,
        non_exportable: true,
        incomplete_after_stage: INCOMPLETE_AFTER_STAGE,
        revision: request.revision,
        topology: request.topology.clone(),
        placement_plan_id,
        channels: channel_pixels
            .into_iter()
            .map(|(role, rgba8)| IntermediateAtlasChannel { role, rgba8 })
            .collect(),
        unavailable_channels: all_importable
            .into_iter()
            .filter(|role| !roles.contains(role))
            .collect(),
        correspondence,
        validity,
        region_ownership: region_ownership
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or(IntermediateAtlasError::IncompletePlacementPlan)?,
        region_id_lookup: Vec::new(),
        slots: inspections,
        algorithm_versions: request.algorithm_versions.clone(),
        diagnostics: request.diagnostics.clone(),
        regions: request.regions.clone(),
        telemetry: Vec::new(),
        rendered_tile: None,
        rendered_tiles: BTreeMap::new(),
        rendered_tile_timings: BTreeMap::new(),
        rendered_display_tiles: BTreeMap::new(),
        pending: vec![
            "profiles",
            "semantic details",
            "effects",
            "final PBR composition",
            "finishing",
            "mips",
            "metadata",
            "export",
            "Blender application",
        ],
    })
}

fn dilate_invalid_semantic(
    semantic: hot_trimmer_domain::CanonicalRect,
    atlas_width: u32,
    correspondence: &mut [[f32; 2]],
    validity: &mut [u8],
    channels: &mut BTreeMap<MaterialChannelRole, Vec<u8>>,
) -> Result<(), IntermediateAtlasError> {
    let local_pixels = usize::try_from(u64::from(semantic.width) * u64::from(semantic.height))
        .map_err(|_| IntermediateAtlasError::InvalidTopology)?;
    let mut nearest = vec![usize::MAX; local_pixels];
    let mut queue = VecDeque::new();
    for y in 0..semantic.height {
        for x in 0..semantic.width {
            let local = (y * semantic.width + x) as usize;
            let atlas = ((semantic.y + y) * atlas_width + semantic.x + x) as usize;
            if validity[atlas] > 0 {
                nearest[local] = local;
                queue.push_back(local);
            }
        }
    }
    if queue.is_empty() {
        return Err(IntermediateAtlasError::MissingRequiredSlot(
            "base_color_validity".into(),
        ));
    }
    while let Some(local) = queue.pop_front() {
        let x = local as u32 % semantic.width;
        let y = local as u32 / semantic.width;
        let neighbors = [
            x.checked_sub(1).map(|nx| (nx, y)),
            (x + 1 < semantic.width).then_some((x + 1, y)),
            y.checked_sub(1).map(|ny| (x, ny)),
            (y + 1 < semantic.height).then_some((x, y + 1)),
        ];
        for (nx, ny) in neighbors.into_iter().flatten() {
            let next = (ny * semantic.width + nx) as usize;
            if nearest[next] == usize::MAX {
                nearest[next] = nearest[local];
                queue.push_back(next);
            }
        }
    }
    for (local, owner_local) in nearest.into_iter().enumerate() {
        let x = local as u32 % semantic.width;
        let y = local as u32 / semantic.width;
        let atlas = ((semantic.y + y) * atlas_width + semantic.x + x) as usize;
        if validity[atlas] > 0 {
            continue;
        }
        let owner_x = owner_local as u32 % semantic.width;
        let owner_y = owner_local as u32 / semantic.width;
        let owner = ((semantic.y + owner_y) * atlas_width + semantic.x + owner_x) as usize;
        correspondence[atlas] = correspondence[owner];
        validity[atlas] = validity[owner];
        for pixels in channels.values_mut() {
            let source = pixels[owner * 4..owner * 4 + 4].to_vec();
            pixels[atlas * 4..atlas * 4 + 4].copy_from_slice(&source);
        }
    }
    Ok(())
}

fn slot_result_id(result: &SynthesizedSlotMaterial, plan_id: &ContentDigest) -> ContentDigest {
    let mut bytes = plan_id.0.as_bytes().to_vec();
    bytes.extend_from_slice(
        format!(
            "{}x{}|{:?}|{:?}",
            result.width, result.height, result.diagnostics, result.stage_result
        )
        .as_bytes(),
    );
    for point in result.correspondence.to_row_major() {
        bytes.extend_from_slice(&point[0].to_le_bytes());
        bytes.extend_from_slice(&point[1].to_le_bytes());
    }
    for value in result.validity.to_row_major() {
        bytes.extend_from_slice(&value.0.to_le_bytes());
    }
    for channel in &result.channels {
        append_channel_identity(&mut bytes, channel);
    }
    ContentDigest::sha256(&bytes)
}

fn append_channel_identity(bytes: &mut Vec<u8>, channel: &PreparedExemplarChannel) {
    bytes.extend_from_slice(format!("{:?}|{:?}|", channel.role(), channel.dimensions()).as_bytes());
    match channel {
        PreparedExemplarChannel::BaseColor { plane, alpha_mode } => {
            bytes.extend_from_slice(format!("{alpha_mode:?}|").as_bytes());
            for value in plane.to_row_major() {
                for component in value.rgb {
                    bytes.extend_from_slice(&component.to_le_bytes());
                }
                bytes.extend_from_slice(&value.alpha.to_le_bytes());
            }
        }
        PreparedExemplarChannel::Scalar { plane, .. } => {
            for value in plane.to_row_major() {
                bytes.extend_from_slice(&value.0.to_le_bytes());
            }
        }
        PreparedExemplarChannel::Normal {
            plane,
            source_convention,
            canonical_convention,
            alpha_policy,
        } => {
            bytes.extend_from_slice(
                format!("{source_convention:?}|{canonical_convention:?}|{alpha_policy:?}|")
                    .as_bytes(),
            );
            for value in plane.to_row_major() {
                for component in value.xyz {
                    bytes.extend_from_slice(&component.to_le_bytes());
                }
                bytes.extend_from_slice(&value.alpha.to_le_bytes());
            }
        }
        PreparedExemplarChannel::MaterialId { plane } => {
            for value in plane.to_row_major() {
                bytes.extend_from_slice(&value.0.to_le_bytes());
            }
        }
        PreparedExemplarChannel::Mask { plane, .. } => {
            for value in plane.to_row_major() {
                bytes.extend_from_slice(&value.0.to_le_bytes());
            }
        }
    }
}

fn placement_plan_uses_stage14_executable_mappings(placement_plan: &PlacementPlan) -> bool {
    placement_plan.placements.iter().all(|placement| {
        matches!(
            placement.candidate.mapping_mode,
            SamplingMode::DirectCrop
                | SamplingMode::PeriodicTile
                | SamplingMode::RepeatX
                | SamplingMode::RepeatY
                | SamplingMode::UniqueContain
                | SamplingMode::UniqueCover
                | SamplingMode::ThreeSliceCap
                | SamplingMode::NineSlicePanel
                | SamplingMode::PlanarRadial
                | SamplingMode::PolarRadial
                | SamplingMode::ExplicitStretch
        )
    })
}

fn placement_plan_has_complete_required_assignments(
    placement_plan: &PlacementPlan,
    slots: &[IntermediateSlotInput<'_>],
) -> bool {
    if placement_plan.placements.len() != slots.len() {
        return false;
    }
    let assigned = placement_plan
        .placements
        .iter()
        .map(|placement| placement.slot_id)
        .collect::<BTreeSet<_>>();
    slots.iter().all(|slot| assigned.contains(&slot.region_id))
}

fn fill_unowned_transparent_pixels(
    region_ownership: &mut [Option<RegionId>],
    topology: &CompiledTemplateTopology,
    slots: &[IntermediateSlotInput<'_>],
) {
    let Some(first_slot) = slots.first() else {
        return;
    };
    let width = topology.output_size.width;
    if width == 0 {
        return;
    }
    for (index, owner) in region_ownership.iter_mut().enumerate() {
        if owner.is_some() {
            continue;
        }
        let x = u32::try_from(index).unwrap_or(u32::MAX) % width;
        let y = u32::try_from(index).unwrap_or(u32::MAX) / width;
        let nearest = slots
            .iter()
            .filter_map(|slot| {
                topology
                    .slots
                    .iter()
                    .find(|topology_slot| topology_slot.slot_key == slot.slot_key)
                    .map(|topology_slot| {
                        (
                            slot.region_id,
                            rect_distance_sq(x, y, topology_slot.allocation),
                        )
                    })
            })
            .min_by_key(|(_, distance)| *distance)
            .map(|(region_id, _)| region_id)
            .unwrap_or(first_slot.region_id);
        *owner = Some(nearest);
    }
}

fn rect_distance_sq(x: u32, y: u32, rect: hot_trimmer_domain::CanonicalRect) -> u64 {
    let right = rect.x.saturating_add(rect.width).saturating_sub(1);
    let bottom = rect.y.saturating_add(rect.height).saturating_sub(1);
    let dx = if x < rect.x {
        rect.x - x
    } else if x > right {
        x - right
    } else {
        0
    };
    let dy = if y < rect.y {
        rect.y - y
    } else if y > bottom {
        y - bottom
    } else {
        0
    };
    u64::from(dx) * u64::from(dx) + u64::from(dy) * u64::from(dy)
}

fn write_rgba(
    target: &mut [u8],
    atlas_pixel: usize,
    channel: &PreparedExemplarChannel,
    x: u32,
    y: u32,
) {
    let at = atlas_pixel * 4;
    let rgba = match channel {
        PreparedExemplarChannel::BaseColor { plane, .. } => {
            let value = plane.pixel(x, y);
            [
                linear_to_srgb(value.rgb[0]),
                linear_to_srgb(value.rgb[1]),
                linear_to_srgb(value.rgb[2]),
                unit(value.alpha),
            ]
        }
        PreparedExemplarChannel::Scalar { plane, .. } => {
            let value = unit(plane.pixel(x, y).0);
            [value, value, value, 255]
        }
        PreparedExemplarChannel::Normal { plane, .. } => {
            let value = plane.pixel(x, y);
            [
                signed(value.xyz[0]),
                signed(value.xyz[1]),
                signed(value.xyz[2]),
                unit(value.alpha),
            ]
        }
        PreparedExemplarChannel::MaterialId { plane } => {
            let value = plane.pixel(x, y).0.to_le_bytes();
            [value[0], value[1], value[2], 255]
        }
        PreparedExemplarChannel::Mask { plane, .. } => {
            let value = unit(plane.pixel(x, y).0);
            [value, value, value, 255]
        }
    };
    target[at..at + 4].copy_from_slice(&rgba);
}

fn unit(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}
fn signed(value: f32) -> u8 {
    unit(value.mul_add(0.5, 0.5))
}
fn linear_to_srgb(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    unit(if value <= 0.003_130_8 {
        12.92 * value
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    })
}
