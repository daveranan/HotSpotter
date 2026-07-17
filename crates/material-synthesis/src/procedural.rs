//! Stage 8E bounded, evidence-fitted procedural material reconstruction.

use std::{collections::{BTreeMap, BTreeSet}, sync::Arc};

use hot_trimmer_domain::{
    AlgorithmProvenance, CompilationDiagnostic, ContentDigest, DiagnosticCode,
    MaterialBehaviorClass, MaterialChannelRole, RecoveryChoice, ScaleProvenance, StageResult,
};
use hot_trimmer_image_io::{
    ImagePlane, LinearColor, LinearScalar, MaskValue, NormalAlphaPolicy, ResolvedAlphaMode,
    TangentNormal,
};
use hot_trimmer_material_analysis::{
    DelitPreparedExemplar, FeatureFieldReport, ScaleOrientationReport, SourceAnalysisReport,
};
use hot_trimmer_render_core::{PreparedExemplarChannel, RenderCancellationToken};
use thiserror::Error;

pub const STAGE_08E_PROCEDURAL_ALGORITHM_ID: &str = "hot_trimmer.fitted_procedural_material";
pub const STAGE_08E_PROCEDURAL_ALGORITHM_VERSION: &str = "8.6.0";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ProceduralModelKind {
    WoodFaceGrain,
    WoodEndGrain,
    BrickTileLattice,
    Corrugation,
    BrushedMetal,
    ConcreteAggregate,
    PaintedMetalLayers,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProceduralUnits { PhysicalMicrometers, RelativeMillionths }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MotifScale {
    pub x: u64,
    pub y: u64,
    pub units: ProceduralUnits,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ParameterAuthority { Measured, UserOverride, EstimatedFromMeasuredEvidence }

#[derive(Clone, Debug, PartialEq)]
pub struct ProceduralParameterEvidence {
    pub analyzed_behavior: MaterialBehaviorClass,
    pub routed_behavior: MaterialBehaviorClass,
    pub classification_confidence_milli: u16,
    pub measured_orientation_millidegrees: Option<u32>,
    pub measured_period_pixels: Option<(i32, i32)>,
    pub scale_provenance: ScaleProvenance,
    /// Measurements are retained even when effective parameters are corrected by the user.
    pub measured_parameters: MeasuredProceduralParameters,
    pub effective_authorities: BTreeMap<ProceduralParameterName, ParameterAuthority>,
    pub overridden_parameters: BTreeSet<ProceduralParameterName>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ProceduralParameterName { ModelKind, Orientation, MotifScale, Palette, Roughness, Noise }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProceduralPalette {
    pub dark: [f32; 3],
    pub mid: [f32; 3],
    pub light: [f32; 3],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProceduralLayerParameters {
    pub primary_width_milli: u16,
    pub secondary_width_milli: u16,
    pub density_milli: u16,
    pub noise_milli: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FittedProceduralParameters {
    pub kind: ProceduralModelKind,
    pub orientation_millidegrees: u32,
    pub motif_scale: MotifScale,
    pub palette: ProceduralPalette,
    pub roughness_milli: u16,
    pub layers: ProceduralLayerParameters,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MeasuredProceduralParameters {
    pub inferred_kind: Option<ProceduralModelKind>,
    pub orientation_millidegrees: Option<u32>,
    pub motif_scale: MotifScale,
    pub palette: ProceduralPalette,
    pub roughness_milli: u16,
    pub noise_milli: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProceduralContentIntent { MaterialOnly, ContainsSemanticDetail }

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProceduralUserOverride {
    pub model_kind: Option<ProceduralModelKind>,
    pub orientation_millidegrees: Option<u32>,
    pub motif_scale: Option<MotifScale>,
    pub palette: Option<ProceduralPalette>,
    pub roughness_milli: Option<u16>,
    pub noise_milli: Option<u16>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProceduralLimits {
    pub max_output_pixels: u64,
    pub max_working_bytes: u64,
    pub max_operations: u64,
    pub max_fit_samples: u32,
}

impl Default for ProceduralLimits {
    fn default() -> Self {
        Self { max_output_pixels: 16_777_216, max_working_bytes: 1_073_741_824,
            max_operations: 1_000_000_000, max_fit_samples: 65_536 }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProceduralFitSettings {
    pub minimum_confidence_milli: u16,
    pub limits: ProceduralLimits,
}

impl Default for ProceduralFitSettings {
    fn default() -> Self { Self { minimum_confidence_milli: 260, limits: ProceduralLimits::default() } }
}

#[derive(Clone, Debug)]
pub struct ProceduralFitRequest {
    pub source: Arc<DelitPreparedExemplar>,
    pub stage_five: Arc<SourceAnalysisReport>,
    pub stage_six: Arc<ScaleOrientationReport>,
    pub stage_seven: Arc<FeatureFieldReport>,
    pub user_override: Option<ProceduralUserOverride>,
    pub content_intent: ProceduralContentIntent,
    pub seed: u64,
    pub settings: ProceduralFitSettings,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProceduralCoordinateTopology { Cartesian, PolarCompatible }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProceduralDomainSpec {
    pub width: u32,
    pub height: u32,
    /// Domain extent in the model's fitted units, independent of output pixels.
    pub extent_x: u64,
    pub extent_y: u64,
    pub topology: ProceduralCoordinateTopology,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ProceduralContribution { BaseColor, Height, Normal, Roughness, Metallic, AmbientOcclusion, LayerMask }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProceduralQaView { ParameterSummary, CoordinateGrid, MotifScale, Orientation, LayerMasks, ChannelRegistration }

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProceduralDiagnostic {
    pub code: ProceduralDiagnosticCode,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProceduralDiagnosticCode {
    RelativeScaleOnly, EstimatedPbrContribution, UserCorrectionApplied,
    SemanticDetailExcluded, EndGrainEstimated, ConfidenceBelowThreshold,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProceduralUnsupportedState {
    BehaviorNotProcedural,
    ConfidenceInsufficient,
    SemanticDetailRequiresDetailContract,
    EndGrainRequiresExplicitModelOrSource,
    MissingPeriodEvidence,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnsupportedProceduralModel {
    pub state: ProceduralUnsupportedState,
    pub message: String,
    pub recovery_choices: Vec<RecoveryChoice>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ProceduralFitOutcome {
    Fitted(FittedProceduralDomainModel),
    Unsupported(UnsupportedProceduralModel),
}

#[derive(Clone, Debug, PartialEq)]
pub struct FittedProceduralDomainModel {
    pub parameters: FittedProceduralParameters,
    pub confidence_milli: u16,
    pub evidence: ProceduralParameterEvidence,
    pub seed: u64,
    pub version: &'static str,
    pub channel_outputs: BTreeSet<ProceduralContribution>,
    pub diagnostics: Vec<ProceduralDiagnostic>,
    pub qa_views: Vec<ProceduralQaView>,
    pub source_registration_digest: ContentDigest,
    pub stage_result: StageResult,
    limits: ProceduralLimits,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneratedProceduralDomain {
    pub width: u32,
    pub height: u32,
    pub units: ProceduralUnits,
    pub topology: ProceduralCoordinateTopology,
    pub channels: Vec<PreparedExemplarChannel>,
    pub layer_mask: ImagePlane<MaskValue>,
    pub model_digest: ContentDigest,
    pub qa_views: Vec<ProceduralQaView>,
    pub stage_result: StageResult,
}

pub trait ProceduralDomainModel {
    fn kind(&self) -> ProceduralModelKind;
    fn parameters(&self) -> &FittedProceduralParameters;
    fn confidence_milli(&self) -> u16;
    fn channel_outputs(&self) -> &BTreeSet<ProceduralContribution>;
    fn diagnostics(&self) -> &[ProceduralDiagnostic];
    fn generate(&self, domain: ProceduralDomainSpec, cancellation: &RenderCancellationToken)
        -> Result<GeneratedProceduralDomain, ProceduralError>;
}

impl ProceduralDomainModel for FittedProceduralDomainModel {
    fn kind(&self) -> ProceduralModelKind { self.parameters.kind }
    fn parameters(&self) -> &FittedProceduralParameters { &self.parameters }
    fn confidence_milli(&self) -> u16 { self.confidence_milli }
    fn channel_outputs(&self) -> &BTreeSet<ProceduralContribution> { &self.channel_outputs }
    fn diagnostics(&self) -> &[ProceduralDiagnostic] { &self.diagnostics }
    fn generate(&self, domain: ProceduralDomainSpec, cancellation: &RenderCancellationToken)
        -> Result<GeneratedProceduralDomain, ProceduralError> { generate_model(self, domain, cancellation) }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ProceduralError {
    #[error("procedural settings or domain are outside bounded ranges")]
    InvalidSettings,
    #[error("Stage 5-7 evidence is not registered to the requested prepared source")]
    RegistrationDrift,
    #[error("procedural reconstruction was cancelled")]
    Cancelled,
    #[error("procedural reconstruction exceeds its declared resource limits")]
    ResourceLimitExceeded,
    #[error("procedural output plane construction failed")]
    PlaneConstruction,
}

pub fn fit_procedural_domain_model(
    request: &ProceduralFitRequest,
    cancellation: &RenderCancellationToken,
)
    -> Result<ProceduralFitOutcome, ProceduralError>
{
    validate_fit_request(request)?;
    check_fit_cancel(cancellation)?;
    if request.content_intent == ProceduralContentIntent::ContainsSemanticDetail
        || matches!(request.stage_five.classification.routed_class(), MaterialBehaviorClass::UniqueDetail)
    {
        return Ok(ProceduralFitOutcome::Unsupported(UnsupportedProceduralModel {
            state: ProceduralUnsupportedState::SemanticDetailRequiresDetailContract,
            message: "vents, labels, doors, text, and other semantic content remain Stage 16 detail/unique-content concerns".into(),
            recovery_choices: vec![RecoveryChoice::ChooseAnotherSource, RecoveryChoice::DisableEffect],
        }));
    }
    let override_value = request.user_override.as_ref();
    let routed = request.stage_five.classification.routed_class();
    let kind = if let Some(kind) = override_value.and_then(|value| value.model_kind) { kind }
        else if routed == MaterialBehaviorClass::RadialDetail {
            return Ok(ProceduralFitOutcome::Unsupported(UnsupportedProceduralModel {
                state: ProceduralUnsupportedState::EndGrainRequiresExplicitModelOrSource,
                message: "radial evidence is not enough to claim authentic end grain; choose the typed estimated end-grain model or provide an end-grain source".into(),
                recovery_choices: vec![RecoveryChoice::ChooseAnotherSource, RecoveryChoice::AdjustSettings],
            }));
        } else if let Some(kind) = kind_from_evidence(routed) { kind }
        else {
            return Ok(ProceduralFitOutcome::Unsupported(UnsupportedProceduralModel {
                state: ProceduralUnsupportedState::BehaviorNotProcedural,
                message: "measured behavior does not support a V1 procedural material family".into(),
                recovery_choices: vec![RecoveryChoice::UseSynthesis, RecoveryChoice::ChooseAnotherSource],
            }));
        };
    let confidence = request.stage_five.classification.confidence_milli
        .min(request.stage_six.global_orientation.confidence_milli.max(300));
    let explicit_family = override_value.and_then(|value| value.model_kind).is_some();
    if confidence < request.settings.minimum_confidence_milli && !explicit_family {
        return Ok(ProceduralFitOutcome::Unsupported(UnsupportedProceduralModel {
            state: ProceduralUnsupportedState::ConfidenceInsufficient,
            message: format!("measured procedural confidence {confidence} is below threshold {}", request.settings.minimum_confidence_milli),
            recovery_choices: vec![RecoveryChoice::AdjustSettings, RecoveryChoice::UseSynthesis, RecoveryChoice::ChooseAnotherSource],
        }));
    }
    let lattice = request.stage_seven.periodicity.candidates.first();
    if matches!(kind, ProceduralModelKind::BrickTileLattice | ProceduralModelKind::Corrugation)
        && lattice.is_none() && override_value.and_then(|value| value.motif_scale).is_none()
    {
        return Ok(ProceduralFitOutcome::Unsupported(UnsupportedProceduralModel {
            state: ProceduralUnsupportedState::MissingPeriodEvidence,
            message: "structured procedural reconstruction requires measured lattice/period evidence or a typed motif-scale correction".into(),
            recovery_choices: vec![RecoveryChoice::AdjustSettings, RecoveryChoice::ChooseAnotherSource],
        }));
    }

    fit_preflight(request)?;

    let mut overridden = BTreeSet::new();
    if override_value.and_then(|v| v.model_kind).is_some() { overridden.insert(ProceduralParameterName::ModelKind); }
    if override_value.and_then(|v| v.orientation_millidegrees).is_some() { overridden.insert(ProceduralParameterName::Orientation); }
    if override_value.and_then(|v| v.motif_scale).is_some() { overridden.insert(ProceduralParameterName::MotifScale); }
    if override_value.and_then(|v| v.palette).is_some() { overridden.insert(ProceduralParameterName::Palette); }
    if override_value.and_then(|v| v.roughness_milli).is_some() { overridden.insert(ProceduralParameterName::Roughness); }
    if override_value.and_then(|v| v.noise_milli).is_some() { overridden.insert(ProceduralParameterName::Noise); }
    let measured_period = lattice.map(|candidate| (candidate.first.dx_pixels, candidate.first.dy_pixels));
    let measured_scale = fitted_scale(request.stage_six.as_ref(), measured_period, request.source.base_color().dimensions());
    let measured_palette = fit_palette(request.source.base_color(), request.settings.limits.max_fit_samples, cancellation)?;
    let measured_orientation = request.stage_six.global_orientation.axis_millidegrees;
    let measured_roughness = fit_roughness(&request.source, cancellation)?;
    let measured_noise = request.stage_five.quality.noise_milli.min(1000);
    let measured_parameters = MeasuredProceduralParameters {
        inferred_kind: kind_from_evidence(request.stage_five.classification.analyzed_class),
        orientation_millidegrees: measured_orientation,
        motif_scale: measured_scale,
        palette: measured_palette,
        roughness_milli: measured_roughness,
        noise_milli: measured_noise,
    };
    let motif_scale = override_value.and_then(|v| v.motif_scale).unwrap_or(measured_scale);
    let palette = override_value.and_then(|v| v.palette).unwrap_or(measured_palette);
    let orientation = override_value.and_then(|v| v.orientation_millidegrees)
        .or(measured_orientation).unwrap_or(0);
    let roughness = override_value.and_then(|v| v.roughness_milli).unwrap_or(measured_roughness);
    let noise = override_value.and_then(|v| v.noise_milli).unwrap_or(measured_noise);
    let layers = fit_layers(kind, request.stage_seven.as_ref(), noise, cancellation)?;
    let effective_authorities = parameter_authorities(&overridden, measured_orientation.is_some());
    let evidence = ProceduralParameterEvidence {
        analyzed_behavior: request.stage_five.classification.analyzed_class,
        routed_behavior: routed,
        classification_confidence_milli: request.stage_five.classification.confidence_milli,
        measured_orientation_millidegrees: request.stage_six.global_orientation.measured_axis_millidegrees,
        measured_period_pixels: measured_period,
        scale_provenance: request.stage_six.scale.provenance,
        measured_parameters,
        effective_authorities,
        overridden_parameters: overridden.clone(),
    };
    let mut diagnostics = Vec::new();
    if motif_scale.units == ProceduralUnits::RelativeMillionths {
        diagnostics.push(ProceduralDiagnostic { code: ProceduralDiagnosticCode::RelativeScaleOnly,
            message: "motif scale is relative because Stage 6 made no world-accurate scale claim".into() });
    }
    if !overridden.is_empty() {
        diagnostics.push(ProceduralDiagnostic { code: ProceduralDiagnosticCode::UserCorrectionApplied,
            message: "typed corrections were applied while measured evidence was retained for QA".into() });
    }
    if kind == ProceduralModelKind::WoodEndGrain {
        diagnostics.push(ProceduralDiagnostic { code: ProceduralDiagnosticCode::EndGrainEstimated,
            message: "end grain is explicitly labeled as a fitted estimate, not recovered authentic source content".into() });
    }
    diagnostics.push(ProceduralDiagnostic { code: ProceduralDiagnosticCode::EstimatedPbrContribution,
        message: "generated structural/PBR contributions are registered estimates from the fitted model".into() });
    let parameters = FittedProceduralParameters { kind, orientation_millidegrees: orientation,
        motif_scale, palette, roughness_milli: roughness, layers };
    let digest = model_digest(&parameters, request.seed, &request.stage_seven.registration_digest);
    let compilation_diagnostics = diagnostics.iter().map(|item| CompilationDiagnostic {
        code: DiagnosticCode::InsufficientInput, stage: Some(8), message: item.message.clone(), context: BTreeMap::new(),
    }).collect();
    Ok(ProceduralFitOutcome::Fitted(FittedProceduralDomainModel {
        parameters, confidence_milli: confidence, evidence, seed: request.seed,
        version: STAGE_08E_PROCEDURAL_ALGORITHM_VERSION,
        channel_outputs: outputs_for(kind), diagnostics,
        qa_views: vec![ProceduralQaView::ParameterSummary, ProceduralQaView::CoordinateGrid,
            ProceduralQaView::MotifScale, ProceduralQaView::Orientation,
            ProceduralQaView::LayerMasks, ProceduralQaView::ChannelRegistration],
        source_registration_digest: request.stage_seven.registration_digest.clone(),
        stage_result: StageResult::Executed { algorithm: AlgorithmProvenance {
            algorithm_id: STAGE_08E_PROCEDURAL_ALGORITHM_ID.into(), version: STAGE_08E_PROCEDURAL_ALGORITHM_VERSION.into(),
        }, settings_hash: digest, diagnostics: compilation_diagnostics }, limits: request.settings.limits,
    }))
}

fn validate_fit_request(request: &ProceduralFitRequest) -> Result<(), ProceduralError> {
    if request.settings.minimum_confidence_milli > 1000 || request.settings.limits.max_output_pixels == 0
        || request.settings.limits.max_working_bytes == 0 || request.settings.limits.max_operations == 0
        || request.settings.limits.max_fit_samples == 0 { return Err(ProceduralError::InvalidSettings); }
    if request.source.prepared_source_digest != request.stage_five.prepared_source_digest
        || request.source.prepared_source_digest != request.stage_six.prepared_source_digest
        || request.source.prepared_source_digest != request.stage_seven.prepared_source_digest
        || request.stage_six.stage_five_cache_key != request.stage_five.cache_key
        || request.stage_seven.stage_six_cache_key != request.stage_six.cache_key
    {
        return Err(ProceduralError::RegistrationDrift);
    }
    if let Some(value) = &request.user_override {
        let palette_valid = value.palette.is_none_or(|palette| [palette.dark, palette.mid, palette.light]
            .into_iter().flatten().all(|component| component.is_finite() && (0.0..=1.0).contains(&component)));
        if value.orientation_millidegrees.is_some_and(|angle| angle >= 180_000)
            || value.motif_scale.is_some_and(|scale| scale.x == 0 || scale.y == 0)
            || value.roughness_milli.is_some_and(|roughness| roughness > 1000)
            || value.noise_milli.is_some_and(|noise| noise > 1000)
            || !palette_valid
        { return Err(ProceduralError::InvalidSettings); }
    }
    Ok(())
}

fn fit_preflight(request: &ProceduralFitRequest) -> Result<(), ProceduralError> {
    let base_pixels = u64::from(request.source.base_color().width())
        .checked_mul(u64::from(request.source.base_color().height())).ok_or(ProceduralError::ResourceLimitExceeded)?;
    let roughness_pixels = request.source.channels.iter().find_map(|channel| match channel {
        PreparedExemplarChannel::Scalar { role: MaterialChannelRole::Roughness, plane } =>
            Some(u64::from(plane.width()) * u64::from(plane.height())), _ => None,
    }).unwrap_or(0);
    let grid_pixels = level_zero_pixels(&request.stage_seven.structure.grid);
    let fiber_pixels = level_zero_pixels(&request.stage_seven.structure.fiber);
    let samples = base_pixels.min(u64::from(request.settings.limits.max_fit_samples));
    let sort_operations = samples.checked_mul(samples.max(1).ilog2().into())
        .ok_or(ProceduralError::ResourceLimitExceeded)?;
    let operations = base_pixels.checked_mul(5)
        .and_then(|value| value.checked_add(roughness_pixels.checked_mul(2)?))
        .and_then(|value| value.checked_add(grid_pixels.checked_mul(2)?))
        .and_then(|value| value.checked_add(fiber_pixels.checked_mul(2)?))
        .and_then(|value| value.checked_add(sort_operations))
        .ok_or(ProceduralError::ResourceLimitExceeded)?;
    let bytes = samples.checked_mul(16).ok_or(ProceduralError::ResourceLimitExceeded)?;
    if operations > request.settings.limits.max_operations || bytes > request.settings.limits.max_working_bytes {
        Err(ProceduralError::ResourceLimitExceeded)
    } else { Ok(()) }
}

fn level_zero_pixels<T>(pyramid: &hot_trimmer_image_io::ResolutionPyramid<T>) -> u64 {
    pyramid.levels().first().map_or(0, |plane| u64::from(plane.width()) * u64::from(plane.height()))
}

fn check_fit_cancel(cancellation: &RenderCancellationToken) -> Result<(), ProceduralError> {
    if cancellation.is_cancelled() { Err(ProceduralError::Cancelled) } else { Ok(()) }
}

fn parameter_authorities(overridden: &BTreeSet<ProceduralParameterName>, measured_orientation: bool)
    -> BTreeMap<ProceduralParameterName, ParameterAuthority>
{
    [ProceduralParameterName::ModelKind, ProceduralParameterName::Orientation,
        ProceduralParameterName::MotifScale, ProceduralParameterName::Palette,
        ProceduralParameterName::Roughness, ProceduralParameterName::Noise].into_iter().map(|name| {
        let authority = if overridden.contains(&name) { ParameterAuthority::UserOverride }
            else if name == ProceduralParameterName::Orientation && !measured_orientation {
                ParameterAuthority::EstimatedFromMeasuredEvidence
            } else { ParameterAuthority::Measured };
        (name, authority)
    }).collect()
}

fn kind_from_evidence(class: MaterialBehaviorClass) -> Option<ProceduralModelKind> {
    match class {
        MaterialBehaviorClass::OrganicDirectional => Some(ProceduralModelKind::WoodFaceGrain),
        MaterialBehaviorClass::PeriodicLatticeStructured => Some(ProceduralModelKind::BrickTileLattice),
        MaterialBehaviorClass::ManufacturedPattern => Some(ProceduralModelKind::Corrugation),
        MaterialBehaviorClass::StochasticDirectional => Some(ProceduralModelKind::BrushedMetal),
        MaterialBehaviorClass::StochasticIsotropic => Some(ProceduralModelKind::ConcreteAggregate),
        MaterialBehaviorClass::LayeredBanded => Some(ProceduralModelKind::PaintedMetalLayers),
        _ => None,
    }
}

fn fitted_scale(scale: &ScaleOrientationReport, period: Option<(i32, i32)>, dimensions: (u32, u32)) -> MotifScale {
    let (px, py) = period.map(|(x, y)| (x.unsigned_abs().max(1), y.unsigned_abs().max(1)))
        .unwrap_or((dimensions.0.max(1) / 8, dimensions.1.max(1) / 8));
    if scale.scale.claims_world_accuracy() {
        let sx = scale.scale.source_pixels_per_meter_x_milli.unwrap_or(1).max(1);
        let sy = scale.scale.source_pixels_per_meter_y_milli.unwrap_or(1).max(1);
        MotifScale { x: u64::from(px) * 1_000_000_000 / sx,
            y: u64::from(py) * 1_000_000_000 / sy, units: ProceduralUnits::PhysicalMicrometers }
    } else {
        MotifScale { x: u64::from(px) * 1_000_000 / u64::from(dimensions.0.max(1)),
            y: u64::from(py) * 1_000_000 / u64::from(dimensions.1.max(1)), units: ProceduralUnits::RelativeMillionths }
    }
}

fn fit_palette(
    base: &ImagePlane<LinearColor>,
    max_samples: u32,
    cancellation: &RenderCancellationToken,
) -> Result<ProceduralPalette, ProceduralError> {
    let total = u64::from(base.width()) * u64::from(base.height());
    let stride = total.div_ceil(u64::from(max_samples)).max(1);
    let mut samples = Vec::new();
    let mut index = 0_u64;
    for y in 0..base.height() {
        if y % 32 == 0 { check_fit_cancel(cancellation)?; }
        for x in 0..base.width() {
        if index % stride == 0 { samples.push(base.pixel(x, y).rgb); } index += 1;
    }}
    check_fit_cancel(cancellation)?;
    samples.sort_by(|a, b| luminance(*a).total_cmp(&luminance(*b)));
    let at = |q: usize| samples[q.min(samples.len().saturating_sub(1))];
    Ok(ProceduralPalette { dark: at(samples.len() / 10), mid: at(samples.len() / 2), light: at(samples.len() * 9 / 10) })
}

fn fit_roughness(source: &DelitPreparedExemplar, cancellation: &RenderCancellationToken)
    -> Result<u16, ProceduralError>
{
    let Some(plane) = source.channels.iter().find_map(|channel| match channel {
        PreparedExemplarChannel::Scalar { role: MaterialChannelRole::Roughness, plane } => Some(plane), _ => None,
    }) else { return Ok(500); };
    let (mut sum, mut count) = (0.0_f64, 0_u64);
    for (index, tile) in plane.tiles().iter().enumerate() {
        if index % 16 == 0 { check_fit_cancel(cancellation)?; }
        for value in &tile.pixels { sum += f64::from(value.0); count += 1; }
    }
    Ok(((sum / count.max(1) as f64) * 1000.0).round().clamp(0.0, 1000.0) as u16)
}

fn fit_layers(kind: ProceduralModelKind, fields: &FeatureFieldReport, noise: u16,
    cancellation: &RenderCancellationToken) -> Result<ProceduralLayerParameters, ProceduralError>
{
    let level = fields.structure.grid.levels().first();
    let grid = match level { Some(plane) => mean_scalar(plane, cancellation)?, None => 0.0 };
    let fiber = match fields.structure.fiber.levels().first() {
        Some(plane) => mean_scalar(plane, cancellation)?, None => 0.0,
    };
    let (primary, secondary, density) = match kind {
        ProceduralModelKind::WoodFaceGrain => (90, 18, (fiber * 1000.0) as u16),
        ProceduralModelKind::WoodEndGrain => (65, 20, 520),
        ProceduralModelKind::BrickTileLattice => (80, 40, (grid * 1000.0) as u16),
        ProceduralModelKind::Corrugation => (500, 70, (grid * 1000.0) as u16),
        ProceduralModelKind::BrushedMetal => (12, 4, (fiber * 1000.0) as u16),
        ProceduralModelKind::ConcreteAggregate => (300, 120, 580),
        ProceduralModelKind::PaintedMetalLayers => (900, 80, 140),
    };
    Ok(ProceduralLayerParameters { primary_width_milli: primary, secondary_width_milli: secondary,
        density_milli: density.min(1000), noise_milli: noise })
}

fn outputs_for(kind: ProceduralModelKind) -> BTreeSet<ProceduralContribution> {
    let mut set = BTreeSet::from([ProceduralContribution::BaseColor, ProceduralContribution::Height,
        ProceduralContribution::Normal, ProceduralContribution::Roughness, ProceduralContribution::LayerMask]);
    if matches!(kind, ProceduralModelKind::BrushedMetal | ProceduralModelKind::PaintedMetalLayers) {
        set.insert(ProceduralContribution::Metallic);
    }
    if matches!(kind, ProceduralModelKind::BrickTileLattice | ProceduralModelKind::ConcreteAggregate) {
        set.insert(ProceduralContribution::AmbientOcclusion);
    }
    set
}

fn generate_model(model: &FittedProceduralDomainModel, domain: ProceduralDomainSpec,
    cancellation: &RenderCancellationToken) -> Result<GeneratedProceduralDomain, ProceduralError>
{
    let pixels = u64::from(domain.width).checked_mul(u64::from(domain.height))
        .ok_or(ProceduralError::ResourceLimitExceeded)?;
    if domain.width == 0 || domain.height == 0 || domain.extent_x == 0 || domain.extent_y == 0
        || pixels > model.limits.max_output_pixels { return Err(ProceduralError::InvalidSettings); }
    let tile = 128.min(domain.width).min(domain.height).max(1);
    let tiles = u64::from(domain.width.div_ceil(tile))
        .checked_mul(u64::from(domain.height.div_ceil(tile))).ok_or(ProceduralError::ResourceLimitExceeded)?;
    let output_plane_count = 6_u64
        + model.channel_outputs.contains(&ProceduralContribution::Metallic) as u64
        + model.channel_outputs.contains(&ProceduralContribution::AmbientOcclusion) as u64;
    // Peak includes all row-major work arrays, normals, cloned tiled channel storage,
    // the mask channel, the separately owned layer mask, and conservative per-tile metadata.
    let bytes = pixels.checked_mul(112)
        .and_then(|value| value.checked_add(tiles.checked_mul(output_plane_count)?.checked_mul(64)?))
        .and_then(|value| value.checked_add(output_plane_count.checked_mul(32)?))
        .ok_or(ProceduralError::ResourceLimitExceeded)?;
    // Includes two five-point derivatives, boundary stencils, normalization, model sampling,
    // and channel-plane construction for every output pixel.
    let operations = pixels.checked_mul(320).ok_or(ProceduralError::ResourceLimitExceeded)?;
    if bytes > model.limits.max_working_bytes || operations > model.limits.max_operations {
        return Err(ProceduralError::ResourceLimitExceeded);
    }
    if cancellation.is_cancelled() { return Err(ProceduralError::Cancelled); }
    let count = usize::try_from(pixels).map_err(|_| ProceduralError::ResourceLimitExceeded)?;
    let mut colors = Vec::with_capacity(count); let mut heights = Vec::with_capacity(count);
    let mut roughness = Vec::with_capacity(count); let mut metallic = Vec::with_capacity(count);
    let mut ao = Vec::with_capacity(count); let mut masks = Vec::with_capacity(count);
    for y in 0..domain.height {
        if y % 32 == 0 && cancellation.is_cancelled() { return Err(ProceduralError::Cancelled); }
        for x in 0..domain.width {
            let coordinate = model_coordinate(domain, x, y, model.parameters.orientation_millidegrees);
            let sample = sample_model(model, coordinate);
            colors.push(LinearColor { rgb: sample.color, alpha: 1.0 });
            heights.push(LinearScalar(sample.height)); roughness.push(LinearScalar(sample.roughness));
            metallic.push(LinearScalar(sample.metallic)); ao.push(LinearScalar(sample.ao));
            masks.push(MaskValue(sample.mask));
        }
    }
    let normals = height_normals(&heights, domain, model.parameters.motif_scale, cancellation)?;
    let mut channels = vec![
        PreparedExemplarChannel::BaseColor { plane: plane(domain, tile, &colors)?, alpha_mode: ResolvedAlphaMode::Opaque },
        PreparedExemplarChannel::Scalar { role: MaterialChannelRole::Height, plane: plane(domain, tile, &heights)? },
        PreparedExemplarChannel::Normal { plane: plane(domain, tile, &normals)?,
            source_convention: hot_trimmer_domain::NormalConvention::OpenGl,
            canonical_convention: hot_trimmer_domain::NormalConvention::OpenGl,
            alpha_policy: NormalAlphaPolicy::Preserve },
        PreparedExemplarChannel::Scalar { role: MaterialChannelRole::Roughness, plane: plane(domain, tile, &roughness)? },
    ];
    if model.channel_outputs.contains(&ProceduralContribution::Metallic) {
        channels.push(PreparedExemplarChannel::Scalar { role: MaterialChannelRole::Metallic, plane: plane(domain, tile, &metallic)? });
    }
    if model.channel_outputs.contains(&ProceduralContribution::AmbientOcclusion) {
        channels.push(PreparedExemplarChannel::Scalar { role: MaterialChannelRole::AmbientOcclusion, plane: plane(domain, tile, &ao)? });
    }
    channels.push(PreparedExemplarChannel::Mask { role: MaterialChannelRole::EdgeMask, plane: plane(domain, tile, &masks)? });
    let digest = model_digest(&model.parameters, model.seed, &model.source_registration_digest);
    Ok(GeneratedProceduralDomain { width: domain.width, height: domain.height,
        units: model.parameters.motif_scale.units, topology: domain.topology, channels,
        layer_mask: plane(domain, tile, &masks)?, model_digest: digest,
        qa_views: model.qa_views.clone(), stage_result: model.stage_result.clone() })
}

#[derive(Clone, Copy)]
struct ModelSample { color: [f32; 3], height: f32, roughness: f32, metallic: f32, ao: f32, mask: f32 }

fn sample_model(model: &FittedProceduralDomainModel, p: (f64, f64)) -> ModelSample {
    let params = &model.parameters;
    let sx = params.motif_scale.x.max(1) as f64; let sy = params.motif_scale.y.max(1) as f64;
    let (u, v) = (p.0 / sx, p.1 / sy); let noise = f64::from(params.layers.noise_milli) / 1000.0;
    let base_rough = f32::from(params.roughness_milli) / 1000.0;
    let (tone, height, mask, metallic, ao, rough_delta) = match params.kind {
        ProceduralModelKind::WoodFaceGrain => {
            let warp = signed_hash(model.seed, (u * 7.0).floor() as i64, (v * 2.0).floor() as i64) * noise * 0.35;
            let rings = (std::f64::consts::TAU * (v + 0.18 * (u * 0.7).sin() + warp)).sin();
            let pore = signed_hash(model.seed ^ 0x51, (u * 31.0) as i64, (v * 127.0) as i64);
            let t = (0.5 + 0.38 * rings + 0.12 * pore).clamp(0.0, 1.0);
            (t, 0.5 + rings * 0.18, (rings.abs() > 0.78) as u8 as f64, 0.0, 1.0, pore * 0.08)
        }
        ProceduralModelKind::WoodEndGrain => {
            let radius = u.hypot(v); let angle = v.atan2(u);
            let irregular = (angle * 7.0 + radius * 0.35).sin() * 0.16 + signed_hash(model.seed, (angle * 50.0) as i64, radius as i64) * noise * 0.12;
            let ring = (std::f64::consts::TAU * (radius + irregular)).sin();
            let ray = (angle * 53.0 + radius).sin().abs();
            let t = (0.48 + ring * 0.34 - ray * 0.08).clamp(0.0, 1.0);
            (t, 0.5 + ring * 0.2, (ring > 0.72) as u8 as f64, 0.0, 1.0, ray * 0.06)
        }
        ProceduralModelKind::BrickTileLattice => {
            let row = v.floor() as i64; let bx = (u + if row & 1 == 0 { 0.0 } else { 0.5 }).rem_euclid(1.0);
            let by = v.rem_euclid(1.0); let mortar = bx.min(1.0 - bx) < 0.055 || by.min(1.0 - by) < 0.075;
            let variation = signed_hash(model.seed, (u + if row & 1 == 0 { 0.0 } else { 0.5 }).floor() as i64, row);
            if mortar { (0.08, 0.12, 1.0, 0.0, 0.62, 0.18) }
            else { ((0.56 + variation * 0.18).clamp(0.0, 1.0), 0.68 + variation * 0.08, 0.0, 0.0, 1.0, variation * 0.06) }
        }
        ProceduralModelKind::Corrugation => {
            let ridge = (std::f64::consts::TAU * u).cos(); let valley = (1.0 - ridge) * 0.5;
            (0.44 + ridge * 0.12, 0.5 + ridge * 0.45, (valley > 0.75) as u8 as f64, 0.35, 0.72 + ridge * 0.18, valley * 0.08)
        }
        ProceduralModelKind::BrushedMetal => {
            let streak = signed_hash(model.seed, (u * 9.0) as i64, (v * 310.0).floor() as i64);
            let long = (std::f64::consts::TAU * v * 37.0 + streak).sin();
            (0.5 + long * 0.08, 0.5 + long * 0.035, (long > 0.72) as u8 as f64, 1.0, 1.0, long * 0.14)
        }
        ProceduralModelKind::ConcreteAggregate => {
            let cell_x = u.floor() as i64; let cell_y = v.floor() as i64;
            let jitter_x = signed_hash(model.seed, cell_x, cell_y) * 0.3;
            let jitter_y = signed_hash(model.seed ^ 0xa7, cell_x, cell_y) * 0.3;
            let d = (u.rem_euclid(1.0) - 0.5 - jitter_x).hypot(v.rem_euclid(1.0) - 0.5 - jitter_y);
            let aggregate = d < 0.24 + noise * 0.08;
            let fine = signed_hash(model.seed ^ 0xc3, (u * 23.0) as i64, (v * 23.0) as i64);
            if aggregate { (0.68 + fine * 0.12, 0.67 + fine * 0.08, 1.0, 0.0, 0.78, 0.1) }
            else { (0.4 + fine * 0.1, 0.48 + fine * 0.05, 0.0, 0.0, 0.94, fine * 0.08) }
        }
        ProceduralModelKind::PaintedMetalLayers => {
            let coarse = signed_hash(model.seed, u.floor() as i64, v.floor() as i64);
            let fine = signed_hash(model.seed ^ 0x9d, (u * 11.0) as i64, (v * 11.0) as i64);
            let chip = coarse + fine * 0.45 > 1.05 - noise * 0.4;
            if chip { (0.12 + fine * 0.05, 0.35 + fine * 0.05, 1.0, 1.0, 0.72, -0.12) }
            else { (0.58 + fine * 0.05, 0.58 + fine * 0.025, 0.0, 0.0, 1.0, fine * 0.04) }
        }
    };
    ModelSample { color: palette_mix(params.palette, tone as f32), height: height.clamp(0.0, 1.0) as f32,
        roughness: (base_rough + rough_delta as f32).clamp(0.0, 1.0), metallic: metallic as f32,
        ao: ao.clamp(0.0, 1.0) as f32, mask: mask as f32 }
}

fn model_coordinate(domain: ProceduralDomainSpec, x: u32, y: u32, angle_millidegrees: u32) -> (f64, f64) {
    let px = if domain.width == 1 { domain.extent_x as f64 * 0.5 }
        else { f64::from(x) * domain.extent_x as f64 / f64::from(domain.width - 1) };
    let py = if domain.height == 1 { domain.extent_y as f64 * 0.5 }
        else { f64::from(y) * domain.extent_y as f64 / f64::from(domain.height - 1) };
    let (px, py) = if domain.topology == ProceduralCoordinateTopology::PolarCompatible {
        let cx = px - domain.extent_x as f64 * 0.5; let cy = py - domain.extent_y as f64 * 0.5;
        (cx.hypot(cy), cy.atan2(cx) * domain.extent_y as f64 / std::f64::consts::TAU)
    } else { (px, py) };
    let angle = f64::from(angle_millidegrees) * std::f64::consts::PI / 180_000.0;
    (px * angle.cos() + py * angle.sin(), -px * angle.sin() + py * angle.cos())
}

fn height_normals(
    height: &[LinearScalar],
    domain: ProceduralDomainSpec,
    motif: MotifScale,
    cancellation: &RenderCancellationToken,
) -> Result<Vec<TangentNormal>, ProceduralError> {
    let width = domain.width; let height_px = domain.height;
    let at = |x: u32, y: u32| height[(y * width + x) as usize].0;
    let step_x = domain.extent_x as f32 / (width.saturating_sub(1).max(1) as f32);
    let step_y = domain.extent_y as f32 / (height_px.saturating_sub(1).max(1) as f32);
    let mut out = Vec::with_capacity(height.len());
    for y in 0..height_px {
        if y % 32 == 0 && cancellation.is_cancelled() { return Err(ProceduralError::Cancelled); }
        for x in 0..width {
        let dx = derivative_1d(x, width, step_x, |sample_x| at(sample_x, y)) * motif.x as f32;
        let dy = derivative_1d(y, height_px, step_y, |sample_y| at(x, sample_y)) * motif.y as f32;
        let mut n = [-dx * 0.7, -dy * 0.7, 1.0];
        let length = (n[0]*n[0]+n[1]*n[1]+n[2]*n[2]).sqrt();
        for value in &mut n { *value /= length; }
        out.push(TangentNormal { xyz: n, alpha: 1.0 });
    }}
    Ok(out)
}

fn derivative_1d(sample: u32, count: u32, step: f32, at: impl Fn(u32) -> f32) -> f32 {
    if count <= 1 { return 0.0; }
    if count == 2 { return (at(1) - at(0)) / step; }
    if count < 5 {
        return if sample == 0 {
            (-3.0 * at(0) + 4.0 * at(1) - at(2)) / (2.0 * step)
        } else if sample == count - 1 {
            (3.0 * at(count - 1) - 4.0 * at(count - 2) + at(count - 3)) / (2.0 * step)
        } else {
            (at(sample + 1) - at(sample - 1)) / (2.0 * step)
        };
    }
    let denominator = 12.0 * step;
    match sample {
        0 => (-25.0 * at(0) + 48.0 * at(1) - 36.0 * at(2) + 16.0 * at(3) - 3.0 * at(4)) / denominator,
        1 => (-3.0 * at(0) - 10.0 * at(1) + 18.0 * at(2) - 6.0 * at(3) + at(4)) / denominator,
        value if value == count - 2 =>
            (3.0 * at(count - 1) + 10.0 * at(count - 2) - 18.0 * at(count - 3)
                + 6.0 * at(count - 4) - at(count - 5)) / denominator,
        value if value == count - 1 =>
            (25.0 * at(count - 1) - 48.0 * at(count - 2) + 36.0 * at(count - 3)
                - 16.0 * at(count - 4) + 3.0 * at(count - 5)) / denominator,
        _ => (at(sample - 2) - 8.0 * at(sample - 1) + 8.0 * at(sample + 1) - at(sample + 2)) / denominator,
    }
}

fn plane<T: Clone>(domain: ProceduralDomainSpec, tile: u32, values: &[T]) -> Result<ImagePlane<T>, ProceduralError> {
    ImagePlane::from_row_major(domain.width, domain.height, tile, values).map_err(|_| ProceduralError::PlaneConstruction)
}

fn palette_mix(palette: ProceduralPalette, t: f32) -> [f32; 3] {
    let (a, b, local) = if t < 0.5 { (palette.dark, palette.mid, t * 2.0) }
        else { (palette.mid, palette.light, (t - 0.5) * 2.0) };
    [a[0] + (b[0] - a[0]) * local, a[1] + (b[1] - a[1]) * local, a[2] + (b[2] - a[2]) * local]
}

fn signed_hash(seed: u64, x: i64, y: i64) -> f64 {
    let mut z = seed ^ (x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ (y as u64).rotate_left(29);
    z ^= z >> 30; z = z.wrapping_mul(0xbf58_476d_1ce4_e5b9); z ^= z >> 27;
    z = z.wrapping_mul(0x94d0_49bb_1331_11eb); z ^= z >> 31;
    (z as f64 / u64::MAX as f64) * 2.0 - 1.0
}

fn luminance(rgb: [f32; 3]) -> f32 { rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722 }
fn mean_scalar(plane: &ImagePlane<LinearScalar>, cancellation: &RenderCancellationToken)
    -> Result<f32, ProceduralError>
{
    let (mut sum, mut count) = (0.0_f64, 0_u64);
    for (index, tile) in plane.tiles().iter().enumerate() {
        if index % 16 == 0 { check_fit_cancel(cancellation)?; }
        for value in &tile.pixels { sum += f64::from(value.0); count += 1; }
    }
    Ok((sum / count.max(1) as f64) as f32)
}
fn model_digest(parameters: &FittedProceduralParameters, seed: u64, registration: &ContentDigest) -> ContentDigest {
    ContentDigest::sha256(format!("{parameters:?}|{seed}|{}|{STAGE_08E_PROCEDURAL_ALGORITHM_VERSION}", registration.0).as_bytes())
}

trait PlaneDimensions { fn dimensions(&self) -> (u32, u32); }
impl<T> PlaneDimensions for ImagePlane<T> { fn dimensions(&self) -> (u32, u32) { (self.width(), self.height()) } }

#[cfg(test)]
mod tests {
    use super::*;
    use hot_trimmer_domain::{
        DelightingPassThroughReason, MaterialCalibrationIntent,
    };
    use hot_trimmer_material_analysis::{
        analyze_source, calibrate_scale_orientation, extract_feature_fields, AnalysisSettings,
        FeatureFieldSettings, ReflectanceProvenance, RouteExecution, ScaleOrientationSettings,
    };

    fn stage_result() -> StageResult { StageResult::PassThrough { reason: "procedural fixture".into() } }

    fn source() -> Arc<DelitPreparedExemplar> {
        let (width, height) = (64, 64);
        let mut colors = Vec::with_capacity((width * height) as usize);
        let mut roughness = Vec::with_capacity(colors.capacity());
        for y in 0..height { for x in 0..width {
            let lattice = if x % 8 == 0 || y % 12 == 0 { 0.12 } else { 0.68 };
            let grain = ((x * 5 + y * 3) % 17) as f32 / 85.0;
            let value = (lattice + grain).min(1.0);
            colors.push(LinearColor { rgb: [value, value * 0.72, value * 0.48], alpha: 1.0 });
            roughness.push(LinearScalar(0.32 + value * 0.36));
        }}
        let base = ImagePlane::from_row_major(width, height, 16, &colors).unwrap();
        Arc::new(DelitPreparedExemplar {
            exemplar_id: "stage-08e-measured-fixture".into(),
            prepared_source_digest: ContentDigest::sha256(b"stage-08e-prepared-source"),
            perspective_confidence_milli: 1000,
            original_prepared_base_color: base.clone(),
            channels: vec![
                PreparedExemplarChannel::BaseColor { plane: base, alpha_mode: ResolvedAlphaMode::Opaque },
                PreparedExemplarChannel::Scalar { role: MaterialChannelRole::Roughness,
                    plane: ImagePlane::from_row_major(width, height, 16, &roughness).unwrap() },
            ],
            coverage: Some(ImagePlane::from_row_major(width, height, 16,
                &vec![MaskValue(1.0); (width * height) as usize]).unwrap()),
            masks: None, reflectance_provenance: ReflectanceProvenance::ImportedPrepared,
            route_execution: RouteExecution::PassThrough(DelightingPassThroughReason::AuthoredTextureOrPbrSet),
            upstream_stage_result: stage_result(), stage_result: stage_result(),
        })
    }

    fn behavior_for(kind: ProceduralModelKind) -> MaterialBehaviorClass {
        match kind {
            ProceduralModelKind::WoodFaceGrain => MaterialBehaviorClass::OrganicDirectional,
            ProceduralModelKind::WoodEndGrain => MaterialBehaviorClass::RadialDetail,
            ProceduralModelKind::BrickTileLattice => MaterialBehaviorClass::PeriodicLatticeStructured,
            ProceduralModelKind::Corrugation => MaterialBehaviorClass::ManufacturedPattern,
            ProceduralModelKind::BrushedMetal => MaterialBehaviorClass::StochasticDirectional,
            ProceduralModelKind::ConcreteAggregate => MaterialBehaviorClass::StochasticIsotropic,
            ProceduralModelKind::PaintedMetalLayers => MaterialBehaviorClass::LayeredBanded,
        }
    }

    fn fit_request(kind: ProceduralModelKind) -> ProceduralFitRequest {
        let token = RenderCancellationToken::new();
        let source = source();
        let mut five = analyze_source(&source, &AnalysisSettings::default(), None, &token).unwrap();
        five.classification.analyzed_class = behavior_for(kind);
        five.classification.confidence_milli = 900;
        let mut six = calibrate_scale_orientation(&source, &five, &MaterialCalibrationIntent::default(),
            &ScaleOrientationSettings::default(), &token).unwrap();
        // Keep a stable measured axis in the typed Stage 6 fixture.
        six.global_orientation.axis_millidegrees = Some(27_000);
        six.global_orientation.measured_axis_millidegrees = Some(27_000);
        six.global_orientation.confidence_milli = 900;
        let seven = extract_feature_fields(&source, &six, &FeatureFieldSettings::default(), &token).unwrap();
        ProceduralFitRequest {
            source, stage_five: Arc::new(five), stage_six: Arc::new(six), stage_seven: Arc::new(seven),
            user_override: Some(ProceduralUserOverride { model_kind: Some(kind),
                orientation_millidegrees: None,
                motif_scale: Some(MotifScale { x: 125_000, y: 125_000,
                    units: ProceduralUnits::RelativeMillionths }),
                palette: None, roughness_milli: None, noise_milli: None }),
            content_intent: ProceduralContentIntent::MaterialOnly, seed: 44,
            settings: ProceduralFitSettings { minimum_confidence_milli: 260,
                limits: ProceduralLimits { max_output_pixels: 65_536, max_working_bytes: 16_000_000,
                    max_operations: 100_000_000, max_fit_samples: 4096 } },
        }
    }

    fn fitted(request: &ProceduralFitRequest, cancellation: &RenderCancellationToken) -> FittedProceduralDomainModel {
        let ProceduralFitOutcome::Fitted(model) = fit_procedural_domain_model(request, cancellation).unwrap()
            else { panic!("expected fitted model") };
        model
    }

    fn scalar_channel(domain: &GeneratedProceduralDomain, role: MaterialChannelRole) -> &ImagePlane<LinearScalar> {
        domain.channels.iter().find_map(|channel| match channel {
            PreparedExemplarChannel::Scalar { role: found, plane } if *found == role => Some(plane), _ => None,
        }).expect("registered scalar channel")
    }

    fn normal_channel(domain: &GeneratedProceduralDomain) -> &ImagePlane<TangentNormal> {
        domain.channels.iter().find_map(|channel| match channel {
            PreparedExemplarChannel::Normal { plane, .. } => Some(plane), _ => None,
        }).expect("registered normal channel")
    }

    fn corresponding_normal_axes(
        low: &ImagePlane<TangentNormal>,
        high: &ImagePlane<TangentNormal>,
    ) -> (f64, f64) {
        assert_eq!(high.width(), low.width() * 2 - 1);
        assert_eq!(high.height(), low.height() * 2 - 1);
        let (mut low_xx, mut low_yy, mut low_xy) = (0.0_f64, 0.0_f64, 0.0_f64);
        let (mut high_xx, mut high_yy, mut high_xy) = (0.0_f64, 0.0_f64, 0.0_f64);
        for y in 2..low.height() - 2 { for x in 2..low.width() - 2 {
            let a = low.pixel(x, y).xyz;
            let b = high.pixel(x * 2, y * 2).xyz;
            low_xx += f64::from(a[0] * a[0]);
            low_yy += f64::from(a[1] * a[1]);
            low_xy += f64::from(a[0] * a[1]);
            high_xx += f64::from(b[0] * b[0]);
            high_yy += f64::from(b[1] * b[1]);
            high_xy += f64::from(b[0] * b[1]);
        }}
        let axis = |xx: f64, yy: f64, xy: f64| {
            (0.5 * (2.0 * xy).atan2(xx - yy).to_degrees()).rem_euclid(180.0)
        };
        (axis(low_xx, low_yy, low_xy), axis(high_xx, high_yy, high_xy))
    }

    fn axis_difference(a: f64, b: f64) -> f64 {
        let difference = (a - b).abs().rem_euclid(180.0); difference.min(180.0 - difference)
    }

    #[test]
    fn algorithm_stage_08e_procedural() {
        let cancellation = RenderCancellationToken::new();
        let kinds = [ProceduralModelKind::WoodFaceGrain, ProceduralModelKind::WoodEndGrain,
            ProceduralModelKind::BrickTileLattice, ProceduralModelKind::Corrugation,
            ProceduralModelKind::BrushedMetal, ProceduralModelKind::ConcreteAggregate,
            ProceduralModelKind::PaintedMetalLayers];
        for kind in kinds {
            let request = fit_request(kind);
            let model = fitted(&request, &cancellation);
            assert_eq!(model, fitted(&request, &cancellation), "fitting must replay deterministically");
            for (width, height, topology) in [(96, 32, ProceduralCoordinateTopology::Cartesian),
                (32, 96, ProceduralCoordinateTopology::Cartesian), (64, 64, ProceduralCoordinateTopology::Cartesian),
                (64, 64, ProceduralCoordinateTopology::PolarCompatible)] {
                let spec = ProceduralDomainSpec { width, height, extent_x: 1_000_000, extent_y: 1_000_000, topology };
                let output = model.generate(spec, &cancellation).unwrap();
                let replay = model.generate(spec, &cancellation).unwrap();
                assert_eq!(output, replay); assert!(output.channels.len() >= 5);
                assert!(output.channels.iter().all(|channel| channel.dimensions() == (width, height)));
            }
        }

        // Exercise measured Stage 5 routing without a model-family override.
        let mut routed = fit_request(ProceduralModelKind::WoodFaceGrain);
        routed.user_override.as_mut().unwrap().model_kind = None;
        let routed_model = fitted(&routed, &cancellation);
        assert_eq!(routed_model.kind(), ProceduralModelKind::WoodFaceGrain);

        // Measured values survive corrections and effective authority remains inspectable.
        let mut corrected = fit_request(ProceduralModelKind::ConcreteAggregate);
        let corrected_palette = ProceduralPalette { dark: [0.01; 3], mid: [0.2; 3], light: [0.9; 3] };
        let correction = corrected.user_override.as_mut().unwrap();
        correction.palette = Some(corrected_palette); correction.roughness_milli = Some(123);
        let corrected_model = fitted(&corrected, &cancellation);
        assert_eq!(corrected_model.parameters.palette, corrected_palette);
        assert_eq!(corrected_model.parameters.roughness_milli, 123);
        assert_ne!(corrected_model.evidence.measured_parameters.palette, corrected_palette);
        assert_ne!(corrected_model.evidence.measured_parameters.roughness_milli, 123);
        assert_eq!(corrected_model.evidence.effective_authorities[&ProceduralParameterName::Palette], ParameterAuthority::UserOverride);

        // A non-family correction cannot authorize unrelated low-confidence evidence.
        let mut partial = fit_request(ProceduralModelKind::WoodFaceGrain);
        Arc::make_mut(&mut partial.stage_five).classification.confidence_milli = 1;
        partial.user_override = Some(ProceduralUserOverride { roughness_milli: Some(600), ..ProceduralUserOverride::default() });
        assert!(matches!(fit_procedural_domain_model(&partial, &cancellation).unwrap(),
            ProceduralFitOutcome::Unsupported(UnsupportedProceduralModel {
                state: ProceduralUnsupportedState::ConfidenceInsufficient, .. })));

        // Foreign Stage 5 or Stage 6 evidence cannot enter a registered fit.
        let mut foreign_five = fit_request(ProceduralModelKind::ConcreteAggregate);
        Arc::make_mut(&mut foreign_five.stage_five).prepared_source_digest = ContentDigest::sha256(b"foreign");
        assert_eq!(fit_procedural_domain_model(&foreign_five, &cancellation), Err(ProceduralError::RegistrationDrift));
        let mut foreign_six = fit_request(ProceduralModelKind::ConcreteAggregate);
        Arc::make_mut(&mut foreign_six.stage_six).stage_five_cache_key =
            hot_trimmer_material_analysis::SourceAnalysisCacheKey(ContentDigest::sha256(b"foreign-five"));
        assert_eq!(fit_procedural_domain_model(&foreign_six, &cancellation), Err(ProceduralError::RegistrationDrift));

        // End grain and semantic detail retain explicit unsupported contracts.
        let mut end_grain = fit_request(ProceduralModelKind::WoodEndGrain);
        end_grain.user_override = None;
        assert!(matches!(fit_procedural_domain_model(&end_grain, &cancellation).unwrap(),
            ProceduralFitOutcome::Unsupported(UnsupportedProceduralModel {
                state: ProceduralUnsupportedState::EndGrainRequiresExplicitModelOrSource, .. })));
        let mut semantic = fit_request(ProceduralModelKind::ConcreteAggregate);
        semantic.content_intent = ProceduralContentIntent::ContainsSemanticDetail;
        assert!(matches!(fit_procedural_domain_model(&semantic, &cancellation).unwrap(),
            ProceduralFitOutcome::Unsupported(UnsupportedProceduralModel {
                state: ProceduralUnsupportedState::SemanticDetailRequiresDetailContract, .. })));

        // Fitting observes cancellation and both declared budgets.
        let cancelled = RenderCancellationToken::new(); cancelled.cancel();
        assert_eq!(fit_procedural_domain_model(&fit_request(ProceduralModelKind::BrushedMetal), &cancelled),
            Err(ProceduralError::Cancelled));
        let mut bounded = fit_request(ProceduralModelKind::BrushedMetal);
        bounded.settings.limits.max_operations = 1;
        assert_eq!(fit_procedural_domain_model(&bounded, &cancellation), Err(ProceduralError::ResourceLimitExceeded));
        bounded.settings.limits.max_operations = 100_000_000; bounded.settings.limits.max_working_bytes = 1;
        assert_eq!(fit_procedural_domain_model(&bounded, &cancellation), Err(ProceduralError::ResourceLimitExceeded));

        // Corresponding world/model coordinates preserve motifs, orientation, and normal strength.
        let model = fitted(&fit_request(ProceduralModelKind::Corrugation), &cancellation);
        let low = model.generate(ProceduralDomainSpec { width: 65, height: 65, extent_x: 1_000_000,
            extent_y: 1_000_000, topology: ProceduralCoordinateTopology::Cartesian }, &cancellation).unwrap();
        let high = model.generate(ProceduralDomainSpec { width: 129, height: 129, extent_x: 1_000_000,
            extent_y: 1_000_000, topology: ProceduralCoordinateTopology::Cartesian }, &cancellation).unwrap();
        let low_height = scalar_channel(&low, MaterialChannelRole::Height);
        let high_height = scalar_channel(&high, MaterialChannelRole::Height);
        for y in 0..65 { for x in 0..65 {
            assert!((low_height.pixel(x, y).0 - high_height.pixel(x * 2, y * 2).0).abs() < 1.0e-6);
        }}
        let low_normal = normal_channel(&low); let high_normal = normal_channel(&high);
        let mut dot_sum = 0.0_f64; let mut count = 0_u64;
        for y in 2..63 { for x in 2..63 {
            let a = low_normal.pixel(x, y).xyz; let b = high_normal.pixel(x * 2, y * 2).xyz;
            dot_sum += f64::from(a[0]*b[0] + a[1]*b[1] + a[2]*b[2]); count += 1;
        }}
        assert!(dot_sum / count as f64 > 0.98, "coordinate-scaled normals must remain stable across resolution");
        let (low_axis, high_axis) = corresponding_normal_axes(low_normal, high_normal);
        assert!(axis_difference(low_axis, high_axis) < 1.0,
            "orientation must remain stable across resolution");
        let crossings = |plane: &ImagePlane<LinearScalar>, y: u32| (1..plane.width())
            .filter(|x| (plane.pixel(*x - 1, y).0 >= 0.5) != (plane.pixel(*x, y).0 >= 0.5)).count();
        assert_eq!(crossings(low_height, 32), crossings(high_height, 64), "motif count must not scale with pixels");

        // The revised peak estimate rejects a budget that covered the old 64-byte charge.
        let mut memory_model = model.clone(); memory_model.limits.max_working_bytes = 400_000;
        assert_eq!(memory_model.generate(ProceduralDomainSpec { width: 64, height: 64,
            extent_x: 1_000_000, extent_y: 1_000_000, topology: ProceduralCoordinateTopology::Cartesian }, &cancellation),
            Err(ProceduralError::ResourceLimitExceeded));
    }
}
