//! Stage 6 honest physical/relative scale and material-axis estimation.

use std::collections::BTreeMap;

use hot_trimmer_domain::{
    AlgorithmProvenance, CompilationDiagnostic, ContentDigest, DiagnosticCode,
    MaterialCalibrationIntent, PhysicalScaleEvidence, StageResult, WorldScaleAvailability,
};
use hot_trimmer_render_core::RenderCancellationToken;
use thiserror::Error;

use crate::{DelitPreparedExemplar, SourceAnalysisReport};

pub const STAGE_06_ALGORITHM_ID: &str = "hot_trimmer.physical_scale_orientation";
pub const STAGE_06_ALGORITHM_VERSION: &str = "6.0.0";
pub const DEFAULT_ORIENTATION_CONFIDENCE_THRESHOLD_MILLI: u16 = 220;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScaleOrientationSettings {
    pub local_grid_x: u8,
    pub local_grid_y: u8,
    pub orientation_confidence_threshold_milli: u16,
    /// Relative X/Y disagreement above this threshold is reported, never inferred from image aspect.
    pub anisotropic_scale_tolerance_milli: u16,
    pub max_working_bytes: u64,
    pub max_operations: u64,
}

impl Default for ScaleOrientationSettings {
    fn default() -> Self {
        Self {
            local_grid_x: 8,
            local_grid_y: 8,
            orientation_confidence_threshold_milli: DEFAULT_ORIENTATION_CONFIDENCE_THRESHOLD_MILLI,
            anisotropic_scale_tolerance_milli: 20,
            max_working_bytes: 536_870_912,
            max_operations: 1_000_000_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrientationAuthority {
    Measured,
    UserOverride,
    UnavailableLowConfidence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GlobalOrientation {
    /// A 180-degree-equivalent material axis. `None` prevents arbitrary low-confidence rotation.
    pub axis_millidegrees: Option<u32>,
    pub measured_axis_millidegrees: Option<u32>,
    pub anisotropy_milli: u16,
    pub confidence_milli: u16,
    pub authority: OrientationAuthority,
    pub destructive_rotation_allowed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalOrientationSample {
    /// Authoritative coordinates are source pixels, not viewport/display coordinates.
    pub source_x_milli: u64,
    pub source_y_milli: u64,
    pub axis_millidegrees: Option<u32>,
    pub anisotropy_milli: u16,
    pub confidence_milli: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScaleDiagnosticCode {
    WorldScaleUnavailable,
    PriorIsNotWorldAccurate,
    InconsistentAnisotropicScale,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScaleDiagnostic {
    pub code: ScaleDiagnosticCode,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverlayCoordinateSpace {
    SourcePixels,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MeasurementOverlay {
    pub coordinate_space: OverlayCoordinateSpace,
    pub source_pixels_per_meter_x_milli: Option<u64>,
    pub source_pixels_per_meter_y_milli: Option<u64>,
    pub world_scale_available: bool,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrientationOverlay {
    pub coordinate_space: OverlayCoordinateSpace,
    pub global: GlobalOrientation,
    pub local: Vec<LocalOrientationSample>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScaleOrientationReport {
    pub cache_key: ContentDigest,
    pub downstream_footprint_key: ContentDigest,
    /// Immutable source and Stage 5 keys prevent cross-source evidence composition.
    pub prepared_source_digest: ContentDigest,
    pub stage_five_cache_key: crate::SourceAnalysisCacheKey,
    pub scale: PhysicalScaleEvidence,
    pub scale_diagnostics: Vec<ScaleDiagnostic>,
    pub global_orientation: GlobalOrientation,
    pub local_orientation: Vec<LocalOrientationSample>,
    pub measurement_overlay: MeasurementOverlay,
    pub orientation_overlay: OrientationOverlay,
    pub stage_result: StageResult,
}

#[derive(Clone, Debug, Default)]
pub struct ScaleOrientationCache {
    entries: BTreeMap<ContentDigest, ScaleOrientationReport>,
}

impl ScaleOrientationCache {
    #[must_use]
    pub fn get(&self, key: &ContentDigest) -> Option<&ScaleOrientationReport> {
        self.entries.get(key)
    }

    pub fn insert_complete(&mut self, report: ScaleOrientationReport) {
        const MAX_ENTRIES: usize = 32;
        if self.entries.len() >= MAX_ENTRIES
            && !self.entries.contains_key(&report.cache_key)
            && let Some(oldest) = self.entries.keys().next().cloned()
        {
            self.entries.remove(&oldest);
        }
        self.entries.insert(report.cache_key.clone(), report);
    }
}

#[must_use]
pub fn scale_orientation_cache_key(
    stage_five: &SourceAnalysisReport,
    intent: &MaterialCalibrationIntent,
    settings: &ScaleOrientationSettings,
) -> ContentDigest {
    report_key(stage_five, intent, settings, false)
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ScaleOrientationError {
    #[error("Stage 6 settings are outside bounded ranges")]
    InvalidSettings,
    #[error("Stage 6 requires a non-empty Stage 4 Base Color")]
    EmptyInput,
    #[error("Stage 6 analysis was cancelled")]
    Cancelled,
    #[error(
        "Stage 6 requires {required_bytes} working bytes and {required_operations} operations, exceeding its declared limits"
    )]
    ResourceLimitExceeded {
        required_bytes: u64,
        required_operations: u64,
    },
}

pub fn calibrate_scale_orientation(
    source: &DelitPreparedExemplar,
    stage_five: &SourceAnalysisReport,
    intent: &MaterialCalibrationIntent,
    settings: &ScaleOrientationSettings,
    cancellation: &RenderCancellationToken,
) -> Result<ScaleOrientationReport, ScaleOrientationError> {
    validate_settings(settings)?;
    check_cancel(cancellation)?;
    let base = source.base_color();
    if base.width() < 3 || base.height() < 3 {
        return Err(ScaleOrientationError::EmptyInput);
    }
    preflight(base.width(), base.height(), settings)?;

    let mut luminance =
        Vec::with_capacity((u64::from(base.width()) * u64::from(base.height())) as usize);
    let mut valid = Vec::with_capacity(luminance.capacity());
    for y in 0..base.height() {
        if y % 32 == 0 {
            check_cancel(cancellation)?;
        }
        for x in 0..base.width() {
            let rgb = base.pixel(x, y).rgb;
            luminance.push(rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722);
            valid.push(match source.coverage.as_ref() {
                Some(coverage)
                    if coverage.width() == base.width() && coverage.height() == base.height() =>
                {
                    coverage.pixel(x, y).0 > 0.0
                }
                Some(_) => false,
                None => true,
            });
        }
    }
    let gradients = scharr_gradients(
        base.width(),
        base.height(),
        &luminance,
        &valid,
        cancellation,
    )?;
    drop(luminance);
    drop(valid);
    let integral = TensorIntegral::build(base.width(), base.height(), &gradients, cancellation)?;
    check_cancel(cancellation)?;
    let global_tensor = integral.query(1, base.width() - 1, 1, base.height() - 1);
    let (measured_axis, global_anisotropy, energy) = tensor_axis(global_tensor.tensor());
    let energy_confidence = energy_confidence(energy, global_tensor.sample_count);
    let measured_confidence =
        ((u32::from(global_anisotropy) * u32::from(energy_confidence)) / 1000) as u16;
    let measured_axis = (measured_confidence >= settings.orientation_confidence_threshold_milli)
        .then_some(measured_axis);
    let (axis, authority, destructive_rotation_allowed) =
        if let Some(overridden) = intent.orientation_override_millidegrees {
            (Some(overridden), OrientationAuthority::UserOverride, true)
        } else if let Some(axis) = measured_axis {
            (Some(axis), OrientationAuthority::Measured, true)
        } else {
            (None, OrientationAuthority::UnavailableLowConfidence, false)
        };
    let global_orientation = GlobalOrientation {
        axis_millidegrees: axis,
        measured_axis_millidegrees: measured_axis,
        anisotropy_milli: global_anisotropy,
        confidence_milli: measured_confidence,
        authority,
        destructive_rotation_allowed,
    };

    let local_orientation = local_field(
        base.width(),
        base.height(),
        &integral,
        settings,
        cancellation,
    )?;
    let scale = intent.scale;
    let scale_diagnostics = scale_diagnostics(scale, settings.anisotropic_scale_tolerance_milli);
    let measurement_overlay = MeasurementOverlay {
        coordinate_space: OverlayCoordinateSpace::SourcePixels,
        source_pixels_per_meter_x_milli: scale.source_pixels_per_meter_x_milli,
        source_pixels_per_meter_y_milli: scale.source_pixels_per_meter_y_milli,
        world_scale_available: scale.claims_world_accuracy(),
        label: scale_label(scale),
    };
    let orientation_overlay = OrientationOverlay {
        coordinate_space: OverlayCoordinateSpace::SourcePixels,
        global: global_orientation,
        local: local_orientation.clone(),
    };
    let cache_key = report_key(stage_five, intent, settings, false);
    let downstream_footprint_key = report_key(stage_five, intent, settings, true);
    let diagnostics = scale_diagnostics
        .iter()
        .map(|diagnostic| CompilationDiagnostic {
            code: DiagnosticCode::InsufficientInput,
            stage: Some(6),
            message: diagnostic.message.clone(),
            context: BTreeMap::new(),
        })
        .collect();
    Ok(ScaleOrientationReport {
        cache_key,
        downstream_footprint_key,
        prepared_source_digest: source.prepared_source_digest.clone(),
        stage_five_cache_key: stage_five.cache_key.clone(),
        scale,
        scale_diagnostics,
        global_orientation,
        local_orientation,
        measurement_overlay,
        orientation_overlay,
        stage_result: StageResult::Executed {
            algorithm: AlgorithmProvenance {
                algorithm_id: STAGE_06_ALGORITHM_ID.into(),
                version: STAGE_06_ALGORITHM_VERSION.into(),
            },
            settings_hash: settings_hash(settings),
            diagnostics,
        },
    })
}

fn validate_settings(settings: &ScaleOrientationSettings) -> Result<(), ScaleOrientationError> {
    if !(1..=32).contains(&settings.local_grid_x)
        || !(1..=32).contains(&settings.local_grid_y)
        || settings.orientation_confidence_threshold_milli > 1000
        || settings.anisotropic_scale_tolerance_milli > 1000
        || settings.max_working_bytes == 0
        || settings.max_operations == 0
    {
        Err(ScaleOrientationError::InvalidSettings)
    } else {
        Ok(())
    }
}

fn preflight(
    width: u32,
    height: u32,
    settings: &ScaleOrientationSettings,
) -> Result<(), ScaleOrientationError> {
    let pixels = u64::from(width).checked_mul(u64::from(height)).ok_or(
        ScaleOrientationError::ResourceLimitExceeded {
            required_bytes: u64::MAX,
            required_operations: u64::MAX,
        },
    )?;
    let integral_pixels = u64::from(width + 1)
        .checked_mul(u64::from(height + 1))
        .ok_or(ScaleOrientationError::ResourceLimitExceeded {
            required_bytes: u64::MAX,
            required_operations: u64::MAX,
        })?;
    // Peak: gradients plus four f64/count integral planes; luminance/coverage are dropped first.
    let required_bytes = pixels
        .checked_mul(16)
        .and_then(|value| value.checked_add(integral_pixels.checked_mul(32)?))
        .ok_or(ScaleOrientationError::ResourceLimitExceeded {
            required_bytes: u64::MAX,
            required_operations: u64::MAX,
        })?;
    let cells = u64::from(settings.local_grid_x) * u64::from(settings.local_grid_y);
    let required_operations = pixels
        .checked_mul(96)
        .and_then(|value| value.checked_add(cells.checked_mul(96)?))
        .ok_or(ScaleOrientationError::ResourceLimitExceeded {
            required_bytes,
            required_operations: u64::MAX,
        })?;
    if required_bytes > settings.max_working_bytes || required_operations > settings.max_operations
    {
        Err(ScaleOrientationError::ResourceLimitExceeded {
            required_bytes,
            required_operations,
        })
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct GradientSample {
    x: f32,
    y: f32,
    valid: bool,
}

fn scharr_gradients(
    width: u32,
    height: u32,
    values: &[f32],
    valid: &[bool],
    cancellation: &RenderCancellationToken,
) -> Result<Vec<GradientSample>, ScaleOrientationError> {
    let mut out = vec![GradientSample::default(); values.len()];
    let at = |x: u32, y: u32| values[(y * width + x) as usize];
    for y in 1..height - 1 {
        if y % 32 == 0 {
            check_cancel(cancellation)?;
        }
        for x in 1..width - 1 {
            let neighborhood_valid = (y - 1..=y + 1)
                .all(|sy| (x - 1..=x + 1).all(|sx| valid[(sy * width + sx) as usize]));
            if !neighborhood_valid {
                continue;
            }
            let gx = 3.0 * (at(x + 1, y - 1) - at(x - 1, y - 1))
                + 10.0 * (at(x + 1, y) - at(x - 1, y))
                + 3.0 * (at(x + 1, y + 1) - at(x - 1, y + 1));
            let gy = 3.0 * (at(x - 1, y + 1) - at(x - 1, y - 1))
                + 10.0 * (at(x, y + 1) - at(x, y - 1))
                + 3.0 * (at(x + 1, y + 1) - at(x + 1, y - 1));
            let magnitude = gx.hypot(gy);
            let cap = (8.0 / magnitude.max(1.0e-9)).min(1.0);
            out[(y * width + x) as usize] = GradientSample {
                x: gx * cap,
                y: gy * cap,
                valid: true,
            };
        }
    }
    Ok(out)
}

#[derive(Clone, Copy, Debug, Default)]
struct TensorStats {
    jxx: f64,
    jxy: f64,
    jyy: f64,
    sample_count: f64,
}

impl TensorStats {
    fn tensor(self) -> (f64, f64, f64) {
        (self.jxx, self.jxy, self.jyy)
    }
}

struct TensorIntegral {
    width: u32,
    height: u32,
    jxx: Vec<f64>,
    jxy: Vec<f64>,
    jyy: Vec<f64>,
    count: Vec<f64>,
}

impl TensorIntegral {
    fn build(
        width: u32,
        height: u32,
        gradients: &[GradientSample],
        cancellation: &RenderCancellationToken,
    ) -> Result<Self, ScaleOrientationError> {
        let stride = width + 1;
        let len = usize::try_from(u64::from(stride) * u64::from(height + 1)).map_err(|_| {
            ScaleOrientationError::ResourceLimitExceeded {
                required_bytes: u64::MAX,
                required_operations: u64::MAX,
            }
        })?;
        let mut result = Self {
            width,
            height,
            jxx: vec![0.0; len],
            jxy: vec![0.0; len],
            jyy: vec![0.0; len],
            count: vec![0.0; len],
        };
        for y in 0..height {
            if y % 32 == 0 {
                check_cancel(cancellation)?;
            }
            for x in 0..width {
                let gradient = gradients[(y * width + x) as usize];
                let values = if gradient.valid {
                    [
                        f64::from(gradient.x * gradient.x),
                        f64::from(gradient.x * gradient.y),
                        f64::from(gradient.y * gradient.y),
                        1.0,
                    ]
                } else {
                    [0.0; 4]
                };
                let index = ((y + 1) * stride + x + 1) as usize;
                let left = index - 1;
                let up = index - stride as usize;
                let diagonal = up - 1;
                result.jxx[index] =
                    values[0] + result.jxx[left] + result.jxx[up] - result.jxx[diagonal];
                result.jxy[index] =
                    values[1] + result.jxy[left] + result.jxy[up] - result.jxy[diagonal];
                result.jyy[index] =
                    values[2] + result.jyy[left] + result.jyy[up] - result.jyy[diagonal];
                result.count[index] =
                    values[3] + result.count[left] + result.count[up] - result.count[diagonal];
            }
        }
        Ok(result)
    }

    fn query(&self, x0: u32, x1: u32, y0: u32, y1: u32) -> TensorStats {
        let x0 = x0.min(self.width);
        let x1 = x1.min(self.width);
        let y0 = y0.min(self.height);
        let y1 = y1.min(self.height);
        let stride = self.width + 1;
        let indices = [
            (y1 * stride + x1) as usize,
            (y1 * stride + x0) as usize,
            (y0 * stride + x1) as usize,
            (y0 * stride + x0) as usize,
        ];
        let sum = |plane: &[f64]| {
            plane[indices[0]] - plane[indices[1]] - plane[indices[2]] + plane[indices[3]]
        };
        TensorStats {
            jxx: sum(&self.jxx),
            jxy: sum(&self.jxy),
            jyy: sum(&self.jyy),
            sample_count: sum(&self.count),
        }
    }
}

fn tensor_axis((jxx, jxy, jyy): (f64, f64, f64)) -> (u32, u16, f64) {
    let trace = jxx + jyy;
    if trace <= 1.0e-12 {
        return (0, 0, 0.0);
    }
    let discriminant = ((jxx - jyy).powi(2) + 4.0 * jxy * jxy).sqrt();
    let anisotropy = (discriminant / trace).clamp(0.0, 1.0);
    // Major eigenvector is the gradient normal; add 90 degrees to report the material axis.
    let gradient_angle = 0.5 * (2.0 * jxy).atan2(jxx - jyy);
    let axis =
        normalize_axis_millidegrees((gradient_angle + std::f64::consts::FRAC_PI_2).to_degrees());
    (axis, (anisotropy * 1000.0).round() as u16, trace)
}

fn energy_confidence(energy: f64, sample_count: f64) -> u16 {
    if sample_count <= 0.0 {
        return 0;
    }
    let mean = energy / sample_count;
    ((mean / (mean + 0.02)).clamp(0.0, 1.0) * 1000.0).round() as u16
}

fn local_field(
    width: u32,
    height: u32,
    integral: &TensorIntegral,
    settings: &ScaleOrientationSettings,
    cancellation: &RenderCancellationToken,
) -> Result<Vec<LocalOrientationSample>, ScaleOrientationError> {
    let mut result =
        Vec::with_capacity(usize::from(settings.local_grid_x) * usize::from(settings.local_grid_y));
    let cell_w = f64::from(width) / f64::from(settings.local_grid_x);
    let cell_h = f64::from(height) / f64::from(settings.local_grid_y);
    let base_radius = (cell_w.min(cell_h) * 0.5).round().max(2.0) as u32;
    for gy in 0..u32::from(settings.local_grid_y) {
        check_cancel(cancellation)?;
        for gx in 0..u32::from(settings.local_grid_x) {
            let cx = ((f64::from(gx) + 0.5) * cell_w)
                .round()
                .clamp(1.0, f64::from(width - 2)) as u32;
            let cy = ((f64::from(gy) + 0.5) * cell_h)
                .round()
                .clamp(1.0, f64::from(height - 2)) as u32;
            let mut combined = (0.0, 0.0, 0.0);
            let mut weighted_samples = 0.0;
            for multiplier in [1_u32, 2, 4] {
                let radius = base_radius.saturating_mul(multiplier);
                let tensor = integral.query(
                    cx.saturating_sub(radius).max(1),
                    (cx + radius + 1).min(width - 1),
                    cy.saturating_sub(radius).max(1),
                    (cy + radius + 1).min(height - 1),
                );
                let weight = 1.0 / f64::from(multiplier);
                combined.0 += tensor.jxx * weight;
                combined.1 += tensor.jxy * weight;
                combined.2 += tensor.jyy * weight;
                weighted_samples += tensor.sample_count * weight;
            }
            let (axis, anisotropy, energy) = tensor_axis(combined);
            let confidence = ((u32::from(anisotropy)
                * u32::from(energy_confidence(energy, weighted_samples)))
                / 1000) as u16;
            result.push(LocalOrientationSample {
                source_x_milli: u64::from(cx) * 1000,
                source_y_milli: u64::from(cy) * 1000,
                axis_millidegrees: (confidence >= settings.orientation_confidence_threshold_milli)
                    .then_some(axis),
                anisotropy_milli: anisotropy,
                confidence_milli: confidence,
            });
        }
    }
    Ok(result)
}

fn normalize_axis_millidegrees(degrees: f64) -> u32 {
    (degrees.rem_euclid(180.0) * 1000.0).round() as u32 % 180_000
}

fn scale_diagnostics(scale: PhysicalScaleEvidence, tolerance_milli: u16) -> Vec<ScaleDiagnostic> {
    let mut diagnostics = Vec::new();
    match scale.world_scale {
        WorldScaleAvailability::UnavailableRelativeOnly => diagnostics.push(ScaleDiagnostic {
            code: ScaleDiagnosticCode::WorldScaleUnavailable,
            message: "Relative material scale is usable, but world-size claims are unavailable."
                .into(),
        }),
        WorldScaleAvailability::UnavailablePriorEstimate => diagnostics.push(ScaleDiagnostic {
            code: ScaleDiagnosticCode::PriorIsNotWorldAccurate,
            message:
                "The material-class prior is only an estimate; world-size claims are unavailable."
                    .into(),
        }),
        WorldScaleAvailability::Available => {}
    }
    if let (Some(x), Some(y)) = (
        scale.source_pixels_per_meter_x_milli,
        scale.source_pixels_per_meter_y_milli,
    ) {
        let difference = x.abs_diff(y);
        let relative_milli = difference.saturating_mul(1000) / x.max(y).max(1);
        if relative_milli > u64::from(tolerance_milli) {
            diagnostics.push(ScaleDiagnostic {
                code: ScaleDiagnosticCode::InconsistentAnisotropicScale,
                message: format!(
                    "Declared physical sampling differs by {relative_milli} milli between X and Y; image aspect was not used to infer this anisotropy."
                ),
            });
        }
    }
    diagnostics
}

fn scale_label(scale: PhysicalScaleEvidence) -> String {
    match scale.world_scale {
        WorldScaleAvailability::Available => format!(
            "{} / {} px/m ({:?}, {}%)",
            scale.source_pixels_per_meter_x_milli.unwrap_or(0) / 1000,
            scale.source_pixels_per_meter_y_milli.unwrap_or(0) / 1000,
            scale.provenance,
            scale.confidence_milli / 10,
        ),
        WorldScaleAvailability::UnavailablePriorEstimate => {
            "Prior estimate only — world scale unavailable".into()
        }
        WorldScaleAvailability::UnavailableRelativeOnly => {
            "Relative scale only — world scale unavailable".into()
        }
    }
}

fn report_key(
    stage_five: &SourceAnalysisReport,
    intent: &MaterialCalibrationIntent,
    settings: &ScaleOrientationSettings,
    footprint: bool,
) -> ContentDigest {
    let scale = intent.scale;
    let bytes = format!(
        "{}|{}|{}|{:?}|{}|{:?}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        STAGE_06_ALGORITHM_VERSION,
        stage_five.cache_key.0.0,
        scale.source_pixels_per_meter_x_milli.unwrap_or(0),
        scale.source_pixels_per_meter_y_milli,
        scale.confidence_milli,
        scale.provenance,
        intent.revision,
        intent.orientation_override_millidegrees.unwrap_or(u32::MAX),
        settings.local_grid_x,
        settings.local_grid_y,
        settings.orientation_confidence_threshold_milli,
        settings.anisotropic_scale_tolerance_milli,
        settings.max_working_bytes,
        settings.max_operations,
        footprint,
    );
    ContentDigest::sha256(bytes.as_bytes())
}

fn settings_hash(settings: &ScaleOrientationSettings) -> ContentDigest {
    ContentDigest::sha256(
        format!(
            "{}|{}|{}|{}|{}|{}",
            settings.local_grid_x,
            settings.local_grid_y,
            settings.orientation_confidence_threshold_milli,
            settings.anisotropic_scale_tolerance_milli,
            settings.max_working_bytes,
            settings.max_operations
        )
        .as_bytes(),
    )
}

fn check_cancel(cancellation: &RenderCancellationToken) -> Result<(), ScaleOrientationError> {
    if cancellation.is_cancelled() {
        Err(ScaleOrientationError::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use hot_trimmer_domain::{
        DelightingPassThroughReason, MaterialCalibrationCommand, MaterialCorpusManifest,
        SourcePixelPointMilli, StageResult, SyntheticGenerator,
    };
    use hot_trimmer_image_io::{ImagePlane, LinearColor, MaskValue, ResolvedAlphaMode};
    use hot_trimmer_render_core::PreparedExemplarChannel;

    use super::*;
    use crate::{AnalysisSettings, ReflectanceProvenance, RouteExecution, analyze_source};

    fn source(width: u32, height: u32, signal: &[f32]) -> DelitPreparedExemplar {
        source_with_coverage(width, height, signal, &vec![true; signal.len()])
    }

    fn source_with_coverage(
        width: u32,
        height: u32,
        signal: &[f32],
        coverage: &[bool],
    ) -> DelitPreparedExemplar {
        let colors: Vec<_> = signal
            .iter()
            .map(|value| LinearColor {
                rgb: [*value; 3],
                alpha: 1.0,
            })
            .collect();
        let base = ImagePlane::from_row_major(width, height, 8, &colors).unwrap();
        DelitPreparedExemplar {
            exemplar_id: "stage-06-evidence".into(),
            prepared_source_digest: ContentDigest::sha256(b"stage-06-fixture"),
            perspective_confidence_milli: 1000,
            original_prepared_base_color: base.clone(),
            channels: vec![PreparedExemplarChannel::BaseColor {
                plane: base,
                alpha_mode: ResolvedAlphaMode::Opaque,
            }],
            coverage: Some(
                ImagePlane::from_row_major(
                    width,
                    height,
                    8,
                    &coverage
                        .iter()
                        .map(|valid| MaskValue(if *valid { 1.0 } else { 0.0 }))
                        .collect::<Vec<_>>(),
                )
                .unwrap(),
            ),
            masks: None,
            reflectance_provenance: ReflectanceProvenance::ImportedPrepared,
            route_execution: RouteExecution::PassThrough(
                DelightingPassThroughReason::AuthoredTextureOrPbrSet,
            ),
            upstream_stage_result: StageResult::PassThrough {
                reason: "planar".into(),
            },
            stage_result: StageResult::PassThrough {
                reason: "de-lit".into(),
            },
        }
    }

    fn axis_error(a: u32, b: u32) -> u32 {
        let difference = a.abs_diff(b) % 180_000;
        difference.min(180_000 - difference)
    }

    fn coordinate_noise(width: u32, height: u32, seed: u64) -> Vec<f32> {
        let mut result = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                let mut value = seed ^ (u64::from(x) << 32) ^ u64::from(y);
                value ^= value >> 30;
                value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
                value ^= value >> 27;
                value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
                value ^= value >> 31;
                result.push((value & 0xffff) as f32 / 65_535.0);
            }
        }
        result
    }

    #[test]
    fn algorithm_stage_06_scale_orientation() {
        let cancellation = RenderCancellationToken::new();
        let settings = ScaleOrientationSettings::default();
        let corpus = MaterialCorpusManifest::bundled().unwrap();
        let directional = corpus
            .synthetic_fixtures
            .iter()
            .find(|fixture| fixture.generator == SyntheticGenerator::Orientation)
            .unwrap()
            .generate();
        let values: Vec<_> = directional.planes["base_color"]
            .iter()
            .map(|value| f32::from(*value) / 65_535.0)
            .collect();
        let directional_source = source(directional.spec.width, directional.spec.height, &values);
        let stage_five = analyze_source(
            &directional_source,
            &AnalysisSettings::default(),
            None,
            &cancellation,
        )
        .unwrap();
        let report = calibrate_scale_orientation(
            &directional_source,
            &stage_five,
            &MaterialCalibrationIntent::default(),
            &settings,
            &cancellation,
        )
        .unwrap();
        let expected = ((directional.spec.expected_properties["rise"] as f64)
            .atan2(directional.spec.expected_properties["run"] as f64)
            .to_degrees()
            * 1000.0)
            .round() as u32;
        assert!(
            axis_error(
                report.global_orientation.axis_millidegrees.unwrap(),
                expected
            ) <= 5_000,
            "directional corpus axis {:?} must be within 5 degrees of {expected}",
            report.global_orientation
        );
        assert!(
            report.global_orientation.confidence_milli
                >= settings.orientation_confidence_threshold_milli
        );
        assert!(
            report
                .local_orientation
                .iter()
                .filter(|sample| sample.axis_millidegrees.is_some())
                .count()
                > report.local_orientation.len() / 2
        );

        for (width, height) in [(64, 64), (96, 48), (48, 96)] {
            for seed in [1, 2, 5, 17] {
                let noise = coordinate_noise(width, height, seed);
                let noisy_source = source(width, height, &noise);
                let noisy_stage_five = analyze_source(
                    &noisy_source,
                    &AnalysisSettings::default(),
                    None,
                    &cancellation,
                )
                .unwrap();
                let noisy = calibrate_scale_orientation(
                    &noisy_source,
                    &noisy_stage_five,
                    &MaterialCalibrationIntent::default(),
                    &settings,
                    &cancellation,
                )
                .unwrap();
                assert_eq!(
                    noisy.global_orientation.axis_millidegrees, None,
                    "2D noise seed {seed} at {width}x{height} must not produce an axis: {:?}",
                    noisy.global_orientation
                );
                assert!(!noisy.global_orientation.destructive_rotation_allowed);
                assert_eq!(
                    noisy
                        .scale_diagnostics
                        .iter()
                        .filter(|d| d.code == ScaleDiagnosticCode::InconsistentAnisotropicScale)
                        .count(),
                    0,
                    "rectangular image aspect must not become physical anisotropy"
                );
            }
        }
        let noise = coordinate_noise(96, 48, 29);
        let noisy_source = source(96, 48, &noise);
        let noisy_stage_five = analyze_source(
            &noisy_source,
            &AnalysisSettings::default(),
            None,
            &cancellation,
        )
        .unwrap();
        let noisy = calibrate_scale_orientation(
            &noisy_source,
            &noisy_stage_five,
            &MaterialCalibrationIntent::default(),
            &settings,
            &cancellation,
        )
        .unwrap();
        assert_eq!(noisy.scale.provenance, ScaleProvenance::RelativeOnly);
        assert!(!noisy.scale.claims_world_accuracy());

        // An invalid padded half contains a strong boundary, but coverage prevents it from
        // becoming measured orientation authority.
        let mut padded = vec![0.25; 64 * 64];
        let mut covered = vec![true; 64 * 64];
        for y in 0..64 {
            for x in 32..64 {
                padded[y * 64 + x] = if y % 2 == 0 { 1.0 } else { 0.0 };
                covered[y * 64 + x] = false;
            }
        }
        let padded_source = source_with_coverage(64, 64, &padded, &covered);
        let padded_stage_five = analyze_source(
            &padded_source,
            &AnalysisSettings::default(),
            None,
            &cancellation,
        )
        .unwrap();
        let padded_report = calibrate_scale_orientation(
            &padded_source,
            &padded_stage_five,
            &MaterialCalibrationIntent::default(),
            &settings,
            &cancellation,
        )
        .unwrap();
        assert_eq!(padded_report.global_orientation.axis_millidegrees, None);
        assert!(
            !padded_report
                .global_orientation
                .destructive_rotation_allowed
        );

        // Local energy normalization follows the weighted window samples, not full image size.
        let grid_four = ScaleOrientationSettings {
            local_grid_x: 4,
            local_grid_y: 4,
            ..settings
        };
        let grid_eight = ScaleOrientationSettings {
            local_grid_x: 8,
            local_grid_y: 8,
            ..settings
        };
        let four = calibrate_scale_orientation(
            &directional_source,
            &stage_five,
            &MaterialCalibrationIntent::default(),
            &grid_four,
            &cancellation,
        )
        .unwrap();
        let eight = calibrate_scale_orientation(
            &directional_source,
            &stage_five,
            &MaterialCalibrationIntent::default(),
            &grid_eight,
            &cancellation,
        )
        .unwrap();
        let mean_confidence = |samples: &[LocalOrientationSample]| -> u32 {
            samples
                .iter()
                .map(|sample| u32::from(sample.confidence_milli))
                .sum::<u32>()
                / samples.len() as u32
        };
        assert!(
            mean_confidence(&four.local_orientation)
                .abs_diff(mean_confidence(&eight.local_orientation))
                <= 75
        );

        let limited = ScaleOrientationSettings {
            max_working_bytes: 1,
            ..settings
        };
        assert!(matches!(
            calibrate_scale_orientation(
                &directional_source,
                &stage_five,
                &MaterialCalibrationIntent::default(),
                &limited,
                &cancellation,
            ),
            Err(ScaleOrientationError::ResourceLimitExceeded { .. })
        ));
        let cancelled = RenderCancellationToken::new();
        cancelled.cancel();
        assert_eq!(
            calibrate_scale_orientation(
                &directional_source,
                &stage_five,
                &MaterialCalibrationIntent::default(),
                &settings,
                &cancelled,
            ),
            Err(ScaleOrientationError::Cancelled)
        );

        let mut intent = MaterialCalibrationIntent::default();
        intent
            .apply(MaterialCalibrationCommand::MeasureTwoPoints {
                start: SourcePixelPointMilli {
                    x: 10_000,
                    y: 20_000,
                },
                end: SourcePixelPointMilli {
                    x: 110_000,
                    y: 20_000,
                },
                distance_micrometers: 250_000,
            })
            .unwrap();
        assert_eq!(intent.scale.source_pixels_per_meter_x_milli, Some(400_000));
        assert_eq!(
            intent.scale.source_pixels_per_meter_x_milli,
            intent.scale.source_pixels_per_meter_y_milli,
            "two-point isotropic measurement must round-trip identically on X/Y"
        );
        let measured = calibrate_scale_orientation(
            &directional_source,
            &stage_five,
            &intent,
            &settings,
            &cancellation,
        )
        .unwrap();
        assert!(measured.scale.claims_world_accuracy());
        assert_ne!(
            measured.downstream_footprint_key,
            report.downstream_footprint_key
        );
        intent
            .apply(MaterialCalibrationCommand::ResetScale)
            .unwrap();
        let reset = calibrate_scale_orientation(
            &directional_source,
            &stage_five,
            &intent,
            &settings,
            &cancellation,
        )
        .unwrap();
        assert!(!reset.scale.claims_world_accuracy());
        assert_ne!(
            reset.downstream_footprint_key,
            measured.downstream_footprint_key
        );

        intent
            .apply(MaterialCalibrationCommand::OverrideScale {
                source_pixels_per_meter_x_milli: Some(500_000),
                source_pixels_per_meter_y_milli: Some(250_000),
                provenance: ScaleProvenance::PriorEstimated,
                confidence_milli: 150,
            })
            .unwrap();
        let prior = calibrate_scale_orientation(
            &directional_source,
            &stage_five,
            &intent,
            &settings,
            &cancellation,
        )
        .unwrap();
        assert_eq!(
            prior.scale.world_scale,
            WorldScaleAvailability::UnavailablePriorEstimate
        );
        assert!(!prior.measurement_overlay.world_scale_available);
        assert!(
            prior
                .scale_diagnostics
                .iter()
                .any(|d| d.code == ScaleDiagnosticCode::InconsistentAnisotropicScale)
        );

        intent
            .apply(MaterialCalibrationCommand::OverrideOrientation {
                axis_millidegrees: 179_000,
            })
            .unwrap();
        let overridden = calibrate_scale_orientation(
            &noisy_source,
            &noisy_stage_five,
            &intent,
            &settings,
            &cancellation,
        )
        .unwrap();
        assert_eq!(
            overridden.global_orientation.authority,
            OrientationAuthority::UserOverride
        );
        assert!(overridden.global_orientation.destructive_rotation_allowed);
        assert_eq!(
            overridden.orientation_overlay.coordinate_space,
            OverlayCoordinateSpace::SourcePixels
        );
    }
}
