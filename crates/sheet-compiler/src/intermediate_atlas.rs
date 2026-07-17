//! Non-exportable composition of authoritative Stage 14 slot results into Stage 9 topology.

use std::collections::{BTreeMap, BTreeSet};

use hot_trimmer_domain::{
    AlgorithmProvenance, CompilationDiagnostic, CompiledTemplateTopology, ContentDigest,
    GridRect, MaterialChannelRole, RegionId, SamplingMode,
};
use crate::ResolvedRegion;
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
    pub slots: Vec<IntermediateSlotInspection>,
    pub algorithm_versions: BTreeMap<u8, AlgorithmProvenance>,
    pub diagnostics: Vec<CompilationDiagnostic>,
    /// Profile-local overlay records produced beside the pixels and slots.
    pub regions: Vec<ResolvedRegion>,
    /// Measured persisted-spine facts.  These describe the exact artifact, never a
    /// separately reconstructed preview.
    pub telemetry: Vec<String>,
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
    if cancelled() { return Err(IntermediateAtlasError::Cancelled); }
    if current_revision() != request.revision { return Err(IntermediateAtlasError::RevisionSuperseded); }
    if !request.placement_plan.validation.complete_assignment
        || !request.placement_plan.validation.required_slots_present
        || !request.placement_plan.validation.registered_mapping_only
    {
        return Err(IntermediateAtlasError::IncompletePlacementPlan);
    }
    let width = request.topology.output_size.width;
    let height = request.topology.output_size.height;
    let pixels = u64::from(width).checked_mul(u64::from(height))
        .and_then(|n| usize::try_from(n).ok()).ok_or(IntermediateAtlasError::InvalidTopology)?;
    if width == 0 || height == 0 { return Err(IntermediateAtlasError::InvalidTopology); }

    let placement_plan_id = ContentDigest::sha256(&request.placement_plan.deterministic_bytes()
        .map_err(|error| IntermediateAtlasError::Identity(error.to_string()))?);
    let required_keys = request.slots.iter().filter(|slot| slot.required)
        .map(|slot| slot.slot_key).collect::<BTreeSet<_>>();
    for topology_slot in &request.topology.slots {
        if !request.slots.iter().any(|slot| slot.slot_key == topology_slot.slot_key) {
            return Err(IntermediateAtlasError::MissingRequiredSlot(topology_slot.slot_key.clone()));
        }
    }
    if required_keys.len() != request.slots.iter().filter(|slot| slot.required).count() {
        return Err(IntermediateAtlasError::IncompletePlacementPlan);
    }

    let roles = request.slots.iter().filter(|slot| slot.required)
        .map(|slot| slot.result.channels.iter().map(PreparedExemplarChannel::role).collect::<BTreeSet<_>>())
        .reduce(|left, right| left.intersection(&right).copied().collect()).unwrap_or_default();
    if !roles.contains(&MaterialChannelRole::BaseColor) {
        return Err(IntermediateAtlasError::MissingRequiredSlot("base_color".into()));
    }
    let mut channel_pixels = roles.iter().map(|role| (*role, vec![0_u8; pixels * 4]))
        .collect::<BTreeMap<_, _>>();
    let mut correspondence = vec![[f32::NAN; 2]; pixels];
    let mut validity = vec![0_u8; pixels];
    let mut inspections = Vec::with_capacity(request.slots.len());

    for input in &request.slots {
        if cancelled() { return Err(IntermediateAtlasError::Cancelled); }
        if current_revision() != request.revision { return Err(IntermediateAtlasError::RevisionSuperseded); }
        let topology_slot = request.topology.slots.iter().find(|slot| slot.slot_key == input.slot_key)
            .ok_or_else(|| IntermediateAtlasError::SlotArtifactMismatch(input.slot_key.into()))?;
        let allocation = topology_slot.allocation;
        if request.placement_plan.placements.iter().find(|plan| plan.slot_id == input.plan.slot_id) != Some(input.plan)
            || input.plan.slot_id != input.region_id
            || input.result.width != allocation.width || input.result.height != allocation.height
            || allocation.x.checked_add(allocation.width).is_none_or(|end| end > width)
            || allocation.y.checked_add(allocation.height).is_none_or(|end| end > height)
            || input.result.diagnostics.executed_mode != input.plan.candidate.mapping_mode
            || input.plan.candidate.domain_id != input.domain.cache_key
        {
            return Err(IntermediateAtlasError::SlotArtifactMismatch(input.slot_key.into()));
        }
        let plan_id = ContentDigest::sha256(&serde_json::to_vec(input.plan)
            .map_err(|error| IntermediateAtlasError::Identity(error.to_string()))?);
        let result_id = slot_result_id(input.result, &plan_id);
        let mut valid_count = 0_u64;
        for y in 0..allocation.height {
            for x in 0..allocation.width {
                let atlas = usize::try_from(u64::from(allocation.y + y) * u64::from(width) + u64::from(allocation.x + x))
                    .map_err(|_| IntermediateAtlasError::InvalidTopology)?;
                correspondence[atlas] = *input.result.correspondence.pixel(x, y);
                let is_valid = input.result.validity.pixel(x, y).0 >= 0.5;
                validity[atlas] = u8::from(is_valid) * 255;
                valid_count += u64::from(is_valid);
                if is_valid {
                    for role in &roles {
                        let channel = input.result.channels.iter().find(|channel| channel.role() == *role)
                            .ok_or_else(|| IntermediateAtlasError::SlotArtifactMismatch(input.slot_key.into()))?;
                        write_rgba(channel_pixels.get_mut(role).expect("role buffer exists"), atlas, channel, x, y);
                    }
                }
            }
        }
        inspections.push(IntermediateSlotInspection {
            region_id: input.region_id, slot_key: input.slot_key.into(), display_name: input.display_name.into(), allocation,
            hotspot: topology_slot.hotspot, mapping_mode: input.plan.candidate.mapping_mode,
            source_transform: input.plan.candidate.transform, isotropic_scale: input.plan.candidate.isotropic_scale,
            sampling_scale: input.plan.sampling_policy.scale,
            valid_pixel_count: valid_count, source_id: input.plan.candidate.source_id.clone(),
            patch_id: input.patch_id.clone(), domain_id: input.domain.cache_key.clone(),
            candidate_id: input.plan.candidate.candidate_id.clone(), sampling_plan_id: plan_id,
            stage_14_result_id: result_id, source_crop: input.plan.candidate.crop, grid_rect: input.grid_rect,
        });
    }
    if cancelled() { return Err(IntermediateAtlasError::Cancelled); }
    if current_revision() != request.revision { return Err(IntermediateAtlasError::RevisionSuperseded); }

    let all_importable = [MaterialChannelRole::Normal, MaterialChannelRole::Height,
        MaterialChannelRole::Roughness, MaterialChannelRole::Metallic,
        MaterialChannelRole::AmbientOcclusion, MaterialChannelRole::Specular,
        MaterialChannelRole::Opacity, MaterialChannelRole::EdgeMask, MaterialChannelRole::MaterialId];
    Ok(IntermediateAtlasArtifact {
        label: INTERMEDIATE_ATLAS_LABEL, non_exportable: true,
        incomplete_after_stage: INCOMPLETE_AFTER_STAGE, revision: request.revision,
        topology: request.topology.clone(), placement_plan_id,
        channels: channel_pixels.into_iter().map(|(role, rgba8)| IntermediateAtlasChannel { role, rgba8 }).collect(),
        unavailable_channels: all_importable.into_iter().filter(|role| !roles.contains(role)).collect(),
        correspondence, validity, slots: inspections, algorithm_versions: request.algorithm_versions.clone(),
        diagnostics: request.diagnostics.clone(), regions: request.regions.clone(), telemetry: Vec::new(),
        pending: vec!["profiles", "semantic details", "effects", "final PBR composition", "finishing",
            "mips", "metadata", "export", "Blender application"],
    })
}

fn slot_result_id(result: &SynthesizedSlotMaterial, plan_id: &ContentDigest) -> ContentDigest {
    let mut bytes = plan_id.0.as_bytes().to_vec();
    bytes.extend_from_slice(format!("{}x{}|{:?}|{:?}", result.width, result.height,
        result.diagnostics, result.stage_result).as_bytes());
    for point in result.correspondence.to_row_major() { bytes.extend_from_slice(&point[0].to_le_bytes()); bytes.extend_from_slice(&point[1].to_le_bytes()); }
    for value in result.validity.to_row_major() { bytes.extend_from_slice(&value.0.to_le_bytes()); }
    for channel in &result.channels { append_channel_identity(&mut bytes, channel); }
    ContentDigest::sha256(&bytes)
}

fn append_channel_identity(bytes: &mut Vec<u8>, channel: &PreparedExemplarChannel) {
    bytes.extend_from_slice(format!("{:?}|{:?}|", channel.role(), channel.dimensions()).as_bytes());
    match channel {
        PreparedExemplarChannel::BaseColor { plane, alpha_mode } => {
            bytes.extend_from_slice(format!("{alpha_mode:?}|").as_bytes());
            for value in plane.to_row_major() { for component in value.rgb { bytes.extend_from_slice(&component.to_le_bytes()); }
                bytes.extend_from_slice(&value.alpha.to_le_bytes()); }
        }
        PreparedExemplarChannel::Scalar { plane, .. } => for value in plane.to_row_major() { bytes.extend_from_slice(&value.0.to_le_bytes()); },
        PreparedExemplarChannel::Normal { plane, source_convention, canonical_convention, alpha_policy } => {
            bytes.extend_from_slice(format!("{source_convention:?}|{canonical_convention:?}|{alpha_policy:?}|").as_bytes());
            for value in plane.to_row_major() { for component in value.xyz { bytes.extend_from_slice(&component.to_le_bytes()); }
                bytes.extend_from_slice(&value.alpha.to_le_bytes()); }
        }
        PreparedExemplarChannel::MaterialId { plane } => for value in plane.to_row_major() { bytes.extend_from_slice(&value.0.to_le_bytes()); },
        PreparedExemplarChannel::Mask { plane, .. } => for value in plane.to_row_major() { bytes.extend_from_slice(&value.0.to_le_bytes()); },
    }
}

fn write_rgba(target: &mut [u8], atlas_pixel: usize, channel: &PreparedExemplarChannel, x: u32, y: u32) {
    let at = atlas_pixel * 4;
    let rgba = match channel {
        PreparedExemplarChannel::BaseColor { plane, .. } => {
            let value = plane.pixel(x, y);
            [linear_to_srgb(value.rgb[0]), linear_to_srgb(value.rgb[1]), linear_to_srgb(value.rgb[2]), unit(value.alpha)]
        }
        PreparedExemplarChannel::Scalar { plane, .. } => { let value = unit(plane.pixel(x, y).0); [value, value, value, 255] }
        PreparedExemplarChannel::Normal { plane, .. } => { let value = plane.pixel(x, y); [signed(value.xyz[0]), signed(value.xyz[1]), signed(value.xyz[2]), unit(value.alpha)] }
        PreparedExemplarChannel::MaterialId { plane } => { let value = plane.pixel(x, y).0.to_le_bytes(); [value[0], value[1], value[2], 255] }
        PreparedExemplarChannel::Mask { plane, .. } => { let value = unit(plane.pixel(x, y).0); [value, value, value, 255] }
    };
    target[at..at + 4].copy_from_slice(&rgba);
}

fn unit(value: f32) -> u8 { (value.clamp(0.0, 1.0) * 255.0).round() as u8 }
fn signed(value: f32) -> u8 { unit(value.mul_add(0.5, 0.5)) }
fn linear_to_srgb(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    unit(if value <= 0.003_130_8 { 12.92 * value } else { 1.055 * value.powf(1.0 / 2.4) - 0.055 })
}
