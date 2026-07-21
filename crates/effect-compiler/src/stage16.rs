use hot_trimmer_domain::{ContentDigest, MaterialChannelRole, StageResult, TemplateSlotRole};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{EffectCapacity, EffectScaleSpace, EffectVariant};

pub const STAGE_16_DETAIL_ALGORITHM_ID: &str = "hot_trimmer.compiled_semantic_details";
pub const STAGE_16_DETAIL_ALGORITHM_VERSION: &str = "1.0.0";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetailFamily {
    RepeatingStrip,
    UniqueDetail,
    RadialDetail,
    TrimCap,
    BoltGroup,
    Vent,
    PanelStamp,
    Groove,
    Decal,
    ProceduralMotif,
    UserPatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetailOrientation {
    Slot,
    Horizontal,
    Vertical,
    Radial,
    ExplicitDegrees,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetailFitPolicy {
    Contain,
    Cover,
    Repeat,
    FailIfOversized,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetailMappingMode {
    Planar,
    PolarAuthored,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetailLod {
    Full,
    SimplifiedHeightNormal,
    NormalOnly,
    RoughnessColor,
    Disabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetailFallback {
    None,
    VariantSelected,
    NormalOnly,
    RoughnessColor,
    Disabled,
    Incompatible,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StampScope {
    MaterialReusableAtlas,
    AssetSpecificDeferred,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StampBlendPolicy {
    Replace,
    Add,
    Multiply,
    Max,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OccupancyRelation {
    AboveProfile,
    BelowProfile,
    AvoidRaised,
    OnlyFlatCenter,
    Ignore,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StampAssetRef {
    pub asset_id: String,
    pub version: String,
    pub digest: ContentDigest,
    pub kind: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailChannelContribution {
    pub channel: MaterialChannelRole,
    pub amount: f64,
    pub blend: StampBlendPolicy,
    pub material_id: Option<u32>,
    pub metallic_explicit: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailDefinition {
    pub name: String,
    pub family: DetailFamily,
    pub physical_size: [f64; 2],
    pub scale_space: EffectScaleSpace,
    pub compatible_roles: Vec<TemplateSlotRole>,
    pub orientation: DetailOrientation,
    pub explicit_rotation_degrees: f64,
    pub aspect_limits: [f64; 2],
    pub minimum_pixels: [u32; 2],
    pub repeat_period_m: Option<[f64; 2]>,
    pub fit_policy: DetailFitPolicy,
    pub mapping_mode: DetailMappingMode,
    pub channels: Vec<DetailChannelContribution>,
    pub fallback: DetailFallback,
    pub provenance: String,
    pub seed: u64,
    pub required_sources: Vec<StampAssetRef>,
    pub required_halo_px: u32,
    pub dependencies: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StampOperation {
    pub asset: StampAssetRef,
    pub scope: StampScope,
    pub target_region: String,
    pub physical_position_m: [f64; 2],
    pub physical_size_m: [f64; 2],
    pub pivot: [f64; 2],
    pub rotation_degrees: f64,
    pub mirror: [bool; 2],
    pub opacity: f64,
    pub blend: StampBlendPolicy,
    pub clipping: DetailFitPolicy,
    pub seed: u64,
    pub spacing_m: [f64; 2],
    pub scatter: f64,
    pub jitter_m: [f64; 2],
    pub layer_order: i32,
    pub occupancy: OccupancyRelation,
    pub channels: Vec<DetailChannelContribution>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StampStroke {
    pub operation: StampOperation,
    pub physical_samples_m: Vec<[f64; 2]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledDetail {
    pub definition: DetailDefinition,
    pub resolved_family: DetailFamily,
    pub physical_size_m: [f64; 2],
    pub repeat_period_m: Option<[f64; 2]>,
    pub slot_size_m: [f64; 2],
    pub pixels_per_meter: [f64; 2],
    pub lod: DetailLod,
    pub fallback: DetailFallback,
    pub fallback_reason: Option<String>,
    pub required_halo_px: u32,
    pub reusable_atlas_operations: Vec<StampOperation>,
    pub asset_specific_deferred_operations: Vec<StampOperation>,
    pub strokes: Vec<StampStroke>,
    pub diagnostics: Vec<String>,
    pub algorithm_id: String,
    pub algorithm_version: String,
    pub cache_identity: ContentDigest,
}

#[derive(Clone, Debug)]
pub struct DetailCompileRequest<'a> {
    pub definitions: &'a [DetailDefinition],
    pub operations: &'a [StampOperation],
    pub strokes: &'a [StampStroke],
    pub slot_role: TemplateSlotRole,
    pub slot_size_m: [f64; 2],
    pub destination_pixels: [u32; 2],
    pub capacity: &'a EffectCapacity,
    pub upstream_identity: &'a ContentDigest,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledDetailSet {
    pub stage_result: StageResult,
    pub details: Vec<CompiledDetail>,
    pub route_qa: Vec<String>,
    pub occupancy_qa: Vec<String>,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum DetailCompileError {
    #[error("detail dimensions, channels, source assets, or operation parameters are malformed")]
    InvalidRequest,
    #[error("detail is incompatible with the requested slot and fallback policy")]
    Incompatible,
    #[error("stamp operation or stroke references no compiled detail definition")]
    OrphanOperation,
}

pub fn compile_details(
    request: DetailCompileRequest<'_>,
) -> Result<CompiledDetailSet, DetailCompileError> {
    if request.definitions.is_empty() && request.operations.is_empty() && request.strokes.is_empty()
    {
        return Ok(CompiledDetailSet {
            stage_result: StageResult::SkippedBecauseUnused {
                reason: "empty detail list".into(),
            },
            details: Vec::new(),
            route_qa: vec!["stage16-detail-route: empty detail list".into()],
            occupancy_qa: Vec::new(),
        });
    }
    if request
        .slot_size_m
        .iter()
        .any(|v| !v.is_finite() || *v <= 0.0)
        || request.destination_pixels.contains(&0)
        || !valid_capacity(request.capacity)
    {
        return Err(DetailCompileError::InvalidRequest);
    }
    let pixels_per_meter = [
        f64::from(request.destination_pixels[0]) / request.slot_size_m[0],
        f64::from(request.destination_pixels[1]) / request.slot_size_m[1],
    ];
    for operation in request.operations {
        validate_operation(operation)?;
        if !request
            .definitions
            .iter()
            .any(|definition| definition.name == operation.target_region)
        {
            return Err(DetailCompileError::OrphanOperation);
        }
    }
    for stroke in request.strokes {
        validate_operation(&stroke.operation)?;
        if stroke
            .physical_samples_m
            .iter()
            .any(|sample| sample.iter().any(|value| !value.is_finite()))
            || !request
                .definitions
                .iter()
                .any(|definition| definition.name == stroke.operation.target_region)
        {
            return Err(DetailCompileError::OrphanOperation);
        }
    }
    let mut details = Vec::new();
    for definition in request.definitions {
        validate_definition(definition)?;
        let mut physical_size = resolve_size(definition, request.slot_size_m, pixels_per_meter);
        let mut fallback = DetailFallback::None;
        let mut fallback_reason = None;
        if !definition.compatible_roles.contains(&request.slot_role) {
            match definition.fallback {
                DetailFallback::Disabled => {
                    fallback = DetailFallback::Disabled;
                    fallback_reason =
                        Some("detail role is incompatible; disabled fallback selected".into());
                }
                DetailFallback::NormalOnly => {
                    fallback = DetailFallback::NormalOnly;
                    fallback_reason =
                        Some("detail role is incompatible; normal-only fallback selected".into());
                }
                DetailFallback::RoughnessColor => {
                    fallback = DetailFallback::RoughnessColor;
                    fallback_reason = Some(
                        "detail role is incompatible; roughness/color fallback selected".into(),
                    );
                }
                _ => return Err(DetailCompileError::Incompatible),
            }
        }
        let aspect = physical_size[0] / physical_size[1].max(f64::EPSILON);
        if aspect < definition.aspect_limits[0] || aspect > definition.aspect_limits[1] {
            if definition.fallback == DetailFallback::Disabled {
                fallback = DetailFallback::Disabled;
                fallback_reason = Some("detail aspect violates declared limits".into());
            } else {
                return Err(DetailCompileError::Incompatible);
            }
        }
        if physical_size[0] > request.slot_size_m[0] || physical_size[1] > request.slot_size_m[1] {
            if matches!(
                definition.fit_policy,
                DetailFitPolicy::Contain | DetailFitPolicy::Cover
            ) {
                return Err(DetailCompileError::Incompatible);
            }
            if definition.fallback == DetailFallback::Disabled {
                fallback = DetailFallback::Disabled;
                fallback_reason = Some("detail exceeds slot physical dimensions".into());
            } else {
                return Err(DetailCompileError::Incompatible);
            }
        }
        if matches!(fallback, DetailFallback::Disabled) {
            physical_size = [0.0, 0.0];
        }
        let lod = lod_for(
            physical_size,
            definition.minimum_pixels,
            fallback,
            request.capacity,
            pixels_per_meter,
        );
        let mut reusable_atlas_operations = Vec::new();
        let mut asset_specific_deferred_operations = Vec::new();
        for operation in request
            .operations
            .iter()
            .filter(|operation| operation.target_region == definition.name)
        {
            validate_operation(operation)?;
            match operation.scope {
                StampScope::MaterialReusableAtlas => {
                    reusable_atlas_operations.push(operation.clone())
                }
                StampScope::AssetSpecificDeferred => {
                    asset_specific_deferred_operations.push(operation.clone());
                }
            }
        }
        let strokes = request
            .strokes
            .iter()
            .filter(|stroke| stroke.operation.target_region == definition.name)
            .cloned()
            .collect::<Vec<_>>();
        let required_halo_px =
            definition.required_halo_px.max(
                ((physical_size[0].max(physical_size[1])
                    * pixels_per_meter[0].min(pixels_per_meter[1]))
                .ceil() as u32)
                    .saturating_add(1),
            );
        let mut compiled = CompiledDetail {
            definition: definition.clone(),
            resolved_family: definition.family,
            physical_size_m: physical_size,
            repeat_period_m: definition.repeat_period_m,
            slot_size_m: request.slot_size_m,
            pixels_per_meter,
            lod,
            fallback,
            fallback_reason,
            required_halo_px,
            reusable_atlas_operations,
            asset_specific_deferred_operations,
            strokes,
            diagnostics: Vec::new(),
            algorithm_id: STAGE_16_DETAIL_ALGORITHM_ID.into(),
            algorithm_version: STAGE_16_DETAIL_ALGORITHM_VERSION.into(),
            cache_identity: ContentDigest(String::new()),
        };
        if let Some(reason) = &compiled.fallback_reason {
            compiled.diagnostics.push(reason.clone());
        }
        compiled.diagnostics.push(format!(
            "resolved semantic detail: family={:?} size={:.9}x{:.9}m lod={:?}",
            compiled.resolved_family,
            compiled.physical_size_m[0],
            compiled.physical_size_m[1],
            compiled.lod
        ));
        let payload = serde_json::to_vec(&(
            request.upstream_identity,
            STAGE_16_DETAIL_ALGORITHM_VERSION,
            &compiled,
        ))
        .map_err(|_| DetailCompileError::InvalidRequest)?;
        compiled.cache_identity = ContentDigest::sha256(&payload);
        details.push(compiled);
    }
    Ok(CompiledDetailSet {
        stage_result: StageResult::Executed {
            algorithm: hot_trimmer_domain::AlgorithmProvenance {
                algorithm_id: STAGE_16_DETAIL_ALGORITHM_ID.into(),
                version: STAGE_16_DETAIL_ALGORITHM_VERSION.into(),
            },
            settings_hash: ContentDigest::sha256(
                &serde_json::to_vec(&details).map_err(|_| DetailCompileError::InvalidRequest)?,
            ),
            diagnostics: Vec::new(),
        },
        details: details.clone(),
        route_qa: details
            .iter()
            .map(|detail| {
                format!(
                    "stage16-detail-route: {}->{:?}",
                    detail.definition.name, detail.lod
                )
            })
            .collect(),
        occupancy_qa: details
            .iter()
            .map(|detail| {
                format!(
                    "stage16-detail-occupancy: {} relation={:?}",
                    detail.definition.name,
                    detail
                        .reusable_atlas_operations
                        .first()
                        .map(|op| op.occupancy)
                        .unwrap_or(OccupancyRelation::Ignore)
                )
            })
            .collect(),
    })
}

pub fn empty_compiled_detail_set() -> CompiledDetailSet {
    CompiledDetailSet {
        stage_result: StageResult::SkippedBecauseUnused {
            reason: "empty detail list".into(),
        },
        details: Vec::new(),
        route_qa: Vec::new(),
        occupancy_qa: Vec::new(),
    }
}

fn resolve_size(
    definition: &DetailDefinition,
    slot_size: [f64; 2],
    pixels_per_meter: [f64; 2],
) -> [f64; 2] {
    match definition.scale_space {
        EffectScaleSpace::World => definition.physical_size,
        EffectScaleSpace::Pixels => [
            definition.physical_size[0] / pixels_per_meter[0],
            definition.physical_size[1] / pixels_per_meter[1],
        ],
        EffectScaleSpace::SlotMinorRelative => {
            let minor = slot_size[0].min(slot_size[1]);
            [
                definition.physical_size[0] * minor,
                definition.physical_size[1] * minor,
            ]
        }
        EffectScaleSpace::SlotMajorRelative => {
            let major = slot_size[0].max(slot_size[1]);
            [
                definition.physical_size[0] * major,
                definition.physical_size[1] * major,
            ]
        }
        EffectScaleSpace::SlotAreaRelative => {
            let scale = (slot_size[0] * slot_size[1]).sqrt();
            [
                definition.physical_size[0] * scale,
                definition.physical_size[1] * scale,
            ]
        }
    }
}

fn lod_for(
    physical_size: [f64; 2],
    minimum_pixels: [u32; 2],
    fallback: DetailFallback,
    capacity: &EffectCapacity,
    pixels_per_meter: [f64; 2],
) -> DetailLod {
    let feature_m = physical_size[0].min(physical_size[1]);
    let raster_pixels = [
        physical_size[0] * pixels_per_meter[0],
        physical_size[1] * pixels_per_meter[1],
    ];
    if fallback == DetailFallback::Disabled || feature_m <= 0.0 {
        DetailLod::Disabled
    } else if fallback == DetailFallback::NormalOnly {
        DetailLod::NormalOnly
    } else if fallback == DetailFallback::RoughnessColor {
        DetailLod::RoughnessColor
    } else if raster_pixels[0] < f64::from(minimum_pixels[0])
        || raster_pixels[1] < f64::from(minimum_pixels[1])
    {
        DetailLod::Disabled
    } else if feature_m >= capacity.minimum_full_height_feature_m
        && capacity
            .allowed_effect_variants
            .contains(&EffectVariant::Full)
    {
        DetailLod::Full
    } else if feature_m >= capacity.minimum_normal_only_feature_m
        && capacity
            .allowed_effect_variants
            .contains(&EffectVariant::Simplified)
    {
        DetailLod::SimplifiedHeightNormal
    } else if feature_m >= capacity.minimum_normal_only_feature_m
        && capacity
            .allowed_effect_variants
            .contains(&EffectVariant::NormalOnly)
    {
        DetailLod::NormalOnly
    } else if feature_m >= capacity.minimum_roughness_only_feature_m {
        DetailLod::RoughnessColor
    } else {
        DetailLod::Disabled
    }
}

fn validate_definition(definition: &DetailDefinition) -> Result<(), DetailCompileError> {
    if definition.name.is_empty()
        || definition
            .physical_size
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.0)
        || !definition.explicit_rotation_degrees.is_finite()
        || definition.aspect_limits[0] <= 0.0
        || definition.aspect_limits[1] < definition.aspect_limits[0]
        || definition.minimum_pixels.contains(&0)
        || definition.required_sources.iter().any(|source| {
            source.asset_id.is_empty() || source.version.is_empty() || source.digest.0.is_empty()
        })
        || definition.channels.iter().any(|channel| {
            !channel.amount.is_finite()
                || (channel.channel == MaterialChannelRole::Metallic && !channel.metallic_explicit)
        })
    {
        return Err(DetailCompileError::InvalidRequest);
    }
    Ok(())
}

fn validate_operation(operation: &StampOperation) -> Result<(), DetailCompileError> {
    if operation.asset.asset_id.is_empty()
        || operation.asset.version.is_empty()
        || operation.asset.digest.0.is_empty()
        || operation
            .physical_size_m
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.0)
        || operation.physical_position_m.iter().any(|v| !v.is_finite())
        || operation.pivot.iter().any(|v| !v.is_finite())
        || !operation.rotation_degrees.is_finite()
        || !(0.0..=1.0).contains(&operation.opacity)
        || operation
            .spacing_m
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0)
        || operation
            .jitter_m
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0)
        || !operation.scatter.is_finite()
    {
        return Err(DetailCompileError::InvalidRequest);
    }
    Ok(())
}

fn valid_capacity(capacity: &EffectCapacity) -> bool {
    [
        capacity.maximum_isotropic_feature_m,
        capacity.maximum_radial_feature_m,
        capacity.minimum_full_height_feature_m,
        capacity.minimum_normal_only_feature_m,
        capacity.minimum_roughness_only_feature_m,
    ]
    .into_iter()
    .all(|value| value.is_finite() && value >= 0.0)
}
