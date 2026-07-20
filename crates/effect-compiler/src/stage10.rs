use std::collections::BTreeMap;

use hot_trimmer_domain::{
    AlgorithmProvenance, CanonicalRect, CompilationDiagnostic, ContentDigest, DiagnosticCode,
    PhysicalScaleEvidence, QuarterTurn, RecoveryChoice, RegionDefinition, RegionId,
    RegionOrientation, SamplingMode, ScaleProvenance, StageResult, StructuralProfile,
    TemplateSlotRole, WorldScaleAvailability,
};
use hot_trimmer_placement_solver::{SlotDemandView, SourceFootprintKind};
use serde::{Deserialize, Serialize};

pub const STAGE_10_ALGORITHM_ID: &str = "hot-trimmer.stage-10.slot-capacity";
pub const STAGE_10_ALGORITHM_VERSION: &str = "1.0.0";

/// Legal effect spaces. Normalized rectangle coordinates are intentionally absent because
/// topology fractions are not physical dimensions.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectScaleSpace {
    World,
    SlotMinorRelative,
    SlotMajorRelative,
    SlotAreaRelative,
    Pixels,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotAxis {
    X,
    Y,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MirrorPolicy {
    Forbidden,
    Allowed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualImportance {
    Background,
    Standard,
    Hero,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceFootprintUnit {
    SourcePixels,
    RelativeTexels,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldDimensionSource {
    Stage9Authored,
    DestinationTexelDensity,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequiredSourceFootprint {
    pub width: f64,
    pub height: f64,
    pub unit: SourceFootprintUnit,
    pub scale_provenance: ScaleProvenance,
    pub world_scale: WorldScaleAvailability,
    pub confidence_milli: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureLod {
    FullHeightNormalColorRoughnessAo,
    SimplifiedHeightNormalRoughness,
    NormalRoughness,
    RoughnessColorIndication,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectVariant {
    Full,
    Simplified,
    Strip,
    Radial,
    Cap,
    MergedRoundedProfile,
    FullyRoundedProfile,
    NormalOnly,
    RoughnessOnly,
    Disabled,
}

/// Materialized for UI/QA consumption; clients inspect it and never recompute these limits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectCapacity {
    pub can_have_flat_center: bool,
    pub minimum_flat_center_m: f64,
    pub maximum_left_profile_width_m: f64,
    pub maximum_right_profile_width_m: f64,
    pub maximum_top_profile_width_m: f64,
    pub maximum_bottom_profile_width_m: f64,
    /// Maximum isotropic diameter.
    pub maximum_isotropic_feature_m: f64,
    /// Maximum slot-centered radius.
    pub maximum_radial_feature_m: f64,
    pub minimum_full_height_feature_m: f64,
    pub minimum_normal_only_feature_m: f64,
    pub minimum_roughness_only_feature_m: f64,
    pub recommended_supersample_factor: u8,
    pub allowed_effect_variants: Vec<EffectVariant>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapacityDiagnosticCode {
    RelativeSourceFootprint,
    OpposingProfilesDoNotFit,
    FlatCenterImpossible,
    IsotropicFeatureDoesNotFit,
    FeatureBelowRasterThreshold,
    SupersampleLimitReached,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Insufficient,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapacityDiagnostic {
    pub code: CapacityDiagnosticCode,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub measurements: BTreeMap<String, f64>,
    pub recovery_choices: Vec<RecoveryChoice>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalFeatureIntent {
    pub value: f64,
    pub scale_space: EffectScaleSpace,
}

/// The destination rectangle is Stage 9's already-compiled output-pixel allocation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotDemandIntent {
    pub destination_rect: CanonicalRect,
    pub desired_texel_density: f64,
    pub world_dimension_source: WorldDimensionSource,
    pub source_scale: PhysicalScaleEvidence,
    pub visual_importance: VisualImportance,
    pub minimum_survivable_feature_m: f64,
    pub minimum_flat_center_m: f64,
    pub requested_features: Vec<PhysicalFeatureIntent>,
    pub opposing_profile_widths_m: Option<[f64; 2]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedSlotDemand {
    pub slot_id: RegionId,
    pub slot_role: TemplateSlotRole,
    pub orientation: RegionOrientation,
    pub hotspot_rect: CanonicalRect,
    pub allocation_rect: CanonicalRect,
    pub destination_pixel_width: u32,
    pub destination_pixel_height: u32,
    pub world_width_m: f64,
    pub world_height_m: f64,
    pub world_dimension_source: WorldDimensionSource,
    pub major_axis: SlotAxis,
    pub minor_axis: SlotAxis,
    pub major_axis_m: f64,
    pub minor_axis_m: f64,
    pub aspect_ratio: f64,
    pub pixels_per_meter_x: f64,
    pub pixels_per_meter_y: f64,
    pub meters_per_pixel_x: f64,
    pub meters_per_pixel_y: f64,
    pub desired_texel_density: f64,
    pub required_source_footprint: RequiredSourceFootprint,
    pub mapping_mode: SamplingMode,
    pub allowed_mapping_modes: Vec<SamplingMode>,
    pub allowed_rotations: Vec<QuarterTurn>,
    pub mirror_policy: MirrorPolicy,
    pub material_group: String,
    pub variation_group: String,
    pub profile_type: StructuralProfile,
    pub weathering_class: String,
    pub visual_importance: VisualImportance,
    pub minimum_survivable_feature_m: f64,
    pub maximum_bevel_width_m: f64,
    pub maximum_isotropic_feature_m: f64,
    pub required_supersampling: u8,
    pub supported_feature_lods: Vec<FeatureLod>,
    pub effect_capacity: EffectCapacity,
    pub diagnostics: Vec<CapacityDiagnostic>,
}

impl SlotDemandView for ResolvedSlotDemand {
    fn slot_id(&self) -> RegionId {
        self.slot_id
    }
    fn role(&self) -> TemplateSlotRole {
        self.slot_role
    }
    fn orientation(&self) -> RegionOrientation {
        self.orientation
    }
    fn destination_pixels(&self) -> (u32, u32) {
        (self.destination_pixel_width, self.destination_pixel_height)
    }
    fn required_source_footprint(&self) -> (f64, f64, SourceFootprintKind) {
        (
            self.required_source_footprint.width,
            self.required_source_footprint.height,
            match self.required_source_footprint.unit {
                SourceFootprintUnit::SourcePixels => SourceFootprintKind::SourcePixels,
                SourceFootprintUnit::RelativeTexels => SourceFootprintKind::RelativeTexels,
            },
        )
    }
    fn allowed_mapping_modes(&self) -> &[SamplingMode] {
        &self.allowed_mapping_modes
    }
    fn allowed_rotations(&self) -> &[QuarterTurn] {
        &self.allowed_rotations
    }
    fn mirror_allowed(&self) -> bool {
        self.mirror_policy == MirrorPolicy::Allowed
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedSlotDemandSet {
    pub stage_result: StageResult,
    pub slots: Vec<ResolvedSlotDemand>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlotDemandError {
    EmptyDestination,
    InvalidWorldDimensions,
    InvalidIntent,
    InvalidSourceScale,
    Cancelled,
}

pub fn resolve_slot_demands(
    inputs: &[(&RegionDefinition, SlotDemandIntent)],
) -> Result<ResolvedSlotDemandSet, SlotDemandError> {
    resolve_slot_demands_with_guard(inputs, &|| false)
}

pub fn resolve_slot_demands_with_guard(
    inputs: &[(&RegionDefinition, SlotDemandIntent)],
    cancelled: &dyn Fn() -> bool,
) -> Result<ResolvedSlotDemandSet, SlotDemandError> {
    let slots = inputs
        .iter()
        .map(|(region, intent)| {
            if cancelled() {
                Err(SlotDemandError::Cancelled)
            } else {
                resolve_slot_demand(region, intent)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    if cancelled() {
        return Err(SlotDemandError::Cancelled);
    }
    let diagnostics = slots
        .iter()
        .flat_map(|slot| &slot.diagnostics)
        .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Insufficient)
        .map(compilation_diagnostic)
        .collect();
    Ok(ResolvedSlotDemandSet {
        stage_result: StageResult::Executed {
            algorithm: AlgorithmProvenance {
                algorithm_id: STAGE_10_ALGORITHM_ID.into(),
                version: STAGE_10_ALGORITHM_VERSION.into(),
            },
            settings_hash: ContentDigest::sha256(format!("{inputs:?}").as_bytes()),
            diagnostics,
        },
        slots,
    })
}

pub fn resolve_slot_demand(
    region: &RegionDefinition,
    intent: &SlotDemandIntent,
) -> Result<ResolvedSlotDemand, SlotDemandError> {
    validate(region, intent)?;
    let pixel_width = intent.destination_rect.width;
    let pixel_height = intent.destination_rect.height;
    let (world_width, world_height) = match intent.world_dimension_source {
        WorldDimensionSource::Stage9Authored => (
            region.uv_fit.world_size_meters[0],
            region.uv_fit.world_size_meters[1],
        ),
        WorldDimensionSource::DestinationTexelDensity => (
            f64::from(pixel_width) / intent.desired_texel_density,
            f64::from(pixel_height) / intent.desired_texel_density,
        ),
    };
    let ppm_x = f64::from(pixel_width) / world_width;
    let ppm_y = f64::from(pixel_height) / world_height;
    let (major_axis, minor_axis, major_axis_m, minor_axis_m) = if world_width >= world_height {
        (SlotAxis::X, SlotAxis::Y, world_width, world_height)
    } else {
        (SlotAxis::Y, SlotAxis::X, world_height, world_width)
    };
    let required_source_footprint = source_footprint(
        world_width,
        world_height,
        pixel_width,
        pixel_height,
        intent.source_scale,
    );
    let minimum_density = ppm_x.min(ppm_y);
    let resolved_features = intent
        .requested_features
        .iter()
        .map(|feature| resolve_feature_scale(*feature, world_width, world_height, ppm_x, ppm_y))
        .collect::<Vec<_>>();
    let required_supersampling = supersample_factor(
        resolved_features
            .iter()
            .map(|feature| feature.raster_width_px)
            .chain(std::iter::once(
                intent.minimum_survivable_feature_m * minimum_density,
            )),
    );
    let effect_capacity = capacity(
        world_width,
        world_height,
        minor_axis_m,
        minimum_density,
        intent.minimum_flat_center_m,
        required_supersampling,
        region.role,
    );
    let mut diagnostics = Vec::new();
    if required_source_footprint.unit == SourceFootprintUnit::RelativeTexels {
        diagnostics.push(CapacityDiagnostic {
            code: CapacityDiagnosticCode::RelativeSourceFootprint,
            severity: DiagnosticSeverity::Info,
            message: "source world scale is unavailable; footprint is relative and makes no meter-to-source-pixel claim".into(),
            measurements: BTreeMap::from([
                ("relative_width".into(), required_source_footprint.width),
                ("relative_height".into(), required_source_footprint.height),
            ]), recovery_choices: Vec::new(),
        });
    }
    diagnose_physical_fit(
        intent,
        &resolved_features,
        &effect_capacity,
        minor_axis_m,
        &mut diagnostics,
    );
    diagnose_raster_fit(&resolved_features, required_supersampling, &mut diagnostics);
    let (mapping_mode, allowed_mapping_modes) = mapping_modes(region.role, region.orientation);
    let supported_feature_lods =
        supported_lods(effect_capacity.maximum_isotropic_feature_m * minimum_density);

    Ok(ResolvedSlotDemand {
        slot_id: region.id,
        slot_role: region.role,
        orientation: region.orientation,
        hotspot_rect: region.hotspot_rect,
        allocation_rect: region.allocation_rect,
        destination_pixel_width: pixel_width,
        destination_pixel_height: pixel_height,
        world_width_m: world_width,
        world_height_m: world_height,
        world_dimension_source: intent.world_dimension_source,
        major_axis,
        minor_axis,
        major_axis_m,
        minor_axis_m,
        aspect_ratio: world_width / world_height,
        pixels_per_meter_x: ppm_x,
        pixels_per_meter_y: ppm_y,
        meters_per_pixel_x: world_width / f64::from(pixel_width),
        meters_per_pixel_y: world_height / f64::from(pixel_height),
        desired_texel_density: intent.desired_texel_density,
        required_source_footprint,
        mapping_mode,
        allowed_mapping_modes,
        allowed_rotations: region.uv_fit.allowed_rotations.clone(),
        mirror_policy: if region.uv_fit.mirror_allowed {
            MirrorPolicy::Allowed
        } else {
            MirrorPolicy::Forbidden
        },
        material_group: region.material_group.clone(),
        variation_group: region.weathering_group.clone(),
        profile_type: region.structural_profile,
        weathering_class: region.weathering_group.clone(),
        visual_importance: intent.visual_importance,
        minimum_survivable_feature_m: intent.minimum_survivable_feature_m,
        maximum_bevel_width_m: (minor_axis_m - intent.minimum_flat_center_m).max(0.0) / 2.0,
        maximum_isotropic_feature_m: effect_capacity.maximum_isotropic_feature_m,
        required_supersampling,
        supported_feature_lods,
        effect_capacity,
        diagnostics,
    })
}

/// Converts legal procedural spaces into slot-local meters. Pixel-space operations remain
/// anisotropic raster operations; physical World features remain isotropic by construction.
#[must_use]
pub fn effect_scale_to_slot_meters(
    space: EffectScaleSpace,
    value: f64,
    slot: &ResolvedSlotDemand,
) -> [f64; 2] {
    match space {
        EffectScaleSpace::World => [value, value],
        EffectScaleSpace::SlotMinorRelative => [value * slot.minor_axis_m; 2],
        EffectScaleSpace::SlotMajorRelative => [value * slot.major_axis_m; 2],
        EffectScaleSpace::SlotAreaRelative => {
            let extent = (slot.world_width_m * slot.world_height_m).sqrt();
            [value * extent; 2]
        }
        EffectScaleSpace::Pixels => [
            value * slot.meters_per_pixel_x,
            value * slot.meters_per_pixel_y,
        ],
    }
}

#[must_use]
pub fn opposing_profiles_fit(minor_axis_m: f64, first_m: f64, second_m: f64, flat_m: f64) -> bool {
    minor_axis_m - first_m - second_m >= flat_m
}

#[derive(Clone, Copy, Debug)]
struct ResolvedFeatureScale {
    physical_width_m: Option<f64>,
    raster_width_px: f64,
}

fn resolve_feature_scale(
    feature: PhysicalFeatureIntent,
    world_width: f64,
    world_height: f64,
    ppm_x: f64,
    ppm_y: f64,
) -> ResolvedFeatureScale {
    let minor_m = world_width.min(world_height);
    let major_m = world_width.max(world_height);
    let minimum_density = ppm_x.min(ppm_y);
    let physical_width_m = match feature.scale_space {
        EffectScaleSpace::World => Some(feature.value),
        EffectScaleSpace::SlotMinorRelative => Some(feature.value * minor_m),
        EffectScaleSpace::SlotMajorRelative => Some(feature.value * major_m),
        EffectScaleSpace::SlotAreaRelative => {
            Some(feature.value * (world_width * world_height).sqrt())
        }
        EffectScaleSpace::Pixels => None,
    };
    ResolvedFeatureScale {
        physical_width_m,
        raster_width_px: physical_width_m.map_or(feature.value, |meters| meters * minimum_density),
    }
}

fn validate(region: &RegionDefinition, intent: &SlotDemandIntent) -> Result<(), SlotDemandError> {
    if intent.destination_rect.width == 0 || intent.destination_rect.height == 0 {
        return Err(SlotDemandError::EmptyDestination);
    }
    let [world_width, world_height] = region.uv_fit.world_size_meters;
    if !world_width.is_finite()
        || !world_height.is_finite()
        || world_width <= 0.0
        || world_height <= 0.0
    {
        return Err(SlotDemandError::InvalidWorldDimensions);
    }
    if !intent.desired_texel_density.is_finite()
        || intent.desired_texel_density <= 0.0
        || !intent.minimum_survivable_feature_m.is_finite()
        || intent.minimum_survivable_feature_m <= 0.0
        || !intent.minimum_flat_center_m.is_finite()
        || intent.minimum_flat_center_m < 0.0
        || intent
            .requested_features
            .iter()
            .any(|f| !f.value.is_finite() || f.value <= 0.0)
        || intent
            .opposing_profile_widths_m
            .is_some_and(|v| v.iter().any(|x| !x.is_finite() || *x < 0.0))
    {
        return Err(SlotDemandError::InvalidIntent);
    }
    let scale = intent.source_scale;
    if scale.source_pixels_per_meter_x_milli == Some(0)
        || scale.source_pixels_per_meter_y_milli == Some(0)
    {
        return Err(SlotDemandError::InvalidSourceScale);
    }
    if matches!(scale.world_scale, WorldScaleAvailability::Available)
        && !scale.claims_world_accuracy()
    {
        return Err(SlotDemandError::InvalidSourceScale);
    }
    Ok(())
}

fn source_footprint(
    world_width: f64,
    world_height: f64,
    pixel_width: u32,
    pixel_height: u32,
    scale: PhysicalScaleEvidence,
) -> RequiredSourceFootprint {
    if scale.claims_world_accuracy() {
        let ppm_x = scale
            .source_pixels_per_meter_x_milli
            .expect("validated x scale") as f64
            / 1_000.0;
        let ppm_y = scale
            .source_pixels_per_meter_y_milli
            .expect("validated y scale") as f64
            / 1_000.0;
        RequiredSourceFootprint {
            width: world_width * ppm_x,
            height: world_height * ppm_y,
            unit: SourceFootprintUnit::SourcePixels,
            scale_provenance: scale.provenance,
            world_scale: scale.world_scale,
            confidence_milli: scale.confidence_milli,
        }
    } else {
        let area = f64::from(pixel_width) * f64::from(pixel_height);
        let aspect = f64::from(pixel_width) / f64::from(pixel_height);
        RequiredSourceFootprint {
            width: (area * aspect).sqrt(),
            height: (area / aspect).sqrt(),
            unit: SourceFootprintUnit::RelativeTexels,
            scale_provenance: scale.provenance,
            world_scale: scale.world_scale,
            confidence_milli: scale.confidence_milli,
        }
    }
}

fn capacity(
    width_m: f64,
    height_m: f64,
    minor_m: f64,
    density: f64,
    flat_m: f64,
    supersample: u8,
    role: TemplateSlotRole,
) -> EffectCapacity {
    let horizontal = (width_m - flat_m).max(0.0) / 2.0;
    let vertical = (height_m - flat_m).max(0.0) / 2.0;
    let isotropic = width_m.min(height_m);
    let mut variants = vec![
        EffectVariant::Full,
        EffectVariant::Simplified,
        EffectVariant::NormalOnly,
        EffectVariant::RoughnessOnly,
        EffectVariant::Disabled,
    ];
    variants.push(match role {
        TemplateSlotRole::RepeatingStrip => EffectVariant::Strip,
        TemplateSlotRole::TrimCap => EffectVariant::Cap,
        TemplateSlotRole::Radial => EffectVariant::Radial,
        TemplateSlotRole::Planar | TemplateSlotRole::UniqueDetail => EffectVariant::Full,
    });
    variants.push(if minor_m < flat_m {
        EffectVariant::FullyRoundedProfile
    } else {
        EffectVariant::MergedRoundedProfile
    });
    variants.sort();
    variants.dedup();
    EffectCapacity {
        can_have_flat_center: minor_m >= flat_m,
        minimum_flat_center_m: flat_m,
        maximum_left_profile_width_m: horizontal,
        maximum_right_profile_width_m: horizontal,
        maximum_top_profile_width_m: vertical,
        maximum_bottom_profile_width_m: vertical,
        maximum_isotropic_feature_m: isotropic,
        maximum_radial_feature_m: isotropic / 2.0,
        minimum_full_height_feature_m: 6.0 / density,
        minimum_normal_only_feature_m: 1.5 / density,
        minimum_roughness_only_feature_m: 0.5 / density,
        recommended_supersample_factor: supersample,
        allowed_effect_variants: variants,
    }
}

fn diagnose_physical_fit(
    intent: &SlotDemandIntent,
    features: &[ResolvedFeatureScale],
    capacity: &EffectCapacity,
    minor_m: f64,
    diagnostics: &mut Vec<CapacityDiagnostic>,
) {
    if !capacity.can_have_flat_center {
        diagnostics.push(CapacityDiagnostic {
            code: CapacityDiagnosticCode::FlatCenterImpossible,
            severity: DiagnosticSeverity::Insufficient,
            message: "slot minor axis is smaller than the required flat center".into(),
            measurements: BTreeMap::from([
                ("minor_axis_m".into(), minor_m),
                (
                    "minimum_flat_center_m".into(),
                    capacity.minimum_flat_center_m,
                ),
            ]),
            recovery_choices: vec![
                RecoveryChoice::AdjustSettings,
                RecoveryChoice::DisableEffect,
            ],
        });
    }
    if let Some([first, second]) = intent.opposing_profile_widths_m {
        if !opposing_profiles_fit(minor_m, first, second, capacity.minimum_flat_center_m) {
            diagnostics.push(CapacityDiagnostic {
                code: CapacityDiagnosticCode::OpposingProfilesDoNotFit,
                severity: DiagnosticSeverity::Insufficient,
                message: "opposing profiles consume the required flat center; use a merged, rounded, normal-only, or disabled variant".into(),
                measurements: BTreeMap::from([("minor_axis_m".into(), minor_m),
                    ("first_profile_m".into(), first), ("second_profile_m".into(), second),
                    ("minimum_flat_center_m".into(), capacity.minimum_flat_center_m)]),
                recovery_choices: vec![RecoveryChoice::AdjustSettings, RecoveryChoice::DisableEffect],
            });
        }
    }
    for feature in features {
        if feature
            .physical_width_m
            .is_some_and(|width| width > capacity.maximum_isotropic_feature_m)
        {
            let physical_width_m = feature.physical_width_m.expect("checked physical width");
            diagnostics.push(CapacityDiagnostic {
                code: CapacityDiagnosticCode::IsotropicFeatureDoesNotFit,
                severity: DiagnosticSeverity::Insufficient,
                message: "world-space isotropic feature exceeds the slot minor dimension".into(),
                measurements: BTreeMap::from([
                    ("feature_width_m".into(), physical_width_m),
                    (
                        "maximum_isotropic_feature_m".into(),
                        capacity.maximum_isotropic_feature_m,
                    ),
                ]),
                recovery_choices: vec![
                    RecoveryChoice::AdjustSettings,
                    RecoveryChoice::DisableEffect,
                ],
            });
        }
    }
}

fn diagnose_raster_fit(
    features: &[ResolvedFeatureScale],
    supersample: u8,
    diagnostics: &mut Vec<CapacityDiagnostic>,
) {
    if let Some(pixels) = features
        .iter()
        .map(|feature| feature.raster_width_px)
        .reduce(f64::min)
    {
        if pixels < 1.5 {
            diagnostics.push(CapacityDiagnostic {
                code: CapacityDiagnosticCode::FeatureBelowRasterThreshold,
                severity: DiagnosticSeverity::Warning,
                message: "the narrowest feature needs a reduced LOD at this output resolution"
                    .into(),
                measurements: BTreeMap::from([("feature_pixels".into(), pixels)]),
                recovery_choices: vec![
                    RecoveryChoice::IncreaseOutputResolution,
                    RecoveryChoice::DisableEffect,
                ],
            });
        }
        if supersample == 8 && pixels * 8.0 < 4.0 {
            diagnostics.push(CapacityDiagnostic {
                code: CapacityDiagnosticCode::SupersampleLimitReached,
                severity: DiagnosticSeverity::Warning,
                message: "8x still provides fewer than four internal samples; physical intent is unchanged".into(),
                measurements: BTreeMap::from([("internal_samples".into(), pixels * 8.0)]),
                recovery_choices: vec![RecoveryChoice::IncreaseOutputResolution, RecoveryChoice::DisableEffect],
            });
        }
    }
}

fn supersample_factor(widths_px: impl Iterator<Item = f64>) -> u8 {
    let pixels = widths_px.reduce(f64::min).unwrap_or(4.0);
    let needed = 4.0 / pixels;
    if needed <= 1.0 {
        1
    } else if needed <= 2.0 {
        2
    } else if needed <= 4.0 {
        4
    } else {
        8
    }
}

fn supported_lods(maximum_pixels: f64) -> Vec<FeatureLod> {
    let mut lods = vec![FeatureLod::RoughnessColorIndication];
    if maximum_pixels >= 1.5 {
        lods.push(FeatureLod::NormalRoughness);
    }
    if maximum_pixels >= 3.0 {
        lods.push(FeatureLod::SimplifiedHeightNormalRoughness);
    }
    if maximum_pixels >= 6.0 {
        lods.push(FeatureLod::FullHeightNormalColorRoughnessAo);
    }
    lods
}

fn mapping_modes(
    role: TemplateSlotRole,
    orientation: RegionOrientation,
) -> (SamplingMode, Vec<SamplingMode>) {
    let modes = match role {
        TemplateSlotRole::Planar => vec![
            SamplingMode::DirectCrop,
            SamplingMode::PeriodicTile,
            SamplingMode::TextureSynthesis,
            SamplingMode::NineSlicePanel,
        ],
        TemplateSlotRole::RepeatingStrip => vec![
            if orientation == RegionOrientation::Vertical {
                SamplingMode::RepeatY
            } else {
                SamplingMode::RepeatX
            },
            SamplingMode::DirectCrop,
            SamplingMode::TextureSynthesis,
        ],
        TemplateSlotRole::UniqueDetail => {
            vec![SamplingMode::UniqueContain, SamplingMode::UniqueCover]
        }
        TemplateSlotRole::TrimCap => vec![SamplingMode::ThreeSliceCap, SamplingMode::DirectCrop],
        TemplateSlotRole::Radial => vec![SamplingMode::PlanarRadial, SamplingMode::PolarRadial],
    };
    (modes[0], modes)
}

fn compilation_diagnostic(diagnostic: &CapacityDiagnostic) -> CompilationDiagnostic {
    CompilationDiagnostic {
        code: DiagnosticCode::InsufficientInput,
        stage: Some(10),
        message: diagnostic.message.clone(),
        context: diagnostic
            .measurements
            .iter()
            .map(|(k, v)| (k.clone(), v.to_string()))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hot_trimmer_domain::{FitAxis, IdColor, RadialParameters, UvFitKind, UvFitPolicy};

    fn region(
        role: TemplateSlotRole,
        orientation: RegionOrientation,
        world: [f64; 2],
    ) -> RegionDefinition {
        RegionDefinition {
            id: RegionId::from_bytes([7; 16]),
            display_name: "fixture".into(),
            id_color: IdColor([1, 2, 3]),
            allocation_rect: CanonicalRect {
                x: 0,
                y: 0,
                width: 2048,
                height: 2048,
            },
            hotspot_rect: CanonicalRect {
                x: 16,
                y: 16,
                width: 2016,
                height: 2016,
            },
            role,
            orientation,
            uv_fit: UvFitPolicy {
                kind: match role {
                    TemplateSlotRole::Planar => UvFitKind::Rectangular,
                    TemplateSlotRole::RepeatingStrip => UvFitKind::Strip,
                    TemplateSlotRole::UniqueDetail => UvFitKind::Unique,
                    TemplateSlotRole::TrimCap => UvFitKind::Cap,
                    TemplateSlotRole::Radial => UvFitKind::Radial,
                },
                fit_axis: FitAxis::Automatic,
                keep_proportion: true,
                allowed_rotations: vec![QuarterTurn::Zero],
                mirror_allowed: role != TemplateSlotRole::Radial,
                world_size_meters: world,
                classification_tags: Vec::new(),
            },
            structural_profile: match role {
                TemplateSlotRole::Radial => StructuralProfile::RadialDisc,
                TemplateSlotRole::TrimCap => StructuralProfile::RoundedBevel,
                _ => StructuralProfile::Flat,
            },
            material_group: "material-a".into(),
            weathering_group: "weather-a".into(),
            radial_parameters: (role == TemplateSlotRole::Radial).then_some(RadialParameters {
                center_x: 0.5,
                center_y: 0.5,
                inner_radius: 0.0,
                outer_radius: 0.5,
            }),
            enabled: true,
            grid_rect: None,
        }
    }

    fn intent(pixels: [u32; 2]) -> SlotDemandIntent {
        SlotDemandIntent {
            destination_rect: CanonicalRect {
                x: 0,
                y: 0,
                width: pixels[0],
                height: pixels[1],
            },
            desired_texel_density: 512.0,
            world_dimension_source: WorldDimensionSource::Stage9Authored,
            source_scale: PhysicalScaleEvidence::default(),
            visual_importance: VisualImportance::Standard,
            minimum_survivable_feature_m: 0.01,
            minimum_flat_center_m: 0.04,
            requested_features: vec![PhysicalFeatureIntent {
                value: 0.01,
                scale_space: EffectScaleSpace::World,
            }],
            opposing_profile_widths_m: None,
        }
    }

    fn close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1.0e-9, "{actual} != {expected}");
    }

    #[test]
    fn algorithm_stage_10_slot_capacity() {
        let broad_region = region(
            TemplateSlotRole::Planar,
            RegionOrientation::Horizontal,
            [4.0, 2.0],
        );
        let broad_intent = intent([2048, 1024]);
        let broad = resolve_slot_demand(&broad_region, &broad_intent).unwrap();
        assert_eq!(
            (
                broad.destination_pixel_width,
                broad.destination_pixel_height
            ),
            (2048, 1024)
        );
        close(broad.pixels_per_meter_x, 512.0);
        close(broad.pixels_per_meter_y, 512.0);
        close(broad.effect_capacity.maximum_isotropic_feature_m, 2.0);
        close(broad.effect_capacity.maximum_radial_feature_m, 1.0);
        assert_eq!(
            broad.required_source_footprint.unit,
            SourceFootprintUnit::RelativeTexels
        );
        close(broad.required_source_footprint.width, 2048.0);
        assert_eq!(broad.major_axis, SlotAxis::X);
        assert!(
            broad
                .diagnostics
                .iter()
                .any(|d| d.code == CapacityDiagnosticCode::RelativeSourceFootprint)
        );

        let horizontal = resolve_slot_demand(
            &region(
                TemplateSlotRole::RepeatingStrip,
                RegionOrientation::Horizontal,
                [10.0, 0.5],
            ),
            &intent([320, 16]),
        )
        .unwrap();
        let vertical = resolve_slot_demand(
            &region(
                TemplateSlotRole::RepeatingStrip,
                RegionOrientation::Vertical,
                [0.5, 10.0],
            ),
            &intent([16, 320]),
        )
        .unwrap();
        close(horizontal.minor_axis_m, 0.5);
        close(vertical.minor_axis_m, 0.5);
        close(
            horizontal.effect_capacity.maximum_isotropic_feature_m,
            vertical.effect_capacity.maximum_isotropic_feature_m,
        );
        assert_eq!(horizontal.mapping_mode, SamplingMode::RepeatX);
        assert_eq!(vertical.mapping_mode, SamplingMode::RepeatY);

        let radial = resolve_slot_demand(
            &region(
                TemplateSlotRole::Radial,
                RegionOrientation::Unspecified,
                [1.0, 1.0],
            ),
            &intent([256, 256]),
        )
        .unwrap();
        close(radial.effect_capacity.maximum_radial_feature_m, 0.5);
        assert_eq!(radial.mirror_policy, MirrorPolicy::Forbidden);
        assert!(
            radial
                .allowed_mapping_modes
                .contains(&SamplingMode::PolarRadial)
        );

        let cap = resolve_slot_demand(
            &region(
                TemplateSlotRole::TrimCap,
                RegionOrientation::Horizontal,
                [0.8, 0.4],
            ),
            &intent([128, 64]),
        )
        .unwrap();
        assert_eq!(cap.mapping_mode, SamplingMode::ThreeSliceCap);
        assert!(
            cap.effect_capacity
                .allowed_effect_variants
                .contains(&EffectVariant::Cap)
        );

        let mut subpixel_intent = intent([128, 8]);
        subpixel_intent.requested_features[0].value = 0.000_1;
        subpixel_intent.minimum_survivable_feature_m = 0.000_1;
        let subpixel = resolve_slot_demand(
            &region(
                TemplateSlotRole::RepeatingStrip,
                RegionOrientation::Horizontal,
                [4.0, 0.25],
            ),
            &subpixel_intent,
        )
        .unwrap();
        assert_eq!(subpixel.required_supersampling, 8);
        assert_eq!(subpixel.minimum_survivable_feature_m, 0.000_1);
        assert!(
            subpixel
                .diagnostics
                .iter()
                .any(|d| d.code == CapacityDiagnosticCode::FeatureBelowRasterThreshold)
        );

        let mut opposing_intent = intent([320, 8]);
        opposing_intent.minimum_flat_center_m = 0.06;
        opposing_intent.opposing_profile_widths_m = Some([0.03, 0.03]);
        let opposing = resolve_slot_demand(
            &region(
                TemplateSlotRole::RepeatingStrip,
                RegionOrientation::Horizontal,
                [10.0, 0.1],
            ),
            &opposing_intent,
        )
        .unwrap();
        assert!(!opposing_profiles_fit(0.1, 0.03, 0.03, 0.06));
        close(opposing.maximum_bevel_width_m, 0.02);
        assert!(
            opposing
                .diagnostics
                .iter()
                .any(|d| d.code == CapacityDiagnosticCode::OpposingProfilesDoNotFit)
        );

        let mut known_intent = intent([2048, 1024]);
        known_intent.source_scale = PhysicalScaleEvidence {
            source_pixels_per_meter_x_milli: Some(800_000),
            source_pixels_per_meter_y_milli: Some(600_000),
            provenance: ScaleProvenance::UserMeasured,
            confidence_milli: 1000,
            world_scale: WorldScaleAvailability::Available,
        };
        let known = resolve_slot_demand(&broad_region, &known_intent).unwrap();
        assert_eq!(
            known.required_source_footprint.unit,
            SourceFootprintUnit::SourcePixels
        );
        close(known.required_source_footprint.width, 3200.0);
        close(known.required_source_footprint.height, 1200.0);

        let mut density_only_intent = known_intent.clone();
        density_only_intent.world_dimension_source = WorldDimensionSource::DestinationTexelDensity;
        let density_only = resolve_slot_demand(
            &region(
                TemplateSlotRole::Planar,
                RegionOrientation::Horizontal,
                [8.0, 4.0],
            ),
            &density_only_intent,
        )
        .unwrap();
        close(density_only.world_width_m, 4.0);
        close(density_only.world_height_m, 2.0);
        close(density_only.required_source_footprint.width, 3200.0);
        assert_eq!(
            density_only.world_dimension_source,
            WorldDimensionSource::DestinationTexelDensity
        );

        let strip_region = region(
            TemplateSlotRole::RepeatingStrip,
            RegionOrientation::Horizontal,
            [10.0, 0.5],
        );
        let mut pixel_feature = intent([320, 16]);
        pixel_feature.minimum_survivable_feature_m = 0.2;
        pixel_feature.requested_features = vec![PhysicalFeatureIntent {
            value: 4.0,
            scale_space: EffectScaleSpace::Pixels,
        }];
        let pixel_feature = resolve_slot_demand(&strip_region, &pixel_feature).unwrap();
        assert_eq!(pixel_feature.required_supersampling, 1);

        let mut relative_feature = intent([320, 16]);
        relative_feature.minimum_survivable_feature_m = 0.2;
        relative_feature.requested_features = vec![PhysicalFeatureIntent {
            value: 0.1,
            scale_space: EffectScaleSpace::SlotMinorRelative,
        }];
        let relative_feature = resolve_slot_demand(&strip_region, &relative_feature).unwrap();
        assert_eq!(relative_feature.required_supersampling, 4);
        close(
            effect_scale_to_slot_meters(
                EffectScaleSpace::SlotMinorRelative,
                0.1,
                &relative_feature,
            )[0],
            0.05,
        );

        let relative_set = resolve_slot_demands(&[(&broad_region, broad_intent.clone())]).unwrap();
        match relative_set.stage_result {
            StageResult::Executed { diagnostics, .. } => assert!(diagnostics.is_empty()),
            _ => panic!("Stage 10 must execute"),
        }
        let opposing_set = resolve_slot_demands(&[(
            &region(
                TemplateSlotRole::RepeatingStrip,
                RegionOrientation::Horizontal,
                [10.0, 0.1],
            ),
            opposing_intent,
        )])
        .unwrap();
        match opposing_set.stage_result {
            StageResult::Executed { diagnostics, .. } => {
                assert_eq!(diagnostics.len(), 1);
                assert_eq!(diagnostics[0].code, DiagnosticCode::InsufficientInput);
            }
            _ => panic!("Stage 10 must execute with insufficiency evidence"),
        }

        let high = resolve_slot_demand(&broad_region, &intent([4096, 2048])).unwrap();
        close(high.world_width_m, broad.world_width_m);
        close(
            high.minimum_survivable_feature_m,
            broad.minimum_survivable_feature_m,
        );
        assert_eq!(high.allocation_rect, broad.allocation_rect);
        assert!(high.pixels_per_meter_x > broad.pixels_per_meter_x);
        close(
            effect_scale_to_slot_meters(EffectScaleSpace::World, 0.125, &horizontal)[0],
            0.125,
        );
        let circle_px = [
            0.1 * horizontal.pixels_per_meter_x,
            0.1 * horizontal.pixels_per_meter_y,
        ];
        close(
            circle_px[0] / horizontal.pixels_per_meter_x,
            circle_px[1] / horizontal.pixels_per_meter_y,
        );
    }
}
