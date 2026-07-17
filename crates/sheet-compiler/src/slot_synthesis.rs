//! Stage 14: bounded, registered execution of validated Stage 13 sampling plans.

use std::collections::BTreeMap;

use hot_trimmer_domain::{
    AlgorithmProvenance, CompilationDiagnostic, ContentDigest, DiagnosticCode, QuarterTurn,
    RecoveryChoice, SamplingMode, SourceSamplingMode, StageResult,
};
use hot_trimmer_image_io::{ImagePlane, LinearColor, LinearScalar, MaskValue, TangentNormal};
use hot_trimmer_material_synthesis::{DomainRoute, PreparedMaterialDomain, SeamAxis};
use hot_trimmer_placement_solver::{
    MirrorTransform, SamplingPlan, SliceCenterPolicy, SliceGeometry, SourceCrop,
    StretchOverrideProvenance,
};
use hot_trimmer_render_core::{PreparedExemplarChannel, RenderCancellationToken};
use thiserror::Error;

pub const STAGE_14_ALGORITHM_ID: &str = "hot-trimmer.stage-14.registered-slot-synthesis";
pub const STAGE_14_ALGORITHM_VERSION: &str = "1.0.0";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlotSynthesisLimits {
    pub max_dimension: u32,
    pub max_pixels: u64,
    pub max_operations: u64,
    pub tile_edge: u32,
}

impl Default for SlotSynthesisLimits {
    fn default() -> Self {
        Self { max_dimension: 16_384, max_pixels: 268_435_456, max_operations: 4_294_967_296, tile_edge: 128 }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SlotSynthesisRequest<'a> {
    pub plan: &'a SamplingPlan,
    pub domain: &'a PreparedMaterialDomain,
    pub output_dimensions: [u32; 2],
    pub limits: SlotSynthesisLimits,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlotSynthesisQaView { SamplingCoordinates, Correspondence, RegisteredMarkers, Validity, MappingMode }

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlotSynthesisDiagnostics {
    pub requested_mode: SamplingMode,
    pub executed_mode: SamplingMode,
    pub sampling_filter: SourceSamplingMode,
    pub explicit_stretch_user_override: bool,
    pub messages: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SynthesizedSlotMaterial {
    pub width: u32,
    pub height: u32,
    pub channels: Vec<PreparedExemplarChannel>,
    /// One shared domain position was used to evaluate every registered channel.
    pub correspondence: ImagePlane<[f32; 2]>,
    pub validity: ImagePlane<MaskValue>,
    pub diagnostics: SlotSynthesisDiagnostics,
    pub qa_views: Vec<SlotSynthesisQaView>,
    pub stage_result: StageResult,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SlotSynthesisError {
    #[error("Stage 14 received a SamplingPlan that does not satisfy Stage 13 validation")]
    InvalidPlan,
    #[error("ExplicitStretch is legal only with visible user-override provenance")]
    MissingExplicitStretchOverride,
    #[error("Stage 14 domain does not match the validated SamplingPlan")]
    DomainMismatch,
    #[error("Stage 14 dimensions, allocation, or operation count exceed declared bounds")]
    ResourceLimitExceeded,
    #[error("Stage 14 evaluation was cancelled")]
    Cancelled,
    #[error("Stage 14 could not allocate a registered intermediate plane")]
    PlaneConstruction,
    #[error("Stage 14 synthesized slice center is unavailable or too small for the requested physical center")]
    InsufficientSynthesizedCenter,
}

impl SlotSynthesisError {
    #[must_use]
    pub fn stage_result(&self) -> StageResult {
        let code = match self {
            Self::Cancelled => DiagnosticCode::Cancelled,
            Self::ResourceLimitExceeded => DiagnosticCode::ResourceLimitExceeded,
            Self::InsufficientSynthesizedCenter => DiagnosticCode::InsufficientInput,
            _ => DiagnosticCode::MalformedInput,
        };
        StageResult::FailedWithRecovery {
            reason: CompilationDiagnostic { code, stage: Some(14), message: self.to_string(), context: BTreeMap::new() },
            // Deliberately excludes Stretch: a failed plan has no raster fallback.
            recovery_choices: if matches!(self, Self::InsufficientSynthesizedCenter) {
                vec![RecoveryChoice::UseSynthesis, RecoveryChoice::ChooseAnotherSource, RecoveryChoice::AdjustSettings]
            } else { vec![RecoveryChoice::AdjustSettings, RecoveryChoice::ChooseAnotherSource] },
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Position { x: f64, y: f64, valid: bool }

pub fn synthesize_slot_material(
    request: SlotSynthesisRequest<'_>,
    cancellation: &RenderCancellationToken,
) -> Result<SynthesizedSlotMaterial, SlotSynthesisError> {
    synthesize_slot_material_with_guard(request, &|| cancellation.is_cancelled())
}

pub fn synthesize_slot_material_with_guard(
    request: SlotSynthesisRequest<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<SynthesizedSlotMaterial, SlotSynthesisError> {
    validate(&request)?;
    if cancelled() { return Err(SlotSynthesisError::Cancelled); }
    let [width, height] = request.output_dimensions;
    let pixels = u64::from(width).checked_mul(u64::from(height)).ok_or(SlotSynthesisError::ResourceLimitExceeded)?;
    let channel_count = u64::try_from(request.domain.registered_channels().len()).unwrap_or(u64::MAX);
    if width > request.limits.max_dimension || height > request.limits.max_dimension || pixels > request.limits.max_pixels
        || pixels.checked_mul(channel_count.saturating_add(2)).is_none_or(|n| n > request.limits.max_operations) {
        return Err(SlotSynthesisError::ResourceLimitExceeded);
    }

    let mut positions = Vec::with_capacity(usize::try_from(pixels).map_err(|_| SlotSynthesisError::ResourceLimitExceeded)?);
    let mut validity = Vec::with_capacity(positions.capacity());
    for y in 0..height {
        if cancelled() { return Err(SlotSynthesisError::Cancelled); }
        for x in 0..width {
            let q = [(f64::from(x) + 0.5) / f64::from(width), (f64::from(y) + 0.5) / f64::from(height)];
            let position = map_position(&request, q);
            let source_valid = position.valid && sample_validity(request.domain, position.x, position.y);
            if !position.x.is_finite() || !position.y.is_finite() { return Err(SlotSynthesisError::InvalidPlan); }
            positions.push([position.x as f32, position.y as f32]);
            validity.push(MaskValue(if source_valid { 1.0 } else { 0.0 }));
        }
    }
    let correspondence = plane(width, height, request.limits.tile_edge, &positions)?;
    let validity = plane(width, height, request.limits.tile_edge, &validity)?;
    let mut channels = Vec::with_capacity(request.domain.registered_channels().len());
    for channel in request.domain.registered_channels() {
        channels.push(sample_channel(channel, &positions, width, height, &request, cancelled)?);
    }
    if cancelled() { return Err(SlotSynthesisError::Cancelled); }
    let algorithm = AlgorithmProvenance { algorithm_id: STAGE_14_ALGORITHM_ID.into(), version: STAGE_14_ALGORITHM_VERSION.into() };
    let settings_hash = ContentDigest::sha256(format!("{:?}|{:?}|{:?}|{:?}|{:?}", request.plan.candidate.mapping_mode,
        request.plan.sampling_policy, request.output_dimensions, request.plan.slot_physical_size, request.plan.candidate.transform).as_bytes());
    Ok(SynthesizedSlotMaterial {
        width, height, channels, correspondence, validity,
        diagnostics: SlotSynthesisDiagnostics {
            requested_mode: request.plan.candidate.mapping_mode,
            executed_mode: request.plan.candidate.mapping_mode,
            sampling_filter: request.plan.sampling_policy.filter,
            explicit_stretch_user_override: matches!(request.plan.stretch_override, StretchOverrideProvenance::UserOverride { .. }),
            messages: vec![if request.plan.candidate.mapping_mode == SamplingMode::ExplicitStretch
                || matches!(request.plan.slice_geometry, SliceGeometry::Nine { center: SliceCenterPolicy::ExplicitStretch, .. }) {
                "ExplicitStretch executed from a visible user override".into()
            } else { "registered slot-local physical correspondence executed without non-uniform scaling".into() }],
        },
        qa_views: vec![SlotSynthesisQaView::SamplingCoordinates, SlotSynthesisQaView::Correspondence,
            SlotSynthesisQaView::RegisteredMarkers, SlotSynthesisQaView::Validity, SlotSynthesisQaView::MappingMode],
        stage_result: StageResult::Executed { algorithm, settings_hash, diagnostics: Vec::new() },
    })
}

fn validate(r: &SlotSynthesisRequest<'_>) -> Result<(), SlotSynthesisError> {
    let p = r.plan;
    if r.output_dimensions.contains(&0) || p.slot_physical_size.iter().any(|v| !v.is_finite() || *v <= 0.0)
        || !p.candidate.isotropic_scale.is_finite() || p.candidate.isotropic_scale <= 0.0
        || !p.source_pixels_per_physical_unit.is_finite() || p.source_pixels_per_physical_unit <= 0.0
        || !p.sampling_policy.scale.is_finite() || p.sampling_policy.scale <= 0.0
        || !p.candidate.eligibility.mapping_permitted || !p.candidate.eligibility.transform_permitted
        || (p.candidate.mapping_mode != SamplingMode::ExplicitStretch && !p.candidate.eligibility.isotropic_scale) {
        return Err(SlotSynthesisError::InvalidPlan);
    }
    // Stage 14 has no registered TextureSynthesis executor. Reject it instead
    // of allowing the generic physical branch to become centered full-source
    // sampling, and require every executable plan to carry its selected crop.
    if p.candidate.mapping_mode == SamplingMode::TextureSynthesis || p.candidate.crop.is_none() {
        return Err(SlotSynthesisError::InvalidPlan);
    }
    if p.candidate.mapping_mode == SamplingMode::ExplicitStretch
        && !matches!(p.stretch_override, StretchOverrideProvenance::UserOverride { .. }) {
        return Err(SlotSynthesisError::MissingExplicitStretchOverride);
    }
    if p.candidate.domain_id != r.domain.cache_key || p.candidate.source_id != r.domain.prepared_source_digest
        || p.prepared_domain_dimensions != [r.domain.width, r.domain.height]
        || p.candidate.correspondence_reference != r.domain.cache_key {
        return Err(SlotSynthesisError::DomainMismatch);
    }
    let c = crop(r);
    if c.width == 0 || c.height == 0
        || c.x.checked_add(c.width).is_none_or(|end| end > r.domain.width)
        || c.y.checked_add(c.height).is_none_or(|end| end > r.domain.height) {
        return Err(SlotSynthesisError::InvalidPlan);
    }
    if let Some(period) = p.candidate.period_pixels
        && (period.contains(&0) || period[0] > c.width || period[1] > c.height) {
        return Err(SlotSynthesisError::InvalidPlan);
    }
    if p.radial_mapping.is_some_and(|radial| !radial.center_x.is_finite() || !radial.center_y.is_finite()
        || !radial.inner_radius.is_finite() || !radial.outer_radius.is_finite()
        || !radial.falloff.is_finite() || !(0.0..=1.0).contains(&radial.center_x)
        || !(0.0..=1.0).contains(&radial.center_y) || radial.inner_radius < 0.0
        || radial.outer_radius <= radial.inner_radius || radial.outer_radius > 2.0
        || !(0.1..=4.0).contains(&radial.falloff)) {
        return Err(SlotSynthesisError::InvalidPlan);
    }
    if matches!(p.candidate.mapping_mode, SamplingMode::PeriodicTile | SamplingMode::RepeatX | SamplingMode::RepeatY)
        && p.candidate.period_pixels.is_none() { return Err(SlotSynthesisError::InvalidPlan); }
    match (p.candidate.mapping_mode, p.slice_geometry) {
        (SamplingMode::ThreeSliceCap, SliceGeometry::Three { leading_cap_pixels, trailing_cap_pixels, center })
            if leading_cap_pixels > 0 && trailing_cap_pixels > 0
                && leading_cap_pixels.checked_add(trailing_cap_pixels).is_some_and(|sum| sum < c.width)
                && center != SliceCenterPolicy::ExplicitStretch => {}
        (SamplingMode::NineSlicePanel, SliceGeometry::Nine { left_pixels, right_pixels, top_pixels, bottom_pixels, center })
            if left_pixels > 0 && right_pixels > 0 && top_pixels > 0 && bottom_pixels > 0
                && left_pixels.checked_add(right_pixels).is_some_and(|sum| sum < c.width)
                && top_pixels.checked_add(bottom_pixels).is_some_and(|sum| sum < c.height)
                && (center != SliceCenterPolicy::ExplicitStretch
                    || matches!(p.stretch_override, StretchOverrideProvenance::UserOverride { .. })) => {}
        (SamplingMode::ThreeSliceCap | SamplingMode::NineSlicePanel, _) => return Err(SlotSynthesisError::InvalidPlan),
        (_, SliceGeometry::None) => {}
        _ => return Err(SlotSynthesisError::InvalidPlan),
    }
    validate_slice_destination(p)?;
    for index in &p.candidate.seam_indices {
        let Some(seam) = usize::try_from(*index).ok().and_then(|index| r.domain.seams.get(index)) else {
            return Err(SlotSynthesisError::InvalidPlan);
        };
        let (expected_length, lower, upper, allowed) = match seam.axis {
            SeamAxis::X => (r.domain.height, c.x, c.x + c.width,
                matches!(p.candidate.mapping_mode, SamplingMode::RepeatX | SamplingMode::PeriodicTile)),
            SeamAxis::Y => (r.domain.width, c.y, c.y + c.height,
                matches!(p.candidate.mapping_mode, SamplingMode::RepeatY | SamplingMode::PeriodicTile)),
        };
        if !allowed || seam.normalized_cost_milli > p.maximum_seam_cost_milli
            || seam.positions.len() != usize::try_from(expected_length).unwrap_or(usize::MAX)
            || seam.positions.iter().any(|position| u32::from(*position) < lower || u32::from(*position) >= upper) {
            return Err(SlotSynthesisError::InvalidPlan);
        }
    }
    let transformed = p.candidate.transform.rotation != QuarterTurn::Zero || p.candidate.transform.mirror != MirrorTransform::None;
    if transformed && !p.sampling_policy.correct_tangent_normals
        && r.domain.registered_channels().iter().any(|channel| matches!(channel, PreparedExemplarChannel::Normal { .. })) {
        return Err(SlotSynthesisError::InvalidPlan);
    }
    validate_synthesized_center(r, c)?;
    if r.limits.tile_edge == 0 { return Err(SlotSynthesisError::ResourceLimitExceeded); }
    Ok(())
}

fn validate_slice_destination(p: &SamplingPlan) -> Result<(), SlotSynthesisError> {
    let size = if matches!(p.candidate.transform.rotation, QuarterTurn::Ninety | QuarterTurn::TwoSeventy) {
        [p.slot_physical_size[1], p.slot_physical_size[0]]
    } else { p.slot_physical_size };
    let scale = p.source_pixels_per_physical_unit * p.sampling_policy.scale;
    let fits = match p.slice_geometry {
        SliceGeometry::Three { leading_cap_pixels, trailing_cap_pixels, .. } =>
            size[0] > f64::from(leading_cap_pixels + trailing_cap_pixels) / scale,
        SliceGeometry::Nine { left_pixels, right_pixels, top_pixels, bottom_pixels, .. } =>
            size[0] > f64::from(left_pixels + right_pixels) / scale
                && size[1] > f64::from(top_pixels + bottom_pixels) / scale,
        SliceGeometry::None => true,
    };
    if fits { Ok(()) } else { Err(SlotSynthesisError::InvalidPlan) }
}

fn validate_synthesized_center(r: &SlotSynthesisRequest<'_>, c: SourceCrop) -> Result<(), SlotSynthesisError> {
    let p = r.plan;
    let synthesized = matches!(r.domain.route, DomainRoute::TextureQuilting | DomainRoute::PatchMatch
        | DomainRoute::StatisticalSynthesis | DomainRoute::ProceduralReconstruction | DomainRoute::LearnedProvider);
    let size = if matches!(p.candidate.transform.rotation, QuarterTurn::Ninety | QuarterTurn::TwoSeventy) {
        [p.slot_physical_size[1], p.slot_physical_size[0]]
    } else { p.slot_physical_size };
    let scale = p.source_pixels_per_physical_unit * p.sampling_policy.scale;
    let fits = match p.slice_geometry {
        SliceGeometry::Three { leading_cap_pixels, trailing_cap_pixels, center: SliceCenterPolicy::Synthesize } => {
            let requested = size[0] - f64::from(leading_cap_pixels + trailing_cap_pixels) / scale;
            synthesized && requested >= 0.0 && requested * scale <= f64::from(c.width - leading_cap_pixels - trailing_cap_pixels) + 1.0e-9
        }
        SliceGeometry::Nine { left_pixels, right_pixels, top_pixels, bottom_pixels, center: SliceCenterPolicy::Synthesize } => {
            let requested_x = size[0] - f64::from(left_pixels + right_pixels) / scale;
            let requested_y = size[1] - f64::from(top_pixels + bottom_pixels) / scale;
            synthesized && requested_x >= 0.0 && requested_y >= 0.0
                && requested_x * scale <= f64::from(c.width - left_pixels - right_pixels) + 1.0e-9
                && requested_y * scale <= f64::from(c.height - top_pixels - bottom_pixels) + 1.0e-9
        }
        _ => true,
    };
    if fits { Ok(()) } else { Err(SlotSynthesisError::InsufficientSynthesizedCenter) }
}

fn crop(r: &SlotSynthesisRequest<'_>) -> SourceCrop {
    r.plan.candidate.crop.expect("validated Stage 14 plan must carry a source crop")
}

fn map_position(r: &SlotSynthesisRequest<'_>, q: [f64; 2]) -> Position {
    let c = crop(r);
    let cw = f64::from(c.width); let ch = f64::from(c.height);
    let destination_size = r.plan.slot_physical_size;
    let transform = r.plan.candidate.transform;
    let local = [(q[0] - 0.5) * destination_size[0], (q[1] - 0.5) * destination_size[1]];
    let source_local = transform_local(local, transform.rotation, transform.mirror);
    let source_size = if matches!(transform.rotation, QuarterTurn::Ninety | QuarterTurn::TwoSeventy) {
        [destination_size[1], destination_size[0]]
    } else { destination_size };
    let m = [source_local[0] + source_size[0] * 0.5, source_local[1] + source_size[1] * 0.5];
    let scale = r.plan.source_pixels_per_physical_unit * r.plan.sampling_policy.scale;
    let mode = r.plan.candidate.mapping_mode;
    let mut p = match mode {
        SamplingMode::UniqueContain | SamplingMode::UniqueCover => {
            let pixels_per_unit = if mode == SamplingMode::UniqueContain {
                (cw / source_size[0]).max(ch / source_size[1])
            } else { (cw / source_size[0]).min(ch / source_size[1]) } * r.plan.sampling_policy.scale;
            let extent = [cw / pixels_per_unit, ch / pixels_per_unit];
            let origin = [(source_size[0] - extent[0]) * 0.5, (source_size[1] - extent[1]) * 0.5];
            let valid = mode != SamplingMode::UniqueContain
                || (m[0] >= origin[0] && m[0] < origin[0] + extent[0] && m[1] >= origin[1] && m[1] < origin[1] + extent[1]);
            Position { x: f64::from(c.x) + (m[0] - origin[0]) * pixels_per_unit,
                y: f64::from(c.y) + (m[1] - origin[1]) * pixels_per_unit, valid }
        }
        SamplingMode::ExplicitStretch => Position { x: f64::from(c.x) + m[0] / source_size[0] * cw,
            y: f64::from(c.y) + m[1] / source_size[1] * ch, valid: true },
        SamplingMode::PolarRadial => {
            let (center_x, center_y, inner_radius, outer_radius, falloff) = r.plan.radial_mapping.map_or(
                (0.5, 0.5, 0.0, 0.5, 1.0), |radial| (radial.center_x, radial.center_y,
                    radial.inner_radius, radial.outer_radius, radial.falloff));
            let radial_local = transform_local([q[0] - center_x, q[1] - center_y],
                transform.rotation, transform.mirror);
            let dx = radial_local[0]; let dy = radial_local[1];
            let radius = dx.hypot(dy); let radial_span = (outer_radius - inner_radius).max(f64::EPSILON);
            let theta = dy.atan2(dx).rem_euclid(std::f64::consts::TAU) / std::f64::consts::TAU;
            Position { x: f64::from(c.x) + theta * cw,
                y: f64::from(c.y) + ((radius - inner_radius) / radial_span).clamp(0.0, 1.0).powf(falloff) * ch,
                valid: radius >= inner_radius && radius <= outer_radius }
        }
        SamplingMode::PlanarRadial => {
            let (center_x, center_y, inner_radius, outer_radius) = r.plan.radial_mapping.map_or(
                (0.5, 0.5, 0.0, f64::INFINITY), |radial| (radial.center_x, radial.center_y, radial.inner_radius, radial.outer_radius));
            let radius = (q[0] - center_x).hypot(q[1] - center_y);
            let radial_local = transform_local([(q[0] - center_x) * destination_size[0],
                (q[1] - center_y) * destination_size[1]], transform.rotation, transform.mirror);
            Position { x: f64::from(c.x) + center_x * cw + radial_local[0] * scale,
                y: f64::from(c.y) + center_y * ch + radial_local[1] * scale,
                valid: radius >= inner_radius && radius <= outer_radius }
        }
        SamplingMode::ThreeSliceCap => three_slice(c, m, source_size, scale, r.plan.slice_geometry),
        SamplingMode::NineSlicePanel => nine_slice(c, m, source_size, scale, r.plan.slice_geometry),
        _ => Position { x: f64::from(c.x) + cw * 0.5 + source_local[0] * scale,
            y: f64::from(c.y) + ch * 0.5 + source_local[1] * scale, valid: true },
    };
    match mode {
        SamplingMode::PeriodicTile => {
            let period = r.plan.candidate.period_pixels.expect("validated periodic plan");
            let phase_x = seam_phase(r, SeamAxis::X, p.y);
            let phase_y = seam_phase(r, SeamAxis::Y, p.x);
            p.x = f64::from(c.x) + (p.x - f64::from(c.x) + phase_x).rem_euclid(f64::from(period[0]));
            p.y = f64::from(c.y) + (p.y - f64::from(c.y) + phase_y).rem_euclid(f64::from(period[1]));
        }
        SamplingMode::RepeatX => {
            let period = r.plan.candidate.period_pixels.map_or(c.width, |v| v[0]).max(1);
            p.y = p.y.clamp(f64::from(c.y), f64::from(c.y + c.height - 1));
            let phase = seam_phase(r, SeamAxis::X, p.y);
            p.x = f64::from(c.x) + (p.x - f64::from(c.x) + phase).rem_euclid(f64::from(period));
        }
        SamplingMode::RepeatY => {
            let period = r.plan.candidate.period_pixels.map_or(c.height, |v| v[1]).max(1);
            p.x = p.x.clamp(f64::from(c.x), f64::from(c.x + c.width - 1));
            let phase = seam_phase(r, SeamAxis::Y, p.x);
            p.y = f64::from(c.y) + (p.y - f64::from(c.y) + phase).rem_euclid(f64::from(period));
        }
        _ => {}
    }
    p
}

fn seam_phase(r: &SlotSynthesisRequest<'_>, axis: SeamAxis, cross_coordinate: f64) -> f64 {
    let c = crop(r);
    r.plan.candidate.seam_indices.iter().find_map(|index| {
        let seam = &r.domain.seams[usize::try_from(*index).expect("validated seam index")];
        if seam.axis != axis { return None; }
        let cross_limit = match axis { SeamAxis::X => r.domain.height, SeamAxis::Y => r.domain.width };
        let cross = (cross_coordinate - 0.5).round().clamp(0.0, f64::from(cross_limit - 1)) as usize;
        let origin = match axis { SeamAxis::X => c.x, SeamAxis::Y => c.y };
        Some(f64::from(seam.positions[cross]) - f64::from(origin))
    }).unwrap_or(0.0)
}

fn transform_local(mut p: [f64; 2], rotation: QuarterTurn, mirror: MirrorTransform) -> [f64; 2] {
    match mirror { MirrorTransform::X => p[0] = -p[0], MirrorTransform::Y => p[1] = -p[1], MirrorTransform::None => {} }
    match rotation { QuarterTurn::Zero => p, QuarterTurn::Ninety => [p[1], -p[0]],
        QuarterTurn::OneEighty => [-p[0], -p[1]], QuarterTurn::TwoSeventy => [-p[1], p[0]] }
}

fn three_slice(c: SourceCrop, m: [f64; 2], size: [f64; 2], scale: f64, geometry: SliceGeometry) -> Position {
    let SliceGeometry::Three { leading_cap_pixels, trailing_cap_pixels, center } = geometry else { unreachable!("validated") };
    let (x, valid) = slice_axis(m[0], size[0], c.x, c.width, leading_cap_pixels, trailing_cap_pixels, scale, center);
    Position { x, y: f64::from(c.y) + (m[1] - size[1] * 0.5) * scale + f64::from(c.height) * 0.5, valid }
}

fn nine_slice(c: SourceCrop, m: [f64; 2], size: [f64; 2], scale: f64, geometry: SliceGeometry) -> Position {
    let SliceGeometry::Nine { left_pixels, right_pixels, top_pixels, bottom_pixels, center } = geometry else { unreachable!("validated") };
    let (x, valid_x) = slice_axis(m[0], size[0], c.x, c.width, left_pixels, right_pixels, scale, center);
    let (y, valid_y) = slice_axis(m[1], size[1], c.y, c.height, top_pixels, bottom_pixels, scale, center);
    Position { x, y, valid: valid_x && valid_y }
}

fn slice_axis(value: f64, destination: f64, origin: u32, extent: u32, leading: u32, trailing: u32,
    scale: f64, center: SliceCenterPolicy) -> (f64, bool) {
    let leading_world = f64::from(leading) / scale; let trailing_world = f64::from(trailing) / scale;
    if value < leading_world { return (f64::from(origin) + value * scale, true); }
    if value >= destination - trailing_world {
        return (f64::from(origin + extent - trailing) + (value - (destination - trailing_world)) * scale, true);
    }
    let offset = (value - leading_world) * scale; let center_pixels = extent - leading - trailing;
    match center {
        SliceCenterPolicy::Repeat => (f64::from(origin + leading) + offset.rem_euclid(f64::from(center_pixels)), true),
        SliceCenterPolicy::Synthesize => (f64::from(origin + leading) + offset, true),
        SliceCenterPolicy::ExplicitStretch => {
            let destination_center = destination - leading_world - trailing_world;
            (f64::from(origin + leading) + (value - leading_world) / destination_center * f64::from(center_pixels), true)
        }
    }
}

fn sample_validity(domain: &PreparedMaterialDomain, x: f64, y: f64) -> bool {
    if x < 0.0 || y < 0.0 || x >= f64::from(domain.width) || y >= f64::from(domain.height) { return false; }
    let pixel_x = (x - 0.5).round().clamp(0.0, f64::from(domain.width - 1)) as u32;
    let pixel_y = (y - 0.5).round().clamp(0.0, f64::from(domain.height - 1)) as u32;
    domain.validity.pixel(pixel_x, pixel_y).0 >= 0.5
}

fn sample_channel(channel: &PreparedExemplarChannel, positions: &[[f32; 2]], width: u32, height: u32,
    r: &SlotSynthesisRequest<'_>, cancelled: &dyn Fn() -> bool) -> Result<PreparedExemplarChannel, SlotSynthesisError> {
    let edge = r.limits.tile_edge; let linear = r.plan.sampling_policy.filter != SourceSamplingMode::Nearest;
    Ok(match channel {
        PreparedExemplarChannel::BaseColor { plane: src, alpha_mode } => PreparedExemplarChannel::BaseColor {
            plane: plane(width, height, edge, &rasterize(positions, width, cancelled, |p| sample_color(src, p, linear))?)?, alpha_mode: *alpha_mode },
        PreparedExemplarChannel::Scalar { role, plane: src } => PreparedExemplarChannel::Scalar { role: *role,
            plane: plane(width, height, edge, &rasterize(positions, width, cancelled, |p| LinearScalar(sample_f32(src, p, linear, |v| v.0)))?)? },
        PreparedExemplarChannel::Normal { plane: src, source_convention, canonical_convention, alpha_policy } => PreparedExemplarChannel::Normal {
            plane: plane(width, height, edge, &rasterize(positions, width, cancelled, |p| transform_normal(sample_normal(src, p, linear), r))?)?,
            source_convention: *source_convention, canonical_convention: *canonical_convention, alpha_policy: *alpha_policy },
        PreparedExemplarChannel::MaterialId { plane: src } => PreparedExemplarChannel::MaterialId {
            plane: plane(width, height, edge, &rasterize(positions, width, cancelled, |p| sample_nearest(src, p))?)? },
        PreparedExemplarChannel::Mask { role, plane: src } => PreparedExemplarChannel::Mask { role: *role,
            plane: plane(width, height, edge, &rasterize(positions, width, cancelled, |p| MaskValue(sample_f32(src, p, linear, |v| v.0)))?)? },
    })
}

fn rasterize<T>(positions: &[[f32; 2]], width: u32, cancelled: &dyn Fn() -> bool,
    mut sample: impl FnMut([f32; 2]) -> T) -> Result<Vec<T>, SlotSynthesisError> {
    let mut values = Vec::with_capacity(positions.len());
    for row in positions.chunks_exact(usize::try_from(width).map_err(|_| SlotSynthesisError::ResourceLimitExceeded)?) {
        if cancelled() { return Err(SlotSynthesisError::Cancelled); }
        values.extend(row.iter().copied().map(&mut sample));
    }
    Ok(values)
}

fn bounds<T>(p: &ImagePlane<T>, at: [f32; 2]) -> (u32, u32, u32, u32, f32, f32) {
    // Correspondence uses texel-boundary coordinates, so pixel centers are N + 0.5.
    let x = (at[0] - 0.5).clamp(0.0, (p.width() - 1) as f32);
    let y = (at[1] - 0.5).clamp(0.0, (p.height() - 1) as f32);
    let x0 = x.floor() as u32; let y0 = y.floor() as u32;
    (x0, y0, (x0 + 1).min(p.width() - 1), (y0 + 1).min(p.height() - 1), x - x.floor(), y - y.floor())
}
fn sample_nearest<T: Copy>(p: &ImagePlane<T>, at: [f32; 2]) -> T { let b = bounds(p, at); *p.pixel(if b.4 < 0.5 { b.0 } else { b.2 }, if b.5 < 0.5 { b.1 } else { b.3 }) }
fn sample_f32<T: Copy>(p: &ImagePlane<T>, at: [f32; 2], linear: bool, f: impl Fn(&T)->f32) -> f32 {
    if !linear { return f(&sample_nearest(p, at)); } let (x0,y0,x1,y1,tx,ty)=bounds(p,at);
    let a=f(p.pixel(x0,y0))*(1.0-tx)+f(p.pixel(x1,y0))*tx; let b=f(p.pixel(x0,y1))*(1.0-tx)+f(p.pixel(x1,y1))*tx; a*(1.0-ty)+b*ty
}
fn sample_color(p: &ImagePlane<LinearColor>, at:[f32;2], linear:bool)->LinearColor { LinearColor {
    rgb: std::array::from_fn(|i| sample_f32(p,at,linear,|v|v.rgb[i])), alpha: sample_f32(p,at,linear,|v|v.alpha) } }
fn sample_normal(p:&ImagePlane<TangentNormal>,at:[f32;2],linear:bool)->TangentNormal { let mut n=TangentNormal {
    xyz:std::array::from_fn(|i|sample_f32(p,at,linear,|v|v.xyz[i])),alpha:sample_f32(p,at,linear,|v|v.alpha)};
    let l=(n.xyz.iter().map(|v|v*v).sum::<f32>()).sqrt().max(f32::EPSILON); n.xyz=n.xyz.map(|v|v/l); n }
fn transform_normal(mut n:TangentNormal,r:&SlotSynthesisRequest<'_>)->TangentNormal {
    if !r.plan.sampling_policy.correct_tangent_normals { return n; }
    let transform = r.plan.candidate.transform;
    let mut xy = match transform.rotation {
        QuarterTurn::Zero => [f64::from(n.xyz[0]), f64::from(n.xyz[1])],
        QuarterTurn::Ninety => [-f64::from(n.xyz[1]), f64::from(n.xyz[0])],
        QuarterTurn::OneEighty => [-f64::from(n.xyz[0]), -f64::from(n.xyz[1])],
        QuarterTurn::TwoSeventy => [f64::from(n.xyz[1]), -f64::from(n.xyz[0])],
    };
    match transform.mirror { MirrorTransform::X => xy[0] = -xy[0], MirrorTransform::Y => xy[1] = -xy[1], MirrorTransform::None => {} }
    n.xyz[0]=xy[0] as f32; n.xyz[1]=xy[1] as f32; n
}
fn plane<T:Clone>(w:u32,h:u32,edge:u32,values:&[T])->Result<ImagePlane<T>,SlotSynthesisError> {
    ImagePlane::from_row_major(w,h,edge,values).map_err(|_|SlotSynthesisError::PlaneConstruction)
}

#[cfg(test)]
mod tests {
    use hot_trimmer_domain::{
        MaterialChannelRole, NormalConvention, RegionId, SamplingPolicy, TemplateSlotRole,
    };
    use hot_trimmer_image_io::{CategoryId, NormalAlphaPolicy, ResolvedAlphaMode};
    use hot_trimmer_material_synthesis::{PreparedMaterialDomain, SelectedSeam};
    use hot_trimmer_placement_solver::{
        CandidateDescriptors, CandidateFamily, CandidateRoute, CandidateTransform, CropCandidate,
        EligibilityEvidence, PositionStrategy,
    };

    use super::*;

    fn fixture() -> (PreparedMaterialDomain, ContentDigest, ContentDigest) {
        fixture_with(DomainRoute::DirectSource, Vec::new())
    }

    fn fixture_with(route: DomainRoute, seams: Vec<SelectedSeam>) -> (PreparedMaterialDomain, ContentDigest, ContentDigest) {
        let width=8; let height=8; let mut colors=Vec::new(); let mut scalars=Vec::new();
        let mut normals=Vec::new(); let mut ids=Vec::new(); let mut masks=Vec::new();
        for y in 0..height { for x in 0..width {
            let marker=(x+y*width) as f32/63.0;
            colors.push(LinearColor{rgb:[marker,x as f32/7.0,y as f32/7.0],alpha:1.0});
            scalars.push(LinearScalar(marker)); normals.push(TangentNormal{xyz:[1.0,0.0,0.0],alpha:1.0});
            ids.push(CategoryId(x+y*width)); masks.push(MaskValue(marker));
        }}
        let channels=vec![
            PreparedExemplarChannel::BaseColor{plane:ImagePlane::from_row_major(width,height,4,&colors).unwrap(),alpha_mode:ResolvedAlphaMode::Opaque},
            PreparedExemplarChannel::Scalar{role:MaterialChannelRole::Roughness,plane:ImagePlane::from_row_major(width,height,4,&scalars).unwrap()},
            PreparedExemplarChannel::Normal{plane:ImagePlane::from_row_major(width,height,4,&normals).unwrap(),source_convention:NormalConvention::OpenGl,
                canonical_convention:NormalConvention::OpenGl,alpha_policy:NormalAlphaPolicy::Preserve},
            PreparedExemplarChannel::MaterialId{plane:ImagePlane::from_row_major(width,height,4,&ids).unwrap()},
            PreparedExemplarChannel::Mask{role:MaterialChannelRole::Opacity,plane:ImagePlane::from_row_major(width,height,4,&masks).unwrap()},
        ];
        let domain_id=ContentDigest::sha256(b"stage-14-domain"); let source_id=ContentDigest::sha256(b"stage-14-source");
        (PreparedMaterialDomain::from_registered_channels_with_route_and_seams(
            domain_id.clone(),source_id.clone(),channels,route,seams).unwrap(),domain_id,source_id)
    }

    fn plan(mode:SamplingMode,domain_id:ContentDigest,source_id:ContentDigest)->SamplingPlan {
        SamplingPlan { slot_id:RegionId::from_bytes([14;16]),role:TemplateSlotRole::Planar,variation_group:"stage-14".into(),
            prepared_domain_dimensions:[8,8], candidate:CropCandidate {
                candidate_id:ContentDigest::sha256(format!("{mode:?}").as_bytes()),source_id,domain_id:domain_id.clone(),
                slot_id:RegionId::from_bytes([14;16]),crop:Some(SourceCrop{x:0,y:0,width:8,height:8}),
                transform:CandidateTransform{rotation:QuarterTurn::Zero,mirror:MirrorTransform::None},isotropic_scale:1.0,
                mapping_mode:mode,family:CandidateFamily::PanelDirect,route:CandidateRoute::Direct,
                position_strategy:PositionStrategy::FeatureAware,period_pixels:Some([2,2]),seam_indices:Vec::new(),
                correspondence_reference:domain_id,descriptors:CandidateDescriptors{saliency_milli:0,stationarity_milli:1000,
                    feature_strength_milli:500,usability_milli:1000},seed:14,eligibility:EligibilityEvidence {
                    mapping_permitted:true,transform_permitted:true,isotropic_scale:true,exact_aspect:true,
                    entire_crop_usable:Some(true),cross_axis_preserved:Some(true),lattice_aligned:Some(true),
                    direct_crop_applicable:true,direct_crop_rejection:None,reasons:Vec::new()}},
            slot_physical_size:[1.0,1.0],source_pixels_per_physical_unit:8.0,
            sampling_policy:SamplingPolicy{filter:SourceSamplingMode::Nearest,scale:1.0,correct_tangent_normals:true},
            radial_mapping:None,
            stretch_override:if mode==SamplingMode::ExplicitStretch { StretchOverrideProvenance::UserOverride{settings_revision:14} }
                else { StretchOverrideProvenance::NotAuthorized },
            slice_geometry:match mode {
                SamplingMode::ThreeSliceCap=>SliceGeometry::Three{leading_cap_pixels:2,trailing_cap_pixels:2,center:SliceCenterPolicy::Repeat},
                SamplingMode::NineSlicePanel=>SliceGeometry::Nine{left_pixels:2,right_pixels:2,top_pixels:2,bottom_pixels:2,center:SliceCenterPolicy::Repeat},
                _=>SliceGeometry::None},maximum_seam_cost_milli:450,unary_cost:0.0 }
    }

    fn execute(mode:SamplingMode)->SynthesizedSlotMaterial { let (domain,d,s)=fixture(); let p=plan(mode,d,s);
        synthesize_slot_material(SlotSynthesisRequest{plan:&p,domain:&domain,output_dimensions:[8,8],
            limits:SlotSynthesisLimits::default()},&RenderCancellationToken::new()).unwrap() }

    fn channels(output:&SynthesizedSlotMaterial)->(&ImagePlane<LinearColor>,&ImagePlane<LinearScalar>,&ImagePlane<CategoryId>) {
        let PreparedExemplarChannel::BaseColor{plane:color,..}=&output.channels[0] else {panic!()};
        let PreparedExemplarChannel::Scalar{plane:scalar,..}=&output.channels[1] else {panic!()};
        let PreparedExemplarChannel::MaterialId{plane:id}=&output.channels[3] else {panic!()}; (color,scalar,id)
    }

    #[test]
    fn numbered_grid_crop_coordinates_are_preserved_through_stage_14() {
        let (domain, domain_id, source_id) = fixture();
        let mut selected = plan(SamplingMode::DirectCrop, domain_id, source_id);
        selected.candidate.crop = Some(SourceCrop { x: 2, y: 1, width: 4, height: 4 });
        selected.slot_physical_size = [1.0, 1.0];
        selected.source_pixels_per_physical_unit = 4.0;
        let output = synthesize_slot_material(SlotSynthesisRequest {
            plan: &selected, domain: &domain, output_dimensions: [4, 4], limits: SlotSynthesisLimits::default(),
        }, &RenderCancellationToken::new()).unwrap();
        assert_eq!(*output.correspondence.pixel(0, 0), [2.5, 1.5]);
        assert_eq!(*output.correspondence.pixel(3, 3), [5.5, 4.5]);
        let PreparedExemplarChannel::MaterialId { plane } = &output.channels[3] else { panic!() };
        assert_eq!(plane.pixel(0, 0).0, 10);
        assert_eq!(plane.pixel(3, 3).0, 37);
        assert!(output.correspondence.to_row_major().iter().all(|point|
            point[0] >= 2.5 && point[0] <= 5.5 && point[1] >= 1.5 && point[1] <= 4.5));
    }

    #[test]
    fn truthful_base_color_stage_14_registered_channels() {
        let modes=[SamplingMode::DirectCrop,SamplingMode::PeriodicTile,SamplingMode::RepeatX,SamplingMode::RepeatY,
            SamplingMode::UniqueContain,SamplingMode::UniqueCover,SamplingMode::ThreeSliceCap,
            SamplingMode::NineSlicePanel,SamplingMode::PlanarRadial,SamplingMode::PolarRadial,SamplingMode::ExplicitStretch];
        for mode in modes { let output=execute(mode); let (color,scalar,id)=channels(&output);
            for y in 0..output.height { for x in 0..output.width { if output.validity.pixel(x,y).0>=0.5 {
                let marker=id.pixel(x,y).0 as f32/63.0;
                assert!((color.pixel(x,y).rgb[0]-marker).abs()<1.0e-6,"color/ID drift in {mode:?}");
                assert!((scalar.pixel(x,y).0-marker).abs()<1.0e-6,"scalar/ID drift in {mode:?}");
            }}}
            assert_eq!(output.diagnostics.executed_mode,mode);
        }

        // Deliberately incompatible numeric units: one physical unit spans all eight source pixels.
        let physical=execute(SamplingMode::DirectCrop);
        assert_eq!(*physical.correspondence.pixel(0,4),[0.5,4.5]);
        assert_eq!(*physical.correspondence.pixel(7,4),[7.5,4.5]);

        // Authored cap widths survive exactly; period metadata is not consulted by slice execution.
        let sliced=execute(SamplingMode::ThreeSliceCap);
        let xs=(0..8).map(|x|sliced.correspondence.pixel(x,4)[0]).collect::<Vec<_>>();
        assert_eq!(&xs[..2],&[0.5,1.5]); assert_eq!(&xs[6..],&[6.5,7.5]);

        // Every legal mode applies the same quarter-turn to correspondence and all channels.
        for mode in modes { let baseline=execute(mode); let (domain,d,s)=fixture(); let mut transformed=plan(mode,d,s);
            transformed.candidate.transform.rotation=QuarterTurn::Ninety;
            transformed.candidate.transform.mirror=MirrorTransform::X;
            let output=synthesize_slot_material(SlotSynthesisRequest{plan:&transformed,domain:&domain,
                output_dimensions:[8,8],limits:SlotSynthesisLimits::default()},&RenderCancellationToken::new()).unwrap();
            assert_ne!(output.correspondence.to_row_major(),baseline.correspondence.to_row_major(),
                "{mode:?} ignored its selected transform");
            let (color,scalar,id)=channels(&output);
            for y in 0..8 { for x in 0..8 { if output.validity.pixel(x,y).0>=0.5 { let marker=id.pixel(x,y).0 as f32/63.0;
                assert!((color.pixel(x,y).rgb[0]-marker).abs()<1.0e-6,"transformed color drift in {mode:?}");
                assert!((scalar.pixel(x,y).0-marker).abs()<1.0e-6,"transformed scalar drift in {mode:?}");
            }}}
        }

        // All twelve quarter-turn/mirror combinations have an asserted physical correspondence on a non-square slot.
        let expected=[
            (QuarterTurn::Zero,MirrorTransform::None,[0.5,0.25]),(QuarterTurn::Ninety,MirrorTransform::None,[0.25,-0.5]),
            (QuarterTurn::OneEighty,MirrorTransform::None,[-0.5,-0.25]),(QuarterTurn::TwoSeventy,MirrorTransform::None,[-0.25,0.5]),
            (QuarterTurn::Zero,MirrorTransform::X,[-0.5,0.25]),(QuarterTurn::Ninety,MirrorTransform::X,[0.25,0.5]),
            (QuarterTurn::OneEighty,MirrorTransform::X,[0.5,-0.25]),(QuarterTurn::TwoSeventy,MirrorTransform::X,[-0.25,-0.5]),
            (QuarterTurn::Zero,MirrorTransform::Y,[0.5,-0.25]),(QuarterTurn::Ninety,MirrorTransform::Y,[-0.25,-0.5]),
            (QuarterTurn::OneEighty,MirrorTransform::Y,[-0.5,0.25]),(QuarterTurn::TwoSeventy,MirrorTransform::Y,[0.25,0.5]),
        ];
        for (rotation,mirror,offset) in expected { let (domain,d,s)=fixture(); let mut p=plan(SamplingMode::DirectCrop,d,s);
            p.slot_physical_size=[2.0,1.0];p.source_pixels_per_physical_unit=2.0;p.candidate.transform=CandidateTransform{rotation,mirror};
            let mapped=map_position(&SlotSynthesisRequest{plan:&p,domain:&domain,output_dimensions:[8,8],limits:SlotSynthesisLimits::default()},[0.75,0.75]);
            assert!((mapped.x-(4.0+offset[0]*2.0)).abs()<1.0e-9 && (mapped.y-(4.0+offset[1]*2.0)).abs()<1.0e-9);
        }

        // Physical direct mapping has a scalar Jacobian; a wide slot does not stretch X independently.
        let (domain,d,s)=fixture(); let mut p=plan(SamplingMode::DirectCrop,d,s);
        p.slot_physical_size=[2.0,1.0]; p.source_pixels_per_physical_unit=2.0;
        let direct=synthesize_slot_material(SlotSynthesisRequest{plan:&p,domain:&domain,output_dimensions:[12,6],
            limits:SlotSynthesisLimits::default()},&RenderCancellationToken::new()).unwrap();
        let a=direct.correspondence.pixel(5,2); let dx=direct.correspondence.pixel(6,2); let dy=direct.correspondence.pixel(5,3);
        assert!(((dx[0]-a[0]).abs()-(dy[1]-a[1]).abs()).abs()<1.0e-6);

        // Rotation transforms imported normals as vectors, never through the color sampler.
        let (domain,d,s)=fixture(); let mut rotated=plan(SamplingMode::DirectCrop,d,s);
        rotated.candidate.transform.rotation=QuarterTurn::Ninety;
        let output=synthesize_slot_material(SlotSynthesisRequest{plan:&rotated,domain:&domain,output_dimensions:[4,4],
            limits:SlotSynthesisLimits::default()},&RenderCancellationToken::new()).unwrap();
        let PreparedExemplarChannel::Normal{plane,..}=&output.channels[2] else {panic!()};
        assert_eq!(plane.pixel(2,2).xyz,[-0.0,1.0,0.0]);

        // Ordinary radial hotspots stay planar; polar correspondence is explicitly and observably different.
        assert_ne!(execute(SamplingMode::PlanarRadial).correspondence.to_row_major(),
            execute(SamplingMode::PolarRadial).correspondence.to_row_major());
        let (domain,d,s)=fixture(); let planar=plan(SamplingMode::PlanarRadial,d,s);
        let planar=synthesize_slot_material(SlotSynthesisRequest{plan:&planar,domain:&domain,output_dimensions:[16,8],
            limits:SlotSynthesisLimits::default()},&RenderCancellationToken::new()).unwrap();
        let center=planar.correspondence.pixel(8,4); let horizontal=planar.correspondence.pixel(12,4);
        let vertical=planar.correspondence.pixel(8,6);
        assert!(((horizontal[0]-center[0]).abs()-(vertical[1]-center[1]).abs()).abs()<1.0e-6,
            "planar radial mapping distorted a circular physical detail");

        // ExplicitStretch cannot be reached by failure or implicit provenance.
        let (domain,d,s)=fixture(); let mut stretch=plan(SamplingMode::ExplicitStretch,d,s); stretch.stretch_override=StretchOverrideProvenance::NotAuthorized;
        let error=synthesize_slot_material(SlotSynthesisRequest{plan:&stretch,domain:&domain,output_dimensions:[8,8],
            limits:SlotSynthesisLimits::default()},&RenderCancellationToken::new()).unwrap_err();
        assert_eq!(error,SlotSynthesisError::MissingExplicitStretchOverride);
        let StageResult::FailedWithRecovery{recovery_choices,..}=error.stage_result() else {panic!()};
        assert!(!recovery_choices.contains(&RecoveryChoice::ExplicitStretch));

        // Unsupported synthesis cannot silently use the generic centered branch.
        let (domain,d,s)=fixture(); let texture=plan(SamplingMode::TextureSynthesis,d,s);
        assert_eq!(synthesize_slot_material(SlotSynthesisRequest{plan:&texture,domain:&domain,output_dimensions:[8,8],
            limits:SlotSynthesisLimits::default()},&RenderCancellationToken::new()).unwrap_err(),SlotSynthesisError::InvalidPlan);

        // SamplingPolicy changes the raster correspondence and filter, proving it is authoritative.
        let (domain,d,s)=fixture(); let mut scaled=plan(SamplingMode::DirectCrop,d,s); scaled.sampling_policy.scale=0.5;
        let scaled=synthesize_slot_material(SlotSynthesisRequest{plan:&scaled,domain:&domain,output_dimensions:[4,4],
            limits:SlotSynthesisLimits::default()},&RenderCancellationToken::new()).unwrap();
        assert_ne!(scaled.correspondence.pixel(0,0),execute(SamplingMode::DirectCrop).correspondence.pixel(0,0));

        // Cancellation is observed between channel rows, not only during correspondence construction.
        let cancel=RenderCancellationToken::new(); let mut samples=0_usize;
        let raster=rasterize(&vec![[0.0,0.0];16],4,&|| cancel.is_cancelled(),|p|{samples+=1;if samples==4{cancel.cancel();}p});
        assert_eq!(raster.unwrap_err(),SlotSynthesisError::Cancelled);

        // Public/deserialized plans cannot smuggle malformed crop, period, slice, or seam geometry into rasterization.
        let (domain,d,s)=fixture(); let mut malformed=plan(SamplingMode::DirectCrop,d,s);
        malformed.candidate.crop=Some(SourceCrop{x:7,y:0,width:2,height:8});
        assert_eq!(synthesize_slot_material(SlotSynthesisRequest{plan:&malformed,domain:&domain,output_dimensions:[8,8],
            limits:SlotSynthesisLimits::default()},&RenderCancellationToken::new()).unwrap_err(),SlotSynthesisError::InvalidPlan);

        let (domain,d,s)=fixture(); let mut malformed=plan(SamplingMode::PeriodicTile,d,s); malformed.candidate.period_pixels=Some([0,2]);
        assert_eq!(synthesize_slot_material(SlotSynthesisRequest{plan:&malformed,domain:&domain,output_dimensions:[8,8],
            limits:SlotSynthesisLimits::default()},&RenderCancellationToken::new()).unwrap_err(),SlotSynthesisError::InvalidPlan);

        let (domain,d,s)=fixture(); let mut malformed=plan(SamplingMode::ThreeSliceCap,d,s);
        malformed.slice_geometry=SliceGeometry::Three{leading_cap_pixels:4,trailing_cap_pixels:4,center:SliceCenterPolicy::Repeat};
        assert_eq!(synthesize_slot_material(SlotSynthesisRequest{plan:&malformed,domain:&domain,output_dimensions:[8,8],
            limits:SlotSynthesisLimits::default()},&RenderCancellationToken::new()).unwrap_err(),SlotSynthesisError::InvalidPlan);

        let (domain,d,s)=fixture(); let mut malformed=plan(SamplingMode::DirectCrop,d,s); malformed.candidate.seam_indices.push(99);
        assert_eq!(synthesize_slot_material(SlotSynthesisRequest{plan:&malformed,domain:&domain,output_dimensions:[8,8],
            limits:SlotSynthesisLimits::default()},&RenderCancellationToken::new()).unwrap_err(),SlotSynthesisError::InvalidPlan);

        // Selected seam paths shift repeat phase per cross-axis sample and must satisfy axis, dimensions, and cost.
        let seam=SelectedSeam{axis:SeamAxis::X,positions:vec![0,1,2,3,0,1,2,3],normalized_cost_milli:100};
        let (domain,d,s)=fixture_with(DomainRoute::GraphCutPeriodicClosure,vec![seam]);
        let mut seamed=plan(SamplingMode::RepeatX,d,s);seamed.candidate.period_pixels=Some([4,2]);
        seamed.candidate.seam_indices=vec![0];seamed.maximum_seam_cost_milli=150;
        let output=synthesize_slot_material(SlotSynthesisRequest{plan:&seamed,domain:&domain,output_dimensions:[8,8],
            limits:SlotSynthesisLimits::default()},&RenderCancellationToken::new()).unwrap();
        for row in 0..8 {
            assert_eq!(output.correspondence.pixel(0,row)[0],0.5+f32::from((row%4) as u16),
                "seam phase used the wrong cross-axis texel at row {row}");
        }
        seamed.maximum_seam_cost_milli=99;
        assert_eq!(synthesize_slot_material(SlotSynthesisRequest{plan:&seamed,domain:&domain,output_dimensions:[8,8],
            limits:SlotSynthesisLimits::default()},&RenderCancellationToken::new()).unwrap_err(),SlotSynthesisError::InvalidPlan);
        let wrong_axis=SelectedSeam{axis:SeamAxis::Y,positions:vec![1;8],normalized_cost_milli:10};
        let (wrong_domain,d,s)=fixture_with(DomainRoute::GraphCutPeriodicClosure,vec![wrong_axis]);
        let mut wrong=plan(SamplingMode::RepeatX,d,s);wrong.candidate.seam_indices=vec![0];
        assert_eq!(synthesize_slot_material(SlotSynthesisRequest{plan:&wrong,domain:&wrong_domain,output_dimensions:[8,8],
            limits:SlotSynthesisLimits::default()},&RenderCancellationToken::new()).unwrap_err(),SlotSynthesisError::InvalidPlan);
        let short_path=SelectedSeam{axis:SeamAxis::X,positions:vec![1;7],normalized_cost_milli:10};
        let (short_domain,d,s)=fixture_with(DomainRoute::GraphCutPeriodicClosure,vec![short_path]);
        let mut short=plan(SamplingMode::RepeatX,d,s);short.candidate.seam_indices=vec![0];
        assert_eq!(synthesize_slot_material(SlotSynthesisRequest{plan:&short,domain:&short_domain,output_dimensions:[8,8],
            limits:SlotSynthesisLimits::default()},&RenderCancellationToken::new()).unwrap_err(),SlotSynthesisError::InvalidPlan);

        // Physical caps and corners must leave a strictly positive destination center on every sliced axis.
        let (domain,d,s)=fixture();let mut narrow=plan(SamplingMode::ThreeSliceCap,d,s);narrow.slot_physical_size=[0.4,1.0];
        assert_eq!(synthesize_slot_material(SlotSynthesisRequest{plan:&narrow,domain:&domain,output_dimensions:[8,8],
            limits:SlotSynthesisLimits::default()},&RenderCancellationToken::new()).unwrap_err(),SlotSynthesisError::InvalidPlan);
        let (domain,d,s)=fixture();let mut shallow=plan(SamplingMode::NineSlicePanel,d,s);shallow.slot_physical_size=[1.0,0.4];
        assert_eq!(synthesize_slot_material(SlotSynthesisRequest{plan:&shallow,domain:&domain,output_dimensions:[8,8],
            limits:SlotSynthesisLimits::default()},&RenderCancellationToken::new()).unwrap_err(),SlotSynthesisError::InvalidPlan);
        let (domain,d,s)=fixture();let mut reduced=plan(SamplingMode::NineSlicePanel,d,s);reduced.sampling_policy.scale=0.5;
        assert_eq!(synthesize_slot_material(SlotSynthesisRequest{plan:&reduced,domain:&domain,output_dimensions:[8,8],
            limits:SlotSynthesisLimits::default()},&RenderCancellationToken::new()).unwrap_err(),SlotSynthesisError::InvalidPlan);

        // Synthesize consumes an actual synthesized center domain and never publishes invalid remainder pixels.
        let (domain,d,s)=fixture_with(DomainRoute::TextureQuilting,Vec::new());
        let mut synthesized=plan(SamplingMode::ThreeSliceCap,d,s);
        synthesized.slice_geometry=SliceGeometry::Three{leading_cap_pixels:2,trailing_cap_pixels:2,center:SliceCenterPolicy::Synthesize};
        let output=synthesize_slot_material(SlotSynthesisRequest{plan:&synthesized,domain:&domain,output_dimensions:[8,8],
            limits:SlotSynthesisLimits::default()},&RenderCancellationToken::new()).unwrap();
        assert!(output.validity.to_row_major().iter().all(|value|value.0==1.0));
        let (direct,d,s)=fixture();let mut unavailable=plan(SamplingMode::ThreeSliceCap,d,s);
        unavailable.slice_geometry=synthesized.slice_geometry;
        assert_eq!(synthesize_slot_material(SlotSynthesisRequest{plan:&unavailable,domain:&direct,output_dimensions:[8,8],
            limits:SlotSynthesisLimits::default()},&RenderCancellationToken::new()).unwrap_err(),SlotSynthesisError::InsufficientSynthesizedCenter);

        // Nine-slice center stretch is explicit, localized, and carries visible user authorization.
        let (domain,d,s)=fixture();let mut panel=plan(SamplingMode::NineSlicePanel,d,s);
        panel.slot_physical_size=[2.0,1.0];panel.slice_geometry=SliceGeometry::Nine{left_pixels:2,right_pixels:2,
            top_pixels:2,bottom_pixels:2,center:SliceCenterPolicy::ExplicitStretch};
        panel.stretch_override=StretchOverrideProvenance::UserOverride{settings_revision:15};
        let output=synthesize_slot_material(SlotSynthesisRequest{plan:&panel,domain:&domain,output_dimensions:[16,8],
            limits:SlotSynthesisLimits::default()},&RenderCancellationToken::new()).unwrap();
        assert!(output.diagnostics.explicit_stretch_user_override);
        panel.stretch_override=StretchOverrideProvenance::NotAuthorized;
        assert_eq!(synthesize_slot_material(SlotSynthesisRequest{plan:&panel,domain:&domain,output_dimensions:[16,8],
            limits:SlotSynthesisLimits::default()},&RenderCancellationToken::new()).unwrap_err(),SlotSynthesisError::InvalidPlan);

        let (domain,d,s)=fixture(); let p=plan(SamplingMode::DirectCrop,d,s);
        let limited=SlotSynthesisLimits{max_pixels:4,..SlotSynthesisLimits::default()};
        assert_eq!(synthesize_slot_material(SlotSynthesisRequest{plan:&p,domain:&domain,output_dimensions:[8,8],limits:limited},
            &RenderCancellationToken::new()).unwrap_err(),SlotSynthesisError::ResourceLimitExceeded);
    }
}
