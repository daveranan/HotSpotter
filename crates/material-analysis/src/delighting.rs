//! Stage 4 explicit, reversible de-lighting. The only input authority is a Stage 3
//! [`PreparedExemplar`]; no source names or channel inventory participate in routing.

use std::collections::BTreeMap;

use hot_trimmer_domain::{
    AlgorithmProvenance, CompilationDiagnostic, ContentDigest, DelightingIntent,
    DelightingPassThroughReason, DelightingRadius, DelightingRouteIntent, DiagnosticCode,
    IntrinsicProviderFallback, RecoveryChoice, StageResult,
};
use hot_trimmer_image_io::{ImagePlane, LinearColor, MaskValue};
use hot_trimmer_render_core::{
    PreparedExemplar, PreparedExemplarChannel, RenderCancellationToken,
};
use thiserror::Error;

pub const STAGE_04_ALGORITHM_ID: &str = "hot_trimmer.classical_low_frequency_delighting";
pub const STAGE_04_ALGORITHM_VERSION: &str = "4.0.0";
pub const INTRINSIC_PROVIDER_INTERFACE_VERSION: u16 = 1;
pub const MAX_FILTER_RADIUS_PIXELS: u32 = 256;
const EPSILON: f32 = 1.0e-5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DelightingWorkLimits {
    pub max_operations: u64,
    pub max_working_bytes: u64,
}

impl Default for DelightingWorkLimits {
    fn default() -> Self {
        Self {
            max_operations: 1_000_000_000,
            max_working_bytes: 1_073_741_824,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReflectanceProvenance {
    ImportedPrepared,
    Estimated,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RouteExecution {
    PassThrough(DelightingPassThroughReason),
    ClassicalLowFrequency,
    IntrinsicProvider { provider_id: String, provider_version: String },
    ProviderUnavailable { provider_id: String, fallback: IntrinsicProviderFallback },
}

#[derive(Clone, Debug, PartialEq)]
pub struct DelightingMasks {
    pub highlight: ImagePlane<MaskValue>,
    pub shadow: ImagePlane<MaskValue>,
    pub clipping: ImagePlane<MaskValue>,
    /// Low values intentionally preserve pigment-versus-shadow ambiguity.
    pub confidence: ImagePlane<MaskValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DelitPreparedExemplar {
    pub exemplar_id: String,
    /// Immutable Stage 3 prepared-source lineage shared by all downstream analysis.
    pub prepared_source_digest: ContentDigest,
    /// Stage 3 geometric confidence in `[0,1000]`, carried without reinterpretation.
    pub perspective_confidence_milli: u16,
    /// Immutable Stage 3 Base Color retained for exact restoration.
    pub original_prepared_base_color: ImagePlane<LinearColor>,
    pub channels: Vec<PreparedExemplarChannel>,
    pub coverage: Option<ImagePlane<MaskValue>>,
    pub masks: Option<DelightingMasks>,
    pub reflectance_provenance: ReflectanceProvenance,
    pub route_execution: RouteExecution,
    pub upstream_stage_result: StageResult,
    pub stage_result: StageResult,
}

impl DelitPreparedExemplar {
    #[must_use]
    pub fn base_color(&self) -> &ImagePlane<LinearColor> {
        self.channels.iter().find_map(|channel| match channel {
            PreparedExemplarChannel::BaseColor { plane, .. } => Some(plane),
            _ => None,
        }).expect("Stage 4 output is constructed only from an exemplar with Base Color")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntrinsicProviderDescriptor {
    pub provider_id: String,
    pub provider_version: String,
    pub interface_version: u16,
    pub model_digest: ContentDigest,
}

pub struct IntrinsicProviderRequest<'a> {
    pub base_color: &'a ImagePlane<LinearColor>,
    pub analyze_masks: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IntrinsicProviderResult {
    pub reflectance: ImagePlane<LinearColor>,
    pub masks: Option<DelightingMasks>,
    /// Provider-defined deterministic diagnostics, kept in sorted key order.
    pub diagnostics: BTreeMap<String, String>,
}

/// Versioned local-only intrinsic-image boundary. Implementations must be deterministic for
/// their descriptor, model digest, input plane, and request.
pub trait LocalIntrinsicProvider {
    fn descriptor(&self) -> IntrinsicProviderDescriptor;
    fn infer(
        &self,
        request: IntrinsicProviderRequest<'_>,
        cancellation: &RenderCancellationToken,
    ) -> Result<IntrinsicProviderResult, IntrinsicProviderError>;
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum IntrinsicProviderError {
    #[error("intrinsic provider execution was cancelled")]
    Cancelled,
    #[error("intrinsic provider failed deterministically: {0}")]
    Failed(String),
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum DelightingError {
    #[error("the Stage 3 exemplar has no Base Color")]
    MissingBaseColor,
    #[error("Stage 4 settings are outside their bounded range")]
    InvalidSettings,
    #[error("Stage 4 work was cancelled")]
    Cancelled,
    #[error("Stage 4 requires {required_operations} operations and {required_bytes} working bytes, exceeding its declared limits")]
    ResourceLimitExceeded {
        required_operations: u64,
        required_bytes: u64,
    },
    #[error("the intrinsic provider returned malformed or incompatible data")]
    InvalidProviderResult,
    #[error("the Stage 4 image plane could not be constructed")]
    PlaneConstruction,
}

/// Executes only the persisted route. `None` means no local model is installed.
pub fn prepare_delit_exemplar(
    exemplar: &PreparedExemplar,
    intent: &DelightingIntent,
    provider: Option<&dyn LocalIntrinsicProvider>,
    cancellation: &RenderCancellationToken,
) -> Result<DelitPreparedExemplar, DelightingError> {
    prepare_delit_exemplar_with_limits(
        exemplar,
        intent,
        provider,
        DelightingWorkLimits::default(),
        cancellation,
    )
}

/// Executes Stage 4 under explicit deterministic operation and allocation limits.
pub fn prepare_delit_exemplar_with_limits(
    exemplar: &PreparedExemplar,
    intent: &DelightingIntent,
    provider: Option<&dyn LocalIntrinsicProvider>,
    limits: DelightingWorkLimits,
    cancellation: &RenderCancellationToken,
) -> Result<DelitPreparedExemplar, DelightingError> {
    let base = base_color(exemplar)?;
    match &intent.route {
        DelightingRouteIntent::PassThrough { reason } => {
            Ok(pass_through(exemplar, base.clone(), *reason, None))
        }
        DelightingRouteIntent::ClassicalLowFrequency => {
            preflight_classical(base, intent, limits)?;
            classical(exemplar, base.clone(), intent, cancellation, RouteExecution::ClassicalLowFrequency, Vec::new())
        }
        DelightingRouteIntent::LocalIntrinsicProvider { provider_id, fallback } => {
            if let Some(provider) = provider {
                let descriptor = provider.descriptor();
                if descriptor.provider_id == *provider_id
                    && descriptor.interface_version == INTRINSIC_PROVIDER_INTERFACE_VERSION
                {
                    return execute_provider(exemplar, base.clone(), provider, descriptor, intent, cancellation);
                }
            }
            provider_unavailable(exemplar, base, intent, provider_id, *fallback, limits, cancellation)
        }
    }
}

fn provider_unavailable(
    exemplar: &PreparedExemplar,
    original: &ImagePlane<LinearColor>,
    intent: &DelightingIntent,
    provider_id: &str,
    fallback: IntrinsicProviderFallback,
    limits: DelightingWorkLimits,
    cancellation: &RenderCancellationToken,
) -> Result<DelitPreparedExemplar, DelightingError> {
    let diagnostic = unavailable_diagnostic(provider_id);
    let execution = RouteExecution::ProviderUnavailable { provider_id: provider_id.to_owned(), fallback };
    match fallback {
        IntrinsicProviderFallback::ClassicalLowFrequency => {
            preflight_classical(original, intent, limits)?;
            classical(exemplar, original.clone(), intent, cancellation, execution, vec![diagnostic])
        }
        IntrinsicProviderFallback::PassThrough => {
            Ok(pass_through(exemplar, original.clone(), DelightingPassThroughReason::UserDisabled, Some((execution, diagnostic))))
        }
        IntrinsicProviderFallback::None => {
            let mut result = pass_through(
                exemplar,
                original.clone(),
                DelightingPassThroughReason::UserDisabled,
                Some((execution, diagnostic.clone())),
            );
            result.stage_result = StageResult::FailedWithRecovery {
                reason: diagnostic,
                recovery_choices: vec![RecoveryChoice::AdjustSettings],
            };
            Ok(result)
        }
    }
}

fn execute_provider(
    exemplar: &PreparedExemplar,
    original: ImagePlane<LinearColor>,
    provider: &dyn LocalIntrinsicProvider,
    descriptor: IntrinsicProviderDescriptor,
    intent: &DelightingIntent,
    cancellation: &RenderCancellationToken,
) -> Result<DelitPreparedExemplar, DelightingError> {
    if descriptor.provider_id.is_empty()
        || descriptor.provider_version.is_empty()
        || descriptor.model_digest.0.len() != 64
        || !descriptor.model_digest.0.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(DelightingError::InvalidProviderResult);
    }
    let result = provider.infer(
        IntrinsicProviderRequest { base_color: &original, analyze_masks: intent.classical.analyze_masks },
        cancellation,
    ).map_err(|error| match error {
        IntrinsicProviderError::Cancelled => DelightingError::Cancelled,
        IntrinsicProviderError::Failed(_) => DelightingError::InvalidProviderResult,
    })?;
    if !valid_provider_result(
        &result,
        exemplar.width,
        exemplar.height,
        intent.classical.analyze_masks,
    ) {
        return Err(DelightingError::InvalidProviderResult);
    }
    let channels = replace_base_color(&exemplar.channels, result.reflectance);
    let settings_hash = provider_settings_hash(&descriptor, intent);
    Ok(DelitPreparedExemplar {
        exemplar_id: exemplar.exemplar_id.clone(),
        prepared_source_digest: exemplar.cache_key.0.clone(),
        perspective_confidence_milli: exemplar.perspective_confidence_milli,
        original_prepared_base_color: original,
        channels,
        coverage: exemplar.usable_mask.clone(),
        masks: result.masks,
        reflectance_provenance: ReflectanceProvenance::Estimated,
        route_execution: RouteExecution::IntrinsicProvider {
            provider_id: descriptor.provider_id.clone(),
            provider_version: descriptor.provider_version.clone(),
        },
        upstream_stage_result: exemplar.stage_result.clone(),
        stage_result: StageResult::Executed {
            algorithm: AlgorithmProvenance {
                algorithm_id: descriptor.provider_id,
                version: descriptor.provider_version,
            },
            settings_hash,
            diagnostics: Vec::new(),
        },
    })
}

fn pass_through(
    exemplar: &PreparedExemplar,
    original: ImagePlane<LinearColor>,
    reason: DelightingPassThroughReason,
    unavailable: Option<(RouteExecution, CompilationDiagnostic)>,
) -> DelitPreparedExemplar {
    let (route_execution, stage_result) = unavailable.map_or_else(
        || (
            RouteExecution::PassThrough(reason),
            StageResult::PassThrough { reason: pass_through_reason(reason).into() },
        ),
        |(execution, diagnostic)| (
            execution,
            StageResult::FailedWithRecovery {
                reason: diagnostic,
                recovery_choices: vec![RecoveryChoice::AdjustSettings],
            },
        ),
    );
    DelitPreparedExemplar {
        exemplar_id: exemplar.exemplar_id.clone(),
        prepared_source_digest: exemplar.cache_key.0.clone(),
        perspective_confidence_milli: exemplar.perspective_confidence_milli,
        original_prepared_base_color: original,
        channels: exemplar.channels.clone(),
        coverage: exemplar.usable_mask.clone(),
        masks: None,
        reflectance_provenance: ReflectanceProvenance::ImportedPrepared,
        route_execution,
        upstream_stage_result: exemplar.stage_result.clone(),
        stage_result,
    }
}

fn classical(
    exemplar: &PreparedExemplar,
    original: ImagePlane<LinearColor>,
    intent: &DelightingIntent,
    cancellation: &RenderCancellationToken,
    route_execution: RouteExecution,
    diagnostics: Vec<CompilationDiagnostic>,
) -> Result<DelitPreparedExemplar, DelightingError> {
    validate_settings(intent)?;
    let width = original.width();
    let height = original.height();
    let radius = resolve_radius(intent.classical.radius, width, height)?;
    let count = usize::try_from(u64::from(width) * u64::from(height)).map_err(|_| DelightingError::InvalidSettings)?;
    let mut log_luminance = Vec::with_capacity(count);
    for y in 0..height {
        if cancellation.is_cancelled() { return Err(DelightingError::Cancelled); }
        for x in 0..width {
            log_luminance.push(luminance(original.pixel(x, y)).max(EPSILON).ln());
        }
    }
    let edge = f32::from(intent.classical.edge_preservation_milli) / 1000.0;
    let horizontal = bilateral_axis(&log_luminance, width, height, radius, edge, true, cancellation)?;
    let illumination = bilateral_axis(&horizontal, width, height, radius, edge, false, cancellation)?;
    let mean_illumination = illumination.iter().copied().sum::<f32>() / count as f32;
    let strength = f32::from(intent.classical.strength_milli) / 1000.0;
    let shadow_recovery = f32::from(intent.classical.shadow_recovery_milli) / 1000.0;
    let highlight_recovery = f32::from(intent.classical.highlight_recovery_milli) / 1000.0;
    let color_preservation = f32::from(intent.classical.color_preservation_milli) / 1000.0;
    let mut corrected = Vec::with_capacity(count);
    for y in 0..height {
        if cancellation.is_cancelled() { return Err(DelightingError::Cancelled); }
        for x in 0..width {
            let index = index(width, x, y);
            let low = illumination[index];
            let recovery = if low < mean_illumination { shadow_recovery } else { highlight_recovery };
            // The edge-preserving illumination estimate can itself retain reflectance steps.
            // Attenuating only the correction field at those steps recombines the structural
            // residual instead of dividing it out of the estimated reflectance.
            let edge_attenuation = structural_edge_attenuation(
                &log_luminance, width, height, x, y, edge,
            );
            let correction = ((mean_illumination - low)
                * strength
                * (1.0 + recovery)
                * edge_attenuation)
                .clamp(-1.386_294_4, 1.386_294_4)
                .exp();
            let source = *original.pixel(x, y);
            let source_luminance = luminance(&source).max(EPSILON);
            let target_luminance = (source_luminance * correction).clamp(0.0, 1.0);
            let neutral = [target_luminance; 3];
            let preserved = source.rgb.map(|component| (component * correction).clamp(0.0, 1.0));
            let rgb = std::array::from_fn(|component| {
                neutral[component] * (1.0 - color_preservation) + preserved[component] * color_preservation
            });
            corrected.push(LinearColor { rgb, alpha: source.alpha });
        }
    }
    let plane = ImagePlane::from_row_major(width, height, original.tile_edge(), &corrected)
        .map_err(|_| DelightingError::PlaneConstruction)?;
    let masks = intent.classical.analyze_masks.then(|| build_masks(&original, &illumination, mean_illumination)).transpose()?;
    let channels = replace_base_color(&exemplar.channels, plane);
    Ok(DelitPreparedExemplar {
        exemplar_id: exemplar.exemplar_id.clone(),
        prepared_source_digest: exemplar.cache_key.0.clone(),
        perspective_confidence_milli: exemplar.perspective_confidence_milli,
        original_prepared_base_color: original,
        channels,
        coverage: exemplar.usable_mask.clone(),
        masks,
        reflectance_provenance: ReflectanceProvenance::Estimated,
        route_execution,
        upstream_stage_result: exemplar.stage_result.clone(),
        stage_result: StageResult::Executed {
            algorithm: AlgorithmProvenance { algorithm_id: STAGE_04_ALGORITHM_ID.into(), version: STAGE_04_ALGORITHM_VERSION.into() },
            settings_hash: classical_settings_hash(intent, radius),
            diagnostics,
        },
    })
}

fn bilateral_axis(
    source: &[f32], width: u32, height: u32, radius: u32, edge_preservation: f32,
    horizontal: bool, cancellation: &RenderCancellationToken,
) -> Result<Vec<f32>, DelightingError> {
    let mut output = vec![0.0; source.len()];
    let range_scale = 2.0 + edge_preservation * 62.0;
    for y in 0..height {
        if cancellation.is_cancelled() { return Err(DelightingError::Cancelled); }
        for x in 0..width {
            let center = source[index(width, x, y)];
            let coordinate = if horizontal { x } else { y };
            let maximum = if horizontal { width - 1 } else { height - 1 };
            let start = coordinate.saturating_sub(radius);
            let end = coordinate.saturating_add(radius).min(maximum);
            let mut weighted = 0.0;
            let mut weight_sum = 0.0;
            for sample in start..=end {
                let (sx, sy) = if horizontal { (sample, y) } else { (x, sample) };
                let value = source[index(width, sx, sy)];
                let distance = coordinate.abs_diff(sample) as f32 / radius.max(1) as f32;
                let spatial_weight = 1.0 - 0.5 * distance;
                let difference = value - center;
                let range_weight = 1.0 / (1.0 + difference * difference * range_scale);
                let weight = spatial_weight * range_weight;
                weighted += value * weight;
                weight_sum += weight;
            }
            output[index(width, x, y)] = weighted / weight_sum.max(EPSILON);
        }
    }
    Ok(output)
}

fn preflight_classical(
    original: &ImagePlane<LinearColor>,
    intent: &DelightingIntent,
    limits: DelightingWorkLimits,
) -> Result<(), DelightingError> {
    validate_settings(intent)?;
    let radius = resolve_radius(intent.classical.radius, original.width(), original.height())?;
    let pixels = u64::from(original.width())
        .checked_mul(u64::from(original.height()))
        .ok_or(DelightingError::InvalidSettings)?;
    // Two separable passes each visit at most 2r+1 samples. The fixed allowance covers
    // luminance conversion, structural-edge detection, correction, and optional masks.
    let operations_per_pixel = u64::from(radius)
        .checked_mul(4)
        .and_then(|value| value.checked_add(42))
        .ok_or(DelightingError::InvalidSettings)?;
    let required_operations = pixels
        .checked_mul(operations_per_pixel)
        .ok_or(DelightingError::InvalidSettings)?;
    // Conservative peak accounting includes the retained original, three f32 work fields,
    // corrected row-major and tiled color, four row-major+tiled masks, coverage, and tile/Vec
    // overhead. Existing Stage 3 input storage is not charged again.
    let bytes_per_pixel = if intent.classical.analyze_masks { 128_u64 } else { 88_u64 };
    let required_bytes = pixels
        .checked_mul(bytes_per_pixel)
        .ok_or(DelightingError::InvalidSettings)?;
    if required_operations > limits.max_operations || required_bytes > limits.max_working_bytes {
        return Err(DelightingError::ResourceLimitExceeded {
            required_operations,
            required_bytes,
        });
    }
    Ok(())
}

fn structural_edge_attenuation(
    log_luminance: &[f32],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    edge_preservation: f32,
) -> f32 {
    let center = log_luminance[index(width, x, y)];
    let mut maximum_difference = 0.0_f32;
    if x > 0 {
        maximum_difference = maximum_difference.max((center - log_luminance[index(width, x - 1, y)]).abs());
    }
    if x + 1 < width {
        maximum_difference = maximum_difference.max((center - log_luminance[index(width, x + 1, y)]).abs());
    }
    if y > 0 {
        maximum_difference = maximum_difference.max((center - log_luminance[index(width, x, y - 1)]).abs());
    }
    if y + 1 < height {
        maximum_difference = maximum_difference.max((center - log_luminance[index(width, x, y + 1)]).abs());
    }
    let threshold = 0.35 + (0.08 - 0.35) * edge_preservation;
    let normalized = maximum_difference / threshold.max(EPSILON);
    let squared = normalized * normalized;
    1.0 / (1.0 + squared * squared)
}

fn valid_provider_result(
    result: &IntrinsicProviderResult,
    width: u32,
    height: u32,
    masks_required: bool,
) -> bool {
    if result.reflectance.width() != width
        || result.reflectance.height() != height
        || result.reflectance.tiles().iter().flat_map(|tile| &tile.pixels).any(|color| {
            !color.alpha.is_finite()
                || !(0.0..=1.0).contains(&color.alpha)
                || color.rgb.iter().any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
        })
    {
        return false;
    }
    let Some(masks) = &result.masks else {
        return !masks_required;
    };
    [&masks.highlight, &masks.shadow, &masks.clipping, &masks.confidence]
        .into_iter()
        .all(|plane| {
            plane.width() == width
                && plane.height() == height
                && plane.tiles().iter().flat_map(|tile| &tile.pixels).all(|value| {
                    value.0.is_finite() && (0.0..=1.0).contains(&value.0)
                })
        })
}

fn build_masks(
    original: &ImagePlane<LinearColor>, illumination: &[f32], mean: f32,
) -> Result<DelightingMasks, DelightingError> {
    let mut highlight = Vec::new();
    let mut shadow = Vec::new();
    let mut clipping = Vec::new();
    let mut confidence = Vec::new();
    for y in 0..original.height() {
        for x in 0..original.width() {
            let color = original.pixel(x, y);
            let lum = luminance(color);
            let low = illumination[index(original.width(), x, y)];
            let highlight_value = smoothstep(0.75, 0.98, lum);
            let shadow_value = smoothstep(0.35, 0.02, lum) * smoothstep(mean - 0.05, mean - 0.7, low);
            let clipped = if color.rgb.iter().any(|value| *value <= EPSILON || *value >= 1.0 - EPSILON) { 1.0 } else { 0.0 };
            let ambiguity = (shadow_value * (1.0 - clipped)).max(highlight_value * 0.75);
            highlight.push(MaskValue(highlight_value));
            shadow.push(MaskValue(shadow_value));
            clipping.push(MaskValue(clipped));
            confidence.push(MaskValue((1.0 - ambiguity).clamp(0.0, 1.0)));
        }
    }
    let make = |values: &[MaskValue]| ImagePlane::from_row_major(
        original.width(), original.height(), original.tile_edge(), values,
    ).map_err(|_| DelightingError::PlaneConstruction);
    Ok(DelightingMasks {
        highlight: make(&highlight)?, shadow: make(&shadow)?, clipping: make(&clipping)?, confidence: make(&confidence)?,
    })
}

fn base_color(exemplar: &PreparedExemplar) -> Result<&ImagePlane<LinearColor>, DelightingError> {
    exemplar.channels.iter().find_map(|channel| match channel {
        PreparedExemplarChannel::BaseColor { plane, .. } => Some(plane),
        _ => None,
    }).ok_or(DelightingError::MissingBaseColor)
}

fn replace_base_color(channels: &[PreparedExemplarChannel], plane: ImagePlane<LinearColor>) -> Vec<PreparedExemplarChannel> {
    let mut replacement = Some(plane);
    channels.iter().map(|channel| match channel {
        PreparedExemplarChannel::BaseColor { alpha_mode, .. } => PreparedExemplarChannel::BaseColor {
            plane: replacement.take().expect("one registered Base Color"), alpha_mode: *alpha_mode,
        },
        _ => channel.clone(),
    }).collect()
}

fn validate_settings(intent: &DelightingIntent) -> Result<(), DelightingError> {
    let settings = intent.classical;
    if settings.strength_milli > 1000 || settings.shadow_recovery_milli > 1000
        || settings.highlight_recovery_milli > 1000 || settings.color_preservation_milli > 1000
        || settings.edge_preservation_milli > 1000
    { return Err(DelightingError::InvalidSettings); }
    Ok(())
}

fn resolve_radius(radius: DelightingRadius, width: u32, height: u32) -> Result<u32, DelightingError> {
    let pixels = match radius {
        DelightingRadius::Pixels(value) => u32::from(value),
        DelightingRadius::RelativeBasisPoints(value) => {
            u32::from(width.min(height)).saturating_mul(u32::from(value)).div_ceil(10_000)
        }
        DelightingRadius::PhysicalMillimeters { millimeters_milli, pixels_per_meter_milli } => {
            u64::from(millimeters_milli).saturating_mul(u64::from(pixels_per_meter_milli)).div_ceil(1_000_000_000) as u32
        }
    };
    if pixels == 0 || pixels > MAX_FILTER_RADIUS_PIXELS { return Err(DelightingError::InvalidSettings); }
    Ok(pixels)
}

fn classical_settings_hash(intent: &DelightingIntent, radius: u32) -> ContentDigest {
    let settings = intent.classical;
    ContentDigest::sha256(format!(
        "{STAGE_04_ALGORITHM_VERSION}|{}|{}|{}|{}|{}|{radius}|{}",
        settings.strength_milli, settings.shadow_recovery_milli, settings.highlight_recovery_milli,
        settings.color_preservation_milli, settings.edge_preservation_milli, settings.analyze_masks,
    ).as_bytes())
}

fn provider_settings_hash(descriptor: &IntrinsicProviderDescriptor, intent: &DelightingIntent) -> ContentDigest {
    ContentDigest::sha256(format!(
        "{}|{}|{}|{}|{:?}", descriptor.provider_id, descriptor.provider_version,
        descriptor.interface_version, descriptor.model_digest.0, intent.classical,
    ).as_bytes())
}

fn unavailable_diagnostic(provider_id: &str) -> CompilationDiagnostic {
    CompilationDiagnostic {
        code: DiagnosticCode::InsufficientInput,
        stage: Some(4),
        message: format!("Local intrinsic provider '{provider_id}' is unavailable; learned inference was not executed."),
        context: BTreeMap::from([("providerId".into(), provider_id.into())]),
    }
}

const fn pass_through_reason(reason: DelightingPassThroughReason) -> &'static str {
    match reason {
        DelightingPassThroughReason::DefaultNewOrUnclassified => "default PassThrough for new or unclassified source",
        DelightingPassThroughReason::AuthoredTextureOrPbrSet => "authored texture or PBR set is already de-lit",
        DelightingPassThroughReason::UserDisabled => "user disabled de-lighting; original prepared Base Color restored",
    }
}

fn luminance(color: &LinearColor) -> f32 {
    color.rgb[0].max(0.0) * 0.2126 + color.rgb[1].max(0.0) * 0.7152 + color.rgb[2].max(0.0) * 0.0722
}

fn index(width: u32, x: u32, y: u32) -> usize {
    usize::try_from(u64::from(y) * u64::from(width) + u64::from(x)).expect("validated plane index")
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let denominator = edge1 - edge0;
    if denominator.abs() < EPSILON { return 0.0; }
    let t = ((value - edge0) / denominator).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use hot_trimmer_domain::{
        ClassicalDelightingSettings, MaterialChannelRole, SourceSetId,
    };
    use hot_trimmer_image_io::{CategoryId, LinearScalar, ResolvedAlphaMode};
    use hot_trimmer_render_core::{
        PreparedExemplarCacheKey, PreparedExemplarScope,
    };

    use super::*;

    fn fixture() -> PreparedExemplar {
        let width = 64;
        let height = 32;
        let mut color = Vec::new();
        let mut scalar = Vec::new();
        let mut ids = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let illumination = 0.35 + 0.6 * x as f32 / (width - 1) as f32;
                let structure = if (x / 8 + y / 8) % 2 == 0 { 0.36 } else { 0.62 };
                color.push(LinearColor {
                    rgb: [structure * illumination, structure * 0.8 * illumination, structure * 0.6 * illumination],
                    alpha: 1.0,
                });
                scalar.push(LinearScalar(x as f32 / width as f32));
                ids.push(CategoryId((x / 8 + y / 8) % 2));
            }
        }
        let plane = |pixels: &[LinearColor]| ImagePlane::from_row_major(width, height, 8, pixels).unwrap();
        PreparedExemplar {
            exemplar_id: "uneven-light".into(),
            cache_key: PreparedExemplarCacheKey(ContentDigest::sha256(b"stage-4-fixture")),
            scope: PreparedExemplarScope {
                source_set_id: SourceSetId::from_bytes([4; 16]), source_revision: 1,
                patch_id: None, patch_revision: 0,
            },
            width,
            height,
            channels: vec![
                PreparedExemplarChannel::BaseColor { plane: plane(&color), alpha_mode: ResolvedAlphaMode::Straight },
                PreparedExemplarChannel::Scalar {
                    role: MaterialChannelRole::Roughness,
                    plane: ImagePlane::from_row_major(width, height, 8, &scalar).unwrap(),
                },
                PreparedExemplarChannel::MaterialId {
                    plane: ImagePlane::from_row_major(width, height, 8, &ids).unwrap(),
                },
            ],
            usable_mask: Some(ImagePlane::from_row_major(width, height, 8, &vec![MaskValue(1.0); (width * height) as usize]).unwrap()),
            perspective_confidence_milli: 900,
            geometry_digest: ContentDigest::sha256(b"geometry"),
            coordinate_field_digest: ContentDigest::sha256(b"coordinates"),
            stage_result: StageResult::PassThrough { reason: "fixture already planar".into() },
        }
    }

    fn classical_intent() -> DelightingIntent {
        DelightingIntent {
            route: DelightingRouteIntent::ClassicalLowFrequency,
            classical: ClassicalDelightingSettings {
                strength_milli: 850,
                shadow_recovery_milli: 150,
                highlight_recovery_milli: 100,
                color_preservation_milli: 1000,
                edge_preservation_milli: 900,
                radius: DelightingRadius::Pixels(10),
                analyze_masks: true,
            },
        }
    }

    fn region_mean(plane: &ImagePlane<LinearColor>, start: u32, end: u32) -> f32 {
        let mut total = 0.0;
        let mut count = 0;
        for y in 0..plane.height() {
            for x in start..end {
                total += luminance(plane.pixel(x, y));
                count += 1;
            }
        }
        total / count as f32
    }

    fn scalar_and_ids(exemplar: &DelitPreparedExemplar) -> (&ImagePlane<LinearScalar>, &ImagePlane<CategoryId>) {
        let scalar = exemplar.channels.iter().find_map(|channel| match channel {
            PreparedExemplarChannel::Scalar { plane, .. } => Some(plane), _ => None,
        }).unwrap();
        let ids = exemplar.channels.iter().find_map(|channel| match channel {
            PreparedExemplarChannel::MaterialId { plane } => Some(plane), _ => None,
        }).unwrap();
        (scalar, ids)
    }

    struct MalformedProvider {
        invalid_color: bool,
        invalid_mask_dimensions: bool,
    }

    impl LocalIntrinsicProvider for MalformedProvider {
        fn descriptor(&self) -> IntrinsicProviderDescriptor {
            IntrinsicProviderDescriptor {
                provider_id: "malformed-provider".into(),
                provider_version: "1.0.0".into(),
                interface_version: INTRINSIC_PROVIDER_INTERFACE_VERSION,
                model_digest: ContentDigest::sha256(b"malformed-provider-model"),
            }
        }

        fn infer(
            &self,
            request: IntrinsicProviderRequest<'_>,
            _cancellation: &RenderCancellationToken,
        ) -> Result<IntrinsicProviderResult, IntrinsicProviderError> {
            let width = request.base_color.width();
            let height = request.base_color.height();
            let mut colors = request.base_color.to_row_major();
            if self.invalid_color {
                colors[0].rgb[0] = f32::NAN;
            }
            let reflectance = ImagePlane::from_row_major(width, height, 8, &colors).unwrap();
            let mask_width = if self.invalid_mask_dimensions { 1 } else { width };
            let mask_height = if self.invalid_mask_dimensions { 1 } else { height };
            let values = vec![MaskValue(1.0); usize::try_from(mask_width * mask_height).unwrap()];
            let mask = || ImagePlane::from_row_major(mask_width, mask_height, 8, &values).unwrap();
            Ok(IntrinsicProviderResult {
                reflectance,
                masks: Some(DelightingMasks {
                    highlight: mask(), shadow: mask(), clipping: mask(), confidence: mask(),
                }),
                diagnostics: BTreeMap::new(),
            })
        }
    }

    #[test]
    fn algorithm_stage_04_delighting() {
        let exemplar = fixture();
        let cancellation = RenderCancellationToken::new();

        // New/unclassified and authored inputs are exact, typed PassThrough; settings are not evaluated.
        let default_result = prepare_delit_exemplar(&exemplar, &DelightingIntent::default(), None, &cancellation).unwrap();
        assert_eq!(default_result.base_color(), base_color(&exemplar).unwrap());
        assert_eq!(default_result.original_prepared_base_color, *base_color(&exemplar).unwrap());
        assert!(default_result.masks.is_none());
        assert!(matches!(default_result.route_execution, RouteExecution::PassThrough(DelightingPassThroughReason::DefaultNewOrUnclassified)));
        let authored = DelightingIntent {
            route: DelightingRouteIntent::PassThrough { reason: DelightingPassThroughReason::AuthoredTextureOrPbrSet },
            classical: ClassicalDelightingSettings { radius: DelightingRadius::Pixels(0), ..ClassicalDelightingSettings::default() },
        };
        let authored_result = prepare_delit_exemplar(&exemplar, &authored, None, &cancellation).unwrap();
        assert_eq!(authored_result.base_color(), base_color(&exemplar).unwrap());

        // Explicit classical routing reduces the illumination gradient while preserving structural contrast.
        let first = prepare_delit_exemplar(&exemplar, &classical_intent(), None, &cancellation).unwrap();
        let second = prepare_delit_exemplar(&exemplar, &classical_intent(), None, &cancellation).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.reflectance_provenance, ReflectanceProvenance::Estimated);
        let before_gradient = (region_mean(base_color(&exemplar).unwrap(), 56, 64) - region_mean(base_color(&exemplar).unwrap(), 0, 8)).abs();
        let after_gradient = (region_mean(first.base_color(), 56, 64) - region_mean(first.base_color(), 0, 8)).abs();
        assert!(after_gradient < before_gradient * 0.65, "{after_gradient} !< {before_gradient}");
        let before_edge = (luminance(base_color(&exemplar).unwrap().pixel(8, 16)) - luminance(base_color(&exemplar).unwrap().pixel(7, 16))).abs();
        let after_edge = (luminance(first.base_color().pixel(8, 16)) - luminance(first.base_color().pixel(7, 16))).abs();
        assert!(after_edge >= before_edge * 0.7, "structural edge was over-smoothed");
        let masks = first.masks.as_ref().unwrap();
        assert!(masks.confidence.to_row_major().iter().any(|value| value.0 < 0.9));
        let (source_scalar, source_ids) = {
            let source = DelitPreparedExemplar {
                exemplar_id: String::new(), original_prepared_base_color: base_color(&exemplar).unwrap().clone(),
                prepared_source_digest: exemplar.cache_key.0.clone(),
                perspective_confidence_milli: exemplar.perspective_confidence_milli,
                channels: exemplar.channels.clone(), coverage: None, masks: None,
                reflectance_provenance: ReflectanceProvenance::ImportedPrepared,
                route_execution: RouteExecution::PassThrough(DelightingPassThroughReason::DefaultNewOrUnclassified),
                upstream_stage_result: exemplar.stage_result.clone(), stage_result: exemplar.stage_result.clone(),
            };
            let (scalar, ids) = scalar_and_ids(&source);
            (scalar.clone(), ids.clone())
        };
        let (output_scalar, output_ids) = scalar_and_ids(&first);
        assert_eq!(output_scalar, &source_scalar);
        assert_eq!(output_ids, &source_ids);

        // Turning the route off restores the retained Stage 3 pixels without recomputation ambiguity.
        let disabled = DelightingIntent {
            route: DelightingRouteIntent::PassThrough { reason: DelightingPassThroughReason::UserDisabled },
            classical: classical_intent().classical,
        };
        let restored = prepare_delit_exemplar(&exemplar, &disabled, None, &cancellation).unwrap();
        assert_eq!(restored.base_color(), base_color(&exemplar).unwrap());
        assert!(matches!(restored.stage_result, StageResult::PassThrough { ref reason } if reason.contains("user disabled")));

        // Missing learned inference is explicit and never silently selects classical work.
        let unavailable = DelightingIntent {
            route: DelightingRouteIntent::LocalIntrinsicProvider {
                provider_id: "not-installed".into(), fallback: IntrinsicProviderFallback::None,
            },
            classical: classical_intent().classical,
        };
        let unavailable_result = prepare_delit_exemplar(&exemplar, &unavailable, None, &cancellation).unwrap();
        assert_eq!(unavailable_result.base_color(), base_color(&exemplar).unwrap());
        assert!(matches!(unavailable_result.route_execution, RouteExecution::ProviderUnavailable { fallback: IntrinsicProviderFallback::None, .. }));
        assert!(matches!(unavailable_result.stage_result, StageResult::FailedWithRecovery { .. }));

        // Classical work fails before allocation when either deterministic budget is exceeded.
        let bounded = prepare_delit_exemplar_with_limits(
            &exemplar,
            &classical_intent(),
            None,
            DelightingWorkLimits { max_operations: 1, max_working_bytes: 1 },
            &cancellation,
        );
        assert!(matches!(bounded, Err(DelightingError::ResourceLimitExceeded { .. })));

        // Installed providers cannot publish non-finite color or malformed analysis masks.
        let provider_intent = DelightingIntent {
            route: DelightingRouteIntent::LocalIntrinsicProvider {
                provider_id: "malformed-provider".into(), fallback: IntrinsicProviderFallback::None,
            },
            classical: classical_intent().classical,
        };
        let invalid_color = MalformedProvider { invalid_color: true, invalid_mask_dimensions: false };
        assert!(matches!(
            prepare_delit_exemplar(&exemplar, &provider_intent, Some(&invalid_color), &cancellation),
            Err(DelightingError::InvalidProviderResult),
        ));
        let invalid_masks = MalformedProvider { invalid_color: false, invalid_mask_dimensions: true };
        assert!(matches!(
            prepare_delit_exemplar(&exemplar, &provider_intent, Some(&invalid_masks), &cancellation),
            Err(DelightingError::InvalidProviderResult),
        ));

        // A valid installed provider executes before its dormant classical fallback is validated.
        let installed_with_invalid_fallback = DelightingIntent {
            route: DelightingRouteIntent::LocalIntrinsicProvider {
                provider_id: "malformed-provider".into(),
                fallback: IntrinsicProviderFallback::ClassicalLowFrequency,
            },
            classical: ClassicalDelightingSettings {
                radius: DelightingRadius::Pixels(0),
                ..classical_intent().classical
            },
        };
        let valid_provider = MalformedProvider { invalid_color: false, invalid_mask_dimensions: false };
        let provider_result = prepare_delit_exemplar(
            &exemplar,
            &installed_with_invalid_fallback,
            Some(&valid_provider),
            &cancellation,
        ).unwrap();
        assert!(matches!(provider_result.route_execution, RouteExecution::IntrinsicProvider { .. }));
        assert!(matches!(
            prepare_delit_exemplar(&exemplar, &installed_with_invalid_fallback, None, &cancellation),
            Err(DelightingError::InvalidSettings),
        ));
    }
}
