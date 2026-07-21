use hot_trimmer_domain::{
    ContentDigest, EdgeDetailIntentV1, EdgeEligibility, ManualRegionRole, MaterialMapKind, RegionId,
    StructuralProfile, TemplateSlotRole, EDGE_DETAIL_INTENT_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const EDGE_DETAIL_ALGORITHM_ID: &str = "hot_trimmer.edge_detail_mvp";
pub const EDGE_DETAIL_ALGORITHM_VERSION: &str = "1.0.0";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeDetailRoleEvaluator {
    RectangularPanel,
    HorizontalStrip,
    VerticalStrip,
    RadialOuter,
    RadialInnerOuter,
    TrimCap,
    Unique,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeDetailSourceModulationRoute {
    None,
    RegisteredHeight,
    HighPassedLinearLuminance,
}

#[derive(Clone, Debug)]
pub struct EdgeDetailRegionInput {
    pub region_id: RegionId,
    pub role: TemplateSlotRole,
    pub manual_role: ManualRegionRole,
    pub structural_profile: StructuralProfile,
    pub slot_size_m: [f64; 2],
    pub destination_pixels: [u32; 2],
    pub edge_eligibility: EdgeEligibility,
    pub stage15_plan_identity: ContentDigest,
    pub source_height_identity: Option<ContentDigest>,
    pub source_luminance_identity: Option<ContentDigest>,
}

#[derive(Clone, Debug)]
pub struct EdgeDetailCompileRequest<'a> {
    pub intent: &'a EdgeDetailIntentV1,
    pub regions: &'a [EdgeDetailRegionInput],
    pub requested_maps: &'a [MaterialMapKind],
    pub resolution_profile: &'a str,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledEdgeDetailCommand {
    pub schema_version: u16,
    pub region_id: RegionId,
    pub role: TemplateSlotRole,
    pub manual_role: ManualRegionRole,
    pub structural_profile: StructuralProfile,
    pub slot_size_m: [f64; 2],
    pub meters_per_pixel: [f64; 2],
    pub edge_eligibility: EdgeEligibility,
    pub evaluator: EdgeDetailRoleEvaluator,
    pub source_modulation_route: EdgeDetailSourceModulationRoute,
    pub source_modulation_identity: Option<ContentDigest>,
    pub requested_physical_extent_m: f64,
    pub seed: u32,
    pub intent_identity: ContentDigest,
    pub stage15_plan_identity: ContentDigest,
    pub requested_maps: Vec<MaterialMapKind>,
    pub resolution_profile: String,
    pub wear_amount: f32,
    pub intensity: f32,
    pub edge_width_m: f32,
    pub bevel_radius_m: f32,
    pub edge_softness: f32,
    pub breakup_amount: f32,
    pub breakup_scale_m: f32,
    pub micro_detail_amount: f32,
    pub micro_detail_scale_m: f32,
    pub source_height_influence: f32,
    pub source_luminance_influence: f32,
    pub height_amplitude_m: f32,
    pub normal_detail_strength: f32,
    pub hue_shift_degrees: f32,
    pub saturation_multiplier: f32,
    pub value_multiplier: f32,
    pub roughness_offset: f32,
    pub exposed_metal_enabled: bool,
    pub metallic_offset: f32,
    pub algorithm_id: String,
    pub algorithm_version: String,
    pub cache_identity: ContentDigest,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledEdgeDetailPlan {
    pub schema_version: u16,
    pub commands: Vec<CompiledEdgeDetailCommand>,
    pub plan_identity: ContentDigest,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum EdgeDetailCompileError {
    #[error("unknown Edge Detail schema version {0}")]
    UnknownSchemaVersion(u16),
    #[error("Edge Detail intent contains a non-finite value")]
    NonFiniteValue,
    #[error("Edge Detail intent contains an out-of-range value")]
    OutOfRange,
    #[error("Edge Detail physical width or scale is invalid")]
    InvalidPhysicalScale,
    #[error("Edge Detail Metallic requires exposedMetalEnabled")]
    MetallicRequiresExposedMetal,
    #[error("Edge Detail target region does not exist")]
    UnknownTargetRegion,
    #[error("Edge Detail bevel does not fit region {0}")]
    BevelDoesNotFit(RegionId),
    #[error("Edge Detail is below the supported physical pixel LOD in region {0}")]
    BelowPhysicalLod(RegionId),
    #[error("Edge Detail source modulation has no legal source in region {0}")]
    MissingSourceModulation(RegionId),
    #[error("Edge Detail identity serialization failed")]
    IdentitySerialization,
}

pub fn compile_edge_detail_plan(
    request: &EdgeDetailCompileRequest<'_>,
) -> Result<CompiledEdgeDetailPlan, EdgeDetailCompileError> {
    validate_intent(request.intent)?;
    let intent_identity = digest(request.intent)?;
    if !request.intent.enabled {
        return Ok(CompiledEdgeDetailPlan {
            schema_version: EDGE_DETAIL_INTENT_SCHEMA_VERSION,
            commands: Vec::new(),
            plan_identity: digest(&(EDGE_DETAIL_ALGORITHM_VERSION, &intent_identity, Vec::<RegionId>::new()))?,
        });
    }
    if request.intent.target_region.is_some_and(|target| {
        !request.regions.iter().any(|region| region.region_id == target)
    }) {
        return Err(EdgeDetailCompileError::UnknownTargetRegion);
    }
    let mut commands = Vec::new();
    for region in request.regions.iter().filter(|region| {
        request.intent.target_region.is_none_or(|target| target == region.region_id)
            && any_eligible(region.edge_eligibility)
    }) {
        let mpp = [
            region.slot_size_m[0] / f64::from(region.destination_pixels[0]),
            region.slot_size_m[1] / f64::from(region.destination_pixels[1]),
        ];
        if region.slot_size_m.into_iter().any(|value| !value.is_finite() || value <= 0.0)
            || region.destination_pixels.contains(&0)
            || mpp.into_iter().any(|value| !value.is_finite() || value <= 0.0)
        {
            return Err(EdgeDetailCompileError::InvalidPhysicalScale);
        }
        if request.intent.bevel_radius_m > region.slot_size_m[0].min(region.slot_size_m[1]) * 0.5 {
            return Err(EdgeDetailCompileError::BevelDoesNotFit(region.region_id));
        }
        let maximum_meters_per_pixel = mpp[0].max(mpp[1]);
        if request.intent.edge_width_m < maximum_meters_per_pixel
            || request.intent.breakup_scale_m < maximum_meters_per_pixel * 2.0
            || request.intent.micro_detail_scale_m < maximum_meters_per_pixel * 2.0
        {
            return Err(EdgeDetailCompileError::BelowPhysicalLod(region.region_id));
        }
        let evaluator = role_evaluator(region.role, region.manual_role, region.structural_profile);
        let (source_modulation_route, source_modulation_identity) = source_route(request.intent, region)?;
        let requested_physical_extent_m = (request.intent.edge_width_m
            * (1.0 + request.intent.breakup_amount * 0.75))
            .max(request.intent.bevel_radius_m);
        let mut command = CompiledEdgeDetailCommand {
            schema_version: EDGE_DETAIL_INTENT_SCHEMA_VERSION,
            region_id: region.region_id,
            role: region.role,
            manual_role: region.manual_role,
            structural_profile: region.structural_profile,
            slot_size_m: region.slot_size_m,
            meters_per_pixel: mpp,
            edge_eligibility: region.edge_eligibility,
            evaluator,
            source_modulation_route,
            source_modulation_identity,
            requested_physical_extent_m,
            seed: request.intent.seed,
            intent_identity: intent_identity.clone(),
            stage15_plan_identity: region.stage15_plan_identity.clone(),
            requested_maps: request.requested_maps.to_vec(),
            resolution_profile: request.resolution_profile.to_owned(),
            wear_amount: request.intent.wear_amount as f32,
            intensity: request.intent.intensity as f32,
            edge_width_m: request.intent.edge_width_m as f32,
            bevel_radius_m: request.intent.bevel_radius_m as f32,
            edge_softness: request.intent.edge_softness as f32,
            breakup_amount: request.intent.breakup_amount as f32,
            breakup_scale_m: request.intent.breakup_scale_m as f32,
            micro_detail_amount: request.intent.micro_detail_amount as f32,
            micro_detail_scale_m: request.intent.micro_detail_scale_m as f32,
            source_height_influence: request.intent.source_height_influence as f32,
            source_luminance_influence: request.intent.source_luminance_influence as f32,
            height_amplitude_m: request.intent.height_amplitude_m as f32,
            normal_detail_strength: request.intent.normal_detail_strength as f32,
            hue_shift_degrees: request.intent.hue_shift_degrees as f32,
            saturation_multiplier: request.intent.saturation_multiplier as f32,
            value_multiplier: request.intent.value_multiplier as f32,
            roughness_offset: request.intent.roughness_offset as f32,
            exposed_metal_enabled: request.intent.exposed_metal_enabled,
            metallic_offset: request.intent.metallic_offset as f32,
            algorithm_id: EDGE_DETAIL_ALGORITHM_ID.into(),
            algorithm_version: EDGE_DETAIL_ALGORITHM_VERSION.into(),
            cache_identity: ContentDigest(String::new()),
        };
        command.cache_identity = digest(&command)?;
        commands.push(command);
    }
    let plan_identity = digest(&(EDGE_DETAIL_ALGORITHM_VERSION, &intent_identity, &commands))?;
    Ok(CompiledEdgeDetailPlan { schema_version: 1, commands, plan_identity })
}

fn validate_intent(intent: &EdgeDetailIntentV1) -> Result<(), EdgeDetailCompileError> {
    if intent.schema_version != EDGE_DETAIL_INTENT_SCHEMA_VERSION {
        return Err(EdgeDetailCompileError::UnknownSchemaVersion(intent.schema_version));
    }
    let values = [intent.wear_amount, intent.intensity, intent.edge_width_m,
        intent.bevel_radius_m, intent.edge_softness, intent.breakup_amount,
        intent.breakup_scale_m, intent.micro_detail_amount, intent.micro_detail_scale_m,
        intent.source_height_influence, intent.source_luminance_influence,
        intent.height_amplitude_m, intent.normal_detail_strength, intent.hue_shift_degrees,
        intent.saturation_multiplier, intent.value_multiplier, intent.roughness_offset,
        intent.metallic_offset];
    if values.into_iter().any(|value| !value.is_finite()) {
        return Err(EdgeDetailCompileError::NonFiniteValue);
    }
    if !(0.0..=1.0).contains(&intent.wear_amount)
        || !(0.0..=1.0).contains(&intent.intensity)
        || !(0.0..=1.0).contains(&intent.edge_softness)
        || !(0.0..=1.0).contains(&intent.breakup_amount)
        || !(0.0..=1.0).contains(&intent.micro_detail_amount)
        || !(0.0..=1.0).contains(&intent.source_height_influence)
        || !(0.0..=1.0).contains(&intent.source_luminance_influence)
        || !(0.0..=2.0).contains(&intent.normal_detail_strength)
        || !(-180.0..=180.0).contains(&intent.hue_shift_degrees)
        || !(0.0..=2.0).contains(&intent.saturation_multiplier)
        || !(0.0..=3.0).contains(&intent.value_multiplier)
        || !(-1.0..=1.0).contains(&intent.roughness_offset)
        || !(0.0..=1.0).contains(&intent.metallic_offset)
    {
        return Err(EdgeDetailCompileError::OutOfRange);
    }
    if intent.edge_width_m <= 0.0 || intent.bevel_radius_m < 0.0
        || intent.breakup_scale_m <= 0.0 || intent.micro_detail_scale_m <= 0.0
    {
        return Err(EdgeDetailCompileError::InvalidPhysicalScale);
    }
    if !intent.exposed_metal_enabled && intent.metallic_offset != 0.0 {
        return Err(EdgeDetailCompileError::MetallicRequiresExposedMetal);
    }
    Ok(())
}

fn source_route(
    intent: &EdgeDetailIntentV1,
    region: &EdgeDetailRegionInput,
) -> Result<(EdgeDetailSourceModulationRoute, Option<ContentDigest>), EdgeDetailCompileError> {
    if intent.source_height_influence > 0.0 {
        if let Some(identity) = &region.source_height_identity {
            return Ok((EdgeDetailSourceModulationRoute::RegisteredHeight, Some(identity.clone())));
        }
    }
    if intent.source_luminance_influence > 0.0 {
        if let Some(identity) = &region.source_luminance_identity {
            return Ok((EdgeDetailSourceModulationRoute::HighPassedLinearLuminance, Some(identity.clone())));
        }
    }
    if intent.source_height_influence > 0.0 || intent.source_luminance_influence > 0.0 {
        return Err(EdgeDetailCompileError::MissingSourceModulation(region.region_id));
    }
    Ok((EdgeDetailSourceModulationRoute::None, None))
}

const fn role_evaluator(role: TemplateSlotRole, manual_role: ManualRegionRole, profile: StructuralProfile) -> EdgeDetailRoleEvaluator {
    match role {
        TemplateSlotRole::Planar => EdgeDetailRoleEvaluator::RectangularPanel,
        TemplateSlotRole::RepeatingStrip => match manual_role {
            ManualRegionRole::VerticalStrip => EdgeDetailRoleEvaluator::VerticalStrip,
            _ => EdgeDetailRoleEvaluator::HorizontalStrip,
        },
        TemplateSlotRole::UniqueDetail => EdgeDetailRoleEvaluator::Unique,
        TemplateSlotRole::TrimCap => EdgeDetailRoleEvaluator::TrimCap,
        TemplateSlotRole::Radial => match profile {
            StructuralProfile::Annulus => EdgeDetailRoleEvaluator::RadialInnerOuter,
            _ => EdgeDetailRoleEvaluator::RadialOuter,
        },
    }
}

const fn any_eligible(edges: EdgeEligibility) -> bool {
    edges.left || edges.right || edges.top || edges.bottom
}

fn digest<T: Serialize>(value: &T) -> Result<ContentDigest, EdgeDetailCompileError> {
    serde_json::to_vec(value)
        .map(|bytes| ContentDigest::sha256(&bytes))
        .map_err(|_| EdgeDetailCompileError::IdentitySerialization)
}
