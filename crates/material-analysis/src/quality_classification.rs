//! Stage 5 bounded source-quality measurement and material-behavior analysis.
//!
//! Public report fields use integer units so serialized/cached evidence is stable:
//! scores and confidences are `[0, 1000]` milli-units, fractions are `[0, 1_000_000]`
//! parts-per-million, shifts are signed pixels, and material resolution is effective pixels
//! on the shorter source axis.

use std::collections::{BTreeMap, BTreeSet};

use hot_trimmer_domain::{
    AlgorithmProvenance, CompilationDiagnostic, ContentDigest, DiagnosticCode,
    MaterialClassificationCommand, MaterialClassificationIntent, RecoveryChoice, StageResult,
};
use hot_trimmer_image_io::TangentNormal;
use hot_trimmer_render_core::{PreparedExemplarChannel, RenderCancellationToken};
use thiserror::Error;

use crate::DelitPreparedExemplar;

pub const STAGE_05_ALGORITHM_ID: &str = "hot_trimmer.source_quality_material_behavior";
pub const STAGE_05_ALGORITHM_VERSION: &str = "5.0.0";
pub const CLASSIFIER_PROVIDER_INTERFACE_VERSION: u16 = 1;
pub const MAX_ANALYSIS_EDGE: u32 = 256;
const SCORE_MAX: u16 = 1000;

pub use hot_trimmer_domain::MaterialBehaviorClass;
pub type ClassificationCommand = MaterialClassificationCommand;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QualityThresholds {
    pub minimum_sharpness_milli: u16,
    pub maximum_noise_milli: u16,
    pub maximum_compression_milli: u16,
    pub minimum_dynamic_range_milli: u16,
    pub maximum_clipped_ppm: u32,
    pub minimum_perspective_confidence_milli: u16,
    pub minimum_usable_area_ppm: u32,
    pub minimum_registration_quality_milli: u16,
    pub minimum_material_resolution_px: u32,
}

impl Default for QualityThresholds {
    fn default() -> Self {
        Self {
            minimum_sharpness_milli: 90,
            maximum_noise_milli: 420,
            maximum_compression_milli: 350,
            minimum_dynamic_range_milli: 80,
            maximum_clipped_ppm: 80_000,
            minimum_perspective_confidence_milli: 550,
            minimum_usable_area_ppm: 350_000,
            minimum_registration_quality_milli: 650,
            minimum_material_resolution_px: 128,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClassificationTolerance {
    /// A top non-unknown class below this confidence remains Mixed/Unknown.
    pub minimum_hard_class_confidence_milli: u16,
    /// A top-two separation below this many milli-units is declared ambiguous.
    pub minimum_top_margin_milli: u16,
}

impl Default for ClassificationTolerance {
    fn default() -> Self {
        Self {
            minimum_hard_class_confidence_milli: 350,
            minimum_top_margin_milli: 35,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnalysisSettings {
    pub thresholds: QualityThresholds,
    pub tolerance: ClassificationTolerance,
    pub max_analysis_edge: u32,
    pub max_registration_shift_px: u8,
}

impl Default for AnalysisSettings {
    fn default() -> Self {
        Self {
            thresholds: QualityThresholds::default(),
            tolerance: ClassificationTolerance::default(),
            max_analysis_edge: MAX_ANALYSIS_EDGE,
            max_registration_shift_px: 4,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceQualityReport {
    /// Mean four-neighbor luminance gradient, milli-normalized to `[0,1000]`.
    pub sharpness_milli: u16,
    /// High-frequency residual not explained by coherent edges, `[0,1000]`.
    pub noise_milli: u16,
    /// Excess discontinuity on 8-pixel block boundaries, `[0,1000]`.
    pub compression_milli: u16,
    /// Luminance P95-P05 in linear-light milli-units, `[0,1000]`.
    pub dynamic_range_milli: u16,
    /// Samples with any Base Color component at 0 or 1, parts-per-million.
    pub clipped_pixels_ppm: u32,
    /// Stage 3 geometric confidence, `[0,1000]`.
    pub perspective_confidence_milli: u16,
    /// Coverage/alpha-valid samples, parts-per-million.
    pub usable_area_ppm: u32,
    /// Worst registered companion-channel zero-shift correspondence, `[0,1000]`.
    pub registration_quality_milli: u16,
    /// Offset with strongest companion correspondence; `(0,0)` is registered.
    pub registration_worst_offset_px: (i8, i8),
    /// Effective usable detail samples on the shorter axis, in pixels.
    pub estimated_material_resolution_px: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QualityWarningCode {
    Soft,
    Noisy,
    Compressed,
    NarrowDynamicRange,
    Clipped,
    LowPerspectiveConfidence,
    LimitedUsableArea,
    MisregisteredChannels,
    LowMaterialResolution,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QualityWarning {
    pub code: QualityWarningCode,
    pub message: String,
    pub recoveries: Vec<RecoveryChoice>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BehaviorMeasurements {
    pub boundary_agreement_milli: u16,
    pub orientation_coherence_milli: u16,
    pub orientation_variation_milli: u16,
    pub periodic_x_milli: u16,
    pub periodic_y_milli: u16,
    pub bandedness_milli: u16,
    pub regularity_milli: u16,
    pub localized_saliency_milli: u16,
    pub radial_symmetry_milli: u16,
    pub stationarity_milli: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RankedClassEvidence {
    pub class: MaterialBehaviorClass,
    /// Normalized class probability in `[0,1000]`; distribution sums to 1000.
    pub probability_milli: u16,
    /// Unnormalized deterministic support strength in `[0,1000]`.
    pub support_milli: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClassificationAuthority {
    DeterministicHeuristics,
    LocalProvider,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterialClassification {
    /// Evidence-selected class; ambiguity is represented as Mixed/Unknown.
    pub analyzed_class: MaterialBehaviorClass,
    pub confidence_milli: u16,
    pub distribution: Vec<RankedClassEvidence>,
    pub measurements: BehaviorMeasurements,
    pub authority: ClassificationAuthority,
    /// Routing-only user intent. It never changes `analyzed_class`, evidence, or quality.
    pub routing_intent: MaterialClassificationIntent,
}

impl MaterialClassification {
    #[must_use]
    pub const fn routed_class(&self) -> MaterialBehaviorClass {
        match self.routing_intent.override_class {
            Some(class) => class,
            None => self.analyzed_class,
        }
    }

    pub fn apply_command(&mut self, command: MaterialClassificationCommand) {
        self.routing_intent.apply(command);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceInspectorEvidence {
    pub quality_summary: String,
    pub analyzed_class: String,
    pub routed_class: String,
    pub confidence_percent: u8,
    pub evidence_summary: String,
    pub warning_count: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceAnalysisReport {
    pub cache_key: SourceAnalysisCacheKey,
    /// Immutable Stage 3/4 prepared-source lineage consumed by this report.
    pub prepared_source_digest: ContentDigest,
    pub quality: SourceQualityReport,
    pub warnings: Vec<QualityWarning>,
    pub classification: MaterialClassification,
    pub stage_result: StageResult,
}

impl SourceAnalysisReport {
    #[must_use]
    pub fn inspector_evidence(&self) -> SourceInspectorEvidence {
        let m = self.classification.measurements;
        SourceInspectorEvidence {
            quality_summary: format!(
                "sharp {}%, usable {}%, registration {}%, effective {} px",
                self.quality.sharpness_milli / 10,
                self.quality.usable_area_ppm / 10_000,
                self.quality.registration_quality_milli / 10,
                self.quality.estimated_material_resolution_px,
            ),
            analyzed_class: self.classification.analyzed_class.label().into(),
            routed_class: self.classification.routed_class().label().into(),
            confidence_percent: u8::try_from(self.classification.confidence_milli / 10)
                .unwrap_or(100),
            evidence_summary: format!(
                "orientation {}%, periods {}%/{}%, bands {}%, saliency {}%, radial {}%",
                m.orientation_coherence_milli / 10,
                m.periodic_x_milli / 10,
                m.periodic_y_milli / 10,
                m.bandedness_milli / 10,
                m.localized_saliency_milli / 10,
                m.radial_symmetry_milli / 10,
            ),
            warning_count: u8::try_from(self.warnings.len()).unwrap_or(u8::MAX),
        }
    }

    pub fn apply_command(&mut self, command: MaterialClassificationCommand) {
        self.classification.apply_command(command);
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SourceAnalysisCacheKey(pub ContentDigest);

#[derive(Clone, Debug, Default)]
pub struct SourceAnalysisCache {
    entries: BTreeMap<SourceAnalysisCacheKey, SourceAnalysisReport>,
}

impl SourceAnalysisCache {
    #[must_use]
    pub fn get(&self, key: &SourceAnalysisCacheKey) -> Option<&SourceAnalysisReport> {
        self.entries.get(key)
    }

    pub fn insert_complete(&mut self, report: SourceAnalysisReport) {
        const MAX_ENTRIES: usize = 32;
        if self.entries.len() >= MAX_ENTRIES && !self.entries.contains_key(&report.cache_key) {
            if let Some(oldest) = self.entries.keys().next().cloned() {
                self.entries.remove(&oldest);
            }
        }
        self.entries.insert(report.cache_key.clone(), report);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClassifierProviderDescriptor {
    pub provider_id: String,
    pub provider_version: String,
    pub interface_version: u16,
    pub model_digest: ContentDigest,
}

pub struct ClassifierProviderRequest<'a> {
    pub quality: &'a SourceQualityReport,
    pub measurements: &'a BehaviorMeasurements,
    pub heuristic_distribution: &'a [RankedClassEvidence],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClassifierProviderOutput {
    pub distribution: Vec<RankedClassEvidence>,
    pub output_version: u16,
}

/// Local-only, versioned classifier boundary. For a fixed descriptor and request, providers must
/// return byte-identical output. Invalid or absent providers leave heuristics authoritative.
pub trait LocalClassifierProvider {
    fn descriptor(&self) -> ClassifierProviderDescriptor;
    fn classify(
        &self,
        request: ClassifierProviderRequest<'_>,
        cancellation: &RenderCancellationToken,
    ) -> Result<ClassifierProviderOutput, ClassifierProviderError>;
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ClassifierProviderError {
    #[error("classifier provider execution was cancelled")]
    Cancelled,
    #[error("classifier provider failed deterministically: {0}")]
    Failed(String),
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SourceAnalysisError {
    #[error("Stage 5 settings are outside bounded ranges")]
    InvalidSettings,
    #[error("Stage 5 requires a non-empty Stage 4 Base Color")]
    EmptyInput,
    #[error("Stage 5 analysis was cancelled")]
    Cancelled,
}

pub fn analyze_source(
    source: &DelitPreparedExemplar,
    settings: &AnalysisSettings,
    provider: Option<&dyn LocalClassifierProvider>,
    cancellation: &RenderCancellationToken,
) -> Result<SourceAnalysisReport, SourceAnalysisError> {
    validate_settings(settings)?;
    check_cancel(cancellation)?;
    let (width, height) = (source.base_color().width(), source.base_color().height());
    if width == 0 || height == 0 {
        return Err(SourceAnalysisError::EmptyInput);
    }
    let step = width
        .max(height)
        .div_ceil(settings.max_analysis_edge)
        .max(1);
    let sampled = sample_luminance(source, step);
    check_cancel(cancellation)?;
    let quality = measure_quality(source, &sampled, step, settings);
    let measurements = measure_behavior(&sampled);
    let heuristic = heuristic_distribution(measurements);
    let (distribution, authority) =
        provider_distribution(provider, &quality, &measurements, &heuristic, cancellation)?;
    let (analyzed_class, confidence) = resolve_class(&distribution, settings.tolerance);
    let warnings = quality_warnings(&quality, settings.thresholds);
    let cache_key = source_analysis_cache_key(source, settings, provider);
    let diagnostics = warnings.iter().map(warning_diagnostic).collect();
    Ok(SourceAnalysisReport {
        cache_key,
        prepared_source_digest: source.prepared_source_digest.clone(),
        quality,
        warnings,
        classification: MaterialClassification {
            analyzed_class,
            confidence_milli: confidence,
            distribution,
            measurements,
            authority,
            routing_intent: MaterialClassificationIntent::default(),
        },
        stage_result: StageResult::Executed {
            algorithm: AlgorithmProvenance {
                algorithm_id: STAGE_05_ALGORITHM_ID.into(),
                version: STAGE_05_ALGORITHM_VERSION.into(),
            },
            settings_hash: settings_digest(settings),
            diagnostics,
        },
    })
}

fn validate_settings(settings: &AnalysisSettings) -> Result<(), SourceAnalysisError> {
    let t = settings.thresholds;
    let all_scores = [
        t.minimum_sharpness_milli,
        t.maximum_noise_milli,
        t.maximum_compression_milli,
        t.minimum_dynamic_range_milli,
        t.minimum_perspective_confidence_milli,
        t.minimum_registration_quality_milli,
        settings.tolerance.minimum_hard_class_confidence_milli,
        settings.tolerance.minimum_top_margin_milli,
    ];
    if settings.max_analysis_edge < 16
        || settings.max_analysis_edge > 1024
        || settings.max_registration_shift_px > 16
        || all_scores.into_iter().any(|value| value > SCORE_MAX)
        || t.maximum_clipped_ppm > 1_000_000
        || t.minimum_usable_area_ppm > 1_000_000
    {
        return Err(SourceAnalysisError::InvalidSettings);
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct SampledPlane {
    width: u32,
    height: u32,
    values: Vec<f32>,
}

fn sample_luminance(source: &DelitPreparedExemplar, step: u32) -> SampledPlane {
    let base = source.base_color();
    let width = base.width().div_ceil(step);
    let height = base.height().div_ceil(step);
    let mut values =
        Vec::with_capacity(usize::try_from(u64::from(width) * u64::from(height)).unwrap_or(0));
    for sy in 0..height {
        for sx in 0..width {
            let color = base.pixel(
                (sx * step).min(base.width() - 1),
                (sy * step).min(base.height() - 1),
            );
            values.push(
                (color.rgb[0] * 0.2126 + color.rgb[1] * 0.7152 + color.rgb[2] * 0.0722)
                    .clamp(0.0, 1.0),
            );
        }
    }
    SampledPlane {
        width,
        height,
        values,
    }
}

fn measure_quality(
    source: &DelitPreparedExemplar,
    sampled: &SampledPlane,
    step: u32,
    settings: &AnalysisSettings,
) -> SourceQualityReport {
    let gradients = gradients(sampled);
    let mean_gradient = mean(&gradients);
    let sharpness_milli = score(mean_gradient * 4.0);
    let noise_milli = score(noise_residual(sampled) * 6.0);
    let compression_milli = score(block_discontinuity(sampled) * 8.0);
    let mut sorted = sampled.values.clone();
    sorted.sort_by(f32::total_cmp);
    let p05 = percentile(&sorted, 5);
    let p95 = percentile(&sorted, 95);
    let dynamic_range_milli = score(p95 - p05);
    let base = source.base_color();
    let mut clipped = 0_u64;
    let mut usable = 0_u64;
    let total = u64::from(sampled.width) * u64::from(sampled.height);
    for sy in 0..sampled.height {
        for sx in 0..sampled.width {
            let x = (sx * step).min(base.width() - 1);
            let y = (sy * step).min(base.height() - 1);
            let color = base.pixel(x, y);
            if color
                .rgb
                .iter()
                .any(|value| *value <= 1.0e-5 || *value >= 1.0 - 1.0e-5)
            {
                clipped += 1;
            }
            let covered = source
                .coverage
                .as_ref()
                .map_or(1.0, |mask| mask.pixel(x, y).0);
            if color.alpha > 1.0 / 255.0 && covered > 0.5 {
                usable += 1;
            }
        }
    }
    let (registration_quality_milli, registration_worst_offset_px) =
        registration_quality(source, step, settings.max_registration_shift_px);
    let usable_area_ppm = fraction_ppm(usable, total);
    let detail_factor = u32::from(sharpness_milli).clamp(100, 1000);
    let estimated_material_resolution_px = base
        .width()
        .min(base.height())
        .saturating_mul(usable_area_ppm)
        / 1_000_000
        * detail_factor
        / 1000;
    SourceQualityReport {
        sharpness_milli,
        noise_milli,
        compression_milli,
        dynamic_range_milli,
        clipped_pixels_ppm: fraction_ppm(clipped, total),
        perspective_confidence_milli: source.perspective_confidence_milli.min(1000),
        usable_area_ppm,
        registration_quality_milli,
        registration_worst_offset_px,
        estimated_material_resolution_px,
    }
}

fn registration_quality(
    source: &DelitPreparedExemplar,
    step: u32,
    max_shift: u8,
) -> (u16, (i8, i8)) {
    let base = sample_luminance(source, step);
    let base_edges = gradients(&base);
    let mut worst_score = SCORE_MAX;
    let mut worst_offset = (0, 0);
    let mut companion_count = 0_u8;
    for channel in &source.channels {
        match channel {
            PreparedExemplarChannel::BaseColor { .. }
            | PreparedExemplarChannel::MaterialId { .. } => continue,
            PreparedExemplarChannel::Scalar { .. }
            | PreparedExemplarChannel::Normal { .. }
            | PreparedExemplarChannel::Mask { .. } => {}
        }
        companion_count = companion_count.saturating_add(1);
        let companion = sample_channel(channel, step, 0, 0);
        let companion_edges = gradients(&companion);
        let zero = correlation(&base_edges, &companion_edges, base.width, base.height, 0, 0);
        let mut best = zero;
        let mut best_offset = (0_i8, 0_i8);
        let shift = i8::try_from(max_shift).unwrap_or(16);
        for dy in -shift..=shift {
            for dx in -shift..=shift {
                let shifted = sample_channel(channel, step, dx, dy);
                let shifted_edges = gradients(&shifted);
                let value = correlation(&base_edges, &shifted_edges, base.width, base.height, 0, 0);
                if value > best + 1.0e-6
                    || ((value - best).abs() <= 1.0e-6 && (dx, dy) < best_offset)
                {
                    best = value;
                    best_offset = (dx, dy);
                }
            }
        }
        let shift_distance =
            u16::from(best_offset.0.unsigned_abs()) + u16::from(best_offset.1.unsigned_abs());
        let alignment = if best <= 1.0e-5 {
            0.5
        } else {
            (zero.max(0.0) / best).clamp(0.0, 1.0)
        };
        let channel_score = score(alignment).saturating_sub(shift_distance.saturating_mul(90));
        if channel_score < worst_score {
            worst_score = channel_score;
            worst_offset = best_offset;
        }
    }
    if companion_count == 0 {
        (SCORE_MAX, (0, 0))
    } else {
        (worst_score, worst_offset)
    }
}

fn sample_channel(
    channel: &PreparedExemplarChannel,
    step: u32,
    offset_x: i8,
    offset_y: i8,
) -> SampledPlane {
    let (source_width, source_height) = channel.dimensions();
    let width = source_width.div_ceil(step);
    let height = source_height.div_ceil(step);
    let mut values = Vec::new();
    for sy in 0..height {
        for sx in 0..width {
            let x = (i64::from((sx * step).min(source_width - 1)) + i64::from(offset_x))
                .clamp(0, i64::from(source_width - 1)) as u32;
            let y = (i64::from((sy * step).min(source_height - 1)) + i64::from(offset_y))
                .clamp(0, i64::from(source_height - 1)) as u32;
            values.push(match channel {
                PreparedExemplarChannel::Scalar { plane, .. } => plane.pixel(x, y).0,
                PreparedExemplarChannel::Normal { plane, .. } => normal_signal(plane.pixel(x, y)),
                PreparedExemplarChannel::Mask { plane, .. } => plane.pixel(x, y).0,
                PreparedExemplarChannel::BaseColor { plane, .. } => {
                    let color = plane.pixel(x, y);
                    color.rgb[0] * 0.2126 + color.rgb[1] * 0.7152 + color.rgb[2] * 0.0722
                }
                PreparedExemplarChannel::MaterialId { plane } => plane.pixel(x, y).0 as f32,
            });
        }
    }
    SampledPlane {
        width,
        height,
        values,
    }
}

fn normal_signal(value: &TangentNormal) -> f32 {
    ((value.xyz[0] + 1.0) * 0.25 + (value.xyz[1] + 1.0) * 0.25).clamp(0.0, 1.0)
}

fn measure_behavior(plane: &SampledPlane) -> BehaviorMeasurements {
    let (major_eigenvalue, minor_eigenvalue) = directional_energy(plane);
    let total = major_eigenvalue + minor_eigenvalue + 1.0e-6;
    let orientation_coherence = (major_eigenvalue - minor_eigenvalue) / total;
    let px = best_period(plane, true);
    let py = best_period(plane, false);
    let stationarity = stationarity(plane);
    BehaviorMeasurements {
        boundary_agreement_milli: score(boundary_agreement(plane)),
        orientation_coherence_milli: score(orientation_coherence),
        orientation_variation_milli: score(orientation_variation(plane)),
        periodic_x_milli: score(px),
        periodic_y_milli: score(py),
        bandedness_milli: score(orientation_coherence * px.max(py)),
        regularity_milli: score((px * py).sqrt()),
        localized_saliency_milli: score(localized_saliency(plane)),
        radial_symmetry_milli: score(radial_symmetry(plane)),
        stationarity_milli: score(stationarity),
    }
}

fn heuristic_distribution(m: BehaviorMeasurements) -> Vec<RankedClassEvidence> {
    let f = |value: u16| f32::from(value) / 1000.0;
    let orientation = f(m.orientation_coherence_milli);
    let variation = f(m.orientation_variation_milli);
    let px = f(m.periodic_x_milli);
    let py = f(m.periodic_y_milli);
    let periods = px.max(py);
    let both_periods = px.min(py);
    let stationarity = f(m.stationarity_milli);
    let saliency = f(m.localized_saliency_milli);
    let radial = f(m.radial_symmetry_milli);
    let boundary = f(m.boundary_agreement_milli);
    let scores = [
        (
            MaterialBehaviorClass::AlreadyTileable,
            boundary.powi(3) * stationarity * (1.0 - saliency * 0.85),
        ),
        (
            MaterialBehaviorClass::StochasticIsotropic,
            (1.0 - orientation) * stationarity * (1.0 - periods * 0.6) * (1.0 - boundary * 0.6),
        ),
        (
            MaterialBehaviorClass::StochasticDirectional,
            orientation * stationarity * (1.0 - periods * 0.7) * (1.0 - boundary * 0.6),
        ),
        (
            MaterialBehaviorClass::PeriodicLatticeStructured,
            both_periods * 0.8 + f(m.regularity_milli) * 0.2,
        ),
        (
            MaterialBehaviorClass::LayeredBanded,
            f(m.bandedness_milli) * (1.0 - both_periods * 0.5),
        ),
        (
            MaterialBehaviorClass::OrganicDirectional,
            variation * (0.5 + orientation * 0.5) * (1.0 - periods * 0.5),
        ),
        (
            MaterialBehaviorClass::ManufacturedPattern,
            periods * (0.5 + 0.5 * f(m.regularity_milli)) * (1.0 - variation * 0.4),
        ),
        (
            MaterialBehaviorClass::UniqueDetail,
            saliency * (1.0 - periods * 0.6),
        ),
        (
            MaterialBehaviorClass::RadialDetail,
            radial * (0.65 + saliency * 0.35),
        ),
        (
            MaterialBehaviorClass::MixedUnknown,
            0.12 + (1.0 - stationarity) * 0.9 + variation * (1.0 - orientation) * 0.4,
        ),
    ];
    normalize_scores(&scores)
}

fn normalize_scores(scores: &[(MaterialBehaviorClass, f32)]) -> Vec<RankedClassEvidence> {
    let total: f32 = scores.iter().map(|(_, value)| value.max(0.001)).sum();
    let mut ranked: Vec<_> = scores
        .iter()
        .map(|(class, value)| RankedClassEvidence {
            class: *class,
            probability_milli: score(value.max(0.001) / total),
            support_milli: score(*value),
        })
        .collect();
    let sum: i32 = ranked
        .iter()
        .map(|item| i32::from(item.probability_milli))
        .sum();
    if let Some(first) = ranked.first_mut() {
        first.probability_milli =
            u16::try_from((i32::from(first.probability_milli) + 1000 - sum).clamp(0, 1000))
                .unwrap_or(0);
    }
    ranked.sort_by(|a, b| {
        b.probability_milli
            .cmp(&a.probability_milli)
            .then(a.class.cmp(&b.class))
    });
    ranked
}

fn provider_distribution(
    provider: Option<&dyn LocalClassifierProvider>,
    quality: &SourceQualityReport,
    measurements: &BehaviorMeasurements,
    heuristic: &[RankedClassEvidence],
    cancellation: &RenderCancellationToken,
) -> Result<(Vec<RankedClassEvidence>, ClassificationAuthority), SourceAnalysisError> {
    let Some(provider) = provider else {
        return Ok((
            heuristic.to_vec(),
            ClassificationAuthority::DeterministicHeuristics,
        ));
    };
    let descriptor = provider.descriptor();
    if descriptor.interface_version != CLASSIFIER_PROVIDER_INTERFACE_VERSION {
        return Ok((
            heuristic.to_vec(),
            ClassificationAuthority::DeterministicHeuristics,
        ));
    }
    let request = ClassifierProviderRequest {
        quality,
        measurements,
        heuristic_distribution: heuristic,
    };
    let output = match provider.classify(request, cancellation) {
        Ok(output) => output,
        Err(ClassifierProviderError::Cancelled) => return Err(SourceAnalysisError::Cancelled),
        Err(ClassifierProviderError::Failed(_)) => {
            return Ok((
                heuristic.to_vec(),
                ClassificationAuthority::DeterministicHeuristics,
            ));
        }
    };
    if output.output_version != CLASSIFIER_PROVIDER_INTERFACE_VERSION
        || !valid_distribution(&output.distribution)
    {
        return Ok((
            heuristic.to_vec(),
            ClassificationAuthority::DeterministicHeuristics,
        ));
    }
    Ok((output.distribution, ClassificationAuthority::LocalProvider))
}

fn valid_distribution(distribution: &[RankedClassEvidence]) -> bool {
    let classes: BTreeSet<_> = distribution.iter().map(|item| item.class).collect();
    distribution.len() == MaterialBehaviorClass::ALL.len()
        && classes.len() == MaterialBehaviorClass::ALL.len()
        && distribution
            .iter()
            .map(|item| u32::from(item.probability_milli))
            .sum::<u32>()
            == 1000
        && distribution
            .iter()
            .all(|item| item.probability_milli <= 1000 && item.support_milli <= 1000)
        && distribution
            .windows(2)
            .all(|pair| pair[0].probability_milli >= pair[1].probability_milli)
}

fn resolve_class(
    distribution: &[RankedClassEvidence],
    tolerance: ClassificationTolerance,
) -> (MaterialBehaviorClass, u16) {
    let Some(top) = distribution.first() else {
        return (MaterialBehaviorClass::MixedUnknown, 0);
    };
    let second = distribution.get(1).map_or(0, |item| item.probability_milli);
    let margin = top.probability_milli.saturating_sub(second);
    if top.class == MaterialBehaviorClass::MixedUnknown
        || top.support_milli < tolerance.minimum_hard_class_confidence_milli
        || margin < tolerance.minimum_top_margin_milli
    {
        (MaterialBehaviorClass::MixedUnknown, top.probability_milli)
    } else {
        (top.class, top.probability_milli)
    }
}

fn quality_warnings(quality: &SourceQualityReport, t: QualityThresholds) -> Vec<QualityWarning> {
    let mut warnings = Vec::new();
    let mut add = |condition, code, message: &str, recoveries| {
        if condition {
            warnings.push(QualityWarning {
                code,
                message: message.into(),
                recoveries,
            });
        }
    };
    add(
        quality.sharpness_milli < t.minimum_sharpness_milli,
        QualityWarningCode::Soft,
        "Source is soft; retain physical scale and use a lower-fidelity texel-density route.",
        vec![
            RecoveryChoice::LowerTexelDensity,
            RecoveryChoice::ChooseAnotherSource,
        ],
    );
    add(
        quality.noise_milli > t.maximum_noise_milli,
        QualityWarningCode::Noisy,
        "Noise may be reproduced as material structure.",
        vec![RecoveryChoice::AdjustSettings, RecoveryChoice::UseSynthesis],
    );
    add(
        quality.compression_milli > t.maximum_compression_milli,
        QualityWarningCode::Compressed,
        "Block compression is visible in measurable boundary discontinuities.",
        vec![
            RecoveryChoice::ChooseAnotherSource,
            RecoveryChoice::LowerTexelDensity,
        ],
    );
    add(
        quality.dynamic_range_milli < t.minimum_dynamic_range_milli,
        QualityWarningCode::NarrowDynamicRange,
        "Source has narrow luminance range; classification evidence is weaker.",
        vec![RecoveryChoice::AdjustSettings],
    );
    add(
        quality.clipped_pixels_ppm > t.maximum_clipped_ppm,
        QualityWarningCode::Clipped,
        "Clipped samples cannot provide reliable reflectance detail.",
        vec![
            RecoveryChoice::ChooseAnotherSource,
            RecoveryChoice::UseSynthesis,
        ],
    );
    add(
        quality.perspective_confidence_milli < t.minimum_perspective_confidence_milli,
        QualityWarningCode::LowPerspectiveConfidence,
        "Planar rectification confidence is low.",
        vec![
            RecoveryChoice::AdjustSettings,
            RecoveryChoice::ChooseAnotherSource,
        ],
    );
    add(
        quality.usable_area_ppm < t.minimum_usable_area_ppm,
        QualityWarningCode::LimitedUsableArea,
        "Only a limited source area is usable; synthesis or lower density remains available.",
        vec![
            RecoveryChoice::UseSynthesis,
            RecoveryChoice::LowerTexelDensity,
        ],
    );
    add(
        quality.registration_quality_milli < t.minimum_registration_quality_milli,
        QualityWarningCode::MisregisteredChannels,
        "PBR companion maps are measurably offset from Base Color.",
        vec![
            RecoveryChoice::AdjustSettings,
            RecoveryChoice::ChooseAnotherSource,
        ],
    );
    add(
        quality.estimated_material_resolution_px < t.minimum_material_resolution_px,
        QualityWarningCode::LowMaterialResolution,
        "Estimated material resolution is below the preferred route threshold.",
        vec![
            RecoveryChoice::LowerTexelDensity,
            RecoveryChoice::UseSynthesis,
            RecoveryChoice::IncreaseOutputResolution,
        ],
    );
    warnings
}

fn warning_diagnostic(warning: &QualityWarning) -> CompilationDiagnostic {
    CompilationDiagnostic {
        code: DiagnosticCode::InsufficientInput,
        stage: Some(5),
        message: warning.message.clone(),
        context: BTreeMap::from([("qualityWarning".into(), format!("{:?}", warning.code))]),
    }
}

#[must_use]
pub fn source_analysis_cache_key(
    source: &DelitPreparedExemplar,
    settings: &AnalysisSettings,
    provider: Option<&dyn LocalClassifierProvider>,
) -> SourceAnalysisCacheKey {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(STAGE_05_ALGORITHM_VERSION.as_bytes());
    bytes.extend_from_slice(&CLASSIFIER_PROVIDER_INTERFACE_VERSION.to_le_bytes());
    bytes.extend_from_slice(settings_digest(settings).0.as_bytes());
    bytes.extend_from_slice(source.exemplar_id.as_bytes());
    bytes.extend_from_slice(
        source
            .original_prepared_base_color
            .width()
            .to_le_bytes()
            .as_slice(),
    );
    bytes.extend_from_slice(
        source
            .original_prepared_base_color
            .height()
            .to_le_bytes()
            .as_slice(),
    );
    for color in source.base_color().to_row_major() {
        for component in color.rgb {
            bytes.extend_from_slice(&component.to_bits().to_le_bytes());
        }
        bytes.extend_from_slice(&color.alpha.to_bits().to_le_bytes());
    }
    bytes.extend_from_slice(&source.perspective_confidence_milli.to_le_bytes());
    match &source.coverage {
        Some(coverage) => {
            bytes.push(1);
            for value in coverage.to_row_major() {
                bytes.extend_from_slice(&value.0.to_bits().to_le_bytes());
            }
        }
        None => bytes.push(0),
    }
    for channel in &source.channels {
        bytes.push(channel.role() as u8);
        match channel {
            PreparedExemplarChannel::BaseColor { plane, .. } => {
                for value in plane.to_row_major() {
                    for component in value.rgb {
                        bytes.extend_from_slice(&component.to_bits().to_le_bytes());
                    }
                    bytes.extend_from_slice(&value.alpha.to_bits().to_le_bytes());
                }
            }
            PreparedExemplarChannel::Scalar { plane, .. } => {
                for value in plane.to_row_major() {
                    bytes.extend_from_slice(&value.0.to_bits().to_le_bytes());
                }
            }
            PreparedExemplarChannel::Normal { plane, .. } => {
                for value in plane.to_row_major() {
                    for component in value.xyz {
                        bytes.extend_from_slice(&component.to_bits().to_le_bytes());
                    }
                    bytes.extend_from_slice(&value.alpha.to_bits().to_le_bytes());
                }
            }
            PreparedExemplarChannel::MaterialId { plane } => {
                for value in plane.to_row_major() {
                    bytes.extend_from_slice(&value.0.to_le_bytes());
                }
            }
            PreparedExemplarChannel::Mask { plane, .. } => {
                for value in plane.to_row_major() {
                    bytes.extend_from_slice(&value.0.to_bits().to_le_bytes());
                }
            }
        }
    }
    if let Some(provider) = provider {
        let descriptor = provider.descriptor();
        bytes.extend_from_slice(descriptor.provider_id.as_bytes());
        bytes.extend_from_slice(descriptor.provider_version.as_bytes());
        bytes.extend_from_slice(&descriptor.interface_version.to_le_bytes());
        bytes.extend_from_slice(descriptor.model_digest.0.as_bytes());
    }
    SourceAnalysisCacheKey(ContentDigest::sha256(&bytes))
}

fn settings_digest(settings: &AnalysisSettings) -> ContentDigest {
    ContentDigest::sha256(format!("{settings:?}").as_bytes())
}

fn gradients(plane: &SampledPlane) -> Vec<f32> {
    let mut out = vec![0.0; plane.values.len()];
    if plane.width < 2 || plane.height < 2 {
        return out;
    }
    for y in 0..plane.height - 1 {
        for x in 0..plane.width - 1 {
            let i = idx(plane.width, x, y);
            let gx = plane.values[idx(plane.width, x + 1, y)] - plane.values[i];
            let gy = plane.values[idx(plane.width, x, y + 1)] - plane.values[i];
            out[i] = (gx * gx + gy * gy).sqrt();
        }
    }
    out
}

fn directional_energy(plane: &SampledPlane) -> (f32, f32) {
    let (jxx, jxy, jyy) = structure_tensor(plane, 0, plane.width, 0, plane.height);
    tensor_eigenvalues(jxx, jxy, jyy)
}

fn structure_tensor(plane: &SampledPlane, x0: u32, x1: u32, y0: u32, y1: u32) -> (f32, f32, f32) {
    let mut jxx = 0.0;
    let mut jxy = 0.0;
    let mut jyy = 0.0;
    if x1.saturating_sub(x0) < 3 || y1.saturating_sub(y0) < 3 {
        return (jxx, jxy, jyy);
    }
    let start_x = x0.max(1);
    let start_y = y0.max(1);
    let end_x = x1.min(plane.width - 1);
    let end_y = y1.min(plane.height - 1);
    for y in start_y..end_y {
        for x in start_x..end_x {
            let sample = |sx, sy| plane.values[idx(plane.width, sx, sy)];
            let gx = 3.0 * (sample(x + 1, y - 1) - sample(x - 1, y - 1))
                + 10.0 * (sample(x + 1, y) - sample(x - 1, y))
                + 3.0 * (sample(x + 1, y + 1) - sample(x - 1, y + 1));
            let gy = 3.0 * (sample(x - 1, y + 1) - sample(x - 1, y - 1))
                + 10.0 * (sample(x, y + 1) - sample(x, y - 1))
                + 3.0 * (sample(x + 1, y + 1) - sample(x + 1, y - 1));
            let magnitude = (gx * gx + gy * gy).sqrt();
            if magnitude <= 1.0e-6 {
                continue;
            }
            const SCHARR_GRADIENT_CAP: f32 = 8.0;
            let scale = (SCHARR_GRADIENT_CAP / magnitude).min(1.0);
            let ix = gx * scale;
            let iy = gy * scale;
            jxx += ix * ix;
            jxy += ix * iy;
            jyy += iy * iy;
        }
    }
    (jxx, jxy, jyy)
}

fn tensor_eigenvalues(jxx: f32, jxy: f32, jyy: f32) -> (f32, f32) {
    let trace = jxx + jyy;
    let discriminant = ((jxx - jyy).powi(2) + 4.0 * jxy * jxy).max(0.0).sqrt();
    (
        (trace + discriminant) * 0.5,
        (trace - discriminant).max(0.0) * 0.5,
    )
}

fn noise_residual(plane: &SampledPlane) -> f32 {
    if plane.width < 3 || plane.height < 3 {
        return 0.0;
    }
    let mut residual = 0.0;
    let mut count = 0_u32;
    for y in 1..plane.height - 1 {
        for x in 1..plane.width - 1 {
            let neighbors = plane.values[idx(plane.width, x - 1, y)]
                + plane.values[idx(plane.width, x + 1, y)]
                + plane.values[idx(plane.width, x, y - 1)]
                + plane.values[idx(plane.width, x, y + 1)];
            residual += (plane.values[idx(plane.width, x, y)] - neighbors * 0.25).abs();
            count += 1;
        }
    }
    residual / count.max(1) as f32
}

fn block_discontinuity(plane: &SampledPlane) -> f32 {
    if plane.width < 9 || plane.height < 9 {
        return 0.0;
    }
    let mut boundary = 0.0;
    let mut boundary_count = 0_u32;
    let mut interior = 0.0;
    let mut interior_count = 0_u32;
    for y in 0..plane.height {
        for x in 1..plane.width {
            let difference = (plane.values[idx(plane.width, x, y)]
                - plane.values[idx(plane.width, x - 1, y)])
            .abs();
            if x % 8 == 0 {
                boundary += difference;
                boundary_count += 1;
            } else {
                interior += difference;
                interior_count += 1;
            }
        }
    }
    let boundary_mean = boundary / boundary_count.max(1) as f32;
    let interior_mean = interior / interior_count.max(1) as f32;
    (boundary_mean - interior_mean).max(0.0)
}

fn boundary_agreement(plane: &SampledPlane) -> f32 {
    if plane.width < 2 || plane.height < 2 {
        return 0.0;
    }
    let mut seam = 0.0;
    let mut adjacent = 0.0;
    let mut count = 0_u32;
    for y in 0..plane.height {
        seam += (plane.values[idx(plane.width, 0, y)]
            - plane.values[idx(plane.width, plane.width - 1, y)])
        .abs();
        adjacent +=
            (plane.values[idx(plane.width, 0, y)] - plane.values[idx(plane.width, 1, y)]).abs();
        adjacent += (plane.values[idx(plane.width, plane.width - 1, y)]
            - plane.values[idx(plane.width, plane.width - 2, y)])
        .abs();
        count += 1;
    }
    for x in 0..plane.width {
        seam += (plane.values[idx(plane.width, x, 0)]
            - plane.values[idx(plane.width, x, plane.height - 1)])
        .abs();
        adjacent +=
            (plane.values[idx(plane.width, x, 0)] - plane.values[idx(plane.width, x, 1)]).abs();
        adjacent += (plane.values[idx(plane.width, x, plane.height - 1)]
            - plane.values[idx(plane.width, x, plane.height - 2)])
        .abs();
        count += 1;
    }
    let seam_mean = seam / count.max(1) as f32;
    let adjacent_mean = adjacent / (count.max(1) * 2) as f32;
    (1.0 - seam_mean / (adjacent_mean * 1.5 + 1.0e-4)).clamp(0.0, 1.0)
}

fn best_period(plane: &SampledPlane, x_axis: bool) -> f32 {
    let limit = if x_axis { plane.width } else { plane.height };
    if limit < 6 {
        return 0.0;
    }
    let max_lag = (limit / 2).min(32);
    let mut correlations = Vec::new();
    for lag in 1..=max_lag {
        let (dx, dy) = if x_axis { (lag, 0) } else { (0, lag) };
        let mut a = Vec::new();
        let mut b = Vec::new();
        for y in 0..plane.height - dy {
            for x in 0..plane.width - dx {
                a.push(plane.values[idx(plane.width, x, y)]);
                b.push(plane.values[idx(plane.width, x + dx, y + dy)]);
            }
        }
        let mean_a = mean(&a);
        let mean_b = mean(&b);
        let mut covariance = 0.0;
        let mut aa = 0.0;
        let mut bb = 0.0;
        for (av, bv) in a.iter().zip(&b) {
            let ac = *av - mean_a;
            let bc = *bv - mean_b;
            covariance += ac * bc;
            aa += ac * ac;
            bb += bc * bc;
        }
        let value = if aa <= 1.0e-8 || bb <= 1.0e-8 {
            0.0
        } else {
            covariance / (aa * bb).sqrt()
        };
        correlations.push(value);
    }
    let mut best: f32 = 0.0;
    for index in 1..correlations.len().saturating_sub(1) {
        let value = correlations[index].clamp(0.0, 1.0);
        let adjacent =
            ((correlations[index - 1] + correlations[index + 1]) * 0.5).clamp(-1.0, 0.999);
        let prominence = ((value - adjacent) / (1.0 - adjacent)).clamp(0.0, 1.0);
        best = best.max(value * prominence);
    }
    let mut axis_change = 0.0;
    let mut axis_count = 0_u32;
    if x_axis {
        for y in 0..plane.height {
            for x in 1..plane.width {
                axis_change += (plane.values[idx(plane.width, x, y)]
                    - plane.values[idx(plane.width, x - 1, y)])
                .abs();
                axis_count += 1;
            }
        }
    } else {
        for y in 1..plane.height {
            for x in 0..plane.width {
                axis_change += (plane.values[idx(plane.width, x, y)]
                    - plane.values[idx(plane.width, x, y - 1)])
                .abs();
                axis_count += 1;
            }
        }
    }
    let mean_axis_change = axis_change / axis_count.max(1) as f32;
    if mean_axis_change <= 1.0e-6 {
        0.0
    } else {
        best
    }
}

fn stationarity(plane: &SampledPlane) -> f32 {
    if plane.width < 4 || plane.height < 4 {
        return 0.0;
    }
    let mid_x = plane.width / 2;
    let mid_y = plane.height / 2;
    let regions = [
        (0, mid_x, 0, mid_y),
        (mid_x, plane.width, 0, mid_y),
        (0, mid_x, mid_y, plane.height),
        (mid_x, plane.width, mid_y, plane.height),
    ];
    let means: Vec<_> = regions
        .iter()
        .map(|&(x0, x1, y0, y1)| region_mean(plane, x0, x1, y0, y1))
        .collect();
    let gradient_means: Vec<_> = regions
        .iter()
        .map(|&(x0, x1, y0, y1)| {
            let mut total = 0.0;
            let mut count = 0_u32;
            for y in y0..y1.saturating_sub(1) {
                for x in x0..x1.saturating_sub(1) {
                    let value = plane.values[idx(plane.width, x, y)];
                    let gx = plane.values[idx(plane.width, x + 1, y)] - value;
                    let gy = plane.values[idx(plane.width, x, y + 1)] - value;
                    total += (gx * gx + gy * gy).sqrt();
                    count += 1;
                }
            }
            total / count.max(1) as f32
        })
        .collect();
    let mean_spread = means.iter().copied().fold(f32::MIN, f32::max)
        - means.iter().copied().fold(f32::MAX, f32::min);
    let gradient_max = gradient_means.iter().copied().fold(f32::MIN, f32::max);
    let gradient_min = gradient_means.iter().copied().fold(f32::MAX, f32::min);
    let gradient_spread = (gradient_max - gradient_min) / (gradient_max + gradient_min + 1.0e-5);
    (1.0 - (mean_spread * 2.0).max(gradient_spread)).clamp(0.0, 1.0)
}

fn orientation_variation(plane: &SampledPlane) -> f32 {
    if plane.width < 8 || plane.height < 8 {
        return 0.0;
    }
    let mid_x = plane.width / 2;
    let mid_y = plane.height / 2;
    let regions = [
        (0, mid_x, 0, mid_y),
        (mid_x, plane.width, 0, mid_y),
        (0, mid_x, mid_y, plane.height),
        (mid_x, plane.width, mid_y, plane.height),
    ];
    let mut vector_x = 0.0;
    let mut vector_y = 0.0;
    let mut weight_sum = 0.0;
    for (x0, x1, y0, y1) in regions {
        let (jxx, jxy, jyy) = structure_tensor(plane, x0, x1, y0, y1);
        let (major, minor) = tensor_eigenvalues(jxx, jxy, jyy);
        let coherence = (major - minor) / (major + minor + 1.0e-6);
        let doubled_angle = (2.0 * jxy).atan2(jxx - jyy);
        vector_x += doubled_angle.cos() * coherence;
        vector_y += doubled_angle.sin() * coherence;
        weight_sum += coherence;
    }
    if weight_sum <= 1.0e-6 {
        0.0
    } else {
        (1.0 - (vector_x * vector_x + vector_y * vector_y).sqrt() / weight_sum).clamp(0.0, 1.0)
    }
}

fn localized_saliency(plane: &SampledPlane) -> f32 {
    if plane.width < 4 || plane.height < 4 {
        return 0.0;
    }
    let global_mean = mean(&plane.values);
    let block_edge = (plane.width.min(plane.height) / 8).max(2);
    let mut strongest_deviation: f32 = 0.0;
    let mut y = 0;
    while y < plane.height {
        let mut x = 0;
        while x < plane.width {
            let block_mean = region_mean(
                plane,
                x,
                (x + block_edge).min(plane.width),
                y,
                (y + block_edge).min(plane.height),
            );
            strongest_deviation = strongest_deviation.max((block_mean - global_mean).abs());
            x += block_edge;
        }
        y += block_edge;
    }
    (strongest_deviation * 4.0).clamp(0.0, 1.0)
}

fn radial_symmetry(plane: &SampledPlane) -> f32 {
    if plane.width < 8 || plane.height < 8 {
        return 0.0;
    }
    let cx = (plane.width - 1) as f32 * 0.5;
    let cy = (plane.height - 1) as f32 * 0.5;
    let bins = usize::try_from(plane.width.min(plane.height) / 2)
        .unwrap_or(0)
        .max(2);
    let mut sums = vec![0.0; bins];
    let mut counts = vec![0_u32; bins];
    for y in 0..plane.height {
        for x in 0..plane.width {
            let radius = (((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt()
                / cx.min(cy).max(1.0)
                * (bins - 1) as f32) as usize;
            if radius < bins {
                sums[radius] += plane.values[idx(plane.width, x, y)];
                counts[radius] += 1;
            }
        }
    }
    let means: Vec<_> = sums
        .iter()
        .zip(&counts)
        .map(|(sum, count)| *sum / (*count).max(1) as f32)
        .collect();
    let global_mean = mean(&plane.values);
    let mut residual_energy = 0.0;
    let mut total_energy = 0.0;
    for y in 0..plane.height {
        for x in 0..plane.width {
            let radius = (((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt()
                / cx.min(cy).max(1.0)
                * (bins - 1) as f32) as usize;
            if radius < bins {
                let value = plane.values[idx(plane.width, x, y)];
                residual_energy += (value - means[radius]).powi(2);
                total_energy += (value - global_mean).powi(2);
            }
        }
    }
    if total_energy <= 1.0e-8 {
        0.0
    } else {
        (1.0 - residual_energy / total_energy).clamp(0.0, 1.0)
    }
}

fn correlation(a: &[f32], b: &[f32], width: u32, height: u32, dx: i8, dy: i8) -> f32 {
    let mut dot = 0.0;
    let mut aa = 0.0;
    let mut bb = 0.0;
    for y in 0..height {
        for x in 0..width {
            let bx = i64::from(x) + i64::from(dx);
            let by = i64::from(y) + i64::from(dy);
            if bx < 0 || by < 0 || bx >= i64::from(width) || by >= i64::from(height) {
                continue;
            }
            let av = a[idx(width, x, y)];
            let bv = b[idx(width, bx as u32, by as u32)];
            dot += av * bv;
            aa += av * av;
            bb += bv * bv;
        }
    }
    if aa <= 1.0e-9 || bb <= 1.0e-9 {
        0.0
    } else {
        dot / (aa * bb).sqrt()
    }
}

fn region_mean(plane: &SampledPlane, x0: u32, x1: u32, y0: u32, y1: u32) -> f32 {
    let mut total = 0.0;
    let mut count = 0_u32;
    for y in y0..y1 {
        for x in x0..x1 {
            total += plane.values[idx(plane.width, x, y)];
            count += 1;
        }
    }
    total / count.max(1) as f32
}

fn percentile(sorted: &[f32], percentile: usize) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[(sorted.len() - 1) * percentile / 100]
}

fn mean(values: &[f32]) -> f32 {
    values.iter().sum::<f32>() / values.len().max(1) as f32
}
fn score(value: f32) -> u16 {
    (value.clamp(0.0, 1.0) * 1000.0).round() as u16
}
fn fraction_ppm(part: u64, total: u64) -> u32 {
    u32::try_from(part.saturating_mul(1_000_000) / total.max(1)).unwrap_or(1_000_000)
}
fn idx(width: u32, x: u32, y: u32) -> usize {
    usize::try_from(u64::from(y) * u64::from(width) + u64::from(x)).expect("bounded sampled plane")
}
fn check_cancel(cancellation: &RenderCancellationToken) -> Result<(), SourceAnalysisError> {
    if cancellation.is_cancelled() {
        Err(SourceAnalysisError::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use hot_trimmer_domain::{
        DelightingPassThroughReason, MaterialCorpusManifest, SyntheticFixture, SyntheticGenerator,
    };
    use hot_trimmer_image_io::{ImagePlane, LinearColor, MaskValue, ResolvedAlphaMode};

    use crate::{ReflectanceProvenance, RouteExecution};

    use super::*;

    const WIDTH: u32 = 64;
    const HEIGHT: u32 = 64;

    fn source_from_signal(
        signal: &[f32],
        companion_offset: Option<(i8, i8)>,
    ) -> DelitPreparedExemplar {
        let colors: Vec<_> = signal
            .iter()
            .map(|value| LinearColor {
                rgb: [
                    *value,
                    (*value * 0.85).clamp(0.0, 1.0),
                    (*value * 0.7).clamp(0.0, 1.0),
                ],
                alpha: 1.0,
            })
            .collect();
        let base = ImagePlane::from_row_major(WIDTH, HEIGHT, 8, &colors).unwrap();
        let mut channels = vec![PreparedExemplarChannel::BaseColor {
            plane: base.clone(),
            alpha_mode: ResolvedAlphaMode::Opaque,
        }];
        if let Some((dx, dy)) = companion_offset {
            let mut shifted = Vec::new();
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    let sx = (i64::from(x) - i64::from(dx)).rem_euclid(i64::from(WIDTH)) as u32;
                    let sy = (i64::from(y) - i64::from(dy)).rem_euclid(i64::from(HEIGHT)) as u32;
                    shifted.push(hot_trimmer_image_io::LinearScalar(
                        signal[idx(WIDTH, sx, sy)],
                    ));
                }
            }
            channels.push(PreparedExemplarChannel::Scalar {
                role: hot_trimmer_domain::MaterialChannelRole::Height,
                plane: ImagePlane::from_row_major(WIDTH, HEIGHT, 8, &shifted).unwrap(),
            });
        }
        DelitPreparedExemplar {
            exemplar_id: "evidence-only".into(),
            prepared_source_digest: ContentDigest::sha256(b"quality-fixture"),
            perspective_confidence_milli: 880,
            original_prepared_base_color: base,
            channels,
            coverage: Some(
                ImagePlane::from_row_major(
                    WIDTH,
                    HEIGHT,
                    8,
                    &vec![MaskValue(1.0); (WIDTH * HEIGHT) as usize],
                )
                .unwrap(),
            ),
            masks: None,
            reflectance_provenance: ReflectanceProvenance::ImportedPrepared,
            route_execution: RouteExecution::PassThrough(
                DelightingPassThroughReason::AuthoredTextureOrPbrSet,
            ),
            upstream_stage_result: StageResult::PassThrough {
                reason: "already planar".into(),
            },
            stage_result: StageResult::PassThrough {
                reason: "already de-lit".into(),
            },
        }
    }

    fn stochastic_signal() -> Vec<f32> {
        (0..WIDTH * HEIGHT)
            .map(|index| {
                let x = index % WIDTH;
                let y = index / WIDTH;
                let mut z = u64::from(x) << 32 | u64::from(y);
                z ^= z >> 30;
                z = z.wrapping_mul(0xbf58_476d_1ce4_e5b9);
                z ^= z >> 27;
                0.25 + ((z & 0xffff) as f32 / 65_535.0) * 0.5
            })
            .collect()
    }

    fn lattice_signal() -> Vec<f32> {
        (0..WIDTH * HEIGHT)
            .map(|index| {
                let x = index % WIDTH;
                let y = index / WIDTH;
                if x % 8 < 2 || y % 12 < 2 { 0.82 } else { 0.18 }
            })
            .collect()
    }

    fn behavior_signal(class: MaterialBehaviorClass) -> Vec<f32> {
        let stochastic = stochastic_signal();
        let lattice = lattice_signal();
        let mut values = vec![0.0; (WIDTH * HEIGHT) as usize];
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let i = idx(WIDTH, x, y);
                values[i] = match class {
                    MaterialBehaviorClass::AlreadyTileable => stochastic[i],
                    MaterialBehaviorClass::StochasticIsotropic => stochastic[i],
                    MaterialBehaviorClass::StochasticDirectional => {
                        let mut smoothed = 0.0;
                        for offset in -3_i64..=3 {
                            let sample_y =
                                (i64::from(y) + offset).rem_euclid(i64::from(HEIGHT)) as u32;
                            smoothed += stochastic[idx(WIDTH, x, sample_y)];
                        }
                        smoothed / 7.0
                    }
                    MaterialBehaviorClass::PeriodicLatticeStructured => lattice[i],
                    MaterialBehaviorClass::LayeredBanded => {
                        if y % 12 < 5 {
                            0.78
                        } else {
                            0.22
                        }
                    }
                    MaterialBehaviorClass::OrganicDirectional => {
                        let mut smoothed = 0.0;
                        for offset in -3_i64..=3 {
                            let (sample_x, sample_y) = if x < WIDTH / 2 {
                                (
                                    x,
                                    (i64::from(y) + offset).rem_euclid(i64::from(HEIGHT)) as u32,
                                )
                            } else {
                                (
                                    (i64::from(x) + offset).rem_euclid(i64::from(WIDTH)) as u32,
                                    y,
                                )
                            };
                            smoothed += stochastic[idx(WIDTH, sample_x, sample_y)];
                        }
                        smoothed / 7.0
                    }
                    MaterialBehaviorClass::ManufacturedPattern => {
                        let motif_x = x % 10;
                        let row_noise = stochastic[idx(WIDTH, 0, y)];
                        if motif_x < 3 {
                            0.8 - row_noise * 0.2
                        } else {
                            0.15 + row_noise * 0.25
                        }
                    }
                    MaterialBehaviorClass::UniqueDetail => {
                        let dx = i64::from(x) - 23;
                        let dy = i64::from(y) - 37;
                        if dx * dx + dy * dy <= 64 {
                            0.95
                        } else {
                            0.28 + stochastic[i] * 0.08
                        }
                    }
                    MaterialBehaviorClass::RadialDetail => {
                        let dx = x as f32 - 31.5;
                        let dy = y as f32 - 31.5;
                        0.5 + ((dx * dx + dy * dy).sqrt() * 0.55).sin() * 0.35
                    }
                    MaterialBehaviorClass::MixedUnknown => {
                        if x < WIDTH / 2 {
                            lattice[i]
                        } else {
                            0.495 + stochastic[i] * 0.01
                        }
                    }
                };
            }
        }
        if class == MaterialBehaviorClass::AlreadyTileable {
            for y in 0..HEIGHT {
                values[idx(WIDTH, WIDTH - 1, y)] = values[idx(WIDTH, 0, y)];
            }
            for x in 0..WIDTH {
                values[idx(WIDTH, x, HEIGHT - 1)] = values[idx(WIDTH, x, 0)];
            }
        }
        values
    }

    fn class_support(report: &SourceAnalysisReport, class: MaterialBehaviorClass) -> u16 {
        report
            .classification
            .distribution
            .iter()
            .find(|entry| entry.class == class)
            .unwrap()
            .support_milli
    }

    fn fixture_signal(fixture: &SyntheticFixture) -> Vec<f32> {
        fixture.planes["base_color"]
            .iter()
            .map(|value| f32::from(*value) / 65_535.0)
            .collect()
    }

    fn source_from_fixture(fixture: &SyntheticFixture) -> DelitPreparedExemplar {
        let mut source = source_from_signal(&fixture_signal(fixture), None);
        let roles = [
            hot_trimmer_domain::MaterialChannelRole::Height,
            hot_trimmer_domain::MaterialChannelRole::Roughness,
            hot_trimmer_domain::MaterialChannelRole::AmbientOcclusion,
        ];
        for ((_, values), role) in fixture
            .planes
            .iter()
            .filter(|(name, _)| name.as_str() != "base_color")
            .zip(roles)
        {
            let pixels: Vec<_> = values
                .iter()
                .map(|value| hot_trimmer_image_io::LinearScalar(f32::from(*value) / 65_535.0))
                .collect();
            source.channels.push(PreparedExemplarChannel::Scalar {
                role,
                plane: ImagePlane::from_row_major(WIDTH, HEIGHT, 8, &pixels).unwrap(),
            });
        }
        source
    }

    fn dominant_or_ambiguous_error(
        report: &SourceAnalysisReport,
        intended: MaterialBehaviorClass,
        tolerance: ClassificationTolerance,
    ) -> Option<String> {
        if intended == MaterialBehaviorClass::MixedUnknown {
            return (report.classification.analyzed_class != MaterialBehaviorClass::MixedUnknown)
                .then(|| {
                    format!(
                        "{intended:?}: expected explicit MixedUnknown, got {:?}",
                        report.classification.analyzed_class
                    )
                });
        }
        if report.classification.analyzed_class == intended {
            return None;
        }
        if report.classification.analyzed_class != MaterialBehaviorClass::MixedUnknown {
            return Some(format!(
                "{intended:?}: hard class {:?} displaced intended class",
                report.classification.analyzed_class
            ));
        }
        let rank = report
            .classification
            .distribution
            .iter()
            .position(|entry| entry.class == intended)
            .expect("complete distribution");
        if rank > 2 {
            return Some(format!(
                "{intended:?}: intended class rank {rank} is outside the ambiguity set"
            ));
        }
        let top = report.classification.distribution[0];
        let intended_evidence = report.classification.distribution[rank];
        if !(top.class == MaterialBehaviorClass::MixedUnknown
            || top
                .probability_milli
                .saturating_sub(intended_evidence.probability_milli)
                < tolerance.minimum_top_margin_milli
            || top.support_milli < tolerance.minimum_hard_class_confidence_milli)
        {
            return Some(format!(
                "{intended:?}: MixedUnknown is outside declared margin/support tolerance"
            ));
        }
        None
    }

    struct InvalidProvider {
        interface_version: u16,
    }
    impl LocalClassifierProvider for InvalidProvider {
        fn descriptor(&self) -> ClassifierProviderDescriptor {
            ClassifierProviderDescriptor {
                provider_id: "invalid-local".into(),
                provider_version: "1".into(),
                interface_version: self.interface_version,
                model_digest: ContentDigest::sha256(b"invalid-local"),
            }
        }

        fn classify(
            &self,
            _request: ClassifierProviderRequest<'_>,
            _cancellation: &RenderCancellationToken,
        ) -> Result<ClassifierProviderOutput, ClassifierProviderError> {
            Ok(ClassifierProviderOutput {
                distribution: Vec::new(),
                output_version: CLASSIFIER_PROVIDER_INTERFACE_VERSION,
            })
        }
    }

    struct CancellingProvider;
    impl LocalClassifierProvider for CancellingProvider {
        fn descriptor(&self) -> ClassifierProviderDescriptor {
            ClassifierProviderDescriptor {
                provider_id: "cancel-local".into(),
                provider_version: "1".into(),
                interface_version: CLASSIFIER_PROVIDER_INTERFACE_VERSION,
                model_digest: ContentDigest::sha256(b"cancel-local"),
            }
        }
        fn classify(
            &self,
            _request: ClassifierProviderRequest<'_>,
            _cancellation: &RenderCancellationToken,
        ) -> Result<ClassifierProviderOutput, ClassifierProviderError> {
            Err(ClassifierProviderError::Cancelled)
        }
    }

    struct UnboundedProvider;
    impl LocalClassifierProvider for UnboundedProvider {
        fn descriptor(&self) -> ClassifierProviderDescriptor {
            ClassifierProviderDescriptor {
                provider_id: "unbounded-local".into(),
                provider_version: "1".into(),
                interface_version: CLASSIFIER_PROVIDER_INTERFACE_VERSION,
                model_digest: ContentDigest::sha256(b"unbounded-local"),
            }
        }
        fn classify(
            &self,
            request: ClassifierProviderRequest<'_>,
            _cancellation: &RenderCancellationToken,
        ) -> Result<ClassifierProviderOutput, ClassifierProviderError> {
            let mut distribution = request.heuristic_distribution.to_vec();
            distribution[0].support_milli = 1001;
            Ok(ClassifierProviderOutput {
                distribution,
                output_version: CLASSIFIER_PROVIDER_INTERFACE_VERSION,
            })
        }
    }

    #[test]
    fn algorithm_stage_05_quality_classification() {
        let cancellation = RenderCancellationToken::new();
        let settings = AnalysisSettings::default();

        // Reports are deterministic, bounded, complete, and cache-addressable from pixels/settings/version.
        let stochastic = source_from_signal(&stochastic_signal(), None);
        let first = analyze_source(&stochastic, &settings, None, &cancellation).unwrap();
        let second = analyze_source(&stochastic, &settings, None, &cancellation).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            first.classification.distribution.len(),
            MaterialBehaviorClass::ALL.len()
        );
        assert_eq!(
            first
                .classification
                .distribution
                .iter()
                .map(|entry| u32::from(entry.probability_milli))
                .sum::<u32>(),
            1000
        );
        assert!(
            first
                .classification
                .distribution
                .windows(2)
                .all(|pair| pair[0].probability_milli >= pair[1].probability_milli)
        );
        assert!(first.quality.sharpness_milli <= 1000 && first.quality.noise_milli <= 1000);
        let mut cache = SourceAnalysisCache::default();
        cache.insert_complete(first.clone());
        assert_eq!(cache.get(&first.cache_key), Some(&first));

        // The universal corpus and the typed behavior vocabulary agree exactly. Each corpus class
        // is then exercised by image evidence; no identifier is supplied to `analyze_source`.
        let corpus = MaterialCorpusManifest::bundled().unwrap();
        let corpus_ids: BTreeSet<_> = corpus
            .behavior_classes
            .iter()
            .map(|entry| entry.id.clone())
            .collect();
        let typed_ids: BTreeSet<_> = MaterialBehaviorClass::ALL
            .into_iter()
            .map(|class| {
                serde_json::to_string(&class)
                    .unwrap()
                    .trim_matches('"')
                    .to_owned()
            })
            .collect();
        assert_eq!(corpus_ids, typed_ids);

        // Generate every checked-in Stage 0 fixture and verify only the measurable feature
        // evidence declared by that fixture. Stage 0 feature generators do not assign semantics.
        let generated_fixtures: Vec<_> = corpus
            .synthetic_fixtures
            .iter()
            .map(|spec| spec.generate())
            .collect();
        assert_eq!(generated_fixtures.len(), corpus.synthetic_fixtures.len());
        let mut fixture_reports = Vec::new();
        for fixture in &generated_fixtures {
            assert_eq!(
                fixture.planes["base_color"].len(),
                usize::try_from(fixture.spec.width * fixture.spec.height).unwrap()
            );
            let report = analyze_source(
                &source_from_fixture(fixture),
                &settings,
                None,
                &cancellation,
            )
            .unwrap();
            fixture_reports.push((fixture.spec.generator, report));
        }
        let fixture_report = |generator| {
            &fixture_reports
                .iter()
                .find(|(candidate, _)| *candidate == generator)
                .expect("complete Stage 0 fixture set")
                .1
        };
        assert!(
            fixture_report(SyntheticGenerator::Structure)
                .classification
                .measurements
                .localized_saliency_milli
                > 300
        );
        assert!(
            fixture_report(SyntheticGenerator::Orientation)
                .classification
                .measurements
                .orientation_coherence_milli
                > 600,
            "oblique corpus orientation lost tensor coherence"
        );
        let periodic = fixture_report(SyntheticGenerator::Periodicity)
            .classification
            .measurements;
        assert!(periodic.periodic_x_milli > 800 && periodic.periodic_y_milli > 800);
        assert!(
            fixture_report(SyntheticGenerator::Saliency)
                .classification
                .measurements
                .localized_saliency_milli
                > fixture_report(SyntheticGenerator::Registration)
                    .classification
                    .measurements
                    .localized_saliency_milli
                    + 100
        );
        let registered = fixture_report(SyntheticGenerator::Registration);
        assert_eq!(registered.quality.registration_quality_milli, 1000);
        assert_eq!(registered.quality.registration_worst_offset_px, (0, 0));

        let reports: BTreeMap<_, _> = MaterialBehaviorClass::ALL
            .into_iter()
            .map(|class| {
                let report = analyze_source(
                    &source_from_signal(&behavior_signal(class), None),
                    &settings,
                    None,
                    &cancellation,
                )
                .unwrap();
                (class, report)
            })
            .collect();
        let measurement = |class| reports[&class].classification.measurements;
        eprintln!(
            "intended | analyzed | top three probability/support | boundary orientation variation px py bands regularity saliency radial stationarity | MixedUnknown reason"
        );
        for class in MaterialBehaviorClass::ALL {
            let report = &reports[&class];
            let top = report.classification.distribution[0];
            let second = report.classification.distribution[1];
            let third = report.classification.distribution[2];
            let mixed_reason = if report.classification.analyzed_class
                != MaterialBehaviorClass::MixedUnknown
            {
                "not selected".to_owned()
            } else if top.class == MaterialBehaviorClass::MixedUnknown {
                "MixedUnknown ranked first".to_owned()
            } else if top.support_milli < settings.tolerance.minimum_hard_class_confidence_milli {
                format!(
                    "top support {} < {}",
                    top.support_milli, settings.tolerance.minimum_hard_class_confidence_milli
                )
            } else {
                format!(
                    "top margin {} < {}",
                    top.probability_milli
                        .saturating_sub(second.probability_milli),
                    settings.tolerance.minimum_top_margin_milli,
                )
            };
            let m = report.classification.measurements;
            eprintln!(
                "{:?} | {:?} | {:?} {}/{}; {:?} {}/{}; {:?} {}/{} | {} {} {} {} {} {} {} {} {} {} | {}",
                class,
                report.classification.analyzed_class,
                top.class,
                top.probability_milli,
                top.support_milli,
                second.class,
                second.probability_milli,
                second.support_milli,
                third.class,
                third.probability_milli,
                third.support_milli,
                m.boundary_agreement_milli,
                m.orientation_coherence_milli,
                m.orientation_variation_milli,
                m.periodic_x_milli,
                m.periodic_y_milli,
                m.bandedness_milli,
                m.regularity_milli,
                m.localized_saliency_milli,
                m.radial_symmetry_milli,
                m.stationarity_milli,
                mixed_reason,
            );
        }
        let mut calibration_failures = Vec::new();
        let measurement_checks = [
            (
                measurement(MaterialBehaviorClass::AlreadyTileable).boundary_agreement_milli > 800,
                "AlreadyTileable: boundary agreement must exceed 800",
            ),
            (
                measurement(MaterialBehaviorClass::StochasticIsotropic).orientation_coherence_milli
                    < 300,
                "StochasticIsotropic: orientation coherence must be below 300",
            ),
            (
                measurement(MaterialBehaviorClass::StochasticDirectional)
                    .orientation_coherence_milli
                    > 600,
                "StochasticDirectional: orientation coherence must exceed 600",
            ),
            (
                measurement(MaterialBehaviorClass::PeriodicLatticeStructured).periodic_x_milli
                    > 800
                    && measurement(MaterialBehaviorClass::PeriodicLatticeStructured)
                        .periodic_y_milli
                        > 800,
                "PeriodicLatticeStructured: both periods must exceed 800",
            ),
            (
                measurement(MaterialBehaviorClass::LayeredBanded).bandedness_milli > 700,
                "LayeredBanded: bandedness must exceed 700",
            ),
            (
                measurement(MaterialBehaviorClass::OrganicDirectional).orientation_variation_milli
                    > 20,
                "OrganicDirectional: orientation variation must exceed 20",
            ),
            (
                measurement(MaterialBehaviorClass::ManufacturedPattern).periodic_x_milli > 800,
                "ManufacturedPattern: X period must exceed 800",
            ),
            (
                measurement(MaterialBehaviorClass::UniqueDetail).localized_saliency_milli > 500,
                "UniqueDetail: localized saliency must exceed 500",
            ),
            (
                measurement(MaterialBehaviorClass::RadialDetail).radial_symmetry_milli > 800,
                "RadialDetail: radial symmetry must exceed 800",
            ),
        ];
        calibration_failures.extend(
            measurement_checks
                .into_iter()
                .filter_map(|(passed, message)| (!passed).then(|| message.to_owned())),
        );
        for class in MaterialBehaviorClass::ALL {
            let report = &reports[&class];
            if !report
                .classification
                .distribution
                .iter()
                .any(|entry| entry.class == class)
            {
                calibration_failures.push(format!("{class:?}: distribution is incomplete"));
            }
            if class_support(report, class) == 0 {
                calibration_failures.push(format!("{class:?}: heuristic support is zero"));
            }
            match class {
                MaterialBehaviorClass::AlreadyTileable
                | MaterialBehaviorClass::StochasticIsotropic
                | MaterialBehaviorClass::StochasticDirectional
                | MaterialBehaviorClass::LayeredBanded
                | MaterialBehaviorClass::UniqueDetail => {
                    if report.classification.analyzed_class != class {
                        calibration_failures.push(format!(
                            "{class:?}: nonambiguous fixture analyzed as {:?}",
                            report.classification.analyzed_class
                        ));
                    }
                }
                MaterialBehaviorClass::PeriodicLatticeStructured => {
                    if report.classification.analyzed_class == MaterialBehaviorClass::MixedUnknown {
                        if let Some(error) =
                            dominant_or_ambiguous_error(report, class, settings.tolerance)
                        {
                            calibration_failures.push(error);
                        }
                    } else if !matches!(
                        report.classification.analyzed_class,
                        MaterialBehaviorClass::PeriodicLatticeStructured
                            | MaterialBehaviorClass::ManufacturedPattern
                    ) {
                        calibration_failures.push(format!(
                            "{class:?}: unacceptable overlap class {:?}",
                            report.classification.analyzed_class
                        ));
                    }
                }
                MaterialBehaviorClass::ManufacturedPattern => {
                    if report.classification.analyzed_class == MaterialBehaviorClass::MixedUnknown {
                        if let Some(error) =
                            dominant_or_ambiguous_error(report, class, settings.tolerance)
                        {
                            calibration_failures.push(error);
                        }
                    } else if !matches!(
                        report.classification.analyzed_class,
                        MaterialBehaviorClass::ManufacturedPattern
                            | MaterialBehaviorClass::LayeredBanded
                    ) {
                        calibration_failures.push(format!(
                            "{class:?}: unacceptable overlap class {:?}",
                            report.classification.analyzed_class
                        ));
                    }
                }
                MaterialBehaviorClass::OrganicDirectional | MaterialBehaviorClass::RadialDetail => {
                    if report.classification.analyzed_class != class {
                        if let Some(error) =
                            dominant_or_ambiguous_error(report, class, settings.tolerance)
                        {
                            calibration_failures.push(error);
                        }
                    }
                }
                MaterialBehaviorClass::MixedUnknown => {
                    if report.classification.analyzed_class != MaterialBehaviorClass::MixedUnknown {
                        calibration_failures.push(format!(
                            "MixedUnknown: analyzed as {:?}",
                            report.classification.analyzed_class
                        ));
                    }
                }
            }
        }
        if class_support(
            &reports[&MaterialBehaviorClass::PeriodicLatticeStructured],
            MaterialBehaviorClass::PeriodicLatticeStructured,
        ) <= class_support(&first, MaterialBehaviorClass::PeriodicLatticeStructured)
        {
            calibration_failures
                .push("Periodic lattice support must exceed stochastic baseline".into());
        }
        assert!(
            calibration_failures.is_empty(),
            "behavior calibration failures:\n{}",
            calibration_failures.join("\n")
        );

        // Naturally composite evidence remains Mixed/Unknown under the declared default tolerance.
        let mut ambiguous = reports[&MaterialBehaviorClass::MixedUnknown].clone();
        assert_eq!(
            ambiguous.classification.analyzed_class,
            MaterialBehaviorClass::MixedUnknown
        );
        let measured_quality = ambiguous.quality.clone();
        let measured_distribution = ambiguous.classification.distribution.clone();
        ambiguous.apply_command(ClassificationCommand::Override {
            class: MaterialBehaviorClass::OrganicDirectional,
        });
        assert_eq!(
            ambiguous.classification.routed_class(),
            MaterialBehaviorClass::OrganicDirectional
        );
        assert_eq!(
            ambiguous.classification.analyzed_class,
            MaterialBehaviorClass::MixedUnknown
        );
        assert_eq!(ambiguous.quality, measured_quality);
        assert_eq!(ambiguous.classification.distribution, measured_distribution);
        ambiguous.apply_command(ClassificationCommand::ResetToAnalysis);
        assert_eq!(
            ambiguous.classification.routed_class(),
            MaterialBehaviorClass::MixedUnknown
        );

        // Every report-changing input participates in the cache key.
        let registration_fixture = generated_fixtures
            .iter()
            .find(|fixture| fixture.spec.generator == SyntheticGenerator::Registration)
            .expect("Stage 0 registration fixture");
        let registration_signal = fixture_signal(registration_fixture);
        let aligned = analyze_source(
            &source_from_fixture(registration_fixture),
            &settings,
            None,
            &cancellation,
        )
        .unwrap();
        let companion_changed = analyze_source(
            &source_from_signal(&registration_signal, Some((3, -2))),
            &settings,
            None,
            &cancellation,
        )
        .unwrap();
        assert_ne!(aligned.cache_key, companion_changed.cache_key);
        let mut coverage_changed_source = stochastic.clone();
        coverage_changed_source.coverage = Some(
            ImagePlane::from_row_major(
                WIDTH,
                HEIGHT,
                8,
                &vec![MaskValue(0.0); (WIDTH * HEIGHT) as usize],
            )
            .unwrap(),
        );
        let coverage_changed =
            analyze_source(&coverage_changed_source, &settings, None, &cancellation).unwrap();
        assert_ne!(first.cache_key, coverage_changed.cache_key);
        let mut perspective_changed_source = stochastic.clone();
        perspective_changed_source.perspective_confidence_milli = 120;
        let perspective_changed =
            analyze_source(&perspective_changed_source, &settings, None, &cancellation).unwrap();
        assert_ne!(first.cache_key, perspective_changed.cache_key);
        let provider_v1 = InvalidProvider {
            interface_version: CLASSIFIER_PROVIDER_INTERFACE_VERSION,
        };
        let provider_v2 = InvalidProvider {
            interface_version: CLASSIFIER_PROVIDER_INTERFACE_VERSION + 1,
        };
        assert_ne!(
            analyze_source(&stochastic, &settings, Some(&provider_v1), &cancellation)
                .unwrap()
                .cache_key,
            analyze_source(&stochastic, &settings, Some(&provider_v2), &cancellation)
                .unwrap()
                .cache_key,
        );

        // Registration is inferred from image correspondence in declared source-pixel units,
        // even when the analysis grid is downsampled.
        let downsampled_settings = AnalysisSettings {
            max_analysis_edge: 32,
            ..settings
        };
        let shifted = analyze_source(
            &source_from_signal(&registration_signal, Some((3, -2))),
            &downsampled_settings,
            None,
            &cancellation,
        )
        .unwrap();
        assert_eq!(shifted.quality.registration_worst_offset_px, (3, -2));
        assert!(
            shifted.quality.registration_quality_milli
                < settings.thresholds.minimum_registration_quality_milli
        );
        assert!(shifted.warnings.iter().any(|warning| warning.code
            == QualityWarningCode::MisregisteredChannels
            && warning.recoveries.contains(&RecoveryChoice::AdjustSettings)));

        // A malformed local model cannot suppress the deterministic heuristic fallback.
        let fallback =
            analyze_source(&stochastic, &settings, Some(&provider_v1), &cancellation).unwrap();
        assert_eq!(
            fallback.classification.authority,
            ClassificationAuthority::DeterministicHeuristics
        );
        assert_eq!(
            fallback.classification.distribution,
            first.classification.distribution
        );
        let unbounded = analyze_source(
            &stochastic,
            &settings,
            Some(&UnboundedProvider),
            &cancellation,
        )
        .unwrap();
        assert_eq!(
            unbounded.classification.authority,
            ClassificationAuthority::DeterministicHeuristics
        );
        assert!(matches!(
            analyze_source(
                &stochastic,
                &settings,
                Some(&CancellingProvider),
                &cancellation
            ),
            Err(SourceAnalysisError::Cancelled),
        ));
        let inspector = shifted.inspector_evidence();
        assert!(inspector.quality_summary.contains("registration"));
        assert!(inspector.evidence_summary.contains("periods"));
    }
}
