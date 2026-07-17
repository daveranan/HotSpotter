//! Stage 8A registered material domains and deterministic periodic graph-cut closure.

use std::{collections::{BTreeMap, BTreeSet}, sync::Arc};

use hot_trimmer_domain::{
    AlgorithmProvenance, CompilationDiagnostic, ContentDigest, DiagnosticCode,
    MaterialChannelRole, RecoveryChoice, StageResult,
};
use hot_trimmer_image_io::{
    CategoryId, ImagePlane, LinearColor, LinearScalar, MaskValue, TangentNormal,
};
use hot_trimmer_material_analysis::{FeatureFieldReport, ScaleOrientationReport, SeamTerm};
use hot_trimmer_render_core::{
    PreparedExemplarChannel, RenderCancellationToken,
};
use hot_trimmer_material_analysis::DelitPreparedExemplar;
use thiserror::Error;

#[path = "quilting.rs"]
mod quilting;
pub use quilting::*;
#[path = "patchmatch.rs"]
mod patchmatch;
pub use patchmatch::*;
#[path = "procedural.rs"]
mod procedural;
pub use procedural::*;
#[path = "router.rs"]
mod router;
pub use router::*;

pub const STAGE_08A_DIRECT_ALGORITHM_ID: &str = "hot_trimmer.direct_source_domain";
pub const STAGE_08A_GRAPHCUT_ALGORITHM_ID: &str = "hot_trimmer.multichannel_graphcut_periodic_closure";
pub const STAGE_08A_ALGORITHM_VERSION: &str = "8.1.0";
const MAX_CACHE_ENTRIES: usize = 12;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ClosureAxes { X, Y, XY }

impl ClosureAxes {
    const fn includes_x(self) -> bool { matches!(self, Self::X | Self::XY) }
    const fn includes_y(self) -> bool { matches!(self, Self::Y | Self::XY) }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DomainRoute {
    /// Select from measured tileability/seam evidence and policy thresholds.
    Auto,
    DirectSource,
    GraphCutPeriodicClosure,
    TextureQuilting,
    PatchMatch,
    StatisticalSynthesis,
    ProceduralReconstruction,
    LearnedProvider,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SeamWeights {
    pub color: u16,
    pub gradient: u16,
    pub height: u16,
    pub vector_normal: u16,
    pub roughness: u16,
    pub structure_cut: u16,
}

impl Default for SeamWeights {
    fn default() -> Self {
        Self { color: 280, gradient: 180, height: 100, vector_normal: 120, roughness: 80, structure_cut: 240 }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraphCutSettings {
    pub axes: ClosureAxes,
    /// Width of each boundary search band. Boundary bands may not overlap.
    pub overlap_pixels: u16,
    /// Explicit bound on candidate seam positions inside the overlap.
    pub max_search_positions: u16,
    /// Penalty for moving the seam by one pixel between adjacent rows/columns.
    pub continuity_milli: u16,
    /// A selected seam above this normalized multi-channel cost is rejected.
    pub max_accepted_seam_cost_milli: u16,
    pub weights: SeamWeights,
    pub max_working_bytes: u64,
    pub max_operations: u64,
}

impl Default for GraphCutSettings {
    fn default() -> Self {
        Self {
            axes: ClosureAxes::XY,
            overlap_pixels: 16,
            max_search_positions: 64,
            continuity_milli: 25,
            max_accepted_seam_cost_milli: 450,
            weights: SeamWeights::default(),
            max_working_bytes: 1_073_741_824,
            max_operations: 1_000_000_000,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DomainRequest {
    pub source: Arc<DelitPreparedExemplar>,
    /// Digest of the complete Stage 3/4 prepared-source lineage.
    pub prepared_source_digest: ContentDigest,
    /// Typed Stage 7 authority. No untyped analysis payload is accepted.
    pub analysis: Arc<FeatureFieldReport>,
    /// The exact Stage 6 authority consumed by Stage 7. Physical quilting sizes
    /// are legal only when this report contains world-accurate scale evidence.
    pub scale_orientation: Arc<ScaleOrientationReport>,
    pub route: DomainRoute,
    pub direct_boundary_threshold_milli: u16,
    pub graph_cut: GraphCutSettings,
    pub quilting: QuiltingSettings,
    pub patch_match: PatchMatchSettings,
    pub seed: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceCoordinate { pub x: u32, pub y: u32 }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeightedSource {
    pub coordinate: SourceCoordinate,
    /// Normalized contribution. The active entries sum to one within float tolerance.
    pub weight: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CorrespondenceSample {
    pub sources: [Option<WeightedSource>; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub enum CorrespondenceField {
    Identity { width: u32, height: u32 },
    Registered(ImagePlane<CorrespondenceSample>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomainOperation {
    PassThrough,
    /// The same operation drives all channels: weights blend continuous data while the
    /// selected contribution is used for categorical IDs and binary masks.
    SeamCompose { contribution_count: u8, categorical_selected_contribution: u8 },
    /// A discrete selection from the shared quilting correspondence. All channels
    /// use this same patch and seam decision.
    QuiltPatch { patch_index: u32 },
    /// A sample selected by the one registered PatchMatch nearest-neighbor field.
    PatchMatch { preserved: bool },
    /// A deterministic sample from the classical statistical route.
    StatisticalSample,
    /// A value evaluated from one registered fitted procedural coordinate field.
    ProceduralSample,
    /// A value returned through the validated local learned-provider boundary.
    LearnedSample,
}

#[derive(Clone, Debug, PartialEq)]
pub enum OperationField {
    Identity { width: u32, height: u32 },
    Registered(ImagePlane<DomainOperation>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProvenanceValue { Original, SeamComposed, ClassicalSynthesized, ProceduralEstimated, LearnedEstimated }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SeamAxis { X, Y }

#[derive(Clone, Debug, PartialEq)]
pub struct SelectedSeam {
    pub axis: SeamAxis,
    /// X coordinate per row for X closure; Y coordinate per column for Y closure.
    pub positions: Vec<u16>,
    pub normalized_cost_milli: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomainQaView {
    RegisteredChannels,
    SeamCost,
    SeamPath,
    BoundaryDifference,
    Correspondence,
    Operations,
    Validity,
    Provenance,
    Quilting,
    SourceUsage,
    NearestNeighborField,
    Coherence,
    RouteComparison,
    Applicability,
    Scale,
    Determinism,
    CacheProvenance,
    LearnedProvider,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PassThroughEvidence {
    pub horizontal_boundary_cost_milli: u16,
    pub vertical_boundary_cost_milli: u16,
    pub accepted_threshold_milli: u16,
    pub exact_source_coordinates: bool,
    pub no_resampling: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainDiagnostics {
    pub selected_route: DomainRoute,
    pub cache_key: ContentDigest,
    pub available_seam_terms: BTreeSet<SeamTerm>,
    pub normalized_weight_milli: BTreeMap<SeamTerm, u16>,
    pub pass_through: Option<PassThroughEvidence>,
    pub seams: Vec<SelectedSeamSummary>,
    pub boundary_cost_before_milli: (u16, u16),
    pub boundary_cost_after_milli: (u16, u16),
    pub messages: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectedSeamSummary {
    pub axis: SeamAxis,
    pub sample_count: u32,
    pub minimum_position: u16,
    pub maximum_position: u16,
    pub normalized_cost_milli: u16,
}

#[derive(Clone, Debug, PartialEq)]
enum DomainChannelStorage {
    /// Exact Stage 4 buffers are shared, not cloned or resampled.
    Direct(Arc<DelitPreparedExemplar>),
    Generated(Vec<PreparedExemplarChannel>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreparedMaterialDomain {
    pub cache_key: ContentDigest,
    pub prepared_source_digest: ContentDigest,
    pub analysis_digest: ContentDigest,
    pub route: DomainRoute,
    pub width: u32,
    pub height: u32,
    channels: DomainChannelStorage,
    pub correspondence: CorrespondenceField,
    pub operations: OperationField,
    pub validity: ImagePlane<MaskValue>,
    pub provenance: ImagePlane<ProvenanceValue>,
    pub seams: Vec<SelectedSeam>,
    pub quilting: Option<QuiltingDiagnostics>,
    pub patch_match: Option<PatchMatchDiagnostics>,
    pub diagnostics: DomainDiagnostics,
    pub qa_views: Vec<DomainQaView>,
    pub stage_result: StageResult,
}

#[derive(Clone, Copy, Debug)]
pub enum RegisteredChannelRef<'a> {
    BaseColor(&'a ImagePlane<LinearColor>),
    Scalar(MaterialChannelRole, &'a ImagePlane<LinearScalar>),
    Normal(&'a ImagePlane<TangentNormal>),
    MaterialId(&'a ImagePlane<CategoryId>),
    Mask(MaterialChannelRole, &'a ImagePlane<MaskValue>),
}

impl PreparedMaterialDomain {
    #[must_use]
    pub fn registered_channels(&self) -> &[PreparedExemplarChannel] {
        match &self.channels {
            DomainChannelStorage::Direct(source) => &source.channels,
            DomainChannelStorage::Generated(channels) => channels,
        }
    }

    #[must_use]
    pub fn channel(&self, role: MaterialChannelRole) -> Option<RegisteredChannelRef<'_>> {
        self.registered_channels().iter().find_map(|channel| match channel {
            PreparedExemplarChannel::BaseColor { plane, .. } if role == MaterialChannelRole::BaseColor => Some(RegisteredChannelRef::BaseColor(plane)),
            PreparedExemplarChannel::Scalar { role: found, plane } if *found == role => Some(RegisteredChannelRef::Scalar(*found, plane)),
            PreparedExemplarChannel::Normal { plane, .. } if role == MaterialChannelRole::Normal => Some(RegisteredChannelRef::Normal(plane)),
            PreparedExemplarChannel::MaterialId { plane } if role == MaterialChannelRole::MaterialId => Some(RegisteredChannelRef::MaterialId(plane)),
            PreparedExemplarChannel::Mask { role: found, plane } if *found == role => Some(RegisteredChannelRef::Mask(*found, plane)),
            _ => None,
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct MaterialDomainCache { entries: BTreeMap<ContentDigest, PreparedMaterialDomain> }

impl MaterialDomainCache {
    #[must_use] pub fn get(&self, key: &ContentDigest) -> Option<&PreparedMaterialDomain> { self.entries.get(key) }
    pub fn insert_complete(&mut self, domain: PreparedMaterialDomain) {
        if self.entries.len() >= MAX_CACHE_ENTRIES && !self.entries.contains_key(&domain.cache_key)
            && let Some(oldest) = self.entries.keys().next().cloned() { self.entries.remove(&oldest); }
        self.entries.insert(domain.cache_key.clone(), domain);
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum DomainError {
    #[error("Stage 8A settings are outside bounded ranges")]
    InvalidSettings,
    #[error("Stage 8A source and Stage 7 fields are not registered")]
    RegistrationDrift,
    #[error("Stage 8A graph-cut {axis:?} overlap requires {required} pixels but only {available} are available")]
    InsufficientOverlap { axis: SeamAxis, required: u32, available: u32 },
    #[error("Stage 8A graph-cut {axis:?} seam cost {cost_milli}/1000 exceeds the accepted maximum {maximum_milli}/1000")]
    UnacceptableSeam { axis: SeamAxis, cost_milli: u16, maximum_milli: u16 },
    #[error("Stage 8A work exceeds the declared byte or operation limit")]
    ResourceLimitExceeded,
    #[error("Stage 8A was cancelled")]
    Cancelled,
    #[error("Stage 8A could not construct a registered plane")]
    PlaneConstruction,
    #[error("Stage 8B quilting is semantically incompatible: {reason}")]
    IncompatibleQuilting { reason: String },
    #[error("Stage 8B quilting found no source patch that avoids unusable pixels ({rejected_candidates} candidates rejected)")]
    UnusableQuiltingSource { rejected_candidates: u32 },
    #[error("Stage 8B quilting cannot cover the requested output with the bounded patch/iteration limits")]
    QuiltingCoverageFailed,
    #[error("Stage 8B quilting seam for patch {patch_index} on {axis:?} costs {cost_milli}/1000, above {maximum_milli}/1000")]
    UnacceptableQuiltingSeam { patch_index: u32, axis: SeamAxis, cost_milli: u16, maximum_milli: u16 },
    #[error("Stage 8B quilting boundary error {horizontal_milli}/1000 x {vertical_milli}/1000 exceeds {maximum_milli}/1000")]
    UnacceptableQuiltingBoundary { horizontal_milli: u16, vertical_milli: u16, maximum_milli: u16 },
    #[error("Stage 8C PatchMatch is semantically incompatible: {reason}")]
    IncompatiblePatchMatch { reason: String },
    #[error("Stage 8C PatchMatch found no usable source patch ({rejected_candidates} candidates rejected)")]
    UnusablePatchMatchSource { rejected_candidates: u64 },
    #[error("Stage 8C PatchMatch left {incomplete_pixels} requested pixels incomplete")]
    IncompletePatchMatch { incomplete_pixels: u64 },
    #[error("Stage 8C PatchMatch did not converge after {iterations} iterations ({changed_pixels} pixels still changing)")]
    PatchMatchNonConverged { iterations: u16, changed_pixels: u64 },
    #[error("Stage 8C PatchMatch boundary error {horizontal_milli}/1000 x {vertical_milli}/1000 exceeds {maximum_milli}/1000")]
    UnacceptablePatchMatchBoundary { horizontal_milli: u16, vertical_milli: u16, maximum_milli: u16 },
    #[error("statistical, procedural, and learned Stage 8 routes are reachable only through the authoritative material-domain router")]
    RouterRequired,
}

impl DomainError {
    #[must_use]
    pub fn stage_result(&self) -> StageResult {
        let (code, recovery_choices) = match self {
            Self::Cancelled => (DiagnosticCode::Cancelled, vec![RecoveryChoice::AdjustSettings]),
            Self::ResourceLimitExceeded => (DiagnosticCode::ResourceLimitExceeded, vec![RecoveryChoice::AdjustSettings]),
            Self::InsufficientOverlap { .. } => (DiagnosticCode::InsufficientInput, vec![RecoveryChoice::ChooseAnotherSource, RecoveryChoice::AdjustSettings]),
            Self::UnacceptableSeam { .. } => (DiagnosticCode::InsufficientInput,
                vec![RecoveryChoice::UseSynthesis, RecoveryChoice::ChooseAnotherSource, RecoveryChoice::AdjustSettings]),
            Self::IncompatibleQuilting { .. } | Self::UnusableQuiltingSource { .. } | Self::QuiltingCoverageFailed
            | Self::UnacceptableQuiltingSeam { .. } | Self::UnacceptableQuiltingBoundary { .. } =>
                (DiagnosticCode::InsufficientInput, vec![RecoveryChoice::ChooseAnotherSource, RecoveryChoice::AdjustSettings]),
            Self::IncompatiblePatchMatch { .. } | Self::UnusablePatchMatchSource { .. }
            | Self::IncompletePatchMatch { .. } | Self::PatchMatchNonConverged { .. } =>
                (DiagnosticCode::InsufficientInput, vec![RecoveryChoice::ChooseAnotherSource, RecoveryChoice::AdjustSettings]),
            Self::UnacceptablePatchMatchBoundary { .. } =>
                (DiagnosticCode::InsufficientInput, vec![RecoveryChoice::ChooseAnotherSource, RecoveryChoice::AdjustSettings]),
            Self::InvalidSettings | Self::RegistrationDrift | Self::PlaneConstruction | Self::RouterRequired => (DiagnosticCode::MalformedInput, vec![RecoveryChoice::AdjustSettings]),
        };
        StageResult::FailedWithRecovery {
            reason: CompilationDiagnostic { code, stage: Some(8), message: self.to_string(), context: BTreeMap::new() },
            recovery_choices,
        }
    }
}

pub fn prepare_material_domain(
    request: &DomainRequest,
    cache: &mut MaterialDomainCache,
    cancellation: &RenderCancellationToken,
) -> Result<PreparedMaterialDomain, DomainError> {
    validate_request(request)?;
    check_cancel(cancellation)?;
    let selected = select_route(request);
    let key = domain_cache_key(request, selected);
    if let Some(domain) = cache.get(&key) { return Ok(domain.clone()); }
    let domain = match selected {
        DomainRoute::DirectSource => direct_domain(request, key, cancellation)?,
        DomainRoute::GraphCutPeriodicClosure => graph_cut_domain(request, key, cancellation)?,
        DomainRoute::TextureQuilting => quilted_domain(request, key, cancellation)?,
        DomainRoute::PatchMatch => patchmatch_domain(request, key, cancellation)?,
        DomainRoute::StatisticalSynthesis | DomainRoute::ProceduralReconstruction
        | DomainRoute::LearnedProvider => return Err(DomainError::RouterRequired),
        DomainRoute::Auto => unreachable!("route selection resolves Auto"),
    };
    cache.insert_complete(domain.clone());
    Ok(domain)
}

#[must_use]
pub fn domain_cache_key(request: &DomainRequest, selected: DomainRoute) -> ContentDigest {
    ContentDigest::sha256(format!(
        "{}|{}|{}|{}|{}|{:?}|{}|{:?}|{}|{}|{}|{}|{:?}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{:?}|{:?}|{}",
        STAGE_08A_ALGORITHM_VERSION, STAGE_08B_ALGORITHM_VERSION, STAGE_08C_ALGORITHM_VERSION, request.prepared_source_digest.0,
        request.analysis.cache_key.0,
        selected, request.seed, request.graph_cut.axes, request.graph_cut.overlap_pixels,
        request.graph_cut.max_search_positions, request.graph_cut.continuity_milli,
        request.graph_cut.max_accepted_seam_cost_milli, request.graph_cut.weights,
        request.graph_cut.max_working_bytes, request.graph_cut.max_operations,
        request.direct_boundary_threshold_milli, request.source.exemplar_id,
        request.source.base_color().width(), request.source.base_color().height(),
        request.analysis.registration_digest.0, request.source.channels.len(),
        request.analysis.seamability.horizontal_cost_milli, request.analysis.seamability.vertical_cost_milli,
        request.scale_orientation.cache_key.0,
        request.scale_orientation.scale,
        request.quilting,
        patchmatch_cache_fragment(&request.patch_match),
    ).as_bytes())
}

fn select_route(request: &DomainRequest) -> DomainRoute {
    match request.route {
        DomainRoute::Auto if request.analysis.seamability.horizontal_cost_milli <= request.direct_boundary_threshold_milli
            && request.analysis.seamability.vertical_cost_milli <= request.direct_boundary_threshold_milli => DomainRoute::DirectSource,
        DomainRoute::Auto => DomainRoute::GraphCutPeriodicClosure,
        route => route,
    }
}

fn validate_request(request: &DomainRequest) -> Result<(), DomainError> {
    let settings = request.graph_cut;
    if request.direct_boundary_threshold_milli > 1000 || settings.overlap_pixels < 2
        || settings.max_search_positions < 2 || settings.max_search_positions > 4096
        || settings.continuity_milli > 1000 || settings.max_accepted_seam_cost_milli > 1000
        || settings.max_working_bytes == 0 || settings.max_operations == 0
        || weight_sum(settings.weights) == 0 { return Err(DomainError::InvalidSettings); }
    let (width, height) = (request.source.base_color().width(), request.source.base_color().height());
    let consumed_fields = consumed_level_zero_fields(request);
    if request.prepared_source_digest != request.source.prepared_source_digest
        || request.prepared_source_digest != request.analysis.prepared_source_digest
        || request.analysis.registration_digest != expected_stage_six_registration_digest(request)
        || request.source.channels.iter().any(|channel| channel.dimensions() != (width, height))
        || request.analysis.qa.level_dimensions.first().copied() != Some((width, height))
        || consumed_fields.len() != 8
        || consumed_fields.iter().any(|plane| (plane.width(), plane.height()) != (width, height))
    { return Err(DomainError::RegistrationDrift); }
    Ok(())
}

fn expected_stage_six_registration_digest(request: &DomainRequest) -> ContentDigest {
    let (width, height) = (request.source.base_color().width(), request.source.base_color().height());
    ContentDigest::sha256(format!("{}|{}|{}|{}|{}|{}", request.source.prepared_source_digest.0,
        request.source.exemplar_id, width, height, request.scale_orientation.cache_key.0,
        request.source.channels.iter().map(|channel| format!("{:?}", channel.role())).collect::<Vec<_>>().join(","))
        .as_bytes())
}

fn consumed_level_zero_fields(request: &DomainRequest) -> Vec<&ImagePlane<LinearScalar>> {
    let fields = &request.analysis.structure;
    [&fields.edge, &fields.line, &fields.boundary, &fields.grid, &fields.fiber, &fields.intersection,
        &request.analysis.seamability.confidence, &request.analysis.usability.confidence]
        .into_iter().filter_map(|pyramid| pyramid.level(0)).collect()
}

fn direct_domain(
    request: &DomainRequest, key: ContentDigest, cancellation: &RenderCancellationToken,
) -> Result<PreparedMaterialDomain, DomainError> {
    let (width, height) = (request.source.base_color().width(), request.source.base_color().height());
    direct_preflight(request, width, height)?;
    let validity = direct_validity(request, width, height, cancellation)?;
    check_cancel(cancellation)?;
    let provenance = plane(width, height, request.source.base_color().tile_edge(), vec![ProvenanceValue::Original; pixel_count(width, height)?])?;
    let pass = PassThroughEvidence {
        horizontal_boundary_cost_milli: request.analysis.seamability.horizontal_cost_milli,
        vertical_boundary_cost_milli: request.analysis.seamability.vertical_cost_milli,
        accepted_threshold_milli: request.direct_boundary_threshold_milli,
        exact_source_coordinates: true,
        no_resampling: true,
    };
    let diagnostics = DomainDiagnostics {
        selected_route: DomainRoute::DirectSource,
        cache_key: key.clone(),
        available_seam_terms: request.analysis.seamability.available_terms.clone(),
        normalized_weight_milli: normalized_weights(request),
        pass_through: Some(pass),
        seams: Vec::new(),
        boundary_cost_before_milli: (request.analysis.seamability.horizontal_cost_milli, request.analysis.seamability.vertical_cost_milli),
        boundary_cost_after_milli: (request.analysis.seamability.horizontal_cost_milli, request.analysis.seamability.vertical_cost_milli),
        messages: vec!["Source evidence satisfied the declared direct-domain boundary threshold; source pixels were shared without resampling.".into()],
    };
    Ok(PreparedMaterialDomain {
        cache_key: key,
        prepared_source_digest: request.prepared_source_digest.clone(),
        analysis_digest: request.analysis.cache_key.clone(),
        route: DomainRoute::DirectSource,
        width, height,
        channels: DomainChannelStorage::Direct(Arc::clone(&request.source)),
        correspondence: CorrespondenceField::Identity { width, height },
        operations: OperationField::Identity { width, height },
        validity,
        provenance,
        seams: Vec::new(),
        quilting: None,
        patch_match: None,
        diagnostics,
        qa_views: vec![DomainQaView::RegisteredChannels, DomainQaView::BoundaryDifference,
            DomainQaView::Correspondence, DomainQaView::Validity, DomainQaView::Provenance],
        stage_result: StageResult::PassThrough { reason: "already tileable under measured multi-channel boundary evidence; exact prepared source retained".into() },
    })
}

fn graph_cut_domain(
    request: &DomainRequest,
    key: ContentDigest,
    cancellation: &RenderCancellationToken,
) -> Result<PreparedMaterialDomain, DomainError> {
    let source = request.source.base_color();
    let (width, height) = (source.width(), source.height());
    let overlap = u32::from(request.graph_cut.overlap_pixels);
    if request.graph_cut.axes.includes_x() && width < overlap.saturating_mul(2) {
        return Err(DomainError::InsufficientOverlap { axis: SeamAxis::X, required: overlap * 2, available: width });
    }
    if request.graph_cut.axes.includes_y() && height < overlap.saturating_mul(2) {
        return Err(DomainError::InsufficientOverlap { axis: SeamAxis::Y, required: overlap * 2, available: height });
    }
    preflight(request, width, height)?;
    let mut seams = Vec::new();
    if request.graph_cut.axes.includes_x() {
        let seam = solve_seam(request, SeamAxis::X, cancellation)?;
        ensure_acceptable_seam(request, &seam)?;
        seams.push(seam);
    }
    if request.graph_cut.axes.includes_y() {
        let seam = solve_seam(request, SeamAxis::Y, cancellation)?;
        ensure_acceptable_seam(request, &seam)?;
        seams.push(seam);
    }
    check_cancel(cancellation)?;
    let samples = build_correspondence(width, height, overlap, &seams, cancellation)?;
    let generated = compose_channels(request, &samples, width, height, cancellation)?;
    let validity = source_validity(request, &samples, width, height, cancellation)?;
    let provenance_values = samples.iter().map(|sample| if active_sources(sample) == 1 { ProvenanceValue::Original } else { ProvenanceValue::SeamComposed }).collect();
    let tile_edge = source.tile_edge();
    let provenance = plane(width, height, tile_edge, provenance_values)?;
    let operations = samples.iter().map(|sample| {
        let count = active_sources(sample) as u8;
        if count == 1 { DomainOperation::PassThrough } else { DomainOperation::SeamCompose {
            contribution_count: count,
            categorical_selected_contribution: categorical_source_index(sample) as u8,
        } }
    }).collect();
    let correspondence = CorrespondenceField::Registered(plane(width, height, tile_edge, samples)?);
    let operations = OperationField::Registered(plane(width, height, tile_edge, operations)?);
    let after = measured_boundary_cost(request, &generated, width, height, &seams);
    let seam_summaries = seams.iter().map(seam_summary).collect();
    let diagnostics = DomainDiagnostics {
        selected_route: DomainRoute::GraphCutPeriodicClosure,
        cache_key: key.clone(),
        available_seam_terms: request.analysis.seamability.available_terms.clone(),
        normalized_weight_milli: normalized_weights(request),
        pass_through: None,
        seams: seam_summaries,
        boundary_cost_before_milli: (request.analysis.seamability.horizontal_cost_milli, request.analysis.seamability.vertical_cost_milli),
        boundary_cost_after_milli: after,
        messages: vec!["A single deterministic seam/correspondence field composed every registered channel; categorical channels used discrete selection.".into()],
    };
    Ok(PreparedMaterialDomain {
        cache_key: key,
        prepared_source_digest: request.prepared_source_digest.clone(),
        analysis_digest: request.analysis.cache_key.clone(),
        route: DomainRoute::GraphCutPeriodicClosure,
        width, height,
        channels: DomainChannelStorage::Generated(generated),
        correspondence,
        operations,
        validity,
        provenance,
        seams,
        quilting: None,
        patch_match: None,
        diagnostics,
        qa_views: vec![DomainQaView::RegisteredChannels, DomainQaView::SeamCost, DomainQaView::SeamPath,
            DomainQaView::BoundaryDifference, DomainQaView::Correspondence, DomainQaView::Operations,
            DomainQaView::Validity, DomainQaView::Provenance],
        stage_result: StageResult::Executed {
            algorithm: AlgorithmProvenance { algorithm_id: STAGE_08A_GRAPHCUT_ALGORITHM_ID.into(), version: STAGE_08A_ALGORITHM_VERSION.into() },
            settings_hash: settings_hash(request),
            diagnostics: Vec::new(),
        },
    })
}

fn ensure_acceptable_seam(request: &DomainRequest, seam: &SelectedSeam) -> Result<(), DomainError> {
    let maximum_milli = request.graph_cut.max_accepted_seam_cost_milli;
    if seam.normalized_cost_milli > maximum_milli {
        Err(DomainError::UnacceptableSeam { axis: seam.axis, cost_milli: seam.normalized_cost_milli, maximum_milli })
    } else { Ok(()) }
}

fn solve_seam(request: &DomainRequest, axis: SeamAxis, cancellation: &RenderCancellationToken) -> Result<SelectedSeam, DomainError> {
    let (width, height) = (request.source.base_color().width(), request.source.base_color().height());
    let overlap = u32::from(request.graph_cut.overlap_pixels);
    let candidates = overlap.min(u32::from(request.graph_cut.max_search_positions));
    let length = if axis == SeamAxis::X { height } else { width };
    let mut previous = vec![0.0_f64; candidates as usize];
    let mut back = vec![vec![0_u16; candidates as usize]; length as usize];
    for position in 0..candidates {
        previous[position as usize] = seam_pixel_cost(request, axis, 0, position);
    }
    for line in 1..length {
        if line % 32 == 0 { check_cancel(cancellation)?; }
        let mut current = vec![f64::INFINITY; candidates as usize];
        for position in 0..candidates {
            let mut best = f64::INFINITY;
            let mut best_previous = 0;
            let start = position.saturating_sub(1);
            let end = (position + 1).min(candidates - 1);
            for prior in start..=end {
                let movement = position.abs_diff(prior) as f64 * f64::from(request.graph_cut.continuity_milli) / 1000.0;
                let candidate = previous[prior as usize] + movement;
                if candidate < best || (candidate == best && prior < best_previous) {
                    best = candidate; best_previous = prior;
                }
            }
            current[position as usize] = best + seam_pixel_cost(request, axis, line, position);
            back[line as usize][position as usize] = best_previous as u16;
        }
        previous = current;
    }
    let mut ending = 0_u32;
    for position in 1..candidates {
        if previous[position as usize] < previous[ending as usize] { ending = position; }
    }
    let normalized = previous[ending as usize] / f64::from(length.max(1));
    let mut positions = vec![0_u16; length as usize];
    positions[length as usize - 1] = ending as u16;
    for line in (1..length).rev() {
        positions[line as usize - 1] = back[line as usize][usize::from(positions[line as usize])];
    }
    Ok(SelectedSeam { axis, positions, normalized_cost_milli: score(normalized as f32) })
}

fn seam_pixel_cost(request: &DomainRequest, axis: SeamAxis, line: u32, position: u32) -> f64 {
    let base = request.source.base_color();
    let (width, height) = (base.width(), base.height());
    let (a, b) = match axis {
        SeamAxis::X => ((position, line), (width - 1 - position, line)),
        SeamAxis::Y => ((line, position), (line, height - 1 - position)),
    };
    let weights = normalized_weight_f64(request);
    let ca = base.pixel(a.0, a.1).rgb; let cb = base.pixel(b.0, b.1).rgb;
    let color = (0..3).map(|i| f64::from((ca[i] - cb[i]).abs())).sum::<f64>() / 3.0;
    let gradient = (luminance_gradient(base, a.0, a.1) - luminance_gradient(base, b.0, b.1)).abs() as f64;
    let height_cost = scalar_difference(request, MaterialChannelRole::Height, a, b);
    let roughness = scalar_difference(request, MaterialChannelRole::Roughness, a, b);
    let normal = normal_difference(request, a, b);
    let structure = structure_penalty(request, a).max(structure_penalty(request, b));
    weights[0] * color + weights[1] * gradient + weights[2] * height_cost
        + weights[3] * normal + weights[4] * roughness + weights[5] * structure
}

fn normalized_weight_f64(request: &DomainRequest) -> [f64; 6] {
    let w = request.graph_cut.weights;
    let available = &request.analysis.seamability.available_terms;
    let raw = [w.color, w.gradient,
        if available.contains(&SeamTerm::Height) { w.height } else { 0 },
        if available.contains(&SeamTerm::VectorNormal) { w.vector_normal } else { 0 },
        if available.contains(&SeamTerm::Roughness) { w.roughness } else { 0 }, w.structure_cut];
    let sum = raw.iter().map(|value| u64::from(*value)).sum::<u64>().max(1) as f64;
    raw.map(|value| f64::from(value) / sum)
}

fn normalized_weights(request: &DomainRequest) -> BTreeMap<SeamTerm, u16> {
    let normalized = normalized_weight_f64(request);
    [SeamTerm::Color, SeamTerm::Gradient, SeamTerm::Height, SeamTerm::VectorNormal,
        SeamTerm::Roughness, SeamTerm::StructuralCrossing].into_iter().zip(normalized)
        .filter(|(_, weight)| *weight > 0.0).map(|(term, weight)| (term, score(weight as f32))).collect()
}

fn scalar_difference(request: &DomainRequest, role: MaterialChannelRole, a: (u32, u32), b: (u32, u32)) -> f64 {
    request.source.channels.iter().find_map(|channel| match channel {
        PreparedExemplarChannel::Scalar { role: found, plane } if *found == role => Some(f64::from((plane.pixel(a.0, a.1).0 - plane.pixel(b.0, b.1).0).abs())),
        _ => None,
    }).unwrap_or(0.0)
}

fn normal_difference(request: &DomainRequest, a: (u32, u32), b: (u32, u32)) -> f64 {
    request.source.channels.iter().find_map(|channel| match channel {
        PreparedExemplarChannel::Normal { plane, .. } => {
            let na = plane.pixel(a.0, a.1).xyz; let nb = plane.pixel(b.0, b.1).xyz;
            Some(f64::from((1.0 - dot(normalize3(na), normalize3(nb))).clamp(0.0, 2.0)) * 0.5)
        }
        _ => None,
    }).unwrap_or(0.0)
}

fn structure_penalty(request: &DomainRequest, point: (u32, u32)) -> f64 {
    let fields = &request.analysis.structure;
    [&fields.edge, &fields.line, &fields.boundary, &fields.grid, &fields.fiber, &fields.intersection]
        .iter().filter_map(|pyramid| pyramid.level(0)).map(|plane| f64::from(plane.pixel(point.0, point.1).0))
        .fold(0.0_f64, f64::max)
}

fn build_correspondence(
    width: u32, height: u32, overlap: u32, seams: &[SelectedSeam], cancellation: &RenderCancellationToken,
) -> Result<Vec<CorrespondenceSample>, DomainError> {
    let x_seam = seams.iter().find(|seam| seam.axis == SeamAxis::X);
    let y_seam = seams.iter().find(|seam| seam.axis == SeamAxis::Y);
    let mut result = Vec::with_capacity(pixel_count(width, height)?);
    for y in 0..height {
        if y % 32 == 0 { check_cancel(cancellation)?; }
        for x in 0..width {
            // In XY closure, opposite boundary pixels must also index the orthogonal seam
            // through the same canonical mirrored coordinate. This closes the four corners
            // without selecting a second seam or a channel-specific correspondence.
            let canonical_x = canonical_boundary_coordinate(x, width, overlap, x_seam.is_some());
            let canonical_y = canonical_boundary_coordinate(y, height, overlap, y_seam.is_some());
            let x_options = axis_options(x, canonical_y, width, overlap, x_seam);
            let y_options = axis_options(y, canonical_x, height, overlap, y_seam);
            let mut sources = [None; 4]; let mut index = 0;
            for (sx, wx) in x_options { for (sy, wy) in &y_options {
                let weight = wx * *wy;
                if weight > 0.0 {
                    sources[index] = Some(WeightedSource { coordinate: SourceCoordinate { x: sx, y: *sy }, weight });
                    index += 1;
                }
            }}
            result.push(CorrespondenceSample { sources });
        }
    }
    Ok(result)
}

fn axis_options(
    coordinate: u32, orthogonal: u32, dimension: u32, overlap: u32,
    seam: Option<&SelectedSeam>,
) -> Vec<(u32, f32)> {
    let Some(seam) = seam else { return vec![(coordinate, 1.0)]; };
    let in_boundary = coordinate < overlap || coordinate >= dimension - overlap;
    if !in_boundary { return vec![(coordinate, 1.0)]; }
    let distance = if coordinate < overlap { coordinate } else { dimension - 1 - coordinate };
    let left = distance; let right = dimension - 1 - distance;
    let cut = u32::from(seam.positions[orthogonal as usize]);
    let right_weight = if distance < cut { 0.0 } else if distance == cut { 0.5 } else { 1.0 };
    if right_weight == 0.0 { vec![(left, 1.0)] }
    else if right_weight == 1.0 { vec![(right, 1.0)] }
    else { vec![(left, 0.5), (right, 0.5)] }
}

fn canonical_boundary_coordinate(coordinate: u32, dimension: u32, overlap: u32, closed: bool) -> u32 {
    if closed && coordinate >= dimension - overlap { dimension - 1 - coordinate } else { coordinate }
}

fn compose_channels(
    request: &DomainRequest, samples: &[CorrespondenceSample], width: u32, height: u32,
    cancellation: &RenderCancellationToken,
) -> Result<Vec<PreparedExemplarChannel>, DomainError> {
    let mut output = Vec::with_capacity(request.source.channels.len());
    let tile_edge = request.source.base_color().tile_edge();
    for channel in &request.source.channels {
        check_cancel(cancellation)?;
        output.push(match channel {
            PreparedExemplarChannel::BaseColor { plane: source, alpha_mode } => {
                let values = samples.iter().map(|sample| blend_color(source, sample)).collect();
                PreparedExemplarChannel::BaseColor { plane: plane(width, height, tile_edge, values)?, alpha_mode: *alpha_mode }
            }
            PreparedExemplarChannel::Scalar { role, plane: source } => {
                let values = samples.iter().map(|sample| LinearScalar(blend_scalar(source, sample))).collect();
                PreparedExemplarChannel::Scalar { role: *role, plane: plane(width, height, tile_edge, values)? }
            }
            PreparedExemplarChannel::Normal { plane: source, source_convention, canonical_convention, alpha_policy } => {
                let values = samples.iter().map(|sample| blend_normal(source, sample)).collect();
                PreparedExemplarChannel::Normal { plane: plane(width, height, tile_edge, values)?, source_convention: *source_convention,
                    canonical_convention: *canonical_convention, alpha_policy: *alpha_policy }
            }
            PreparedExemplarChannel::MaterialId { plane: source } => {
                let values = samples.iter().map(|sample| *source.pixel(categorical_source(sample).x, categorical_source(sample).y)).collect();
                PreparedExemplarChannel::MaterialId { plane: plane(width, height, tile_edge, values)? }
            }
            PreparedExemplarChannel::Mask { role, plane: source } => {
                let values = if *role == MaterialChannelRole::Opacity {
                    samples.iter().map(|sample| MaskValue(blend_mask(source, sample))).collect()
                } else {
                    samples.iter().map(|sample| *source.pixel(categorical_source(sample).x, categorical_source(sample).y)).collect()
                };
                PreparedExemplarChannel::Mask { role: *role, plane: plane(width, height, tile_edge, values)? }
            }
        });
    }
    Ok(output)
}

fn blend_color(source: &ImagePlane<LinearColor>, sample: &CorrespondenceSample) -> LinearColor {
    let mut rgb = [0.0; 3]; let mut alpha = 0.0;
    for contribution in sample.sources.into_iter().flatten() {
        let value = source.pixel(contribution.coordinate.x, contribution.coordinate.y);
        for (out, input) in rgb.iter_mut().zip(value.rgb) { *out += input * contribution.weight; }
        alpha += value.alpha * contribution.weight;
    }
    LinearColor { rgb, alpha }
}

fn blend_scalar(source: &ImagePlane<LinearScalar>, sample: &CorrespondenceSample) -> f32 {
    sample.sources.into_iter().flatten().map(|c| source.pixel(c.coordinate.x, c.coordinate.y).0 * c.weight).sum()
}

fn blend_mask(source: &ImagePlane<MaskValue>, sample: &CorrespondenceSample) -> f32 {
    sample.sources.into_iter().flatten()
        .map(|c| source.pixel(c.coordinate.x, c.coordinate.y).0 * c.weight).sum::<f32>().clamp(0.0, 1.0)
}

fn blend_normal(source: &ImagePlane<TangentNormal>, sample: &CorrespondenceSample) -> TangentNormal {
    let mut xyz = [0.0; 3]; let mut alpha = 0.0;
    for contribution in sample.sources.into_iter().flatten() {
        let value = source.pixel(contribution.coordinate.x, contribution.coordinate.y);
        for (out, input) in xyz.iter_mut().zip(value.xyz) { *out += input * contribution.weight; }
        alpha += value.alpha * contribution.weight;
    }
    TangentNormal { xyz: normalize3(xyz), alpha }
}

fn categorical_source(sample: &CorrespondenceSample) -> SourceCoordinate {
    sample.sources[categorical_source_index(sample)].expect("selected contribution exists").coordinate
}

fn categorical_source_index(sample: &CorrespondenceSample) -> usize {
    sample.sources.into_iter().flatten().enumerate().max_by(|(ia, a), (ib, b)| {
        a.weight.total_cmp(&b.weight).then_with(|| ib.cmp(ia))
    }).map(|(index, _)| index).expect("every correspondence has a source")
}

fn source_validity(
    request: &DomainRequest, samples: &[CorrespondenceSample], width: u32, height: u32,
    cancellation: &RenderCancellationToken,
) -> Result<ImagePlane<MaskValue>, DomainError> {
    let usability = request.analysis.usability.confidence.level(0).ok_or(DomainError::RegistrationDrift)?;
    let mut values = Vec::with_capacity(pixel_count(width, height)?);
    for y in 0..height {
        if y % 32 == 0 { check_cancel(cancellation)?; }
        for x in 0..width {
            let sample = &samples[(y * width + x) as usize];
            let mut value = 0.0;
            for source in sample.sources.into_iter().flatten() {
                let mut confidence = usability.pixel(source.coordinate.x, source.coordinate.y).0;
                if let Some(coverage) = &request.source.coverage { confidence *= coverage.pixel(source.coordinate.x, source.coordinate.y).0; }
                value += confidence * source.weight;
            }
            values.push(MaskValue(value.clamp(0.0, 1.0)));
        }
    }
    plane(width, height, request.source.base_color().tile_edge(), values)
}

fn direct_validity(
    request: &DomainRequest, width: u32, height: u32, cancellation: &RenderCancellationToken,
) -> Result<ImagePlane<MaskValue>, DomainError> {
    let usability = request.analysis.usability.confidence.level(0).ok_or(DomainError::RegistrationDrift)?;
    let mut values = Vec::with_capacity(pixel_count(width, height)?);
    for y in 0..height {
        if y % 32 == 0 { check_cancel(cancellation)?; }
        for x in 0..width {
            let mut confidence = usability.pixel(x, y).0;
            if let Some(coverage) = &request.source.coverage { confidence *= coverage.pixel(x, y).0; }
            values.push(MaskValue(confidence.clamp(0.0, 1.0)));
        }
    }
    plane(width, height, request.source.base_color().tile_edge(), values)
}

fn measured_boundary_cost(
    request: &DomainRequest, channels: &[PreparedExemplarChannel], width: u32, height: u32,
    seams: &[SelectedSeam],
) -> (u16, u16) {
    let mut result = (request.analysis.seamability.horizontal_cost_milli, request.analysis.seamability.vertical_cost_milli);
    if let Some(seam) = seams.iter().find(|seam| seam.axis == SeamAxis::X) {
        result.0 = measured_axis_boundary_cost(request, channels, width, height, seam);
    }
    if let Some(seam) = seams.iter().find(|seam| seam.axis == SeamAxis::Y) {
        result.1 = measured_axis_boundary_cost(request, channels, width, height, seam);
    }
    result
}

fn measured_axis_boundary_cost(
    request: &DomainRequest, channels: &[PreparedExemplarChannel], width: u32, height: u32,
    seam: &SelectedSeam,
) -> u16 {
    let Some(base) = channels.iter().find_map(|channel| match channel {
        PreparedExemplarChannel::BaseColor { plane, .. } => Some(plane), _ => None,
    }) else { return 1000; };
    let weights = normalized_weight_f64(request);
    let length = if seam.axis == SeamAxis::X { height } else { width };
    let mut total = 0.0_f64;
    for line in 0..length {
        let (a, b) = match seam.axis {
            SeamAxis::X => ((0, line), (width - 1, line)),
            SeamAxis::Y => ((line, 0), (line, height - 1)),
        };
        let color = f64::from(color_difference(base.pixel(a.0, a.1), base.pixel(b.0, b.1)));
        let gradient = f64::from((periodic_luminance_gradient(base, a.0, a.1)
            - periodic_luminance_gradient(base, b.0, b.1)).abs());
        let height_cost = output_scalar_difference(channels, MaterialChannelRole::Height, a, b);
        let roughness = output_scalar_difference(channels, MaterialChannelRole::Roughness, a, b);
        let normal = output_normal_difference(channels, a, b);
        let position = u32::from(seam.positions[line as usize]);
        let source_pair = match seam.axis {
            SeamAxis::X => ((position, line), (width - 1 - position, line)),
            SeamAxis::Y => ((line, position), (line, height - 1 - position)),
        };
        let structure = structure_penalty(request, source_pair.0).max(structure_penalty(request, source_pair.1));
        total += weights[0] * color + weights[1] * gradient + weights[2] * height_cost
            + weights[3] * normal + weights[4] * roughness + weights[5] * structure;
    }
    score((total / f64::from(length.max(1))) as f32)
}

fn output_scalar_difference(
    channels: &[PreparedExemplarChannel], role: MaterialChannelRole, a: (u32, u32), b: (u32, u32),
) -> f64 {
    channels.iter().find_map(|channel| match channel {
        PreparedExemplarChannel::Scalar { role: found, plane } if *found == role =>
            Some(f64::from((plane.pixel(a.0, a.1).0 - plane.pixel(b.0, b.1).0).abs())),
        _ => None,
    }).unwrap_or(0.0)
}

fn output_normal_difference(channels: &[PreparedExemplarChannel], a: (u32, u32), b: (u32, u32)) -> f64 {
    channels.iter().find_map(|channel| match channel {
        PreparedExemplarChannel::Normal { plane, .. } => {
            let na = normalize3(plane.pixel(a.0, a.1).xyz); let nb = normalize3(plane.pixel(b.0, b.1).xyz);
            Some(f64::from((1.0 - dot(na, nb)).clamp(0.0, 2.0)) * 0.5)
        }
        _ => None,
    }).unwrap_or(0.0)
}

fn color_difference(a: &LinearColor, b: &LinearColor) -> f32 {
    (0..3).map(|index| (a.rgb[index] - b.rgb[index]).abs()).sum::<f32>() / 3.0
}

fn luminance_gradient(plane: &ImagePlane<LinearColor>, x: u32, y: u32) -> f32 {
    let left = x.saturating_sub(1); let right = (x + 1).min(plane.width() - 1);
    let top = y.saturating_sub(1); let bottom = (y + 1).min(plane.height() - 1);
    let lum = |x, y| { let c = plane.pixel(x, y).rgb; c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722 };
    (lum(right, y) - lum(left, y)).hypot(lum(x, bottom) - lum(x, top)) * 0.5
}

fn periodic_luminance_gradient(plane: &ImagePlane<LinearColor>, x: u32, y: u32) -> f32 {
    let left = (x + plane.width() - 1) % plane.width(); let right = (x + 1) % plane.width();
    let top = (y + plane.height() - 1) % plane.height(); let bottom = (y + 1) % plane.height();
    let lum = |x, y| { let c = plane.pixel(x, y).rgb; c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722 };
    (lum(right, y) - lum(left, y)).hypot(lum(x, bottom) - lum(x, top)) * 0.5
}

fn normalize3(value: [f32; 3]) -> [f32; 3] {
    let length = dot(value, value).sqrt();
    if length > 1.0e-8 { [value[0] / length, value[1] / length, value[2] / length] } else { [0.0, 0.0, 1.0] }
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }

fn seam_summary(seam: &SelectedSeam) -> SelectedSeamSummary {
    SelectedSeamSummary {
        axis: seam.axis,
        sample_count: seam.positions.len() as u32,
        minimum_position: seam.positions.iter().copied().min().unwrap_or(0),
        maximum_position: seam.positions.iter().copied().max().unwrap_or(0),
        normalized_cost_milli: seam.normalized_cost_milli,
    }
}

fn preflight(request: &DomainRequest, width: u32, height: u32) -> Result<(), DomainError> {
    let pixels = u64::from(width).checked_mul(u64::from(height)).ok_or(DomainError::ResourceLimitExceeded)?;
    let channels = request.source.channels.len() as u64;
    let bytes = pixels.checked_mul(128 + channels * 24).ok_or(DomainError::ResourceLimitExceeded)?;
    let search = u64::from(request.graph_cut.overlap_pixels.min(request.graph_cut.max_search_positions));
    let operations = pixels.checked_mul(20 + channels * 8 + search * 3).ok_or(DomainError::ResourceLimitExceeded)?;
    if bytes > request.graph_cut.max_working_bytes || operations > request.graph_cut.max_operations {
        Err(DomainError::ResourceLimitExceeded)
    } else { Ok(()) }
}

fn direct_preflight(request: &DomainRequest, width: u32, height: u32) -> Result<(), DomainError> {
    let pixels = u64::from(width).checked_mul(u64::from(height)).ok_or(DomainError::ResourceLimitExceeded)?;
    // Validity and provenance are the only full-size Direct allocations. Source channels,
    // correspondence, and operations remain shared/identity representations.
    let bytes = pixels.checked_mul(16).ok_or(DomainError::ResourceLimitExceeded)?;
    let operations = pixels.checked_mul(4).ok_or(DomainError::ResourceLimitExceeded)?;
    if bytes > request.graph_cut.max_working_bytes || operations > request.graph_cut.max_operations {
        Err(DomainError::ResourceLimitExceeded)
    } else { Ok(()) }
}

fn weight_sum(weights: SeamWeights) -> u64 {
    [weights.color, weights.gradient, weights.height, weights.vector_normal, weights.roughness, weights.structure_cut]
        .into_iter().map(u64::from).sum()
}

fn settings_hash(request: &DomainRequest) -> ContentDigest {
    ContentDigest::sha256(format!("{:?}|{}|{}|{}|{}|{:?}|{}", request.graph_cut.axes,
        request.graph_cut.overlap_pixels, request.graph_cut.max_search_positions,
        request.graph_cut.continuity_milli, request.graph_cut.max_accepted_seam_cost_milli,
        request.graph_cut.weights, request.seed).as_bytes())
}

fn active_sources(sample: &CorrespondenceSample) -> usize { sample.sources.iter().flatten().count() }

fn score(value: f32) -> u16 { (value.clamp(0.0, 1.0) * 1000.0).round() as u16 }

fn pixel_count(width: u32, height: u32) -> Result<usize, DomainError> {
    u64::from(width).checked_mul(u64::from(height)).and_then(|value| usize::try_from(value).ok())
        .ok_or(DomainError::ResourceLimitExceeded)
}

fn plane<T: Clone>(width: u32, height: u32, tile_edge: u32, values: Vec<T>) -> Result<ImagePlane<T>, DomainError> {
    ImagePlane::from_row_major(width, height, tile_edge, &values).map_err(|_| DomainError::PlaneConstruction)
}

fn check_cancel(cancellation: &RenderCancellationToken) -> Result<(), DomainError> {
    if cancellation.is_cancelled() { Err(DomainError::Cancelled) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hot_trimmer_domain::{NormalConvention, StageResult};
    use hot_trimmer_domain::{MaterialCorpusManifest, SyntheticGenerator};
    use hot_trimmer_image_io::{NormalAlphaPolicy, ResolvedAlphaMode, ResolutionPyramid};
    use hot_trimmer_material_analysis::{
        FeatureDebugView, FeatureFieldQa, PeriodicityEvidenceMethod, PeriodicityField,
        GlobalOrientation, MeasurementOverlay, OrientationAuthority, OrientationOverlay,
        OverlayCoordinateSpace, ScaleOrientationReport, SeamabilityField, StructureFields, UsabilityField,
    };
    use hot_trimmer_material_analysis::{analyze_source, calibrate_scale_orientation, extract_feature_fields,
        AnalysisSettings, FeatureFieldSettings, ReflectanceProvenance, RouteExecution,
        ScaleOrientationSettings};

    fn scalar_plane(width: u32, height: u32, values: Vec<f32>) -> ImagePlane<LinearScalar> {
        plane(width, height, 4, values.into_iter().map(LinearScalar).collect()).expect("scalar plane")
    }

    fn pyramid(image: ImagePlane<LinearScalar>) -> ResolutionPyramid<LinearScalar> {
        ResolutionPyramid::from_levels(vec![image]).expect("pyramid")
    }

    fn stage_result() -> StageResult { StageResult::PassThrough { reason: "fixture".into() } }

    fn fixture(width: u32, height: u32, boundary_cost: u16) -> DomainRequest {
        let mut colors = Vec::new();
        let mut normals = Vec::new();
        let mut ids = Vec::new();
        let mut roughness = Vec::new();
        let mut opacity = Vec::new();
        for y in 0..height { for x in 0..width {
            let value = ((x * 17 + y * 11 + (x * y) % 7) % 31) as f32 / 31.0;
            colors.push(LinearColor { rgb: [value, value * 0.8, value * 0.6], alpha: 1.0 });
            normals.push(TangentNormal { xyz: normalize3([(x as f32 / width as f32 - 0.5) * 0.3, 0.1, 1.0]), alpha: 1.0 });
            ids.push(CategoryId(if x < width / 2 { 3 } else { 19 }));
            roughness.push(LinearScalar(0.2 + value * 0.6));
            opacity.push(MaskValue((x as f32 / (width - 1).max(1) as f32).clamp(0.0, 1.0)));
        }}
        let base = plane(width, height, 4, colors).expect("base");
        let prepared = ContentDigest::sha256(b"prepared");
        let source = DelitPreparedExemplar {
            exemplar_id: "evidence_fixture".into(),
            prepared_source_digest: prepared.clone(),
            perspective_confidence_milli: 1000,
            original_prepared_base_color: base.clone(),
            channels: vec![
                PreparedExemplarChannel::BaseColor { plane: base, alpha_mode: ResolvedAlphaMode::Opaque },
                PreparedExemplarChannel::Scalar { role: MaterialChannelRole::Roughness, plane: plane(width, height, 4, roughness).expect("roughness") },
                PreparedExemplarChannel::Normal { plane: plane(width, height, 4, normals).expect("normals"),
                    source_convention: NormalConvention::OpenGl, canonical_convention: NormalConvention::OpenGl,
                    alpha_policy: NormalAlphaPolicy::Preserve },
                PreparedExemplarChannel::MaterialId { plane: plane(width, height, 4, ids).expect("ids") },
                PreparedExemplarChannel::Mask { role: MaterialChannelRole::Opacity,
                    plane: plane(width, height, 4, opacity).expect("opacity") },
            ],
            coverage: Some(plane(width, height, 4, vec![MaskValue(1.0); (width * height) as usize]).expect("coverage")),
            masks: None,
            reflectance_provenance: ReflectanceProvenance::ImportedPrepared,
            route_execution: RouteExecution::PassThrough(hot_trimmer_domain::DelightingPassThroughReason::AuthoredTextureOrPbrSet),
            upstream_stage_result: stage_result(),
            stage_result: stage_result(),
        };
        let zeros = vec![0.0; (width * height) as usize];
        let mut structural = zeros.clone();
        for y in 0..height {
            structural[(y * width) as usize] = 1.0;
            structural[(y * width + width - 1) as usize] = 1.0;
        }
        let structure = StructureFields {
            edge: pyramid(scalar_plane(width, height, structural)),
            line: pyramid(scalar_plane(width, height, zeros.clone())),
            boundary: pyramid(scalar_plane(width, height, zeros.clone())),
            grid: pyramid(scalar_plane(width, height, zeros.clone())),
            fiber: pyramid(scalar_plane(width, height, zeros.clone())),
            intersection: pyramid(scalar_plane(width, height, zeros.clone())),
        };
        let scale_orientation = Arc::new(ScaleOrientationReport {
            cache_key: ContentDigest::sha256(b"scale-orientation"),
            downstream_footprint_key: ContentDigest::sha256(b"scale-footprint"),
            prepared_source_digest: prepared.clone(),
            stage_five_cache_key: hot_trimmer_material_analysis::SourceAnalysisCacheKey(ContentDigest::sha256(b"stage-five")),
            scale: hot_trimmer_domain::PhysicalScaleEvidence::default(), scale_diagnostics: Vec::new(),
            global_orientation: GlobalOrientation { axis_millidegrees: None, measured_axis_millidegrees: None,
                anisotropy_milli: 0, confidence_milli: 0, authority: OrientationAuthority::UnavailableLowConfidence,
                destructive_rotation_allowed: false }, local_orientation: Vec::new(),
            measurement_overlay: MeasurementOverlay { coordinate_space: OverlayCoordinateSpace::SourcePixels,
                source_pixels_per_meter_x_milli: None, source_pixels_per_meter_y_milli: None,
                world_scale_available: false, label: "relative-only fixture".into() },
            orientation_overlay: OrientationOverlay { coordinate_space: OverlayCoordinateSpace::SourcePixels,
                global: GlobalOrientation { axis_millidegrees: None, measured_axis_millidegrees: None,
                    anisotropy_milli: 0, confidence_milli: 0, authority: OrientationAuthority::UnavailableLowConfidence,
                    destructive_rotation_allowed: false }, local: Vec::new() }, stage_result: stage_result(),
        });
        let registered = ContentDigest::sha256(format!("{}|{}|{}|{}|{}|{}", prepared.0,
            source.exemplar_id, width, height, scale_orientation.cache_key.0,
            source.channels.iter().map(|channel| format!("{:?}", channel.role())).collect::<Vec<_>>().join(",")).as_bytes());
        let analysis = FeatureFieldReport {
            cache_key: ContentDigest::sha256(b"analysis"),
            prepared_source_digest: prepared.clone(),
            stage_six_cache_key: scale_orientation.cache_key.clone(),
            registration_digest: registered,
            saliency: pyramid(scalar_plane(width, height, zeros.clone())),
            structure,
            stationarity: pyramid(scalar_plane(width, height, zeros.clone())),
            periodicity: PeriodicityField { confidence: pyramid(scalar_plane(width, height, zeros.clone())), candidates: Vec::new(),
                evidence_method: PeriodicityEvidenceMethod::BoundedNormalizedAutocorrelation },
            seamability: SeamabilityField { confidence: pyramid(scalar_plane(width, height, vec![0.8; (width * height) as usize])),
                available_terms: BTreeSet::from([SeamTerm::Color, SeamTerm::Gradient, SeamTerm::VectorNormal,
                    SeamTerm::Roughness, SeamTerm::StructuralCrossing]), horizontal_cost_milli: boundary_cost,
                vertical_cost_milli: boundary_cost },
            usability: UsabilityField { confidence: pyramid(scalar_plane(width, height, vec![1.0; (width * height) as usize])), reasons: Vec::new() },
            qa: FeatureFieldQa { coordinate_space: "registered_source_pixels", level_dimensions: vec![(width, height)],
                views: vec![FeatureDebugView::Seamability] },
            stage_result: stage_result(),
        };
        DomainRequest {
            source: Arc::new(source),
            prepared_source_digest: prepared,
            analysis: Arc::new(analysis),
            scale_orientation,
            route: DomainRoute::Auto,
            direct_boundary_threshold_milli: 80,
            graph_cut: GraphCutSettings { axes: ClosureAxes::XY, overlap_pixels: 3, max_search_positions: 3,
                continuity_milli: 20, ..GraphCutSettings::default() },
            quilting: QuiltingSettings::default(),
            patch_match: PatchMatchSettings::default(),
            seed: 41,
        }
    }

    #[test]
    fn algorithm_stage_08a_graphcut_registered_closure() {
        let mut cache = MaterialDomainCache::default();
        let cancellation = RenderCancellationToken::new();

        let graph_request = fixture(12, 10, 700);
        let domain = prepare_material_domain(&graph_request, &mut cache, &cancellation).expect("graph-cut domain");
        assert_eq!(domain.route, DomainRoute::GraphCutPeriodicClosure);
        assert_eq!(domain.seams.len(), 2);
        assert!(domain.seams.iter().find(|seam| seam.axis == SeamAxis::X).expect("X seam")
            .positions.iter().all(|position| *position > 0), "lower-cost seam must avoid structured boundary");
        assert_eq!(domain.registered_channels().len(), graph_request.source.channels.len());
        let RegisteredChannelRef::BaseColor(color) = domain.channel(MaterialChannelRole::BaseColor).expect("base color") else { panic!("typed base color") };
        for y in 0..domain.height { assert_eq!(color.pixel(0, y), color.pixel(domain.width - 1, y)); }
        for x in 0..domain.width { assert_eq!(color.pixel(x, 0), color.pixel(x, domain.height - 1)); }
        let RegisteredChannelRef::Normal(normal) = domain.channel(MaterialChannelRole::Normal).expect("normal") else { panic!("typed normal") };
        for tile in normal.tiles() { for value in &tile.pixels { assert!((dot(value.xyz, value.xyz) - 1.0).abs() < 1.0e-5); } }
        let RegisteredChannelRef::MaterialId(ids) = domain.channel(MaterialChannelRole::MaterialId).expect("IDs") else { panic!("typed IDs") };
        for tile in ids.tiles() { for id in &tile.pixels { assert!(matches!(id.0, 3 | 19), "IDs are selected, never blended"); } }
        let RegisteredChannelRef::Mask(MaterialChannelRole::Opacity, opacity) = domain.channel(MaterialChannelRole::Opacity).expect("opacity") else { panic!("typed opacity") };
        let source_opacity = graph_request.source.channels.iter().find_map(|channel| match channel {
            PreparedExemplarChannel::Mask { role: MaterialChannelRole::Opacity, plane } => Some(plane), _ => None,
        }).expect("source opacity");
        let CorrespondenceField::Registered(correspondence) = &domain.correspondence else { panic!("registered correspondence") };
        let mut saw_opacity_blend = false;
        for y in 0..domain.height { for x in 0..domain.width {
            let sample = correspondence.pixel(x, y);
            if active_sources(sample) > 1 {
                saw_opacity_blend = true;
                let expected = blend_mask(source_opacity, sample);
                assert!((opacity.pixel(x, y).0 - expected).abs() < 1.0e-6, "opacity follows continuous weights");
            }
        }}
        assert!(saw_opacity_blend);
        let after = domain.diagnostics.boundary_cost_after_milli;
        assert_eq!(after.0, 0, "X Base Color and all available registered terms close exactly");
        assert!(after.1 <= graph_request.graph_cut.max_accepted_seam_cost_milli,
            "post-closure multi-channel Y cost stays below the declared threshold");
        assert!(after.0 < domain.diagnostics.boundary_cost_before_milli.0
            && after.1 < domain.diagnostics.boundary_cost_before_milli.1,
            "multi-channel diagnostics must measure an improvement on both closed axes");
        assert!(matches!(domain.correspondence, CorrespondenceField::Registered(_)));
        assert!(matches!(domain.operations, OperationField::Registered(_)));
        assert_eq!(prepare_material_domain(&graph_request, &mut cache, &cancellation).expect("cached").cache_key, domain.cache_key);

        let direct_request = fixture(12, 10, 20);
        let direct = prepare_material_domain(&direct_request, &mut cache, &cancellation).expect("direct domain");
        assert_eq!(direct.route, DomainRoute::DirectSource);
        assert!(matches!(direct.correspondence, CorrespondenceField::Identity { .. }));
        assert!(direct.diagnostics.pass_through.as_ref().is_some_and(|evidence| evidence.no_resampling && evidence.exact_source_coordinates));

        let mut unacceptable = fixture(12, 10, 700);
        unacceptable.graph_cut.max_accepted_seam_cost_milli = 0;
        assert!(matches!(prepare_material_domain(&unacceptable, &mut cache, &cancellation),
            Err(DomainError::UnacceptableSeam { .. })), "visible high-cost closure must fail explicitly");

        let mut foreign_analysis = fixture(12, 10, 700);
        Arc::make_mut(&mut foreign_analysis.analysis).prepared_source_digest = ContentDigest::sha256(b"foreign-prepared");
        assert_eq!(prepare_material_domain(&foreign_analysis, &mut cache, &cancellation), Err(DomainError::RegistrationDrift));

        let mut bounded_direct = fixture(12, 10, 20);
        bounded_direct.graph_cut.max_working_bytes = 1;
        assert_eq!(prepare_material_domain(&bounded_direct, &mut cache, &cancellation), Err(DomainError::ResourceLimitExceeded));

        let mut insufficient = fixture(5, 5, 700);
        insufficient.graph_cut.overlap_pixels = 3;
        assert!(matches!(prepare_material_domain(&insufficient, &mut cache, &cancellation),
            Err(DomainError::InsufficientOverlap { axis: SeamAxis::X, .. })));
        let cancelled = RenderCancellationToken::new(); cancelled.cancel();
        assert_eq!(prepare_material_domain(&graph_request, &mut MaterialDomainCache::default(), &cancelled), Err(DomainError::Cancelled));
    }

    fn quilting_fixture(duplicate_weight: u16) -> DomainRequest {
        let mut request = fixture(12, 10, 700);
        request.route = DomainRoute::TextureQuilting;
        request.quilting = QuiltingSettings {
            output_width: 27, output_height: 23,
            patch_size: QuiltingPatchSize::RelativeMilli { width: 500, height: 500 },
            overlap_milli: 400, pyramid_levels: 2, candidate_count: 42,
            near_best_count: 1, near_best_threshold_milli: 0,
            weights: QuiltingWeights { overlap: 1, histogram: 0, structure: 0,
                duplicate_use: duplicate_weight, boundary_periodicity: 0 },
            max_candidate_count: 64, max_output_pixels: 4096, max_patch_count: 128,
            max_iterations: 128, max_working_bytes: 16_000_000, max_operations: 20_000_000,
            ..QuiltingSettings::default()
        };
        let source = Arc::make_mut(&mut request.source);
        for channel in &mut source.channels {
            match channel {
                PreparedExemplarChannel::BaseColor { plane: image, .. } => *image = plane(12, 10, 4,
                    vec![LinearColor { rgb: [0.4, 0.4, 0.4], alpha: 1.0 }; 120]).unwrap(),
                PreparedExemplarChannel::Scalar { plane: image, .. } => *image = plane(12, 10, 4,
                    vec![LinearScalar(0.5); 120]).unwrap(),
                PreparedExemplarChannel::Normal { plane: image, .. } => *image = plane(12, 10, 4,
                    vec![TangentNormal { xyz: [0.0, 0.0, 1.0], alpha: 1.0 }; 120]).unwrap(),
                PreparedExemplarChannel::MaterialId { plane: image } => *image = plane(12, 10, 4,
                    vec![CategoryId(3); 120]).unwrap(),
                PreparedExemplarChannel::Mask { plane: image, .. } => *image = plane(12, 10, 4,
                    vec![MaskValue(1.0); 120]).unwrap(),
            }
        }
        let analysis = Arc::make_mut(&mut request.analysis);
        let zero = || pyramid(scalar_plane(12, 10, vec![0.0; 120]));
        analysis.structure.edge = zero(); analysis.structure.line = zero();
        analysis.structure.boundary = zero(); analysis.structure.grid = zero();
        analysis.structure.fiber = zero(); analysis.structure.intersection = zero();
        request
    }

    fn corpus_quilting_request(generator: SyntheticGenerator) -> DomainRequest {
        let corpus = MaterialCorpusManifest::bundled().unwrap();
        let generated = corpus.synthetic_fixtures.iter().find(|fixture| fixture.generator == generator).unwrap().generate();
        let (width, height) = (generated.spec.width, generated.spec.height);
        let values: Vec<f32> = generated.planes["base_color"].iter().map(|value| f32::from(*value) / 65_535.0).collect();
        let mut request = fixture(width, height, 700);
        let source = Arc::make_mut(&mut request.source);
        for channel in &mut source.channels {
            match channel {
                PreparedExemplarChannel::BaseColor { plane: image, .. } => *image = plane(width, height, 16,
                    values.iter().map(|value| LinearColor { rgb: [*value; 3], alpha: 1.0 }).collect()).unwrap(),
                PreparedExemplarChannel::Scalar { plane: image, .. } => *image = plane(width, height, 16,
                    values.iter().copied().map(LinearScalar).collect()).unwrap(),
                PreparedExemplarChannel::Normal { plane: image, .. } => *image = plane(width, height, 16,
                    vec![TangentNormal { xyz: [0.0, 0.0, 1.0], alpha: 1.0 }; values.len()]).unwrap(),
                PreparedExemplarChannel::MaterialId { plane: image } => *image = plane(width, height, 16,
                    vec![CategoryId(3); values.len()]).unwrap(),
                PreparedExemplarChannel::Mask { plane: image, .. } => *image = plane(width, height, 16,
                    vec![MaskValue(1.0); values.len()]).unwrap(),
            }
        }
        source.original_prepared_base_color = source.base_color().clone();
        source.coverage = Some(plane(width, height, 16, vec![MaskValue(1.0); values.len()]).unwrap());
        let five = analyze_source(source, &AnalysisSettings::default(), None, &RenderCancellationToken::new()).unwrap();
        let six = calibrate_scale_orientation(source, &five, &hot_trimmer_domain::MaterialCalibrationIntent::default(),
            &ScaleOrientationSettings::default(), &RenderCancellationToken::new()).unwrap();
        let seven = extract_feature_fields(source, &six, &FeatureFieldSettings::default(), &RenderCancellationToken::new()).unwrap();
        request.scale_orientation = Arc::new(six); request.analysis = Arc::new(seven);
        request.route = DomainRoute::TextureQuilting;
        request.quilting = QuiltingSettings { output_width: 97, output_height: 89,
            patch_size: QuiltingPatchSize::RelativeMilli { width: 500, height: 500 },
            overlap_milli: 375, pyramid_levels: 4, candidate_count: 40, near_best_count: 6,
            max_candidate_count: 64, max_output_pixels: 16_384, max_patch_count: 128,
            max_iterations: 128, max_working_bytes: 64_000_000, max_operations: 100_000_000,
            max_accepted_seam_cost_milli: 1000, max_boundary_periodicity_error_milli: 1000,
            ..QuiltingSettings::default() };
        request
    }

    fn dominant_axis_degrees(image: &ImagePlane<LinearColor>) -> f64 {
        let (mut xx, mut yy, mut xy) = (0.0_f64, 0.0_f64, 0.0_f64);
        let lum = |x, y| { let c = image.pixel(x, y).rgb; f64::from(c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722) };
        for y in 1..image.height() - 1 { for x in 1..image.width() - 1 {
            let gx = lum(x + 1, y) - lum(x - 1, y); let gy = lum(x, y + 1) - lum(x, y - 1);
            xx += gx * gx; yy += gy * gy; xy += gx * gy;
        }}
        (0.5 * (2.0 * xy).atan2(xx - yy).to_degrees() + 90.0).rem_euclid(180.0)
    }

    fn axis_difference_degrees(a: f64, b: f64) -> f64 {
        let delta = (a - b).abs().rem_euclid(180.0); delta.min(180.0 - delta)
    }

    #[test]
    fn algorithm_stage_08b_quilting() {
        let cancellation = RenderCancellationToken::new();
        let low = quilting_fixture(0);
        let low_domain = prepare_material_domain(&low, &mut MaterialDomainCache::default(), &cancellation).unwrap();
        let replay = prepare_material_domain(&low, &mut MaterialDomainCache::default(), &cancellation).unwrap();
        assert_eq!(low_domain.correspondence, replay.correspondence, "fixed seed must replay exactly");
        assert_eq!(low_domain.route, DomainRoute::TextureQuilting);
        assert!(low_domain.width > low.source.base_color().width() && low_domain.height > low.source.base_color().height());
        let low_qa = low_domain.quilting.as_ref().unwrap();
        assert!(low_qa.placements.len() > 4 && !low_qa.overlap_seams.is_empty());
        assert!(low_domain.qa_views.contains(&DomainQaView::Quilting)
            && low_domain.qa_views.contains(&DomainQaView::SourceUsage));
        let CorrespondenceField::Registered(correspondence) = &low_domain.correspondence else { panic!("registered quilting correspondence") };
        assert!(correspondence.tiles().iter().flat_map(|tile| &tile.pixels)
            .all(|sample| active_sources(sample) == 1), "one shared discrete seam cut must drive all channels");
        let RegisteredChannelRef::Normal(normal) = low_domain.channel(MaterialChannelRole::Normal).unwrap() else { panic!("normal") };
        assert!(normal.tiles().iter().flat_map(|tile| &tile.pixels)
            .all(|value| (dot(value.xyz, value.xyz) - 1.0).abs() < 1.0e-6));

        let high = quilting_fixture(1000);
        let high_domain = prepare_material_domain(&high, &mut MaterialDomainCache::default(), &cancellation).unwrap();
        assert!(high_domain.quilting.as_ref().unwrap().source_usage.len() > low_qa.source_usage.len(),
            "duplicate penalty must measurably diversify a fixed-seed source stream");

        let stochastic = corpus_quilting_request(SyntheticGenerator::Registration);
        let stochastic_domain = prepare_material_domain(&stochastic, &mut MaterialDomainCache::default(), &cancellation).unwrap();
        let stochastic_qa = stochastic_domain.quilting.as_ref().unwrap();
        let first_row: Vec<_> = stochastic_qa.placements.iter().filter(|placement| placement.output_y == 0)
            .map(|placement| placement.output_x).collect();
        let advances: BTreeSet<_> = first_row.windows(2).map(|pair| pair[1] - pair[0]).collect();
        assert!(advances.len() > 1, "stochastic corpus quilting must not use a fixed output grid advance");
        let RegisteredChannelRef::BaseColor(stochastic_color) = stochastic_domain.channel(MaterialChannelRole::BaseColor).unwrap() else { panic!("color") };
        let RegisteredChannelRef::Scalar(MaterialChannelRole::Roughness, stochastic_roughness) = stochastic_domain.channel(MaterialChannelRole::Roughness).unwrap() else { panic!("roughness") };
        for y in 0..stochastic_domain.height { for x in 0..stochastic_domain.width {
            assert!((stochastic_color.pixel(x, y).rgb[0] - stochastic_roughness.pixel(x, y).0).abs() < 1.0e-6,
                "registered corpus channels must never drift");
        }}
        let shift = u32::from(stochastic_qa.patch_size_pixels.0 - stochastic_qa.overlap_pixels.0);
        let mut exact_repeats = 0_u64; let mut comparisons = 0_u64;
        for y in 0..stochastic_domain.height { for x in 0..stochastic_domain.width - shift {
            exact_repeats += u64::from(stochastic_color.pixel(x, y) == stochastic_color.pixel(x + shift, y)); comparisons += 1;
        }}
        assert!(exact_repeats * 5 < comparisons, "corpus expansion must not expose a dominant fixed-patch repeat");

        let mut directional_corpus = corpus_quilting_request(SyntheticGenerator::Orientation);
        let stage6_axis = directional_corpus.scale_orientation.global_orientation.axis_millidegrees
            .expect("directional corpus requires Stage 6 authority");
        let measured_input = dominant_axis_degrees(directional_corpus.source.base_color());
        directional_corpus.quilting.semantics = QuiltingSemanticConstraint::Directional {
            behavior: hot_trimmer_domain::MaterialBehaviorClass::StochasticDirectional,
            requested_angle_millidegrees: stage6_axis as i32,
            tolerance_millidegrees: 8_000,
        };
        let directional_domain = prepare_material_domain(&directional_corpus, &mut MaterialDomainCache::default(), &cancellation).unwrap();
        let RegisteredChannelRef::BaseColor(directional_output) = directional_domain.channel(MaterialChannelRole::BaseColor).unwrap() else { panic!("directional output") };
        assert!(axis_difference_degrees(measured_input, dominant_axis_degrees(directional_output)) <= 8.0,
            "directional corpus output must remain within the declared angular tolerance");

        let mut unique = quilting_fixture(1000);
        unique.quilting.semantics = QuiltingSemanticConstraint::UniqueDetail;
        assert!(matches!(prepare_material_domain(&unique, &mut MaterialDomainCache::default(), &cancellation),
            Err(DomainError::IncompatibleQuilting { .. })), "unique semantics must route away, never smear");

        let mut physical = quilting_fixture(1000);
        physical.quilting.patch_size = QuiltingPatchSize::PhysicalMicrometers { width: 6_000, height: 5_000 };
        assert!(matches!(prepare_material_domain(&physical, &mut MaterialDomainCache::default(), &cancellation),
            Err(DomainError::IncompatibleQuilting { .. })), "relative/prior Stage 6 scale must not authorize physical sizing");
        let scale = &mut Arc::make_mut(&mut physical.scale_orientation).scale;
        scale.source_pixels_per_meter_x_milli = Some(1_000_000);
        scale.source_pixels_per_meter_y_milli = Some(1_000_000);
        scale.provenance = hot_trimmer_domain::ScaleProvenance::UserMeasured;
        scale.confidence_milli = 1000;
        scale.world_scale = hot_trimmer_domain::WorldScaleAvailability::Available;
        let physical_domain = prepare_material_domain(&physical, &mut MaterialDomainCache::default(), &cancellation).unwrap();
        assert_eq!(physical_domain.quilting.as_ref().unwrap().patch_size_pixels, (6, 5));

        let mut seam_rejected = corpus_quilting_request(SyntheticGenerator::Registration);
        seam_rejected.quilting.max_accepted_seam_cost_milli = 0;
        assert!(matches!(prepare_material_domain(&seam_rejected, &mut MaterialDomainCache::default(), &cancellation),
            Err(DomainError::UnacceptableQuiltingSeam { .. })));
        let mut boundary_rejected = corpus_quilting_request(SyntheticGenerator::Registration);
        boundary_rejected.quilting.max_boundary_periodicity_error_milli = 0;
        assert!(matches!(prepare_material_domain(&boundary_rejected, &mut MaterialDomainCache::default(), &cancellation),
            Err(DomainError::UnacceptableQuiltingBoundary { .. })));

        let mut unusable = quilting_fixture(1000);
        Arc::make_mut(&mut unusable.analysis).usability.confidence = pyramid(scalar_plane(12, 10, vec![0.0; 120]));
        assert!(matches!(prepare_material_domain(&unusable, &mut MaterialDomainCache::default(), &cancellation),
            Err(DomainError::UnusableQuiltingSource { rejected_candidates }) if rejected_candidates > 0));
        let cancelled = RenderCancellationToken::new(); cancelled.cancel();
        assert_eq!(prepare_material_domain(&low, &mut MaterialDomainCache::default(), &cancelled), Err(DomainError::Cancelled));
        let mut bounded = quilting_fixture(1000); bounded.quilting.max_working_bytes = 1;
        assert_eq!(prepare_material_domain(&bounded, &mut MaterialDomainCache::default(), &cancellation), Err(DomainError::ResourceLimitExceeded));
    }

    fn patchmatch_settings(width: u32, height: u32) -> PatchMatchSettings {
        PatchMatchSettings { output_width: width, output_height: height, patch_radius: 2,
            pyramid_levels: 3, iterations_per_level: 8, random_search_radius: 32,
            random_candidates_per_radius: 2, convergence_change_threshold_milli: 50,
            max_output_pixels: 32_768, max_working_bytes: 64_000_000,
            max_operations: 120_000_000, ..PatchMatchSettings::default() }
    }

    #[test]
    fn algorithm_stage_08c_patchmatch() {
        let cancellation = RenderCancellationToken::new();
        let mut stochastic = corpus_quilting_request(SyntheticGenerator::Registration);
        stochastic.route = DomainRoute::PatchMatch;
        stochastic.patch_match = patchmatch_settings(83, 79);
        let domain = prepare_material_domain(&stochastic, &mut MaterialDomainCache::default(), &cancellation).unwrap();
        let replay = prepare_material_domain(&stochastic, &mut MaterialDomainCache::default(), &cancellation).unwrap();
        assert_eq!(domain.correspondence, replay.correspondence, "NNF must replay byte-exactly");
        assert_eq!(domain.patch_match, replay.patch_match, "diagnostics must replay byte-exactly");
        let qa = domain.patch_match.as_ref().unwrap();
        assert!(qa.converged && qa.incomplete_pixels == 0 && qa.operation_count > 0);
        assert!(qa.final_changed_pixels * 1000 <= qa.synthesized_pixels * 50,
            "convergence requires actual NNF stabilization below the configured changed fraction");
        assert_eq!(qa.boundary_error_milli, (0, 0), "seamless expansion boundaries are measured after reconstruction");
        assert!(domain.qa_views.contains(&DomainQaView::NearestNeighborField)
            && domain.qa_views.contains(&DomainQaView::Coherence)
            && domain.qa_views.contains(&DomainQaView::SourceUsage));
        let CorrespondenceField::Registered(nnf) = &domain.correspondence else { panic!("registered NNF") };
        assert!(nnf.tiles().iter().flat_map(|tile| &tile.pixels).all(|sample| active_sources(sample) == 1));
        let RegisteredChannelRef::BaseColor(color) = domain.channel(MaterialChannelRole::BaseColor).unwrap() else { panic!("color") };
        let RegisteredChannelRef::Scalar(MaterialChannelRole::Roughness, roughness) = domain.channel(MaterialChannelRole::Roughness).unwrap() else { panic!("roughness") };
        for y in 0..domain.height { for x in 0..domain.width {
            assert!((color.pixel(x, y).rgb[0] - roughness.pixel(x, y).0).abs() < 1.0e-6,
                "one NNF must reconstruct every registered channel");
        }}
        let RegisteredChannelRef::Normal(normal) = domain.channel(MaterialChannelRole::Normal).unwrap() else { panic!("normal") };
        assert!(normal.tiles().iter().flat_map(|tile| &tile.pixels).all(|n| (dot(n.xyz, n.xyz) - 1.0).abs() < 1.0e-6));

        let mut directional = corpus_quilting_request(SyntheticGenerator::Orientation);
        directional.route = DomainRoute::PatchMatch;
        directional.patch_match = patchmatch_settings(81, 77);
        let axis = directional.scale_orientation.global_orientation.axis_millidegrees.unwrap();
        directional.patch_match.semantics = PatchMatchSemanticConstraint::Directional {
            behavior: hot_trimmer_domain::MaterialBehaviorClass::StochasticDirectional,
            requested_angle_millidegrees: axis as i32, tolerance_millidegrees: 8_000 };
        let measured = dominant_axis_degrees(directional.source.base_color());
        let directional_domain = prepare_material_domain(&directional, &mut MaterialDomainCache::default(), &cancellation).unwrap();
        let RegisteredChannelRef::BaseColor(output) = directional_domain.channel(MaterialChannelRole::BaseColor).unwrap() else { panic!("output") };
        assert!(axis_difference_degrees(measured, dominant_axis_degrees(output)) <= 8.0,
            "unrotated constrained correspondences must preserve direction");

        let mut completion = fixture(12, 10, 700); completion.route = DomainRoute::PatchMatch;
        completion.patch_match = patchmatch_settings(12, 10);
        completion.patch_match.seamless_x = false; completion.patch_match.seamless_y = false;
        completion.patch_match.semantics = PatchMatchSemanticConstraint::ProtectedUniqueCompletion;
        completion.patch_match.completion_mask = Some(Arc::new(plane(12, 10, 4, (0..120).map(|i|
            MaskValue(if (i % 12) >= 4 && (i % 12) <= 7 && (i / 12) >= 3 && (i / 12) <= 6 { 1.0 } else { 0.0 })).collect()).unwrap()));
        completion.patch_match.source_exclusion_mask = Some(Arc::new(plane(12, 10, 4, (0..120).map(|i|
            MaskValue(if i % 12 < 3 { 1.0 } else { 0.0 })).collect()).unwrap()));
        let completed = prepare_material_domain(&completion, &mut MaterialDomainCache::default(), &cancellation).unwrap();
        let CorrespondenceField::Registered(completion_nnf) = &completed.correspondence else { panic!("completion NNF") };
        for y in 3..=6 { for x in 4..=7 { assert!(completion_nnf.pixel(x, y).sources[0].unwrap().coordinate.x >= 3,
            "completion may never sample an excluded source region"); }}
        assert_eq!(completed.patch_match.as_ref().unwrap().preserved_pixels, 104);

        let mut unique = stochastic.clone(); unique.patch_match.semantics = PatchMatchSemanticConstraint::UniqueDetail;
        assert!(matches!(prepare_material_domain(&unique, &mut MaterialDomainCache::default(), &cancellation),
            Err(DomainError::IncompatiblePatchMatch { .. })), "unique content must not repeat without explicit compatible constraints");
        let mut manufactured = stochastic.clone(); manufactured.patch_match.semantics = PatchMatchSemanticConstraint::ManufacturedPattern;
        assert!(matches!(prepare_material_domain(&manufactured, &mut MaterialDomainCache::default(), &cancellation),
            Err(DomainError::IncompatiblePatchMatch { .. })));
        let mut nonconverged = stochastic.clone(); nonconverged.patch_match.iterations_per_level = 1;
        nonconverged.patch_match.convergence_change_threshold_milli = 0;
        assert!(matches!(prepare_material_domain(&nonconverged, &mut MaterialDomainCache::default(), &cancellation),
            Err(DomainError::PatchMatchNonConverged { .. })), "nonconvergence must never publish output");
        let mut bounded = stochastic.clone(); bounded.patch_match.max_working_bytes = 1;
        assert_eq!(prepare_material_domain(&bounded, &mut MaterialDomainCache::default(), &cancellation), Err(DomainError::ResourceLimitExceeded));
        let mut descriptor_bounded = stochastic.clone();
        let source_pixels = u64::from(descriptor_bounded.source.base_color().width())
            * u64::from(descriptor_bounded.source.base_color().height());
        let output_pixels = u64::from(descriptor_bounded.patch_match.output_width)
            * u64::from(descriptor_bounded.patch_match.output_height);
        let legacy_estimate = output_pixels * (96 + descriptor_bounded.source.channels.len() as u64 * 24)
            + source_pixels * 24;
        let actual_requirement = patchmatch_required_working_bytes(&descriptor_bounded).unwrap();
        assert!(actual_requirement > legacy_estimate, "the complete typed descriptor pyramid must be charged at its actual allocation size");
        descriptor_bounded.patch_match.max_working_bytes = legacy_estimate + 1;
        assert_eq!(prepare_material_domain(&descriptor_bounded, &mut MaterialDomainCache::default(), &cancellation),
            Err(DomainError::ResourceLimitExceeded), "preflight must reject before allocating a descriptor pyramid over budget");
        let cancelled = RenderCancellationToken::new(); cancelled.cancel();
        assert_eq!(prepare_material_domain(&stochastic, &mut MaterialDomainCache::default(), &cancelled), Err(DomainError::Cancelled));
    }

    fn router_fixture(behavior: hot_trimmer_domain::MaterialBehaviorClass) -> Stage8RouterRequest {
        let cancellation = RenderCancellationToken::new();
        let mut domain = fixture(12, 10, 20);
        domain.quilting = QuiltingSettings {
            output_width: 12, output_height: 10,
            patch_size: QuiltingPatchSize::RelativeMilli { width: 500, height: 500 },
            pyramid_levels: 1, candidate_count: 4, near_best_count: 2,
            max_candidate_count: 8, max_patch_count: 32, max_iterations: 32,
            max_operations: 1_000_000, max_working_bytes: 4_000_000,
            ..QuiltingSettings::default()
        };
        domain.patch_match = PatchMatchSettings {
            output_width: 12, output_height: 10, patch_radius: 1,
            pyramid_levels: 1, iterations_per_level: 1, random_search_radius: 4,
            random_candidates_per_radius: 1, max_iterations: 4,
            max_operations: 1_000_000, max_working_bytes: 4_000_000,
            ..PatchMatchSettings::default()
        };
        let mut stage_five = analyze_source(&domain.source, &AnalysisSettings::default(), None, &cancellation).unwrap();
        stage_five.classification.analyzed_class = behavior;
        stage_five.classification.confidence_milli = 900;
        Arc::make_mut(&mut domain.scale_orientation).stage_five_cache_key = stage_five.cache_key.clone();
        Stage8RouterRequest { domain, stage_five: Arc::new(stage_five),
            policy: MaterialDomainRoutePolicy::default(), procedural_override: None,
            procedural_settings: ProceduralFitSettings { limits: ProceduralLimits {
                max_output_pixels: 65_536, max_working_bytes: 16_000_000,
                max_operations: 100_000_000, max_fit_samples: 4096,
            }, ..ProceduralFitSettings::default() }, output_width: 12, output_height: 10 }
    }

    struct TestLearnedProvider { device: LearnedExecutionDevice, include_estimated_maps: bool }
    impl LocalLearnedMaterialProvider for TestLearnedProvider {
        fn descriptor(&self) -> LearnedProviderDescriptor { LearnedProviderDescriptor {
            provider_id: "approved-test-provider".into(), provider_version: "1.0.0".into(),
            interface_version: LEARNED_PROVIDER_INTERFACE_VERSION,
            model_digest: ContentDigest::sha256(b"approved-test-model"),
            capabilities: if self.include_estimated_maps { BTreeSet::from([LearnedCapability::SeamlessExpansion,
                LearnedCapability::EstimatedHeight, LearnedCapability::EstimatedNormal]) }
                else { BTreeSet::from([LearnedCapability::SeamlessExpansion]) },
            device_policy: LearnedDevicePolicy::CpuOnly, deterministic: true,
            maximum_input_pixels: 1024, maximum_output_pixels: 1024, maximum_working_bytes: 1_000_000,
        }}
        fn infer(&self, request: &LearnedMaterialRequest<'_>, _: &RenderCancellationToken)
            -> Result<LearnedMaterialOutput, LearnedProviderError>
        {
            if !self.include_estimated_maps && request.requested.iter().any(|capability|
                matches!(capability, LearnedCapability::EstimatedHeight | LearnedCapability::EstimatedNormal))
            { return Err(LearnedProviderError::UnsupportedCapability); }
            let count = (request.output_width * request.output_height) as usize;
            let channels = request.source.channels.iter().map(|channel| match channel {
                PreparedExemplarChannel::BaseColor { plane, alpha_mode } => PreparedExemplarChannel::BaseColor {
                    plane: ImagePlane::from_row_major(request.output_width, request.output_height, 4,
                        &vec![*plane.pixel(0, 0); count]).unwrap(), alpha_mode: *alpha_mode },
                PreparedExemplarChannel::Scalar { role, plane } => PreparedExemplarChannel::Scalar { role: *role,
                    plane: ImagePlane::from_row_major(request.output_width, request.output_height, 4,
                        &vec![*plane.pixel(0, 0); count]).unwrap() },
                PreparedExemplarChannel::Normal { plane, source_convention, canonical_convention, alpha_policy } => PreparedExemplarChannel::Normal {
                    plane: ImagePlane::from_row_major(request.output_width, request.output_height, 4,
                        &vec![*plane.pixel(0, 0); count]).unwrap(), source_convention: *source_convention,
                    canonical_convention: *canonical_convention, alpha_policy: *alpha_policy },
                PreparedExemplarChannel::MaterialId { plane } => PreparedExemplarChannel::MaterialId {
                    plane: ImagePlane::from_row_major(request.output_width, request.output_height, 4,
                        &vec![*plane.pixel(0, 0); count]).unwrap() },
                PreparedExemplarChannel::Mask { role, plane } => PreparedExemplarChannel::Mask { role: *role,
                    plane: ImagePlane::from_row_major(request.output_width, request.output_height, 4,
                        &vec![*plane.pixel(0, 0); count]).unwrap() },
            }).collect::<Vec<_>>();
            Ok(LearnedMaterialOutput { output_digest: canonical_learned_output_digest(&channels), channels,
                confidence_milli: 800, model_digest: ContentDigest::sha256(b"approved-test-model"),
                deterministic: true, device: self.device, diagnostics: vec!["fixture provider".into()] })
        }
    }

    #[test]
    fn algorithm_stage_08_router_domain_route_goldens() {
        let cancellation = RenderCancellationToken::new();
        for behavior in hot_trimmer_domain::MaterialBehaviorClass::ALL {
            let request = router_fixture(behavior);
            let first = prepare_stage_08_material_domain(&request, None, &mut MaterialDomainCache::default(), &cancellation);
            match first {
                Ok(result) => {
                    let allowed: &[DomainRoute] = match behavior {
                        hot_trimmer_domain::MaterialBehaviorClass::AlreadyTileable => &[DomainRoute::DirectSource, DomainRoute::GraphCutPeriodicClosure],
                        hot_trimmer_domain::MaterialBehaviorClass::StochasticIsotropic => &[DomainRoute::TextureQuilting, DomainRoute::StatisticalSynthesis, DomainRoute::ProceduralReconstruction],
                        hot_trimmer_domain::MaterialBehaviorClass::StochasticDirectional => &[DomainRoute::TextureQuilting, DomainRoute::ProceduralReconstruction],
                        hot_trimmer_domain::MaterialBehaviorClass::LayeredBanded => &[DomainRoute::ProceduralReconstruction],
                        hot_trimmer_domain::MaterialBehaviorClass::OrganicDirectional => &[DomainRoute::TextureQuilting, DomainRoute::ProceduralReconstruction],
                        hot_trimmer_domain::MaterialBehaviorClass::UniqueDetail | hot_trimmer_domain::MaterialBehaviorClass::RadialDetail => &[DomainRoute::DirectSource],
                        hot_trimmer_domain::MaterialBehaviorClass::MixedUnknown => &[DomainRoute::GraphCutPeriodicClosure, DomainRoute::TextureQuilting, DomainRoute::PatchMatch],
                        hot_trimmer_domain::MaterialBehaviorClass::PeriodicLatticeStructured
                        | hot_trimmer_domain::MaterialBehaviorClass::ManufacturedPattern => panic!("structured fixture without period evidence must be insufficient"),
                    };
                    assert!(allowed.contains(&result.domain.route), "unexpected {behavior:?} route {:?}", result.domain.route);
                    if behavior == hot_trimmer_domain::MaterialBehaviorClass::AlreadyTileable {
                        assert_eq!(result.domain.route, DomainRoute::GraphCutPeriodicClosure,
                            "one discontinuous registered channel must reject direct periodic publication");
                    }
                    assert!(result.diagnostics.registration_valid && result.diagnostics.seam_or_period_valid
                        && result.diagnostics.correspondence_valid && result.diagnostics.deterministic
                        && result.diagnostics.cache_provenance_valid);
                    let replay = prepare_stage_08_material_domain(&request, None, &mut MaterialDomainCache::default(), &cancellation).unwrap();
                    assert_eq!(result.domain, replay.domain, "{behavior:?} must replay byte-identically");
                }
                Err(Stage8RouterError::ActionableInsufficiency(diagnostics)) => {
                    assert!(matches!(behavior, hot_trimmer_domain::MaterialBehaviorClass::PeriodicLatticeStructured
                        | hot_trimmer_domain::MaterialBehaviorClass::ManufacturedPattern
                        | hot_trimmer_domain::MaterialBehaviorClass::StochasticDirectional
                        | hot_trimmer_domain::MaterialBehaviorClass::LayeredBanded
                        | hot_trimmer_domain::MaterialBehaviorClass::OrganicDirectional));
                    assert!(!diagnostics.messages.is_empty());
                    if matches!(behavior, hot_trimmer_domain::MaterialBehaviorClass::PeriodicLatticeStructured
                        | hot_trimmer_domain::MaterialBehaviorClass::ManufacturedPattern) {
                        assert!(diagnostics.compared_routes.iter().any(|route| route.route == DomainRoute::DirectSource
                            && !route.applicable && route.rejection.as_deref().is_some_and(|reason| reason.contains("period"))));
                    }
                }
                Err(error) => panic!("unexpected router contract failure for {behavior:?}: {error}"),
            }
        }

        let mut statistical = router_fixture(hot_trimmer_domain::MaterialBehaviorClass::StochasticIsotropic);
        statistical.policy.pinned_route = Some(DomainRoute::StatisticalSynthesis);
        let statistical_result = prepare_stage_08_material_domain(&statistical, None,
            &mut MaterialDomainCache::default(), &cancellation).unwrap();
        assert_eq!(statistical_result.domain.route, DomainRoute::StatisticalSynthesis);
        let CorrespondenceField::Registered(field) = &statistical_result.domain.correspondence else { panic!("registered statistical field") };
        let coherent = (0..field.height()).flat_map(|y| (0..field.width() - 1).map(move |x| {
            let a = field.pixel(x, y).sources[0].unwrap().coordinate;
            let b = field.pixel(x + 1, y).sources[0].unwrap().coordinate;
            a.x.abs_diff(b.x) <= 3 || a.x.abs_diff(b.x) >= field.width() - 3
        })).filter(|value| *value).count();
        assert!(coherent * 2 > (field.width() as usize - 1) * field.height() as usize,
            "statistical synthesis must retain local neighborhoods, not independently resample pixels");
        let mut transposed = statistical.clone(); transposed.output_width = 10; transposed.output_height = 12;
        let transposed_result = prepare_stage_08_material_domain(&transposed, None,
            &mut MaterialDomainCache::default(), &cancellation).unwrap();
        assert_ne!(statistical_result.domain.cache_key, transposed_result.domain.cache_key,
            "statistical cache provenance must distinguish transposed dimensions");

        let mut pinned = router_fixture(hot_trimmer_domain::MaterialBehaviorClass::StochasticIsotropic);
        pinned.policy.pinned_route = Some(DomainRoute::DirectSource);
        let Err(Stage8RouterError::ActionableInsufficiency(pinned_diagnostics)) = prepare_stage_08_material_domain(
            &pinned, None, &mut MaterialDomainCache::default(), &cancellation) else { panic!("inapplicable pin must fail closed") };
        assert!(pinned_diagnostics.messages[0].contains("fail-closed"));
        assert!(pinned_diagnostics.compared_routes.iter().all(|route| !route.attempted));

        let mut learned = router_fixture(hot_trimmer_domain::MaterialBehaviorClass::RadialDetail);
        learned.policy.source_class_override = Some(MaterialSourceClass::WoodEndGrain);
        learned.policy.pinned_route = Some(DomainRoute::LearnedProvider);
        let provider = TestLearnedProvider { device: LearnedExecutionDevice::Cpu, include_estimated_maps: true };
        let learned_result = prepare_stage_08_material_domain(&learned, Some(&provider),
            &mut MaterialDomainCache::default(), &cancellation).unwrap();
        assert_eq!(learned_result.domain.route, DomainRoute::LearnedProvider);
        assert!(matches!(learned_result.diagnostics.learned_provider, LearnedProviderState::Used { .. }));
        let expansion_only = TestLearnedProvider { device: LearnedExecutionDevice::Cpu, include_estimated_maps: false };
        let expansion_result = prepare_stage_08_material_domain(&learned, Some(&expansion_only),
            &mut MaterialDomainCache::default(), &cancellation).unwrap();
        assert_eq!(expansion_result.domain.route, DomainRoute::LearnedProvider,
            "estimated PBR maps are optional for a seamless-expansion provider");
        let wrong_device = TestLearnedProvider { device: LearnedExecutionDevice::Gpu, include_estimated_maps: true };
        let Err(Stage8RouterError::ActionableInsufficiency(device_diagnostics)) = prepare_stage_08_material_domain(
            &learned, Some(&wrong_device), &mut MaterialDomainCache::default(), &cancellation) else { panic!("device mismatch must fail closed") };
        assert!(device_diagnostics.compared_routes.iter().any(|route| route.route == DomainRoute::LearnedProvider
            && route.rejection.as_deref().is_some_and(|reason| reason.contains("device-policy"))));

        let mut policy = MaterialDomainRoutePolicy::default();
        policy.apply(RoutePolicyCommand::Override(DomainRoute::GraphCutPeriodicClosure));
        policy.apply(RoutePolicyCommand::Pin(DomainRoute::DirectSource));
        policy.apply(RoutePolicyCommand::ResetRoute);
        assert_eq!(policy, MaterialDomainRoutePolicy::default());
    }
}
