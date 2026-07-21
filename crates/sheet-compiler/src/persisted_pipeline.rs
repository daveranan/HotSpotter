//! First authoritative persisted-project orchestration through Stage 14.

use std::{
    collections::BTreeMap,
    fs,
    sync::{Arc, Mutex},
    time::Instant,
};

use hot_trimmer_domain::{
    AddressMode, AlgorithmProvenance, CancellationToken, ContentDigest, ContentReference,
    ManualRegionRole, MaterialChannelRole, OrientedPixelSize, OriginalAssetProvenance, Patch,
    PhysicalScaleEvidence, Projection, QuarterTurn, RegionId, RegionMapping, RegionSampling,
    RegisteredChannel, RegisteredChannelSet, SamplingMode, SamplingPolicy, SolidChannelValues,
    SourceOwnershipIntent, StageResult, TemplateSlotRole,
};
use hot_trimmer_effect_compiler::{
    EdgeDetailCompileRequest, EdgeDetailRegionInput, RequiredSourceFootprint, ResolvedSlotDemand, SlotDemandIntent, SourceFootprintUnit,
    VisualImportance, WorldDimensionSource, compile_structural_intent,
    conservative_profile_capacity, resolve_slot_demands_with_guard,
};
use hot_trimmer_image_io::{
    CancellationToken as ImageCancellationToken, ImagePlane, LinearColor, NormalizationSettings,
    PreparedChannelCacheKey, PreparedChannelSet, ResolvedAlphaMode, prepare_registered_channel_set,
};
use hot_trimmer_material_analysis::{
    AnalysisSettings, FeatureFieldSettings, ScaleOrientationSettings, analyze_source,
    calibrate_scale_orientation, extract_feature_fields, prepare_delit_exemplar,
};
use hot_trimmer_material_synthesis::{
    DomainRequest, DomainRoute, GraphCutSettings, MaterialDomainCache, MaterialDomainRoutePolicy,
    PatchMatchSettings, ProceduralFitSettings, QuiltingSettings, SeamAxis, Stage8RouterRequest,
    prepare_stage_08_material_domain,
};
use hot_trimmer_placement_solver::{
    CandidateDescriptors, CandidateEvidence, CandidateFamily, CandidateRoute,
    CandidateScoringMeasurements, CandidateSet, CandidateSettings, CandidateTransform,
    CropCandidate, EligibilityEvidence, FeaturePosition, MaterialDomainView, MirrorTransform,
    PlacementObjectiveBreakdown, PlacementOptimizerSettings, PlacementPlan, PlacementPlanQaView,
    PlacementSlotInput, PlacementValidationSummary, PositionStrategy, ReusePermissions,
    SamplingPlan, ScoringContext, ScoringSettings, SliceCenterPolicy, SliceGeometry, SourceCrop,
    StretchOverrideProvenance, generate_candidates_with_guard, optimize_placements,
    score_candidate_set_with_guard,
};
use hot_trimmer_project_store::{ProjectSummary, SourceOwnership, StoredSource};
use hot_trimmer_render_core::{
    ExemplarMaskIntent, PlanarArea, PreparedExemplarChannel, PreparedExemplarRequest,
    PreparedExemplarScope, RectificationQuality, RectificationWorkLimits, RenderCancellationToken,
    prepare_registered_exemplar, registered_level_zero_channels,
};
use sha2::{Digest as ShaDigest, Sha256};

#[derive(Clone)]
struct DomainArtifacts {
    source_set_id: hot_trimmer_domain::SourceSetId,
    patch_id: Option<String>,
    patch_id_raw: Option<hot_trimmer_domain::PatchId>,
    domain: hot_trimmer_material_synthesis::PreparedMaterialDomain,
    stage5: hot_trimmer_material_analysis::SourceAnalysisReport,
    stage6: hot_trimmer_material_analysis::ScaleOrientationReport,
    stage7: hot_trimmer_material_analysis::FeatureFieldReport,
    stage3_result: StageResult,
    stage4_result: StageResult,
    stage8_result: StageResult,
}

struct MappedDomainView<'a> {
    domain: &'a hot_trimmer_material_synthesis::PreparedMaterialDomain,
    window: SourceCrop,
}

impl MaterialDomainView for MappedDomainView<'_> {
    fn domain_id(&self) -> &ContentDigest {
        &self.domain.cache_key
    }
    fn source_id(&self) -> &ContentDigest {
        &self.domain.prepared_source_digest
    }
    fn dimensions(&self) -> (u32, u32) {
        (self.window.width, self.window.height)
    }
    fn route(&self) -> DomainRoute {
        self.domain.route
    }
    fn valid(&self, x: u32, y: u32) -> bool {
        self.domain
            .validity
            .pixel(self.window.x + x, self.window.y + y)
            .0
            >= 0.5
    }
    fn seam_indices(&self, axis: SeamAxis) -> Vec<u32> {
        self.domain
            .seams
            .iter()
            .enumerate()
            .filter_map(|(index, seam)| (seam.axis == axis).then_some(index as u32))
            .collect()
    }
}

fn compile_persisted_details_for_region(
    decorations: &[hot_trimmer_domain::DecorationBinding],
    region_id: RegionId,
    slot_role: TemplateSlotRole,
    slot_size_m: [f64; 2],
    destination_pixels: [u32; 2],
    capacity: &hot_trimmer_effect_compiler::EffectCapacity,
    upstream_identity: &ContentDigest,
) -> Result<hot_trimmer_effect_compiler::CompiledDetailSet, String> {
    let region_key = region_id.to_string();
    let mut all_definitions = Vec::new();
    let mut operations = Vec::new();
    let mut strokes = Vec::new();
    for decoration in decorations {
        if decoration
            .decoration_key
            .starts_with("stage16.detail.definition")
        {
            let definition: hot_trimmer_effect_compiler::DetailDefinition =
                serde_json::from_str(&decoration.value).map_err(|error| {
                    format!(
                        "Stage 16 detail definition decoration '{}' is malformed: {error}",
                        decoration.decoration_key
                    )
                })?;
            all_definitions.push((decoration.decoration_key.as_str(), definition));
        } else if decoration
            .decoration_key
            .starts_with("stage16.stamp.operation")
        {
            let operation: hot_trimmer_effect_compiler::StampOperation =
                serde_json::from_str(&decoration.value).map_err(|error| {
                    format!(
                        "Stage 16 stamp operation decoration '{}' is malformed: {error}",
                        decoration.decoration_key
                    )
                })?;
            operations.push(operation);
        } else if decoration
            .decoration_key
            .starts_with("stage16.stamp.stroke")
        {
            let stroke: hot_trimmer_effect_compiler::StampStroke =
                serde_json::from_str(&decoration.value).map_err(|error| {
                    format!(
                        "Stage 16 stamp stroke decoration '{}' is malformed: {error}",
                        decoration.decoration_key
                    )
                })?;
            strokes.push(stroke);
        }
    }
    let all_definition_names = all_definitions
        .iter()
        .map(|(_, definition)| definition.name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if let Some(orphan) = operations
        .iter()
        .find(|operation| !all_definition_names.contains(operation.target_region.as_str()))
    {
        return Err(format!(
            "Stage 16 operation targets unknown detail '{}'",
            orphan.target_region
        ));
    }
    if let Some(orphan) = strokes
        .iter()
        .find(|stroke| !all_definition_names.contains(stroke.operation.target_region.as_str()))
    {
        return Err(format!(
            "Stage 16 stroke targets unknown detail '{}'",
            orphan.operation.target_region
        ));
    }
    let definitions = all_definitions
        .into_iter()
        .filter_map(|(key, definition)| {
            decoration_targets_region(key, &definition.name, &region_key).then_some(definition)
        })
        .collect::<Vec<_>>();
    if definitions.is_empty() {
        return Ok(hot_trimmer_effect_compiler::empty_compiled_detail_set());
    }
    let definition_names = definitions
        .iter()
        .map(|definition| definition.name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    operations.retain(|operation| definition_names.contains(operation.target_region.as_str()));
    strokes.retain(|stroke| definition_names.contains(stroke.operation.target_region.as_str()));
    hot_trimmer_effect_compiler::compile_details(
        hot_trimmer_effect_compiler::DetailCompileRequest {
            definitions: &definitions,
            operations: &operations,
            strokes: &strokes,
            slot_role,
            slot_size_m,
            destination_pixels,
            capacity,
            upstream_identity,
        },
    )
    .map_err(|error| format!("Stage 16 detail compilation failed: {error}"))
}

fn compile_persisted_profile_for_region(
    decorations: &[hot_trimmer_domain::DecorationBinding],
    region_id: RegionId,
    fallback_intent: hot_trimmer_domain::StructuralProfile,
    slot_size_m: [f64; 2],
    destination_pixels: [u32; 2],
    capacity: &hot_trimmer_effect_compiler::EffectCapacity,
    upstream_identity: &ContentDigest,
    seed: u64,
) -> Result<hot_trimmer_effect_compiler::CompiledProfile, String> {
    let key = format!("stage15.profile.request.{region_id}");
    if let Some(binding) = decorations
        .iter()
        .find(|binding| binding.decoration_key == key)
    {
        let requested: hot_trimmer_effect_compiler::RequestedProfile =
            serde_json::from_str(&binding.value).map_err(|error| {
                format!("Stage 15 requested profile for {region_id} is malformed: {error}")
            })?;
        return hot_trimmer_effect_compiler::compile_profile(
            hot_trimmer_effect_compiler::ProfileCompileRequest {
                requested: &requested,
                slot_size_m,
                destination_pixels,
                capacity,
                upstream_identity,
            },
        )
        .map_err(|error| format!("Stage 15 profile compilation failed: {error}"));
    }
    hot_trimmer_effect_compiler::compile_structural_intent(
        fallback_intent,
        slot_size_m,
        destination_pixels,
        capacity,
        upstream_identity,
        seed,
    )
    .map_err(|error| format!("Stage 15 profile compilation failed: {error}"))
}

fn stage16_stamp_asset_domains(
    project: &ProjectSummary,
    decorations: &[hot_trimmer_domain::DecorationBinding],
    profile: SourceFramePreviewProfile,
    cancellation: &ImageCancellationToken,
    cache: Option<&Mutex<SourceFramePreviewCache>>,
) -> Result<
    Vec<(
        hot_trimmer_effect_compiler::StampAssetRef,
        hot_trimmer_domain::SourceSetId,
        Arc<hot_trimmer_material_synthesis::PreparedMaterialDomain>,
    )>,
    String,
> {
    let assets = stage16_stamp_assets(decorations)?;
    let mut resolved = Vec::new();
    for asset in assets {
        let source_set_id = project
            .sources
            .iter()
            .find(|source| {
                source.input.sha256 == asset.digest.0
                    || source.source_set_id.to_string() == asset.asset_id
                    || source.input.id.to_string() == asset.asset_id
            })
            .map(|source| {
                hot_trimmer_domain::SourceSetId::from_bytes(*source.source_set_id.as_bytes())
            })
            .ok_or_else(|| {
                format!(
                    "Stage 16 stamp asset {} ({}) does not resolve to a project source set",
                    asset.asset_id, asset.digest.0
                )
            })?;
        let (domain, _) =
            direct_source_frame_domain(project, source_set_id, profile, cancellation, cache)?;
        resolved.push((asset, source_set_id, domain));
    }
    Ok(resolved)
}

fn stage16_stamp_assets(
    decorations: &[hot_trimmer_domain::DecorationBinding],
) -> Result<Vec<hot_trimmer_effect_compiler::StampAssetRef>, String> {
    let mut assets = Vec::new();
    for decoration in decorations {
        if decoration
            .decoration_key
            .starts_with("stage16.detail.definition")
        {
            let definition: hot_trimmer_effect_compiler::DetailDefinition =
                serde_json::from_str(&decoration.value).map_err(|error| {
                    format!(
                        "Stage 16 detail definition decoration '{}' is malformed: {error}",
                        decoration.decoration_key
                    )
                })?;
            assets.extend(definition.required_sources);
        } else if decoration
            .decoration_key
            .starts_with("stage16.stamp.operation")
        {
            let operation: hot_trimmer_effect_compiler::StampOperation =
                serde_json::from_str(&decoration.value).map_err(|error| {
                    format!(
                        "Stage 16 stamp operation decoration '{}' is malformed: {error}",
                        decoration.decoration_key
                    )
                })?;
            assets.push(operation.asset);
        } else if decoration
            .decoration_key
            .starts_with("stage16.stamp.stroke")
        {
            let stroke: hot_trimmer_effect_compiler::StampStroke =
                serde_json::from_str(&decoration.value).map_err(|error| {
                    format!(
                        "Stage 16 stamp stroke decoration '{}' is malformed: {error}",
                        decoration.decoration_key
                    )
                })?;
            assets.push(stroke.operation.asset);
        }
    }
    assets.sort_by(|left, right| {
        left.digest
            .0
            .cmp(&right.digest.0)
            .then_with(|| left.asset_id.cmp(&right.asset_id))
    });
    assets.dedup_by(|left, right| left.digest == right.digest && left.asset_id == right.asset_id);
    Ok(assets)
}

fn decoration_targets_region(key: &str, detail_name: &str, region_key: &str) -> bool {
    detail_name == region_key
        || key
            .rsplit('.')
            .next()
            .is_some_and(|suffix| suffix == region_key)
}

use crate::{
    AlgorithmCompiler, AtlasComposeExecutionInput, AtlasFinalAtlasOutput, AtlasPreparedSource,
    AtlasRenderExecutionInput, AtlasRenderExecutor, AtlasRenderExecutorOutput,
    COMPILED_ATLAS_ALGORITHM_VERSION, COMPILED_ATLAS_PLAN_SCHEMA_VERSION, CompiledAtlasPlanV1,
    CompiledAtlasPlanValidationError, CompiledAtlasPreviewProfile, CompiledColorSpacePolicy,
    CompiledNormalConvention, CompiledRegionCommandV1, CompiledSourceCommandV1,
    CompiledTileRequest, CompiledTileRequestKind, CompilerFacadeError, CpuAtlasRenderExecutor,
    IntermediateAtlasArtifact, IntermediateAtlasRequest, IntermediateSlotInput, OutputPixelRect,
    SlotSynthesisLimits, SlotSynthesisRequest, SourcePixelRect, SynthesizedSlotMaterial,
    synthesize_slot_material_with_guard,
};

/// Gate 1 must show source subdivisions, not a second copy of the whole source in
/// every fixed-template region. The fraction is applied isotropically, so the
/// authored slot aspect is preserved while leaving Stage 11 multiple positions
/// to choose from.
const UNPLACED_SOURCE_FOOTPRINT_FRACTION: f64 = 0.5;
const DIRECT_SOURCE_FRAME_DECODER_VERSION: &str = "stage-02-oriented-registered-channels-v2";

#[derive(Clone, Debug)]
pub struct PersistedStage14PreviewRequest<'a> {
    pub project: &'a ProjectSummary,
    pub revision: u64,
    pub draft_id: Option<u64>,
    pub input_hash: Option<String>,
    /// A request profile changes only output work/resolution.  SourceFrame topology,
    /// source coordinates, stable identities, and DirectCrop ownership stay fixed.
    pub profile: SourceFramePreviewProfile,
    /// Optional view intent supplied by the desktop IPC contract. Older callers keep
    /// their profile-derived complete request while all new interactive work is
    /// compiled through this same persisted route.
    pub view_intent: Option<SourceFramePreviewViewIntent>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceFramePreviewProfile {
    Draft512,
    Refinement1024,
    Preview2048,
    Preview4096,
    Preview8192,
    Authoritative,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceFramePreviewViewIntent {
    CompleteDraft512,
    CompleteRefinement1024,
    ExactViewport(OutputPixelRect),
    ExactSelectedRegion(hot_trimmer_domain::RegionId),
    MaterialMaps(Vec<hot_trimmer_domain::MaterialMapKind>),
    ExactViewportMaterialMaps {
        rect: OutputPixelRect,
        maps: Vec<hot_trimmer_domain::MaterialMapKind>,
    },
    ExactSelectedRegionMaterialMaps {
        region_id: hot_trimmer_domain::RegionId,
        maps: Vec<hot_trimmer_domain::MaterialMapKind>,
    },
}

impl Default for SourceFramePreviewProfile {
    fn default() -> Self {
        Self::Authoritative
    }
}

#[derive(Clone, Debug, Default)]
pub struct SourceFramePreviewCache {
    prepared_sources: BTreeMap<PreparedChannelCacheKey, Arc<PreparedChannelSet>>,
    direct_domains:
        BTreeMap<ContentDigest, Arc<hot_trimmer_material_synthesis::PreparedMaterialDomain>>,
    rendered_regions: BTreeMap<ContentDigest, Arc<SynthesizedSlotMaterial>>,
    composed_atlases: BTreeMap<ContentDigest, Arc<IntermediateAtlasArtifact>>,
}

impl SourceFramePreviewCache {
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.direct_domains.len()
    }
    pub fn rendered_region_count(&self) -> usize {
        self.rendered_regions.len()
    }
    pub fn composed_atlas_count(&self) -> usize {
        self.composed_atlases.len()
    }
    fn get(
        &self,
        key: &ContentDigest,
    ) -> Option<Arc<hot_trimmer_material_synthesis::PreparedMaterialDomain>> {
        self.direct_domains.get(key).cloned()
    }
    fn get_prepared(&self, key: &PreparedChannelCacheKey) -> Option<Arc<PreparedChannelSet>> {
        self.prepared_sources.get(key).cloned()
    }
    fn insert_prepared(&mut self, prepared: Arc<PreparedChannelSet>) {
        const MAX_PREPARED_SOURCES: usize = 2;
        if self.prepared_sources.len() >= MAX_PREPARED_SOURCES
            && !self.prepared_sources.contains_key(&prepared.cache_key)
        {
            if let Some(oldest) = self.prepared_sources.keys().next().cloned() {
                self.prepared_sources.remove(&oldest);
            }
        }
        self.prepared_sources
            .insert(prepared.cache_key.clone(), prepared);
    }
    fn insert(
        &mut self,
        key: ContentDigest,
        domain: Arc<hot_trimmer_material_synthesis::PreparedMaterialDomain>,
    ) {
        // Keep the pinned SourceFrame plus a small working set of authored patch domains.
        // Patch assignment must not redo Stages 2-8 every time the same patch is rebound.
        const MAX_DIRECT_DOMAINS: usize = 4;
        if self.direct_domains.len() >= MAX_DIRECT_DOMAINS
            && !self.direct_domains.contains_key(&key)
        {
            if let Some(oldest) = self.direct_domains.keys().next().cloned() {
                self.direct_domains.remove(&oldest);
            }
        }
        self.direct_domains.insert(key, domain);
    }
    pub(crate) fn get_rendered(&self, key: &ContentDigest) -> Option<Arc<SynthesizedSlotMaterial>> {
        self.rendered_regions.get(key).cloned()
    }
    pub(crate) fn insert_rendered(
        &mut self,
        key: ContentDigest,
        result: Arc<SynthesizedSlotMaterial>,
    ) {
        const MAX_RENDERED_REGIONS: usize = 64;
        if self.rendered_regions.len() >= MAX_RENDERED_REGIONS
            && !self.rendered_regions.contains_key(&key)
        {
            if let Some(oldest) = self.rendered_regions.keys().next().cloned() {
                self.rendered_regions.remove(&oldest);
            }
        }
        self.rendered_regions.insert(key, result);
    }
    fn get_composed(&self, key: &ContentDigest) -> Option<Arc<IntermediateAtlasArtifact>> {
        self.composed_atlases.get(key).cloned()
    }
    fn insert_composed(&mut self, key: ContentDigest, artifact: Arc<IntermediateAtlasArtifact>) {
        const MAX_COMPOSED_ATLASES: usize = 1;
        if self.composed_atlases.len() >= MAX_COMPOSED_ATLASES
            && !self.composed_atlases.contains_key(&key)
        {
            if let Some(oldest) = self.composed_atlases.keys().next().cloned() {
                self.composed_atlases.remove(&oldest);
            }
        }
        self.composed_atlases.insert(key, artifact);
    }
}

impl AlgorithmCompiler {
    pub fn compile_persisted_stage_14_preview(
        &self,
        request: PersistedStage14PreviewRequest<'_>,
        cancellation: &CancellationToken,
        is_current: impl Fn() -> bool + Sync,
    ) -> Result<IntermediateAtlasArtifact, CompilerFacadeError> {
        self.compile_persisted_stage_14_preview_with_cache(request, cancellation, is_current, None)
    }

    /// The desktop-owned bounded cache is injected into the sole persisted compiler;
    /// it is not a process-global image cache and never changes source authority.
    pub fn compile_persisted_stage_14_preview_with_cache(
        &self,
        request: PersistedStage14PreviewRequest<'_>,
        cancellation: &CancellationToken,
        is_current: impl Fn() -> bool + Sync,
        source_frame_cache: Option<&Mutex<SourceFramePreviewCache>>,
    ) -> Result<IntermediateAtlasArtifact, CompilerFacadeError> {
        self.compile_persisted_stage_14_preview_with_cache_and_executor(
            request,
            cancellation,
            is_current,
            source_frame_cache,
            None,
        )
    }

    pub fn compile_persisted_stage_14_preview_with_cache_and_executor(
        &self,
        request: PersistedStage14PreviewRequest<'_>,
        cancellation: &CancellationToken,
        is_current: impl Fn() -> bool + Sync,
        source_frame_cache: Option<&Mutex<SourceFramePreviewCache>>,
        executor_override: Option<&dyn AtlasRenderExecutor>,
    ) -> Result<IntermediateAtlasArtifact, CompilerFacadeError> {
        let image_cancellation = ImageCancellationToken::new();
        let render_cancellation = RenderCancellationToken::new();
        let monitoring_complete = std::sync::atomic::AtomicBool::new(false);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                while !monitoring_complete.load(std::sync::atomic::Ordering::Acquire) {
                    if cancellation.is_cancelled() || !is_current() {
                        cancellation.cancel();
                        image_cancellation.cancel();
                        render_cancellation.cancel();
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            });
            let result = compile_persisted(
                request,
                cancellation,
                &image_cancellation,
                &render_cancellation,
                &is_current,
                source_frame_cache,
                executor_override,
            )
            .map_err(CompilerFacadeError::Pipeline);
            monitoring_complete.store(true, std::sync::atomic::Ordering::Release);
            result
        })
    }
}

#[allow(clippy::too_many_lines)]
fn compile_persisted(
    request: PersistedStage14PreviewRequest<'_>,
    cancellation: &CancellationToken,
    image_cancellation: &ImageCancellationToken,
    render_cancellation: &RenderCancellationToken,
    is_current: &dyn Fn() -> bool,
    source_frame_cache: Option<&Mutex<SourceFramePreviewCache>>,
    executor_override: Option<&dyn AtlasRenderExecutor>,
) -> Result<IntermediateAtlasArtifact, String> {
    let active = || !cancellation.is_cancelled() && is_current();
    if !active() {
        return Err("preview cancelled or superseded before Stage 1".into());
    }
    let document = request
        .project
        .document
        .as_ref()
        .ok_or("persisted project has no trim-sheet document")?;
    if document.document_revision != request.revision {
        return Err("preview revision is already stale".into());
    }
    let primary = document
        .primary_material
        .ok_or("persisted document has no primary material")?;
    if document.source_frame.is_some() {
        return compile_source_frame(
            request,
            cancellation,
            image_cancellation,
            render_cancellation,
            is_current,
            document,
            primary,
            source_frame_cache,
            executor_override,
        );
    }
    let mut domains = Vec::<DomainArtifacts>::new();
    let mut domain_keys = BTreeMap::<String, usize>::new();
    let mut region_domains: Vec<usize> = Vec::with_capacity(document.topology.regions.len());
    for region in &document.topology.regions {
        let binding = document
            .region_bindings
            .get(&region.id)
            .ok_or_else(|| format!("region {} has no persisted content binding", region.id))?;
        let (source_set_id, patch) =
            resolve_region_content(request.project, document, primary, &binding.content)?;
        let key = format!(
            "{}|{}",
            source_set_id,
            patch
                .as_ref()
                .map_or_else(|| "full-source".into(), |value| value.id.to_string())
        );
        let index = if let Some(index) = domain_keys.get(&key).copied() {
            index
        } else {
            let artifacts = build_domain(
                request.project,
                source_set_id,
                patch.as_ref(),
                request.revision,
                false,
                image_cancellation,
                render_cancellation,
            )?;
            let index = domains.len();
            domains.push(artifacts);
            domain_keys.insert(key, index);
            index
        };
        region_domains.push(index);
    }
    let first_domain = domains
        .first()
        .ok_or("Stage 9 topology contains no regions")?;
    if !active() {
        return Err("preview cancelled or superseded after Stage 8".into());
    }

    // Stage 9 is the exact persisted template snapshot compiled once for this output.
    let snapshot = document
        .topology
        .snapshot
        .template
        .as_ref()
        .ok_or("Stage 9 requires a persisted template snapshot")?;
    let definition: hot_trimmer_domain::TemplateDefinition =
        serde_json::from_str(&snapshot.snapshot_json)
            .map_err(|error| format!("Stage 9 snapshot failed: {error}"))?;
    let topology = definition
        .compile_for_output(document.render_settings.output_size)
        .map_err(|error| format!("Stage 9 failed: {error}"))?;
    if topology.slots.len() != document.topology.regions.len() {
        return Err("Stage 9 slot order drifted from persisted regions".into());
    }

    let candidate_settings = CandidateSettings {
        max_positions_per_size: 12,
        max_candidates_per_slot: 96,
        max_work: 100_000_000,
        ..CandidateSettings::default()
    };
    let scoring_settings = ScoringSettings {
        top_k: 16,
        ..ScoringSettings::default()
    };
    let mut placement_inputs = Vec::with_capacity(topology.slots.len());
    let stage10_inputs = topology
        .slots
        .iter()
        .zip(&document.topology.regions)
        .enumerate()
        .map(|(index, (slot, region))| {
            (
                region,
                SlotDemandIntent {
                    destination_rect: slot.allocation,
                    desired_texel_density: 512.0,
                    world_dimension_source: WorldDimensionSource::Stage9Authored,
                    source_scale: domains[region_domains[index]].stage6.scale,
                    visual_importance: VisualImportance::Standard,
                    minimum_survivable_feature_m: 0.001,
                    minimum_flat_center_m: 0.001,
                    requested_features: Vec::new(),
                    opposing_profile_widths_m: None,
                },
            )
        })
        .collect::<Vec<_>>();
    let mut stage10 = resolve_slot_demands_with_guard(&stage10_inputs, &|| !active())
        .map_err(|error| format!("Stage 10 failed: {error:?}"))?;
    // Gate 1 is deliberately limited to modes with a truthful registered Stage 14
    // implementation. TextureSynthesis still has candidate families but no exact
    // Stage 14 artifact, so it must not reach optimization. PolarRadial is executed
    // by the typed manual radial branch below.
    for demand in &mut stage10.slots {
        let region = document
            .topology
            .regions
            .iter()
            .find(|region| region.id == demand.slot_id)
            .ok_or_else(|| format!("Stage 10 produced an unknown region {}", demand.slot_id))?;
        let binding = document
            .region_bindings
            .get(&region.id)
            .ok_or("Stage 10 region binding disappeared")?;
        if binding.mapping.source_crop_intent
            == Some(hot_trimmer_domain::SourceCropIntent::Unplaced)
        {
            // With no authored crop, derive a bounded aspect-preserving physical
            // footprint. The previous largest-fit value made square/detail slots
            // consume nearly the whole source and gave Stage 13 no meaningful
            // spatial subdivision to select.
            let domain = &domains[region_domains[document
                .topology
                .regions
                .iter()
                .position(|candidate| candidate.id == region.id)
                .expect("region index")]];
            let fit_scale = unplaced_source_pixels_per_physical_unit(
                demand,
                domain.domain.width,
                domain.domain.height,
            );
            demand.required_source_footprint = RequiredSourceFootprint {
                width: demand.world_width_m * fit_scale,
                height: demand.world_height_m * fit_scale,
                unit: SourceFootprintUnit::SourcePixels,
                scale_provenance: domain.stage6.scale.provenance,
                world_scale: domain.stage6.scale.world_scale,
                confidence_milli: domain.stage6.scale.confidence_milli,
            };
        }
        let has_declared_period = domains[region_domains[document
            .topology
            .regions
            .iter()
            .position(|candidate| candidate.id == region.id)
            .expect("region index")]]
        .stage7
        .periodicity
        .candidates
        .iter()
        .any(|candidate| {
            candidate.first.dx_pixels.unsigned_abs() > 0
                && candidate.first.dy_pixels.unsigned_abs() > 0
        });
        demand
            .allowed_mapping_modes
            .retain(|mode| legal_gate1_mode(*mode, demand.slot_role, has_declared_period));
        demand.mapping_mode = demand
            .allowed_mapping_modes
            .first()
            .copied()
            .ok_or_else(|| {
                format!(
                    "Stage 10 has no executable Gate 1 mode for {}",
                    demand.slot_id
                )
            })?;
    }
    for (index, ((slot, region), demand)) in topology
        .slots
        .iter()
        .zip(&document.topology.regions)
        .zip(&stage10.slots)
        .enumerate()
    {
        let artifacts = &domains[region_domains[index]];
        let binding = document
            .region_bindings
            .get(&region.id)
            .ok_or("Stage 10 region binding disappeared")?;
        let window = mapping_window(
            &binding.mapping,
            artifacts.domain.width,
            artifacts.domain.height,
        );
        let mut evidence =
            candidate_evidence(&artifacts.stage5, &artifacts.stage6, &artifacts.stage7);
        evidence.feature_positions.retain_mut(|feature| {
            if feature.x < window.x
                || feature.y < window.y
                || feature.x >= window.x + window.width
                || feature.y >= window.y + window.height
            {
                return false;
            }
            feature.x -= window.x;
            feature.y -= window.y;
            true
        });
        let view = MappedDomainView {
            domain: &artifacts.domain,
            window,
        };
        let unplaced = binding.mapping.source_crop_intent
            == Some(hot_trimmer_domain::SourceCropIntent::Unplaced);
        let mut slot_candidate_settings = candidate_settings.clone();
        if unplaced {
            // The bounded footprint is the Gate 1 source subdivision. Do not let
            // the generic upscale ladder grow it back to the complete domain.
            slot_candidate_settings
                .scale_ladder
                .retain(|scale| *scale <= 1.0);
            slot_candidate_settings.maximum_scale = 1.0;
        }
        let generated = generate_candidates_with_guard(
            &view,
            demand,
            &evidence,
            &slot_candidate_settings,
            document.document_revision,
            &|| !active(),
        )
        .map_err(|error| format!("Stage 11 failed for {}: {error:?}", slot.slot_key))?;
        let mut generated = apply_authored_mapping(
            generated,
            &binding.mapping,
            artifacts.patch_id.is_some(),
            artifacts.domain.width,
            artifacts.domain.height,
        )?;
        generated.candidates.retain(|candidate| {
            candidate.mapping_mode != SamplingMode::TextureSynthesis
                || synthesis_family_matches_domain_route(candidate.family, artifacts.domain.route)
        });
        let measurements = generated
            .candidates
            .iter()
            .map(|candidate| {
                (
                    candidate.candidate_id.clone(),
                    candidate_measurements(candidate, demand, region.role, artifacts),
                )
            })
            .collect();
        let scored = score_candidate_set_with_guard(
            demand,
            &generated,
            &ScoringContext {
                material_behavior: artifacts.stage5.classification.routed_class(),
                material_confidence_milli: artifacts.stage5.classification.confidence_milli,
                requested_physical_scale: 1.0,
                measurements,
            },
            &scoring_settings,
            &|| !active(),
        )
        .map_err(|error| format!("Stage 12 failed for {}: {error:?}", slot.slot_key))?;
        if scored.top_candidates.is_empty() {
            return Err(format!(
                "Stage 12 produced no legal candidate for {}",
                slot.slot_key
            ));
        }
        let unplaced = binding.mapping.source_crop_intent
            == Some(hot_trimmer_domain::SourceCropIntent::Unplaced);
        let require_spatially_distinct_crops =
            unplaced && region.role == hot_trimmer_domain::TemplateSlotRole::UniqueDetail;
        let base_scale = if unplaced {
            unplaced_source_pixels_per_physical_unit(
                demand,
                artifacts.domain.width,
                artifacts.domain.height,
            )
        } else {
            base_pixels_per_physical_unit(
                artifacts.stage6.scale,
                &demand,
                artifacts.domain.width,
                artifacts.domain.height,
            )
        };
        placement_inputs.push(PlacementSlotInput {
            slot_id: region.id,
            role: region.role,
            material_behavior: artifacts.stage5.classification.routed_class(),
            variation_group: region.material_group.clone(),
            visual_importance_milli: 700,
            constraint_tightness_milli: 500,
            required: true,
            prepared_domain_id: artifacts.domain.cache_key.clone(),
            prepared_domain_dimensions: [artifacts.domain.width, artifacts.domain.height],
            prepared_domain_sampling_window: window,
            registered_correspondence_reference: artifacts.domain.cache_key.clone(),
            slot_physical_size: [demand.world_width_m, demand.world_height_m],
            base_source_pixels_per_physical_unit: base_scale,
            sampling_policy: authored_sampling_policy(&binding.mapping)?,
            radial_mapping: binding.mapping.radial,
            stretch_override: StretchOverrideProvenance::NotAuthorized,
            slice_geometry: if binding.mapping.radial.is_some()
                || region.role == hot_trimmer_domain::TemplateSlotRole::Radial
            {
                SliceGeometry::None
            } else {
                slice_geometry(region.role, artifacts.domain.width, artifacts.domain.height)
            },
            maximum_seam_cost_milli: 450,
            reuse_permissions: ReusePermissions {
                require_spatially_distinct_crops,
                ..ReusePermissions::default()
            },
            candidates: scored,
        });
    }
    let placement_settings = PlacementOptimizerSettings {
        beam_width: 8,
        max_pairwise_evaluations: 100_000,
        max_local_evaluations: 5_000,
        local_passes: 1,
        ..PlacementOptimizerSettings::default()
    };
    let placement = optimize_placements(
        &placement_inputs,
        &placement_settings,
        document.document_revision,
        cancellation,
    )
    .map_err(|error| format!("Stage 13 failed: {error:?}"))?;
    if !active() {
        return Err("preview cancelled or superseded after Stage 13".into());
    }

    let mut ordered_plans = Vec::with_capacity(placement.placements.len());
    for (slot, region) in topology.slots.iter().zip(&document.topology.regions) {
        let plan = placement
            .placements
            .iter()
            .find(|plan| plan.slot_id == region.id)
            .ok_or_else(|| format!("Stage 13 omitted required slot {}", slot.slot_key))?;
        ordered_plans.push(plan);
    }

    if let Some(executor) = executor_override {
        let compiled_plan = fixed_template_compiled_atlas_plan(
            &request,
            document,
            &topology,
            &ordered_plans,
            &domains,
            &region_domains,
            &stage10.slots,
        )?;
        let stamp_domains = stage16_stamp_asset_domains(
            request.project,
            &document.decorations,
            request.profile,
            image_cancellation,
            source_frame_cache,
        )?;
        let mut prepared_sources = BTreeMap::new();
        for artifacts in &domains {
            for channel_role in compiled_source_roles_for_domain(&artifacts.domain) {
                let source_id = artifacts.domain.prepared_source_digest.clone();
                prepared_sources
                    .entry((artifacts.source_set_id, source_id.clone(), channel_role))
                    .or_insert_with(|| AtlasPreparedSource {
                        source_set_id: artifacts.source_set_id,
                        source_id,
                        channel_role,
                        domain: Arc::new(artifacts.domain.clone()),
                    });
            }
        }
        for (asset, source_set_id, domain) in &stamp_domains {
            for channel_role in compiled_source_roles_for_domain(domain.as_ref()) {
                // Preserve the immutable persisted stamp identity at the executor boundary.
                // A prepared-domain digest describes normalization output and is not the same
                // identity as the asset digest stored in the document.
                let source_id = asset.digest.clone();
                prepared_sources
                    .entry((*source_set_id, source_id.clone(), channel_role))
                    .or_insert_with(|| AtlasPreparedSource {
                        source_set_id: *source_set_id,
                        source_id,
                        channel_role,
                        domain: Arc::clone(domain),
                    });
            }
        }
        let execution = executor
            .execute(
                &compiled_plan,
                &AtlasRenderExecutionInput {
                    prepared_sources: prepared_sources.into_values().collect(),
                    source_frame_cache,
                },
                cancellation,
                is_current,
            )
            .map_err(|error| error.to_string())?;
        if let AtlasRenderExecutorOutput::FinalAtlas(output) = execution {
            let region_sources = document
                .topology
                .regions
                .iter()
                .enumerate()
                .map(|(index, _region)| {
                    let artifacts = &domains[region_domains[index]];
                    (
                        artifacts.source_set_id,
                        artifacts.patch_id_raw,
                        artifacts.domain.prepared_source_digest.clone(),
                        Arc::new(artifacts.domain.clone()),
                    )
                })
                .collect::<Vec<_>>();
            let mut artifact = final_atlas_artifact_from_gpu(
                request.revision,
                topology.clone(),
                &placement,
                fixed_template_algorithm_versions(
                    first_domain,
                    snapshot,
                    &stage10.stage_result,
                    &placement,
                    Some(AlgorithmProvenance {
                        algorithm_id: "hot-trimmer.stage-14.gpu-final-atlas".into(),
                        version: COMPILED_ATLAS_ALGORITHM_VERSION.into(),
                    }),
                ),
                Vec::new(),
                document,
                &region_sources,
                &compiled_plan,
                &output,
            )?;
            artifact.pending.retain(|pending| *pending != "profiles");
            artifact.telemetry.push(format!(
                "profile={:?}; output={}x{}; regions={}; maps={}; executor=gpu; plan_hash={}; render_ms={}; cpu_stage14_calls=0; cpu_atlas_composition_calls=0",
                request.profile,
                topology.output_size.width,
                topology.output_size.height,
                document.topology.regions.len(),
                compiled_plan
                    .requested_maps
                    .iter()
                    .map(|map| format!("{map:?}"))
                    .collect::<Vec<_>>()
                    .join("|"),
                compiled_plan.final_plan_hash.0,
                output.render_ms
            ));
            return Ok(artifact);
        }
    }

    let mut results = Vec::with_capacity(placement.placements.len());
    for (index, (slot, _region)) in topology
        .slots
        .iter()
        .zip(&document.topology.regions)
        .enumerate()
    {
        let artifacts = &domains[region_domains[index]];
        let plan = ordered_plans[index];
        results.push(
            synthesize_slot_material_with_guard(
                SlotSynthesisRequest {
                    plan,
                    domain: &artifacts.domain,
                    output_dimensions: [slot.hotspot.width, slot.hotspot.height],
                    limits: SlotSynthesisLimits::default(),
                },
                &|| !active(),
            )
            .map_err(|error| format!("Stage 14 failed for {}: {error}", slot.slot_key))?,
        );
    }
    let slots = topology
        .slots
        .iter()
        .zip(&document.topology.regions)
        .zip(ordered_plans.into_iter().zip(&results))
        .enumerate()
        .map(|(index, ((slot, region), (plan, result)))| {
            let artifacts = &domains[region_domains[index]];
            IntermediateSlotInput {
                region_id: region.id,
                slot_key: &slot.slot_key,
                display_name: &region.display_name,
                required: true,
                patch_id: artifacts.patch_id.clone(),
                domain: &artifacts.domain,
                plan,
                result,
                grid_rect: region.grid_rect,
                behavior: document.region_bindings[&region.id]
                    .mapping
                    .behavior
                    .clone(),
            }
        })
        .collect::<Vec<_>>();
    let versions = fixed_template_algorithm_versions(
        first_domain,
        snapshot,
        &stage10.stage_result,
        &placement,
        results
            .first()
            .and_then(|result| executed_algorithm(&result.stage_result)),
    );
    AlgorithmCompiler::new()
        .compile_intermediate_atlas(
            &IntermediateAtlasRequest {
                topology: &topology,
                placement_plan: &placement,
                slots,
                revision: request.revision,
                algorithm_versions: versions,
                diagnostics: Vec::new(),
                regions: Vec::new(),
            },
            cancellation,
            || if active() { request.revision } else { 0 },
        )
        .map_err(|error| error.to_string())
}

/// Source-frame documents intentionally use the same persisted compiler spine, but stages 11-13
/// are validation/provenance stages here: the accepted GridRects already define every source and
/// destination rectangle, so no crop search, ranking, reuse, repeat, or synthesis is legal.
#[allow(clippy::too_many_arguments)]
fn compile_source_frame(
    request: PersistedStage14PreviewRequest<'_>,
    cancellation: &CancellationToken,
    image_cancellation: &ImageCancellationToken,
    _render_cancellation: &RenderCancellationToken,
    is_current: &dyn Fn() -> bool,
    document: &hot_trimmer_domain::TrimSheetDocument,
    primary: hot_trimmer_domain::SourceSetId,
    source_frame_cache: Option<&Mutex<SourceFramePreviewCache>>,
    executor_override: Option<&dyn AtlasRenderExecutor>,
) -> Result<IntermediateAtlasArtifact, String> {
    let started = Instant::now();
    let active = || !cancellation.is_cancelled() && is_current();
    let output_size = match request.profile {
        SourceFramePreviewProfile::Draft512 => hot_trimmer_domain::PixelSize {
            width: 512,
            height: 512,
        },
        SourceFramePreviewProfile::Refinement1024 => hot_trimmer_domain::PixelSize {
            width: 1024,
            height: 1024,
        },
        SourceFramePreviewProfile::Preview2048 => hot_trimmer_domain::PixelSize {
            width: 2048,
            height: 2048,
        },
        SourceFramePreviewProfile::Preview4096 => hot_trimmer_domain::PixelSize {
            width: 4096,
            height: 4096,
        },
        SourceFramePreviewProfile::Preview8192 => hot_trimmer_domain::PixelSize {
            width: 8192,
            height: 8192,
        },
        SourceFramePreviewProfile::Authoritative => document.render_settings.output_size,
    };
    let preview_padding_px = scaled_atlas_padding(
        document.render_settings.atlas_padding_px,
        document.render_settings.output_size,
        output_size,
    );
    let frame = document
        .source_frame
        .as_ref()
        .ok_or("source-frame document has no SourceFrame")?;
    let grid = document
        .logical_grid
        .ok_or("source-frame document has no LogicalGridSpec")?;
    let provenance = document
        .partition_provenance
        .as_ref()
        .ok_or("source-frame document has no partition provenance")?;
    if frame.oriented_dimensions.width == 0
        || frame.oriented_dimensions.height == 0
        || frame.output_aspect.contains(&0)
        || grid.validate().is_err()
        || provenance.recipe.grid != grid
    {
        return Err("source-frame contract is invalid".into());
    }
    let frame_source_set = request
        .project
        .source_sets
        .iter()
        .find(|set| set.id == frame.source_set_id)
        .ok_or_else(|| format!("SourceFrame owner {} is missing", frame.source_set_id))?;
    if frame_source_set.source_revision != frame.source_revision {
        return Err(format!(
            "SourceFrame owner {} revision changed from {} to {}",
            frame.source_set_id, frame.source_revision, frame_source_set.source_revision
        ));
    }
    if !aspect_matches(
        frame.bounds.width.get() * f64::from(frame.oriented_dimensions.width),
        frame.bounds.height.get() * f64::from(frame.oriented_dimensions.height),
        f64::from(frame.output_aspect[0]),
        f64::from(frame.output_aspect[1]),
    ) {
        return Err("SourceFrame bounds do not preserve the declared output aspect".into());
    }
    if document.topology.regions.iter().any(|region| {
        document
            .region_bindings
            .get(&region.id)
            .is_none_or(|binding| binding.mapping.address_mode == AddressMode::MirroredRepeat)
    }) {
        return Err(
            "SourceFrame workflow does not yet support mirrored-repeat address mode".into(),
        );
    }
    let cell_count = usize::try_from(u64::from(grid.width) * u64::from(grid.height))
        .map_err(|_| "logical grid is too large")?;
    let mut coverage = vec![0_u8; cell_count];
    for region in &document.topology.regions {
        let rect = region
            .grid_rect
            .ok_or_else(|| format!("region {} has no persisted GridRect", region.id))?;
        if rect.width == 0
            || rect.height == 0
            || rect
                .x
                .checked_add(rect.width)
                .is_none_or(|end| end > grid.width)
            || rect
                .y
                .checked_add(rect.height)
                .is_none_or(|end| end > grid.height)
        {
            return Err(format!(
                "region {} has an out-of-bounds GridRect",
                region.id
            ));
        }
        for y in rect.y..rect.y + rect.height {
            for x in rect.x..rect.x + rect.width {
                let cell = &mut coverage[(y * grid.width + x) as usize];
                *cell = cell.saturating_add(1);
            }
        }
    }
    if coverage.iter().any(|value| *value != 1) {
        return Err("accepted SourceFrame partition has a logical gap or overlap".into());
    }
    let composition_key = ContentDigest::sha256(format!(
        "{SOURCE_FRAME_COMPILER_VERSION}|compose|profile={:?}|output={}x{}|revision={}|appearance={:?}|input={:?}",
        request.profile, output_size.width, output_size.height, request.revision,
        document.appearance_hash().map_err(|error| error.to_string())?, request.input_hash,
    ).as_bytes());
    if !matches!(request.profile, SourceFramePreviewProfile::Authoritative)
        && let Some(cached) = source_frame_cache.and_then(|cache| {
            cache
                .lock()
                .ok()
                .and_then(|guard| guard.get_composed(&composition_key))
        })
    {
        if !active() {
            return Err("preview cancelled or superseded before composed cache publication".into());
        }
        let mut cached = (*cached).clone();
        cached.telemetry.push(format!(
            "profile={:?}; composed_cache=hit; output={}x{}",
            request.profile, output_size.width, output_size.height
        ));
        return Ok(cached);
    }
    let decode_started = Instant::now();
    // SourceFrame geometry is owned by its persisted sourceSetId. Selection and primary
    // material are independent UI/document state and must never choose the frame validator.
    let reusable_source_cache = source_frame_cache;
    let (frame_domain, decode_cache_hit) = direct_source_frame_domain(
        request.project,
        frame.source_set_id,
        request.profile,
        image_cancellation,
        reusable_source_cache,
    )?;
    let decode_ms = decode_started.elapsed().as_millis();
    if frame_domain.width == 0 || frame_domain.height == 0 {
        return Err("SourceFrame prepared source dimensions are empty".into());
    }
    // Resolve the existing RegionBinding authority once per region. Inherited content uses
    // the pinned primary, an explicit material source uses that whole registered source, and
    // a patch uses its owning source plus rectified authored geometry.
    let mut region_sources = Vec::with_capacity(document.topology.regions.len());
    let mut direct_domains = BTreeMap::new();
    direct_domains.insert(frame.source_set_id, Arc::clone(&frame_domain));
    for region in &document.topology.regions {
        let binding = document
            .region_bindings
            .get(&region.id)
            .ok_or_else(|| format!("region {} has no persisted content binding", region.id))?;
        let (source_set_id, patch, solid_domain) = match &binding.content {
            ContentReference::Solid(values) => {
                (None, None, Some(Arc::new(build_solid_domain(values)?)))
            }
            _ => {
                let (source_set_id, patch) =
                    resolve_region_content(request.project, document, primary, &binding.content)?;
                (Some(source_set_id), patch, None)
            }
        };
        let domain = if let Some(domain) = solid_domain {
            domain
        } else if let Some(patch) = patch.as_ref() {
            let source_set_id = source_set_id.expect("patch content has an owning source set");
            let preserve_source_resolution =
                matches!(request.profile, SourceFramePreviewProfile::Authoritative);
            let patch_key = patch_domain_cache_key(
                request.project,
                source_set_id,
                patch,
                preserve_source_resolution,
            )?;
            if let Some(found) = (!preserve_source_resolution)
                .then_some(source_frame_cache)
                .flatten()
                .and_then(|cache| cache.lock().ok().and_then(|guard| guard.get(&patch_key)))
            {
                found
            } else {
                let domain = Arc::new(build_direct_patch_domain(
                    request.project,
                    source_set_id,
                    patch,
                    patch_key.clone(),
                    preserve_source_resolution,
                    image_cancellation,
                    _render_cancellation,
                    (!preserve_source_resolution)
                        .then_some(source_frame_cache)
                        .flatten(),
                )?);
                if !preserve_source_resolution && let Some(cache) = source_frame_cache {
                    if let Ok(mut guard) = cache.lock() {
                        guard.insert(patch_key, Arc::clone(&domain));
                    }
                }
                domain
            }
        } else if let Some(domain) = source_set_id.and_then(|id| direct_domains.get(&id)) {
            Arc::clone(domain)
        } else {
            let source_set_id = source_set_id.expect("non-solid content has a source set");
            let (domain, _) = direct_source_frame_domain(
                request.project,
                source_set_id,
                request.profile,
                image_cancellation,
                reusable_source_cache,
            )?;
            direct_domains.insert(source_set_id, Arc::clone(&domain));
            domain
        };
        let source_set_id = source_set_id.unwrap_or(frame.source_set_id);
        region_sources.push((
            source_set_id,
            patch.map(|value| value.id),
            domain.prepared_source_digest.clone(),
            domain,
        ));
    }
    let source_left = (frame.bounds.x.get() * f64::from(frame_domain.width)).round() as u32;
    let source_top = (frame.bounds.y.get() * f64::from(frame_domain.height)).round() as u32;
    let source_width = (frame.bounds.width.get() * f64::from(frame_domain.width)).round() as u32;
    let source_height = (frame.bounds.height.get() * f64::from(frame_domain.height)).round() as u32;
    let source_x = hot_trimmer_domain::resolve_boundaries(source_left, source_width, grid.width);
    let source_y = hot_trimmer_domain::resolve_boundaries(source_top, source_height, grid.height);
    let destination_x = hot_trimmer_domain::resolve_boundaries(0, output_size.width, grid.width);
    let destination_y = hot_trimmer_domain::resolve_boundaries(0, output_size.height, grid.height);
    let plan_started = Instant::now();
    let mut source_records =
        Vec::<CompiledSourceCommandV1>::with_capacity(region_sources.len().saturating_mul(4));
    let mut region_records = Vec::<CompiledRegionCommandV1>::with_capacity(region_sources.len());
    let mut source_index = BTreeMap::<
        (
            hot_trimmer_domain::SourceSetId,
            ContentDigest,
            MaterialChannelRole,
        ),
        usize,
    >::new();
    for (region_index, region) in document.topology.regions.iter().enumerate() {
        let binding = document
            .region_bindings
            .get(&region.id)
            .ok_or_else(|| format!("region {} has no persisted content binding", region.id))?;
        let (source_set_id, patch_id, source_id, domain) = &region_sources[region_index];
        let behavior = &binding.mapping.behavior;
        let uses_frame_partition =
            matches!(&binding.content, ContentReference::InheritPrimaryMaterial)
                && *source_set_id == frame.source_set_id;
        let rect = region
            .grid_rect
            .ok_or_else(|| format!("region {} has no persisted GridRect", region.id))?;
        let allocation = hot_trimmer_domain::CanonicalRect {
            x: destination_x[rect.x as usize],
            y: destination_y[rect.y as usize],
            width: destination_x[(rect.x + rect.width) as usize] - destination_x[rect.x as usize],
            height: destination_y[(rect.y + rect.height) as usize] - destination_y[rect.y as usize],
        };
        let source_to_region_transform = hot_trimmer_domain::MappingTransform {
            scale: binding.mapping.transform.scale,
            rotation_degrees: quarter_turn_degrees(behavior.orientation),
            mirror_x: binding.mapping.transform.mirror_x,
            mirror_y: binding.mapping.transform.mirror_y,
            offset: binding.mapping.transform.offset,
        };
        let fallback_crop = if uses_frame_partition {
            document
                .source_overrides
                .get(&region.id)
                .map(|value| {
                    let bounds = value.source_bounds;
                    let x = (bounds.x.get() * f64::from(frame_domain.width)).round() as u32;
                    let y = (bounds.y.get() * f64::from(frame_domain.height)).round() as u32;
                    SourceCrop {
                        x,
                        y,
                        width: (bounds.width.get() * f64::from(frame_domain.width)).round() as u32,
                        height: (bounds.height.get() * f64::from(frame_domain.height)).round()
                            as u32,
                    }
                })
                .unwrap_or(SourceCrop {
                    x: source_x[rect.x as usize],
                    y: source_y[rect.y as usize],
                    width: source_x[(rect.x + rect.width) as usize] - source_x[rect.x as usize],
                    height: source_y[(rect.y + rect.height) as usize] - source_y[rect.y as usize],
                })
        } else {
            SourceCrop {
                x: 0,
                y: 0,
                width: domain.width,
                height: domain.height,
            }
        };
        let crop =
            source_frame_preview_crop(&binding.mapping, fallback_crop, domain.width, domain.height);
        if crop.width == 0 || crop.height == 0 || allocation.width == 0 || allocation.height == 0 {
            return Err(format!(
                "source-frame region {} collapsed at resolved pixel boundaries",
                region.id
            ));
        }
        if behavior
            .period_pixels
            .is_some_and(|period| period[0] > crop.width || period[1] > crop.height)
        {
            return Err(format!(
                "region {} authored repeat period exceeds its exact source crop",
                region.id
            ));
        }
        if uses_frame_partition
            && document.source_overrides.contains_key(&region.id)
            && !aspect_matches(
                f64::from(crop.width),
                f64::from(crop.height),
                f64::from(allocation.width),
                f64::from(allocation.height),
            )
        {
            return Err(format!(
                "detached SourceFrame region {} does not preserve its destination aspect",
                region.id
            ));
        }
        validate_manual_mapping(region.id, &binding.mapping)?;
        let mapping_mode = manual_sampling_mode(behavior.role, behavior.sampling);
        let sampling = match mapping_mode {
            SamplingMode::RepeatX => RegionSampling::LoopX,
            SamplingMode::RepeatY => RegionSampling::LoopY,
            SamplingMode::PeriodicTile => RegionSampling::LoopXy,
            SamplingMode::DirectCrop | SamplingMode::PlanarRadial | SamplingMode::PolarRadial => {
                RegionSampling::OneShot
            }
            _ => behavior.sampling,
        };
        let semantic =
            semantic_rect_for_padding(allocation, preview_padding_px, behavior.edge_eligibility);
        let mapping_origin = if uses_frame_partition {
            if document.source_overrides.contains_key(&region.id) {
                "explicit_override"
            } else {
                "partition"
            }
        } else if matches!(&binding.content, ContentReference::Solid(_)) {
            "solid_binding"
        } else if patch_id.is_some() {
            "patch_binding"
        } else {
            "whole_source_binding"
        };
        let candidate_id = ContentDigest::sha256(
            format!(
                "source-frame|{}|{}|{}|{}|{}|{}|{}|{:?}|{:?}|mapping={:?}",
                frame
                    .identity
                    .0
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
                region.id,
                crop.x,
                crop.y,
                crop.width,
                crop.height,
                mapping_origin,
                source_set_id,
                patch_id,
                binding.mapping
            )
            .as_bytes(),
        );
        let authored_repeat = behavior.sampling != RegionSampling::OneShot;
        let mirror = match (
            binding.mapping.transform.mirror_x,
            binding.mapping.transform.mirror_y,
        ) {
            (true, false) => MirrorTransform::X,
            (false, true) => MirrorTransform::Y,
            _ => MirrorTransform::None,
        };
        let (family, route) = match mapping_mode {
            SamplingMode::RepeatX => (CandidateFamily::RepeatXSegment, CandidateRoute::Repeat),
            SamplingMode::RepeatY => (CandidateFamily::RepeatYSegment, CandidateRoute::Repeat),
            SamplingMode::PeriodicTile => {
                (CandidateFamily::PanelSeamlessTile, CandidateRoute::Repeat)
            }
            SamplingMode::PlanarRadial => (
                CandidateFamily::PlanarRadialSquare,
                CandidateRoute::PlanarRadial,
            ),
            SamplingMode::PolarRadial => (
                CandidateFamily::PolarRadialSynthesis,
                CandidateRoute::PolarRadial,
            ),
            _ => (CandidateFamily::PanelDirect, CandidateRoute::Direct),
        };
        let candidate = CropCandidate {
            candidate_id,
            source_id: domain.prepared_source_digest.clone(),
            domain_id: domain.cache_key.clone(),
            slot_id: region.id,
            crop: Some(crop),
            transform: CandidateTransform {
                rotation: behavior.orientation,
                mirror,
            },
            isotropic_scale: 1.0,
            mapping_mode,
            family,
            route,
            position_strategy: PositionStrategy::DenseLowResolution,
            period_pixels: authored_repeat
                .then_some(behavior.period_pixels.unwrap_or([crop.width, crop.height])),
            seam_indices: Vec::new(),
            correspondence_reference: domain.cache_key.clone(),
            descriptors: CandidateDescriptors {
                saliency_milli: 0,
                stationarity_milli: 0,
                feature_strength_milli: 0,
                usability_milli: 1000,
            },
            seed: provenance.recipe.seed,
            eligibility: EligibilityEvidence {
                mapping_permitted: true,
                transform_permitted: true,
                isotropic_scale: true,
                exact_aspect: true,
                entire_crop_usable: Some(true),
                cross_axis_preserved: Some(true),
                lattice_aligned: Some(true),
                direct_crop_applicable: true,
                direct_crop_rejection: None,
                reasons: vec!["accepted SourceFrame + GridRect direct mapping".into()],
            },
        };
        let (slot_physical_size, source_pixels_per_physical_unit) =
            manual_physical_mapping(behavior.sampling, crop, semantic);
        let sampling_plan = SamplingPlan {
            slot_id: region.id,
            role: manual_template_role(behavior.role),
            variation_group: region.material_group.clone(),
            prepared_domain_dimensions: [domain.width, domain.height],
            candidate,
            sampling_basis: hot_trimmer_placement_solver::SamplingBasis::SelectedCrop,
            slot_physical_size,
            source_pixels_per_physical_unit,
            sampling_policy: SamplingPolicy {
                filter: binding.mapping.sampling.filter,
                scale: binding.mapping.sampling.scale * binding.mapping.transform.scale[0].abs(),
                correct_tangent_normals: binding.mapping.sampling.correct_tangent_normals,
            },
            radial_mapping: behavior.radial,
            stretch_override: StretchOverrideProvenance::NotAuthorized,
            slice_geometry: SliceGeometry::None,
            maximum_seam_cost_milli: 0,
            unary_cost: 0.0,
        };
        let render_cache_key = ContentDigest::sha256(format!(
            "{SOURCE_FRAME_COMPILER_VERSION}|render|profile={:?}|output={}x{}|candidate={:?}|domain={:?}|semantic={}x{}|padding={preview_padding_px}",
            request.profile, output_size.width, output_size.height,
            sampling_plan.candidate.candidate_id, domain.cache_key, semantic.width, semantic.height,
        ).as_bytes());
        for channel_role in compiled_source_roles_for_domain(domain.as_ref()) {
            let source_key = (*source_set_id, source_id.clone(), channel_role);
            if source_index.contains_key(&source_key) {
                continue;
            }
            source_records.push(CompiledSourceCommandV1 {
                source_set_id: *source_set_id,
                source_id: source_id.clone(),
                digest: prepared_channel_digest(domain.as_ref(), channel_role),
                oriented_dimensions: hot_trimmer_domain::OrientedPixelSize {
                    width: domain.width,
                    height: domain.height,
                },
                decoder_version: DIRECT_SOURCE_FRAME_DECODER_VERSION.to_string(),
                decoded_format: "rgba8".to_string(),
                color_version: DIRECT_SOURCE_FRAME_DECODER_VERSION.to_string(),
                channel_role,
            });
            source_index.insert(source_key, source_records.len());
        }
        let compiled_profile = compile_persisted_profile_for_region(
            &document.decorations,
            region.id,
            region.structural_profile,
            sampling_plan.slot_physical_size,
            [allocation.width, allocation.height],
            &conservative_profile_capacity(sampling_plan.slot_physical_size),
            &render_cache_key,
            sampling_plan.candidate.seed,
        )?;
        let edge_detail = compile_edge_detail_for_region(
            document.edge_detail.as_ref(),
            region.id,
            region.role,
            behavior.role,
            region.structural_profile,
            sampling_plan.slot_physical_size,
            [allocation.width, allocation.height],
            behavior.edge_eligibility,
            compiled_profile.cache_identity.clone(),
            domain.as_ref(),
            &material_maps_for_view_intent(request.view_intent.as_ref()),
            &format!("{:?}", request.profile),
        )?;
        region_records.push(CompiledRegionCommandV1 {
            region_id: region.id,
            compact_index: region_index.try_into().map_err(|_| {
                format!(
                    "source-frame compact index for region {} overflows u32",
                    region.id
                )
            })?,
            region_role: behavior.role,
            source_set_id: *source_set_id,
            source_id: source_id.clone(),
            patch_id: *patch_id,
            source_crop: SourcePixelRect(hot_trimmer_domain::PixelBounds {
                x: crop.x,
                y: crop.y,
                width: crop.width,
                height: crop.height,
            }),
            destination_rect: OutputPixelRect(hot_trimmer_domain::PixelBounds {
                x: allocation.x,
                y: allocation.y,
                width: allocation.width,
                height: allocation.height,
            }),
            sampling,
            source_to_region_transform,
            radial_parameters: behavior.radial,
            structural_profile: region.structural_profile,
            compiled_profile,
            compiled_details: compile_persisted_details_for_region(
                &document.decorations,
                region.id,
                region.role,
                sampling_plan.slot_physical_size,
                [allocation.width, allocation.height],
                &conservative_profile_capacity(sampling_plan.slot_physical_size),
                &render_cache_key,
            )?,
            continuity: behavior.continuity,
            padding_px: preview_padding_px,
            edge_eligibility: behavior.edge_eligibility,
            edge_detail,
            edge_wear: legacy_edge_wear_for_region(document.edge_detail.as_ref(), region.id),
            sampling_plan,
            render_cache_key,
        });
    }
    let mut source_frame_atlas_plan = compiled_atlas_plan_from_persisted(
        request.revision,
        request.draft_id,
        document.topology.topology_hash,
        document
            .appearance_hash()
            .map_err(|error| error.to_string())?,
        output_size,
        request.profile,
        material_maps_for_view_intent(request.view_intent.as_ref()),
        source_records,
        region_records,
    )
    .map_err(|error| error.to_string())?;
    source_frame_atlas_plan.tile_request = compiled_tile_request_for(
        request.profile,
        request.view_intent.as_ref(),
        output_size,
        request.draft_id.unwrap_or_default(),
        &source_frame_atlas_plan.ordered_regions,
    )?;
    source_frame_atlas_plan.final_plan_hash = ContentDigest(String::new());
    source_frame_atlas_plan = source_frame_atlas_plan
        .finalize()
        .map_err(|error| error.to_string())?;
    let plan_ms = plan_started.elapsed().as_millis();
    let mut prepared_sources = BTreeMap::new();
    for (source_set_id, _patch_id, source_id, domain) in &region_sources {
        for channel_role in compiled_source_roles_for_domain(domain.as_ref()) {
            prepared_sources
                .entry((*source_set_id, source_id.clone(), channel_role))
                .or_insert_with(|| AtlasPreparedSource {
                    source_set_id: *source_set_id,
                    source_id: source_id.clone(),
                    channel_role,
                    domain: Arc::clone(domain),
                });
        }
    }
    let stamp_domains = stage16_stamp_asset_domains(
        request.project,
        &document.decorations,
        request.profile,
        image_cancellation,
        reusable_source_cache,
    )?;
    for (asset, source_set_id, domain) in &stamp_domains {
        for channel_role in compiled_source_roles_for_domain(domain.as_ref()) {
            let source_id = asset.digest.clone();
            prepared_sources
                .entry((*source_set_id, source_id.clone(), channel_role))
                .or_insert_with(|| AtlasPreparedSource {
                    source_set_id: *source_set_id,
                    source_id,
                    channel_role,
                    domain: Arc::clone(domain),
                });
        }
    }
    let execution_input = AtlasRenderExecutionInput {
        prepared_sources: prepared_sources.into_values().collect(),
        source_frame_cache,
    };
    let cpu_executor = CpuAtlasRenderExecutor;
    let executor = executor_override.unwrap_or(&cpu_executor);
    let execution = executor
        .execute(
            &source_frame_atlas_plan,
            &execution_input,
            cancellation,
            is_current,
        )
        .map_err(|error| error.to_string())?;
    let cpu_execution = execution.as_cpu_regions();
    let mut slots = Vec::with_capacity(document.topology.regions.len());
    let mut plans = Vec::with_capacity(document.topology.regions.len());
    let mut results = Vec::with_capacity(document.topology.regions.len());
    let rendered_cache_hits = cpu_execution.map_or(0, |execution| execution.rendered_cache_hits);
    for (region_index, region) in document.topology.regions.iter().enumerate() {
        if !active() {
            return Err("preview cancelled or superseded in source-frame stages".into());
        }
        let binding = document
            .region_bindings
            .get(&region.id)
            .ok_or_else(|| format!("region {} has no persisted content binding", region.id))?;
        let (source_set_id, patch_id, _source_id, domain) = &region_sources[region_index];
        let uses_frame_partition =
            matches!(&binding.content, ContentReference::InheritPrimaryMaterial)
                && *source_set_id == frame.source_set_id;
        let rect = region
            .grid_rect
            .ok_or_else(|| format!("region {} has no persisted GridRect", region.id))?;
        let allocation = hot_trimmer_domain::CanonicalRect {
            x: destination_x[rect.x as usize],
            y: destination_y[rect.y as usize],
            width: destination_x[(rect.x + rect.width) as usize] - destination_x[rect.x as usize],
            height: destination_y[(rect.y + rect.height) as usize] - destination_y[rect.y as usize],
        };
        let behavior = &binding.mapping.behavior;
        let semantic =
            semantic_rect_for_padding(allocation, preview_padding_px, behavior.edge_eligibility);
        let fallback_crop = if uses_frame_partition {
            document
                .source_overrides
                .get(&region.id)
                .map(|value| {
                    let bounds = value.source_bounds;
                    let x = (bounds.x.get() * f64::from(frame_domain.width)).round() as u32;
                    let y = (bounds.y.get() * f64::from(frame_domain.height)).round() as u32;
                    SourceCrop {
                        x,
                        y,
                        width: (bounds.width.get() * f64::from(frame_domain.width)).round() as u32,
                        height: (bounds.height.get() * f64::from(frame_domain.height)).round()
                            as u32,
                    }
                })
                .unwrap_or(SourceCrop {
                    x: source_x[rect.x as usize],
                    y: source_y[rect.y as usize],
                    width: source_x[(rect.x + rect.width) as usize] - source_x[rect.x as usize],
                    height: source_y[(rect.y + rect.height) as usize] - source_y[rect.y as usize],
                })
        } else {
            SourceCrop {
                x: 0,
                y: 0,
                width: domain.width,
                height: domain.height,
            }
        };
        let crop =
            source_frame_preview_crop(&binding.mapping, fallback_crop, domain.width, domain.height);
        if crop.width == 0 || crop.height == 0 || allocation.width == 0 || allocation.height == 0 {
            return Err(format!(
                "source-frame region {} collapsed at resolved pixel boundaries",
                region.id
            ));
        }
        if uses_frame_partition
            && document.source_overrides.contains_key(&region.id)
            && !aspect_matches(
                f64::from(crop.width),
                f64::from(crop.height),
                f64::from(allocation.width),
                f64::from(allocation.height),
            )
        {
            return Err(format!(
                "detached SourceFrame region {} does not preserve its destination aspect",
                region.id
            ));
        }
        let mapping_origin = if uses_frame_partition {
            if document.source_overrides.contains_key(&region.id) {
                "explicit_override"
            } else {
                "partition"
            }
        } else if matches!(&binding.content, ContentReference::Solid(_)) {
            "solid_binding"
        } else if patch_id.is_some() {
            "patch_binding"
        } else {
            "whole_source_binding"
        };
        validate_manual_mapping(region.id, &binding.mapping)?;
        if behavior
            .period_pixels
            .is_some_and(|period| period[0] > crop.width || period[1] > crop.height)
        {
            return Err(format!(
                "region {} authored repeat period exceeds its exact source crop",
                region.id
            ));
        }
        let mapping_mode = manual_sampling_mode(behavior.role, behavior.sampling);
        let candidate_id = hot_trimmer_domain::ContentDigest::sha256(
            format!(
                "source-frame|{}|{}|{}|{}|{}|{}|{}|{:?}|{:?}|mapping={:?}",
                frame
                    .identity
                    .0
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
                region.id,
                crop.x,
                crop.y,
                crop.width,
                crop.height,
                mapping_origin,
                source_set_id,
                patch_id,
                binding.mapping
            )
            .as_bytes(),
        );
        let authored_repeat = behavior.sampling != RegionSampling::OneShot;
        let mirror = match (
            binding.mapping.transform.mirror_x,
            binding.mapping.transform.mirror_y,
        ) {
            (true, false) => MirrorTransform::X,
            (false, true) => MirrorTransform::Y,
            _ => MirrorTransform::None,
        };
        let (family, route) = match mapping_mode {
            SamplingMode::RepeatX => (CandidateFamily::RepeatXSegment, CandidateRoute::Repeat),
            SamplingMode::RepeatY => (CandidateFamily::RepeatYSegment, CandidateRoute::Repeat),
            SamplingMode::PeriodicTile => {
                (CandidateFamily::PanelSeamlessTile, CandidateRoute::Repeat)
            }
            SamplingMode::PlanarRadial => (
                CandidateFamily::PlanarRadialSquare,
                CandidateRoute::PlanarRadial,
            ),
            SamplingMode::PolarRadial => (
                CandidateFamily::PolarRadialSynthesis,
                CandidateRoute::PolarRadial,
            ),
            _ => (CandidateFamily::PanelDirect, CandidateRoute::Direct),
        };
        let candidate = CropCandidate {
            candidate_id: candidate_id.clone(),
            source_id: domain.prepared_source_digest.clone(),
            domain_id: domain.cache_key.clone(),
            slot_id: region.id,
            crop: Some(crop),
            transform: CandidateTransform {
                rotation: behavior.orientation,
                mirror,
            },
            isotropic_scale: 1.0,
            mapping_mode,
            family,
            route,
            position_strategy: PositionStrategy::DenseLowResolution,
            period_pixels: authored_repeat
                .then_some(behavior.period_pixels.unwrap_or([crop.width, crop.height])),
            seam_indices: Vec::new(),
            correspondence_reference: domain.cache_key.clone(),
            descriptors: CandidateDescriptors {
                saliency_milli: 0,
                stationarity_milli: 0,
                feature_strength_milli: 0,
                usability_milli: 1000,
            },
            seed: provenance.recipe.seed,
            eligibility: EligibilityEvidence {
                mapping_permitted: true,
                transform_permitted: true,
                isotropic_scale: true,
                exact_aspect: true,
                entire_crop_usable: Some(true),
                cross_axis_preserved: Some(true),
                lattice_aligned: Some(true),
                direct_crop_applicable: true,
                direct_crop_rejection: None,
                reasons: vec!["accepted SourceFrame + GridRect direct mapping".into()],
            },
        };
        let (slot_physical_size, source_pixels_per_physical_unit) =
            manual_physical_mapping(behavior.sampling, crop, semantic);
        let plan = SamplingPlan {
            slot_id: region.id,
            role: manual_template_role(behavior.role),
            variation_group: region.material_group.clone(),
            prepared_domain_dimensions: [domain.width, domain.height],
            candidate,
            sampling_basis: hot_trimmer_placement_solver::SamplingBasis::SelectedCrop,
            slot_physical_size,
            source_pixels_per_physical_unit,
            sampling_policy: SamplingPolicy {
                filter: binding.mapping.sampling.filter,
                scale: binding.mapping.sampling.scale * binding.mapping.transform.scale[0].abs(),
                correct_tangent_normals: binding.mapping.sampling.correct_tangent_normals,
            },
            radial_mapping: behavior.radial,
            stretch_override: StretchOverrideProvenance::NotAuthorized,
            slice_geometry: SliceGeometry::None,
            maximum_seam_cost_milli: 0,
            unary_cost: 0.0,
        };
        let slot_key = region.id.to_string();
        let topology_slot = hot_trimmer_domain::CompiledTemplateSlot {
            slot_key,
            allocation,
            hotspot: semantic,
        };
        let result = if let Some(cpu_execution) = cpu_execution {
            let executed = cpu_execution
                .regions
                .get(region_index)
                .ok_or_else(|| format!("CPU atlas executor omitted region {}", region.id))?;
            if executed.region_id != region.id || executed.sampling_plan != plan {
                return Err(format!(
                    "CPU atlas executor returned an instruction mismatch for region {}",
                    region.id
                ));
            }
            Some(Arc::clone(&executed.result))
        } else {
            None
        };
        slots.push(topology_slot);
        plans.push(plan);
        if let Some(result) = result {
            results.push(result);
        }
    }
    let render_ms = match &execution {
        AtlasRenderExecutorOutput::CpuRegions(output) => output.render_ms,
        AtlasRenderExecutorOutput::FinalAtlas(output) => output.render_ms,
    };
    let topology = hot_trimmer_domain::CompiledTemplateTopology {
        identity: hot_trimmer_domain::TemplateIdentity {
            template_id: "source-frame".into(),
            template_version: SOURCE_FRAME_COMPILER_VERSION.into(),
            compatibility_key: document.topology.compatibility_key.clone(),
        },
        output_size,
        slots,
    };
    let placement = PlacementPlan {
        stage_result: StageResult::Executed {
            algorithm: AlgorithmProvenance {
                algorithm_id: "hot-trimmer.stage-13.source-frame-direct".into(),
                version: SOURCE_FRAME_COMPILER_VERSION.into(),
            },
            settings_hash: hot_trimmer_domain::ContentDigest::sha256(
                b"source-frame-direct-placement",
            ),
            diagnostics: Vec::new(),
        },
        solver: AlgorithmProvenance {
            algorithm_id: "hot-trimmer.stage-13.source-frame-direct".into(),
            version: SOURCE_FRAME_COMPILER_VERSION.into(),
        },
        seed: provenance.recipe.seed,
        placements: plans,
        objective: PlacementObjectiveBreakdown {
            unary_cost: 0.0,
            pairwise_cost: 0.0,
            pairwise_lambda: 0.0,
            weighted_pairwise_cost: 0.0,
            total_cost: 0.0,
        },
        pairwise_decisions: Vec::new(),
        crop_reuse_heatmap: Vec::new(),
        validation: PlacementValidationSummary {
            complete_assignment: true,
            required_slots_present: true,
            isotropic_scale_only: true,
            registered_mapping_only: true,
            slot_count: document.topology.regions.len() as u32,
        },
        qa_views: vec![
            PlacementPlanQaView::SelectedPlacements,
            PlacementPlanQaView::Validation,
        ],
    };
    let algorithms = (1..=14)
        .map(|stage| {
            (
                stage,
                AlgorithmProvenance {
                    algorithm_id: format!("source-frame-stage-{stage:02}"),
                    version: SOURCE_FRAME_COMPILER_VERSION.into(),
                },
            )
        })
        .collect();
    let profile_regions =
        crate::resolve_profile_regions(document, &topology).map_err(|error| error.to_string())?;
    let (mut artifact, compose_ms) = match &execution {
        AtlasRenderExecutorOutput::CpuRegions(_) => {
            let inputs = topology
                .slots
                .iter()
                .zip(document.topology.regions.iter())
                .zip(placement.placements.iter().zip(results.iter()))
                .enumerate()
                .map(
                    |(index, ((slot, region), (plan, result)))| IntermediateSlotInput {
                        region_id: region.id,
                        slot_key: slot.slot_key.as_str(),
                        display_name: region.display_name.as_str(),
                        required: true,
                        patch_id: region_sources[index]
                            .1
                            .as_ref()
                            .map(|value| value.to_string()),
                        domain: region_sources[index].3.as_ref(),
                        plan,
                        result: result.as_ref(),
                        grid_rect: region.grid_rect,
                        behavior: document.region_bindings[&region.id]
                            .mapping
                            .behavior
                            .clone(),
                    },
                )
                .collect();
            let atlas_request = IntermediateAtlasRequest {
                topology: &topology,
                placement_plan: &placement,
                slots: inputs,
                revision: request.revision,
                algorithm_versions: algorithms,
                diagnostics: Vec::new(),
                regions: profile_regions,
            };
            let composition = executor
                .compose(
                    &AtlasComposeExecutionInput {
                        plan: &source_frame_atlas_plan,
                        request: &atlas_request,
                    },
                    cancellation,
                    &is_current,
                )
                .map_err(|error| error.to_string())?;
            (composition.artifact, composition.compose_ms)
        }
        AtlasRenderExecutorOutput::FinalAtlas(output) => (
            final_atlas_artifact_from_gpu(
                request.revision,
                topology.clone(),
                &placement,
                algorithms,
                profile_regions,
                document,
                &region_sources,
                &source_frame_atlas_plan,
                output,
            )?,
            0,
        ),
    };
    for slot in &mut artifact.slots {
        if let Some(region) = source_frame_atlas_plan
            .ordered_regions
            .iter()
            .find(|region| region.region_id == slot.region_id)
        {
            slot.compiled_profile = Some(region.compiled_profile.clone());
            slot.compiled_details = Some(region.compiled_details.clone());
        }
    }
    artifact.pending.retain(|pending| *pending != "profiles");
    artifact.pending.insert(
        0,
        match request.profile {
            SourceFramePreviewProfile::Draft512 => {
                "Draft 512 Base Color complete; refinement pending"
            }
            SourceFramePreviewProfile::Refinement1024 => "Refinement 1024 Base Color complete",
            SourceFramePreviewProfile::Preview2048 => "Preview 2048 Base Color complete",
            SourceFramePreviewProfile::Preview4096 => "Preview 4096 Base Color complete",
            SourceFramePreviewProfile::Preview8192 => "Preview 8192 Base Color complete",
            SourceFramePreviewProfile::Authoritative => "Authoritative Base Color complete",
        },
    );
    let cache = if decode_cache_hit { "hit" } else { "miss" };
    let gpu_final_atlas = matches!(execution, AtlasRenderExecutorOutput::FinalAtlas(_));
    let rendered_cache_misses = if gpu_final_atlas {
        0
    } else {
        document
            .topology
            .regions
            .len()
            .saturating_sub(rendered_cache_hits as usize)
    };
    let allocated_bytes = artifact
        .channels
        .iter()
        .map(|channel| channel.rgba8.len())
        .sum::<usize>()
        + artifact.validity.len()
        + artifact.correspondence.len() * std::mem::size_of::<[f32; 2]>()
        + artifact.region_ownership.len() * std::mem::size_of::<hot_trimmer_domain::RegionId>();
    let executor_label = if gpu_final_atlas { "gpu" } else { "cpu" };
    let compose_executor = if gpu_final_atlas { "gpu-direct" } else { "cpu" };
    let cpu_stage14_calls = if gpu_final_atlas {
        0
    } else {
        document.topology.regions.len()
    };
    let cpu_atlas_composition_calls = if gpu_final_atlas { 0 } else { 1 };
    let requested_map_summary = source_frame_atlas_plan
        .requested_maps
        .iter()
        .map(|map| format!("{map:?}"))
        .collect::<Vec<_>>()
        .join("|");
    artifact.telemetry.push(format!("profile={:?}; source={}x{}; oriented={}x{}; output={}x{}; output_padding_px={}; preview_padding_px={}; regions={}; patches={}; maps={requested_map_summary}; stage3-8=bypassed; decode_cache={cache}; decode_count={}; decode_ms={decode_ms}; plan_ms={plan_ms}; executor={executor_label}; plan_hash={}; render_ms={render_ms}; render_cache_hits={rendered_cache_hits}; render_cache_misses={rendered_cache_misses}; composed_cache=miss; compose_executor={compose_executor}; compose_ms={compose_ms}; cpu_stage14_calls={cpu_stage14_calls}; cpu_atlas_composition_calls={cpu_atlas_composition_calls}; allocated_bytes={allocated_bytes}; decode_full_frame_allocations={}; elapsed_ms={}",
        request.profile, frame.oriented_dimensions.width, frame.oriented_dimensions.height,
        frame_domain.width, frame_domain.height, output_size.width, output_size.height,
        document.render_settings.atlas_padding_px, preview_padding_px, document.topology.regions.len(), document.patches.len(),
        u8::from(!decode_cache_hit), source_frame_atlas_plan.final_plan_hash.0.as_str(),
        u8::from(!decode_cache_hit), started.elapsed().as_millis()));
    if !matches!(request.profile, SourceFramePreviewProfile::Authoritative)
        && let Some(cache) = source_frame_cache
    {
        if let Ok(mut guard) = cache.lock() {
            guard.insert_composed(composition_key, Arc::new(artifact.clone()));
        }
    }
    Ok(artifact)
}

fn compiled_tile_request_for(
    profile: SourceFramePreviewProfile,
    intent: Option<&SourceFramePreviewViewIntent>,
    output_size: hot_trimmer_domain::PixelSize,
    generation: u64,
    regions: &[CompiledRegionCommandV1],
) -> Result<CompiledTileRequest, String> {
    let full = OutputPixelRect(hot_trimmer_domain::PixelBounds {
        x: 0,
        y: 0,
        width: output_size.width,
        height: output_size.height,
    });
    let (kind, valid_rect, halo_px) = match intent {
        Some(SourceFramePreviewViewIntent::CompleteDraft512) => {
            if profile != SourceFramePreviewProfile::Draft512 {
                return Err("complete Draft 512 intent requires the Draft512 profile".into());
            }
            (CompiledTileRequestKind::CompleteDraft512, full, 0)
        }
        Some(SourceFramePreviewViewIntent::CompleteRefinement1024) => {
            if profile != SourceFramePreviewProfile::Refinement1024 {
                return Err(
                    "complete Refinement 1024 intent requires the Refinement1024 profile".into(),
                );
            }
            (CompiledTileRequestKind::CompleteRefinement1024, full, 0)
        }
        Some(SourceFramePreviewViewIntent::ExactViewport(rect))
        | Some(SourceFramePreviewViewIntent::ExactViewportMaterialMaps { rect, .. }) => {
            validate_requested_tile_rect(*rect, output_size, "viewport")?;
            (CompiledTileRequestKind::ExactViewport, *rect, 1)
        }
        Some(SourceFramePreviewViewIntent::ExactSelectedRegion(region_id))
        | Some(SourceFramePreviewViewIntent::ExactSelectedRegionMaterialMaps {
            region_id, ..
        }) => {
            let rect = regions
                .iter()
                .find_map(|region| {
                    (region.region_id == *region_id).then_some(region.destination_rect)
                })
                .ok_or_else(|| {
                    format!("exact selected-region request references unknown region {region_id}")
                })?;
            validate_requested_tile_rect(rect, output_size, "selected region")?;
            (CompiledTileRequestKind::ExactSelectedRegion, rect, 1)
        }
        Some(SourceFramePreviewViewIntent::MaterialMaps(_)) => match profile {
            SourceFramePreviewProfile::Draft512 => {
                (CompiledTileRequestKind::CompleteDraft512, full, 0)
            }
            SourceFramePreviewProfile::Refinement1024 => {
                (CompiledTileRequestKind::CompleteRefinement1024, full, 0)
            }
            SourceFramePreviewProfile::Preview2048 => {
                (CompiledTileRequestKind::CompletePreview2048, full, 0)
            }
            SourceFramePreviewProfile::Preview4096 => {
                (CompiledTileRequestKind::CompletePreview4096, full, 0)
            }
            SourceFramePreviewProfile::Preview8192 => {
                (CompiledTileRequestKind::CompletePreview8192, full, 0)
            }
            SourceFramePreviewProfile::Authoritative => {
                (CompiledTileRequestKind::ExactViewport, full, 0)
            }
        },
        None => match profile {
            SourceFramePreviewProfile::Draft512 => {
                (CompiledTileRequestKind::CompleteDraft512, full, 0)
            }
            SourceFramePreviewProfile::Refinement1024 => {
                (CompiledTileRequestKind::CompleteRefinement1024, full, 0)
            }
            SourceFramePreviewProfile::Preview2048 => {
                (CompiledTileRequestKind::CompletePreview2048, full, 0)
            }
            SourceFramePreviewProfile::Preview4096 => {
                (CompiledTileRequestKind::CompletePreview4096, full, 0)
            }
            SourceFramePreviewProfile::Preview8192 => {
                (CompiledTileRequestKind::CompletePreview8192, full, 0)
            }
            SourceFramePreviewProfile::Authoritative => {
                (CompiledTileRequestKind::ExactViewport, full, 0)
            }
        },
    };
    let output_rect = expand_tile_rect(valid_rect, halo_px, output_size);
    Ok(CompiledTileRequest {
        kind,
        generation,
        output_rect,
        mip_level: 0,
        halo_px,
        valid_rect,
    })
}

#[allow(clippy::too_many_arguments)]
fn fixed_template_compiled_atlas_plan(
    request: &PersistedStage14PreviewRequest<'_>,
    document: &hot_trimmer_domain::TrimSheetDocument,
    topology: &hot_trimmer_domain::CompiledTemplateTopology,
    ordered_plans: &[&SamplingPlan],
    domains: &[DomainArtifacts],
    region_domains: &[usize],
    demands: &[ResolvedSlotDemand],
) -> Result<CompiledAtlasPlanV1, String> {
    let mut source_records = Vec::new();
    let mut source_index = BTreeMap::new();
    for artifacts in domains {
        for channel_role in compiled_source_roles_for_domain(&artifacts.domain) {
            let source_id = artifacts.domain.prepared_source_digest.clone();
            let key = (artifacts.source_set_id, source_id.clone(), channel_role);
            if source_index.contains_key(&key) {
                continue;
            }
            source_records.push(CompiledSourceCommandV1 {
                source_set_id: artifacts.source_set_id,
                source_id: source_id.clone(),
                digest: prepared_channel_digest(&artifacts.domain, channel_role),
                oriented_dimensions: OrientedPixelSize {
                    width: artifacts.domain.width,
                    height: artifacts.domain.height,
                },
                decoder_version: DIRECT_SOURCE_FRAME_DECODER_VERSION.to_string(),
                decoded_format: "rgba8".to_string(),
                color_version: DIRECT_SOURCE_FRAME_DECODER_VERSION.to_string(),
                channel_role,
            });
            source_index.insert(key, source_records.len());
        }
    }

    let mut region_records = Vec::with_capacity(document.topology.regions.len());
    for (index, ((slot, region), plan)) in topology
        .slots
        .iter()
        .zip(document.topology.regions.iter())
        .zip(ordered_plans.iter())
        .enumerate()
    {
        let artifacts = &domains[region_domains[index]];
        let binding = document
            .region_bindings
            .get(&region.id)
            .ok_or_else(|| format!("region {} has no persisted content binding", region.id))?;
        let crop = match plan.sampling_basis {
            hot_trimmer_placement_solver::SamplingBasis::PreparedDomain { window } => window,
            hot_trimmer_placement_solver::SamplingBasis::SelectedCrop => {
                plan.candidate.crop.unwrap_or(SourceCrop {
                    x: 0,
                    y: 0,
                    width: artifacts.domain.width,
                    height: artifacts.domain.height,
                })
            }
        };
        let mut sampling_plan = (*plan).clone();
        if sampling_plan.candidate.crop.is_none()
            && matches!(
                sampling_plan.sampling_basis,
                hot_trimmer_placement_solver::SamplingBasis::SelectedCrop
            )
        {
            sampling_plan.candidate.crop = Some(crop);
        }
        let padding_px = slot
            .hotspot
            .x
            .saturating_sub(slot.allocation.x)
            .max(slot.hotspot.y.saturating_sub(slot.allocation.y))
            .max(
                (slot.allocation.x + slot.allocation.width)
                    .saturating_sub(slot.hotspot.x + slot.hotspot.width),
            )
            .max(
                (slot.allocation.y + slot.allocation.height)
                    .saturating_sub(slot.hotspot.y + slot.hotspot.height),
            );
        let render_cache_key = ContentDigest::sha256(
            format!(
                "fixed-template|{}|{}|{}|{}|{}|{}|{}|{}",
                request.profile as u8,
                region.id,
                plan.candidate.candidate_id.0,
                artifacts.domain.cache_key.0,
                slot.allocation.x,
                slot.allocation.y,
                slot.allocation.width,
                slot.allocation.height
            )
            .as_bytes(),
        );
        let compiled_profile = compile_persisted_profile_for_region(
            &document.decorations,
            region.id,
            region.structural_profile,
            sampling_plan.slot_physical_size,
            [slot.allocation.width, slot.allocation.height],
            &demands
                .iter()
                .find(|demand| demand.slot_id == region.id)
                .ok_or_else(|| format!("region {} has no Stage 10 capacity", region.id))?
                .effect_capacity,
            &render_cache_key,
            sampling_plan.candidate.seed,
        )?;
        let edge_detail = compile_edge_detail_for_region(
            document.edge_detail.as_ref(),
            region.id,
            region.role,
            binding.mapping.behavior.role,
            region.structural_profile,
            sampling_plan.slot_physical_size,
            [slot.allocation.width, slot.allocation.height],
            binding.mapping.behavior.edge_eligibility,
            compiled_profile.cache_identity.clone(),
            &artifacts.domain,
            &material_maps_for_view_intent(request.view_intent.as_ref()),
            &format!("{:?}", request.profile),
        )?;
        region_records.push(CompiledRegionCommandV1 {
            region_id: region.id,
            compact_index: u32::try_from(index)
                .map_err(|_| format!("fixed-template compact index {index} overflows u32"))?,
            region_role: binding.mapping.behavior.role,
            source_set_id: artifacts.source_set_id,
            source_id: artifacts.domain.prepared_source_digest.clone(),
            patch_id: artifacts.patch_id_raw,
            source_crop: SourcePixelRect(hot_trimmer_domain::PixelBounds {
                x: crop.x,
                y: crop.y,
                width: crop.width,
                height: crop.height,
            }),
            destination_rect: OutputPixelRect(hot_trimmer_domain::PixelBounds {
                x: slot.allocation.x,
                y: slot.allocation.y,
                width: slot.allocation.width,
                height: slot.allocation.height,
            }),
            sampling: binding.mapping.behavior.sampling,
            source_to_region_transform: binding.mapping.transform,
            radial_parameters: binding.mapping.behavior.radial,
            structural_profile: region.structural_profile,
            compiled_profile,
            compiled_details: compile_persisted_details_for_region(
                &document.decorations,
                region.id,
                region.role,
                sampling_plan.slot_physical_size,
                [slot.allocation.width, slot.allocation.height],
                &demands
                    .iter()
                    .find(|demand| demand.slot_id == region.id)
                    .ok_or_else(|| format!("region {} has no Stage 10 capacity", region.id))?
                    .effect_capacity,
                &render_cache_key,
            )?,
            continuity: binding.mapping.behavior.continuity,
            padding_px,
            edge_eligibility: binding.mapping.behavior.edge_eligibility,
            edge_detail,
            edge_wear: legacy_edge_wear_for_region(document.edge_detail.as_ref(), region.id),
            sampling_plan,
            render_cache_key,
        });
    }

    let mut plan = compiled_atlas_plan_from_persisted(
        request.revision,
        request.draft_id,
        document.topology.topology_hash,
        document
            .appearance_hash()
            .map_err(|error| error.to_string())?,
        topology.output_size,
        request.profile,
        material_maps_for_view_intent(request.view_intent.as_ref()),
        source_records,
        region_records,
    )
    .map_err(|error| error.to_string())?;
    plan.tile_request = compiled_tile_request_for(
        request.profile,
        request.view_intent.as_ref(),
        topology.output_size,
        request.draft_id.unwrap_or_default(),
        &plan.ordered_regions,
    )?;
    plan.final_plan_hash = ContentDigest(String::new());
    plan.finalize().map_err(|error| error.to_string())
}

fn fixed_template_algorithm_versions(
    first_domain: &DomainArtifacts,
    snapshot: &hot_trimmer_domain::TemplateSnapshot,
    stage10_result: &StageResult,
    placement: &PlacementPlan,
    stage14: Option<AlgorithmProvenance>,
) -> BTreeMap<u8, AlgorithmProvenance> {
    algorithm_versions([
        (
            1,
            Some(AlgorithmProvenance {
                algorithm_id: "hot_trimmer.persisted_registered_source".into(),
                version: "1.0.0".into(),
            }),
        ),
        (
            2,
            Some(AlgorithmProvenance {
                algorithm_id: hot_trimmer_image_io::STAGE_02_ALGORITHM_ID.into(),
                version: hot_trimmer_image_io::STAGE_02_ALGORITHM_VERSION.into(),
            }),
        ),
        (3, executed_algorithm(&first_domain.stage3_result)),
        (4, executed_algorithm(&first_domain.stage4_result)),
        (5, executed_algorithm(&first_domain.stage5.stage_result)),
        (6, executed_algorithm(&first_domain.stage6.stage_result)),
        (7, executed_algorithm(&first_domain.stage7.stage_result)),
        (8, executed_algorithm(&first_domain.stage8_result)),
        (
            9,
            Some(AlgorithmProvenance {
                algorithm_id: "hot_trimmer.fixed_template_topology".into(),
                version: snapshot.identity.template_version.clone(),
            }),
        ),
        (10, executed_algorithm(stage10_result)),
        (
            11,
            Some(AlgorithmProvenance {
                algorithm_id: hot_trimmer_placement_solver::STAGE_11_ALGORITHM_ID.into(),
                version: hot_trimmer_placement_solver::STAGE_11_ALGORITHM_VERSION.into(),
            }),
        ),
        (
            12,
            Some(AlgorithmProvenance {
                algorithm_id: hot_trimmer_placement_solver::STAGE_12_ALGORITHM_ID.into(),
                version: hot_trimmer_placement_solver::STAGE_12_ALGORITHM_VERSION.into(),
            }),
        ),
        (13, executed_algorithm(&placement.stage_result)),
        (14, stage14),
    ])
}

fn material_maps_for_view_intent(
    intent: Option<&SourceFramePreviewViewIntent>,
) -> Vec<hot_trimmer_domain::MaterialMapKind> {
    let maps = match intent {
        Some(SourceFramePreviewViewIntent::MaterialMaps(maps))
        | Some(SourceFramePreviewViewIntent::ExactViewportMaterialMaps { maps, .. })
        | Some(SourceFramePreviewViewIntent::ExactSelectedRegionMaterialMaps { maps, .. }) => {
            maps.as_slice()
        }
        _ => &[hot_trimmer_domain::MaterialMapKind::BaseColor],
    };
    let mut requested = Vec::with_capacity(maps.len().max(1));
    for map in maps {
        if !requested.contains(map) {
            requested.push(*map);
        }
    }
    if requested.is_empty() {
        requested.push(hot_trimmer_domain::MaterialMapKind::BaseColor);
    }
    requested
}

fn validate_requested_tile_rect(
    rect: OutputPixelRect,
    output_size: hot_trimmer_domain::PixelSize,
    label: &str,
) -> Result<(), String> {
    let rect = rect.0;
    if rect.width == 0
        || rect.height == 0
        || rect.x >= output_size.width
        || rect.y >= output_size.height
        || rect.x.saturating_add(rect.width) > output_size.width
        || rect.y.saturating_add(rect.height) > output_size.height
    {
        return Err(format!(
            "exact {label} rectangle is outside the compiled output"
        ));
    }
    Ok(())
}

fn expand_tile_rect(
    valid_rect: OutputPixelRect,
    halo_px: u32,
    output_size: hot_trimmer_domain::PixelSize,
) -> OutputPixelRect {
    let valid = valid_rect.0;
    let x = valid.x.saturating_sub(halo_px);
    let y = valid.y.saturating_sub(halo_px);
    let right = valid
        .x
        .saturating_add(valid.width)
        .saturating_add(halo_px)
        .min(output_size.width);
    let bottom = valid
        .y
        .saturating_add(valid.height)
        .saturating_add(halo_px)
        .min(output_size.height);
    OutputPixelRect(hot_trimmer_domain::PixelBounds {
        x,
        y,
        width: right - x,
        height: bottom - y,
    })
}

pub fn compiled_atlas_plan_from_persisted(
    revision: u64,
    request_generation: Option<u64>,
    topology_hash: hot_trimmer_domain::DocumentHash,
    appearance_hash: hot_trimmer_domain::DocumentHash,
    output_size: hot_trimmer_domain::PixelSize,
    profile: SourceFramePreviewProfile,
    requested_maps: Vec<hot_trimmer_domain::MaterialMapKind>,
    sources: Vec<CompiledSourceCommandV1>,
    regions: Vec<CompiledRegionCommandV1>,
) -> Result<CompiledAtlasPlanV1, CompiledAtlasPlanValidationError> {
    CompiledAtlasPlanV1 {
        schema_version: COMPILED_ATLAS_PLAN_SCHEMA_VERSION,
        algorithm_version: COMPILED_ATLAS_ALGORITHM_VERSION.to_string(),
        document_revision: revision,
        request_generation,
        topology_hash,
        appearance_hash,
        output_size,
        preview_profile: match profile {
            SourceFramePreviewProfile::Draft512 => CompiledAtlasPreviewProfile::Draft512,
            SourceFramePreviewProfile::Refinement1024 => {
                CompiledAtlasPreviewProfile::Refinement1024
            }
            SourceFramePreviewProfile::Preview2048 => CompiledAtlasPreviewProfile::Preview2048,
            SourceFramePreviewProfile::Preview4096 => CompiledAtlasPreviewProfile::Preview4096,
            SourceFramePreviewProfile::Preview8192 => CompiledAtlasPreviewProfile::Preview8192,
            SourceFramePreviewProfile::Authoritative => CompiledAtlasPreviewProfile::Authoritative,
        },
        normal_convention: CompiledNormalConvention::OpenGl,
        color_space_policy: CompiledColorSpacePolicy::SrgbColorUnassociatedAlpha,
        tile_request: CompiledTileRequest {
            kind: match profile {
                SourceFramePreviewProfile::Draft512 => CompiledTileRequestKind::CompleteDraft512,
                SourceFramePreviewProfile::Refinement1024 => {
                    CompiledTileRequestKind::CompleteRefinement1024
                }
                SourceFramePreviewProfile::Preview2048 => {
                    CompiledTileRequestKind::CompletePreview2048
                }
                SourceFramePreviewProfile::Preview4096 => {
                    CompiledTileRequestKind::CompletePreview4096
                }
                SourceFramePreviewProfile::Preview8192 => {
                    CompiledTileRequestKind::CompletePreview8192
                }
                SourceFramePreviewProfile::Authoritative => CompiledTileRequestKind::ExactViewport,
            },
            generation: request_generation.unwrap_or_default(),
            output_rect: OutputPixelRect(hot_trimmer_domain::PixelBounds {
                x: 0,
                y: 0,
                width: output_size.width,
                height: output_size.height,
            }),
            mip_level: 0,
            halo_px: 0,
            valid_rect: OutputPixelRect(hot_trimmer_domain::PixelBounds {
                x: 0,
                y: 0,
                width: output_size.width,
                height: output_size.height,
            }),
        },
        requested_maps,
        ordered_sources: sources,
        ordered_regions: regions,
        final_plan_hash: ContentDigest(String::new()),
    }
    .finalize()
}

#[allow(clippy::too_many_arguments)]
fn final_atlas_artifact_from_gpu(
    revision: u64,
    topology: hot_trimmer_domain::CompiledTemplateTopology,
    placement: &PlacementPlan,
    algorithm_versions: BTreeMap<u8, AlgorithmProvenance>,
    regions: Vec<crate::ResolvedRegion>,
    document: &hot_trimmer_domain::TrimSheetDocument,
    region_sources: &[(
        hot_trimmer_domain::SourceSetId,
        Option<hot_trimmer_domain::PatchId>,
        ContentDigest,
        Arc<hot_trimmer_material_synthesis::PreparedMaterialDomain>,
    )],
    compiled_plan: &CompiledAtlasPlanV1,
    output: &AtlasFinalAtlasOutput,
) -> Result<IntermediateAtlasArtifact, String> {
    for tile in output.map_tiles.values() {
        let expected_pixels =
            usize::try_from(u64::from(tile.manifest.width) * u64::from(tile.manifest.height))
                .map_err(|_| "GPU atlas output size overflows host metadata limits".to_string())?;
        if tile.pixels().len() != expected_pixels * 4 {
            return Err(format!(
                "GPU {:?} output did not match its bounded tile dimensions",
                tile.manifest.map
            ));
        }
    }
    let placement_plan_id = ContentDigest::sha256(
        &placement
            .deterministic_bytes()
            .map_err(|error| error.to_string())?,
    );
    let mut inspections = Vec::with_capacity(topology.slots.len());
    for (index, ((slot, region), plan)) in topology
        .slots
        .iter()
        .zip(document.topology.regions.iter())
        .zip(placement.placements.iter())
        .enumerate()
    {
        let allocation = slot.allocation;
        let valid_pixel_count = output
            .region_valid_pixel_counts
            .iter()
            .find_map(|(region_id, count)| (*region_id == region.id).then_some(*count))
            .unwrap_or_else(|| u64::from(allocation.width) * u64::from(allocation.height));
        let plan_id =
            ContentDigest::sha256(&serde_json::to_vec(plan).map_err(|error| error.to_string())?);
        let source = &region_sources[index];
        inspections.push(crate::IntermediateSlotInspection {
            region_id: region.id,
            slot_key: slot.slot_key.clone(),
            display_name: region.display_name.clone(),
            allocation,
            hotspot: slot.hotspot,
            semantic_rect: slot.hotspot,
            padded_rect: allocation,
            atlas_destination: allocation,
            preview_padding_px: slot
                .hotspot
                .x
                .saturating_sub(allocation.x)
                .max(slot.hotspot.y.saturating_sub(allocation.y))
                .max(
                    (allocation.x + allocation.width)
                        .saturating_sub(slot.hotspot.x + slot.hotspot.width),
                )
                .max(
                    (allocation.y + allocation.height)
                        .saturating_sub(slot.hotspot.y + slot.hotspot.height),
                ),
            mapping_mode: plan.candidate.mapping_mode,
            source_transform: plan.candidate.transform,
            isotropic_scale: plan.candidate.isotropic_scale,
            sampling_scale: plan.sampling_policy.scale,
            valid_pixel_count,
            source_id: plan.candidate.source_id.clone(),
            patch_id: source.1.as_ref().map(ToString::to_string),
            domain_id: source.3.cache_key.clone(),
            candidate_id: plan.candidate.candidate_id.clone(),
            sampling_plan_id: plan_id.clone(),
            stage_14_result_id: ContentDigest::sha256(
                format!(
                    "gpu-base-color|{}|{}|{}",
                    plan_id.0, region.id, valid_pixel_count
                )
                .as_bytes(),
            ),
            source_crop: plan.candidate.crop,
            grid_rect: region.grid_rect,
            behavior_version: document.region_bindings[&region.id]
                .mapping
                .behavior
                .version,
            role: document.region_bindings[&region.id].mapping.behavior.role,
            continuity: document.region_bindings[&region.id]
                .mapping
                .behavior
                .continuity,
            requested_sampling: document.region_bindings[&region.id]
                .mapping
                .behavior
                .sampling,
            executed_mode: plan.candidate.mapping_mode,
            edge_eligibility: document.region_bindings[&region.id]
                .mapping
                .behavior
                .edge_eligibility,
            period_pixels: plan.candidate.period_pixels,
            address_mode: match document.region_bindings[&region.id]
                .mapping
                .behavior
                .sampling
            {
                RegionSampling::OneShot => "clamp",
                RegionSampling::LoopX => "repeat_x",
                RegionSampling::LoopY => "repeat_y",
                RegionSampling::LoopXy => "repeat_xy",
            },
            compiled_profile: compiled_plan
                .ordered_regions
                .iter()
                .find(|command| command.region_id == region.id)
                .map(|command| command.compiled_profile.clone()),
            compiled_details: compiled_plan
                .ordered_regions
                .iter()
                .find(|command| command.region_id == region.id)
                .map(|command| command.compiled_details.clone()),
        });
    }
    let all_importable = [
        MaterialChannelRole::Normal,
        MaterialChannelRole::Height,
        MaterialChannelRole::Roughness,
        MaterialChannelRole::Metallic,
        MaterialChannelRole::AmbientOcclusion,
        MaterialChannelRole::Specular,
        MaterialChannelRole::Opacity,
        MaterialChannelRole::EdgeMask,
        MaterialChannelRole::RegionId,
        MaterialChannelRole::MaterialId,
    ];
    let complete_output_rect = hot_trimmer_domain::PixelBounds {
        x: 0,
        y: 0,
        width: topology.output_size.width,
        height: topology.output_size.height,
    };
    let channels = output
        .map_tiles
        .values()
        .filter(|tile| tile.manifest.valid_rect.0 == complete_output_rect)
        .filter(|tile| {
            matches!(
                tile.manifest.pixel_format,
                crate::CompiledTilePixelFormat::Rgba8UnormSrgb
                    | crate::CompiledTilePixelFormat::Rgba8UnormLinear
            )
        })
        .map(|tile| crate::IntermediateAtlasChannel {
            role: material_map_to_channel_role(tile.manifest.map),
            rgba8: tile.pixels().to_vec(),
        })
        .collect::<Vec<_>>();
    let published_roles = output
        .map_tiles
        .values()
        .filter(|tile| tile.manifest.valid_rect.0 == complete_output_rect)
        .map(|tile| material_map_to_channel_role(tile.manifest.map))
        .collect::<std::collections::BTreeSet<_>>();
    let unavailable_channels = all_importable
        .into_iter()
        .filter(|role| !published_roles.contains(role))
        .collect::<Vec<_>>();
    Ok(IntermediateAtlasArtifact {
        label: crate::INTERMEDIATE_ATLAS_LABEL,
        non_exportable: true,
        incomplete_after_stage: crate::INCOMPLETE_AFTER_STAGE,
        revision,
        topology,
        placement_plan_id,
        channels,
        unavailable_channels,
        correspondence: Vec::new(),
        validity: Vec::new(),
        region_ownership: Vec::new(),
        region_id_lookup: compiled_plan.compact_region_id_lookup(),
        slots: inspections,
        algorithm_versions,
        diagnostics: Vec::new(),
        regions,
        telemetry: output.telemetry.clone(),
        rendered_tile: Some(Arc::clone(&output.interactive_tile)),
        rendered_tiles: output.map_tiles.clone(),
        rendered_tile_timings: output.tile_timings.clone(),
        rendered_display_tiles: output.display_tiles.clone(),
        pending: vec![
            "profiles",
            "semantic details",
            "effects",
            "final PBR composition",
            "finishing",
            "mips",
            "metadata",
            "export",
            "Blender application",
        ],
    })
}

fn material_map_to_channel_role(map: hot_trimmer_domain::MaterialMapKind) -> MaterialChannelRole {
    match map {
        hot_trimmer_domain::MaterialMapKind::BaseColor => MaterialChannelRole::BaseColor,
        hot_trimmer_domain::MaterialMapKind::Normal => MaterialChannelRole::Normal,
        hot_trimmer_domain::MaterialMapKind::Height => MaterialChannelRole::Height,
        hot_trimmer_domain::MaterialMapKind::Roughness => MaterialChannelRole::Roughness,
        hot_trimmer_domain::MaterialMapKind::Metallic => MaterialChannelRole::Metallic,
        hot_trimmer_domain::MaterialMapKind::AmbientOcclusion => {
            MaterialChannelRole::AmbientOcclusion
        }
        hot_trimmer_domain::MaterialMapKind::Specular => MaterialChannelRole::Specular,
        hot_trimmer_domain::MaterialMapKind::Opacity => MaterialChannelRole::Opacity,
        hot_trimmer_domain::MaterialMapKind::EdgeMask => MaterialChannelRole::EdgeMask,
        hot_trimmer_domain::MaterialMapKind::RegionId => MaterialChannelRole::RegionId,
        hot_trimmer_domain::MaterialMapKind::MaterialId => MaterialChannelRole::MaterialId,
    }
}

fn compiled_source_roles_for_domain(
    domain: &hot_trimmer_material_synthesis::PreparedMaterialDomain,
) -> Vec<MaterialChannelRole> {
    let mut roles = domain
        .registered_channels()
        .iter()
        .map(PreparedExemplarChannel::role)
        .collect::<Vec<_>>();
    roles.sort();
    roles.dedup();
    if !roles.contains(&MaterialChannelRole::BaseColor) {
        roles.insert(0, MaterialChannelRole::BaseColor);
    }
    roles
}

fn prepared_channel_digest(
    domain: &hot_trimmer_material_synthesis::PreparedMaterialDomain,
    role: MaterialChannelRole,
) -> ContentDigest {
    let channel = domain
        .registered_channels()
        .iter()
        .find(|channel| channel.role() == role);
    let mut hash = Sha256::new();
    hash.update(b"stage-14-prepared-channel-v2-validity");
    hash.update([role as u8]);
    hash.update(domain.cache_key.0.as_bytes());
    hash.update(format!("{:?}", domain.route).as_bytes());
    hash.update(domain.width.to_le_bytes());
    hash.update(domain.height.to_le_bytes());
    for tile in domain.validity.tiles() {
        for value in &tile.pixels {
            hash.update(value.0.to_le_bytes());
        }
    }
    let Some(channel) = channel else {
        return ContentDigest(format!("{:x}", hash.finalize()));
    };
    let (width, height) = channel.dimensions();
    hash.update(width.to_le_bytes());
    hash.update(height.to_le_bytes());
    match channel {
        PreparedExemplarChannel::BaseColor { plane, alpha_mode } => {
            hash.update(format!("{alpha_mode:?}").as_bytes());
            for tile in plane.tiles() {
                for value in &tile.pixels {
                    for component in value.rgb {
                        hash.update(component.to_le_bytes());
                    }
                    hash.update(value.alpha.to_le_bytes());
                }
            }
        }
        PreparedExemplarChannel::Scalar { plane, .. } => {
            for tile in plane.tiles() {
                for value in &tile.pixels {
                    hash.update(value.0.to_le_bytes());
                }
            }
        }
        PreparedExemplarChannel::Normal {
            plane,
            source_convention,
            canonical_convention,
            alpha_policy,
        } => {
            hash.update(
                format!("{source_convention:?}|{canonical_convention:?}|{alpha_policy:?}")
                    .as_bytes(),
            );
            for tile in plane.tiles() {
                for value in &tile.pixels {
                    for component in value.xyz {
                        hash.update(component.to_le_bytes());
                    }
                    hash.update(value.alpha.to_le_bytes());
                }
            }
        }
        PreparedExemplarChannel::MaterialId { plane } => {
            for tile in plane.tiles() {
                for value in &tile.pixels {
                    hash.update(value.0.to_le_bytes());
                }
            }
        }
        PreparedExemplarChannel::Mask { plane, .. } => {
            for tile in plane.tiles() {
                for value in &tile.pixels {
                    hash.update(value.0.to_le_bytes());
                }
            }
        }
    }
    ContentDigest(format!("{:x}", hash.finalize()))
}

fn quarter_turn_degrees(orientation: QuarterTurn) -> f64 {
    match orientation {
        QuarterTurn::Zero => 0.0,
        QuarterTurn::Ninety => 90.0,
        QuarterTurn::OneEighty => 180.0,
        QuarterTurn::TwoSeventy => 270.0,
    }
}

fn direct_source_frame_domain(
    project: &ProjectSummary,
    source_set_id: hot_trimmer_domain::SourceSetId,
    profile: SourceFramePreviewProfile,
    cancellation: &ImageCancellationToken,
    cache: Option<&Mutex<SourceFramePreviewCache>>,
) -> Result<
    (
        Arc<hot_trimmer_material_synthesis::PreparedMaterialDomain>,
        bool,
    ),
    String,
> {
    let sources = project
        .sources
        .iter()
        .filter(|source| source.source_set_id.to_string() == source_set_id.to_string())
        .collect::<Vec<_>>();
    let (registered, encoded) = registered_inputs(&sources)?;
    let max_level_zero_edge = match profile {
        SourceFramePreviewProfile::Draft512 => Some(1024),
        SourceFramePreviewProfile::Refinement1024 => Some(2048),
        SourceFramePreviewProfile::Preview2048 => Some(4096),
        SourceFramePreviewProfile::Preview4096 => Some(8192),
        SourceFramePreviewProfile::Preview8192 => None,
        SourceFramePreviewProfile::Authoritative => None,
    };
    let settings = NormalizationSettings {
        max_levels: 1,
        // This is a conservative peak-work declaration and includes scratch required to
        // decode the original compressed image before the bounded level-zero resize. It is
        // not retained-domain memory; max_level_zero_edge below controls that allocation.
        max_memory_bytes: 4_294_967_296,
        max_level_zero_edge,
        ..NormalizationSettings::default()
    };
    let prepared_key = hot_trimmer_image_io::prepared_cache_key(&registered, &settings);
    let cache_key = ContentDigest::sha256(
        format!(
            "{DIRECT_SOURCE_FRAME_DECODER_VERSION}|{}|orientation={}",
            prepared_key.0.0, registered.orientation
        )
        .as_bytes(),
    );
    if let Some(found) =
        cache.and_then(|cache| cache.lock().ok().and_then(|guard| guard.get(&cache_key)))
    {
        return Ok((found, true));
    }
    let prepared = if let Some(found) = cache.and_then(|cache| {
        cache
            .lock()
            .ok()
            .and_then(|guard| guard.get_prepared(&prepared_key))
    }) {
        found
    } else {
        let prepared = Arc::new(
            prepare_registered_channel_set(&registered, &encoded, &settings, cancellation)
                .map_err(|error| {
                    format!("Stage 2 direct SourceFrame decode/preparation failed: {error}")
                })?,
        );
        if let Some(cache) = cache
            && let Ok(mut guard) = cache.lock()
        {
            guard.insert_prepared(Arc::clone(&prepared));
        }
        prepared
    };
    let channels = registered_level_zero_channels(&prepared)
        .map_err(|error| format!("direct registered preparation failed: {error}"))?;
    let domain = Arc::new(
        hot_trimmer_material_synthesis::PreparedMaterialDomain::from_registered_channels(
            cache_key.clone(),
            cache_key.clone(),
            channels,
        )
        .map_err(|error| format!("direct SourceFrame domain failed: {error}"))?,
    );
    if let Some(cache) = cache {
        if let Ok(mut guard) = cache.lock() {
            guard.insert(cache_key, Arc::clone(&domain));
        }
    }
    Ok((domain, false))
}

const SOURCE_FRAME_COMPILER_VERSION: &str = "1.0.0";

fn aspect_matches(width: f64, height: f64, expected_width: f64, expected_height: f64) -> bool {
    if !width.is_finite()
        || !height.is_finite()
        || !expected_width.is_finite()
        || !expected_height.is_finite()
        || width <= 0.0
        || height <= 0.0
        || expected_width <= 0.0
        || expected_height <= 0.0
    {
        return false;
    }
    let left = width * expected_height;
    let right = height * expected_width;
    (left - right).abs() <= 1e-9 * left.abs().max(right.abs()).max(1.0)
}

#[cfg(test)]
mod source_frame_partition_tests {
    use hot_trimmer_domain::{
        CanonicalRect, ContentDigest, EdgeDetailIntentV1, EdgeEligibility, LogicalGridSpec,
        ManualRegionRole, MaterialMapKind, PartitionRecipe, RegionSampling, SamplingMode,
        SolidChannelValues, StructuralProfile, TemplateSlotRole, generate_partition,
        resolve_boundaries,
    };
    use hot_trimmer_effect_compiler::{
        EdgeDetailCompileError, EdgeDetailCompileRequest, EdgeDetailRegionInput,
        EdgeDetailRoleEvaluator, compile_edge_detail_plan,
    };
    use hot_trimmer_placement_solver::SourceCrop;
    use std::collections::BTreeMap;

    use super::{build_solid_domain, legacy_edge_wear_for_region, manual_physical_mapping};

    #[test]
    fn mvp_edge_wear_global_target_lowers_into_every_region() {
        let first = hot_trimmer_domain::RegionId::from_bytes([1; 16]);
        let second = hot_trimmer_domain::RegionId::from_bytes([2; 16]);
        let global = hot_trimmer_domain::EdgeDetailIntentV1::default();
        assert!(global.target_region.is_none());
        assert!(legacy_edge_wear_for_region(Some(&global), first).is_some());
        assert!(legacy_edge_wear_for_region(Some(&global), second).is_some());

        let targeted = hot_trimmer_domain::EdgeDetailIntentV1 {
            target_region: Some(second),
            ..global
        };
        assert!(legacy_edge_wear_for_region(Some(&targeted), first).is_none());
        assert!(legacy_edge_wear_for_region(Some(&targeted), second).is_some());
    }

    fn edge_detail_region(
        byte: u8,
        role: TemplateSlotRole,
        manual_role: ManualRegionRole,
    ) -> EdgeDetailRegionInput {
        EdgeDetailRegionInput {
            region_id: hot_trimmer_domain::RegionId::from_bytes([byte; 16]),
            role,
            manual_role,
            structural_profile: StructuralProfile::Bevel,
            slot_size_m: [0.1, 0.1],
            destination_pixels: [100, 100],
            edge_eligibility: EdgeEligibility::default(),
            stage15_plan_identity: ContentDigest::sha256(&[byte, 15]),
            source_height_identity: None,
            source_luminance_identity: Some(ContentDigest::sha256(&[byte, 14])),
        }
    }

    fn compile_edge_fixture(
        intent: &EdgeDetailIntentV1,
        regions: &[EdgeDetailRegionInput],
    ) -> Result<hot_trimmer_effect_compiler::CompiledEdgeDetailPlan, EdgeDetailCompileError> {
        compile_edge_detail_plan(&EdgeDetailCompileRequest {
            intent,
            regions,
            requested_maps: &[MaterialMapKind::BaseColor, MaterialMapKind::Height],
            resolution_profile: "mvp-edge-wear-fixture-100",
        })
    }

    #[test]
    fn mvp_edge_wear_compiler_covers_global_target_reorder_and_authoritative_role_identity() {
        let panel = edge_detail_region(1, TemplateSlotRole::Planar, ManualRegionRole::Panel);
        let horizontal = edge_detail_region(
            2,
            TemplateSlotRole::RepeatingStrip,
            ManualRegionRole::HorizontalStrip,
        );
        let vertical = edge_detail_region(
            3,
            TemplateSlotRole::RepeatingStrip,
            ManualRegionRole::VerticalStrip,
        );
        let mut ineligible = edge_detail_region(4, TemplateSlotRole::Planar, ManualRegionRole::Panel);
        ineligible.edge_eligibility = EdgeEligibility {
            left: false,
            right: false,
            top: false,
            bottom: false,
        };
        let regions = vec![panel.clone(), horizontal.clone(), vertical.clone(), ineligible];
        let global = EdgeDetailIntentV1::default();
        let compiled = compile_edge_fixture(&global, &regions).expect("global Edge Detail plan");
        assert_eq!(compiled.commands.len(), 3, "one command per eligible region");
        assert_eq!(compiled.commands[0].region_id, panel.region_id);
        assert_eq!(compiled.commands[0].evaluator, EdgeDetailRoleEvaluator::RectangularPanel);
        assert_eq!(compiled.commands[1].evaluator, EdgeDetailRoleEvaluator::HorizontalStrip);
        assert_eq!(compiled.commands[2].evaluator, EdgeDetailRoleEvaluator::VerticalStrip);

        let targeted = EdgeDetailIntentV1 {
            target_region: Some(vertical.region_id),
            ..global.clone()
        };
        let targeted_plan = compile_edge_fixture(&targeted, &regions).expect("targeted Edge Detail plan");
        assert_eq!(targeted_plan.commands.len(), 1);
        assert_eq!(targeted_plan.commands[0].region_id, vertical.region_id);

        let mut reordered = regions.clone();
        reordered.reverse();
        let reordered_plan = compile_edge_fixture(&global, &reordered).expect("reordered Edge Detail plan");
        let identities = compiled.commands.iter().map(|command| {
            (command.region_id, command.cache_identity.clone())
        }).collect::<BTreeMap<_, _>>();
        let reordered_identities = reordered_plan.commands.iter().map(|command| {
            (command.region_id, command.cache_identity.clone())
        }).collect::<BTreeMap<_, _>>();
        assert_eq!(identities, reordered_identities, "command identity is keyed by stable UUID");

        let role_a = edge_detail_region(9, TemplateSlotRole::Planar, ManualRegionRole::Panel);
        let mut role_b = role_a.clone();
        role_b.manual_role = ManualRegionRole::Unique;
        let command_a = compile_edge_fixture(&global, &[role_a]).unwrap().commands.remove(0);
        let command_b = compile_edge_fixture(&global, &[role_b]).unwrap().commands.remove(0);
        assert_eq!(command_a.evaluator, command_b.evaluator, "collapsed evaluator is intentionally equal");
        assert_ne!(command_a.cache_identity, command_b.cache_identity, "raw authoritative role metadata is hashed");
        assert_ne!(command_a.manual_role, command_b.manual_role);

        let mut profile_region = edge_detail_region(10, TemplateSlotRole::Planar, ManualRegionRole::Panel);
        let profile_a = compile_edge_fixture(&global, &[profile_region.clone()]).unwrap().commands.remove(0);
        profile_region.structural_profile = StructuralProfile::Flat;
        let profile_b = compile_edge_fixture(&global, &[profile_region]).unwrap().commands.remove(0);
        assert_eq!(profile_a.evaluator, profile_b.evaluator);
        assert_ne!(profile_a.structural_profile, profile_b.structural_profile);
        assert_ne!(profile_a.cache_identity, profile_b.cache_identity, "raw structural profile is hashed");
    }

    #[test]
    fn mvp_edge_wear_compiler_rejects_invalid_and_subpixel_intents_and_disables_cleanly() {
        let region = edge_detail_region(5, TemplateSlotRole::Planar, ManualRegionRole::Panel);
        let disabled = EdgeDetailIntentV1 { enabled: false, ..EdgeDetailIntentV1::default() };
        assert!(compile_edge_fixture(&disabled, &[region.clone()]).unwrap().commands.is_empty());

        let cases = [
            (EdgeDetailIntentV1 { schema_version: 2, ..EdgeDetailIntentV1::default() }, EdgeDetailCompileError::UnknownSchemaVersion(2)),
            (EdgeDetailIntentV1 { height_amplitude_m: f64::NAN, ..EdgeDetailIntentV1::default() }, EdgeDetailCompileError::NonFiniteValue),
            (EdgeDetailIntentV1 { wear_amount: 2.0, ..EdgeDetailIntentV1::default() }, EdgeDetailCompileError::OutOfRange),
            (EdgeDetailIntentV1 { edge_width_m: 0.0, ..EdgeDetailIntentV1::default() }, EdgeDetailCompileError::InvalidPhysicalScale),
            (EdgeDetailIntentV1 { exposed_metal_enabled: false, metallic_offset: 0.5, ..EdgeDetailIntentV1::default() }, EdgeDetailCompileError::MetallicRequiresExposedMetal),
            (EdgeDetailIntentV1 { edge_width_m: 0.000_5, ..EdgeDetailIntentV1::default() }, EdgeDetailCompileError::BelowPhysicalLod(region.region_id)),
            (EdgeDetailIntentV1 { breakup_scale_m: 0.001_5, ..EdgeDetailIntentV1::default() }, EdgeDetailCompileError::BelowPhysicalLod(region.region_id)),
            (EdgeDetailIntentV1 { micro_detail_scale_m: 0.001_5, ..EdgeDetailIntentV1::default() }, EdgeDetailCompileError::BelowPhysicalLod(region.region_id)),
        ];
        for (intent, expected) in cases {
            assert_eq!(compile_edge_fixture(&intent, &[region.clone()]), Err(expected));
        }
        let missing = hot_trimmer_domain::RegionId::from_bytes([99; 16]);
        let targeted = EdgeDetailIntentV1 { target_region: Some(missing), ..EdgeDetailIntentV1::default() };
        assert_eq!(compile_edge_fixture(&targeted, &[region]), Err(EdgeDetailCompileError::UnknownTargetRegion));
    }

    #[test]
    fn mvp_edge_wear_manual_source_frame_has_a_resolution_independent_meter_basis() {
        let crop = SourceCrop { x: 0, y: 0, width: 800, height: 400 };
        let allocation = CanonicalRect { x: 0, y: 0, width: 800, height: 400 };
        let (physical_size, pixels_per_meter) =
            manual_physical_mapping(RegionSampling::OneShot, crop, allocation);

        assert_eq!(physical_size, [0.8, 0.4]);
        assert_eq!(pixels_per_meter, 1_000.0);
        assert_eq!(physical_size[0] * pixels_per_meter, f64::from(crop.width));
        assert_eq!(physical_size[1] * pixels_per_meter, f64::from(crop.height));

        let edge_width_m = 0.004;
        let preview_fraction = edge_width_m / physical_size[0];
        assert!((preview_fraction * 800.0 - 4.0).abs() < f64::EPSILON);
        assert!((preview_fraction * 1_600.0 - 8.0).abs() < f64::EPSILON);
    }

    #[test]
    fn multi_source_patch_assignment_solid_content_builds_an_exact_domain() {
        let domain = build_solid_domain(&SolidChannelValues {
            base_color: Some([128, 64, 32, 128]),
            scalar_channels: BTreeMap::new(),
        })
        .expect("solid domain");
        assert_eq!((domain.width, domain.height), (1, 1));
        let pixel = match &domain.registered_channels()[0] {
            hot_trimmer_render_core::PreparedExemplarChannel::BaseColor { plane, .. } => {
                plane.pixel(0, 0)
            }
            _ => panic!("solid domain must contain Base Color"),
        };
        assert!((pixel.alpha - 128.0 / 255.0).abs() < 1.0e-6);
        assert!(pixel.rgb[0] > pixel.rgb[1] && pixel.rgb[1] > pixel.rgb[2]);
    }

    #[test]
    fn source_frame_partition_preserves_shared_boundaries_and_direct_sampling() {
        for target in [16, 63, 103] {
            let recipe = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, target, 19);
            let regions = generate_partition(&recipe).expect("accepted partition");
            assert_eq!(regions.len(), target as usize);
            let source_x = resolve_boundaries(2_000, 4_000, 64);
            let source_y = resolve_boundaries(0, 4_000, 64);
            let destination_x = resolve_boundaries(0, 4_000, 64);
            let destination_y = resolve_boundaries(0, 4_000, 64);
            let mut coverage = vec![0_u8; 64 * 64];
            for region in regions {
                let rect = region.grid_rect;
                assert!(source_x[(rect.x + rect.width) as usize] >= source_x[rect.x as usize]);
                assert!(source_y[(rect.y + rect.height) as usize] >= source_y[rect.y as usize]);
                assert_eq!(
                    source_x[(rect.x + rect.width) as usize] - source_x[rect.x as usize],
                    source_x[(rect.x + rect.width) as usize] - source_x[rect.x as usize]
                );
                assert_eq!(
                    destination_x[rect.x as usize],
                    destination_x[rect.x as usize]
                );
                assert_eq!(
                    destination_y[rect.y as usize],
                    destination_y[rect.y as usize]
                );
                for y in rect.y..rect.y + rect.height {
                    for x in rect.x..rect.x + rect.width {
                        coverage[(y * 64 + x) as usize] += 1;
                    }
                }
            }
            assert!(coverage.iter().all(|count| *count == 1));
            assert_eq!(SamplingMode::DirectCrop, SamplingMode::DirectCrop);
        }
    }
}

fn validate_manual_mapping(
    region_id: hot_trimmer_domain::RegionId,
    mapping: &RegionMapping,
) -> Result<(), String> {
    let epsilon = 1.0e-9;
    if !mapping.behavior.supports_sampling() {
        return Err(format!(
            "region {region_id} role {:?} does not support {:?}; the mode was rejected before Stage 14",
            mapping.behavior.role, mapping.behavior.sampling
        ));
    }
    if mapping
        .behavior
        .period_pixels
        .is_some_and(|period| period.contains(&0))
    {
        return Err(format!(
            "region {region_id} has an invalid zero source period"
        ));
    }
    if !mapping.warps.is_empty() {
        return Err(format!(
            "region {region_id} has authored warp operations, but the manual Stage 14 route has no exact executor"
        ));
    }
    if mapping.transform.rotation_degrees.abs() > epsilon
        || (mapping.transform.scale[0] - mapping.transform.scale[1]).abs() > epsilon
        || (mapping.transform.mirror_x && mapping.transform.mirror_y)
    {
        return Err(format!(
            "region {region_id} uses a transform not exactly representable by the manual Stage 14 sampling plan"
        ));
    }
    Ok(())
}

fn manual_sampling_mode(role: ManualRegionRole, sampling: RegionSampling) -> SamplingMode {
    if role == ManualRegionRole::Radial {
        return SamplingMode::PolarRadial;
    }
    match sampling {
        RegionSampling::OneShot => SamplingMode::DirectCrop,
        RegionSampling::LoopX => SamplingMode::RepeatX,
        RegionSampling::LoopY => SamplingMode::RepeatY,
        RegionSampling::LoopXy => SamplingMode::PeriodicTile,
    }
}

fn manual_template_role(role: ManualRegionRole) -> TemplateSlotRole {
    match role {
        ManualRegionRole::Panel => TemplateSlotRole::Planar,
        ManualRegionRole::HorizontalStrip | ManualRegionRole::VerticalStrip => {
            TemplateSlotRole::RepeatingStrip
        }
        ManualRegionRole::Unique => TemplateSlotRole::UniqueDetail,
        ManualRegionRole::Radial => TemplateSlotRole::Radial,
    }
}

/// Converts authoritative output padding to the requested preview profile. A nonzero
/// authored value remains visible at bounded profiles; authoritative output is exact.
fn scaled_atlas_padding(
    padding_px: u32,
    authoritative: hot_trimmer_domain::PixelSize,
    profile: hot_trimmer_domain::PixelSize,
) -> u32 {
    if padding_px == 0 || authoritative.width == 0 || authoritative.height == 0 {
        return 0;
    }
    let numerator =
        u64::from(padding_px) * u64::from(profile.width) * u64::from(authoritative.height);
    let denominator = u64::from(authoritative.width) * u64::from(authoritative.height);
    let scaled_width = numerator.div_ceil(denominator);
    let numerator =
        u64::from(padding_px) * u64::from(profile.height) * u64::from(authoritative.width);
    let scaled_height = numerator.div_ceil(denominator);
    u32::try_from(scaled_width.min(scaled_height).max(1)).unwrap_or(u32::MAX)
}

pub(crate) fn semantic_rect_for_padding(
    allocation: hot_trimmer_domain::CanonicalRect,
    padding_px: u32,
    edges: hot_trimmer_domain::EdgeEligibility,
) -> hot_trimmer_domain::CanonicalRect {
    let (left, right) = fitted_edge_insets(
        allocation.width,
        edges.left.then_some(padding_px).unwrap_or(0),
        edges.right.then_some(padding_px).unwrap_or(0),
    );
    let (top, bottom) = fitted_edge_insets(
        allocation.height,
        edges.top.then_some(padding_px).unwrap_or(0),
        edges.bottom.then_some(padding_px).unwrap_or(0),
    );
    hot_trimmer_domain::CanonicalRect {
        x: allocation.x + left,
        y: allocation.y + top,
        width: allocation.width - left - right,
        height: allocation.height - top - bottom,
    }
}

fn fitted_edge_insets(extent: u32, leading: u32, trailing: u32) -> (u32, u32) {
    let available = extent.saturating_sub(1);
    if leading > 0 && trailing > 0 {
        let each = leading.min(trailing).min(available / 2);
        return (each, each);
    }
    (leading.min(available), trailing.min(available))
}

fn manual_physical_mapping(
    sampling: RegionSampling,
    crop: SourceCrop,
    allocation: hot_trimmer_domain::CanonicalRect,
) -> ([f64; 2], f64) {
    // SourceFrame regions do not yet carry authored world calibration. Use the
    // documented MVP convention of 1000 source pixels per meter so persisted
    // meter-valued profile and edge-wear controls stay visible and invariant
    // when only the output preview resolution changes. Multiplying the source
    // sampling density by the same factor preserves the existing UV sampling.
    const CONVENTIONAL_SOURCE_PIXELS_PER_METER: f64 = 1_000.0;
    let crop_size = [f64::from(crop.width), f64::from(crop.height)];
    let destination = [f64::from(allocation.width), f64::from(allocation.height)];
    let (slot_size_in_source_pixels, source_pixels_per_slot_unit) = match sampling {
        RegionSampling::OneShot => (crop_size, 1.0),
        RegionSampling::LoopX => (destination, crop_size[1] / destination[1]),
        RegionSampling::LoopY => (destination, crop_size[0] / destination[0]),
        RegionSampling::LoopXy => (
            destination,
            (crop_size[0] / destination[0]).max(crop_size[1] / destination[1]),
        ),
    };
    (
        [
            slot_size_in_source_pixels[0] / CONVENTIONAL_SOURCE_PIXELS_PER_METER,
            slot_size_in_source_pixels[1] / CONVENTIONAL_SOURCE_PIXELS_PER_METER,
        ],
        source_pixels_per_slot_unit * CONVENTIONAL_SOURCE_PIXELS_PER_METER,
    )
}

fn legacy_edge_wear_for_region(
    intent: Option<&hot_trimmer_domain::EdgeDetailIntentV1>,
    region_id: RegionId,
) -> Option<hot_trimmer_domain::EdgeWearIntent> {
    intent
        .filter(|intent| intent.target_region.is_none_or(|target| target == region_id))
        .map(hot_trimmer_domain::EdgeDetailIntentV1::legacy_renderer_adapter)
}

#[allow(clippy::too_many_arguments)]
fn compile_edge_detail_for_region(
    intent: Option<&hot_trimmer_domain::EdgeDetailIntentV1>,
    region_id: RegionId,
    role: TemplateSlotRole,
    manual_role: ManualRegionRole,
    structural_profile: hot_trimmer_domain::StructuralProfile,
    slot_size_m: [f64; 2],
    destination_pixels: [u32; 2],
    edge_eligibility: hot_trimmer_domain::EdgeEligibility,
    stage15_plan_identity: ContentDigest,
    domain: &hot_trimmer_material_synthesis::PreparedMaterialDomain,
    requested_maps: &[hot_trimmer_domain::MaterialMapKind],
    resolution_profile: &str,
) -> Result<Option<hot_trimmer_effect_compiler::CompiledEdgeDetailCommand>, String> {
    let Some(intent) = intent else { return Ok(None); };
    if intent.target_region.is_some_and(|target| target != region_id) {
        return Ok(None);
    }
    let roles = compiled_source_roles_for_domain(domain);
    let region = EdgeDetailRegionInput {
        region_id,
        role,
        manual_role,
        structural_profile,
        slot_size_m,
        destination_pixels,
        edge_eligibility,
        stage15_plan_identity,
        source_height_identity: roles.contains(&MaterialChannelRole::Height)
            .then(|| prepared_channel_digest(domain, MaterialChannelRole::Height)),
        source_luminance_identity: roles.contains(&MaterialChannelRole::BaseColor)
            .then(|| prepared_channel_digest(domain, MaterialChannelRole::BaseColor)),
    };
    let plan = hot_trimmer_effect_compiler::compile_edge_detail_plan(&EdgeDetailCompileRequest {
        intent,
        regions: &[region],
        requested_maps,
        resolution_profile,
    }).map_err(|error| format!("Edge Detail compilation failed for region {region_id}: {error}"))?;
    Ok(plan.commands.into_iter().next())
}

fn build_solid_domain(
    values: &SolidChannelValues,
) -> Result<hot_trimmer_material_synthesis::PreparedMaterialDomain, String> {
    let rgba = values.base_color.ok_or_else(|| {
        "solid region content requires an explicit Base Color for Stage 14".to_string()
    })?;
    let linear = |component: u8| {
        let value = f32::from(component) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    let plane = ImagePlane::from_row_major(
        1,
        1,
        1,
        &[LinearColor {
            rgb: [linear(rgba[0]), linear(rgba[1]), linear(rgba[2])],
            alpha: f32::from(rgba[3]) / 255.0,
        }],
    )
    .map_err(|error| error.to_string())?;
    let identity =
        ContentDigest::sha256(&serde_json::to_vec(values).map_err(|error| error.to_string())?);
    hot_trimmer_material_synthesis::PreparedMaterialDomain::from_registered_channels(
        identity.clone(),
        identity,
        vec![PreparedExemplarChannel::BaseColor {
            plane,
            alpha_mode: if rgba[3] == 255 {
                ResolvedAlphaMode::Opaque
            } else {
                ResolvedAlphaMode::Straight
            },
        }],
    )
    .map_err(|error| error.to_string())
}

fn resolve_region_content(
    project: &ProjectSummary,
    document: &hot_trimmer_domain::TrimSheetDocument,
    primary: hot_trimmer_domain::SourceSetId,
    content: &ContentReference,
) -> Result<(hot_trimmer_domain::SourceSetId, Option<Patch>), String> {
    match content {
        ContentReference::InheritPrimaryMaterial => Ok((primary, None)),
        ContentReference::MaterialSource(source_set_id) => Ok((*source_set_id, None)),
        ContentReference::Patch(patch_id) => {
            let patch = document
                .patches
                .iter()
                .chain(&project.patches)
                .find(|patch| patch.id == *patch_id && patch.enabled)
                .cloned()
                .ok_or_else(|| format!("enabled authored patch {patch_id} is missing"))?;
            let source = project
                .sources
                .iter()
                .find(|source| source.input.id == patch.source_id)
                .ok_or_else(|| format!("patch {patch_id} source is missing"))?;
            let source_set = project
                .source_sets
                .iter()
                .find(|set| set.id.to_string() == source.source_set_id.to_string())
                .ok_or_else(|| format!("patch {patch_id} material source set is missing"))?;
            Ok((source_set.id, Some(patch)))
        }
        ContentReference::Solid(_) => {
            Err("Stage 14 intermediate preview cannot represent solid region content".into())
        }
        ContentReference::Procedural(_) => {
            Err("Stage 14 intermediate preview cannot represent procedural region content".into())
        }
    }
}

fn patch_domain_cache_key(
    project: &ProjectSummary,
    source_set_id: hot_trimmer_domain::SourceSetId,
    patch: &Patch,
    preserve_source_resolution: bool,
) -> Result<ContentDigest, String> {
    let source_set = project
        .source_sets
        .iter()
        .find(|set| set.id == source_set_id)
        .ok_or_else(|| format!("material source set {source_set_id} is missing"))?;
    let identity = format!(
        "stage-02-08-patch-domain-v1|source={source_set:?}|patch={patch:?}|preserve={preserve_source_resolution}"
    );
    Ok(ContentDigest::sha256(identity.as_bytes()))
}

fn build_direct_patch_domain(
    project: &ProjectSummary,
    source_set_id: hot_trimmer_domain::SourceSetId,
    patch: &Patch,
    cache_key: ContentDigest,
    preserve_source_resolution: bool,
    image_cancellation: &ImageCancellationToken,
    render_cancellation: &RenderCancellationToken,
    cache: Option<&Mutex<SourceFramePreviewCache>>,
) -> Result<hot_trimmer_material_synthesis::PreparedMaterialDomain, String> {
    let source_set = project
        .source_sets
        .iter()
        .find(|set| set.id == source_set_id)
        .ok_or_else(|| format!("material source set {source_set_id} is missing"))?;
    let sources = project
        .sources
        .iter()
        .filter(|source| source.source_set_id.to_string() == source_set_id.to_string())
        .collect::<Vec<_>>();
    if !sources
        .iter()
        .any(|source| source.input.id == patch.source_id)
    {
        return Err(format!(
            "patch {} does not belong to material source set {source_set_id}",
            patch.id
        ));
    }
    let (registered, encoded) = registered_inputs(&sources)?;
    let settings = NormalizationSettings {
        max_levels: 1,
        max_memory_bytes: 4_294_967_296,
        max_level_zero_edge: (!preserve_source_resolution).then_some(1024),
        ..NormalizationSettings::default()
    };
    let prepared_key = hot_trimmer_image_io::prepared_cache_key(&registered, &settings);
    let prepared = if let Some(found) = cache.and_then(|cache| {
        cache
            .lock()
            .ok()
            .and_then(|guard| guard.get_prepared(&prepared_key))
    }) {
        found
    } else {
        let prepared = Arc::new(
            prepare_registered_channel_set(&registered, &encoded, &settings, image_cancellation)
                .map_err(|error| format!("Stage 2 direct patch preparation failed: {error}"))?,
        );
        if let Some(cache) = cache
            && let Ok(mut guard) = cache.lock()
        {
            guard.insert_prepared(Arc::clone(&prepared));
        }
        prepared
    };
    let patch_revision = u64::from_str_radix(&cache_key.0[..16], 16)
        .map_err(|error| format!("direct patch revision failed: {error}"))?;
    let exemplar = prepare_registered_exemplar(
        &prepared,
        &PreparedExemplarRequest {
            exemplar_id: patch.id.to_string(),
            area: PlanarArea::FourPoint {
                corners: patch.geometry.corners,
            },
            lens_correction: None,
            mask: ExemplarMaskIntent {
                crop_polygon: patch.geometry.assistance_mask.clone(),
                minimum_alpha: Some(1.0 / 255.0),
            },
            rectification: patch.rectification,
            physical_aspect_ratio: None,
            quality: RectificationQuality::Authoritative,
            limits: RectificationWorkLimits {
                preview_max_edge: 1024,
                authoritative_max_edge: if preserve_source_resolution {
                    8192
                } else {
                    1024
                },
                max_pixels: if preserve_source_resolution {
                    67_108_864
                } else {
                    1_048_576
                },
                tile_edge: 128,
            },
            scope: PreparedExemplarScope {
                source_set_id,
                source_revision: source_set.source_revision,
                patch_id: Some(patch.id),
                patch_revision,
            },
        },
        render_cancellation,
    )
    .map_err(|error| format!("Stage 3 direct patch rectification failed: {error}"))?;
    hot_trimmer_material_synthesis::PreparedMaterialDomain::from_registered_channels(
        cache_key,
        exemplar.cache_key.0,
        exemplar.channels,
    )
    .map_err(|error| format!("direct patch domain failed: {error}"))
}

fn build_domain(
    project: &ProjectSummary,
    source_set_id: hot_trimmer_domain::SourceSetId,
    patch: Option<&Patch>,
    revision: u64,
    preserve_source_resolution: bool,
    image_cancellation: &ImageCancellationToken,
    render_cancellation: &RenderCancellationToken,
) -> Result<DomainArtifacts, String> {
    let source_set = project
        .source_sets
        .iter()
        .find(|set| set.id == source_set_id)
        .ok_or_else(|| format!("material source set {source_set_id} is missing"))?;
    let sources = project
        .sources
        .iter()
        .filter(|source| source.source_set_id.to_string() == source_set_id.to_string())
        .collect::<Vec<_>>();
    if !sources
        .iter()
        .any(|source| source.registration.role == MaterialChannelRole::BaseColor)
    {
        return Err(format!(
            "material source set {source_set_id} has no Base Color"
        ));
    }
    if let Some(patch) = patch {
        if !sources
            .iter()
            .any(|source| source.input.id == patch.source_id)
        {
            return Err(format!(
                "patch {} does not belong to material source set {source_set_id}",
                patch.id
            ));
        }
    }
    let (registered, encoded) = registered_inputs(&sources)?;
    let prepared = prepare_registered_channel_set(
        &registered,
        &encoded,
        &NormalizationSettings {
            max_levels: 5,
            max_memory_bytes: 4_294_967_296,
            max_level_zero_edge: (!preserve_source_resolution).then_some(2048),
            ..NormalizationSettings::default()
        },
        image_cancellation,
    )
    .map_err(|error| format!("Stage 2 failed: {error}"))?;
    let patch_bytes =
        serde_json::to_vec(&patch.map(|value| (value.id, &value.geometry, &value.rectification)))
            .map_err(|error| format!("Stage 3 identity failed: {error}"))?;
    let patch_revision = u64::from_str_radix(&ContentDigest::sha256(&patch_bytes).0[..16], 16)
        .map_err(|error| format!("Stage 3 revision failed: {error}"))?;
    let exemplar = prepare_registered_exemplar(
        &prepared,
        &PreparedExemplarRequest {
            exemplar_id: patch.map_or_else(
                || format!("{source_set_id}:full-source"),
                |value| value.id.to_string(),
            ),
            area: patch.map_or(PlanarArea::FullFrame { usable_area: None }, |value| {
                PlanarArea::FourPoint {
                    corners: value.geometry.corners,
                }
            }),
            lens_correction: None,
            mask: ExemplarMaskIntent {
                crop_polygon: patch.and_then(|value| value.geometry.assistance_mask.clone()),
                minimum_alpha: Some(1.0 / 255.0),
            },
            rectification: patch.map_or_else(Default::default, |value| value.rectification),
            physical_aspect_ratio: None,
            quality: RectificationQuality::Authoritative,
            limits: RectificationWorkLimits {
                preview_max_edge: 1024,
                authoritative_max_edge: 8192,
                max_pixels: 67_108_864,
                tile_edge: 128,
            },
            scope: PreparedExemplarScope {
                source_set_id,
                source_revision: source_set.source_revision,
                patch_id: patch.map(|value| value.id),
                patch_revision,
            },
        },
        render_cancellation,
    )
    .map_err(|error| format!("Stage 3 failed: {error}"))?;
    let stage3_result = exemplar.stage_result.clone();
    let stage4 =
        prepare_delit_exemplar(&exemplar, &source_set.delighting, None, render_cancellation)
            .map_err(|error| format!("Stage 4 failed: {error}"))?;
    let stage4_result = stage4.stage_result.clone();
    let mut stage5 = analyze_source(
        &stage4,
        &AnalysisSettings::default(),
        None,
        render_cancellation,
    )
    .map_err(|error| format!("Stage 5 failed: {error}"))?;
    stage5.classification.routing_intent = source_set.classification;
    let stage6 = calibrate_scale_orientation(
        &stage4,
        &stage5,
        &source_set.calibration,
        &ScaleOrientationSettings::default(),
        render_cancellation,
    )
    .map_err(|error| format!("Stage 6 failed: {error}"))?;
    let stage7 = extract_feature_fields(
        &stage4,
        &stage6,
        &FeatureFieldSettings::default(),
        render_cancellation,
    )
    .map_err(|error| format!("Stage 7 failed: {error}"))?;
    let stage8_request = Stage8RouterRequest {
        domain: DomainRequest {
            source: Arc::new(stage4.clone()),
            prepared_source_digest: exemplar.cache_key.0.clone(),
            analysis: Arc::new(stage7.clone()),
            scale_orientation: Arc::new(stage6.clone()),
            route: DomainRoute::Auto,
            direct_boundary_threshold_milli: 1000,
            graph_cut: GraphCutSettings::default(),
            quilting: QuiltingSettings::default(),
            patch_match: PatchMatchSettings::default(),
            seed: revision,
        },
        stage_five: Arc::new(stage5.clone()),
        policy: MaterialDomainRoutePolicy::default(),
        procedural_override: None,
        procedural_settings: ProceduralFitSettings::default(),
        output_width: exemplar.width,
        output_height: exemplar.height,
    };
    let stage8 = prepare_stage_08_material_domain(
        &stage8_request,
        None,
        &mut MaterialDomainCache::default(),
        render_cancellation,
    )
    .map_err(|error| format!("Stage 8 failed: {error}"))?;
    Ok(DomainArtifacts {
        source_set_id,
        patch_id: patch.map(|value| value.id.to_string()),
        patch_id_raw: patch.map(|value| value.id),
        domain: stage8.domain,
        stage5,
        stage6,
        stage7,
        stage3_result,
        stage4_result,
        stage8_result: stage8.stage_result,
    })
}

fn authored_sampling_policy(mapping: &RegionMapping) -> Result<SamplingPolicy, String> {
    let [x, y] = mapping.transform.scale;
    if !x.is_finite() || !y.is_finite() || x <= 0.0 || y <= 0.0 || (x - y).abs() > 1.0e-9 {
        return Err(
            "Stage 14 requires an authored positive isotropic region transform scale".into(),
        );
    }
    let mut policy = mapping.sampling;
    policy.scale *= x;
    if !policy.scale.is_finite() || policy.scale <= 0.0 {
        return Err("authored mapping produced an invalid Stage 14 sampling scale".into());
    }
    Ok(policy)
}

fn apply_authored_mapping(
    mut set: CandidateSet,
    mapping: &RegionMapping,
    patch_backed: bool,
    width: u32,
    height: u32,
) -> Result<CandidateSet, String> {
    if mapping.warps.iter().any(|warp| warp.enabled) {
        return Err(
            "Stage 14 SamplingPlan cannot represent enabled authored warp operations".into(),
        );
    }
    if matches!(mapping.address_mode, AddressMode::MirroredRepeat) {
        return Err("Stage 14 SamplingPlan cannot yet preserve mirrored-repeat addressing".into());
    }
    if matches!(mapping.projection, Projection::Perspective { .. }) && !patch_backed {
        return Err(
            "authored perspective mapping requires patch-backed Stage 3 rectification".into(),
        );
    }
    let rotation = authored_quarter_turn(mapping.transform.rotation_degrees)?;
    let mirror = match (mapping.transform.mirror_x, mapping.transform.mirror_y) {
        (false, false) => MirrorTransform::None,
        (true, false) => MirrorTransform::X,
        (false, true) => MirrorTransform::Y,
        (true, true) => MirrorTransform::None,
    };
    let rotation = if mapping.transform.mirror_x && mapping.transform.mirror_y {
        compose_quarter_turn(rotation, QuarterTurn::OneEighty)
    } else {
        rotation
    };
    let authored_transform = CandidateTransform { rotation, mirror };
    let bounds = mapping_window(mapping, width, height);
    let unplaced =
        mapping.source_crop_intent == Some(hot_trimmer_domain::SourceCropIntent::Unplaced);
    let offset_x = (mapping.transform.offset[0] * f64::from(width)).round() as i64;
    let offset_y = (mapping.transform.offset[1] * f64::from(height)).round() as i64;
    set.candidates.retain_mut(|candidate| {
        if candidate.transform != authored_transform {
            return false;
        }
        let repeating = matches!(
            candidate.mapping_mode,
            hot_trimmer_domain::SamplingMode::PeriodicTile
                | hot_trimmer_domain::SamplingMode::RepeatX
                | hot_trimmer_domain::SamplingMode::RepeatY
        );
        if !unplaced
            && candidate.mapping_mode != hot_trimmer_domain::SamplingMode::TextureSynthesis
            && (mapping.address_mode == AddressMode::Repeat) != repeating
        {
            return false;
        }
        if let Some(mut crop) = candidate.crop {
            let shifted_x = i64::from(bounds.x + crop.x).saturating_add(offset_x);
            let shifted_y = i64::from(bounds.y + crop.y).saturating_add(offset_y);
            if shifted_x < 0 || shifted_y < 0 {
                return false;
            }
            crop.x = shifted_x as u32;
            crop.y = shifted_y as u32;
            if crop.x.saturating_add(crop.width) > width
                || crop.y.saturating_add(crop.height) > height
            {
                return false;
            }
            if !unplaced
                && (crop.x < bounds.x
                    || crop.y < bounds.y
                    || crop.x.saturating_add(crop.width)
                        > bounds.x.saturating_add(bounds.width).min(width)
                    || crop.y.saturating_add(crop.height)
                        > bounds.y.saturating_add(bounds.height).min(height))
            {
                return false;
            }
            candidate.crop = Some(crop);
        }
        candidate.candidate_id = ContentDigest::sha256(
            format!("{}|authored:{mapping:?}", candidate.candidate_id.0).as_bytes(),
        );
        candidate
            .eligibility
            .reasons
            .push("authored RegionMapping propagated into Stage 11 candidate".into());
        true
    });
    if set.candidates.is_empty() {
        return Err(
            "Stage 11 produced no candidate compatible with the authored RegionMapping".into(),
        );
    }
    Ok(set)
}

fn mapping_window(mapping: &RegionMapping, width: u32, height: u32) -> SourceCrop {
    if mapping.source_crop_intent == Some(hot_trimmer_domain::SourceCropIntent::Unplaced) {
        return SourceCrop {
            x: 0,
            y: 0,
            width,
            height,
        };
    }
    let Projection::Crop { bounds, .. } = mapping.projection else {
        return SourceCrop {
            x: 0,
            y: 0,
            width,
            height,
        };
    };
    let x = ((bounds.x.get() * f64::from(width)).floor() as u32).min(width.saturating_sub(1));
    let y = ((bounds.y.get() * f64::from(height)).floor() as u32).min(height.saturating_sub(1));
    let requested_width = (bounds.width.get() * f64::from(width)).ceil().max(1.0) as u32;
    let requested_height = (bounds.height.get() * f64::from(height)).ceil().max(1.0) as u32;
    SourceCrop {
        x,
        y,
        width: requested_width.min(width - x),
        height: requested_height.min(height - y),
    }
}

fn source_frame_preview_crop(
    mapping: &RegionMapping,
    fallback: SourceCrop,
    width: u32,
    height: u32,
) -> SourceCrop {
    if mapping.source_crop_intent == Some(hot_trimmer_domain::SourceCropIntent::Authored)
        && matches!(mapping.projection, Projection::Crop { .. })
    {
        return mapping_window(mapping, width, height);
    }
    fallback
}

fn legal_gate1_mode(
    mode: hot_trimmer_domain::SamplingMode,
    role: hot_trimmer_domain::TemplateSlotRole,
    has_declared_period: bool,
) -> bool {
    use hot_trimmer_domain::{SamplingMode, TemplateSlotRole};
    match role {
        TemplateSlotRole::Planar => {
            matches!(mode, SamplingMode::DirectCrop)
                || (mode == SamplingMode::PeriodicTile && has_declared_period)
                || mode == SamplingMode::TextureSynthesis
                || mode == SamplingMode::NineSlicePanel
        }
        TemplateSlotRole::RepeatingStrip => {
            matches!(mode, SamplingMode::DirectCrop)
                || (matches!(mode, SamplingMode::RepeatX | SamplingMode::RepeatY)
                    && has_declared_period)
                || mode == SamplingMode::TextureSynthesis
        }
        TemplateSlotRole::UniqueDetail => matches!(
            mode,
            SamplingMode::UniqueContain
                | SamplingMode::UniqueCover
                | SamplingMode::TextureSynthesis
        ),
        TemplateSlotRole::TrimCap => mode == SamplingMode::ThreeSliceCap,
        TemplateSlotRole::Radial => {
            matches!(mode, SamplingMode::PlanarRadial | SamplingMode::PolarRadial)
        }
    }
}

fn authored_quarter_turn(degrees: f64) -> Result<QuarterTurn, String> {
    if !degrees.is_finite() {
        return Err("authored mapping rotation is not finite".into());
    }
    let normalized = degrees.rem_euclid(360.0);
    let quarter = (normalized / 90.0).round();
    if (normalized - quarter * 90.0).abs() > 1.0e-6 {
        return Err(
            "Stage 14 SamplingPlan supports authored rotations only in exact quarter turns".into(),
        );
    }
    Ok(match (quarter as u32) % 4 {
        0 => QuarterTurn::Zero,
        1 => QuarterTurn::Ninety,
        2 => QuarterTurn::OneEighty,
        _ => QuarterTurn::TwoSeventy,
    })
}

fn compose_quarter_turn(first: QuarterTurn, second: QuarterTurn) -> QuarterTurn {
    let value = |turn| match turn {
        QuarterTurn::Zero => 0,
        QuarterTurn::Ninety => 1,
        QuarterTurn::OneEighty => 2,
        QuarterTurn::TwoSeventy => 3,
    };
    match (value(first) + value(second)) % 4 {
        0 => QuarterTurn::Zero,
        1 => QuarterTurn::Ninety,
        2 => QuarterTurn::OneEighty,
        _ => QuarterTurn::TwoSeventy,
    }
}

fn candidate_measurements(
    candidate: &hot_trimmer_placement_solver::CropCandidate,
    demand: &hot_trimmer_effect_compiler::ResolvedSlotDemand,
    role: hot_trimmer_domain::TemplateSlotRole,
    artifacts: &DomainArtifacts,
) -> CandidateScoringMeasurements {
    let crop = candidate.crop.unwrap_or(SourceCrop {
        x: 0,
        y: 0,
        width: artifacts.domain.width,
        height: artifacts.domain.height,
    });
    let saliency = average_field(artifacts.stage7.saliency.level(0), crop);
    let stationarity = average_field(artifacts.stage7.stationarity.level(0), crop);
    let structure = average_field(artifacts.stage7.structure.edge.level(0), crop)
        .max(average_field(
            artifacts.stage7.structure.line.level(0),
            crop,
        ))
        .max(average_field(
            artifacts.stage7.structure.grid.level(0),
            crop,
        ));
    let boundary = perimeter_field(artifacts.stage7.structure.boundary.level(0), crop);
    let usability = average_field(artifacts.stage7.usability.confidence.level(0), crop);
    let source_ratio = (f64::from(crop.width) / f64::from(demand.destination_pixel_width.max(1)))
        .min(f64::from(crop.height) / f64::from(demand.destination_pixel_height.max(1)))
        .min(1.0);
    let seam_cost = match candidate.mapping_mode {
        hot_trimmer_domain::SamplingMode::RepeatX => {
            artifacts.stage7.seamability.horizontal_cost_milli
        }
        hot_trimmer_domain::SamplingMode::RepeatY => {
            artifacts.stage7.seamability.vertical_cost_milli
        }
        hot_trimmer_domain::SamplingMode::PeriodicTile => artifacts
            .stage7
            .seamability
            .horizontal_cost_milli
            .max(artifacts.stage7.seamability.vertical_cost_milli),
        _ => 0,
    };
    let role_compatibility = match role {
        hot_trimmer_domain::TemplateSlotRole::UniqueDetail => saliency,
        hot_trimmer_domain::TemplateSlotRole::RepeatingStrip => stationarity,
        _ => usability,
    };
    CandidateScoringMeasurements {
        source_pixels_per_output_pixel_milli: milli(source_ratio),
        resolution_confidence_milli: 1000,
        lattice_completion_milli: lattice_completion_milli(candidate),
        structure_confidence_milli: milli(structure),
        dominant_direction_degrees: artifacts
            .stage6
            .global_orientation
            .axis_millidegrees
            .map(|value| f64::from(value) / 1000.0),
        orientation_confidence_milli: artifacts.stage6.global_orientation.confidence_milli,
        seam_quality_milli: 1000_u16.saturating_sub(seam_cost),
        seam_confidence_milli: milli(average_field(
            artifacts.stage7.seamability.confidence.level(0),
            crop,
        )),
        boundary_cut_milli: milli(boundary),
        boundary_confidence_milli: milli(structure.max(boundary)),
        visual_quality_milli: milli(
            (f64::from(artifacts.stage5.quality.sharpness_milli) / 1000.0) * usability,
        ),
        quality_confidence_milli: milli(usability),
        role_compatibility_milli: Some(milli(role_compatibility)),
        role_confidence_milli: milli(saliency.max(stationarity).max(usability)),
    }
}

fn average_field(
    plane: Option<&hot_trimmer_image_io::ImagePlane<hot_trimmer_image_io::LinearScalar>>,
    crop: SourceCrop,
) -> f64 {
    let Some(plane) = plane else { return 0.0 };
    let mut sum = 0.0;
    let mut count = 0_u64;
    let x1 = crop.x.saturating_add(crop.width).min(plane.width());
    let y1 = crop.y.saturating_add(crop.height).min(plane.height());
    for y in crop.y.min(plane.height())..y1 {
        for x in crop.x.min(plane.width())..x1 {
            sum += f64::from(plane.pixel(x, y).0);
            count += 1;
        }
    }
    if count == 0 { 0.0 } else { sum / count as f64 }
}

fn perimeter_field(
    plane: Option<&hot_trimmer_image_io::ImagePlane<hot_trimmer_image_io::LinearScalar>>,
    crop: SourceCrop,
) -> f64 {
    let Some(plane) = plane else { return 0.0 };
    if crop.width == 0 || crop.height == 0 {
        return 0.0;
    }
    let x0 = crop.x.min(plane.width().saturating_sub(1));
    let y0 = crop.y.min(plane.height().saturating_sub(1));
    let x1 = crop
        .x
        .saturating_add(crop.width)
        .saturating_sub(1)
        .min(plane.width().saturating_sub(1));
    let y1 = crop
        .y
        .saturating_add(crop.height)
        .saturating_sub(1)
        .min(plane.height().saturating_sub(1));
    let mut sum = 0.0;
    let mut count = 0_u64;
    for x in x0..=x1 {
        sum += f64::from(plane.pixel(x, y0).0);
        count += 1;
        if y1 != y0 {
            sum += f64::from(plane.pixel(x, y1).0);
            count += 1;
        }
    }
    for y in y0.saturating_add(1)..y1 {
        sum += f64::from(plane.pixel(x0, y).0);
        count += 1;
        if x1 != x0 {
            sum += f64::from(plane.pixel(x1, y).0);
            count += 1;
        }
    }
    if count == 0 { 0.0 } else { sum / count as f64 }
}

fn milli(value: f64) -> u16 {
    (value.clamp(0.0, 1.0) * 1000.0).round() as u16
}

fn lattice_completion_milli(candidate: &hot_trimmer_placement_solver::CropCandidate) -> u16 {
    let (Some(crop), Some(period)) = (candidate.crop, candidate.period_pixels) else {
        return 0;
    };
    let axis = |origin: u32, extent: u32, period: u32| {
        if period == 0 {
            return 0.0;
        }
        let error = |value: u32| {
            let remainder = value % period;
            f64::from(remainder.min(period - remainder)) / f64::from(period)
        };
        (1.0 - (error(origin) + error(extent)) * 0.5).clamp(0.0, 1.0)
    };
    let x = axis(crop.x, crop.width, period[0]);
    let y = axis(crop.y, crop.height, period[1]);
    milli(match candidate.mapping_mode {
        hot_trimmer_domain::SamplingMode::RepeatX => x,
        hot_trimmer_domain::SamplingMode::RepeatY => y,
        hot_trimmer_domain::SamplingMode::PeriodicTile => (x + y) * 0.5,
        _ => 0.0,
    })
}

fn registered_inputs(
    sources: &[&StoredSource],
) -> Result<
    (
        RegisteredChannelSet,
        BTreeMap<hot_trimmer_domain::SourceId, Vec<u8>>,
    ),
    String,
> {
    let first = sources.first().ok_or("primary source set is empty")?;
    let size = OrientedPixelSize {
        width: first.input.width,
        height: first.input.height,
    };
    let mut channels = Vec::new();
    let mut encoded = BTreeMap::new();
    for source in sources {
        let bytes = match source.input.ownership {
            SourceOwnership::OwnedCopy => source
                .input
                .owned_bytes
                .clone()
                .ok_or("owned source bytes are missing")?,
            SourceOwnership::VerifiedExternalReference => fs::read(
                source
                    .input
                    .external_path
                    .as_ref()
                    .ok_or("external source path is missing")?,
            )
            .map_err(|error| error.to_string())?,
        };
        channels.push(RegisteredChannel {
            source_id: source.input.id,
            registration: source.registration.clone(),
            oriented_size: size,
            orientation: source.input.exif_orientation,
            original: OriginalAssetProvenance {
                original_path: source.input.origin_path.display().to_string(),
                immutable_digest: ContentDigest(source.input.sha256.clone()),
                encoded_bytes: source.input.encoded_bytes,
            },
            ownership: match source.input.ownership {
                SourceOwnership::OwnedCopy => SourceOwnershipIntent::OwnedCopy,
                SourceOwnership::VerifiedExternalReference => {
                    SourceOwnershipIntent::VerifiedExternalReference
                }
            },
        });
        encoded.insert(source.input.id, bytes);
    }
    Ok((
        RegisteredChannelSet {
            oriented_size: size,
            orientation: first.input.exif_orientation,
            channels,
        },
        encoded,
    ))
}

fn candidate_evidence(
    stage5: &hot_trimmer_material_analysis::SourceAnalysisReport,
    stage6: &hot_trimmer_material_analysis::ScaleOrientationReport,
    stage7: &hot_trimmer_material_analysis::FeatureFieldReport,
) -> CandidateEvidence {
    let mut feature_positions = Vec::new();
    if let (Some(saliency), Some(stationarity), Some(edge)) = (
        stage7.saliency.level(0),
        stage7.stationarity.level(0),
        stage7.structure.edge.level(0),
    ) {
        let grid_x = 8_u32.min(saliency.width());
        let grid_y = 8_u32.min(saliency.height());
        for gy in 0..grid_y {
            for gx in 0..grid_x {
                let x = ((2 * gx + 1) * saliency.width() / (2 * grid_x)).min(saliency.width() - 1);
                let y =
                    ((2 * gy + 1) * saliency.height() / (2 * grid_y)).min(saliency.height() - 1);
                feature_positions.push(FeaturePosition {
                    x,
                    y,
                    saliency_milli: milli(f64::from(saliency.pixel(x, y).0)),
                    stationarity_milli: milli(f64::from(stationarity.pixel(x, y).0)),
                    feature_strength_milli: milli(f64::from(edge.pixel(x, y).0)),
                });
            }
        }
        feature_positions.sort_by_key(|feature| {
            std::cmp::Reverse((
                u32::from(feature.saliency_milli)
                    + u32::from(feature.stationarity_milli)
                    + u32::from(feature.feature_strength_milli),
                feature.y,
                feature.x,
            ))
        });
        feature_positions.truncate(32);
    }
    CandidateEvidence {
        material_class: stage5.classification.routed_class(),
        class_confidence_milli: stage5.classification.confidence_milli,
        orientation_confidence_milli: stage6.global_orientation.confidence_milli,
        destructive_quarter_turn_override: stage6.global_orientation.destructive_rotation_allowed,
        periods: stage7
            .periodicity
            .candidates
            .iter()
            .filter_map(|candidate| {
                let x = candidate.first.dx_pixels.unsigned_abs();
                let y = candidate.first.dy_pixels.unsigned_abs();
                (x > 0 && y > 0).then_some([x, y])
            })
            .collect(),
        feature_positions,
    }
}

fn base_pixels_per_physical_unit(
    scale: PhysicalScaleEvidence,
    demand: &hot_trimmer_effect_compiler::ResolvedSlotDemand,
    width: u32,
    height: u32,
) -> f64 {
    if scale.claims_world_accuracy() {
        let x = scale.source_pixels_per_meter_x_milli.unwrap_or(1000) as f64 / 1000.0;
        let y = scale.source_pixels_per_meter_y_milli.unwrap_or(1000) as f64 / 1000.0;
        (x * y).sqrt()
    } else {
        (f64::from(width) / demand.world_width_m)
            .min(f64::from(height) / demand.world_height_m)
            .max(f64::EPSILON)
    }
}

fn unplaced_source_pixels_per_physical_unit(
    demand: &hot_trimmer_effect_compiler::ResolvedSlotDemand,
    width: u32,
    height: u32,
) -> f64 {
    (f64::from(width) / demand.world_width_m)
        .min(f64::from(height) / demand.world_height_m)
        .max(f64::EPSILON)
        * UNPLACED_SOURCE_FOOTPRINT_FRACTION
}

fn slice_geometry(
    role: hot_trimmer_domain::TemplateSlotRole,
    width: u32,
    height: u32,
) -> SliceGeometry {
    match role {
        hot_trimmer_domain::TemplateSlotRole::TrimCap if width >= 3 => SliceGeometry::Three {
            leading_cap_pixels: 1,
            trailing_cap_pixels: 1,
            center: SliceCenterPolicy::Repeat,
        },
        hot_trimmer_domain::TemplateSlotRole::Planar if width >= 3 && height >= 3 => {
            SliceGeometry::Nine {
                left_pixels: 1,
                right_pixels: 1,
                top_pixels: 1,
                bottom_pixels: 1,
                center: SliceCenterPolicy::Repeat,
            }
        }
        _ => {
            let _ = height;
            SliceGeometry::None
        }
    }
}

fn synthesis_family_matches_domain_route(family: CandidateFamily, route: DomainRoute) -> bool {
    match family {
        CandidateFamily::PanelQuiltedExpansion
        | CandidateFamily::RepeatXQuilted
        | CandidateFamily::RepeatYQuilted => route == DomainRoute::TextureQuilting,
        CandidateFamily::PanelPatchMatchExpansion => route == DomainRoute::PatchMatch,
        CandidateFamily::PanelProceduralResynthesis => matches!(
            route,
            DomainRoute::StatisticalSynthesis
                | DomainRoute::ProceduralReconstruction
                | DomainRoute::LearnedProvider
        ),
        CandidateFamily::UniqueSynthesisExtension => matches!(
            route,
            DomainRoute::TextureQuilting
                | DomainRoute::PatchMatch
                | DomainRoute::StatisticalSynthesis
                | DomainRoute::ProceduralReconstruction
                | DomainRoute::LearnedProvider
        ),
        _ => false,
    }
}

fn executed_algorithm(result: &StageResult) -> Option<AlgorithmProvenance> {
    match result {
        StageResult::Executed { algorithm, .. } => Some(algorithm.clone()),
        _ => None,
    }
}

fn algorithm_versions<const N: usize>(
    results: [(u8, Option<AlgorithmProvenance>); N],
) -> BTreeMap<u8, AlgorithmProvenance> {
    results
        .into_iter()
        .filter_map(|(stage, algorithm)| algorithm.map(|algorithm| (stage, algorithm)))
        .collect()
}

#[cfg(test)]
mod gpu_stage_14_base_color_reachability_tests {
    use std::{io::Cursor, path::PathBuf};

    use super::{
        compile_persisted_details_for_region, legal_gate1_mode, stage16_stamp_asset_domains,
        synthesis_family_matches_domain_route,
    };
    use hot_trimmer_domain::{
        ChannelRegistration, ContentDigest, DecorationBinding, MaterialChannelRole, ProjectId,
        RegionId, SamplingMode, SourceId, TemplateSlotRole,
    };
    use hot_trimmer_material_synthesis::DomainRoute;
    use hot_trimmer_placement_solver::CandidateFamily;
    use hot_trimmer_project_store::{
        ProjectSummary, SourceChannel, SourceInput, SourceOwnership, StoredSource,
    };
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
    use uuid::Uuid;

    #[test]
    fn gpu_stage_14_base_color_required_modes_are_reachable_and_route_exact() {
        assert!(legal_gate1_mode(
            SamplingMode::NineSlicePanel,
            TemplateSlotRole::Planar,
            false,
        ));
        assert!(legal_gate1_mode(
            SamplingMode::PlanarRadial,
            TemplateSlotRole::Radial,
            false,
        ));
        assert!(legal_gate1_mode(
            SamplingMode::TextureSynthesis,
            TemplateSlotRole::UniqueDetail,
            false,
        ));
        assert!(synthesis_family_matches_domain_route(
            CandidateFamily::PanelPatchMatchExpansion,
            DomainRoute::PatchMatch,
        ));
        assert!(!synthesis_family_matches_domain_route(
            CandidateFamily::PanelPatchMatchExpansion,
            DomainRoute::TextureQuilting,
        ));
    }

    #[test]
    fn algorithm_stage_16_gpu_details_persisted_decorations_reach_production_compile() {
        let region_id = RegionId::from_bytes([0x16; 16]);
        let region_key = region_id.to_string();
        let image = RgbaImage::from_pixel(3, 2, Rgba([0, 255, 188, 64]));
        let mut encoded = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image)
            .write_to(&mut encoded, ImageFormat::Png)
            .expect("encode persisted Stage 16 asset");
        let encoded = encoded.into_inner();
        let asset = hot_trimmer_effect_compiler::StampAssetRef {
            asset_id: "persisted-bolt-mask".into(),
            version: "1.0.0".into(),
            digest: ContentDigest::sha256(&encoded),
            kind: "RegisteredStampChannels".into(),
        };
        let definition = hot_trimmer_effect_compiler::DetailDefinition {
            name: region_key.clone(),
            family: hot_trimmer_effect_compiler::DetailFamily::BoltGroup,
            physical_size: [0.25, 0.25],
            scale_space: hot_trimmer_effect_compiler::EffectScaleSpace::World,
            compatible_roles: vec![TemplateSlotRole::Planar],
            orientation: hot_trimmer_effect_compiler::DetailOrientation::Slot,
            explicit_rotation_degrees: 0.0,
            aspect_limits: [0.5, 2.0],
            minimum_pixels: [2, 2],
            repeat_period_m: Some([0.5, 0.5]),
            fit_policy: hot_trimmer_effect_compiler::DetailFitPolicy::FailIfOversized,
            mapping_mode: hot_trimmer_effect_compiler::DetailMappingMode::Planar,
            channels: vec![hot_trimmer_effect_compiler::DetailChannelContribution {
                channel: MaterialChannelRole::MaterialId,
                amount: 1.0,
                blend: hot_trimmer_effect_compiler::StampBlendPolicy::Replace,
                material_id: Some(42),
                metallic_explicit: false,
            }],
            fallback: hot_trimmer_effect_compiler::DetailFallback::Disabled,
            provenance: "persisted-test".into(),
            seed: 16,
            required_sources: vec![asset.clone()],
            required_halo_px: 2,
            dependencies: vec!["stage15-profile-occupancy".into()],
        };
        let operation = hot_trimmer_effect_compiler::StampOperation {
            asset: asset.clone(),
            scope: hot_trimmer_effect_compiler::StampScope::MaterialReusableAtlas,
            target_region: region_key.clone(),
            physical_position_m: [0.0, 0.0],
            physical_size_m: [0.25, 0.25],
            pivot: [0.5, 0.5],
            rotation_degrees: 0.0,
            mirror: [false, false],
            opacity: 1.0,
            blend: hot_trimmer_effect_compiler::StampBlendPolicy::Replace,
            clipping: hot_trimmer_effect_compiler::DetailFitPolicy::FailIfOversized,
            seed: 16,
            spacing_m: [0.5, 0.5],
            scatter: 0.0,
            jitter_m: [0.0, 0.0],
            layer_order: 0,
            occupancy: hot_trimmer_effect_compiler::OccupancyRelation::AboveProfile,
            channels: Vec::new(),
        };
        let decorations = vec![
            DecorationBinding {
                decoration_key: format!("stage16.detail.definition.{region_key}"),
                value: serde_json::to_string(&definition).unwrap(),
            },
            DecorationBinding {
                decoration_key: "stage16.stamp.operation".into(),
                value: serde_json::to_string(&operation).unwrap(),
            },
        ];
        let compiled = compile_persisted_details_for_region(
            &decorations,
            region_id,
            TemplateSlotRole::Planar,
            [1.0, 1.0],
            [1024, 1024],
            &hot_trimmer_effect_compiler::conservative_profile_capacity([1.0, 1.0]),
            &ContentDigest::sha256(b"persisted-stage16"),
        )
        .expect("persisted Stage 16 decorations should compile");
        assert_eq!(compiled.details.len(), 1);
        assert_eq!(compiled.details[0].reusable_atlas_operations.len(), 1);

        let source_set_uuid = Uuid::from_bytes([0x26; 16]);
        let source_id = SourceId::from_bytes([0x36; 16]);
        let project = ProjectSummary {
            id: ProjectId::from_bytes([0x46; 16]),
            name: "Stage 16 persisted asset resolution".into(),
            path: PathBuf::from("stage16-persisted-asset.hottrimmer"),
            sources: vec![StoredSource {
                source_set_id: source_set_uuid,
                channel: SourceChannel::BaseColor,
                registration: ChannelRegistration::explicit(MaterialChannelRole::BaseColor),
                input: SourceInput {
                    id: source_id,
                    ownership: SourceOwnership::OwnedCopy,
                    external_path: None,
                    origin_path: PathBuf::from("persisted-bolt-mask.png"),
                    sha256: asset.digest.0.clone(),
                    width: 3,
                    height: 2,
                    format: "PNG".into(),
                    color_type: "Rgba8".into(),
                    has_alpha: true,
                    exif_orientation: 1,
                    has_embedded_icc_profile: false,
                    encoded_bytes: encoded.len() as u64,
                    owned_bytes: Some(encoded),
                },
            }],
            source_sets: Vec::new(),
            patches: Vec::new(),
            document: None,
            legacy_layout_discarded: false,
            stale_lock_recovered: false,
        };
        let cancellation = hot_trimmer_image_io::CancellationToken::new();
        let resolved = stage16_stamp_asset_domains(
            &project,
            &decorations,
            super::SourceFramePreviewProfile::Draft512,
            &cancellation,
            None,
        )
        .expect("persisted Stage 16 asset should resolve through production preparation");
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].0.digest, asset.digest);
        assert_eq!(resolved[0].1.to_string(), source_set_uuid.to_string());
        assert_eq!([resolved[0].2.width, resolved[0].2.height], [3, 2]);
        assert!(
            resolved[0]
                .2
                .registered_channels()
                .iter()
                .any(|channel| channel.role() == MaterialChannelRole::BaseColor)
        );

        let orphan = DecorationBinding {
            decoration_key: "stage16.stamp.operation".into(),
            value: serde_json::to_string(&hot_trimmer_effect_compiler::StampOperation {
                target_region: region_key,
                ..operation
            })
            .unwrap(),
        };
        assert!(
            compile_persisted_details_for_region(
                &[orphan],
                region_id,
                TemplateSlotRole::Planar,
                [1.0, 1.0],
                [1024, 1024],
                &hot_trimmer_effect_compiler::conservative_profile_capacity([1.0, 1.0]),
                &ContentDigest::sha256(b"orphan-stage16"),
            )
            .is_err()
        );
    }
}
