use hot_trimmer_domain::{ContentDigest, StructuralProfile};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{EffectCapacity, EffectVariant};

pub const STAGE_15_PROFILE_ALGORITHM_ID: &str = "hot_trimmer.compiled_structural_profile";
pub const STAGE_15_PROFILE_ALGORITHM_VERSION: &str = "1.0.0";
pub const MAX_CUSTOM_PROFILE_POINTS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "unit", content = "value")]
pub enum ProfileLength {
    Meters(f64),
    RelativeMinor(f64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileProgram {
    Flat,
    ConvexBevel,
    ConcaveGroove,
    RoundedBevel,
    DoubleBevel,
    RaisedLip,
    RecessedSeam,
    PanelFrame,
    FullyRoundedStrip,
    MergedOpposingBevel,
    RadialDisc,
    Annulus,
    CustomCurve,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileLegalityPolicy {
    Clamp,
    FullyRounded,
    MergeOpposing,
    NormalOnly,
    Disabled,
    Incompatible,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileFallback {
    None,
    Clamped,
    FullyRounded,
    MergedOpposing,
    NormalOnly,
    Disabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileLod {
    FullHeight,
    SimplifiedHeight,
    NormalOnly,
    RoughnessOnly,
    Disabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileLodPolicy {
    Auto,
    Force(ProfileLod),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileEvaluator {
    AnalyticSdf,
    AnalyticRadialSdf,
    PiecewiseLinearCurve,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileSdf {
    Rectangle,
    Disc,
    Annulus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileQaView {
    Occupancy,
    Lod,
    Fallback,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileOccupancySemantics {
    pub signed_distance: bool,
    pub inside_outside: bool,
    pub flat_center: bool,
    pub raised: bool,
    pub recessed: bool,
    pub cap: bool,
    pub groove: bool,
    pub profile_exclusion: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomProfilePoint {
    pub position: f64,
    pub height: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestedProfile {
    pub program: ProfileProgram,
    pub first_width: ProfileLength,
    pub second_width: ProfileLength,
    pub minimum_flat_center: ProfileLength,
    pub amplitude: ProfileLength,
    pub angle_degrees: f64,
    pub inner_radius: ProfileLength,
    pub outer_radius: ProfileLength,
    pub legality_policy: ProfileLegalityPolicy,
    pub lod_policy: ProfileLodPolicy,
    pub maximum_supersampling: u8,
    pub seed: u64,
    pub custom_curve: Vec<CustomProfilePoint>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledProfile {
    pub requested_program: ProfileProgram,
    pub program: ProfileProgram,
    pub sdf: ProfileSdf,
    pub first_width_m: f64,
    pub second_width_m: f64,
    pub minimum_flat_center_m: f64,
    pub amplitude_m: f64,
    pub angle_degrees: f64,
    pub inner_radius_m: f64,
    pub outer_radius_m: f64,
    pub slot_size_m: [f64; 2],
    pub pixels_per_meter: [f64; 2],
    pub lod: ProfileLod,
    pub supersampling: u8,
    pub evaluator: ProfileEvaluator,
    pub fallback: ProfileFallback,
    pub fallback_reason: Option<String>,
    pub occupancy: ProfileOccupancySemantics,
    pub required_halo_px: u32,
    pub seed: u64,
    pub custom_curve: Vec<CustomProfilePoint>,
    pub compact_resource_references: Vec<ContentDigest>,
    pub algorithm_id: String,
    pub algorithm_version: String,
    pub diagnostics: Vec<String>,
    pub cache_identity: ContentDigest,
}

#[derive(Clone, Debug)]
pub struct ProfileCompileRequest<'a> {
    pub requested: &'a RequestedProfile,
    pub slot_size_m: [f64; 2],
    pub destination_pixels: [u32; 2],
    pub capacity: &'a EffectCapacity,
    pub upstream_identity: &'a ContentDigest,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ProfileCompileError {
    #[error("profile dimensions, scale, or parameters are malformed")]
    InvalidRequest,
    #[error("requested opposing profiles are physically incompatible")]
    Incompatible,
}

impl RequestedProfile {
    #[must_use]
    pub fn from_structural_intent(intent: StructuralProfile, seed: u64) -> Self {
        let (program, width, amplitude, inner, outer) = match intent {
            StructuralProfile::Flat => (ProfileProgram::Flat, 0.0, 0.0, 0.0, 0.0),
            StructuralProfile::Bevel => (ProfileProgram::ConvexBevel, 0.125, 0.0625, 0.0, 0.0),
            StructuralProfile::Groove => (ProfileProgram::ConcaveGroove, 0.125, 0.0625, 0.0, 0.0),
            StructuralProfile::RoundedBevel => {
                (ProfileProgram::RoundedBevel, 0.125, 0.0625, 0.0, 0.0)
            }
            StructuralProfile::PanelFrame => (ProfileProgram::PanelFrame, 0.20, 0.04, 0.0, 0.0),
            StructuralProfile::RadialDisc => (ProfileProgram::RadialDisc, 0.06, 0.06, 0.0, 0.42),
            StructuralProfile::Annulus => (ProfileProgram::Annulus, 0.04, 0.04, 0.24, 0.44),
        };
        Self {
            program,
            first_width: ProfileLength::RelativeMinor(width),
            second_width: ProfileLength::RelativeMinor(width),
            minimum_flat_center: ProfileLength::Meters(0.001),
            amplitude: ProfileLength::RelativeMinor(amplitude),
            angle_degrees: 45.0,
            inner_radius: ProfileLength::RelativeMinor(inner),
            outer_radius: ProfileLength::RelativeMinor(outer),
            legality_policy: ProfileLegalityPolicy::Clamp,
            lod_policy: ProfileLodPolicy::Auto,
            maximum_supersampling: 8,
            seed,
            custom_curve: Vec::new(),
        }
    }
}

#[must_use]
pub fn conservative_profile_capacity(slot_size_m: [f64; 2]) -> EffectCapacity {
    let minor = slot_size_m[0].min(slot_size_m[1]);
    let minimum_flat = 0.001_f64.min(minor);
    let edge = ((minor - minimum_flat).max(0.0) * 0.5).max(0.0);
    EffectCapacity {
        can_have_flat_center: minor >= minimum_flat,
        minimum_flat_center_m: minimum_flat,
        maximum_left_profile_width_m: edge,
        maximum_right_profile_width_m: edge,
        maximum_top_profile_width_m: edge,
        maximum_bottom_profile_width_m: edge,
        maximum_isotropic_feature_m: minor,
        maximum_radial_feature_m: minor * 0.5,
        minimum_full_height_feature_m: 4.0 * minor / 4096.0,
        minimum_normal_only_feature_m: 0.5 * minor / 4096.0,
        minimum_roughness_only_feature_m: 0.25 * minor / 4096.0,
        recommended_supersample_factor: 1,
        allowed_effect_variants: vec![
            crate::EffectVariant::Full,
            crate::EffectVariant::Simplified,
            crate::EffectVariant::Strip,
            crate::EffectVariant::Radial,
            crate::EffectVariant::Cap,
            crate::EffectVariant::NormalOnly,
            crate::EffectVariant::RoughnessOnly,
            crate::EffectVariant::Disabled,
            crate::EffectVariant::FullyRoundedProfile,
            crate::EffectVariant::MergedRoundedProfile,
        ],
    }
}

pub fn compile_profile(
    request: ProfileCompileRequest<'_>,
) -> Result<CompiledProfile, ProfileCompileError> {
    let requested = request.requested;
    if request
        .slot_size_m
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.0)
        || request.destination_pixels.contains(&0)
        || !requested.angle_degrees.is_finite()
        || !(0.0..89.9).contains(&requested.angle_degrees)
        || !matches!(requested.maximum_supersampling, 1 | 2 | 4 | 8)
        || requested.custom_curve.len() > MAX_CUSTOM_PROFILE_POINTS
        || !valid_custom_curve(requested)
        || !valid_capacity(request.capacity)
    {
        return Err(ProfileCompileError::InvalidRequest);
    }
    let minor = request.slot_size_m[0].min(request.slot_size_m[1]);
    let resolve = |length| resolve_length(length, minor);
    let mut first = resolve(requested.first_width)?;
    let mut second = resolve(requested.second_width)?;
    let minimum_flat =
        resolve(requested.minimum_flat_center)?.max(request.capacity.minimum_flat_center_m);
    let mut amplitude = resolve(requested.amplitude)?;
    let inner = resolve(requested.inner_radius)?;
    let outer = resolve(requested.outer_radius)?;
    let requested_program = requested.program;
    let mut program = requested.program;
    let mut fallback = ProfileFallback::None;
    let mut fallback_reason = None;
    let mut forced_lod = None;
    if !request
        .capacity
        .allowed_effect_variants
        .contains(&required_variant(program))
    {
        let required = required_variant(program);
        let reason = format!("{program:?} requires {required:?}, which Stage 10 did not allow");
        apply_capacity_fallback(
            &mut program,
            &mut fallback,
            &mut forced_lod,
            &mut fallback_reason,
            requested.legality_policy,
            reason,
        )?;
    }
    let radial = matches!(
        program,
        ProfileProgram::RadialDisc | ProfileProgram::Annulus
    );
    if program_requires_flat_center(program) && !request.capacity.can_have_flat_center {
        apply_capacity_fallback(
            &mut program,
            &mut fallback,
            &mut forced_lod,
            &mut fallback_reason,
            requested.legality_policy,
            format!(
                "Stage 10 capacity cannot reserve the required {:.9}m flat center",
                minimum_flat
            ),
        )?;
    }
    if radial {
        let radial_limit = request
            .capacity
            .maximum_radial_feature_m
            .min(request.capacity.maximum_isotropic_feature_m * 0.5);
        if outer > radial_limit || inner > outer {
            apply_capacity_fallback(
                &mut program,
                &mut fallback,
                &mut forced_lod,
                &mut fallback_reason,
                requested.legality_policy,
                format!(
                    "radial feature inner={inner:.9}m outer={outer:.9}m exceeds Stage 10 radius limit {radial_limit:.9}m"
                ),
            )?;
        }
        if program == ProfileProgram::Annulus && outer - inner < first + second {
            let reason = format!(
                "annulus radial widths outer={first:.9}m inner={second:.9}m overlap within band {:.9}m",
                outer - inner
            );
            match requested.legality_policy {
                ProfileLegalityPolicy::Clamp => {
                    let available = (outer - inner).max(0.0);
                    let sum = first + second;
                    if available <= 0.0 || sum <= 0.0 {
                        program = ProfileProgram::Flat;
                        fallback = ProfileFallback::Disabled;
                        forced_lod = Some(ProfileLod::Disabled);
                    } else {
                        first *= available / sum;
                        second *= available / sum;
                        fallback = ProfileFallback::Clamped;
                    }
                    fallback_reason = Some(reason);
                }
                _ => apply_capacity_fallback(
                    &mut program,
                    &mut fallback,
                    &mut forced_lod,
                    &mut fallback_reason,
                    requested.legality_policy,
                    reason,
                )?,
            }
        }
    } else if program != ProfileProgram::Flat {
        let first_limit = request
            .capacity
            .maximum_left_profile_width_m
            .min(request.capacity.maximum_top_profile_width_m);
        let second_limit = request
            .capacity
            .maximum_right_profile_width_m
            .min(request.capacity.maximum_bottom_profile_width_m);
        if first > first_limit || second > second_limit {
            let reason = format!(
                "profile widths first={first:.9}m second={second:.9}m exceed Stage 10 edge limits first={first_limit:.9}m second={second_limit:.9}m"
            );
            match requested.legality_policy {
                ProfileLegalityPolicy::Clamp => {
                    first = first.min(first_limit);
                    second = second.min(second_limit);
                    fallback = ProfileFallback::Clamped;
                    fallback_reason = Some(reason);
                }
                ProfileLegalityPolicy::FullyRounded => {
                    program = ProfileProgram::FullyRoundedStrip;
                    first = minor * 0.5;
                    second = minor * 0.5;
                    fallback = ProfileFallback::FullyRounded;
                    fallback_reason = Some(reason);
                }
                ProfileLegalityPolicy::MergeOpposing => {
                    program = ProfileProgram::MergedOpposingBevel;
                    first = minor * 0.5;
                    second = minor * 0.5;
                    fallback = ProfileFallback::MergedOpposing;
                    fallback_reason = Some(reason);
                }
                _ => apply_capacity_fallback(
                    &mut program,
                    &mut fallback,
                    &mut forced_lod,
                    &mut fallback_reason,
                    requested.legality_policy,
                    reason,
                )?,
            }
        }
    }
    let opposing = program_requires_flat_center(program);
    if opposing && minor - first - second < minimum_flat {
        let reason = format!(
            "opposing widths {:.9}m + {:.9}m leave less than {:.9}m flat center in {:.9}m",
            first, second, minimum_flat, minor
        );
        match requested.legality_policy {
            ProfileLegalityPolicy::Clamp => {
                let available = (minor - minimum_flat).max(0.0);
                let sum = first + second;
                if available <= 0.0 || sum <= 0.0 {
                    program = ProfileProgram::Flat;
                    fallback = ProfileFallback::Disabled;
                    forced_lod = Some(ProfileLod::Disabled);
                } else {
                    first *= available / sum;
                    second *= available / sum;
                    fallback = ProfileFallback::Clamped;
                }
                fallback_reason = Some(reason);
            }
            ProfileLegalityPolicy::FullyRounded => {
                program = ProfileProgram::FullyRoundedStrip;
                first = minor * 0.5;
                second = minor * 0.5;
                fallback = ProfileFallback::FullyRounded;
                fallback_reason = Some(reason);
            }
            ProfileLegalityPolicy::MergeOpposing => {
                program = ProfileProgram::MergedOpposingBevel;
                first = minor * 0.5;
                second = minor * 0.5;
                fallback = ProfileFallback::MergedOpposing;
                fallback_reason = Some(reason);
            }
            ProfileLegalityPolicy::NormalOnly => {
                fallback = ProfileFallback::NormalOnly;
                forced_lod = Some(ProfileLod::NormalOnly);
                fallback_reason = Some(reason);
            }
            ProfileLegalityPolicy::Disabled => {
                program = ProfileProgram::Flat;
                fallback = ProfileFallback::Disabled;
                forced_lod = Some(ProfileLod::Disabled);
                fallback_reason = Some(reason);
            }
            ProfileLegalityPolicy::Incompatible => return Err(ProfileCompileError::Incompatible),
        }
    }
    let pixels_per_meter = [
        f64::from(request.destination_pixels[0]) / request.slot_size_m[0],
        f64::from(request.destination_pixels[1]) / request.slot_size_m[1],
    ];
    let width_pixels = first.max(second) * pixels_per_meter[0].min(pixels_per_meter[1]);
    let mut lod = forced_lod.unwrap_or_else(|| {
        capacity_lod(
            requested.lod_policy,
            program,
            first.max(second),
            request.capacity,
        )
    });
    if !lod_variant_allowed(lod, request.capacity) {
        let downgraded = highest_allowed_lod(first.max(second), request.capacity);
        if fallback == ProfileFallback::None {
            fallback = if downgraded == ProfileLod::Disabled {
                ProfileFallback::Disabled
            } else {
                ProfileFallback::NormalOnly
            };
        }
        fallback_reason = Some(format!(
            "{lod:?} is not allowed by Stage 10 capacity; using {downgraded:?}"
        ));
        lod = downgraded;
    }
    if matches!(
        lod,
        ProfileLod::NormalOnly | ProfileLod::RoughnessOnly | ProfileLod::Disabled
    ) {
        amplitude = amplitude.max(0.0);
    }
    let required = if width_pixels < 0.5 {
        8
    } else if width_pixels < 1.0 {
        4
    } else if width_pixels < 2.0 {
        2
    } else {
        1
    };
    let recommended = request
        .capacity
        .recommended_supersample_factor
        .next_power_of_two()
        .clamp(1, 8);
    let supersampling = required
        .max(u32::from(recommended))
        .min(u32::from(requested.maximum_supersampling)) as u8;
    let sdf = match program {
        ProfileProgram::RadialDisc => ProfileSdf::Disc,
        ProfileProgram::Annulus => ProfileSdf::Annulus,
        _ => ProfileSdf::Rectangle,
    };
    let evaluator = match program {
        ProfileProgram::CustomCurve => ProfileEvaluator::PiecewiseLinearCurve,
        ProfileProgram::RadialDisc | ProfileProgram::Annulus => ProfileEvaluator::AnalyticRadialSdf,
        _ => ProfileEvaluator::AnalyticSdf,
    };
    let raised = matches!(
        program,
        ProfileProgram::ConvexBevel
            | ProfileProgram::RoundedBevel
            | ProfileProgram::DoubleBevel
            | ProfileProgram::RaisedLip
            | ProfileProgram::PanelFrame
            | ProfileProgram::FullyRoundedStrip
            | ProfileProgram::MergedOpposingBevel
            | ProfileProgram::RadialDisc
            | ProfileProgram::Annulus
            | ProfileProgram::CustomCurve
    );
    let recessed = matches!(
        program,
        ProfileProgram::ConcaveGroove | ProfileProgram::RecessedSeam
    );
    let occupancy = ProfileOccupancySemantics {
        signed_distance: program != ProfileProgram::Flat,
        inside_outside: program != ProfileProgram::Flat,
        flat_center: program_requires_flat_center(program),
        raised,
        recessed,
        cap: matches!(
            program,
            ProfileProgram::FullyRoundedStrip | ProfileProgram::MergedOpposingBevel
        ),
        groove: matches!(
            program,
            ProfileProgram::ConcaveGroove | ProfileProgram::RecessedSeam
        ),
        profile_exclusion: program != ProfileProgram::Flat,
    };
    let required_halo_px = (width_pixels.ceil() as u32).saturating_add(1).min(256);
    let mut compiled = CompiledProfile {
        requested_program,
        program,
        sdf,
        first_width_m: first,
        second_width_m: second,
        minimum_flat_center_m: minimum_flat,
        amplitude_m: amplitude,
        angle_degrees: requested.angle_degrees,
        inner_radius_m: inner,
        outer_radius_m: outer,
        slot_size_m: request.slot_size_m,
        pixels_per_meter,
        lod,
        supersampling,
        evaluator,
        fallback,
        fallback_reason,
        occupancy,
        required_halo_px,
        seed: requested.seed,
        custom_curve: requested.custom_curve.clone(),
        compact_resource_references: if requested.custom_curve.is_empty() {
            Vec::new()
        } else {
            vec![ContentDigest::sha256(
                &serde_json::to_vec(&requested.custom_curve)
                    .map_err(|_| ProfileCompileError::InvalidRequest)?,
            )]
        },
        algorithm_id: STAGE_15_PROFILE_ALGORITHM_ID.into(),
        algorithm_version: STAGE_15_PROFILE_ALGORITHM_VERSION.into(),
        diagnostics: Vec::new(),
        cache_identity: ContentDigest(String::new()),
    };
    if let Some(reason) = &compiled.fallback_reason {
        compiled.diagnostics.push(reason.clone());
    }
    compiled.diagnostics.push(format!(
        "resolved physical profile: first={:.9}m second={:.9}m amplitude={:.9}m lod={:?} supersampling={}x",
        compiled.first_width_m, compiled.second_width_m, compiled.amplitude_m, compiled.lod, compiled.supersampling
    ));
    let identity_payload = serde_json::to_vec(&(
        request.upstream_identity,
        STAGE_15_PROFILE_ALGORITHM_VERSION,
        &compiled,
    ))
    .map_err(|_| ProfileCompileError::InvalidRequest)?;
    compiled.cache_identity = ContentDigest::sha256(&identity_payload);
    Ok(compiled)
}

pub fn compile_structural_intent(
    intent: StructuralProfile,
    slot_size_m: [f64; 2],
    destination_pixels: [u32; 2],
    capacity: &EffectCapacity,
    upstream_identity: &ContentDigest,
    seed: u64,
) -> Result<CompiledProfile, ProfileCompileError> {
    let requested = RequestedProfile::from_structural_intent(intent, seed);
    compile_profile(ProfileCompileRequest {
        requested: &requested,
        slot_size_m,
        destination_pixels,
        capacity,
        upstream_identity,
    })
}

fn resolve_length(length: ProfileLength, minor: f64) -> Result<f64, ProfileCompileError> {
    let value = match length {
        ProfileLength::Meters(value) => value,
        ProfileLength::RelativeMinor(value) => value * minor,
    };
    if value.is_finite() && value >= 0.0 {
        Ok(value)
    } else {
        Err(ProfileCompileError::InvalidRequest)
    }
}

fn valid_custom_curve(requested: &RequestedProfile) -> bool {
    if requested.program != ProfileProgram::CustomCurve {
        return requested.custom_curve.is_empty();
    }
    requested.custom_curve.len() >= 2
        && requested.custom_curve.iter().all(|point| {
            point.position.is_finite()
                && point.height.is_finite()
                && (0.0..=1.0).contains(&point.position)
        })
        && requested
            .custom_curve
            .windows(2)
            .all(|pair| pair[0].position < pair[1].position)
}

fn program_uses_opposing_edges(program: ProfileProgram) -> bool {
    matches!(
        program,
        ProfileProgram::ConvexBevel
            | ProfileProgram::ConcaveGroove
            | ProfileProgram::RoundedBevel
            | ProfileProgram::DoubleBevel
            | ProfileProgram::RaisedLip
            | ProfileProgram::RecessedSeam
            | ProfileProgram::PanelFrame
            | ProfileProgram::FullyRoundedStrip
            | ProfileProgram::MergedOpposingBevel
            | ProfileProgram::CustomCurve
    )
}

fn program_requires_flat_center(program: ProfileProgram) -> bool {
    program_uses_opposing_edges(program)
        && !matches!(
            program,
            ProfileProgram::FullyRoundedStrip | ProfileProgram::MergedOpposingBevel
        )
}

fn valid_capacity(capacity: &EffectCapacity) -> bool {
    [
        capacity.minimum_flat_center_m,
        capacity.maximum_left_profile_width_m,
        capacity.maximum_right_profile_width_m,
        capacity.maximum_top_profile_width_m,
        capacity.maximum_bottom_profile_width_m,
        capacity.maximum_isotropic_feature_m,
        capacity.maximum_radial_feature_m,
        capacity.minimum_full_height_feature_m,
        capacity.minimum_normal_only_feature_m,
        capacity.minimum_roughness_only_feature_m,
    ]
    .into_iter()
    .all(|value| value.is_finite() && value >= 0.0)
        && matches!(capacity.recommended_supersample_factor, 1 | 2 | 4 | 8)
}

fn required_variant(program: ProfileProgram) -> EffectVariant {
    match program {
        ProfileProgram::Flat => EffectVariant::Disabled,
        ProfileProgram::RadialDisc | ProfileProgram::Annulus => EffectVariant::Radial,
        ProfileProgram::FullyRoundedStrip => EffectVariant::FullyRoundedProfile,
        ProfileProgram::MergedOpposingBevel => EffectVariant::MergedRoundedProfile,
        _ => EffectVariant::Full,
    }
}

fn apply_capacity_fallback(
    program: &mut ProfileProgram,
    fallback: &mut ProfileFallback,
    forced_lod: &mut Option<ProfileLod>,
    fallback_reason: &mut Option<String>,
    policy: ProfileLegalityPolicy,
    reason: String,
) -> Result<(), ProfileCompileError> {
    match policy {
        ProfileLegalityPolicy::Incompatible => Err(ProfileCompileError::Incompatible),
        ProfileLegalityPolicy::NormalOnly => {
            *fallback = ProfileFallback::NormalOnly;
            *forced_lod = Some(ProfileLod::NormalOnly);
            *fallback_reason = Some(reason);
            Ok(())
        }
        ProfileLegalityPolicy::Clamp
        | ProfileLegalityPolicy::FullyRounded
        | ProfileLegalityPolicy::MergeOpposing
        | ProfileLegalityPolicy::Disabled => {
            *program = ProfileProgram::Flat;
            *fallback = ProfileFallback::Disabled;
            *forced_lod = Some(ProfileLod::Disabled);
            *fallback_reason = Some(reason);
            Ok(())
        }
    }
}

fn capacity_lod(
    policy: ProfileLodPolicy,
    program: ProfileProgram,
    feature_m: f64,
    capacity: &EffectCapacity,
) -> ProfileLod {
    match policy {
        ProfileLodPolicy::Force(lod) => lod,
        ProfileLodPolicy::Auto if program == ProfileProgram::Flat => ProfileLod::Disabled,
        ProfileLodPolicy::Auto => highest_allowed_lod(feature_m, capacity),
    }
}

fn highest_allowed_lod(feature_m: f64, capacity: &EffectCapacity) -> ProfileLod {
    if feature_m >= capacity.minimum_full_height_feature_m
        && capacity
            .allowed_effect_variants
            .contains(&EffectVariant::Full)
    {
        ProfileLod::FullHeight
    } else if feature_m >= capacity.minimum_normal_only_feature_m
        && capacity
            .allowed_effect_variants
            .contains(&EffectVariant::Simplified)
    {
        ProfileLod::SimplifiedHeight
    } else if feature_m >= capacity.minimum_normal_only_feature_m
        && capacity
            .allowed_effect_variants
            .contains(&EffectVariant::NormalOnly)
    {
        ProfileLod::NormalOnly
    } else if feature_m >= capacity.minimum_roughness_only_feature_m
        && capacity
            .allowed_effect_variants
            .contains(&EffectVariant::RoughnessOnly)
    {
        ProfileLod::RoughnessOnly
    } else {
        ProfileLod::Disabled
    }
}

fn lod_variant_allowed(lod: ProfileLod, capacity: &EffectCapacity) -> bool {
    let variant = match lod {
        ProfileLod::FullHeight => EffectVariant::Full,
        ProfileLod::SimplifiedHeight => EffectVariant::Simplified,
        ProfileLod::NormalOnly => EffectVariant::NormalOnly,
        ProfileLod::RoughnessOnly => EffectVariant::RoughnessOnly,
        ProfileLod::Disabled => EffectVariant::Disabled,
    };
    capacity.allowed_effect_variants.contains(&variant)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(program: ProfileProgram) -> RequestedProfile {
        let mut requested = RequestedProfile::from_structural_intent(StructuralProfile::Bevel, 19);
        requested.program = program;
        requested.custom_curve = if program == ProfileProgram::CustomCurve {
            vec![
                CustomProfilePoint {
                    position: 0.0,
                    height: 0.0,
                },
                CustomProfilePoint {
                    position: 0.5,
                    height: 1.0,
                },
                CustomProfilePoint {
                    position: 1.0,
                    height: 0.0,
                },
            ]
        } else {
            Vec::new()
        };
        requested
    }

    fn compile(requested: &RequestedProfile, pixels: [u32; 2]) -> CompiledProfile {
        compile_profile(ProfileCompileRequest {
            requested,
            slot_size_m: [2.0, 0.5],
            destination_pixels: pixels,
            capacity: &conservative_profile_capacity([2.0, 0.5]),
            upstream_identity: &ContentDigest::sha256(b"stage15-test-upstream"),
        })
        .expect("profile should compile")
    }

    #[test]
    fn algorithm_stage_15_gpu_profiles_compile_complete_program_set_in_physical_units() {
        for program in [
            ProfileProgram::Flat,
            ProfileProgram::ConvexBevel,
            ProfileProgram::ConcaveGroove,
            ProfileProgram::RoundedBevel,
            ProfileProgram::DoubleBevel,
            ProfileProgram::RaisedLip,
            ProfileProgram::RecessedSeam,
            ProfileProgram::PanelFrame,
            ProfileProgram::FullyRoundedStrip,
            ProfileProgram::MergedOpposingBevel,
            ProfileProgram::RadialDisc,
            ProfileProgram::Annulus,
            ProfileProgram::CustomCurve,
        ] {
            let requested = if program == ProfileProgram::Annulus {
                let mut requested = request(program);
                requested.inner_radius = ProfileLength::RelativeMinor(0.10);
                requested.outer_radius = ProfileLength::RelativeMinor(0.40);
                requested
            } else {
                request(program)
            };
            let compiled = compile(&requested, [4096, 1024]);
            assert_eq!(compiled.program, program);
            assert_eq!(compiled.slot_size_m, [2.0, 0.5]);
            assert_eq!(
                compiled.algorithm_version,
                STAGE_15_PROFILE_ALGORITHM_VERSION
            );
            assert!(!compiled.cache_identity.0.is_empty());
        }
    }

    #[test]
    fn algorithm_stage_15_gpu_profiles_legality_lod_and_scale_are_deterministic() {
        let mut requested = request(ProfileProgram::DoubleBevel);
        requested.first_width = ProfileLength::Meters(0.4);
        requested.second_width = ProfileLength::Meters(0.4);
        requested.minimum_flat_center = ProfileLength::Meters(0.1);
        requested.legality_policy = ProfileLegalityPolicy::MergeOpposing;
        let a = compile(&requested, [1024, 256]);
        let b = compile(&requested, [8192, 2048]);
        assert_eq!(a.fallback, ProfileFallback::MergedOpposing);
        assert_eq!(a.program, ProfileProgram::MergedOpposingBevel);
        assert!(a.first_width_m + a.second_width_m <= 0.5 + f64::EPSILON);
        assert_eq!(a.first_width_m, b.first_width_m);
        assert_eq!(a.amplitude_m, b.amplitude_m);
        assert_eq!(a.seed, b.seed);
        assert_ne!(a.pixels_per_meter, b.pixels_per_meter);
        assert_ne!(a.cache_identity, b.cache_identity);

        requested.legality_policy = ProfileLegalityPolicy::Incompatible;
        assert_eq!(
            compile_profile(ProfileCompileRequest {
                requested: &requested,
                slot_size_m: [2.0, 0.5],
                destination_pixels: [1024, 256],
                capacity: &conservative_profile_capacity([2.0, 0.5]),
                upstream_identity: &ContentDigest::sha256(b"stage15-incompatible"),
            }),
            Err(ProfileCompileError::Incompatible)
        );
    }

    #[test]
    fn stage10_capacity_limits_are_authoritative() {
        let mut requested = request(ProfileProgram::DoubleBevel);
        requested.first_width = ProfileLength::Meters(0.18);
        requested.second_width = ProfileLength::Meters(0.04);
        requested.legality_policy = ProfileLegalityPolicy::Clamp;
        let mut capacity = conservative_profile_capacity([2.0, 0.5]);
        capacity.maximum_left_profile_width_m = 0.05;
        capacity.maximum_top_profile_width_m = 0.05;
        capacity.maximum_right_profile_width_m = 0.03;
        capacity.maximum_bottom_profile_width_m = 0.03;
        capacity.minimum_full_height_feature_m = 0.20;
        capacity.minimum_normal_only_feature_m = 0.04;
        capacity.minimum_roughness_only_feature_m = 0.02;
        capacity.recommended_supersample_factor = 4;

        let compiled = compile_profile(ProfileCompileRequest {
            requested: &requested,
            slot_size_m: [2.0, 0.5],
            destination_pixels: [1024, 256],
            capacity: &capacity,
            upstream_identity: &ContentDigest::sha256(b"stage10-capacity-authority"),
        })
        .expect("capacity clamp should compile with fallback metadata");

        assert_eq!(compiled.fallback, ProfileFallback::Clamped);
        assert_eq!(compiled.first_width_m, 0.05);
        assert_eq!(compiled.second_width_m, 0.03);
        assert_eq!(compiled.lod, ProfileLod::SimplifiedHeight);
        assert_eq!(compiled.supersampling, 4);
        assert!(
            compiled
                .fallback_reason
                .as_deref()
                .unwrap_or_default()
                .contains("Stage 10 edge limits")
        );
    }

    #[test]
    fn stage10_capacity_disables_disallowed_variants_and_radial_overflow() {
        let mut radial = request(ProfileProgram::RadialDisc);
        radial.outer_radius = ProfileLength::Meters(0.30);
        radial.legality_policy = ProfileLegalityPolicy::Disabled;
        let mut capacity = conservative_profile_capacity([2.0, 0.5]);
        capacity.maximum_radial_feature_m = 0.20;
        let disabled = compile_profile(ProfileCompileRequest {
            requested: &radial,
            slot_size_m: [2.0, 0.5],
            destination_pixels: [1024, 256],
            capacity: &capacity,
            upstream_identity: &ContentDigest::sha256(b"stage10-radial-capacity"),
        })
        .expect("radial overflow should honor disabled policy");
        assert_eq!(disabled.program, ProfileProgram::Flat);
        assert_eq!(disabled.fallback, ProfileFallback::Disabled);
        assert_eq!(disabled.lod, ProfileLod::Disabled);
        assert_eq!(disabled.occupancy, ProfileOccupancySemantics::default());

        let mut bevel = request(ProfileProgram::ConvexBevel);
        bevel.legality_policy = ProfileLegalityPolicy::Incompatible;
        capacity
            .allowed_effect_variants
            .retain(|variant| !matches!(variant, EffectVariant::Full | EffectVariant::Simplified));
        assert_eq!(
            compile_profile(ProfileCompileRequest {
                requested: &bevel,
                slot_size_m: [2.0, 0.5],
                destination_pixels: [1024, 256],
                capacity: &capacity,
                upstream_identity: &ContentDigest::sha256(b"stage10-disallowed-variant"),
            }),
            Err(ProfileCompileError::Incompatible)
        );
    }

    #[test]
    fn flat_center_capacity_only_applies_to_rectangular_opposing_profiles() {
        let mut capacity = conservative_profile_capacity([2.0, 0.5]);
        capacity.can_have_flat_center = false;
        capacity.minimum_flat_center_m = 0.75;

        for program in [
            ProfileProgram::RadialDisc,
            ProfileProgram::Annulus,
            ProfileProgram::FullyRoundedStrip,
            ProfileProgram::MergedOpposingBevel,
        ] {
            let mut requested = request(program);
            if program == ProfileProgram::Annulus {
                requested.inner_radius = ProfileLength::RelativeMinor(0.10);
                requested.outer_radius = ProfileLength::RelativeMinor(0.40);
            }
            requested.minimum_flat_center = ProfileLength::Meters(0.75);
            let compiled = compile_profile(ProfileCompileRequest {
                requested: &requested,
                slot_size_m: [2.0, 0.5],
                destination_pixels: [1024, 256],
                capacity: &capacity,
                upstream_identity: &ContentDigest::sha256(b"stage10-flat-center-classes"),
            })
            .expect("program class should not require rectangular flat-center capacity");
            assert_eq!(compiled.program, program);
            assert_eq!(compiled.fallback, ProfileFallback::None);
        }

        let mut bevel = request(ProfileProgram::ConvexBevel);
        bevel.minimum_flat_center = ProfileLength::Meters(0.75);
        bevel.legality_policy = ProfileLegalityPolicy::Disabled;
        let disabled = compile_profile(ProfileCompileRequest {
            requested: &bevel,
            slot_size_m: [2.0, 0.5],
            destination_pixels: [1024, 256],
            capacity: &capacity,
            upstream_identity: &ContentDigest::sha256(b"stage10-flat-center-required"),
        })
        .expect("rectangular opposing profile should honor disabled fallback");
        assert_eq!(disabled.program, ProfileProgram::Flat);
        assert_eq!(disabled.fallback, ProfileFallback::Disabled);
    }

    #[test]
    fn annulus_widths_must_fit_independently_within_radial_band() {
        let mut requested = request(ProfileProgram::Annulus);
        requested.inner_radius = ProfileLength::Meters(0.20);
        requested.outer_radius = ProfileLength::Meters(0.30);
        requested.first_width = ProfileLength::Meters(0.08);
        requested.second_width = ProfileLength::Meters(0.07);
        requested.legality_policy = ProfileLegalityPolicy::Clamp;

        let compiled = compile_profile(ProfileCompileRequest {
            requested: &requested,
            slot_size_m: [2.0, 0.8],
            destination_pixels: [1024, 512],
            capacity: &conservative_profile_capacity([2.0, 0.8]),
            upstream_identity: &ContentDigest::sha256(b"stage10-annulus-band-width"),
        })
        .expect("annular overlap should clamp according to policy");

        assert_eq!(compiled.program, ProfileProgram::Annulus);
        assert_eq!(compiled.fallback, ProfileFallback::Clamped);
        assert!(compiled.first_width_m + compiled.second_width_m <= 0.10 + f64::EPSILON);
        assert!(
            compiled
                .fallback_reason
                .as_deref()
                .unwrap_or_default()
                .contains("annulus radial widths")
        );
    }
}
