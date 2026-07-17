//! Stage 8F authoritative material-domain router and honest local learned-provider boundary.

use std::{collections::{BTreeMap, BTreeSet}, sync::Arc};

use hot_trimmer_domain::{
    AlgorithmProvenance, CompilationDiagnostic, ContentDigest, DiagnosticCode,
    MaterialBehaviorClass, StageResult,
};
use hot_trimmer_image_io::MaskValue;
use hot_trimmer_material_analysis::SourceAnalysisReport;
use hot_trimmer_render_core::{PreparedExemplarChannel, RenderCancellationToken};
use sha2::{Digest, Sha256};
use thiserror::Error;

use super::*;

pub const STAGE_08_ROUTER_ALGORITHM_ID: &str = "hot_trimmer.material_domain_router";
pub const STAGE_08_ROUTER_ALGORITHM_VERSION: &str = "8.7.0";
pub const STAGE_08D_STATISTICAL_ALGORITHM_ID: &str = "hot_trimmer.classical_statistical_periodic_synthesis";
pub const STAGE_08D_STATISTICAL_ALGORITHM_VERSION: &str = "8.5.0";
pub const LEARNED_PROVIDER_INTERFACE_VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum MaterialSourceClass {
    ExistingTileablePbr,
    FineConcretePlaster,
    RustGrunge,
    BrushedMetal,
    BrickTile,
    WoodFaceGrain,
    WoodEndGrain,
    ManufacturedBorder,
    UniqueVentPanel,
    RadialDrainWasher,
    MixedUnknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceClassAuthority { MeasuredBehavior, UserPolicy }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LearnedPolicy { Disabled, AllowFallback, PreferWhenApplicable }

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterialDomainRoutePolicy {
    pub override_route: Option<DomainRoute>,
    pub pinned_route: Option<DomainRoute>,
    pub source_class_override: Option<MaterialSourceClass>,
    pub learned: LearnedPolicy,
    pub maximum_preview_edge: u16,
}

impl Default for MaterialDomainRoutePolicy {
    fn default() -> Self {
        Self { override_route: None, pinned_route: None, source_class_override: None,
            learned: LearnedPolicy::AllowFallback, maximum_preview_edge: 128 }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutePolicyCommand {
    Override(DomainRoute),
    Pin(DomainRoute),
    ResetRoute,
    ResetAll,
}

impl MaterialDomainRoutePolicy {
    pub fn apply(&mut self, command: RoutePolicyCommand) {
        match command {
            RoutePolicyCommand::Override(route) => self.override_route = Some(route),
            RoutePolicyCommand::Pin(route) => { self.pinned_route = Some(route); self.override_route = None; }
            RoutePolicyCommand::ResetRoute => { self.override_route = None; self.pinned_route = None; }
            RoutePolicyCommand::ResetAll => *self = Self::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum LearnedCapability { DelitMaterialField, SeamlessExpansion, SuperResolution, EstimatedHeight, EstimatedNormal }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LearnedDevicePolicy { CpuOnly, GpuAllowed, GpuRequired }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LearnedExecutionDevice { Cpu, Gpu }

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LearnedProviderDescriptor {
    pub provider_id: String,
    pub provider_version: String,
    pub interface_version: u16,
    pub model_digest: ContentDigest,
    pub capabilities: BTreeSet<LearnedCapability>,
    pub device_policy: LearnedDevicePolicy,
    pub deterministic: bool,
    pub maximum_input_pixels: u64,
    pub maximum_output_pixels: u64,
    pub maximum_working_bytes: u64,
}

pub struct LearnedMaterialRequest<'a> {
    pub source: &'a hot_trimmer_material_analysis::DelitPreparedExemplar,
    pub analysis: &'a FeatureFieldReport,
    pub requested: BTreeSet<LearnedCapability>,
    pub output_width: u32,
    pub output_height: u32,
    pub seed: u64,
    pub deterministic: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LearnedMaterialOutput {
    pub channels: Vec<PreparedExemplarChannel>,
    pub confidence_milli: u16,
    pub model_digest: ContentDigest,
    pub output_digest: ContentDigest,
    pub deterministic: bool,
    pub device: LearnedExecutionDevice,
    pub diagnostics: Vec<String>,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum LearnedProviderError {
    #[error("learned provider does not support the requested capability")]
    UnsupportedCapability,
    #[error("learned provider rejected the declared bounds")]
    BoundsExceeded,
    #[error("learned provider was cancelled")]
    Cancelled,
    #[error("learned provider inference failed: {0}")]
    Inference(String),
}

pub trait LocalLearnedMaterialProvider {
    fn descriptor(&self) -> LearnedProviderDescriptor;
    fn infer(&self, request: &LearnedMaterialRequest<'_>, cancellation: &RenderCancellationToken)
        -> Result<LearnedMaterialOutput, LearnedProviderError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LearnedProviderState {
    Disabled,
    Absent,
    Available { provider_id: String, provider_version: String, model_digest: ContentDigest },
    Rejected { reason: String },
    Used { provider_id: String, provider_version: String, model_digest: ContentDigest,
        output_digest: ContentDigest, confidence_milli: u16 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RouteCost {
    pub fidelity_milli: u16,
    pub seam_milli: u16,
    pub semantic_risk_milli: u16,
    pub compute_milli: u16,
    pub uncertainty_milli: u16,
    pub total_milli: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouteComparison {
    pub route: DomainRoute,
    pub applicable: bool,
    pub preview_dimensions: (u16, u16),
    pub cost: Option<RouteCost>,
    pub rejection: Option<String>,
    pub attempted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Stage8DomainDiagnostics {
    pub source_class: MaterialSourceClass,
    pub source_class_authority: SourceClassAuthority,
    pub selected_route: Option<DomainRoute>,
    pub compared_routes: Vec<RouteComparison>,
    pub learned_provider: LearnedProviderState,
    pub registration_valid: bool,
    pub seam_or_period_valid: bool,
    pub scale_valid: bool,
    pub correspondence_valid: bool,
    pub deterministic: bool,
    pub cache_provenance_valid: bool,
    pub messages: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stage8MaterialDomainResult {
    pub domain: PreparedMaterialDomain,
    pub diagnostics: Stage8DomainDiagnostics,
    pub stage_result: StageResult,
}

#[derive(Clone, Debug)]
pub struct Stage8RouterRequest {
    pub domain: DomainRequest,
    pub stage_five: Arc<SourceAnalysisReport>,
    pub policy: MaterialDomainRoutePolicy,
    pub procedural_override: Option<ProceduralUserOverride>,
    pub procedural_settings: ProceduralFitSettings,
    pub output_width: u32,
    pub output_height: u32,
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum Stage8RouterError {
    #[error("Stage 8 evidence is not registered across Stages 4-7")]
    RegistrationDrift,
    #[error("Stage 8 router settings are outside bounded ranges")]
    InvalidSettings,
    #[error("Stage 8 routing was cancelled")]
    Cancelled,
    #[error("no semantically applicable Stage 8 route succeeded")]
    ActionableInsufficiency(Stage8DomainDiagnostics),
}

/// The only authoritative entry point for the complete Stage 8 engine set.
pub fn prepare_stage_08_material_domain(
    request: &Stage8RouterRequest,
    provider: Option<&dyn LocalLearnedMaterialProvider>,
    cache: &mut MaterialDomainCache,
    cancellation: &RenderCancellationToken,
) -> Result<Stage8MaterialDomainResult, Stage8RouterError> {
    validate_router_request(request)?;
    if cancellation.is_cancelled() { return Err(Stage8RouterError::Cancelled); }
    let (source_class, authority) = classify_source(request);
    let preview = preview_dimensions(request);
    let provider_descriptor = provider.map(|value| value.descriptor());
    let mut learned_state = match request.policy.learned {
        LearnedPolicy::Disabled => LearnedProviderState::Disabled,
        _ if provider.is_none() => LearnedProviderState::Absent,
        _ => descriptor_state(provider_descriptor.as_ref().expect("provider descriptor")),
    };
    let mut compared = comparisons(request, source_class, preview, provider_descriptor.as_ref());
    apply_user_route_policy(request, source_class, preview, &mut compared);
    compared.sort_by_key(|item| (item.cost.map_or(u16::MAX, |cost| cost.total_milli), route_rank(item.route)));
    let forced_route = request.policy.pinned_route.or(request.policy.override_route);
    let attempt_order: Vec<_> = if let Some(forced) = forced_route {
        compared.iter().position(|item| item.route == forced && item.applicable).into_iter().collect()
    } else { (0..compared.len()).filter(|index| compared[*index].applicable).collect() };

    for index in attempt_order {
        if !compared[index].applicable { continue; }
        compared[index].attempted = true;
        let route = compared[index].route;
        match execute_route(request, route, provider, provider_descriptor.as_ref(), cache, cancellation) {
            Ok((domain, used)) => {
                if let Some(state) = used { learned_state = state; }
                let checks = match validate_chosen_domain(request, &domain, source_class) {
                    Ok(checks) => checks,
                    Err(reason) => { compared[index].applicable = false; compared[index].rejection = Some(reason); continue; }
                };
                let diagnostics = Stage8DomainDiagnostics {
                    source_class, source_class_authority: authority, selected_route: Some(route),
                    compared_routes: compared, learned_provider: learned_state,
                    registration_valid: checks.0, seam_or_period_valid: checks.1, scale_valid: checks.2,
                    correspondence_valid: checks.3, deterministic: checks.4,
                    cache_provenance_valid: checks.5,
                    messages: vec![format!("selected {route:?} from measured applicability and normalized bounded-preview cost")],
                };
                let settings_hash = router_settings_hash(request, &diagnostics);
                return Ok(Stage8MaterialDomainResult { domain, diagnostics,
                    stage_result: StageResult::Executed { algorithm: AlgorithmProvenance {
                        algorithm_id: STAGE_08_ROUTER_ALGORITHM_ID.into(), version: STAGE_08_ROUTER_ALGORITHM_VERSION.into(),
                    }, settings_hash, diagnostics: Vec::new() } });
            }
            Err(reason) => {
                compared[index].applicable = false;
                compared[index].rejection = Some(reason);
            }
        }
    }
    let forced_message = forced_route.map(|route| format!(
        "forced route {route:?} was inapplicable or failed; pin/override is fail-closed and no alternate route was selected"));
    let diagnostics = Stage8DomainDiagnostics {
        source_class, source_class_authority: authority, selected_route: None,
        compared_routes: compared, learned_provider: learned_state,
        registration_valid: true, seam_or_period_valid: false, scale_valid: true,
        correspondence_valid: false, deterministic: true, cache_provenance_valid: false,
        messages: vec![forced_message.unwrap_or_else(||
            "provide a better registered source, measure/confirm a period, or choose an applicable typed route".into())],
    };
    Err(Stage8RouterError::ActionableInsufficiency(diagnostics))
}

fn validate_router_request(request: &Stage8RouterRequest) -> Result<(), Stage8RouterError> {
    if request.policy.maximum_preview_edge < 16 || request.policy.maximum_preview_edge > 512
        || request.output_width == 0 || request.output_height == 0
        || u64::from(request.output_width) * u64::from(request.output_height) > request.procedural_settings.limits.max_output_pixels
    { return Err(Stage8RouterError::InvalidSettings); }
    if request.stage_five.prepared_source_digest != request.domain.prepared_source_digest
        || request.domain.scale_orientation.stage_five_cache_key != request.stage_five.cache_key
        || request.domain.analysis.stage_six_cache_key != request.domain.scale_orientation.cache_key
    { return Err(Stage8RouterError::RegistrationDrift); }
    super::validate_request(&request.domain).map_err(|_| Stage8RouterError::RegistrationDrift)
}

fn classify_source(request: &Stage8RouterRequest) -> (MaterialSourceClass, SourceClassAuthority) {
    if let Some(class) = request.policy.source_class_override { return (class, SourceClassAuthority::UserPolicy); }
    let m = request.stage_five.classification.measurements;
    let class = match request.stage_five.classification.routed_class() {
        MaterialBehaviorClass::AlreadyTileable => MaterialSourceClass::ExistingTileablePbr,
        MaterialBehaviorClass::StochasticIsotropic if m.stationarity_milli >= 600 => MaterialSourceClass::FineConcretePlaster,
        MaterialBehaviorClass::StochasticIsotropic => MaterialSourceClass::RustGrunge,
        MaterialBehaviorClass::StochasticDirectional => MaterialSourceClass::BrushedMetal,
        MaterialBehaviorClass::PeriodicLatticeStructured => MaterialSourceClass::BrickTile,
        MaterialBehaviorClass::LayeredBanded => MaterialSourceClass::BrushedMetal,
        MaterialBehaviorClass::OrganicDirectional => MaterialSourceClass::WoodFaceGrain,
        MaterialBehaviorClass::ManufacturedPattern => MaterialSourceClass::ManufacturedBorder,
        MaterialBehaviorClass::UniqueDetail => MaterialSourceClass::UniqueVentPanel,
        MaterialBehaviorClass::RadialDetail => MaterialSourceClass::RadialDrainWasher,
        MaterialBehaviorClass::MixedUnknown => MaterialSourceClass::MixedUnknown,
    };
    (class, SourceClassAuthority::MeasuredBehavior)
}

fn descriptor_state(descriptor: &LearnedProviderDescriptor) -> LearnedProviderState {
    let invalid = descriptor.interface_version != LEARNED_PROVIDER_INTERFACE_VERSION
        || !descriptor.deterministic || descriptor.maximum_input_pixels == 0
        || descriptor.maximum_output_pixels == 0 || descriptor.maximum_working_bytes == 0
        || descriptor.model_digest.0.is_empty();
    if invalid { LearnedProviderState::Rejected { reason: "provider descriptor violates the deterministic versioned boundary".into() } }
    else { LearnedProviderState::Available { provider_id: descriptor.provider_id.clone(),
        provider_version: descriptor.provider_version.clone(), model_digest: descriptor.model_digest.clone() } }
}

fn preview_dimensions(request: &Stage8RouterRequest) -> (u16, u16) {
    let limit = u32::from(request.policy.maximum_preview_edge);
    let width = request.output_width.min(limit).max(1);
    let height = ((u64::from(request.output_height) * u64::from(width) / u64::from(request.output_width)).max(1) as u32).min(limit);
    (width as u16, height as u16)
}

fn comparisons(request: &Stage8RouterRequest, class: MaterialSourceClass, preview: (u16, u16),
    provider: Option<&LearnedProviderDescriptor>) -> Vec<RouteComparison>
{
    let routes = [DomainRoute::DirectSource, DomainRoute::GraphCutPeriodicClosure,
        DomainRoute::TextureQuilting, DomainRoute::PatchMatch, DomainRoute::StatisticalSynthesis,
        DomainRoute::ProceduralReconstruction, DomainRoute::LearnedProvider];
    routes.into_iter().map(|route| {
        let rejection = applicability_rejection(request, class, route, provider);
        let applicable = rejection.is_none();
        RouteComparison { route, applicable, preview_dimensions: preview,
            cost: applicable.then(|| normalized_cost(request, class, route)), rejection, attempted: false }
    }).collect()
}

fn applicability_rejection(request: &Stage8RouterRequest, class: MaterialSourceClass, route: DomainRoute,
    provider: Option<&LearnedProviderDescriptor>) -> Option<String>
{
    let behavior = request.stage_five.classification.routed_class();
    let structured = matches!(class, MaterialSourceClass::BrickTile | MaterialSourceClass::ManufacturedBorder
        | MaterialSourceClass::UniqueVentPanel | MaterialSourceClass::RadialDrainWasher | MaterialSourceClass::WoodEndGrain);
    match route {
        DomainRoute::Auto => Some("Auto is a policy request, not an executable route".into()),
        DomainRoute::DirectSource if class == MaterialSourceClass::ExistingTileablePbr
            && (request.domain.analysis.seamability.horizontal_cost_milli > request.domain.direct_boundary_threshold_milli
                || request.domain.analysis.seamability.vertical_cost_milli > request.domain.direct_boundary_threshold_milli) =>
            Some("measured boundary evidence does not support periodic direct sampling".into()),
        DomainRoute::DirectSource if matches!(class, MaterialSourceClass::BrickTile | MaterialSourceClass::ManufacturedBorder)
            && !measured_period_alignment(request, class) =>
            Some("structured direct sampling requires measured axis-aligned period evidence that exactly divides the registered domain".into()),
        DomainRoute::DirectSource if matches!(class, MaterialSourceClass::FineConcretePlaster | MaterialSourceClass::RustGrunge
            | MaterialSourceClass::BrushedMetal | MaterialSourceClass::WoodFaceGrain | MaterialSourceClass::WoodEndGrain
            | MaterialSourceClass::MixedUnknown) => Some("source class requires closure, expansion, or explicit reconstruction".into()),
        DomainRoute::GraphCutPeriodicClosure if matches!(class, MaterialSourceClass::BrushedMetal
            | MaterialSourceClass::BrickTile | MaterialSourceClass::WoodFaceGrain
            | MaterialSourceClass::ManufacturedBorder | MaterialSourceClass::UniqueVentPanel
            | MaterialSourceClass::RadialDrainWasher | MaterialSourceClass::WoodEndGrain) =>
            Some("graph-cut closure is not a documented fallback for directional grain, brushed structure, lattice, motif, unique, or radial semantics".into()),
        DomainRoute::TextureQuilting if !matches!(class, MaterialSourceClass::FineConcretePlaster
            | MaterialSourceClass::RustGrunge | MaterialSourceClass::BrushedMetal | MaterialSourceClass::WoodFaceGrain
            | MaterialSourceClass::MixedUnknown) => Some("quilting is not applicable to this semantic source class".into()),
        DomainRoute::PatchMatch if !matches!(class, MaterialSourceClass::UniqueVentPanel | MaterialSourceClass::MixedUnknown) =>
            Some("PatchMatch is reserved for constrained unique/base-material expansion".into()),
        DomainRoute::StatisticalSynthesis if structured || behavior != MaterialBehaviorClass::StochasticIsotropic =>
            Some("statistical synthesis is forbidden for semantic, periodic, directional, unique, or radial structure".into()),
        DomainRoute::ProceduralReconstruction if !matches!(class, MaterialSourceClass::FineConcretePlaster
            | MaterialSourceClass::BrushedMetal | MaterialSourceClass::BrickTile | MaterialSourceClass::WoodFaceGrain
            | MaterialSourceClass::WoodEndGrain) => Some("no fitted V1 procedural family applies to this source class".into()),
        DomainRoute::LearnedProvider if request.policy.learned == LearnedPolicy::Disabled => Some("learned routes are disabled by policy".into()),
        DomainRoute::LearnedProvider if provider.is_none() => Some("no approved local learned provider is installed; classical/procedural fallback remains authoritative".into()),
        DomainRoute::LearnedProvider if !matches!(class, MaterialSourceClass::WoodEndGrain | MaterialSourceClass::MixedUnknown) =>
            Some("learned expansion is not needed by the preferred documented route".into()),
        DomainRoute::LearnedProvider if provider.is_some_and(|p| p.interface_version != LEARNED_PROVIDER_INTERFACE_VERSION
            || !p.deterministic || !p.capabilities.contains(&LearnedCapability::SeamlessExpansion)) =>
            Some("provider is incompatible, nondeterministic, or lacks seamless expansion".into()),
        _ => None,
    }
}

fn measured_period_alignment(request: &Stage8RouterRequest, class: MaterialSourceClass) -> bool {
    let base = request.domain.source.base_color();
    let (width, height) = (base.width(), base.height());
    let boundary_aligned = request.domain.analysis.seamability.horizontal_cost_milli
        <= request.domain.direct_boundary_threshold_milli
        && request.domain.analysis.seamability.vertical_cost_milli
            <= request.domain.direct_boundary_threshold_milli;
    if !boundary_aligned { return false; }
    request.domain.analysis.periodicity.candidates.iter().any(|candidate| {
        if candidate.confidence_milli < 180 || candidate.evidence_samples == 0 { return false; }
        let vectors = [Some(candidate.first), candidate.second];
        let x_period = vectors.into_iter().flatten().find_map(|vector| {
            (vector.dy_pixels == 0 && vector.dx_pixels != 0 && vector.confidence_milli >= 180)
                .then_some(vector.dx_pixels.unsigned_abs())
        });
        let y_period = vectors.into_iter().flatten().find_map(|vector| {
            (vector.dx_pixels == 0 && vector.dy_pixels != 0 && vector.confidence_milli >= 180)
                .then_some(vector.dy_pixels.unsigned_abs())
        });
        let x_aligned = x_period.is_some_and(|period| period > 0 && width % period == 0);
        let y_aligned = y_period.is_some_and(|period| period > 0 && height % period == 0);
        match class {
            MaterialSourceClass::BrickTile => x_aligned && y_aligned,
            MaterialSourceClass::ManufacturedBorder => x_aligned || y_aligned,
            _ => false,
        }
    })
}

fn normalized_cost(request: &Stage8RouterRequest, class: MaterialSourceClass, route: DomainRoute) -> RouteCost {
    let m = request.stage_five.classification.measurements;
    let seam = request.domain.analysis.seamability.horizontal_cost_milli
        .max(request.domain.analysis.seamability.vertical_cost_milli);
    let preferred = preferred_route(class);
    let fidelity = if route == preferred { 80 } else { 220 };
    let semantic = match route {
        DomainRoute::StatisticalSynthesis => 1000_u16.saturating_sub(m.stationarity_milli),
        DomainRoute::GraphCutPeriodicClosure => m.localized_saliency_milli,
        DomainRoute::TextureQuilting | DomainRoute::PatchMatch => m.regularity_milli / 2,
        DomainRoute::LearnedProvider => 300,
        _ => 80,
    };
    let compute = match route { DomainRoute::DirectSource => 20, DomainRoute::GraphCutPeriodicClosure => 180,
        DomainRoute::TextureQuilting => 420, DomainRoute::PatchMatch => 600,
        DomainRoute::StatisticalSynthesis => 360, DomainRoute::ProceduralReconstruction => 260,
        DomainRoute::LearnedProvider => 500, DomainRoute::Auto => 1000 };
    let uncertainty = 1000_u16.saturating_sub(request.stage_five.classification.confidence_milli);
    let seam_cost = if route == DomainRoute::DirectSource { seam } else { seam / 4 };
    let total = ((u32::from(fidelity) * 3 + u32::from(seam_cost) * 2 + u32::from(semantic) * 4
        + u32::from(compute) + u32::from(uncertainty) * 2) / 12).min(1000) as u16;
    RouteCost { fidelity_milli: fidelity, seam_milli: seam_cost, semantic_risk_milli: semantic,
        compute_milli: compute, uncertainty_milli: uncertainty, total_milli: total }
}

fn preferred_route(class: MaterialSourceClass) -> DomainRoute {
    match class {
        MaterialSourceClass::ExistingTileablePbr | MaterialSourceClass::BrickTile
        | MaterialSourceClass::ManufacturedBorder | MaterialSourceClass::UniqueVentPanel
        | MaterialSourceClass::RadialDrainWasher => DomainRoute::DirectSource,
        MaterialSourceClass::FineConcretePlaster | MaterialSourceClass::RustGrunge
        | MaterialSourceClass::BrushedMetal | MaterialSourceClass::WoodFaceGrain => DomainRoute::TextureQuilting,
        MaterialSourceClass::WoodEndGrain => DomainRoute::ProceduralReconstruction,
        MaterialSourceClass::MixedUnknown => DomainRoute::GraphCutPeriodicClosure,
    }
}

fn apply_user_route_policy(request: &Stage8RouterRequest, class: MaterialSourceClass, preview: (u16, u16),
    compared: &mut Vec<RouteComparison>)
{
    let forced = request.policy.pinned_route.or(request.policy.override_route);
    if let Some(route) = forced {
        if let Some(item) = compared.iter_mut().find(|item| item.route == route) {
            if item.applicable { item.cost = Some(RouteCost { total_milli: 0, ..item.cost.expect("applicable cost") }); }
            else { item.rejection = Some(format!("forced route {route:?} rejected for {class:?}: {}",
                item.rejection.as_deref().unwrap_or("not applicable"))); }
        } else {
            compared.push(RouteComparison { route, applicable: false, preview_dimensions: preview, cost: None,
                rejection: Some(format!("route {route:?} is not executable for {class:?}")), attempted: false });
        }
    } else if request.policy.learned == LearnedPolicy::PreferWhenApplicable
        && let Some(item) = compared.iter_mut().find(|item| item.route == DomainRoute::LearnedProvider && item.applicable)
    { item.cost = Some(RouteCost { total_milli: 0, ..item.cost.expect("applicable cost") }); }
}

fn execute_route(request: &Stage8RouterRequest, route: DomainRoute,
    provider: Option<&dyn LocalLearnedMaterialProvider>, descriptor: Option<&LearnedProviderDescriptor>,
    cache: &mut MaterialDomainCache, cancellation: &RenderCancellationToken)
    -> Result<(PreparedMaterialDomain, Option<LearnedProviderState>), String>
{
    match route {
        DomainRoute::DirectSource | DomainRoute::GraphCutPeriodicClosure
        | DomainRoute::TextureQuilting | DomainRoute::PatchMatch => {
            let mut domain_request = request.domain.clone(); domain_request.route = route;
            let source_class = classify_source(request).0;
            if route == DomainRoute::TextureQuilting {
                domain_request.quilting.semantics = match source_class {
                    MaterialSourceClass::BrushedMetal | MaterialSourceClass::WoodFaceGrain =>
                        QuiltingSemanticConstraint::Directional {
                            behavior: request.stage_five.classification.routed_class(),
                            requested_angle_millidegrees: request.domain.scale_orientation.global_orientation
                                .axis_millidegrees.unwrap_or(0) as i32,
                            tolerance_millidegrees: 30_000,
                        },
                    MaterialSourceClass::MixedUnknown => QuiltingSemanticConstraint::MixedUnknown,
                    _ => QuiltingSemanticConstraint::StochasticIsotropic,
                };
            }
            if route == DomainRoute::PatchMatch && source_class == MaterialSourceClass::MixedUnknown {
                domain_request.patch_match.semantics = PatchMatchSemanticConstraint::MixedUnknown;
            }
            prepare_material_domain(&domain_request, cache, cancellation).map(|domain| (domain, None)).map_err(|error| error.to_string())
        }
        DomainRoute::StatisticalSynthesis => statistical_domain(request, cancellation).map(|domain| (domain, None)),
        DomainRoute::ProceduralReconstruction => procedural_domain(request, cancellation).map(|domain| (domain, None)),
        DomainRoute::LearnedProvider => learned_domain(request, provider.ok_or("learned provider is absent")?,
            descriptor.ok_or("learned provider descriptor is absent")?, cancellation),
        DomainRoute::Auto => Err("Auto is not executable".into()),
    }
}

fn statistical_domain(request: &Stage8RouterRequest, cancellation: &RenderCancellationToken)
    -> Result<PreparedMaterialDomain, String>
{
    let (width, height) = (request.output_width, request.output_height);
    let source = request.domain.source.base_color();
    let mut samples = Vec::with_capacity((u64::from(width) * u64::from(height)) as usize);
    for y in 0..height {
        if y % 16 == 0 && cancellation.is_cancelled() { return Err("statistical synthesis was cancelled".into()); }
        for x in 0..width {
            samples.push(multiscale_statistical_sample(request.domain.seed, x, y, width, height,
                source.width(), source.height()));
        }
    }
    let channels = super::compose_channels(&request.domain, &samples, width, height, cancellation).map_err(|e| e.to_string())?;
    let quality = statistical_quality(source, &channels)?;
    if quality.total_error_milli > 650 {
        return Err(format!("statistical publication gate rejected multiscale mismatch {}/1000", quality.total_error_milli));
    }
    let validity = super::source_validity(&request.domain, &samples, width, height, cancellation).map_err(|e| e.to_string())?;
    let tile = source.tile_edge();
    let correspondence = CorrespondenceField::Registered(super::plane(width, height, tile, samples).map_err(|e| e.to_string())?);
    let operations = OperationField::Registered(super::plane(width, height, tile,
        vec![DomainOperation::StatisticalSample; (width * height) as usize]).map_err(|e| e.to_string())?);
    let provenance = super::plane(width, height, tile,
        vec![ProvenanceValue::ClassicalSynthesized; (width * height) as usize]).map_err(|e| e.to_string())?;
    let key = ContentDigest::sha256(format!("{}|{}|{}|{}|{}|{}", STAGE_08D_STATISTICAL_ALGORITHM_VERSION,
        request.domain.prepared_source_digest.0, request.domain.analysis.cache_key.0, request.domain.seed,
        width, height).as_bytes());
    Ok(PreparedMaterialDomain { cache_key: key.clone(), prepared_source_digest: request.domain.prepared_source_digest.clone(),
        analysis_digest: request.domain.analysis.cache_key.clone(), route: DomainRoute::StatisticalSynthesis,
        width, height, channels: DomainChannelStorage::Generated(channels), correspondence, operations, validity,
        provenance, seams: Vec::new(), quilting: None, patch_match: None,
        diagnostics: DomainDiagnostics { selected_route: DomainRoute::StatisticalSynthesis, cache_key: key.clone(),
            available_seam_terms: request.domain.analysis.seamability.available_terms.clone(), normalized_weight_milli: BTreeMap::new(),
            pass_through: None, seams: Vec::new(), boundary_cost_before_milli: (request.domain.analysis.seamability.horizontal_cost_milli,
                request.domain.analysis.seamability.vertical_cost_milli), boundary_cost_after_milli: (0, 0),
            messages: vec![format!("deterministic periodic multiscale field preserved registered correspondence; mean {}, variance {}, gradient {}, and lag-spectrum {} milli error",
                quality.mean_error_milli, quality.variance_error_milli, quality.gradient_error_milli,
                quality.lag_spectrum_error_milli)] },
        qa_views: authoritative_qa_views(), stage_result: StageResult::Executed { algorithm: AlgorithmProvenance {
            algorithm_id: STAGE_08D_STATISTICAL_ALGORITHM_ID.into(), version: STAGE_08D_STATISTICAL_ALGORITHM_VERSION.into(),
        }, settings_hash: key, diagnostics: Vec::new() } })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StatisticalQuality {
    mean_error_milli: u16,
    variance_error_milli: u16,
    gradient_error_milli: u16,
    lag_spectrum_error_milli: u16,
    total_error_milli: u16,
}

fn multiscale_statistical_sample(seed: u64, x: u32, y: u32, width: u32, height: u32,
    source_width: u32, source_height: u32) -> CorrespondenceSample
{
    let px = if width > 1 && x == width - 1 { 0 } else { x };
    let py = if height > 1 && y == height - 1 { 0 } else { y };
    let u = if width > 1 { f64::from(px) / f64::from(width - 1) } else { 0.0 };
    let v = if height > 1 { f64::from(py) / f64::from(height - 1) } else { 0.0 };
    let minimum = f64::from(source_width.min(source_height));
    let mut dx = 0.0; let mut dy = 0.0;
    for (octave, frequency) in [1.0_f64, 2.0, 4.0].into_iter().enumerate() {
        let hash = splitmix08f(seed ^ (octave as u64).rotate_left(29));
        let phase_x = (hash as f64 / u64::MAX as f64) * std::f64::consts::TAU;
        let phase_y = ((hash.rotate_left(31)) as f64 / u64::MAX as f64) * std::f64::consts::TAU;
        let amplitude = minimum / (24.0 * frequency);
        dx += ((std::f64::consts::TAU * frequency * u + phase_x).sin()
            * (std::f64::consts::TAU * frequency * v + phase_y).cos()) * amplitude;
        dy += ((std::f64::consts::TAU * frequency * v + phase_y).sin()
            * (std::f64::consts::TAU * frequency * u + phase_x).cos()) * amplitude;
    }
    let sx = (u * f64::from(source_width) + dx).rem_euclid(f64::from(source_width));
    let sy = (v * f64::from(source_height) + dy).rem_euclid(f64::from(source_height));
    let x0 = sx.floor() as u32 % source_width; let y0 = sy.floor() as u32 % source_height;
    let x1 = (x0 + 1) % source_width; let y1 = (y0 + 1) % source_height;
    let fx = (sx - sx.floor()) as f32; let fy = (sy - sy.floor()) as f32;
    CorrespondenceSample { sources: [
        Some(WeightedSource { coordinate: SourceCoordinate { x: x0, y: y0 }, weight: (1.0 - fx) * (1.0 - fy) }),
        Some(WeightedSource { coordinate: SourceCoordinate { x: x1, y: y0 }, weight: fx * (1.0 - fy) }),
        Some(WeightedSource { coordinate: SourceCoordinate { x: x0, y: y1 }, weight: (1.0 - fx) * fy }),
        Some(WeightedSource { coordinate: SourceCoordinate { x: x1, y: y1 }, weight: fx * fy }),
    ] }
}

fn statistical_quality(source: &hot_trimmer_image_io::ImagePlane<hot_trimmer_image_io::LinearColor>,
    channels: &[PreparedExemplarChannel]) -> Result<StatisticalQuality, String>
{
    let generated = channels.iter().find_map(|channel| match channel {
        PreparedExemplarChannel::BaseColor { plane, .. } => Some(plane), _ => None,
    }).ok_or("statistical synthesis did not produce Base Color")?;
    let a = color_statistics(source); let b = color_statistics(generated);
    let error = |left: f64, right: f64, floor: f64| -> u16 {
        (((left - right).abs() / left.abs().max(right.abs()).max(floor)).min(1.0) * 1000.0).round() as u16
    };
    let mean = error(a.0, b.0, 0.02); let variance = error(a.1, b.1, 0.005);
    let gradient = error(a.2, b.2, 0.01);
    let lag = ((0..3).map(|index| u32::from(error(a.3[index], b.3[index], 0.05))).sum::<u32>() / 3) as u16;
    let total = ((u32::from(mean) + u32::from(variance) * 2 + u32::from(gradient) * 2 + u32::from(lag) * 3) / 8) as u16;
    Ok(StatisticalQuality { mean_error_milli: mean, variance_error_milli: variance,
        gradient_error_milli: gradient, lag_spectrum_error_milli: lag, total_error_milli: total })
}

fn color_statistics(plane: &hot_trimmer_image_io::ImagePlane<hot_trimmer_image_io::LinearColor>)
    -> (f64, f64, f64, [f64; 3])
{
    let luminance = |x: u32, y: u32| { let c = plane.pixel(x, y).rgb;
        f64::from(c[0]) * 0.2126 + f64::from(c[1]) * 0.7152 + f64::from(c[2]) * 0.0722 };
    let count = f64::from(plane.width()) * f64::from(plane.height());
    let mean = (0..plane.height()).flat_map(|y| (0..plane.width()).map(move |x| luminance(x, y))).sum::<f64>() / count;
    let variance = (0..plane.height()).flat_map(|y| (0..plane.width()).map(move |x| {
        let d = luminance(x, y) - mean; d * d })).sum::<f64>() / count;
    let mut gradient = 0.0;
    for y in 0..plane.height() { for x in 0..plane.width() {
        gradient += (luminance((x + 1) % plane.width(), y) - luminance(x, y)).abs();
        gradient += (luminance(x, (y + 1) % plane.height()) - luminance(x, y)).abs();
    }}
    gradient /= count * 2.0;
    let mut correlations = [0.0; 3];
    for (index, lag) in [1_u32, 2, 4].into_iter().enumerate() {
        let mut covariance = 0.0;
        for y in 0..plane.height() { for x in 0..plane.width() {
            let centered = luminance(x, y) - mean;
            covariance += centered * (luminance((x + lag) % plane.width(), y) - mean);
            covariance += centered * (luminance(x, (y + lag) % plane.height()) - mean);
        }}
        correlations[index] = covariance / (count * 2.0 * variance.max(1.0e-9));
    }
    (mean, variance, gradient, correlations)
}

fn procedural_domain(request: &Stage8RouterRequest, cancellation: &RenderCancellationToken)
    -> Result<PreparedMaterialDomain, String>
{
    let fit = ProceduralFitRequest { source: Arc::clone(&request.domain.source), stage_five: Arc::clone(&request.stage_five),
        stage_six: Arc::clone(&request.domain.scale_orientation), stage_seven: Arc::clone(&request.domain.analysis),
        user_override: procedural_override(request), content_intent: ProceduralContentIntent::MaterialOnly,
        seed: request.domain.seed, settings: request.procedural_settings };
    let ProceduralFitOutcome::Fitted(model) = fit_procedural_domain_model(&fit, cancellation).map_err(|e| e.to_string())?
        else { return Err("measured evidence and typed corrections did not support a procedural model".into()); };
    let generated = model.generate(ProceduralDomainSpec { width: request.output_width, height: request.output_height,
        extent_x: 1_000_000, extent_y: 1_000_000,
        topology: if model.kind() == ProceduralModelKind::WoodEndGrain { ProceduralCoordinateTopology::PolarCompatible }
            else { ProceduralCoordinateTopology::Cartesian } }, cancellation).map_err(|e| e.to_string())?;
    let (width, height) = (generated.width, generated.height); let tile = generated.layer_mask.tile_edge();
    let key = generated.model_digest.clone();
    Ok(PreparedMaterialDomain { cache_key: key.clone(), prepared_source_digest: request.domain.prepared_source_digest.clone(),
        analysis_digest: request.domain.analysis.cache_key.clone(), route: DomainRoute::ProceduralReconstruction,
        width, height, channels: DomainChannelStorage::Generated(generated.channels),
        correspondence: CorrespondenceField::Identity { width, height },
        operations: OperationField::Registered(super::plane(width, height, tile,
            vec![DomainOperation::ProceduralSample; (width * height) as usize]).map_err(|e| e.to_string())?),
        validity: super::plane(width, height, tile, vec![MaskValue(1.0); (width * height) as usize]).map_err(|e| e.to_string())?,
        provenance: super::plane(width, height, tile,
            vec![ProvenanceValue::ProceduralEstimated; (width * height) as usize]).map_err(|e| e.to_string())?,
        seams: Vec::new(), quilting: None, patch_match: None,
        diagnostics: DomainDiagnostics { selected_route: DomainRoute::ProceduralReconstruction, cache_key: key.clone(),
            available_seam_terms: request.domain.analysis.seamability.available_terms.clone(), normalized_weight_milli: BTreeMap::new(),
            pass_through: None, seams: Vec::new(), boundary_cost_before_milli: (request.domain.analysis.seamability.horizontal_cost_milli,
                request.domain.analysis.seamability.vertical_cost_milli), boundary_cost_after_milli: (0, 0),
            messages: vec!["all channels were evaluated from one fitted material-coordinate model".into()] },
        qa_views: authoritative_qa_views(), stage_result: generated.stage_result })
}

fn procedural_override(request: &Stage8RouterRequest) -> Option<ProceduralUserOverride> {
    if request.procedural_override.is_some() { return request.procedural_override.clone(); }
    let kind = match classify_source(request).0 {
        MaterialSourceClass::FineConcretePlaster => ProceduralModelKind::ConcreteAggregate,
        MaterialSourceClass::BrushedMetal => ProceduralModelKind::BrushedMetal,
        MaterialSourceClass::BrickTile => ProceduralModelKind::BrickTileLattice,
        MaterialSourceClass::WoodFaceGrain => ProceduralModelKind::WoodFaceGrain,
        MaterialSourceClass::WoodEndGrain => ProceduralModelKind::WoodEndGrain,
        _ => return None,
    };
    Some(ProceduralUserOverride { model_kind: Some(kind), ..ProceduralUserOverride::default() })
}

fn learned_domain(request: &Stage8RouterRequest, provider: &dyn LocalLearnedMaterialProvider,
    descriptor: &LearnedProviderDescriptor, cancellation: &RenderCancellationToken)
    -> Result<(PreparedMaterialDomain, Option<LearnedProviderState>), String>
{
    let input_pixels = u64::from(request.domain.source.base_color().width()) * u64::from(request.domain.source.base_color().height());
    let output_pixels = u64::from(request.output_width) * u64::from(request.output_height);
    if descriptor.interface_version != LEARNED_PROVIDER_INTERFACE_VERSION || !descriptor.deterministic
        || input_pixels > descriptor.maximum_input_pixels || output_pixels > descriptor.maximum_output_pixels
    { return Err("learned provider descriptor or bounds are invalid".into()); }
    let mut requested = BTreeSet::from([LearnedCapability::SeamlessExpansion]);
    for optional in [LearnedCapability::EstimatedHeight, LearnedCapability::EstimatedNormal] {
        if descriptor.capabilities.contains(&optional) { requested.insert(optional); }
    }
    let learned_request = LearnedMaterialRequest { source: &request.domain.source, analysis: &request.domain.analysis,
        requested, output_width: request.output_width, output_height: request.output_height,
        seed: request.domain.seed, deterministic: true };
    if !learned_request.requested.iter().all(|capability| descriptor.capabilities.contains(capability)) {
        return Err("learned provider lacks one or more requested output capabilities".into());
    }
    let output = provider.infer(&learned_request, cancellation).map_err(|e| e.to_string())?;
    let replay = provider.infer(&learned_request, cancellation).map_err(|e| format!("learned deterministic replay failed: {e}"))?;
    let actual_digest = validate_learned_output(&output, descriptor, request.output_width, request.output_height)?;
    let replay_digest = validate_learned_output(&replay, descriptor, request.output_width, request.output_height)?;
    if actual_digest != replay_digest || output.output_digest != actual_digest || replay.output_digest != replay_digest {
        return Err("learned provider failed byte-level deterministic replay or claimed an unverified output digest".into());
    }
    let (width, height) = (request.output_width, request.output_height); let tile = channel_tile_edge(&output.channels[0]);
    let key = ContentDigest::sha256(format!("{}|{}|{}|{}|{:?}", descriptor.model_digest.0, actual_digest.0,
        request.domain.analysis.registration_digest.0, request.domain.seed, output.device).as_bytes());
    let domain = PreparedMaterialDomain { cache_key: key.clone(), prepared_source_digest: request.domain.prepared_source_digest.clone(),
        analysis_digest: request.domain.analysis.cache_key.clone(), route: DomainRoute::LearnedProvider,
        width, height, channels: DomainChannelStorage::Generated(output.channels),
        correspondence: CorrespondenceField::Identity { width, height },
        operations: OperationField::Registered(super::plane(width, height, tile,
            vec![DomainOperation::LearnedSample; (width * height) as usize]).map_err(|e| e.to_string())?),
        validity: super::plane(width, height, tile, vec![MaskValue(1.0); (width * height) as usize]).map_err(|e| e.to_string())?,
        provenance: super::plane(width, height, tile,
            vec![ProvenanceValue::LearnedEstimated; (width * height) as usize]).map_err(|e| e.to_string())?,
        seams: Vec::new(), quilting: None, patch_match: None,
        diagnostics: DomainDiagnostics { selected_route: DomainRoute::LearnedProvider, cache_key: key.clone(),
            available_seam_terms: request.domain.analysis.seamability.available_terms.clone(), normalized_weight_milli: BTreeMap::new(),
            pass_through: None, seams: Vec::new(), boundary_cost_before_milli: (request.domain.analysis.seamability.horizontal_cost_milli,
                request.domain.analysis.seamability.vertical_cost_milli), boundary_cost_after_milli: (0, 0), messages: output.diagnostics },
        qa_views: authoritative_qa_views(), stage_result: StageResult::Executed { algorithm: AlgorithmProvenance {
            algorithm_id: format!("local-learned:{}", descriptor.provider_id), version: descriptor.provider_version.clone(),
        }, settings_hash: key, diagnostics: vec![CompilationDiagnostic { code: DiagnosticCode::InsufficientInput, stage: Some(8),
            message: "learned channels are labeled Estimated; classical/procedural routes remain available".into(), context: BTreeMap::new() }] } };
    Ok((domain, Some(LearnedProviderState::Used { provider_id: descriptor.provider_id.clone(),
        provider_version: descriptor.provider_version.clone(), model_digest: descriptor.model_digest.clone(),
        output_digest: actual_digest, confidence_milli: output.confidence_milli })))
}

fn validate_learned_output(output: &LearnedMaterialOutput, descriptor: &LearnedProviderDescriptor,
    width: u32, height: u32) -> Result<ContentDigest, String>
{
    let device_valid = match descriptor.device_policy {
        LearnedDevicePolicy::CpuOnly => output.device == LearnedExecutionDevice::Cpu,
        LearnedDevicePolicy::GpuAllowed => true,
        LearnedDevicePolicy::GpuRequired => output.device == LearnedExecutionDevice::Gpu,
    };
    if output.model_digest != descriptor.model_digest || !output.deterministic || output.confidence_milli > 1000
        || !device_valid || output.channels.is_empty()
        || output.channels.iter().any(|channel| channel.dimensions() != (width, height))
    { return Err("learned output failed model, device-policy, determinism, confidence, or registration validation".into()); }
    Ok(canonical_learned_output_digest(&output.channels))
}

#[must_use]
pub fn canonical_learned_output_digest(channels: &[PreparedExemplarChannel]) -> ContentDigest {
    let mut hash = Sha256::new();
    hash.update((channels.len() as u64).to_le_bytes());
    for channel in channels {
        hash.update(format!("{:?}", channel.role()).as_bytes());
        let (width, height) = channel.dimensions(); hash.update(width.to_le_bytes()); hash.update(height.to_le_bytes());
        match channel {
            PreparedExemplarChannel::BaseColor { plane, alpha_mode } => {
                hash.update(format!("{alpha_mode:?}").as_bytes()); hash.update(plane.tile_edge().to_le_bytes());
                for tile in plane.tiles() { for pixel in &tile.pixels { for value in pixel.rgb { hash.update(value.to_bits().to_le_bytes()); }
                    hash.update(pixel.alpha.to_bits().to_le_bytes()); }}
            }
            PreparedExemplarChannel::Scalar { plane, .. } => {
                hash.update(plane.tile_edge().to_le_bytes());
                for tile in plane.tiles() { for pixel in &tile.pixels { hash.update(pixel.0.to_bits().to_le_bytes()); }}
            }
            PreparedExemplarChannel::Normal { plane, source_convention, canonical_convention, alpha_policy } => {
                hash.update(format!("{source_convention:?}|{canonical_convention:?}|{alpha_policy:?}").as_bytes());
                hash.update(plane.tile_edge().to_le_bytes());
                for tile in plane.tiles() { for pixel in &tile.pixels { for value in pixel.xyz { hash.update(value.to_bits().to_le_bytes()); }
                    hash.update(pixel.alpha.to_bits().to_le_bytes()); }}
            }
            PreparedExemplarChannel::MaterialId { plane } => {
                hash.update(plane.tile_edge().to_le_bytes());
                for tile in plane.tiles() { for pixel in &tile.pixels { hash.update(pixel.0.to_le_bytes()); }}
            }
            PreparedExemplarChannel::Mask { plane, .. } => {
                hash.update(plane.tile_edge().to_le_bytes());
                for tile in plane.tiles() { for pixel in &tile.pixels { hash.update(pixel.0.to_bits().to_le_bytes()); }}
            }
        }
    }
    ContentDigest(format!("{:x}", hash.finalize()))
}

fn validate_chosen_domain(request: &Stage8RouterRequest, domain: &PreparedMaterialDomain, class: MaterialSourceClass)
    -> Result<(bool, bool, bool, bool, bool, bool), String>
{
    let registration = !domain.registered_channels().is_empty()
        && domain.registered_channels().iter().all(|channel| channel.dimensions() == (domain.width, domain.height))
        && (domain.validity.width(), domain.validity.height()) == (domain.width, domain.height)
        && (domain.provenance.width(), domain.provenance.height()) == (domain.width, domain.height);
    let correspondence = match &domain.correspondence { CorrespondenceField::Identity { width, height } => (*width, *height) == (domain.width, domain.height),
        CorrespondenceField::Registered(field) => (field.width(), field.height()) == (domain.width, domain.height) };
    let periodic_expected = !matches!(class, MaterialSourceClass::UniqueVentPanel | MaterialSourceClass::RadialDrainWasher);
    let boundary = measured_domain_boundary(domain);
    let threshold = if domain.route == DomainRoute::DirectSource { request.domain.direct_boundary_threshold_milli }
        else { request.domain.graph_cut.max_accepted_seam_cost_milli };
    let direct_alignment = domain.route != DomainRoute::DirectSource
        || !matches!(class, MaterialSourceClass::BrickTile | MaterialSourceClass::ManufacturedBorder)
        || measured_period_alignment(request, class);
    let period = !periodic_expected || direct_alignment && boundary.0 <= threshold && boundary.1 <= threshold;
    let scale = request.domain.scale_orientation.prepared_source_digest == request.domain.prepared_source_digest;
    let deterministic = domain.route != DomainRoute::LearnedProvider || domain.provenance.tiles().iter()
        .all(|tile| tile.pixels.iter().all(|value| *value == ProvenanceValue::LearnedEstimated));
    let cache = !domain.cache_key.0.is_empty() && domain.prepared_source_digest == request.domain.prepared_source_digest
        && domain.analysis_digest == request.domain.analysis.cache_key;
    if registration && correspondence && period && scale && deterministic && cache { Ok((true, true, true, true, true, true)) }
    else { Err(format!("chosen domain validation failed: registration={registration}, period={period} (measured boundary {}/{}, threshold {threshold}), scale={scale}, correspondence={correspondence}, determinism={deterministic}, cache={cache}", boundary.0, boundary.1)) }
}

fn measured_domain_boundary(domain: &PreparedMaterialDomain) -> (u16, u16) {
    let (width, height) = (domain.width, domain.height); if width == 0 || height == 0 { return (1000, 1000); }
    let mut worst_horizontal = 0.0_f64; let mut worst_vertical = 0.0_f64;
    for channel in domain.registered_channels() {
        let (horizontal, vertical) = match channel {
            PreparedExemplarChannel::BaseColor { plane, .. } => {
                ((0..height).map(|y| { let a = plane.pixel(0, y).rgb; let b = plane.pixel(width - 1, y).rgb;
                    a.into_iter().zip(b).map(|(x, y)| f64::from((x - y).abs())).sum::<f64>() / 3.0 }).sum::<f64>() / f64::from(height),
                (0..width).map(|x| { let a = plane.pixel(x, 0).rgb; let b = plane.pixel(x, height - 1).rgb;
                    a.into_iter().zip(b).map(|(x, y)| f64::from((x - y).abs())).sum::<f64>() / 3.0 }).sum::<f64>() / f64::from(width))
            }
            PreparedExemplarChannel::Scalar { plane, .. } => {
                ((0..height).map(|y| f64::from((plane.pixel(0, y).0 - plane.pixel(width - 1, y).0).abs())).sum::<f64>() / f64::from(height),
                (0..width).map(|x| f64::from((plane.pixel(x, 0).0 - plane.pixel(x, height - 1).0).abs())).sum::<f64>() / f64::from(width))
            }
            PreparedExemplarChannel::Normal { plane, .. } => {
                let difference = |a: [f32; 3], b: [f32; 3]| (1.0 - a.into_iter().zip(b).map(|(x, y)| x * y).sum::<f32>()).clamp(0.0, 2.0) * 0.5;
                ((0..height).map(|y| f64::from(difference(plane.pixel(0, y).xyz, plane.pixel(width - 1, y).xyz))).sum::<f64>() / f64::from(height),
                (0..width).map(|x| f64::from(difference(plane.pixel(x, 0).xyz, plane.pixel(x, height - 1).xyz))).sum::<f64>() / f64::from(width))
            }
            PreparedExemplarChannel::MaterialId { plane } => {
                ((0..height).map(|y| if plane.pixel(0, y) != plane.pixel(width - 1, y) { 1.0 } else { 0.0 }).sum::<f64>() / f64::from(height),
                (0..width).map(|x| if plane.pixel(x, 0) != plane.pixel(x, height - 1) { 1.0 } else { 0.0 }).sum::<f64>() / f64::from(width))
            }
            PreparedExemplarChannel::Mask { plane, .. } => {
                ((0..height).map(|y| f64::from((plane.pixel(0, y).0 - plane.pixel(width - 1, y).0).abs())).sum::<f64>() / f64::from(height),
                (0..width).map(|x| f64::from((plane.pixel(x, 0).0 - plane.pixel(x, height - 1).0).abs())).sum::<f64>() / f64::from(width))
            }
        };
        worst_horizontal = worst_horizontal.max(horizontal);
        worst_vertical = worst_vertical.max(vertical);
    }
    ((worst_horizontal.clamp(0.0, 1.0) * 1000.0).round() as u16,
        (worst_vertical.clamp(0.0, 1.0) * 1000.0).round() as u16)
}

fn authoritative_qa_views() -> Vec<DomainQaView> {
    vec![DomainQaView::RegisteredChannels, DomainQaView::BoundaryDifference, DomainQaView::Correspondence,
        DomainQaView::Operations, DomainQaView::Validity, DomainQaView::Provenance,
        DomainQaView::RouteComparison, DomainQaView::Applicability, DomainQaView::Scale,
        DomainQaView::Determinism, DomainQaView::CacheProvenance]
}

fn channel_tile_edge(channel: &PreparedExemplarChannel) -> u32 {
    match channel {
        PreparedExemplarChannel::BaseColor { plane, .. } => plane.tile_edge(),
        PreparedExemplarChannel::Scalar { plane, .. } => plane.tile_edge(),
        PreparedExemplarChannel::Normal { plane, .. } => plane.tile_edge(),
        PreparedExemplarChannel::MaterialId { plane } => plane.tile_edge(),
        PreparedExemplarChannel::Mask { plane, .. } => plane.tile_edge(),
    }
}

fn router_settings_hash(request: &Stage8RouterRequest, diagnostics: &Stage8DomainDiagnostics) -> ContentDigest {
    ContentDigest::sha256(format!("{}|{}|{}|{:?}|{:?}|{:?}|{}x{}|{}", STAGE_08_ROUTER_ALGORITHM_VERSION,
        request.domain.prepared_source_digest.0, request.domain.analysis.cache_key.0, diagnostics.source_class,
        request.policy.pinned_route, request.policy.override_route, request.output_width, request.output_height,
        request.domain.seed).as_bytes())
}

const fn route_rank(route: DomainRoute) -> u8 {
    match route { DomainRoute::DirectSource => 0, DomainRoute::GraphCutPeriodicClosure => 1,
        DomainRoute::TextureQuilting => 2, DomainRoute::PatchMatch => 3,
        DomainRoute::StatisticalSynthesis => 4, DomainRoute::ProceduralReconstruction => 5,
        DomainRoute::LearnedProvider => 6, DomainRoute::Auto => 7 }
}

fn splitmix08f(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}
