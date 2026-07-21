use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::{self, File, OpenOptions},
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use hot_trimmer_domain::{
    ALGORITHM_STACK_CONTRACT_VERSION, AlgorithmProvenance, AssignmentProvenance,
    AuthoredLayoutPreset, CancellationToken as EngineCancellationToken, Channel, ChannelBitDepth,
    ChannelRegistration, CompilerRequestHeader, ContentDigest, ContentReference, DelightingIntent,
    ErrorCode, FoundationStatusRequest, IPC_PROTOCOL_VERSION, MaterialBehaviorClass,
    MaterialCalibrationCommand, MaterialCalibrationIntent, MaterialChannelRole,
    MaterialClassificationCommand, MaterialClassificationIntent, MaterialMapKind, NormalConvention,
    OrientedPixelSize, OriginalAssetProvenance, OutputSpecHeader, PartitionRecipe, PatchCommand,
    PatchGeometry, PatchId, PixelBounds, Projection, RegionId, RegisteredChannel,
    RegisteredChannelSet, RevisionAuthority, SourceId, SourceOwnershipIntent, SourceSetId,
    DecorationBinding, StructuralProfile, TemplateSlotRole, TrimSheetDocument,
    TrimSheetDocumentCommand, UserFacingError,
};
use hot_trimmer_export::{
    ExportMemoryBudgets, ExportProgress, FitAxis, HOTTRIM_MANIFEST_FILE_NAME, HottrimManifest,
    HottrimSlot, ManifestExportInput, MapRecord, NormalizedRect, TiledExportError, UvFit,
    UvFitKind, choose_bounded_tile_edge, manifest_from_template, write_package_manifest,
};
use hot_trimmer_image_io::{
    CancellationToken, ColorPolicy, DecodeLimits, InspectedImage, NormalizationSettings,
    PreparedChannelSet, inspect_bytes_with_policy, inspect_path_cancellable,
    prepare_registered_channel_set,
};
use hot_trimmer_material_analysis::{
    AnalysisSettings, ScaleOrientationCache, ScaleOrientationSettings, SourceAnalysisCache,
    analyze_source, calibrate_scale_orientation, prepare_delit_exemplar,
    scale_orientation_cache_key, source_analysis_cache_key,
};
use hot_trimmer_project_store::{
    ProjectStore, ProjectSummary, SourceChannel, SourceInput, SourceOwnership, StoreError,
    StoredSource,
};
use hot_trimmer_render_core::{
    ExemplarMaskIntent, PlanarArea, PreparedExemplar, PreparedExemplarCache,
    PreparedExemplarRequest, PreparedExemplarScope, RectificationQuality, RectificationWorkLimits,
    RenderCancellationToken, prepare_registered_exemplar,
};
use hot_trimmer_sheet_compiler::{
    AlgorithmCompiler, CompiledAtlasTileManifest, CompiledMapSet, GpuAtlasTileCache,
    OutputPixelRect, PreviewMapKind, ResolvedRegion,
};
use image::{
    ColorType, ImageEncoder,
    codecs::png::{CompressionType, FilterType, PngEncoder},
};
use png::{
    BitDepth as PngBitDepth, ColorType as PngColorType, Compression as PngCompression,
    Encoder as PngStreamEncoder, Filter as PngFilter,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as ShaDigest, Sha256};
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::paths::AppPaths;

const MAX_RECENT_PROJECTS: usize = 10;
const USER_LAYOUT_PRESET_LIBRARY_FILE: &str = "authored-layout-presets.v1.json";

#[derive(Clone, Copy, Debug)]
pub struct StartupState {
    pub previous_shutdown_clean: bool,
}

pub struct ProjectSession {
    store: Option<ProjectStore>,
    dirty: bool,
    is_draft: bool,
    baseline: Option<PathBuf>,
    app_data_dir: PathBuf,
    recovery_dir: PathBuf,
    draft_dir: PathBuf,
    source_projection_cache: Mutex<HashMap<(String, String, String), SourceProjection>>,
    preview_prepared_sources: HashMap<(String, u64), Arc<PreparedChannelSet>>,
    prepared_exemplars: PreparedExemplarCache,
    source_analysis_cache: SourceAnalysisCache,
    scale_orientation_cache: ScaleOrientationCache,
}

impl ProjectSession {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            store: None,
            dirty: false,
            is_draft: false,
            baseline: None,
            app_data_dir: paths.app_data.clone(),
            recovery_dir: paths.recovery.clone(),
            draft_dir: paths.drafts.clone(),
            source_projection_cache: Mutex::new(HashMap::new()),
            preview_prepared_sources: HashMap::new(),
            prepared_exemplars: PreparedExemplarCache::default(),
            source_analysis_cache: SourceAnalysisCache::default(),
            scale_orientation_cache: ScaleOrientationCache::default(),
        }
    }

    fn adopt(&mut self, store: ProjectStore, is_draft: bool) -> Result<(), UserFacingError> {
        if self.store.is_some() && self.dirty {
            return Err(error(
                ErrorCode::DirtyProject,
                "Save or close the current project first.",
            ));
        }
        let baseline = self
            .recovery_dir
            .join(format!("baseline-{}.hottrimmer", Uuid::new_v4()));
        store.backup_atomic(&baseline).map_err(store_error)?;
        self.store = Some(store);
        self.source_projection_cache
            .lock()
            .map_err(|_| poisoned())?
            .clear();
        self.preview_prepared_sources.clear();
        self.prepared_exemplars = PreparedExemplarCache::default();
        self.source_analysis_cache = SourceAnalysisCache::default();
        self.scale_orientation_cache = ScaleOrientationCache::default();
        self.baseline = Some(baseline);
        self.dirty = false;
        self.is_draft = is_draft;
        Ok(())
    }

    fn mark_mutated(&mut self) {
        self.dirty = true;
        if let Some(store) = &self.store {
            let _ = store.create_recovery_snapshot(&self.recovery_dir);
        }
    }

    fn source_projection_cached(
        &self,
        source: &StoredSource,
    ) -> Result<SourceProjection, UserFacingError> {
        let key = (
            source.input.id.to_string(),
            source.input.sha256.clone(),
            serde_json::to_string(&source.registration)
                .map_err(|failure| error(ErrorCode::Internal, &failure.to_string()))?,
        );
        if let Some(projection) = self
            .source_projection_cache
            .lock()
            .map_err(|_| poisoned())?
            .get(&key)
            .cloned()
        {
            return Ok(projection);
        }
        let projection = source_projection(source)?;
        self.source_projection_cache
            .lock()
            .map_err(|_| poisoned())?
            .insert(key, projection.clone());
        Ok(projection)
    }
}

pub type SharedProjectSession = Arc<Mutex<ProjectSession>>;
pub type PendingProjectPath = Arc<Mutex<Option<String>>>;
pub type SharedImportJob = Arc<Mutex<Option<CancellationToken>>>;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportProgress {
    stage: &'static str,
    fraction: f32,
}
pub type SharedPreviewService = Arc<PreviewService>;

#[derive(Default)]
pub struct PreviewService {
    latest_draft_id: AtomicU64,
    cancellation_count: AtomicU64,
    source_frame_cache: Mutex<hot_trimmer_sheet_compiler::SourceFramePreviewCache>,
    gpu_source_cache: Mutex<hot_trimmer_sheet_compiler::GpuAtlasSourceTextureCache>,
    gpu_tile_cache: Mutex<GpuAtlasTileCache>,
    previewed_candidate_recipes: Mutex<BTreeSet<(u64, hot_trimmer_domain::DocumentHash)>>,
    gpu_capabilities: hot_trimmer_preview::GpuCapabilityService,
}

impl PreviewService {
    fn reset(&self) {
        self.latest_draft_id.fetch_add(1, Ordering::AcqRel);
        self.clear_cached_outputs();
    }

    fn clear_cached_outputs(&self) {
        if let Ok(mut cache) = self.source_frame_cache.lock() {
            *cache = hot_trimmer_sheet_compiler::SourceFramePreviewCache::default();
        }
        if let Ok(mut cache) = self.gpu_source_cache.lock() {
            *cache = hot_trimmer_sheet_compiler::GpuAtlasSourceTextureCache::default();
        }
        if let Ok(mut cache) = self.gpu_tile_cache.lock() {
            *cache = GpuAtlasTileCache::default();
        }
        if let Ok(mut candidates) = self.previewed_candidate_recipes.lock() {
            candidates.clear();
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundationStatus {
    protocol_version: u16,
    app_version: &'static str,
    platform: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupStatus {
    previous_shutdown_clean: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentProject {
    name: String,
    path: String,
    last_opened_unix: i64,
    available: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceProjection {
    id: String,
    channel: SourceChannel,
    display_name: String,
    original: OriginalSourceProjection,
    storage: SourceStorageProjection,
    oriented_size: OrientedSizeProjection,
    orientation: u16,
    interpretation: hot_trimmer_domain::ChannelInterpretation,
    normal_convention: NormalConvention,
    assignment_provenance: AssignmentProvenance,
    confidence_milli: u16,
    thumbnail_data_url: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OriginalSourceProjection {
    path: String,
    immutable_digest: String,
    encoded_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceStorageProjection {
    ownership: SourceOwnership,
    external_path: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OrientedSizeProjection {
    width: u32,
    height: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RegisteredChannelSetProjection {
    oriented_size: OrientedSizeProjection,
    orientation: u16,
    channels: Vec<SourceProjection>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MaterialSourceProjection {
    id: String,
    name: String,
    exemplar_group: Option<String>,
    source_revision: u64,
    registration_digest: String,
    delighting: DelightingIntent,
    classification: MaterialClassificationIntent,
    calibration: MaterialCalibrationIntent,
    registered_channels: Option<RegisteredChannelSetProjection>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectProjection {
    id: String,
    name: String,
    path: String,
    schema_version: u32,
    dirty: bool,
    is_draft: bool,
    material_sources: Vec<MaterialSourceProjection>,
    patches: Vec<hot_trimmer_domain::Patch>,
    document: Option<TrimSheetDocument>,
    legacy_layout_discarded: bool,
    can_undo_document: bool,
    can_redo_document: bool,
    can_undo_patch: bool,
    can_redo_patch: bool,
    feedback_authoring: FeedbackAuthoringProjection,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackAuthoringProjection {
    command_version: u16,
    records: Vec<FeedbackDetailRecordProjection>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackDetailRecordProjection {
    operation_id: String,
    enabled: bool,
    intent: FeedbackDetailIntent,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPathRequest {
    protocol_version: u16,
    path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectRequest {
    protocol_version: u16,
    path: String,
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSourceRequest {
    protocol_version: u16,
    path: String,
    ownership: SourceOwnership,
    channel: SourceChannel,
    source_set_id: Uuid,
    assignment_provenance: AssignmentProvenance,
    confidence_milli: u16,
    normal_convention: NormalConvention,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSlotRequest {
    protocol_version: u16,
    channel: SourceChannel,
    source_set_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSetRequest {
    protocol_version: u16,
    source_set_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetExemplarGroupRequest {
    protocol_version: u16,
    material_source_id: Uuid,
    exemplar_group: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetDelightingIntentRequest {
    protocol_version: u16,
    material_source_id: Uuid,
    delighting: DelightingIntent,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialClassificationCommandRequest {
    protocol_version: u16,
    material_source_id: Uuid,
    classification_command: MaterialClassificationCommand,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialCalibrationCommandRequest {
    protocol_version: u16,
    material_source_id: Uuid,
    calibration_command: MaterialCalibrationCommand,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectNameRequest {
    protocol_version: u16,
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSetNameRequest {
    protocol_version: u16,
    source_set_id: Uuid,
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAuthoredLayoutPresetRequest {
    protocol_version: u16,
    preset: AuthoredLayoutPreset,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteAuthoredLayoutPresetRequest {
    protocol_version: u16,
    preset_id: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthoredLayoutPresetLibrary {
    schema_version: u16,
    presets: Vec<AuthoredLayoutPreset>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDocumentRequest {
    protocol_version: u16,
    template_id: String,
    template_version: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceFramePartitionRequest {
    protocol_version: u16,
    target_region_count: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentCommandRequest {
    protocol_version: u16,
    command: TrimSheetDocumentCommand,
}

pub const FEEDBACK_COMMAND_VERSION: u16 = 1;
pub const STAGE_15_20_DEBUG_SCHEMA_VERSION: u16 = 1;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackWorkbenchCommandRequest {
    protocol_version: u16,
    command_version: u16,
    command: FeedbackWorkbenchCommand,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum FeedbackWorkbenchCommand {
    SetProfile {
        region_id: RegionId,
        requested: hot_trimmer_effect_compiler::RequestedProfile,
    },
    SetEdgeWear {
        intent: hot_trimmer_domain::EdgeWearIntent,
    },
    UpsertDetail {
        operation_id: Option<String>,
        enabled: bool,
        intent: FeedbackDetailIntent,
    },
    DuplicateDetail { operation_id: String },
    SetDetailEnabled { operation_id: String, enabled: bool },
    DeleteDetail { operation_id: String },
    ReorderDetails { operation_ids: Vec<String> },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum FeedbackDetailIntent {
    Definition(hot_trimmer_effect_compiler::DetailDefinition),
    Operation(hot_trimmer_effect_compiler::StampOperation),
    Stroke(hot_trimmer_effect_compiler::StampStroke),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackWorkbenchCommandResult {
    command_version: u16,
    committed_identity: String,
    project: ProjectProjection,
    status: &'static str,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum FeedbackExecutionState {
    InstalledNotRequested,
    Requested,
    Executed,
    CacheHit,
    SkippedBecauseUnused,
    DeferredOnly,
    Failed,
    Cancelled,
    Superseded,
    NotInstalled,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stage15To20DebugRequest {
    protocol_version: u16,
    schema_version: u16,
    selected_region_id: Option<String>,
    requested_view: String,
    preview_profile: String,
    comparison_mode: String,
    selected_operation_id: Option<String>,
    active_tool: String,
    preview_state: FeedbackExecutionState,
    request_identity: String,
    pixel_dispatch_count: u32,
    execution_outcome: FeedbackExecutionState,
    preview_error: Option<UserFacingError>,
    last_command_result: Option<String>,
    paint_summary: Option<serde_json::Value>,
    tile: Option<serde_json::Value>,
    compiled_inspection: Option<serde_json::Value>,
    workbench_state: Option<serde_json::Value>,
    #[serde(default)]
    bounded_telemetry: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stage15To20DebugPayload {
    schema: &'static str,
    schema_version: u16,
    summary: String,
    payload: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchCommandRequest {
    protocol_version: u16,
    command: PatchCommand,
    coalescing_group: Option<u64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedPatchPreviewRequest {
    protocol_version: u16,
    patch_id: PatchId,
    max_edge: u32,
    #[serde(default)]
    geometry: Option<PatchGeometry>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedPatchPreviewProjection {
    patch_id: PatchId,
    material_source_id: String,
    width: u32,
    height: u32,
    data_url: String,
    perspective_confidence_milli: u16,
    delighting_route: String,
    delighting_strength_milli: u16,
    source_analysis: SourceInspectorProjection,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInspectorProjection {
    quality_summary: String,
    analyzed_class: MaterialBehaviorClass,
    routed_class: MaterialBehaviorClass,
    confidence_percent: u8,
    evidence_summary: String,
    warning_count: u8,
    scale_summary: String,
    orientation_summary: String,
    world_scale_available: bool,
    orientation_overlay: Vec<OrientationOverlayProjection>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrientationOverlayProjection {
    source_x_milli: u64,
    source_y_milli: u64,
    axis_millidegrees: Option<u32>,
    confidence_milli: u16,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileDocumentRequest {
    protocol_version: u16,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewDocumentRequest {
    protocol_version: u16,
    draft_id: u64,
    map_view: PreviewMapKind,
    region_id: Option<RegionId>,
    projection: Option<Projection>,
    max_edge: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseProjectRequest {
    protocol_version: u16,
    save: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledMapsProjection {
    base_color: String,
    normal: String,
    height: String,
    roughness: String,
    metallic: String,
    ambient_occlusion: String,
    region_id: String,
    material_id: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledSheetProjection {
    document_revision: u64,
    topology_hash: String,
    appearance_hash: String,
    renderer_version: String,
    width: u32,
    height: u32,
    maps: CompiledMapsProjection,
    regions: Vec<ResolvedRegion>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSheetProjection {
    draft_id: u64,
    document_revision: u64,
    topology_hash: String,
    appearance_hash: String,
    width: u32,
    height: u32,
    map_view: PreviewMapKind,
    data_url: String,
    regions: Vec<ResolvedRegion>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuTiledPreviewPayloadRequest {
    protocol_version: u16,
    generation: u64,
    opaque_handle: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseGpuTiledPreviewPayloadRequest {
    protocol_version: u16,
    generation: u64,
    opaque_handle: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuTiledPreviewTelemetry {
    generation: u64,
    native_publish_ms: u128,
    raw_ipc_bytes: u64,
    raw_ipc_ms: u128,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuTiledPreviewPublication {
    manifest: CompiledAtlasTileManifest,
    telemetry: GpuTiledPreviewTelemetry,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stage14PreviewRequest {
    protocol_version: u16,
    revision: u64,
    #[serde(default)]
    region_id: Option<RegionId>,
    #[serde(default)]
    transient_projection: Option<Projection>,
    #[serde(default)]
    draft_id: Option<u64>,
    #[serde(default)]
    input_hash: Option<String>,
    #[serde(default)]
    profile: PreviewProfile,
    #[serde(default)]
    view_intent: Option<PreviewViewIntent>,
    #[serde(default)]
    viewport_rect: Option<PixelBounds>,
    #[serde(default)]
    requested_maps: Vec<MaterialMapKind>,
    /// A transient candidate is compiled by the same persisted spine against a cloned summary.
    /// It is never stored until the matching AcceptSourceFramePartition command is issued.
    #[serde(default)]
    candidate_recipe: Option<PartitionRecipe>,
    #[serde(default)]
    feedback_view: Option<FeedbackContributionView>,
    #[serde(default)]
    feedback_comparison_mode: Option<FeedbackComparisonMode>,
    #[serde(default)]
    feedback_selected_operation_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FeedbackContributionView {
    Stage15Occupancy,
    Stage15Height,
    Stage15ProfileRoute,
    Stage15Lod,
    Stage15Fallback,
    Stage16RegisteredMask,
    Stage16Height,
    Stage16VectorNormal,
    Stage16ScalarRoughness,
    Stage16ScalarMetallic,
    Stage16ScalarAmbientOcclusion,
    Stage16BaseColor,
    Stage16MaterialId,
    Stage16MaterialIdValidity,
    Stage16Route,
    Stage16Occupancy,
    Stage16Lod,
    Stage16Scope,
    Stage16AssetResolution,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FeedbackComparisonMode {
    After,
    Before,
    SelectedOperationIsolation,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackQaTileRequest {
    protocol_version: u16,
    command_version: u16,
    revision: u64,
    generation: u64,
    region_id: RegionId,
    #[serde(default)]
    all_regions: bool,
    view: FeedbackContributionView,
    profile: PreviewProfile,
    comparison_mode: FeedbackComparisonMode,
    selected_operation_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackPreviewExecution {
    request_identity: String,
    client_generation: u64,
    published_generation: u64,
    revision: u64,
    region_id: RegionId,
    all_regions: bool,
    view: FeedbackContributionView,
    requested_map: &'static str,
    profile: PreviewProfile,
    comparison_mode: FeedbackComparisonMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    selected_operation_id: Option<String>,
    outcome: FeedbackExecutionState,
    cache_reused: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeStage14ExportRequest {
    protocol_version: u16,
    revision: u64,
    path: String,
    #[serde(default)]
    requested_maps: Vec<MaterialMapKind>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeStage14ExportOutput {
    id: String,
    map: String,
    file_name: String,
    checksum: String,
    bytes: u64,
    width: u32,
    height: u32,
    pixel_format: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeStage14ExportProjection {
    path: String,
    revision: u64,
    bytes_written: u64,
    outputs: Vec<NativeStage14ExportOutput>,
    progress: Vec<ExportProgress>,
    telemetry: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    project: Option<ProjectProjection>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeStage14ExportProgressEvent {
    revision: u64,
    progress: ExportProgress,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PreviewViewIntent {
    CompleteDraft512,
    CompleteRefinement1024,
    ExactViewport,
    ExactSelectedRegion,
}

fn compiled_view_intent(
    request: &Stage14PreviewRequest,
) -> Result<Option<hot_trimmer_sheet_compiler::SourceFramePreviewViewIntent>, UserFacingError> {
    let rect = |value: Option<PixelBounds>, label: &str| {
        value.map(OutputPixelRect).ok_or_else(|| {
            error(
                ErrorCode::InvalidInput,
                &format!("{label} requires an exact output rectangle."),
            )
        })
    };
    let maps = request.requested_maps.clone();
    let has_maps = !maps.is_empty();
    match request.view_intent {
        None if has_maps => Ok(Some(
            hot_trimmer_sheet_compiler::SourceFramePreviewViewIntent::MaterialMaps(maps),
        )),
        None => Ok(None),
        Some(PreviewViewIntent::CompleteDraft512) if has_maps => Ok(Some(
            hot_trimmer_sheet_compiler::SourceFramePreviewViewIntent::MaterialMaps(maps),
        )),
        Some(PreviewViewIntent::CompleteDraft512) => Ok(Some(
            hot_trimmer_sheet_compiler::SourceFramePreviewViewIntent::CompleteDraft512,
        )),
        Some(PreviewViewIntent::CompleteRefinement1024) if has_maps => Ok(Some(
            hot_trimmer_sheet_compiler::SourceFramePreviewViewIntent::MaterialMaps(maps),
        )),
        Some(PreviewViewIntent::CompleteRefinement1024) => Ok(Some(
            hot_trimmer_sheet_compiler::SourceFramePreviewViewIntent::CompleteRefinement1024,
        )),
        Some(PreviewViewIntent::ExactViewport) if has_maps => Ok(Some(
            hot_trimmer_sheet_compiler::SourceFramePreviewViewIntent::ExactViewportMaterialMaps {
                rect: rect(request.viewport_rect, "Exact viewport")?,
                maps,
            },
        )),
        Some(PreviewViewIntent::ExactViewport) => Ok(Some(
            hot_trimmer_sheet_compiler::SourceFramePreviewViewIntent::ExactViewport(rect(
                request.viewport_rect,
                "Exact viewport",
            )?),
        )),
        Some(PreviewViewIntent::ExactSelectedRegion) => {
            let Some(region_id) = request.region_id else {
                return Err(error(
                    ErrorCode::InvalidInput,
                    "Exact selected-region preview requires a selected region.",
                ));
            };
            if has_maps {
                Ok(Some(hot_trimmer_sheet_compiler::SourceFramePreviewViewIntent::ExactSelectedRegionMaterialMaps {
                    region_id,
                    maps,
                }))
            } else {
                Ok(Some(
                    hot_trimmer_sheet_compiler::SourceFramePreviewViewIntent::ExactSelectedRegion(
                        region_id,
                    ),
                ))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PreviewProfile {
    Draft512,
    Refinement1024,
    Preview2048,
    Preview4096,
    Preview8192,
    #[default]
    Authoritative,
}

impl From<PreviewProfile> for hot_trimmer_sheet_compiler::SourceFramePreviewProfile {
    fn from(value: PreviewProfile) -> Self {
        match value {
            PreviewProfile::Draft512 => Self::Draft512,
            PreviewProfile::Refinement1024 => Self::Refinement1024,
            PreviewProfile::Preview2048 => Self::Preview2048,
            PreviewProfile::Preview4096 => Self::Preview4096,
            PreviewProfile::Preview8192 => Self::Preview8192,
            PreviewProfile::Authoritative => Self::Authoritative,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stage14SlotProjection {
    region_id: RegionId,
    slot_key: String,
    display_name: String,
    allocation_bounds: hot_trimmer_domain::CanonicalRect,
    hotspot_bounds: hot_trimmer_domain::CanonicalRect,
    semantic_rect: hot_trimmer_domain::CanonicalRect,
    padded_rect: hot_trimmer_domain::CanonicalRect,
    atlas_destination: hot_trimmer_domain::CanonicalRect,
    preview_padding_px: u32,
    mapping_mode: String,
    source_transform: hot_trimmer_placement_solver::CandidateTransform,
    isotropic_scale: f64,
    sampling_scale: f64,
    validity: String,
    correspondence: String,
    source_id: String,
    patch_id: Option<String>,
    domain_id: String,
    candidate_id: String,
    sampling_plan_id: String,
    stage_14_result_id: String,
    source_crop: Option<hot_trimmer_placement_solver::SourceCrop>,
    source_bounds: Option<hot_trimmer_domain::NormalizedBounds>,
    mapping_origin: Option<hot_trimmer_domain::MappingOrigin>,
    grid_rect: Option<hot_trimmer_domain::GridRect>,
    behavior_version: u16,
    role: hot_trimmer_domain::ManualRegionRole,
    continuity: hot_trimmer_domain::RegionContinuity,
    requested_sampling: hot_trimmer_domain::RegionSampling,
    executed_mode: String,
    edge_eligibility: hot_trimmer_domain::EdgeEligibility,
    period_pixels: Option<[u32; 2]>,
    address_mode: String,
    compiled_profile: Option<hot_trimmer_effect_compiler::CompiledProfile>,
    compiled_details: Option<hot_trimmer_effect_compiler::CompiledDetailSet>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntermediateAtlasProjection {
    label: &'static str,
    non_exportable: bool,
    incomplete_after_stage: u8,
    revision: u64,
    document_revision: u64,
    topology_hash: String,
    appearance_hash: String,
    renderer_version: &'static str,
    width: u32,
    height: u32,
    topology: hot_trimmer_domain::CompiledTemplateTopology,
    placement_plan_id: String,
    maps: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tile_manifest: Option<GpuTiledPreviewPublication>,
    tile_manifests: BTreeMap<String, GpuTiledPreviewPublication>,
    #[serde(skip_serializing_if = "Option::is_none")]
    feedback_execution: Option<FeedbackPreviewExecution>,
    region_id_lookup: Vec<hot_trimmer_sheet_compiler::CompiledCompactRegionIdLookup>,
    regions: Vec<ResolvedRegion>,
    unavailable_channels: Vec<String>,
    slots: Vec<Stage14SlotProjection>,
    pending: Vec<&'static str>,
    telemetry: Vec<String>,
    final_compile_available: bool,
    export_available: bool,
    blender_available: bool,
    source_frame: Option<hot_trimmer_domain::SourceFrame>,
    #[serde(skip_serializing_if = "Option::is_none")]
    project: Option<ProjectProjection>,
}

#[tauri::command]
pub fn foundation_status(
    request: FoundationStatusRequest,
) -> Result<FoundationStatus, UserFacingError> {
    request.validate().map_err(UserFacingError::from)?;
    Ok(FoundationStatus {
        protocol_version: IPC_PROTOCOL_VERSION,
        app_version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
    })
}

#[tauri::command]
pub fn startup_status(state: State<'_, StartupState>) -> StartupStatus {
    StartupStatus {
        previous_shutdown_clean: state.previous_shutdown_clean,
    }
}

#[tauri::command]
pub fn create_project(
    request: CreateProjectRequest,
    session: State<'_, SharedProjectSession>,
    preview_service: State<'_, SharedPreviewService>,
) -> Result<ProjectProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    let store =
        ProjectStore::create(Path::new(&request.path), request.name.trim()).map_err(store_error)?;
    session.adopt(store, false)?;
    preview_service.reset();
    remember_open_project_best_effort(&session);
    project_projection(&session)
}

#[tauri::command]
pub fn create_draft_project(
    request: FoundationStatusRequest,
    session: State<'_, SharedProjectSession>,
    preview_service: State<'_, SharedPreviewService>,
) -> Result<ProjectProjection, UserFacingError> {
    request.validate().map_err(UserFacingError::from)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    fs::create_dir_all(&session.draft_dir).map_err(io_error)?;
    let path = session
        .draft_dir
        .join(format!("Untitled-{}.hottrimmer", Uuid::new_v4()));
    let store = ProjectStore::create(&path, "Untitled").map_err(store_error)?;
    session.adopt(store, true)?;
    preview_service.reset();
    project_projection(&session)
}

#[tauri::command]
pub fn open_project(
    request: ProjectPathRequest,
    session: State<'_, SharedProjectSession>,
    preview_service: State<'_, SharedPreviewService>,
) -> Result<ProjectProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    let store = ProjectStore::open(Path::new(&request.path)).map_err(store_error)?;
    session.adopt(store, false)?;
    preview_service.reset();
    remember_open_project_best_effort(&session);
    project_projection(&session)
}

#[tauri::command]
pub fn import_source(
    request: ImportSourceRequest,
    session: State<'_, SharedProjectSession>,
    import_job: State<'_, SharedImportJob>,
    app: AppHandle,
) -> Result<ProjectProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let cancellation = CancellationToken::new();
    *import_job.lock().map_err(|_| poisoned())? = Some(cancellation.clone());
    let _ = app.emit(
        "import-progress",
        ImportProgress {
            stage: "Reading and decoding",
            fraction: 0.05,
        },
    );
    let path = PathBuf::from(&request.path);
    let inspected = inspect_path_cancellable(
        &path,
        DecodeLimits::default(),
        color_policy(request.channel),
        &cancellation,
    )
    .map_err(image_error)?;
    let _ = app.emit(
        "import-progress",
        ImportProgress {
            stage: "Validating registration",
            fraction: 0.82,
        },
    );
    if cancellation.is_cancelled() {
        return Err(image_error(hot_trimmer_image_io::ImageIoError::Cancelled));
    }
    let input = source_input(&path, request.ownership, &inspected);
    let mut session = session.lock().map_err(|_| poisoned())?;
    let store = session.store.as_mut().ok_or_else(no_project)?;
    store
        .replace_registered_source_in_set(
            request.source_set_id,
            &input,
            ChannelRegistration {
                role: request.channel.material_role(),
                interpretation: request.channel.material_role().required_interpretation(),
                normal_convention: request.normal_convention,
                assignment_provenance: request.assignment_provenance,
                confidence_milli: request.confidence_milli,
            },
        )
        .map_err(|failure| source_registration_error(failure, request.channel))?;
    store.refresh_document_assets().map_err(store_error)?;
    session
        .prepared_exemplars
        .invalidate_source(SourceSetId::from_bytes(*request.source_set_id.as_bytes()));
    session.mark_mutated();
    *import_job.lock().map_err(|_| poisoned())? = None;
    let _ = app.emit(
        "import-progress",
        ImportProgress {
            stage: "Complete",
            fraction: 1.0,
        },
    );
    project_projection(&session)
}

#[tauri::command]
pub fn cancel_import(
    request: FoundationStatusRequest,
    import_job: State<'_, SharedImportJob>,
) -> Result<(), UserFacingError> {
    request.validate().map_err(UserFacingError::from)?;
    if let Some(job) = import_job.lock().map_err(|_| poisoned())?.as_ref() {
        job.cancel();
    }
    Ok(())
}

#[tauri::command]
pub fn remove_source(
    request: SourceSlotRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    session
        .store
        .as_mut()
        .ok_or_else(no_project)?
        .remove_source_in_set(request.source_set_id, request.channel)
        .map_err(store_error)?;
    session
        .prepared_exemplars
        .invalidate_source(SourceSetId::from_bytes(*request.source_set_id.as_bytes()));
    session.mark_mutated();
    project_projection(&session)
}

#[tauri::command]
pub fn remove_source_set(
    request: SourceSetRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    let summary = session
        .store
        .as_ref()
        .ok_or_else(no_project)?
        .summary()
        .map_err(store_error)?;
    if let Some(document) = summary.document.as_ref() {
        let id = SourceSetId::from_bytes(*request.source_set_id.as_bytes());
        let referenced = document.primary_material == Some(id)
            || document.source_frame.as_ref().is_some_and(|frame| frame.source_set_id == id)
            || document.region_bindings.values().any(|binding| matches!(&binding.content, ContentReference::MaterialSource(value) if *value == id));
        if referenced {
            return Err(error(
                ErrorCode::InvalidInput,
                "The source is referenced by the primary material, SourceFrame, or a region. Assign an explicit fallback first.",
            ));
        }
    }
    session
        .store
        .as_mut()
        .ok_or_else(no_project)?
        .remove_source_set(request.source_set_id)
        .map_err(store_error)?;
    session
        .prepared_exemplars
        .invalidate_source(SourceSetId::from_bytes(*request.source_set_id.as_bytes()));
    session.mark_mutated();
    project_projection(&session)
}

#[tauri::command]
pub fn set_exemplar_group(
    request: SetExemplarGroupRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    session
        .store
        .as_mut()
        .ok_or_else(no_project)?
        .set_exemplar_group(
            request.material_source_id,
            request.exemplar_group.as_deref(),
        )
        .map_err(store_error)?;
    session.mark_mutated();
    project_projection(&session)
}

#[tauri::command]
pub fn set_delighting_intent(
    request: SetDelightingIntentRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    session
        .store
        .as_mut()
        .ok_or_else(no_project)?
        .set_delighting_intent(request.material_source_id, &request.delighting)
        .map_err(store_error)?;
    session.source_analysis_cache = SourceAnalysisCache::default();
    session.mark_mutated();
    project_projection(&session)
}

#[tauri::command]
pub fn apply_material_classification_command(
    request: MaterialClassificationCommandRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    session
        .store
        .as_mut()
        .ok_or_else(no_project)?
        .apply_material_classification_command(
            request.material_source_id,
            request.classification_command,
        )
        .map_err(store_error)?;
    session.mark_mutated();
    project_projection(&session)
}

#[tauri::command]
pub fn apply_material_calibration_command(
    request: MaterialCalibrationCommandRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    session
        .store
        .as_mut()
        .ok_or_else(no_project)?
        .apply_material_calibration_command(request.material_source_id, request.calibration_command)
        .map_err(store_error)?;
    session.mark_mutated();
    project_projection(&session)
}

#[tauri::command]
pub fn rename_project(
    request: ProjectNameRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    session
        .store
        .as_mut()
        .ok_or_else(no_project)?
        .rename_project(&request.name)
        .map_err(store_error)?;
    session.mark_mutated();
    project_projection(&session)
}

#[tauri::command]
pub fn rename_source_set(
    request: SourceSetNameRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    session
        .store
        .as_mut()
        .ok_or_else(no_project)?
        .rename_source_set(request.source_set_id, &request.name)
        .map_err(store_error)?;
    session.mark_mutated();
    project_projection(&session)
}

#[tauri::command]
pub fn list_authored_layout_presets(
    request: FoundationStatusRequest,
    paths: State<'_, AppPaths>,
) -> Result<Vec<AuthoredLayoutPreset>, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    Ok(read_authored_layout_preset_library(&paths)?.presets)
}

#[tauri::command]
pub fn save_authored_layout_preset(
    request: SaveAuthoredLayoutPresetRequest,
    paths: State<'_, AppPaths>,
) -> Result<Vec<AuthoredLayoutPreset>, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    hot_trimmer_domain::validate_authored_layout_preset(&request.preset)
        .map_err(|reason| error(ErrorCode::InvalidInput, &reason.to_string()))?;
    if request.preset.preset_id.starts_with("builtin.")
        || request.preset.preset_id.trim().is_empty()
    {
        return Err(error(
            ErrorCode::InvalidInput,
            "Built-in layout presets are immutable.",
        ));
    }
    let mut library = read_authored_layout_preset_library(&paths)?;
    if let Some(existing) = library
        .presets
        .iter_mut()
        .find(|preset| preset.preset_id == request.preset.preset_id)
    {
        *existing = request.preset;
    } else {
        library.presets.push(request.preset);
    }
    library.presets.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then(left.preset_id.cmp(&right.preset_id))
    });
    write_authored_layout_preset_library(&paths, &library)?;
    Ok(library.presets)
}

#[tauri::command]
pub fn delete_authored_layout_preset(
    request: DeleteAuthoredLayoutPresetRequest,
    paths: State<'_, AppPaths>,
) -> Result<Vec<AuthoredLayoutPreset>, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    if request.preset_id.starts_with("builtin.") {
        return Err(error(
            ErrorCode::InvalidInput,
            "Built-in layout presets are immutable.",
        ));
    }
    let mut library = read_authored_layout_preset_library(&paths)?;
    let before = library.presets.len();
    library
        .presets
        .retain(|preset| preset.preset_id != request.preset_id);
    if library.presets.len() == before {
        return Err(error(
            ErrorCode::InvalidInput,
            "The saved layout preset no longer exists.",
        ));
    }
    write_authored_layout_preset_library(&paths, &library)?;
    Ok(library.presets)
}

#[tauri::command]
pub fn create_trim_sheet_document(
    request: CreateDocumentRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    session
        .store
        .as_mut()
        .ok_or_else(no_project)?
        .create_trim_sheet_document(&request.template_id, &request.template_version)
        .map_err(store_error)?;
    session.mark_mutated();
    project_projection(&session)
}

#[tauri::command]
pub fn create_source_frame_document(
    request: FoundationStatusRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectProjection, UserFacingError> {
    request.validate().map_err(UserFacingError::from)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    session
        .store
        .as_mut()
        .ok_or_else(no_project)?
        .create_source_frame_document()
        .map_err(store_error)?;
    session.mark_mutated();
    project_projection(&session)
}

#[tauri::command]
pub fn create_stage_15_16_feedback_sample(
    request: FoundationStatusRequest,
    session: State<'_, SharedProjectSession>,
    preview_service: State<'_, SharedPreviewService>,
) -> Result<ProjectProjection, UserFacingError> {
    request.validate().map_err(UserFacingError::from)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    create_stage_15_16_feedback_sample_impl(&mut session, preview_service.inner().as_ref())
}

fn create_stage_15_16_feedback_sample_impl(
    session: &mut ProjectSession,
    preview_service: &PreviewService,
) -> Result<ProjectProjection, UserFacingError> {
    let existing = session
        .store
        .as_ref()
        .ok_or_else(no_project)?
        .summary()
        .map_err(store_error)?;
    if existing.document.is_some() || !existing.sources.is_empty() {
        return Err(error(
            ErrorCode::InvalidInput,
            "Create the bundled feedback sample in a new empty draft.",
        ));
    }
    let mut rgba = Vec::with_capacity(64 * 64 * 4);
    for y in 0..64_u32 {
        for x in 0..64_u32 {
            let checker = ((x / 8) + (y / 8)) % 2;
            let border = x % 16 < 2 || y % 16 < 2;
            rgba.extend_from_slice(if border {
                &[62, 76, 86, 255]
            } else if checker == 0 {
                &[154, 126, 92, 255]
            } else {
                &[112, 91, 68, 255]
            });
        }
    }
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(&rgba, 64, 64, ColorType::Rgba8.into())
        .map_err(|failure| error(ErrorCode::Internal, &failure.to_string()))?;
    let inspected = inspect_bytes_with_policy(
        png,
        DecodeLimits::default(),
        ColorPolicy::ConvertToSrgb,
    )
    .map_err(image_error)?;
    let source_set_id = Uuid::parse_str("20152016-0000-4000-8000-000000000001")
        .expect("feedback sample source UUID");
    let mut input = source_input(
        Path::new("feedback-sample-stage15-16.png"),
        SourceOwnership::OwnedCopy,
        &inspected,
    );
    input.id = "20152016-0000-4000-8000-000000000010".parse().expect("feedback Base Color source UUID");
    let asset_digest = input.sha256.clone();
    let store = session.store.as_mut().ok_or_else(no_project)?;
    store
        .replace_registered_source_in_set(
            source_set_id,
            &input,
            ChannelRegistration {
                role: MaterialChannelRole::BaseColor,
                interpretation: MaterialChannelRole::BaseColor.required_interpretation(),
                normal_convention: NormalConvention::NotApplicable,
                assignment_provenance: AssignmentProvenance::UserAssigned,
                confidence_milli: 1000,
            },
        )
        .map_err(|failure| source_registration_error(failure, SourceChannel::BaseColor))?;
    for (role, channel, source_id) in [
        (MaterialChannelRole::Height, SourceChannel::Height, "20152016-0000-4000-8000-000000000011"),
        (MaterialChannelRole::MaterialId, SourceChannel::MaterialId, "20152016-0000-4000-8000-000000000012"),
        (MaterialChannelRole::EdgeMask, SourceChannel::EdgeMask, "20152016-0000-4000-8000-000000000013"),
    ] {
        let mut channel_input = input.clone();
        channel_input.id = source_id.parse().expect("feedback registered-channel source UUID");
        store
            .replace_registered_source_in_set(
                source_set_id,
                &channel_input,
                ChannelRegistration {
                    role,
                    interpretation: role.required_interpretation(),
                    normal_convention: NormalConvention::NotApplicable,
                    assignment_provenance: AssignmentProvenance::UserAssigned,
                    confidence_milli: 1000,
                },
            )
            .map_err(|failure| source_registration_error(failure, channel))?;
    }
    store.refresh_document_assets().map_err(store_error)?;
    store.create_source_frame_document().map_err(store_error)?;
    store.rename_project("Stage 15-16 Feedback Sample").map_err(store_error)?;
    let sample_summary = store.summary().map_err(store_error)?;
    let asset_version = sample_summary
        .source_sets
        .iter()
        .find(|source_set| source_set.id.to_string() == source_set_id.to_string())
        .ok_or_else(|| error(ErrorCode::ProjectInvalid, "The feedback sample source set is missing."))?
        .source_revision
        .to_string();
    let document = sample_summary
        .document
        .ok_or_else(no_project)?;
    let region = document
        .topology
        .regions
        .first()
        .ok_or_else(|| error(ErrorCode::ProjectInvalid, "The feedback sample has no regions."))?;
    let requested = hot_trimmer_effect_compiler::RequestedProfile {
        program: hot_trimmer_effect_compiler::ProfileProgram::ConvexBevel,
        first_width: hot_trimmer_effect_compiler::ProfileLength::Meters(0.004),
        second_width: hot_trimmer_effect_compiler::ProfileLength::Meters(0.004),
        minimum_flat_center: hot_trimmer_effect_compiler::ProfileLength::Meters(0.001),
        amplitude: hot_trimmer_effect_compiler::ProfileLength::Meters(0.002),
        angle_degrees: 45.0,
        inner_radius: hot_trimmer_effect_compiler::ProfileLength::Meters(0.0),
        outer_radius: hot_trimmer_effect_compiler::ProfileLength::Meters(0.0),
        legality_policy: hot_trimmer_effect_compiler::ProfileLegalityPolicy::Clamp,
        lod_policy: hot_trimmer_effect_compiler::ProfileLodPolicy::Auto,
        maximum_supersampling: 8,
        seed: 201_520,
        custom_curve: Vec::new(),
    };
    store
        .execute_document_command(&TrimSheetDocumentCommand::SetFeedbackProfile {
            region_id: region.id,
            structural_profile: StructuralProfile::Bevel,
            compiled_request: serde_json::to_string(&requested)
                .map_err(|failure| error(ErrorCode::Internal, &failure.to_string()))?,
        })
        .map_err(store_error)?;
    let asset = hot_trimmer_effect_compiler::StampAssetRef {
        asset_id: source_set_id.to_string(),
        version: asset_version,
        digest: hot_trimmer_domain::ContentDigest(asset_digest),
        kind: "registered_stamp_channels".into(),
    };
    let definition = hot_trimmer_effect_compiler::DetailDefinition {
        name: region.id.to_string(),
        family: hot_trimmer_effect_compiler::DetailFamily::PanelStamp,
        physical_size: [0.03, 0.03],
        scale_space: hot_trimmer_effect_compiler::EffectScaleSpace::World,
        compatible_roles: vec![region.role],
        orientation: hot_trimmer_effect_compiler::DetailOrientation::Slot,
        explicit_rotation_degrees: 0.0,
        aspect_limits: [0.25, 4.0],
        minimum_pixels: [2, 2],
        repeat_period_m: None,
        fit_policy: hot_trimmer_effect_compiler::DetailFitPolicy::Contain,
        mapping_mode: hot_trimmer_effect_compiler::DetailMappingMode::Planar,
        channels: vec![hot_trimmer_effect_compiler::DetailChannelContribution {
            channel: MaterialChannelRole::Height,
            amount: 0.0015,
            blend: hot_trimmer_effect_compiler::StampBlendPolicy::Add,
            material_id: None,
            metallic_explicit: false,
        }],
        fallback: hot_trimmer_effect_compiler::DetailFallback::NormalOnly,
        provenance: "bundled Prompt 20A deterministic feedback sample".into(),
        seed: 201_516,
        required_sources: vec![asset.clone()],
        required_halo_px: 2,
        dependencies: Vec::new(),
    };
    let operation = hot_trimmer_effect_compiler::StampOperation {
        asset,
        scope: hot_trimmer_effect_compiler::StampScope::MaterialReusableAtlas,
        target_region: region.id.to_string(),
        physical_position_m: [0.05, 0.05],
        physical_size_m: [0.03, 0.03],
        pivot: [0.5, 0.5],
        rotation_degrees: 15.0,
        mirror: [false, false],
        opacity: 1.0,
        blend: hot_trimmer_effect_compiler::StampBlendPolicy::Add,
        clipping: hot_trimmer_effect_compiler::DetailFitPolicy::Contain,
        seed: 201_520,
        spacing_m: [0.04, 0.04],
        scatter: 0.0,
        jitter_m: [0.0, 0.0],
        layer_order: 0,
        occupancy: hot_trimmer_effect_compiler::OccupancyRelation::OnlyFlatCenter,
        channels: vec![
            hot_trimmer_effect_compiler::DetailChannelContribution {
                channel: MaterialChannelRole::Height,
                amount: 0.0015,
                blend: hot_trimmer_effect_compiler::StampBlendPolicy::Add,
                material_id: None,
                metallic_explicit: false,
            },
            hot_trimmer_effect_compiler::DetailChannelContribution {
                channel: MaterialChannelRole::MaterialId,
                amount: 1.0,
                blend: hot_trimmer_effect_compiler::StampBlendPolicy::Replace,
                material_id: Some(0),
                metallic_explicit: false,
            },
        ],
    };
    for (id, intent) in [
        ("20152016-0000-4000-8000-000000000002", FeedbackDetailIntent::Definition(definition)),
        ("20152016-0000-4000-8000-000000000003", FeedbackDetailIntent::Operation(operation)),
    ] {
        store
            .execute_document_command(&TrimSheetDocumentCommand::UpsertDecoration {
                decoration: feedback_decoration(id, true, &intent)?,
            })
            .map_err(store_error)?;
    }
    session.mark_mutated();
    preview_service.reset();
    project_projection(session)
}

#[tauri::command]
pub fn regenerate_source_frame_partition(
    request: SourceFramePartitionRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    session
        .store
        .as_mut()
        .ok_or_else(no_project)?
        .regenerate_source_frame_partition(request.target_region_count)
        .map_err(store_error)?;
    session.mark_mutated();
    project_projection(&session)
}

#[tauri::command]
pub fn apply_document_command(
    request: DocumentCommandRequest,
    session: State<'_, SharedProjectSession>,
    preview_service: State<'_, SharedPreviewService>,
) -> Result<ProjectProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    if let TrimSheetDocumentCommand::AcceptSourceFramePartition { recipe } = &request.command {
        let revision = session
            .store
            .as_ref()
            .ok_or_else(no_project)?
            .summary()
            .map_err(store_error)?
            .document
            .ok_or_else(no_project)?
            .document_revision;
        let previewed = preview_service
            .previewed_candidate_recipes
            .lock()
            .map_err(|_| poisoned())?
            .contains(&(revision, recipe.hash()));
        if !previewed {
            return Err(error(
                ErrorCode::LayoutInvalid,
                "Preview this exact layout candidate before accepting it.",
            ));
        }
    }
    session
        .store
        .as_mut()
        .ok_or_else(no_project)?
        .execute_document_command(&request.command)
        .map_err(store_error)?;
    session.mark_mutated();
    project_projection(&session)
}

#[tauri::command]
pub fn apply_feedback_workbench_command(
    request: FeedbackWorkbenchCommandRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<FeedbackWorkbenchCommandResult, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    if request.command_version != FEEDBACK_COMMAND_VERSION {
        return Err(error(
            ErrorCode::ProtocolMismatch,
            "The Profile & Detail Contributions command version is stale or unknown.",
        ));
    }
    let mut session = session.lock().map_err(|_| poisoned())?;
    let summary = session
        .store
        .as_ref()
        .ok_or_else(no_project)?
        .summary()
        .map_err(store_error)?;
    let document = summary.document.as_ref().ok_or_else(no_project)?;
    let (domain_command, committed_identity) = match request.command {
        FeedbackWorkbenchCommand::SetProfile {
            region_id,
            requested,
        } => {
            let region = document
                .topology
                .regions
                .iter()
                .find(|region| region.id == region_id)
                .ok_or_else(|| error(ErrorCode::InvalidInput, "The selected region no longer exists."))?;
            let structural_profile = structural_profile_for_feedback(requested.program, region.role)?;
            let compiled_request = serde_json::to_string(&requested)
                .map_err(|failure| error(ErrorCode::Internal, &failure.to_string()))?;
            let identity = hot_trimmer_domain::ContentDigest::sha256(compiled_request.as_bytes()).0;
            (
                TrimSheetDocumentCommand::SetFeedbackProfile {
                    region_id,
                    structural_profile,
                    compiled_request,
                },
                identity,
            )
        }
        FeedbackWorkbenchCommand::SetEdgeWear { intent } => {
            let identity = hot_trimmer_domain::ContentDigest::sha256(
                &serde_json::to_vec(&intent)
                    .map_err(|failure| error(ErrorCode::Internal, &failure.to_string()))?,
            )
            .0;
            (
                TrimSheetDocumentCommand::SetEdgeWearIntent { intent },
                identity,
            )
        }
        FeedbackWorkbenchCommand::UpsertDetail {
            operation_id,
            enabled,
            intent,
        } => {
            validate_feedback_detail_target(document, &intent)?;
            let operation_id = operation_id.unwrap_or_else(|| Uuid::new_v4().to_string());
            validate_feedback_operation_id(&operation_id)?;
            let decoration = feedback_decoration(&operation_id, enabled, &intent)?;
            (
                TrimSheetDocumentCommand::UpsertDecoration { decoration },
                operation_id,
            )
        }
        FeedbackWorkbenchCommand::DuplicateDetail { operation_id } => {
            validate_feedback_operation_id(&operation_id)?;
            let existing = find_feedback_decoration(document, &operation_id)?;
            let intent = parse_feedback_detail(existing)?;
            let duplicate_id = Uuid::new_v4().to_string();
            let decoration = feedback_decoration(
                &duplicate_id,
                !existing.decoration_key.starts_with("stage16.disabled."),
                &intent,
            )?;
            (
                TrimSheetDocumentCommand::UpsertDecoration { decoration },
                duplicate_id,
            )
        }
        FeedbackWorkbenchCommand::SetDetailEnabled {
            operation_id,
            enabled,
        } => {
            validate_feedback_operation_id(&operation_id)?;
            let existing = find_feedback_decoration(document, &operation_id)?;
            let intent = parse_feedback_detail(existing)?;
            let replacement = feedback_decoration(&operation_id, enabled, &intent)?;
            (
                TrimSheetDocumentCommand::ReplaceDecoration {
                    old_decoration_key: existing.decoration_key.clone(),
                    decoration: replacement,
                },
                operation_id,
            )
        }
        FeedbackWorkbenchCommand::DeleteDetail { operation_id } => {
            validate_feedback_operation_id(&operation_id)?;
            let existing = find_feedback_decoration(document, &operation_id)?;
            (
                TrimSheetDocumentCommand::DeleteDecoration {
                    decoration_key: existing.decoration_key.clone(),
                },
                operation_id,
            )
        }
        FeedbackWorkbenchCommand::ReorderDetails { operation_ids } => {
            let detail_bindings = document
                .decorations
                .iter()
                .filter(|decoration| is_feedback_detail_key(&decoration.decoration_key))
                .collect::<Vec<_>>();
            if operation_ids.len() != detail_bindings.len()
                || operation_ids.iter().collect::<BTreeSet<_>>().len() != operation_ids.len()
            {
                return Err(error(ErrorCode::InvalidInput, "The detail order is stale."));
            }
            let mut ordered = document
                .decorations
                .iter()
                .filter(|decoration| !is_feedback_detail_key(&decoration.decoration_key))
                .map(|decoration| decoration.decoration_key.clone())
                .collect::<Vec<_>>();
            for operation_id in &operation_ids {
                validate_feedback_operation_id(operation_id)?;
                ordered.push(find_feedback_decoration(document, operation_id)?.decoration_key.clone());
            }
            (
                TrimSheetDocumentCommand::ReorderDecorations {
                    decoration_keys: ordered,
                },
                hot_trimmer_domain::ContentDigest::sha256(operation_ids.join("|").as_bytes()).0,
            )
        }
    };
    session
        .store
        .as_mut()
        .ok_or_else(no_project)?
        .execute_document_command(&domain_command)
        .map_err(store_error)?;
    session.mark_mutated();
    Ok(FeedbackWorkbenchCommandResult {
        command_version: FEEDBACK_COMMAND_VERSION,
        committed_identity,
        project: project_projection(&session)?,
        status: "executed",
    })
}

fn structural_profile_for_feedback(
    program: hot_trimmer_effect_compiler::ProfileProgram,
    role: TemplateSlotRole,
) -> Result<StructuralProfile, UserFacingError> {
    use hot_trimmer_effect_compiler::ProfileProgram;
    let radial = matches!(program, ProfileProgram::RadialDisc | ProfileProgram::Annulus);
    if radial != matches!(role, TemplateSlotRole::Radial) && program != ProfileProgram::Flat {
        return Err(error(
            ErrorCode::InvalidInput,
            "That structural profile is not legal for the selected slot role.",
        ));
    }
    match program {
        ProfileProgram::Flat => Ok(StructuralProfile::Flat),
        ProfileProgram::ConvexBevel => Ok(StructuralProfile::Bevel),
        ProfileProgram::ConcaveGroove => Ok(StructuralProfile::Groove),
        ProfileProgram::RoundedBevel => Ok(StructuralProfile::RoundedBevel),
        ProfileProgram::PanelFrame => Ok(StructuralProfile::PanelFrame),
        ProfileProgram::RadialDisc => Ok(StructuralProfile::RadialDisc),
        ProfileProgram::Annulus => Ok(StructuralProfile::Annulus),
        _ => Err(error(
            ErrorCode::InvalidInput,
            "Prompt 20A exposes only Flat, Bevel, Rounded Bevel, Groove, Panel Frame, Radial Disc, and Annulus.",
        )),
    }
}

fn validate_feedback_detail_target(
    document: &TrimSheetDocument,
    intent: &FeedbackDetailIntent,
) -> Result<(), UserFacingError> {
    let target = match intent {
        FeedbackDetailIntent::Definition(value) => value.name.as_str(),
        FeedbackDetailIntent::Operation(value) => value.target_region.as_str(),
        FeedbackDetailIntent::Stroke(value) => value.operation.target_region.as_str(),
    };
    let target = target
        .parse::<RegionId>()
        .map_err(|_| error(ErrorCode::InvalidInput, "Detail target must be a stable physical region identity."))?;
    if !document.topology.regions.iter().any(|region| region.id == target) {
        return Err(error(ErrorCode::InvalidInput, "The detail target region no longer exists."));
    }
    Ok(())
}

fn validate_feedback_operation_id(value: &str) -> Result<(), UserFacingError> {
    Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| error(ErrorCode::InvalidInput, "The detail operation identity is malformed."))
}

fn feedback_decoration(
    operation_id: &str,
    enabled: bool,
    intent: &FeedbackDetailIntent,
) -> Result<DecorationBinding, UserFacingError> {
    let target = match intent {
        FeedbackDetailIntent::Definition(value) => value.name.as_str(),
        FeedbackDetailIntent::Operation(value) => value.target_region.as_str(),
        FeedbackDetailIntent::Stroke(value) => value.operation.target_region.as_str(),
    };
    let kind = match intent {
        FeedbackDetailIntent::Definition(_) => "detail.definition",
        FeedbackDetailIntent::Operation(_) => "stamp.operation",
        FeedbackDetailIntent::Stroke(_) => "stamp.stroke",
    };
    let prefix = if enabled { "stage16" } else { "stage16.disabled" };
    let value = match intent {
        FeedbackDetailIntent::Definition(value) => serde_json::to_string(value),
        FeedbackDetailIntent::Operation(value) => serde_json::to_string(value),
        FeedbackDetailIntent::Stroke(value) => serde_json::to_string(value),
    }
    .map_err(|failure| error(ErrorCode::Internal, &failure.to_string()))?;
    Ok(DecorationBinding {
        decoration_key: format!("{prefix}.{kind}.{operation_id}.{target}"),
        value,
    })
}

fn is_feedback_detail_key(key: &str) -> bool {
    key.starts_with("stage16.detail.definition.")
        || key.starts_with("stage16.stamp.operation.")
        || key.starts_with("stage16.stamp.stroke.")
        || key.starts_with("stage16.disabled.detail.definition.")
        || key.starts_with("stage16.disabled.stamp.operation.")
        || key.starts_with("stage16.disabled.stamp.stroke.")
}

fn rewrite_feedback_raw_field(
    document: &mut TrimSheetDocument,
    view: FeedbackContributionView,
) -> Result<(), UserFacingError> {
    for binding in &mut document.decorations {
        if binding.decoration_key.contains(".detail.definition.") {
            let mut definition: hot_trimmer_effect_compiler::DetailDefinition = serde_json::from_str(&binding.value)
                .map_err(|failure| error(ErrorCode::InvalidInput, &format!("The persisted detail definition is malformed: {failure}")))?;
            rewrite_feedback_channels(&mut definition.channels, view);
            binding.value = serde_json::to_string(&definition).map_err(|failure| error(ErrorCode::Internal, &failure.to_string()))?;
        } else if binding.decoration_key.contains(".stamp.operation.") {
            let mut operation: hot_trimmer_effect_compiler::StampOperation = serde_json::from_str(&binding.value)
                .map_err(|failure| error(ErrorCode::InvalidInput, &format!("The persisted stamp operation is malformed: {failure}")))?;
            rewrite_feedback_operation_field(&mut operation, view);
            binding.value = serde_json::to_string(&operation).map_err(|failure| error(ErrorCode::Internal, &failure.to_string()))?;
        } else if binding.decoration_key.contains(".stamp.stroke.") {
            let mut stroke: hot_trimmer_effect_compiler::StampStroke = serde_json::from_str(&binding.value)
                .map_err(|failure| error(ErrorCode::InvalidInput, &format!("The persisted stamp stroke is malformed: {failure}")))?;
            rewrite_feedback_operation_field(&mut stroke.operation, view);
            binding.value = serde_json::to_string(&stroke).map_err(|failure| error(ErrorCode::Internal, &failure.to_string()))?;
        }
    }
    Ok(())
}

fn rewrite_feedback_operation_field(
    operation: &mut hot_trimmer_effect_compiler::StampOperation,
    view: FeedbackContributionView,
) {
    rewrite_feedback_channels(&mut operation.channels, view);
}

fn rewrite_feedback_channels(
    channels: &mut Vec<hot_trimmer_effect_compiler::DetailChannelContribution>,
    view: FeedbackContributionView,
) {
    use FeedbackContributionView as View;
    if matches!(view, View::Stage16RegisteredMask) {
        channels.retain(|channel| channel.channel == MaterialChannelRole::Height);
        for channel in channels {
            channel.channel = MaterialChannelRole::EdgeMask;
            channel.amount = 1.0;
            channel.blend = hot_trimmer_effect_compiler::StampBlendPolicy::Replace;
        }
    } else if matches!(view, View::Stage16MaterialIdValidity) {
        channels.retain(|channel| channel.channel == MaterialChannelRole::MaterialId);
        for channel in channels {
            channel.amount = 1.0;
            channel.material_id = Some(255);
            channel.blend = hot_trimmer_effect_compiler::StampBlendPolicy::Replace;
        }
    }
}

fn find_feedback_decoration<'a>(
    document: &'a TrimSheetDocument,
    operation_id: &str,
) -> Result<&'a DecorationBinding, UserFacingError> {
    let needle = format!(".{operation_id}.");
    document
        .decorations
        .iter()
        .find(|decoration| is_feedback_detail_key(&decoration.decoration_key) && decoration.decoration_key.contains(&needle))
        .ok_or_else(|| error(ErrorCode::InvalidInput, "The detail operation no longer exists."))
}

fn parse_feedback_detail(binding: &DecorationBinding) -> Result<FeedbackDetailIntent, UserFacingError> {
    let parse = |failure: serde_json::Error| error(ErrorCode::InvalidInput, &format!("The persisted detail intent is malformed: {failure}"));
    if binding.decoration_key.contains(".detail.definition.") {
        serde_json::from_str(&binding.value).map(FeedbackDetailIntent::Definition).map_err(parse)
    } else if binding.decoration_key.contains(".stamp.operation.") {
        serde_json::from_str(&binding.value).map(FeedbackDetailIntent::Operation).map_err(parse)
    } else {
        serde_json::from_str(&binding.value).map(FeedbackDetailIntent::Stroke).map_err(parse)
    }
}

#[tauri::command]
pub fn stage_15_20_debug_payload(
    request: Stage15To20DebugRequest,
    session: State<'_, SharedProjectSession>,
    preview_service: State<'_, SharedPreviewService>,
) -> Result<Stage15To20DebugPayload, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    if request.schema_version != STAGE_15_20_DEBUG_SCHEMA_VERSION {
        return Err(error(
            ErrorCode::ProtocolMismatch,
            "The Stage 15-20 debug payload schema is stale or unknown.",
        ));
    }
    let session = session.lock().map_err(|_| poisoned())?;
    let summary = session
        .store
        .as_ref()
        .ok_or_else(no_project)?
        .summary()
        .map_err(store_error)?;
    let document = summary.document.as_ref().ok_or_else(no_project)?;
    let appearance_hash = hash_hex(
        document
            .appearance_hash()
            .map_err(|failure| error(ErrorCode::LayoutInvalid, &failure.to_string()))?,
    );
    let safe_telemetry = request
        .bounded_telemetry
        .into_iter()
        .rev()
        .take(32)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|line| sanitize_debug_string(&line))
        .collect::<Vec<_>>();
    let stage16_detail_count = document
        .decorations
        .iter()
        .filter(|decoration| is_feedback_detail_key(&decoration.decoration_key))
        .count();
    let edge_wear_enabled = document.edge_wear.as_ref().is_some_and(|intent| intent.enabled);
    let stage16_count = stage16_detail_count + usize::from(edge_wear_enabled);
    let compiled_profile = request.compiled_inspection.as_ref().and_then(|value| value.get("compiledProfile"));
    let compiled_details = request.compiled_inspection.as_ref().and_then(|value| value.get("compiledDetails"));
    let cache_hit = matches!(&request.execution_outcome, FeedbackExecutionState::CacheHit);
    // The client carries the outcome of the versioned native request.  Do not
    // infer this from a display-view name: a Stage 16 request may evaluate the
    // Stage 15 occupancy dependency, and cache / cancellation outcomes apply
    // to that actual execution route.
    let stage15_state = if compiled_profile.is_some() {
        request.preview_state.clone()
    } else {
        FeedbackExecutionState::InstalledNotRequested
    };
    let stage16_state = if stage16_count == 0 {
        FeedbackExecutionState::SkippedBecauseUnused
    } else if edge_wear_enabled {
        request.preview_state.clone()
    } else if compiled_details.is_some() {
        request.preview_state.clone()
    } else {
        FeedbackExecutionState::InstalledNotRequested
    };
    let tile_evidence = request.tile.clone();
    let execution_evidence = serde_json::json!({
        "requestIdentity": request.request_identity.clone(),
        "outcome": request.execution_outcome.clone(),
        "dispatch": { "count": request.pixel_dispatch_count, "records": safe_telemetry.iter().filter(|line| line.contains("dispatch")).cloned().collect::<Vec<_>>() },
        "cache": { "hit": cache_hit, "records": safe_telemetry.iter().filter(|line| line.to_ascii_lowercase().contains("cache")).cloned().collect::<Vec<_>>() },
        "timings": tile_evidence.as_ref().and_then(|value| value.get("telemetry")).cloned(),
        "upload": safe_telemetry.iter().filter(|line| line.to_ascii_lowercase().contains("upload")).cloned().collect::<Vec<_>>(),
        "residency": safe_telemetry.iter().filter(|line| line.to_ascii_lowercase().contains("residen")).cloned().collect::<Vec<_>>(),
        "pins": safe_telemetry.iter().filter(|line| line.to_ascii_lowercase().contains("pin")).cloned().collect::<Vec<_>>(),
        "formats": tile_evidence.as_ref().and_then(|value| value.pointer("/manifest/format")).cloned(),
        "shaderIdentities": safe_telemetry.iter().filter(|line| line.to_ascii_lowercase().contains("shader")).cloned().collect::<Vec<_>>(),
        "readback": tile_evidence.as_ref().and_then(|value| value.get("telemetry")).cloned(),
        "cpuRaster": { "profile": 0, "detailMaskSdfStamp": 0, "evidence": "GPU tile publication path; no CPU raster fallback installed" },
    });
    let project_label = Path::new(&summary.path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("feedback-project")
        .to_owned();
    let payload = serde_json::json!({
        "app": {
            "version": env!("CARGO_PKG_VERSION"),
            "build": option_env!("HOT_TRIMMER_BUILD_ID").unwrap_or("development"),
            "protocolVersion": IPC_PROTOCOL_VERSION,
            "schemaVersion": STAGE_15_20_DEBUG_SCHEMA_VERSION,
        },
        "gpu": {
            "capabilityGeneration": preview_service.gpu_capabilities.generation(),
            "adapterBackendCapabilities": safe_telemetry.iter().find(|line| line.contains("gpu_")).cloned(),
        },
        "identity": {
            "projectId": summary.id.to_string(),
            "projectLabel": project_label,
            "documentId": document.id.to_string(),
            "documentRevision": document.document_revision,
            "topologyRevision": document.topology_revision,
            "appearanceRevision": document.appearance_revision,
            "topologyHash": hash_hex(document.topology.topology_hash),
            "appearanceHash": appearance_hash,
        },
        "selection": {
            "regionId": request.selected_region_id,
            "physicalScale": request.compiled_inspection.as_ref().and_then(|value| value.get("compiledProfile")).and_then(|value| value.get("slotSizeM")).cloned(),
        },
        "request": {
            "identity": request.request_identity,
            "view": request.requested_view,
            "profile": request.preview_profile,
            "comparisonMode": request.comparison_mode,
            "selectedOperationId": request.selected_operation_id,
        },
        "tilePublication": request.tile.and_then(|value| sanitize_debug_value(value, None)),
        "executionEvidence": execution_evidence,
        "stages": {
            "15": { "state": stage15_state, "inspection": compiled_profile.cloned().and_then(|value| sanitize_debug_value(value, None)), "cpuProfileRasterCounters": { "count": 0, "route": "gpu" } },
            "16": { "state": stage16_state, "intentCount": stage16_count, "inspection": compiled_details.cloned().and_then(|value| sanitize_debug_value(value, None)), "cpuDetailMaskSdfStampRasterCounters": { "count": 0, "route": "gpu" } },
            "18": { "state": FeedbackExecutionState::NotInstalled, "installedVersion": serde_json::Value::Null },
            "17": { "state": FeedbackExecutionState::NotInstalled, "installedVersion": serde_json::Value::Null },
            "19": { "state": FeedbackExecutionState::NotInstalled, "installedVersion": serde_json::Value::Null },
            "20": { "state": FeedbackExecutionState::NotInstalled, "installedVersion": serde_json::Value::Null },
        },
        "workbench20A": {
            "version": "20A.1",
            "schemaVersion": STAGE_15_20_DEBUG_SCHEMA_VERSION,
            "activeTool": request.active_tool,
            "lastTypedCommandResult": request.last_command_result.map(|value| sanitize_debug_string(&value)),
            "authoringPreviewError": request.preview_error,
            "state": request.workbench_state.and_then(|value| sanitize_debug_value(value, None)),
        },
        "previewClient": request.paint_summary.and_then(|value| sanitize_debug_value(value, None)),
        "boundedTelemetry": safe_telemetry,
    });
    let summary_text = format!(
        "Profile & Detail Contributions | view={} | region={} | Stage 15={stage15_state:?} | Stage 16={stage16_state:?} | Stages 18/17/19/20=NotInstalled",
        request.requested_view,
        request.selected_region_id.as_deref().unwrap_or("none"),
    );
    Ok(Stage15To20DebugPayload {
        schema: "hot-trimmer.stage15-20-feedback",
        schema_version: STAGE_15_20_DEBUG_SCHEMA_VERSION,
        summary: summary_text,
        payload,
    })
}

fn sanitize_debug_string(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    if lower.contains("data:image")
        || lower.contains("base64")
        || lower.contains("clipboard")
        || lower.contains("credential")
        || lower.contains("environment variable")
        || value.contains(":\\")
        || value.starts_with('/')
        || lower.contains("/users/")
    {
        "[redacted prohibited debug value]".into()
    } else {
        value.chars().take(2_048).collect()
    }
}

fn sanitize_debug_value(value: serde_json::Value, key: Option<&str>) -> Option<serde_json::Value> {
    let forbidden_key = key.is_some_and(|key| {
        let key = key.to_ascii_lowercase();
        key.contains("pixel")
            || key.contains("encoded")
            || key.contains("clipboard")
            || key.contains("credential")
            || key.contains("environment")
            || key == "path"
            || key.ends_with("path")
            || key == "bytes"
            || key.ends_with("bytes")
    });
    if forbidden_key {
        return None;
    }
    match value {
        serde_json::Value::String(value) => Some(serde_json::Value::String(sanitize_debug_string(&value))),
        serde_json::Value::Array(values) => Some(serde_json::Value::Array(
            values
                .into_iter()
                .take(256)
                .filter_map(|value| sanitize_debug_value(value, None))
                .collect(),
        )),
        serde_json::Value::Object(values) => Some(serde_json::Value::Object(
            values
                .into_iter()
                .filter_map(|(key, value)| sanitize_debug_value(value, Some(&key)).map(|value| (key, value)))
                .collect(),
        )),
        other => Some(other),
    }
}


#[tauri::command]
pub fn apply_patch_command(
    request: PatchCommandRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    if let PatchCommand::Delete { patch_id } = &request.command {
        let referenced_regions = session
            .store
            .as_ref()
            .ok_or_else(no_project)?
            .summary()
            .map_err(store_error)?
            .document
            .into_iter()
            .flat_map(|document| document.region_bindings.into_values())
            .filter(
                |binding| matches!(binding.content, ContentReference::Patch(id) if id == *patch_id),
            )
            .map(|binding| binding.region_id.to_string())
            .collect::<Vec<_>>();
        if !referenced_regions.is_empty() {
            return Err(error(
                ErrorCode::InvalidInput,
                &format!(
                    "Patch {patch_id} is assigned to region(s): {}",
                    referenced_regions.join(", ")
                ),
            ));
        }
    }
    let invalidated = {
        let store = session.store.as_mut().ok_or_else(no_project)?;
        let outcome = store
            .execute_patch_command(&request.command, request.coalescing_group)
            .map_err(store_error)?;
        store.refresh_document_assets().map_err(store_error)?;
        outcome.invalidated_patch_ids
    };
    for patch_id in invalidated {
        session.prepared_exemplars.invalidate_patch(patch_id);
    }
    session.mark_mutated();
    project_projection(&session)
}

#[tauri::command]
pub fn undo_patch_command(
    request: FoundationStatusRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectProjection, UserFacingError> {
    request.validate().map_err(UserFacingError::from)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    let invalidated = {
        let store = session.store.as_mut().ok_or_else(no_project)?;
        let outcome = store.undo_patch_command().map_err(store_error)?;
        store.refresh_document_assets().map_err(store_error)?;
        outcome.invalidated_patch_ids
    };
    for patch_id in invalidated {
        session.prepared_exemplars.invalidate_patch(patch_id);
    }
    session.mark_mutated();
    project_projection(&session)
}

#[tauri::command]
pub fn redo_patch_command(
    request: FoundationStatusRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectProjection, UserFacingError> {
    request.validate().map_err(UserFacingError::from)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    let invalidated = {
        let store = session.store.as_mut().ok_or_else(no_project)?;
        let outcome = store.redo_patch_command().map_err(store_error)?;
        store.refresh_document_assets().map_err(store_error)?;
        outcome.invalidated_patch_ids
    };
    for patch_id in invalidated {
        session.prepared_exemplars.invalidate_patch(patch_id);
    }
    session.mark_mutated();
    project_projection(&session)
}

#[tauri::command]
pub fn prepare_patch_preview(
    request: PreparedPatchPreviewRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<PreparedPatchPreviewProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    if !(64..=2048).contains(&request.max_edge) {
        return Err(error(
            ErrorCode::InvalidInput,
            "Patch preview edge must be between 64 and 2048 pixels.",
        ));
    }
    let mut session = session.lock().map_err(|_| poisoned())?;
    let summary = session
        .store
        .as_ref()
        .ok_or_else(no_project)?
        .summary()
        .map_err(store_error)?;
    let patch = summary
        .patches
        .iter()
        .find(|patch| patch.id == request.patch_id)
        .ok_or_else(|| {
            error(
                ErrorCode::InvalidInput,
                "The selected patch no longer exists.",
            )
        })?;
    let transient = request.geometry.is_some();
    let geometry = request.geometry.as_ref().unwrap_or(&patch.geometry);
    let anchor = summary
        .sources
        .iter()
        .find(|source| source.input.id == patch.source_id)
        .ok_or_else(|| {
            error(
                ErrorCode::ProjectInvalid,
                "The patch source no longer exists.",
            )
        })?;
    let source_set_id = SourceSetId::from_bytes(*anchor.source_set_id.as_bytes());
    let source_set = summary
        .source_sets
        .iter()
        .find(|source_set| source_set.id == source_set_id)
        .ok_or_else(|| {
            error(
                ErrorCode::ProjectInvalid,
                "The patch material source no longer exists.",
            )
        })?;
    let prepared_source_key = (source_set_id.to_string(), source_set.source_revision);
    let prepared =
        if let Some(prepared) = session.preview_prepared_sources.get(&prepared_source_key) {
            Arc::clone(prepared)
        } else {
            let (registered, encoded_sources) =
                preview_registered_channel_set(&session, &summary.sources, anchor.source_set_id)?;
            let normalization_settings = NormalizationSettings {
                max_levels: 1,
                max_memory_bytes: 268_435_456,
                ..NormalizationSettings::default()
            };
            let prepared = Arc::new(
                prepare_registered_channel_set(
                    &registered,
                    &encoded_sources,
                    &normalization_settings,
                    &CancellationToken::new(),
                )
                .map_err(|failure| {
                    error(
                        ErrorCode::ImageImportFailed,
                        &format!("Source preparation failed: {failure}"),
                    )
                })?,
            );
            if session.preview_prepared_sources.len() >= 8 {
                session.preview_prepared_sources.clear();
            }
            session
                .preview_prepared_sources
                .insert(prepared_source_key, Arc::clone(&prepared));
            prepared
        };
    let patch_bytes = serde_json::to_vec(&(patch.id, geometry, patch.rectification))
        .map_err(|failure| error(ErrorCode::Internal, &failure.to_string()))?;
    let revision_digest = ContentDigest::sha256(&patch_bytes);
    let patch_revision = u64::from_str_radix(&revision_digest.0[..16], 16)
        .map_err(|failure| error(ErrorCode::Internal, &failure.to_string()))?;
    let exemplar_request = PreparedExemplarRequest {
        exemplar_id: patch.id.to_string(),
        area: PlanarArea::FourPoint {
            corners: geometry.corners,
        },
        lens_correction: None,
        mask: ExemplarMaskIntent {
            crop_polygon: geometry.assistance_mask.clone(),
            minimum_alpha: Some(1.0 / 255.0),
        },
        rectification: patch.rectification,
        physical_aspect_ratio: None,
        quality: RectificationQuality::Preview,
        limits: RectificationWorkLimits {
            preview_max_edge: request.max_edge,
            authoritative_max_edge: 8_192,
            max_pixels: 67_108_864,
            tile_edge: 128,
        },
        scope: PreparedExemplarScope {
            source_set_id,
            source_revision: source_set.source_revision,
            patch_id: Some(patch.id),
            patch_revision,
        },
    };
    let key = hot_trimmer_render_core::exemplar_cache_key(&prepared.cache_key, &exemplar_request);
    let cached = (!transient)
        .then(|| session.prepared_exemplars.get(&key))
        .flatten()
        .cloned();
    let exemplar = if let Some(cached) = cached {
        cached
    } else {
        let value = prepare_registered_exemplar(
            &prepared,
            &exemplar_request,
            &RenderCancellationToken::new(),
        )
        .map_err(|failure| error(ErrorCode::PatchGeometryInvalid, &failure.to_string()))?;
        if !transient {
            session.prepared_exemplars.insert_complete(value.clone());
        }
        value
    };
    let stage_four = prepare_delit_exemplar(
        &exemplar,
        &source_set.delighting,
        None,
        &RenderCancellationToken::new(),
    )
    .map_err(|failure| {
        error(
            ErrorCode::InvalidInput,
            &format!("Stage 4 source preparation failed: {failure}"),
        )
    })?;
    let analysis_settings = AnalysisSettings::default();
    let analysis_key = source_analysis_cache_key(&stage_four, &analysis_settings, None);
    let mut stage_five = if let Some(cached) = session.source_analysis_cache.get(&analysis_key) {
        cached.clone()
    } else {
        let report = analyze_source(
            &stage_four,
            &analysis_settings,
            None,
            &RenderCancellationToken::new(),
        )
        .map_err(|failure| {
            error(
                ErrorCode::InvalidInput,
                &format!("Stage 5 source analysis failed: {failure}"),
            )
        })?;
        session
            .source_analysis_cache
            .insert_complete(report.clone());
        report
    };
    stage_five.classification.routing_intent = source_set.classification;
    let inspector = stage_five.inspector_evidence();
    let stage_six_settings = ScaleOrientationSettings::default();
    let stage_six_key =
        scale_orientation_cache_key(&stage_five, &source_set.calibration, &stage_six_settings);
    let stage_six = if let Some(cached) = session.scale_orientation_cache.get(&stage_six_key) {
        cached.clone()
    } else {
        let report = calibrate_scale_orientation(
            &stage_four,
            &stage_five,
            &source_set.calibration,
            &stage_six_settings,
            &RenderCancellationToken::new(),
        )
        .map_err(|failure| {
            error(
                ErrorCode::InvalidInput,
                &format!("Stage 6 calibration failed: {failure}"),
            )
        })?;
        session
            .scale_orientation_cache
            .insert_complete(report.clone());
        report
    };
    prepared_preview_projection(
        &exemplar,
        stage_four.base_color(),
        &format!("{:?}", stage_four.route_execution),
        source_set.delighting.classical.strength_milli,
        source_set_id.to_string(),
        SourceInspectorProjection {
            quality_summary: inspector.quality_summary,
            analyzed_class: stage_five.classification.analyzed_class,
            routed_class: stage_five.classification.routed_class(),
            confidence_percent: inspector.confidence_percent,
            evidence_summary: inspector.evidence_summary,
            warning_count: inspector.warning_count,
            scale_summary: stage_six.measurement_overlay.label.clone(),
            orientation_summary: match stage_six.global_orientation.axis_millidegrees {
                Some(axis) => format!(
                    "axis {:.1}°, confidence {}%",
                    axis as f64 / 1000.0,
                    stage_six.global_orientation.confidence_milli / 10
                ),
                None => format!(
                    "orientation unavailable, confidence {}%",
                    stage_six.global_orientation.confidence_milli / 10
                ),
            },
            world_scale_available: stage_six.measurement_overlay.world_scale_available,
            orientation_overlay: stage_six
                .local_orientation
                .iter()
                .map(|sample| OrientationOverlayProjection {
                    source_x_milli: sample.source_x_milli,
                    source_y_milli: sample.source_y_milli,
                    axis_millidegrees: sample.axis_millidegrees,
                    confidence_milli: sample.confidence_milli,
                })
                .collect(),
        },
    )
}

#[tauri::command]
pub fn undo_document_command(
    request: FoundationStatusRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectProjection, UserFacingError> {
    request.validate().map_err(UserFacingError::from)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    session
        .store
        .as_mut()
        .ok_or_else(no_project)?
        .undo_document_command()
        .map_err(store_error)?;
    session.mark_mutated();
    project_projection(&session)
}

#[tauri::command]
pub fn redo_document_command(
    request: FoundationStatusRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectProjection, UserFacingError> {
    request.validate().map_err(UserFacingError::from)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    session
        .store
        .as_mut()
        .ok_or_else(no_project)?
        .redo_document_command()
        .map_err(store_error)?;
    session.mark_mutated();
    project_projection(&session)
}

#[tauri::command]
pub async fn compile_trim_sheet_document(
    request: CompileDocumentRequest,
    session: State<'_, SharedProjectSession>,
    preview_service: State<'_, SharedPreviewService>,
) -> Result<CompiledSheetProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let session = Arc::clone(session.inner());
    let preview_service = Arc::clone(preview_service.inner());
    tauri::async_runtime::spawn_blocking(move || {
        compile_trim_sheet_document_impl(&session, &preview_service)
    })
    .await
    .map_err(|join| error(ErrorCode::Internal, &format!("Build worker failed: {join}")))?
}

#[tauri::command]
pub async fn preview_through_stage_14(
    request: Stage14PreviewRequest,
    session: State<'_, SharedProjectSession>,
    preview_service: State<'_, SharedPreviewService>,
) -> Result<IntermediateAtlasProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let session = Arc::clone(session.inner());
    let preview_service = Arc::clone(preview_service.inner());
    let job = preview_service
        .latest_draft_id
        .fetch_add(1, Ordering::AcqRel)
        .saturating_add(1);
    tauri::async_runtime::spawn_blocking(move || {
        build_stage_14_preview(&session, &preview_service, request, job)
    })
    .await
    .map_err(|join| {
        error(
            ErrorCode::Internal,
            &format!("Stage 14 preview worker failed: {join}"),
        )
    })?
}

#[tauri::command]
pub async fn preview_stage_15_16_feedback(
    request: FeedbackQaTileRequest,
    session: State<'_, SharedProjectSession>,
    preview_service: State<'_, SharedPreviewService>,
) -> Result<IntermediateAtlasProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    if request.command_version != FEEDBACK_COMMAND_VERSION {
        return Err(error(
            ErrorCode::ProtocolMismatch,
            "The Stage 15/16 QA request version is stale or unknown.",
        ));
    }
    let map = feedback_map_for_view(request.view)?.ok_or_else(|| error(
        ErrorCode::InvalidInput,
        "This compiler QA view is metadata-only and intentionally dispatches zero pixel work.",
    ))?;
    let request_identity = feedback_preview_cache_identity(&request, map)?;
    preview_through_stage_14(
        Stage14PreviewRequest {
            protocol_version: request.protocol_version,
            revision: request.revision,
            region_id: Some(request.region_id),
            transient_projection: None,
            draft_id: Some(request.generation),
            input_hash: Some(request_identity),
            profile: request.profile,
            view_intent: feedback_preview_view_intent(&request),
            viewport_rect: None,
            requested_maps: vec![map],
            candidate_recipe: None,
            feedback_view: Some(request.view),
            feedback_comparison_mode: Some(request.comparison_mode),
            feedback_selected_operation_id: request.selected_operation_id,
        },
        session,
        preview_service,
    )
    .await
}

fn feedback_preview_view_intent(request: &FeedbackQaTileRequest) -> Option<PreviewViewIntent> {
    (!request.all_regions).then_some(PreviewViewIntent::ExactSelectedRegion)
}

fn feedback_map_for_view(
    view: FeedbackContributionView,
) -> Result<Option<MaterialMapKind>, UserFacingError> {
    use FeedbackContributionView as View;
    Ok(Some(match view {
        View::Stage15Occupancy => MaterialMapKind::AmbientOcclusion,
        View::Stage15Height | View::Stage16Height => MaterialMapKind::Height,
        View::Stage16RegisteredMask => MaterialMapKind::EdgeMask,
        View::Stage16VectorNormal => MaterialMapKind::Normal,
        View::Stage16ScalarRoughness => MaterialMapKind::Roughness,
        View::Stage16ScalarMetallic => MaterialMapKind::Metallic,
        View::Stage16ScalarAmbientOcclusion => MaterialMapKind::AmbientOcclusion,
        View::Stage16BaseColor => MaterialMapKind::BaseColor,
        View::Stage16MaterialId | View::Stage16MaterialIdValidity => MaterialMapKind::MaterialId,
        View::Stage15ProfileRoute | View::Stage15Lod | View::Stage15Fallback
        | View::Stage16Route | View::Stage16Occupancy | View::Stage16Lod
        | View::Stage16Scope | View::Stage16AssetResolution => return Ok(None),
    }))
}

fn feedback_preview_cache_identity(
    request: &FeedbackQaTileRequest,
    map: MaterialMapKind,
) -> Result<String, UserFacingError> {
    let payload = serde_json::to_vec(&(
        "stage15-16-feedback-v1",
        request.revision,
        request.region_id,
        request.all_regions,
        request.view,
        map,
        request.profile,
        request.comparison_mode,
        request.selected_operation_id.as_deref(),
    ))
    .map_err(|failure| error(ErrorCode::Internal, &failure.to_string()))?;
    Ok(ContentDigest::sha256(&payload).0)
}

#[tauri::command]
pub async fn export_stage_14_material_maps(
    request: NativeStage14ExportRequest,
    session: State<'_, SharedProjectSession>,
    preview_service: State<'_, SharedPreviewService>,
    app: AppHandle,
) -> Result<NativeStage14ExportProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let session = Arc::clone(session.inner());
    let preview_service = Arc::clone(preview_service.inner());
    tauri::async_runtime::spawn_blocking(move || {
        export_stage_14_material_maps_impl(&session, &preview_service, request, &app)
    })
    .await
    .map_err(|join| {
        error(
            ErrorCode::Internal,
            &format!("Stage 14 export worker failed: {join}"),
        )
    })?
}

/// Returns cache-owned interactive tile bytes as a Tauri binary IPC response.
/// The JSON manifest is published separately and contains no pixel transport.
#[tauri::command]
pub fn get_gpu_tiled_preview_payload(
    request: GpuTiledPreviewPayloadRequest,
    preview_service: State<'_, SharedPreviewService>,
) -> Result<tauri::ipc::Response, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let preview_service = preview_service.inner();
    if preview_service.latest_draft_id.load(Ordering::Acquire) != request.generation {
        return Err(error(
            ErrorCode::OperationCancelled,
            "The requested preview tile belongs to a superseded generation.",
        ));
    }
    let bytes = preview_service
        .gpu_tile_cache
        .lock()
        .map_err(|_| poisoned())?
        .resolve(&request.opaque_handle)
        .filter(|tile| tile.manifest.generation == request.generation)
        .map(|tile| tile.pixels().to_vec())
        .ok_or_else(|| {
            error(
                ErrorCode::InvalidInput,
                "The preview tile handle is unavailable.",
            )
        })?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
pub fn release_gpu_tiled_preview_payload(
    request: ReleaseGpuTiledPreviewPayloadRequest,
    preview_service: State<'_, SharedPreviewService>,
) -> Result<(), UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let preview_service = preview_service.inner();
    let released = preview_service
        .gpu_tile_cache
        .lock()
        .map_err(|_| poisoned())?
        .release(request.generation, &request.opaque_handle);
    if !released {
        return Err(error(
            ErrorCode::InvalidInput,
            "The preview tile handle is unavailable.",
        ));
    }
    Ok(())
}

fn publish_gpu_tiled_preview(
    preview_service: &PreviewService,
    manifest: CompiledAtlasTileManifest,
    pixels: Arc<[u8]>,
) -> Result<GpuTiledPreviewPublication, UserFacingError> {
    if preview_service.latest_draft_id.load(Ordering::Acquire) != manifest.generation {
        return Err(error(
            ErrorCode::OperationCancelled,
            "Preview publication was superseded before the tile entered the native cache.",
        ));
    }
    let started = Instant::now();
    let published = {
        let mut cache = preview_service
            .gpu_tile_cache
            .lock()
            .map_err(|_| poisoned())?;
        cache.begin_generation(manifest.generation);
        cache.publish(manifest, pixels).map_err(|failure| {
            error(
                ErrorCode::Internal,
                &format!("Tiled preview publication failed: {failure}"),
            )
        })?
    };
    if preview_service.latest_draft_id.load(Ordering::Acquire) != published.generation {
        let _ = preview_service
            .gpu_tile_cache
            .lock()
            .map(|mut cache| cache.release(published.generation, &published.opaque_handle));
        return Err(error(
            ErrorCode::OperationCancelled,
            "Preview publication was superseded before the manifest was published.",
        ));
    }
    let native_publish_ms = started.elapsed().as_millis();
    Ok(GpuTiledPreviewPublication {
        telemetry: GpuTiledPreviewTelemetry {
            generation: published.generation,
            native_publish_ms,
            raw_ipc_bytes: u64::from(published.row_stride) * u64::from(published.height),
            raw_ipc_ms: native_publish_ms,
        },
        manifest: published,
    })
}

#[allow(clippy::too_many_lines)]
#[cfg(any())]
fn removed_preview_specific_fabrication(
    session: &SharedProjectSession,
    preview_service: &PreviewService,
    requested_revision: u64,
    job: u64,
) -> Result<IntermediateAtlasProjection, UserFacingError> {
    use hot_trimmer_domain::{
        MaterialChannelRole, PixelSize, SamplingMode, SamplingPolicy, SourceSamplingMode,
        StageResult,
    };
    use hot_trimmer_image_io::{CategoryId, ImagePlane, LinearColor, LinearScalar, TangentNormal};
    use hot_trimmer_material_synthesis::PreparedMaterialDomain;
    use hot_trimmer_placement_solver::{
        CandidateDescriptors, CandidateFamily, CandidateRoute, CandidateTransform, CropCandidate,
        EligibilityEvidence, MirrorTransform, PlacementObjectiveBreakdown, PlacementPlan,
        PlacementPlanQaView, PlacementValidationSummary, PositionStrategy, SamplingPlan,
        SliceGeometry, SourceCrop, StretchOverrideProvenance,
    };
    use hot_trimmer_render_core::PreparedExemplarChannel;
    use hot_trimmer_sheet_compiler::{
        IntermediateAtlasRequest, IntermediateSlotInput, SlotSynthesisLimits, SlotSynthesisRequest,
        synthesize_slot_material,
    };

    let summary = {
        let guard = session.lock().map_err(|_| poisoned())?;
        guard
            .store
            .as_ref()
            .ok_or_else(no_project)?
            .summary()
            .map_err(store_error)?
    };
    let document = summary
        .document
        .as_ref()
        .ok_or_else(|| error(ErrorCode::LayoutInvalid, "Create a trim sheet first."))?;
    if document.document_revision != requested_revision {
        return Err(error(
            ErrorCode::OperationCancelled,
            "A newer document revision superseded this preview.",
        ));
    }
    document
        .primary_material
        .ok_or_else(|| error(ErrorCode::InvalidInput, "Choose a primary material first."))?;
    let selected = summary
        .sources
        .iter()
        .map(|source| registered_map_cached(source, preview_service))
        .collect::<Result<Vec<_>, _>>()?;
    if !selected
        .iter()
        .any(|map| map.kind == MaterialMapKind::BaseColor)
    {
        return Err(error(
            ErrorCode::InvalidInput,
            "Preview through Stage 14 requires imported Base Color.",
        ));
    }
    let dimensions = selected
        .first()
        .map(|map| [map.width, map.height])
        .unwrap_or([0, 0]);
    if selected
        .iter()
        .any(|map| [map.width, map.height] != dimensions)
    {
        return Err(error(
            ErrorCode::SourceRegistrationFailed,
            "Imported registered channels are not aligned.",
        ));
    }
    let mut channels = Vec::new();
    for map in &selected {
        let pixel_count = usize::try_from(u64::from(map.width) * u64::from(map.height))
            .map_err(|_| error(ErrorCode::InvalidInput, "Imported material is too large."))?;
        match map.kind {
            MaterialMapKind::BaseColor => {
                let values = map
                    .rgba8
                    .chunks_exact(4)
                    .map(|pixel| LinearColor {
                        rgb: [
                            srgb_to_linear(pixel[0]),
                            srgb_to_linear(pixel[1]),
                            srgb_to_linear(pixel[2]),
                        ],
                        alpha: f32::from(pixel[3]) / 255.0,
                    })
                    .collect::<Vec<_>>();
                channels.push(PreparedExemplarChannel::BaseColor {
                    plane: ImagePlane::from_row_major(map.width, map.height, 128, &values)
                        .map_err(|failure| error(ErrorCode::InvalidInput, &failure.to_string()))?,
                    alpha_mode: hot_trimmer_image_io::ResolvedAlphaMode::Straight,
                });
            }
            MaterialMapKind::Normal => {
                let values = map
                    .rgba8
                    .chunks_exact(4)
                    .map(|pixel| TangentNormal {
                        xyz: [
                            f32::from(pixel[0]) / 127.5 - 1.0,
                            f32::from(pixel[1]) / 127.5 - 1.0,
                            f32::from(pixel[2]) / 127.5 - 1.0,
                        ],
                        alpha: f32::from(pixel[3]) / 255.0,
                    })
                    .collect::<Vec<_>>();
                channels.push(PreparedExemplarChannel::Normal {
                    plane: ImagePlane::from_row_major(map.width, map.height, 128, &values)
                        .map_err(|failure| error(ErrorCode::InvalidInput, &failure.to_string()))?,
                    source_convention: NormalConvention::OpenGl,
                    canonical_convention: NormalConvention::OpenGl,
                    alpha_policy: hot_trimmer_image_io::NormalAlphaPolicy::Preserve,
                });
            }
            MaterialMapKind::MaterialId => {
                let values = map
                    .rgba8
                    .chunks_exact(4)
                    .map(|pixel| CategoryId(u32::from_le_bytes([pixel[0], pixel[1], pixel[2], 0])))
                    .collect::<Vec<_>>();
                channels.push(PreparedExemplarChannel::MaterialId {
                    plane: ImagePlane::from_row_major(map.width, map.height, 128, &values)
                        .map_err(|failure| error(ErrorCode::InvalidInput, &failure.to_string()))?,
                });
            }
            kind => {
                let role = match kind {
                    MaterialMapKind::Height => MaterialChannelRole::Height,
                    MaterialMapKind::Roughness => MaterialChannelRole::Roughness,
                    MaterialMapKind::Metallic => MaterialChannelRole::Metallic,
                    MaterialMapKind::AmbientOcclusion => MaterialChannelRole::AmbientOcclusion,
                    MaterialMapKind::Specular => MaterialChannelRole::Specular,
                    MaterialMapKind::Opacity => MaterialChannelRole::Opacity,
                    MaterialMapKind::EdgeMask => MaterialChannelRole::EdgeMask,
                    _ => continue,
                };
                let values = map
                    .rgba8
                    .chunks_exact(4)
                    .take(pixel_count)
                    .map(|pixel| LinearScalar(f32::from(pixel[0]) / 255.0))
                    .collect::<Vec<_>>();
                channels.push(PreparedExemplarChannel::Scalar {
                    role,
                    plane: ImagePlane::from_row_major(map.width, map.height, 128, &values)
                        .map_err(|failure| error(ErrorCode::InvalidInput, &failure.to_string()))?,
                });
            }
        }
    }
    let domain_id = ContentDigest::sha256(
        format!(
            "{}|{:?}",
            primary,
            document
                .appearance_hash()
                .map_err(|failure| error(ErrorCode::LayoutInvalid, &failure.to_string()))?
        )
        .as_bytes(),
    );
    let source_id = ContentDigest::sha256(
        selected
            .iter()
            .flat_map(|map| map.sha256.as_bytes())
            .copied()
            .collect::<Vec<_>>()
            .as_slice(),
    );
    let domain =
        PreparedMaterialDomain::from_registered_channels(domain_id.clone(), source_id, channels)
            .map_err(|failure| {
                error(
                    ErrorCode::InvalidInput,
                    &format!("Stage 8 registered domain failed: {failure}"),
                )
            })?;
    // Source-frame layouts are already an accepted, integer rectangle topology.  Do not force
    // them through a template snapshot: that made every candidate look like a usable control
    // while Stage 14 could only compile old template state.
    let topology = if let Some(snapshot) = document.topology.snapshot.template.as_ref() {
        let definition: hot_trimmer_domain::TemplateDefinition =
            serde_json::from_str(&snapshot.snapshot_json).map_err(|failure| {
                error(
                    ErrorCode::LayoutInvalid,
                    &format!("Persisted template is invalid: {failure}"),
                )
            })?;
        definition
            .compile_for_output(PixelSize {
                width: document.render_settings.output_size.width,
                height: document.render_settings.output_size.height,
            })
            .map_err(|failure| error(ErrorCode::LayoutInvalid, &failure.to_string()))?
    } else {
        CompiledTemplateTopology {
            identity: TemplateIdentity {
                template_id: "source-frame-partition".into(),
                template_version: document.partition_provenance.as_ref().map_or_else(
                    || "authored".into(),
                    |value| value.recipe.recipe_version.to_string(),
                ),
                compatibility_key: document.topology.compatibility_key.clone(),
            },
            output_size: document.render_settings.output_size,
            slots: document
                .topology
                .regions
                .iter()
                .map(|region| CompiledTemplateSlot {
                    slot_key: region.id.to_string(),
                    allocation: region.allocation_rect,
                    hotspot: region.hotspot_rect,
                })
                .collect(),
        }
    };
    let resolved = hot_trimmer_sheet_compiler::resolve_compile_plan(document, &selected)
        .map_err(|failure| error(ErrorCode::LayoutInvalid, &failure.to_string()))?;

    let mut plans = Vec::new();
    for (index, slot) in topology.slots.iter().enumerate() {
        let region = document.topology.regions.get(index).ok_or_else(|| {
            error(
                ErrorCode::LayoutInvalid,
                "Stage 9 slot order drifted from the document.",
            )
        })?;
        let mode = match region.orientation {
            hot_trimmer_domain::RegionOrientation::Horizontal => SamplingMode::RepeatX,
            hot_trimmer_domain::RegionOrientation::Vertical => SamplingMode::RepeatY,
            _ => SamplingMode::PeriodicTile,
        };
        let period = [domain.width.max(1), domain.height.max(1)];
        let candidate = CropCandidate {
            candidate_id: ContentDigest::sha256(
                format!("{}|{}|stage-11", slot.slot_key, domain_id.0).as_bytes(),
            ),
            source_id: domain.prepared_source_digest.clone(),
            domain_id: domain_id.clone(),
            slot_id: region.id,
            crop: Some(SourceCrop {
                x: 0,
                y: 0,
                width: domain.width,
                height: domain.height,
            }),
            transform: CandidateTransform {
                rotation: hot_trimmer_domain::QuarterTurn::Zero,
                mirror: MirrorTransform::None,
            },
            isotropic_scale: 1.0,
            mapping_mode: mode,
            family: match mode {
                SamplingMode::RepeatX => CandidateFamily::RepeatXSegment,
                SamplingMode::RepeatY => CandidateFamily::RepeatYSegment,
                _ => CandidateFamily::PanelSeamlessTile,
            },
            route: CandidateRoute::Repeat,
            position_strategy: PositionStrategy::PeriodAligned,
            period_pixels: Some(period),
            seam_indices: Vec::new(),
            correspondence_reference: domain_id.clone(),
            descriptors: CandidateDescriptors {
                saliency_milli: 0,
                stationarity_milli: 1000,
                feature_strength_milli: 0,
                usability_milli: 1000,
            },
            seed: 14,
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
                reasons: vec!["selected by the bounded Stage 14 preview route".into()],
            },
        };
        plans.push(SamplingPlan {
            slot_id: region.id,
            role: region.role,
            variation_group: region.material_group.clone(),
            prepared_domain_dimensions: [domain.width, domain.height],
            candidate,
            sampling_basis: hot_trimmer_placement_solver::SamplingBasis::SelectedCrop,
            slot_physical_size: [
                f64::from(slot.allocation.width),
                f64::from(slot.allocation.height),
            ],
            source_pixels_per_physical_unit: 1.0,
            sampling_policy: SamplingPolicy {
                filter: SourceSamplingMode::Linear,
                scale: 1.0,
                correct_tangent_normals: true,
            },
            stretch_override: StretchOverrideProvenance::NotAuthorized,
            slice_geometry: SliceGeometry::None,
            maximum_seam_cost_milli: 450,
            unary_cost: 0.0,
        });
    }
    let placement = PlacementPlan {
        stage_result: StageResult::Executed {
            algorithm: AlgorithmProvenance {
                algorithm_id: hot_trimmer_placement_solver::STAGE_13_ALGORITHM_ID.into(),
                version: hot_trimmer_placement_solver::STAGE_13_ALGORITHM_VERSION.into(),
            },
            settings_hash: ContentDigest::sha256(b"stage-14-preview-a-selected-placement"),
            diagnostics: Vec::new(),
        },
        solver: AlgorithmProvenance {
            algorithm_id: hot_trimmer_placement_solver::STAGE_13_ALGORITHM_ID.into(),
            version: hot_trimmer_placement_solver::STAGE_13_ALGORITHM_VERSION.into(),
        },
        seed: 14,
        placements: plans.clone(),
        objective: PlacementObjectiveBreakdown {
            unary_cost: 0.0,
            pairwise_cost: 0.0,
            pairwise_lambda: 1.0,
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
            slot_count: u32::try_from(plans.len()).unwrap_or(u32::MAX),
        },
        qa_views: vec![
            PlacementPlanQaView::SelectedPlacements,
            PlacementPlanQaView::Validation,
        ],
    };
    let render_cancellation = RenderCancellationToken::new();
    let mut results = Vec::new();
    for (slot, plan) in topology.slots.iter().zip(&plans) {
        if preview_service.latest_draft_id.load(Ordering::Acquire) != job {
            return Err(error(
                ErrorCode::OperationCancelled,
                "A newer Stage 14 preview superseded this request.",
            ));
        }
        results.push(
            synthesize_slot_material(
                SlotSynthesisRequest {
                    plan,
                    domain: &domain,
                    output_dimensions: [slot.allocation.width, slot.allocation.height],
                    limits: SlotSynthesisLimits::default(),
                },
                &render_cancellation,
            )
            .map_err(|failure| {
                error(
                    ErrorCode::InvalidInput,
                    &format!("Required Stage 14 slot failed: {failure}"),
                )
            })?,
        );
    }
    let patch_id = summary
        .patches
        .iter()
        .find(|patch| {
            summary.sources.iter().any(|source| {
                source.input.id == patch.source_id
                    && source.source_set_id.to_string() == primary.to_string()
            })
        })
        .map(|patch| patch.id.to_string());
    let slot_inputs = topology
        .slots
        .iter()
        .zip(document.topology.regions.iter())
        .zip(plans.iter().zip(results.iter()))
        .map(|((slot, region), (plan, result))| IntermediateSlotInput {
            region_id: region.id,
            slot_key: slot.slot_key.as_str(),
            display_name: region.display_name.as_str(),
            required: true,
            patch_id: patch_id.as_deref(),
            domain: &domain,
            plan,
            result,
            grid_rect: region.grid_rect,
            behavior: document.region_bindings[&region.id]
                .mapping
                .behavior
                .clone(),
        })
        .collect();
    let algorithms = (1..=14)
        .map(|stage| {
            (
                stage,
                AlgorithmProvenance {
                    algorithm_id: format!("installed-stage-{stage:02}"),
                    version: if stage == 14 {
                        hot_trimmer_sheet_compiler::STAGE_14_ALGORITHM_VERSION.into()
                    } else {
                        "installed".into()
                    },
                },
            )
        })
        .collect();
    let atlas_request = IntermediateAtlasRequest {
        topology: &topology,
        placement_plan: &placement,
        slots: slot_inputs,
        revision: requested_revision,
        algorithm_versions: algorithms,
        diagnostics: Vec::new(),
        regions: Vec::new(),
    };
    let cancellation = EngineCancellationToken::new();
    let artifact = AlgorithmCompiler::new()
        .compile_intermediate_atlas(&atlas_request, &cancellation, || {
            session
                .lock()
                .ok()
                .and_then(|guard| {
                    guard
                        .store
                        .as_ref()?
                        .summary()
                        .ok()?
                        .document
                        .map(|value| value.document_revision)
                })
                .unwrap_or(0)
        })
        .map_err(|failure| {
            preview_service
                .cancellation_count
                .fetch_add(1, Ordering::AcqRel);
            error(ErrorCode::OperationCancelled, &failure.to_string())
        })?;
    let mut maps = BTreeMap::new();
    for channel in &artifact.channels {
        maps.insert(
            channel_key(channel.role).into(),
            png_data_url(
                topology.output_size.width,
                topology.output_size.height,
                channel.rgba8.clone(),
            )?,
        );
    }
    let slots = artifact
        .slots
        .iter()
        .filter_map(|slot| {
            resolved
                .regions
                .iter()
                .find(|region| region.region_id == slot.region_id)
                .map(|region| Stage14SlotProjection {
                    region_id: region.region_id,
                    slot_key: slot.slot_key.clone(),
                    display_name: slot.display_name.clone(),
                    allocation_bounds: slot.allocation,
                    hotspot_bounds: slot.hotspot,
                    mapping_mode: format!("{:?}", slot.mapping_mode),
                    source_transform: slot.source_transform,
                    isotropic_scale: slot.isotropic_scale,
                    sampling_scale: slot.sampling_scale,
                    validity: format!("{} valid pixels", slot.valid_pixel_count),
                    correspondence: "authoritative Stage 14 correspondence".into(),
                    source_id: slot.source_id.0.clone(),
                    patch_id: slot.patch_id.clone(),
                    domain_id: slot.domain_id.0.clone(),
                    candidate_id: slot.candidate_id.0.clone(),
                    sampling_plan_id: slot.sampling_plan_id.0.clone(),
                    stage_14_result_id: slot.stage_14_result_id.0.clone(),
                    source_crop: slot.source_crop,
                    source_bounds: region.source_bounds,
                    mapping_origin: region.mapping_origin,
                    grid_rect: slot.grid_rect,
                    behavior_version: slot.behavior_version,
                    role: slot.role,
                    continuity: slot.continuity,
                    requested_sampling: slot.requested_sampling,
                    executed_mode: format!("{:?}", slot.executed_mode),
                    edge_eligibility: slot.edge_eligibility,
                    period_pixels: slot.period_pixels,
                    address_mode: slot.address_mode.into(),
                })
        })
        .collect();
    Ok(IntermediateAtlasProjection {
        label: artifact.label,
        non_exportable: true,
        incomplete_after_stage: 14,
        revision: artifact.revision,
        document_revision: artifact.revision,
        topology_hash: hash_hex(document.topology.topology_hash),
        appearance_hash: hash_hex(
            document
                .appearance_hash()
                .map_err(|failure| error(ErrorCode::LayoutInvalid, &failure.to_string()))?,
        ),
        renderer_version: "intermediate-stage-14",
        width: topology.output_size.width,
        height: topology.output_size.height,
        topology: artifact.topology,
        placement_plan_id: artifact.placement_plan_id.0,
        maps,
        regions: resolved.regions,
        unavailable_channels: artifact
            .unavailable_channels
            .iter()
            .map(|role| format!("{role:?}"))
            .collect(),
        slots,
        pending: artifact.pending,
        telemetry: artifact.telemetry,
        final_compile_available: false,
        export_available: false,
        blender_available: false,
        source_frame: document.source_frame.clone(),
    })
}

fn srgb_to_linear(value: u8) -> f32 {
    let value = f32::from(value) / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn refreshed_stage_14_project_summary(
    session: &SharedProjectSession,
    preview_service: &PreviewService,
) -> Result<(ProjectSummary, bool), UserFacingError> {
    let mut guard = session.lock().map_err(|_| poisoned())?;
    let refreshed = {
        let store = guard.store.as_mut().ok_or_else(no_project)?;
        store.refresh_document_assets().map_err(store_error)?
    };
    if refreshed {
        preview_service.clear_cached_outputs();
        guard.mark_mutated();
    }
    let summary = guard
        .store
        .as_ref()
        .ok_or_else(no_project)?
        .summary()
        .map_err(store_error)?;
    Ok((summary, refreshed))
}

fn export_stage_14_material_maps_impl(
    session: &SharedProjectSession,
    preview_service: &PreviewService,
    request: NativeStage14ExportRequest,
    app: &AppHandle,
) -> Result<NativeStage14ExportProjection, UserFacingError> {
    let final_path = PathBuf::from(&request.path);
    if final_path.as_os_str().is_empty() {
        return Err(error(
            ErrorCode::InvalidInput,
            "Choose an export destination.",
        ));
    }
    let (mut summary, refreshed_document_assets) =
        refreshed_stage_14_project_summary(session, preview_service)?;
    let document = summary
        .document
        .as_ref()
        .ok_or_else(|| error(ErrorCode::LayoutInvalid, "Create a trim sheet first."))?
        .clone();
    let accepted_revision = document.document_revision;
    let accepted_refreshed_revision =
        refreshed_document_assets && accepted_revision == request.revision.saturating_add(1);
    if accepted_revision != request.revision && !accepted_refreshed_revision {
        return Err(error(
            ErrorCode::OperationCancelled,
            "A newer document revision superseded this export.",
        ));
    }
    let requested_maps = if request.requested_maps.is_empty() {
        native_export_enabled_maps(&document)
    } else {
        request.requested_maps
    };
    let mut unique_maps = Vec::with_capacity(requested_maps.len());
    for map in requested_maps {
        if !unique_maps.contains(&map) {
            unique_maps.push(map);
        }
    }
    if unique_maps.is_empty() {
        unique_maps.push(MaterialMapKind::BaseColor);
    }
    summary.document = Some(document.clone());
    let full_rect = OutputPixelRect(PixelBounds {
        x: 0,
        y: 0,
        width: document.render_settings.output_size.width,
        height: document.render_settings.output_size.height,
    });
    let gpu = preview_service
        .gpu_capabilities
        .initialize()
        .map_err(|failure| error(ErrorCode::LayoutInvalid, &failure.to_string()))?;
    let tile_edge = native_export_tile_edge(full_rect, gpu.capabilities()).map_err(export_error)?;
    let planned_tiles = native_export_planned_tiles(full_rect, tile_edge, &unique_maps);
    let job = preview_service
        .latest_draft_id
        .fetch_add(1, Ordering::AcqRel)
        .saturating_add(1);
    let revision_current = AtomicBool::new(true);
    let monitoring_complete = AtomicBool::new(false);
    let cancellation = EngineCancellationToken::new();
    let revisions = RevisionAuthority::new(accepted_revision);
    let export_token = EngineCancellationToken::new();
    let mut telemetry = Vec::new();
    let package = std::thread::scope(|scope| {
        scope.spawn(|| {
            while !monitoring_complete.load(Ordering::Acquire) {
                let live = session
                    .lock()
                    .ok()
                    .and_then(|guard| {
                        guard
                            .store
                            .as_ref()?
                            .summary()
                            .ok()?
                            .document
                            .map(|document| document.document_revision == accepted_revision)
                    })
                    .unwrap_or(false);
                if !live {
                    revision_current.store(false, Ordering::Release);
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        let result = (|| {
            let mut writer = begin_native_stage14_export_package(
                &final_path,
                accepted_revision,
                revisions.clone(),
                &export_token,
                &document,
                full_rect,
                &unique_maps,
                &planned_tiles,
            )?;
            let gpu_executor = hot_trimmer_sheet_compiler::GpuAtlasRenderExecutor {
                service: &preview_service.gpu_capabilities,
                source_texture_cache: &preview_service.gpu_source_cache,
            };
            let current = || {
                let live = preview_service.latest_draft_id.load(Ordering::Acquire) == job
                    && revision_current.load(Ordering::Acquire);
                if !live {
                    revisions.supersede_with(accepted_revision.saturating_add(1));
                    export_token.cancel();
                    cancellation.cancel();
                }
                live
            };
            for planned_rect in native_export_unique_rects(&planned_tiles) {
                ensure_native_export_current(&export_token, &current)?;
                let artifact = AlgorithmCompiler::new()
                    .compile_persisted_stage_14_preview_with_cache_and_executor(
                        hot_trimmer_sheet_compiler::PersistedStage14PreviewRequest {
                            project: &summary,
                            revision: accepted_revision,
                            draft_id: Some(job),
                            input_hash: Some(format!(
                                "native-export:{}:{}:{}:{}",
                                planned_rect.0.x,
                                planned_rect.0.y,
                                planned_rect.0.width,
                                planned_rect.0.height
                            )),
                            profile:
                                hot_trimmer_sheet_compiler::SourceFramePreviewProfile::Authoritative,
                            view_intent: Some(
                                hot_trimmer_sheet_compiler::SourceFramePreviewViewIntent::ExactViewportMaterialMaps {
                                    rect: planned_rect,
                                    maps: unique_maps.clone(),
                                },
                            ),
                        },
                        &cancellation,
                        || {
                            preview_service.latest_draft_id.load(Ordering::Acquire) == job
                                && revision_current.load(Ordering::Acquire)
                        },
                        Some(&preview_service.source_frame_cache),
                        Some(&gpu_executor),
                    )
                    .map_err(|failure| TiledExportError::GpuValidation(failure.to_string()))?;
                telemetry.extend(artifact.telemetry);
                for map in &unique_maps {
                    let planned = planned_tiles
                        .iter()
                        .find(|tile| tile.map == *map && tile.rect == planned_rect)
                        .ok_or_else(|| {
                            TiledExportError::Encoder(format!(
                                "missing planned native export tile for {map:?}"
                            ))
                        })?;
                    let tile = artifact.rendered_tiles.get(map).ok_or_else(|| {
                        TiledExportError::Readback(format!(
                            "The GPU export did not publish the requested {map:?} map."
                        ))
                    })?;
                    let timing = artifact
                        .rendered_tile_timings
                        .get(map)
                        .copied()
                        .unwrap_or_default();
                    let progress = writer.write_tile(
                        &planned.id,
                        NativeExportTile {
                            map: *map,
                            manifest: &tile.manifest,
                            pixels: tile.pixels(),
                            render_ms: timing.render_ms,
                            readback_ms: timing.readback_ms,
                        },
                        &export_token,
                        &current,
                    )?;
                    emit_native_stage14_export_progress(app, accepted_revision, &progress);
                }
            }
            let mut emit_progress = |progress: &ExportProgress| {
                emit_native_stage14_export_progress(app, accepted_revision, progress);
            };
            writer.finish(&export_token, &current, &document, &mut emit_progress)
        })();
        monitoring_complete.store(true, Ordering::Release);
        result
    })
    .map_err(export_error)?;
    let output_count = package.outputs.len();
    let refreshed_project = if refreshed_document_assets {
        let guard = session.lock().map_err(|_| poisoned())?;
        Some(project_projection(&guard)?)
    } else {
        None
    };
    Ok(NativeStage14ExportProjection {
        path: final_path.display().to_string(),
        revision: accepted_revision,
        bytes_written: package.bytes_written,
        outputs: package.outputs,
        progress: package.progress,
        telemetry: telemetry
            .into_iter()
            .chain([format!(
                "native_export_bytes={}; native_export_outputs={}; native_export_tile_edge={}; native_export_format=hot-trimmer-package-v1",
                package.bytes_written, output_count, tile_edge
            )])
            .collect(),
        project: refreshed_project,
    })
}

struct NativeExportTile<'a> {
    map: MaterialMapKind,
    manifest: &'a CompiledAtlasTileManifest,
    pixels: &'a [u8],
    render_ms: u128,
    readback_ms: u128,
}

#[derive(Clone, Debug)]
struct NativeExportPlannedTile {
    id: String,
    map: MaterialMapKind,
    rect: OutputPixelRect,
}

#[derive(Debug)]
struct NativeExportPackageResult {
    bytes_written: u64,
    outputs: Vec<NativeStage14ExportOutput>,
    progress: Vec<ExportProgress>,
}

struct NativeExportPackageWriter {
    final_path: PathBuf,
    temporary_path: PathBuf,
    revisions: RevisionAuthority,
    expected_revision: u64,
    maps: BTreeMap<MaterialMapKind, NativeExportMapWriter>,
    progress: Vec<ExportProgress>,
    total_tiles: u32,
    completed_tiles: u32,
    finalized: bool,
}

struct NativeExportPackageInitGuard {
    path: PathBuf,
    active: bool,
}

#[derive(Clone, Copy, Debug)]
struct NativeExportPixelLayout {
    bit_depth: u8,
    source_bytes_per_pixel: usize,
    output_bytes_per_pixel: usize,
    png_color_type: PngColorType,
    png_bit_depth: PngBitDepth,
    pixel_format: &'static str,
}

struct NativeExportMapWriter {
    map: MaterialMapKind,
    file_name: String,
    spool_path: PathBuf,
    spool: Option<File>,
    width: u32,
    height: u32,
    layout: NativeExportPixelLayout,
    color_space: String,
    region_id_palette: BTreeMap<u32, [u8; 4]>,
}

fn native_export_enabled_maps(document: &TrimSheetDocument) -> Vec<MaterialMapKind> {
    let ordered = [
        (Channel::BaseColor, MaterialMapKind::BaseColor),
        (Channel::Height, MaterialMapKind::Height),
        (Channel::Normal, MaterialMapKind::Normal),
        (Channel::Roughness, MaterialMapKind::Roughness),
        (Channel::Metallic, MaterialMapKind::Metallic),
        (Channel::AmbientOcclusion, MaterialMapKind::AmbientOcclusion),
        (Channel::RegionId, MaterialMapKind::RegionId),
    ];
    let requested = ordered
        .into_iter()
        .filter_map(|(channel, map)| {
            document
                .render_settings
                .channels
                .get(&channel)
                .is_some_and(|policy| policy.enabled)
                .then_some(map)
        })
        .collect::<Vec<_>>();
    if requested.is_empty() {
        vec![MaterialMapKind::BaseColor]
    } else {
        requested
    }
}

fn native_export_tile_edge(
    full_rect: OutputPixelRect,
    caps: &hot_trimmer_preview::GpuCapabilityRecord,
) -> Result<u32, TiledExportError> {
    let budgets = ExportMemoryBudgets::default();
    let concurrency = budgets.total_in_flight_tiles.max(1);
    choose_bounded_tile_edge(
        full_rect.0.width.max(full_rect.0.height),
        caps.maximum_texture_dimension_2d,
        4,
        2,
        budgets
            .gpu_output_intermediate_residency_bytes
            .min(budgets.staging_buffers_bytes)
            / u64::from(concurrency),
        caps.copy_bytes_per_row_alignment,
    )
}

fn native_export_planned_tiles(
    full_rect: OutputPixelRect,
    tile_edge: u32,
    maps: &[MaterialMapKind],
) -> Vec<NativeExportPlannedTile> {
    let mut planned = Vec::new();
    let tile_edge = tile_edge.max(1);
    let right = full_rect.0.x.saturating_add(full_rect.0.width);
    let bottom = full_rect.0.y.saturating_add(full_rect.0.height);
    let mut y = full_rect.0.y;
    while y < bottom {
        let height = tile_edge.min(bottom - y);
        let mut x = full_rect.0.x;
        while x < right {
            let width = tile_edge.min(right - x);
            let rect = OutputPixelRect(PixelBounds {
                x,
                y,
                width,
                height,
            });
            for map in maps {
                planned.push(NativeExportPlannedTile {
                    id: native_export_output_id(*map, rect),
                    map: *map,
                    rect,
                });
            }
            x = x.saturating_add(width);
        }
        y = y.saturating_add(height);
    }
    planned
}

fn native_export_unique_rects(planned_tiles: &[NativeExportPlannedTile]) -> Vec<OutputPixelRect> {
    let mut rects = Vec::new();
    for tile in planned_tiles {
        if !rects.contains(&tile.rect) {
            rects.push(tile.rect);
        }
    }
    rects
}

fn native_export_output_id(map: MaterialMapKind, rect: OutputPixelRect) -> String {
    format!(
        "{}@{},{}-{}x{}",
        material_map_view_key(map),
        rect.0.x,
        rect.0.y,
        rect.0.width,
        rect.0.height
    )
}

fn begin_native_stage14_export_package(
    final_path: &Path,
    revision: u64,
    revisions: RevisionAuthority,
    cancellation: &EngineCancellationToken,
    document: &TrimSheetDocument,
    full_rect: OutputPixelRect,
    maps: &[MaterialMapKind],
    planned_tiles: &[NativeExportPlannedTile],
) -> Result<NativeExportPackageWriter, TiledExportError> {
    if planned_tiles.is_empty() {
        return Err(TiledExportError::InvalidRequest(
            "native Stage 14 export requires at least one GPU map tile".into(),
        ));
    }
    let temporary_path = native_export_temporary_package_path(final_path);
    let _ = fs::remove_dir_all(&temporary_path);
    fs::create_dir_all(temporary_path.join("maps"))?;
    let mut init_guard = NativeExportPackageInitGuard {
        path: temporary_path,
        active: true,
    };
    let region_id_palette = native_export_region_id_palette(document);
    let mut map_buffers = BTreeMap::new();
    for map in maps {
        map_buffers.insert(
            *map,
            NativeExportMapWriter::new(
                *map,
                document,
                &init_guard.path,
                full_rect.0.width,
                full_rect.0.height,
                &region_id_palette,
            )?,
        );
    }
    if cancellation.is_cancelled() {
        return Err(TiledExportError::Cancelled);
    }
    let temporary_path = init_guard.disarm();
    Ok(NativeExportPackageWriter {
        final_path: final_path.to_path_buf(),
        temporary_path,
        revisions,
        expected_revision: revision,
        maps: map_buffers,
        progress: Vec::new(),
        total_tiles: planned_tiles.len() as u32,
        completed_tiles: 0,
        finalized: false,
    })
}

impl NativeExportPackageInitGuard {
    fn disarm(&mut self) -> PathBuf {
        self.active = false;
        self.path.clone()
    }
}

impl Drop for NativeExportPackageInitGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn emit_native_stage14_export_progress(app: &AppHandle, revision: u64, progress: &ExportProgress) {
    let _ = app.emit(
        "stage-14-export-progress",
        NativeStage14ExportProgressEvent {
            revision,
            progress: progress.clone(),
        },
    );
}

impl NativeExportPackageWriter {
    fn write_tile(
        &mut self,
        output_id: &str,
        tile: NativeExportTile<'_>,
        cancellation: &EngineCancellationToken,
        is_current: &impl Fn() -> bool,
    ) -> Result<ExportProgress, TiledExportError> {
        ensure_native_export_current(cancellation, is_current)?;
        let map = self.maps.get_mut(&tile.map).ok_or_else(|| {
            TiledExportError::Encoder(format!(
                "missing final map buffer for native export tile {output_id}"
            ))
        })?;
        map.blit_valid_tile(&tile)?;
        self.completed_tiles = self.completed_tiles.saturating_add(1);
        let progress = ExportProgress {
            map: material_map_view_key(tile.map).to_owned(),
            mip_level: tile.manifest.mip_level,
            completed_tiles: self.completed_tiles,
            total_tiles: self.total_tiles,
            render_ms: tile.render_ms,
            readback_ms: tile.readback_ms,
            encode_ms: 0,
            bytes_written: 0,
            estimated_remaining_tiles: self.total_tiles.saturating_sub(self.completed_tiles),
        };
        self.progress.push(progress.clone());
        Ok(progress)
    }

    fn finish(
        mut self,
        cancellation: &EngineCancellationToken,
        is_current: &impl Fn() -> bool,
        document: &TrimSheetDocument,
        progress_sink: &mut dyn FnMut(&ExportProgress),
    ) -> Result<NativeExportPackageResult, TiledExportError> {
        ensure_native_export_current(cancellation, is_current)?;
        if self.revisions.current() != self.expected_revision {
            return Err(TiledExportError::StaleRevision);
        }
        let mut outputs = Vec::new();
        let mut records = BTreeMap::new();
        let mut bytes_written = 0_u64;
        for map in self.maps.values_mut() {
            ensure_native_export_current(cancellation, is_current)?;
            let finalize_started = Instant::now();
            let file_path = Path::new("maps").join(&map.file_name);
            let absolute_path = self.temporary_path.join(&file_path);
            map.encode_png_to_file(&absolute_path, cancellation, is_current)?;
            map.close_and_remove_spool();
            let (encoded_bytes, checksum) = checksum_file(&absolute_path)?;
            bytes_written = bytes_written.saturating_add(encoded_bytes);
            let progress = ExportProgress {
                map: material_map_view_key(map.map).to_owned(),
                mip_level: 0,
                completed_tiles: self.total_tiles,
                total_tiles: self.total_tiles,
                render_ms: 0,
                readback_ms: 0,
                encode_ms: finalize_started.elapsed().as_millis(),
                bytes_written: encoded_bytes,
                estimated_remaining_tiles: 0,
            };
            progress_sink(&progress);
            self.progress.push(progress);
            let relative_path = file_path.to_string_lossy().replace('\\', "/");
            records.insert(
                material_map_view_key(map.map).to_owned(),
                MapRecord {
                    role: material_map_view_key(map.map).to_owned(),
                    relative_path: relative_path.clone(),
                    dimensions: [map.width, map.height],
                    bit_depth: map.layout.bit_depth,
                    color_space: map.color_space.clone(),
                    checksum: checksum.clone(),
                },
            );
            outputs.push(NativeStage14ExportOutput {
                id: material_map_view_key(map.map).to_owned(),
                map: material_map_view_key(map.map).to_owned(),
                file_name: relative_path,
                checksum,
                bytes: encoded_bytes,
                width: map.width,
                height: map.height,
                pixel_format: map.layout.pixel_format.to_owned(),
            });
        }
        let manifest = native_export_manifest_from_document(document, records)?;
        write_package_manifest(&self.temporary_path, &manifest)
            .map_err(|failure| TiledExportError::Encoder(failure.to_string()))?;
        bytes_written = bytes_written.saturating_add(
            fs::metadata(self.temporary_path.join(HOTTRIM_MANIFEST_FILE_NAME))?.len(),
        );
        ensure_native_export_current(cancellation, is_current)?;
        if self.revisions.current() != self.expected_revision {
            return Err(TiledExportError::StaleRevision);
        }
        if self.final_path.exists() {
            return Err(TiledExportError::InvalidRequest(format!(
                "export package destination already exists: {}",
                self.final_path.display()
            )));
        }
        fs::rename(&self.temporary_path, &self.final_path)?;
        self.finalized = true;
        let progress = std::mem::take(&mut self.progress);
        Ok(NativeExportPackageResult {
            bytes_written,
            outputs,
            progress,
        })
    }
}

impl Drop for NativeExportPackageWriter {
    fn drop(&mut self) {
        if !self.finalized {
            let _ = fs::remove_dir_all(&self.temporary_path);
        }
    }
}

#[cfg(test)]
fn write_native_stage14_export_package(
    final_path: &Path,
    revision: u64,
    revisions: RevisionAuthority,
    cancellation: &EngineCancellationToken,
    document: &TrimSheetDocument,
    tiles: Vec<NativeExportTile<'_>>,
    is_current: impl Fn() -> bool,
) -> Result<NativeExportPackageResult, TiledExportError> {
    let planned_tiles = tiles
        .iter()
        .map(|tile| NativeExportPlannedTile {
            id: native_export_output_id(tile.map, tile.manifest.valid_rect),
            map: tile.map,
            rect: tile.manifest.valid_rect,
        })
        .collect::<Vec<_>>();
    let full_rect = OutputPixelRect(PixelBounds {
        x: 0,
        y: 0,
        width: document.render_settings.output_size.width,
        height: document.render_settings.output_size.height,
    });
    let maps = tiles.iter().map(|tile| tile.map).collect::<Vec<_>>();
    let mut writer = begin_native_stage14_export_package(
        final_path,
        revision,
        revisions,
        cancellation,
        document,
        full_rect,
        &maps,
        &planned_tiles,
    )?;
    ensure_native_export_current(cancellation, &is_current)?;
    for (tile, planned) in tiles.into_iter().zip(planned_tiles.iter()) {
        writer.write_tile(&planned.id, tile, cancellation, &is_current)?;
    }
    let mut ignore_progress = |_progress: &ExportProgress| {};
    writer.finish(cancellation, &is_current, document, &mut ignore_progress)
}

impl NativeExportMapWriter {
    fn new(
        map: MaterialMapKind,
        document: &TrimSheetDocument,
        package_path: &Path,
        width: u32,
        height: u32,
        region_id_palette: &BTreeMap<u32, [u8; 4]>,
    ) -> Result<Self, TiledExportError> {
        let layout = native_export_pixel_layout(document, map)?;
        let len = usize::try_from(
            u64::from(width)
                .saturating_mul(u64::from(height))
                .saturating_mul(layout.output_bytes_per_pixel as u64),
        )
        .map_err(|_| TiledExportError::OutOfMemory("final map spool is too large".into()))?;
        let spool_path = package_path.join(format!(".{}.raw", native_export_map_file_stem(map)));
        let spool = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&spool_path)?;
        spool.set_len(len as u64)?;
        Ok(Self {
            map,
            file_name: native_export_map_file_name(map),
            spool_path,
            spool: Some(spool),
            width,
            height,
            layout,
            color_space: native_export_color_space(map).to_owned(),
            region_id_palette: region_id_palette.clone(),
        })
    }

    fn blit_valid_tile(&mut self, tile: &NativeExportTile<'_>) -> Result<u64, TiledExportError> {
        let valid = tile.manifest.valid_rect.0;
        let output = tile.manifest.output_rect.0;
        if valid.x.saturating_add(valid.width) > self.width
            || valid.y.saturating_add(valid.height) > self.height
            || valid.x < output.x
            || valid.y < output.y
        {
            return Err(TiledExportError::Encoder(format!(
                "tile {} has invalid export bounds",
                native_export_output_id(tile.map, tile.manifest.valid_rect)
            )));
        }
        let src_x = valid.x - output.x;
        let src_y = valid.y - output.y;
        let row_stride = usize::try_from(tile.manifest.row_stride)
            .map_err(|_| TiledExportError::Encoder("tile row stride is too large".into()))?;
        let expected = row_stride
            .checked_mul(
                usize::try_from(tile.manifest.height)
                    .map_err(|_| TiledExportError::Encoder("tile height is too large".into()))?,
            )
            .ok_or_else(|| TiledExportError::Encoder("tile payload size overflows".into()))?;
        if tile.pixels.len() != expected {
            return Err(TiledExportError::Encoder(
                "tile payload does not match its manifest dimensions".into(),
            ));
        }
        if tile.manifest.pixel_format != native_export_source_pixel_format(self.map) {
            return Err(TiledExportError::Encoder(format!(
                "tile {:?} has pixel format {:?}, but native export expects {:?}",
                self.map,
                tile.manifest.pixel_format,
                native_export_source_pixel_format(self.map)
            )));
        }
        let row_len = usize::try_from(
            u64::from(valid.width).saturating_mul(self.layout.output_bytes_per_pixel as u64),
        )
        .map_err(|_| TiledExportError::Encoder("final map row size overflows".into()))?;
        let mut row = vec![0; row_len];
        for y in 0..valid.height {
            row.fill(0);
            for x in 0..valid.width {
                let source_index = usize::try_from(
                    u64::from(src_y + y)
                        .saturating_mul(u64::from(tile.manifest.row_stride))
                        .saturating_add(
                            u64::from(src_x + x)
                                .saturating_mul(self.layout.source_bytes_per_pixel as u64),
                        ),
                )
                .map_err(|_| TiledExportError::Encoder("tile source offset overflows".into()))?;
                let destination_index = usize::try_from(
                    u64::from(x).saturating_mul(self.layout.output_bytes_per_pixel as u64),
                )
                .map_err(|_| TiledExportError::Encoder("final map row offset overflows".into()))?;
                let source_end = source_index
                    .checked_add(self.layout.source_bytes_per_pixel)
                    .ok_or_else(|| {
                        TiledExportError::Encoder("tile source offset overflows".into())
                    })?;
                self.write_converted_pixel(
                    &tile.pixels[source_index..source_end],
                    &mut row
                        [destination_index..destination_index + self.layout.output_bytes_per_pixel],
                )?;
            }
            let destination_row_offset = u64::from(valid.y + y)
                .saturating_mul(u64::from(self.width))
                .saturating_add(u64::from(valid.x))
                .saturating_mul(self.layout.output_bytes_per_pixel as u64);
            let spool = self.spool_mut()?;
            spool.seek(SeekFrom::Start(destination_row_offset))?;
            spool.write_all(&row)?;
        }
        Ok(u64::from(valid.width)
            .saturating_mul(u64::from(valid.height))
            .saturating_mul(self.layout.output_bytes_per_pixel as u64))
    }

    fn write_converted_pixel(
        &self,
        source: &[u8],
        destination: &mut [u8],
    ) -> Result<(), TiledExportError> {
        match self.map {
            MaterialMapKind::BaseColor | MaterialMapKind::Normal => {
                destination.copy_from_slice(source);
            }
            MaterialMapKind::RegionId => {
                let compact_index = u32::from_le_bytes(source.try_into().map_err(|_| {
                    TiledExportError::Encoder("region ID source sample is malformed".into())
                })?);
                let color = if compact_index == u32::MAX {
                    [0, 0, 0, 0]
                } else {
                    self.region_id_palette
                        .get(&compact_index)
                        .copied()
                        .unwrap_or([0, 0, 0, 0])
                };
                destination.copy_from_slice(&color);
            }
            MaterialMapKind::Height
            | MaterialMapKind::Roughness
            | MaterialMapKind::Metallic
            | MaterialMapKind::AmbientOcclusion => {
                let value = f32::from_le_bytes(source.try_into().map_err(|_| {
                    TiledExportError::Encoder("scalar source sample is malformed".into())
                })?);
                if self.layout.bit_depth == 8 {
                    destination[0] = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
                } else {
                    let encoded = (value.clamp(0.0, 1.0) * 65535.0).round() as u16;
                    destination.copy_from_slice(&encoded.to_be_bytes());
                }
            }
            MaterialMapKind::Specular
            | MaterialMapKind::Opacity
            | MaterialMapKind::EdgeMask
            | MaterialMapKind::MaterialId => {
                return Err(TiledExportError::UnsupportedFeatureOrFormat(format!(
                    "unsupported native export conversion for {:?}",
                    self.map
                )));
            }
        }
        Ok(())
    }

    fn encode_png_to_file(
        &mut self,
        path: &Path,
        cancellation: &EngineCancellationToken,
        is_current: &impl Fn() -> bool,
    ) -> Result<(), TiledExportError> {
        let spool = self.spool_mut()?;
        spool.flush()?;
        spool.seek(SeekFrom::Start(0))?;
        let file = File::create(path)?;
        let output = BufWriter::new(file);
        let mut encoder = PngStreamEncoder::new(output, self.width, self.height);
        encoder.set_color(self.layout.png_color_type);
        encoder.set_depth(self.layout.png_bit_depth);
        encoder.set_compression(PngCompression::Fast);
        encoder.set_filter(PngFilter::Sub);
        let mut header = encoder
            .write_header()
            .map_err(|failure| TiledExportError::Encoder(failure.to_string()))?;
        let mut writer = header
            .stream_writer()
            .map_err(|failure| TiledExportError::Encoder(failure.to_string()))?;
        let row_len = usize::try_from(
            u64::from(self.width).saturating_mul(self.layout.output_bytes_per_pixel as u64),
        )
        .map_err(|_| TiledExportError::Encoder("final PNG row size overflows".into()))?;
        let mut row = vec![0; row_len];
        for _ in 0..self.height {
            ensure_native_export_current(cancellation, is_current)?;
            self.spool_mut()?.read_exact(&mut row)?;
            writer
                .write_all(&row)
                .map_err(|failure| TiledExportError::Encoder(failure.to_string()))?;
        }
        ensure_native_export_current(cancellation, is_current)?;
        writer
            .finish()
            .map_err(|failure| TiledExportError::Encoder(failure.to_string()))?;
        Ok(())
    }

    fn close_and_remove_spool(&mut self) {
        let _ = self.spool.take();
        let _ = fs::remove_file(&self.spool_path);
    }

    fn spool_mut(&mut self) -> Result<&mut File, TiledExportError> {
        self.spool.as_mut().ok_or_else(|| {
            TiledExportError::Encoder(format!(
                "native export spool for {:?} is already closed",
                self.map
            ))
        })
    }
}

impl Drop for NativeExportMapWriter {
    fn drop(&mut self) {
        let _ = self.spool.take();
        let _ = fs::remove_file(&self.spool_path);
    }
}

fn native_export_pixel_layout(
    document: &TrimSheetDocument,
    map: MaterialMapKind,
) -> Result<NativeExportPixelLayout, TiledExportError> {
    let bit_depth = native_export_bit_depth(document, map)?;
    let layout = match map {
        MaterialMapKind::BaseColor => NativeExportPixelLayout {
            bit_depth: 8,
            source_bytes_per_pixel: 4,
            output_bytes_per_pixel: 4,
            png_color_type: PngColorType::Rgba,
            png_bit_depth: PngBitDepth::Eight,
            pixel_format: "PNG RGBA8 sRGB",
        },
        MaterialMapKind::Normal => {
            if bit_depth == 16 {
                return Err(TiledExportError::UnsupportedFeatureOrFormat(
                    "Normal requests 16-bit output, but the current GPU normal pass publishes RGBA8; 16-bit Normal export requires a true 16-bit render/readback target".into(),
                ));
            }
            NativeExportPixelLayout {
                bit_depth: 8,
                source_bytes_per_pixel: 4,
                output_bytes_per_pixel: 4,
                png_color_type: PngColorType::Rgba,
                png_bit_depth: PngBitDepth::Eight,
                pixel_format: "PNG RGBA8 linear",
            }
        }
        MaterialMapKind::RegionId => NativeExportPixelLayout {
            bit_depth: 8,
            source_bytes_per_pixel: 4,
            output_bytes_per_pixel: 4,
            png_color_type: PngColorType::Rgba,
            png_bit_depth: PngBitDepth::Eight,
            pixel_format: "PNG RGBA8 categorical",
        },
        MaterialMapKind::Height
        | MaterialMapKind::Roughness
        | MaterialMapKind::Metallic
        | MaterialMapKind::AmbientOcclusion => {
            if bit_depth == 8 {
                NativeExportPixelLayout {
                    bit_depth: 8,
                    source_bytes_per_pixel: 4,
                    output_bytes_per_pixel: 1,
                    png_color_type: PngColorType::Grayscale,
                    png_bit_depth: PngBitDepth::Eight,
                    pixel_format: "PNG L8 linear",
                }
            } else {
                NativeExportPixelLayout {
                    bit_depth: 16,
                    source_bytes_per_pixel: 4,
                    output_bytes_per_pixel: 2,
                    png_color_type: PngColorType::Grayscale,
                    png_bit_depth: PngBitDepth::Sixteen,
                    pixel_format: "PNG L16 linear",
                }
            }
        }
        MaterialMapKind::Specular
        | MaterialMapKind::Opacity
        | MaterialMapKind::EdgeMask
        | MaterialMapKind::MaterialId => {
            return Err(TiledExportError::UnsupportedFeatureOrFormat(format!(
                "{map:?} is not currently supported by the native GPU export encoder"
            )));
        }
    };
    Ok(layout)
}

fn native_export_source_pixel_format(
    map: MaterialMapKind,
) -> hot_trimmer_sheet_compiler::CompiledTilePixelFormat {
    match map {
        MaterialMapKind::BaseColor => {
            hot_trimmer_sheet_compiler::CompiledTilePixelFormat::Rgba8UnormSrgb
        }
        MaterialMapKind::Height
        | MaterialMapKind::Roughness
        | MaterialMapKind::Metallic
        | MaterialMapKind::AmbientOcclusion => {
            hot_trimmer_sheet_compiler::CompiledTilePixelFormat::R32Float
        }
        MaterialMapKind::RegionId => hot_trimmer_sheet_compiler::CompiledTilePixelFormat::R32Uint,
        _ => hot_trimmer_sheet_compiler::CompiledTilePixelFormat::Rgba8UnormLinear,
    }
}

fn native_export_region_id_palette(document: &TrimSheetDocument) -> BTreeMap<u32, [u8; 4]> {
    document
        .topology
        .regions
        .iter()
        .enumerate()
        .map(|(index, region)| {
            let [red, green, blue] = region.id_color.0;
            (index as u32, [red, green, blue, 255])
        })
        .collect()
}

fn checksum_file(path: &Path) -> Result<(u64, String), TiledExportError> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        total = total.saturating_add(read as u64);
    }
    let digest = hasher.finalize();
    Ok((
        total,
        digest.iter().map(|byte| format!("{byte:02x}")).collect(),
    ))
}

fn native_export_temporary_package_path(final_path: &Path) -> PathBuf {
    let file_name = final_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("export.hottrim");
    final_path.with_file_name(format!(".{file_name}.tmp-{}", Uuid::new_v4()))
}

fn native_export_map_file_name(map: MaterialMapKind) -> String {
    format!("{}.png", native_export_map_file_stem(map))
}

fn native_export_map_file_stem(map: MaterialMapKind) -> &'static str {
    match map {
        MaterialMapKind::BaseColor => "base_color",
        MaterialMapKind::Normal => "normal",
        MaterialMapKind::Height => "height",
        MaterialMapKind::Roughness => "roughness",
        MaterialMapKind::Metallic => "metallic",
        MaterialMapKind::AmbientOcclusion => "ambient_occlusion",
        MaterialMapKind::RegionId => "region_id",
        MaterialMapKind::MaterialId => "material_id",
        MaterialMapKind::Specular => "specular",
        MaterialMapKind::Opacity => "opacity",
        MaterialMapKind::EdgeMask => "edge_mask",
    }
}

fn native_export_color_space(map: MaterialMapKind) -> &'static str {
    match map {
        MaterialMapKind::BaseColor => "sRGB",
        MaterialMapKind::RegionId | MaterialMapKind::MaterialId => "categorical",
        _ => "linear",
    }
}

fn native_export_bit_depth(
    document: &TrimSheetDocument,
    map: MaterialMapKind,
) -> Result<u8, TiledExportError> {
    let channel = match map {
        MaterialMapKind::BaseColor => Channel::BaseColor,
        MaterialMapKind::Normal => Channel::Normal,
        MaterialMapKind::Height => Channel::Height,
        MaterialMapKind::Roughness => Channel::Roughness,
        MaterialMapKind::Metallic => Channel::Metallic,
        MaterialMapKind::AmbientOcclusion => Channel::AmbientOcclusion,
        MaterialMapKind::RegionId => Channel::RegionId,
        MaterialMapKind::MaterialId => Channel::MaterialId,
        MaterialMapKind::Specular | MaterialMapKind::Opacity | MaterialMapKind::EdgeMask => {
            return Ok(8);
        }
    };
    let bit_depth = match document
        .render_settings
        .channels
        .get(&channel)
        .map(|policy| policy.bit_depth)
    {
        Some(ChannelBitDepth::Eight) => 8,
        Some(ChannelBitDepth::ThirtyTwoFloat) => {
            return Err(TiledExportError::UnsupportedFeatureOrFormat(format!(
                "{map:?} requests 32-bit float output, but the native package encoder currently supports PNG 8-bit and 16-bit material maps"
            )));
        }
        Some(ChannelBitDepth::Sixteen) | None => {
            if matches!(map, MaterialMapKind::BaseColor | MaterialMapKind::RegionId) {
                8
            } else {
                16
            }
        }
    };
    Ok(bit_depth)
}

fn native_export_manifest_from_document(
    document: &TrimSheetDocument,
    maps: BTreeMap<String, MapRecord>,
) -> Result<HottrimManifest, TiledExportError> {
    let material_id = document
        .primary_material
        .map(|id| id.to_string())
        .unwrap_or_else(|| document.id.to_string());
    let material_name = document
        .primary_material
        .and_then(|id| document.materials.iter().find(|material| material.id == id))
        .map_or_else(
            || "Hot Trimmer Material".to_owned(),
            |material| material.name.clone(),
        );
    let input = ManifestExportInput {
        project_id: document.id.to_string(),
        material_id,
        material_name,
        material_revision: document.document_revision,
        output_size: [
            document.render_settings.output_size.width,
            document.render_settings.output_size.height,
        ],
        normal_orientation: "OpenGL".to_owned(),
        maps,
    };
    if let Some(snapshot) = document.topology.snapshot.template.as_ref() {
        let definition: hot_trimmer_domain::TemplateDefinition =
            serde_json::from_str(&snapshot.snapshot_json)
                .map_err(|failure| TiledExportError::Encoder(failure.to_string()))?;
        return manifest_from_template(&definition, input)
            .map_err(|failure| TiledExportError::Encoder(failure.to_string()));
    }
    Ok(HottrimManifest {
        schema_version: 1,
        project_id: input.project_id,
        material_id: input.material_id,
        material_name: input.material_name,
        material_revision: input.material_revision,
        template_id: "source-frame".to_owned(),
        template_version: document.partition_provenance.as_ref().map_or_else(
            || "authored".to_owned(),
            |value| value.recipe.recipe_version.to_string(),
        ),
        compatibility_key: document.topology.compatibility_key.clone(),
        template_snapshot_hash: hash_hex(document.topology.topology_hash),
        output_size: input.output_size,
        normal_orientation: input.normal_orientation,
        maps: input.maps,
        slots: document
            .topology
            .regions
            .iter()
            .map(|region| native_export_slot_manifest(document, region))
            .collect(),
    })
}

fn native_export_slot_manifest(
    document: &TrimSheetDocument,
    region: &hot_trimmer_domain::RegionDefinition,
) -> HottrimSlot {
    let output = document.render_settings.output_size;
    let behavior = document
        .region_bindings
        .get(&region.id)
        .map(|binding| &binding.mapping.behavior);
    let radial_mapping = behavior.and_then(|behavior| behavior.radial).or_else(|| {
        region
            .radial_parameters
            .map(hot_trimmer_domain::RadialMappingSettings::from)
    });
    let radial_parameters = radial_mapping.map(native_export_radial_parameters);
    let is_radial = radial_mapping.is_some()
        || behavior
            .is_some_and(|behavior| behavior.role == hot_trimmer_domain::ManualRegionRole::Radial);
    let sampling = behavior.map(|behavior| native_export_sampling_name(behavior.sampling));
    let repeat_period_pixels = behavior.and_then(|behavior| behavior.period_pixels);
    HottrimSlot {
        slot_id: region.id.to_string(),
        region_id: region.id.to_string(),
        name: region.display_name.clone(),
        allocation_rect: export_pixel_rect(region.allocation_rect),
        pixel_hotspot_rect: export_pixel_rect(region.hotspot_rect),
        normalized_hotspot_rect: export_normalized_rect(
            region.hotspot_rect,
            output.width,
            output.height,
        ),
        role: format!("{:?}", region.role),
        uv_fit: UvFit {
            kind: if is_radial {
                UvFitKind::Radial
            } else {
                UvFitKind::Rectangular
            },
            fit_axis: if is_radial {
                FitAxis::None
            } else {
                FitAxis::Automatic
            },
            keep_proportion: true,
            allowed_rotations: if is_radial {
                vec![0]
            } else {
                vec![0, 90, 180, 270]
            },
            mirror_allowed: !is_radial,
            classification_tags: vec![format!("{:?}", region.structural_profile)],
        },
        world_size_meters: [
            f64::from(region.allocation_rect.width),
            f64::from(region.allocation_rect.height),
        ],
        variation_group: region.material_group.clone(),
        enabled: region.enabled,
        region_id_color: region.id_color.0,
        radial_parameters,
        behavior_role: behavior
            .map(|behavior| native_export_behavior_role_name(behavior.role).to_owned()),
        sampling: sampling.map(str::to_owned),
        repeat_period_pixels,
        orientation: behavior
            .map(|behavior| native_export_orientation_name(behavior.orientation).to_owned()),
        radial_mapping,
    }
}

fn native_export_radial_parameters(
    value: hot_trimmer_domain::RadialMappingSettings,
) -> hot_trimmer_domain::RadialParameters {
    hot_trimmer_domain::RadialParameters {
        center_x: value.center_x,
        center_y: value.center_y,
        inner_radius: value.inner_radius,
        outer_radius: value.outer_radius,
    }
}

fn native_export_behavior_role_name(role: hot_trimmer_domain::ManualRegionRole) -> &'static str {
    match role {
        hot_trimmer_domain::ManualRegionRole::Panel => "panel",
        hot_trimmer_domain::ManualRegionRole::HorizontalStrip => "horizontal_strip",
        hot_trimmer_domain::ManualRegionRole::VerticalStrip => "vertical_strip",
        hot_trimmer_domain::ManualRegionRole::Unique => "unique",
        hot_trimmer_domain::ManualRegionRole::Radial => "radial",
    }
}

fn native_export_sampling_name(sampling: hot_trimmer_domain::RegionSampling) -> &'static str {
    match sampling {
        hot_trimmer_domain::RegionSampling::OneShot => "one_shot",
        hot_trimmer_domain::RegionSampling::LoopX => "loop_x",
        hot_trimmer_domain::RegionSampling::LoopY => "loop_y",
        hot_trimmer_domain::RegionSampling::LoopXy => "loop_xy",
    }
}

fn native_export_orientation_name(orientation: hot_trimmer_domain::QuarterTurn) -> &'static str {
    match orientation {
        hot_trimmer_domain::QuarterTurn::Zero => "zero",
        hot_trimmer_domain::QuarterTurn::Ninety => "ninety",
        hot_trimmer_domain::QuarterTurn::OneEighty => "one_eighty",
        hot_trimmer_domain::QuarterTurn::TwoSeventy => "two_seventy",
    }
}

fn export_pixel_rect(rect: hot_trimmer_domain::CanonicalRect) -> hot_trimmer_export::PixelRect {
    hot_trimmer_export::PixelRect {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    }
}

fn export_normalized_rect(
    rect: hot_trimmer_domain::CanonicalRect,
    width: u32,
    height: u32,
) -> NormalizedRect {
    NormalizedRect {
        x: f64::from(rect.x) / f64::from(width),
        y: f64::from(rect.y) / f64::from(height),
        width: f64::from(rect.width) / f64::from(width),
        height: f64::from(rect.height) / f64::from(height),
    }
}

fn ensure_native_export_current(
    cancellation: &EngineCancellationToken,
    is_current: &impl Fn() -> bool,
) -> Result<(), TiledExportError> {
    if cancellation.is_cancelled() || !is_current() {
        return Err(TiledExportError::StaleRevision);
    }
    Ok(())
}

fn export_error(error_value: TiledExportError) -> UserFacingError {
    match error_value {
        TiledExportError::Cancelled | TiledExportError::StaleRevision => error(
            ErrorCode::OperationCancelled,
            "The export was cancelled or superseded before publication.",
        ),
        TiledExportError::InvalidRequest(message)
        | TiledExportError::GpuValidation(message)
        | TiledExportError::UnsupportedFeatureOrFormat(message)
        | TiledExportError::OutOfMemory(message)
        | TiledExportError::DeviceLost(message)
        | TiledExportError::Readback(message)
        | TiledExportError::Encoder(message) => error(ErrorCode::LayoutInvalid, &message),
        TiledExportError::Filesystem(failure) => io_error(failure),
    }
}

fn build_stage_14_preview(
    session: &SharedProjectSession,
    preview_service: &PreviewService,
    request: Stage14PreviewRequest,
    job: u64,
) -> Result<IntermediateAtlasProjection, UserFacingError> {
    let (mut summary, refreshed_document_assets) =
        refreshed_stage_14_project_summary(session, preview_service)?;
    let accepted_document = summary
        .document
        .as_ref()
        .ok_or_else(|| error(ErrorCode::LayoutInvalid, "Create a trim sheet first."))?
        .clone();
    let accepted_revision = accepted_document.document_revision;
    let accepted_refreshed_revision =
        refreshed_document_assets && accepted_revision == request.revision.saturating_add(1);
    if accepted_revision != request.revision && !accepted_refreshed_revision {
        return Err(error(
            ErrorCode::OperationCancelled,
            "A newer document revision superseded this preview.",
        ));
    }
    if let Some(view) = request.feedback_view {
        let document = summary.document.as_mut().expect("accepted document was present");
        let selected_needle = request
            .feedback_selected_operation_id
            .as_deref()
            .map(|id| format!(".{id}."));
        let stage15_view = matches!(view, FeedbackContributionView::Stage15Occupancy | FeedbackContributionView::Stage15Height);
        let stage16_pixel_view = matches!(view,
            FeedbackContributionView::Stage16RegisteredMask
            | FeedbackContributionView::Stage16Height
            | FeedbackContributionView::Stage16VectorNormal
            | FeedbackContributionView::Stage16ScalarRoughness
            | FeedbackContributionView::Stage16ScalarMetallic
            | FeedbackContributionView::Stage16ScalarAmbientOcclusion
            | FeedbackContributionView::Stage16BaseColor
            | FeedbackContributionView::Stage16MaterialId
            | FeedbackContributionView::Stage16MaterialIdValidity);
        if stage15_view || matches!(request.feedback_comparison_mode, Some(FeedbackComparisonMode::Before)) {
            document.decorations.retain(|binding| !is_feedback_detail_key(&binding.decoration_key));
        } else if matches!(request.feedback_comparison_mode, Some(FeedbackComparisonMode::SelectedOperationIsolation)) {
            let needle = selected_needle.ok_or_else(|| error(ErrorCode::InvalidInput, "Selected-operation isolation requires a selected operation."))?;
            document.decorations.retain(|binding| {
                !is_feedback_detail_key(&binding.decoration_key)
                    || binding.decoration_key.contains(".detail.definition.")
                    || binding.decoration_key.contains(&needle)
            });
        }
        if stage16_pixel_view {
            document.decorations.retain(|binding| !binding.decoration_key.starts_with("stage15.profile.request."));
            if let Some(region_id) = request.region_id {
                let document_revision = document.document_revision;
                let appearance_revision = document.appearance_revision;
                let topology_revision = document.topology_revision;
                let mut flattened = document
                    .apply_command(&TrimSheetDocumentCommand::SetRegionStructuralProfile {
                        region_id,
                        structural_profile: StructuralProfile::Flat,
                    })
                    .map_err(|failure| error(ErrorCode::LayoutInvalid, &failure.to_string()))?;
                // This is a transient QA clone, not a persisted authoring command. Preserve
                // the accepted revision while retaining the domain-owned refreshed hash.
                flattened.document_revision = document_revision;
                flattened.appearance_revision = appearance_revision;
                flattened.topology_revision = topology_revision;
                *document = flattened;
            }
            rewrite_feedback_raw_field(document, view)?;
        }
    }
    if request.transient_projection.is_some() && request.region_id.is_none() {
        return Err(error(
            ErrorCode::InvalidInput,
            "Transient crop requests require both regionId and transientProjection.",
        ));
    }
    let view_intent = compiled_view_intent(&request)?;
    if let Some(recipe) = request.candidate_recipe.clone() {
        if request.region_id.is_some() {
            return Err(error(
                ErrorCode::InvalidInput,
                "Candidate topology previews cannot be combined with a transient crop.",
            ));
        }
        let candidate = accepted_document
            .accept_source_frame_partition(recipe)
            .map_err(|failure| error(ErrorCode::LayoutInvalid, &failure.to_string()))?;
        // This is an in-memory candidate snapshot.  Match the accepted revision only so the
        // compiler's persisted-revision guard continues to protect cancellation/publication.
        let mut candidate = candidate;
        candidate.document_revision = accepted_revision;
        candidate.appearance_revision = accepted_revision;
        candidate.topology_revision = accepted_document.topology_revision;
        summary.document = Some(candidate);
    }
    if let (Some(region_id), Some(projection)) =
        (request.region_id, request.transient_projection.clone())
    {
        let mut transient_document = summary
            .document
            .as_ref()
            .expect("summary document was present")
            .clone();
        let binding = transient_document
            .region_bindings
            .get_mut(&region_id)
            .ok_or_else(|| {
                error(
                    ErrorCode::InvalidInput,
                    "Transient crop region is not in the persisted topology.",
                )
            })?;
        binding.mapping.projection = projection;
        binding.mapping.source_crop_intent = Some(hot_trimmer_domain::SourceCropIntent::Authored);
        summary.document = Some(transient_document);
    }
    let document = summary
        .document
        .as_ref()
        .expect("summary document was present");
    let revision_current = AtomicBool::new(true);
    let monitoring_complete = AtomicBool::new(false);
    let cancellation = EngineCancellationToken::new();
    let artifact = std::thread::scope(|scope| {
        scope.spawn(|| {
            while !monitoring_complete.load(Ordering::Acquire) {
                let live = session
                    .lock()
                    .ok()
                    .and_then(|guard| {
                        guard
                            .store
                            .as_ref()?
                            .summary()
                            .ok()?
                            .document
                            .map(|document| document.document_revision == accepted_revision)
                    })
                    .unwrap_or(false);
                if !live {
                    revision_current.store(false, Ordering::Release);
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        let gpu_executor = hot_trimmer_sheet_compiler::GpuAtlasRenderExecutor {
            service: &preview_service.gpu_capabilities,
            source_texture_cache: &preview_service.gpu_source_cache,
        };
        let result = AlgorithmCompiler::new()
            .compile_persisted_stage_14_preview_with_cache_and_executor(
                hot_trimmer_sheet_compiler::PersistedStage14PreviewRequest {
                    project: &summary,
                    revision: accepted_revision,
                    // Native owns publication generation. The frontend draft ID is
                    // scheduling metadata and is absent on several valid commands.
                    draft_id: Some(job),
                    input_hash: request.input_hash.clone(),
                    profile: request.profile.into(),
                    view_intent,
                },
                &cancellation,
                || {
                    preview_service.latest_draft_id.load(Ordering::Acquire) == job
                        && revision_current.load(Ordering::Acquire)
                },
                Some(&preview_service.source_frame_cache),
                Some(&gpu_executor),
            );
        monitoring_complete.store(true, Ordering::Release);
        result
    })
    .map_err(|failure| error(ErrorCode::OperationCancelled, &failure.to_string()))?;
    let preview_is_current = || {
        session
            .lock()
            .ok()
            .and_then(|guard| {
                guard
                    .store
                    .as_ref()?
                    .summary()
                    .ok()?
                    .document
                    .map(|document| document.document_revision == accepted_revision)
            })
            .unwrap_or(false)
            && preview_service.latest_draft_id.load(Ordering::Acquire) == job
    };
    if !preview_is_current() {
        preview_service
            .cancellation_count
            .fetch_add(1, Ordering::AcqRel);
        return Err(error(
            ErrorCode::OperationCancelled,
            "A newer document revision superseded this preview.",
        ));
    }
    if document.source_frame.is_some() && artifact.regions.is_empty() {
        return Err(error(
            ErrorCode::LayoutInvalid,
            "The compiled SourceFrame artifact did not publish profile-local region metadata.",
        ));
    }
    let output_size = artifact.topology.output_size;
    let mut maps = BTreeMap::new();
    let mut tile_manifest = None;
    let mut tile_manifests = BTreeMap::new();
    if let Some(rendered_tile) = artifact.rendered_tile.clone() {
        let rendered_tile = rendered_tile.for_publication_generation(job);
        let publication = publish_gpu_tiled_preview(
            preview_service,
            rendered_tile.manifest.clone(),
            rendered_tile.payload(),
        )?;
        tile_manifests.insert(
            material_map_view_key(rendered_tile.manifest.map).to_string(),
            publication.clone(),
        );
        for (map, tile) in &artifact.rendered_display_tiles {
            if *map == rendered_tile.manifest.map {
                continue;
            }
            let tile = tile.for_publication_generation(job);
            let publication =
                publish_gpu_tiled_preview(preview_service, tile.manifest.clone(), tile.payload())?;
            tile_manifests.insert(material_map_view_key(*map).to_string(), publication);
        }
        tile_manifest = Some(publication);
    } else {
        for channel in &artifact.channels {
            maps.insert(
                channel_key(channel.role).into(),
                png_data_url(
                    artifact.topology.output_size.width,
                    artifact.topology.output_size.height,
                    channel.rgba8.clone(),
                )?,
            );
        }
    }
    if !preview_is_current() {
        cancellation.cancel();
        preview_service
            .cancellation_count
            .fetch_add(1, Ordering::AcqRel);
        return Err(error(
            ErrorCode::OperationCancelled,
            "A newer document revision superseded this preview after publication.",
        ));
    }
    let mut artifact = artifact;
    match preview_service.gpu_capabilities.initialize() {
        Ok(state) => artifact
            .telemetry
            .push(state.capabilities().diagnostic_line()),
        Err(error) => artifact.telemetry.push(format!(
            "gpu_capability_generation={}; status=unsupported; reason={error}",
            preview_service.gpu_capabilities.generation()
        )),
    }
    if let Some(tile_manifest) = &tile_manifest {
        artifact.telemetry.push(format!("native_tile_publish_ms={}; raw_ipc_bytes={}; raw_ipc_ms={}; tile_generation={}; tile_dimensions={}x{}; rgba_nontransparent_bounds={}; cancellation_count={}",
            tile_manifest.telemetry.native_publish_ms, tile_manifest.telemetry.raw_ipc_bytes,
            tile_manifest.telemetry.raw_ipc_ms, tile_manifest.telemetry.generation,
            tile_manifest.manifest.width, tile_manifest.manifest.height, "executor_tile",
            preview_service.cancellation_count.load(Ordering::Acquire)));
    } else {
        artifact.telemetry.push(format!(
            "cpu_channel_publication_maps={}; cancellation_count={}",
            maps.len(),
            preview_service.cancellation_count.load(Ordering::Acquire)
        ));
    }
    let slots = artifact
        .slots
        .iter()
        .map(|slot| {
            let region = artifact
                .regions
                .iter()
                .find(|region| region.region_id == slot.region_id);
            Ok(Stage14SlotProjection {
                region_id: slot.region_id,
                slot_key: slot.slot_key.clone(),
                display_name: region.map_or_else(
                    || slot.display_name.clone(),
                    |region| region.display_name.clone(),
                ),
                allocation_bounds: slot.allocation,
                hotspot_bounds: slot.hotspot,
                semantic_rect: slot.semantic_rect,
                padded_rect: slot.padded_rect,
                atlas_destination: slot.atlas_destination,
                preview_padding_px: slot.preview_padding_px,
                mapping_mode: format!("{:?}", slot.mapping_mode),
                source_transform: slot.source_transform,
                isotropic_scale: slot.isotropic_scale,
                sampling_scale: slot.sampling_scale,
                validity: format!("{} valid pixels", slot.valid_pixel_count),
                correspondence: "authoritative Stage 14 correspondence".into(),
                source_id: slot.source_id.0.clone(),
                patch_id: slot.patch_id.clone(),
                domain_id: slot.domain_id.0.clone(),
                candidate_id: slot.candidate_id.0.clone(),
                sampling_plan_id: slot.sampling_plan_id.0.clone(),
                stage_14_result_id: slot.stage_14_result_id.0.clone(),
                source_crop: slot.source_crop,
                source_bounds: region.and_then(|region| region.source_bounds),
                mapping_origin: region.and_then(|region| region.mapping_origin),
                grid_rect: slot.grid_rect,
                behavior_version: slot.behavior_version,
                role: slot.role,
                continuity: slot.continuity,
                requested_sampling: slot.requested_sampling,
                executed_mode: format!("{:?}", slot.executed_mode),
                edge_eligibility: slot.edge_eligibility,
                period_pixels: slot.period_pixels,
                address_mode: slot.address_mode.into(),
                compiled_profile: slot.compiled_profile.clone(),
                compiled_details: slot.compiled_details.clone(),
            })
        })
        .collect::<Result<Vec<_>, UserFacingError>>()?;
    if let Some(recipe) = request.candidate_recipe.as_ref() {
        preview_service
            .previewed_candidate_recipes
            .lock()
            .map_err(|_| poisoned())?
            .insert((request.revision, recipe.hash()));
    }
    let refreshed_project = if refreshed_document_assets {
        let guard = session.lock().map_err(|_| poisoned())?;
        Some(project_projection(&guard)?)
    } else {
        None
    };
    let feedback_execution = request.feedback_view.map(|view| {
        let map = request.requested_maps[0];
        let requested_map = material_map_view_key(map);
        let publication = tile_manifests
            .get(requested_map)
            .or(tile_manifest.as_ref())
            .expect("feedback pixel requests publish their requested GPU tile");
        let cache_reused = artifact
            .telemetry
            .iter()
            .any(|entry| entry.contains("composed_cache=hit"));
        FeedbackPreviewExecution {
            request_identity: request.input_hash.clone().expect("feedback requests carry an exact identity"),
            client_generation: request.draft_id.unwrap_or(job),
            published_generation: publication.manifest.generation,
            revision: request.revision,
            region_id: request.region_id.expect("feedback requests target one region"),
            all_regions: !matches!(request.view_intent, Some(PreviewViewIntent::ExactSelectedRegion)),
            view,
            requested_map,
            profile: request.profile,
            comparison_mode: request.feedback_comparison_mode.expect("feedback requests carry comparison mode"),
            selected_operation_id: request.feedback_selected_operation_id.clone(),
            outcome: if cache_reused { FeedbackExecutionState::CacheHit } else { FeedbackExecutionState::Executed },
            cache_reused,
        }
    });
    Ok(IntermediateAtlasProjection {
        label: artifact.label,
        non_exportable: true,
        incomplete_after_stage: 14,
        revision: artifact.revision,
        document_revision: artifact.revision,
        topology_hash: hash_hex(document.topology.topology_hash),
        appearance_hash: hash_hex(
            document
                .appearance_hash()
                .map_err(|failure| error(ErrorCode::LayoutInvalid, &failure.to_string()))?,
        ),
        renderer_version: "intermediate-stage-14",
        width: artifact.topology.output_size.width,
        height: output_size.height,
        topology: artifact.topology,
        placement_plan_id: artifact.placement_plan_id.0,
        maps,
        tile_manifest,
        tile_manifests,
        feedback_execution,
        region_id_lookup: artifact.region_id_lookup,
        regions: artifact.regions,
        unavailable_channels: artifact
            .unavailable_channels
            .iter()
            .map(|role| format!("{role:?}"))
            .collect(),
        slots,
        pending: artifact.pending,
        telemetry: artifact.telemetry,
        final_compile_available: false,
        export_available: false,
        blender_available: false,
        source_frame: document.source_frame.clone(),
        project: refreshed_project,
    })
}

const fn channel_key(role: hot_trimmer_domain::MaterialChannelRole) -> &'static str {
    use hot_trimmer_domain::MaterialChannelRole;
    match role {
        MaterialChannelRole::BaseColor => "baseColor",
        MaterialChannelRole::Normal => "normal",
        MaterialChannelRole::Height => "height",
        MaterialChannelRole::Roughness => "roughness",
        MaterialChannelRole::Metallic => "metallic",
        MaterialChannelRole::AmbientOcclusion => "ambientOcclusion",
        MaterialChannelRole::Specular => "specular",
        MaterialChannelRole::Opacity => "opacity",
        MaterialChannelRole::EdgeMask => "edgeMask",
        MaterialChannelRole::RegionId => "regionId",
        MaterialChannelRole::MaterialId => "materialId",
    }
}

const fn material_map_view_key(map: MaterialMapKind) -> &'static str {
    match map {
        MaterialMapKind::BaseColor => "baseColor",
        MaterialMapKind::Normal => "normal",
        MaterialMapKind::Height => "height",
        MaterialMapKind::Roughness => "roughness",
        MaterialMapKind::Metallic => "metallic",
        MaterialMapKind::AmbientOcclusion => "ambientOcclusion",
        MaterialMapKind::RegionId => "regionId",
        MaterialMapKind::MaterialId => "materialId",
        MaterialMapKind::Specular => "specular",
        MaterialMapKind::Opacity => "opacity",
        MaterialMapKind::EdgeMask => "edgeMask",
    }
}

fn compile_trim_sheet_document_impl(
    session: &SharedProjectSession,
    _preview_service: &PreviewService,
) -> Result<CompiledSheetProjection, UserFacingError> {
    let summary = {
        let session = session.lock().map_err(|_| poisoned())?;
        session
            .store
            .as_ref()
            .ok_or_else(no_project)?
            .summary()
            .map_err(store_error)?
    };
    let document = summary
        .document
        .as_ref()
        .ok_or_else(|| error(ErrorCode::LayoutInvalid, "Create a trim sheet first."))?;
    let request = CompilerRequestHeader {
        contract_version: ALGORITHM_STACK_CONTRACT_VERSION,
        source_digests: summary
            .sources
            .iter()
            .map(|source| ContentDigest(source.input.sha256.clone()))
            .collect(),
        settings_hash: ContentDigest(hash_hex(
            document
                .appearance_hash()
                .map_err(|invalid| error(ErrorCode::LayoutInvalid, &invalid.to_string()))?,
        )),
        algorithm_versions: (1_u8..=20)
            .map(|stage| {
                (
                    stage,
                    AlgorithmProvenance {
                        algorithm_id: format!("stage-{stage:02}"),
                        version: String::from("0.0.0-unsupported"),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>(),
        template_topology_hash: ContentDigest(hash_hex(document.topology.topology_hash)),
        output: OutputSpecHeader {
            width: document.render_settings.output_size.width,
            height: document.render_settings.output_size.height,
            mip_count: 1,
        },
        seed: 0,
        revision: document.document_revision,
    };
    match AlgorithmCompiler::new().compile(&request, &EngineCancellationToken::new()) {
        Ok(_) => Err(error(
            ErrorCode::Internal,
            "No authoritative output projection is installed for the algorithm stack.",
        )),
        Err(unsupported) => Err(error(
            ErrorCode::LayoutInvalid,
            &format!("Build unavailable: {unsupported}"),
        )),
    }
}

#[cfg(any())]
#[tauri::command]
pub async fn preview_trim_sheet_document(
    request: PreviewDocumentRequest,
    _session: State<'_, SharedProjectSession>,
    _preview_service: State<'_, SharedPreviewService>,
) -> Result<PreviewSheetProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    Err(error(
        ErrorCode::LayoutInvalid,
        "Preview is unavailable until the algorithm stack installs its first source route.",
    ))
}

#[cfg(any())]
fn preview_trim_sheet_document_impl(
    request: PreviewDocumentRequest,
    session: &SharedProjectSession,
    preview_service: &PreviewService,
) -> Result<PreviewSheetProjection, UserFacingError> {
    let summary = {
        let session = session.lock().map_err(|_| poisoned())?;
        session
            .store
            .as_ref()
            .ok_or_else(no_project)?
            .summary()
            .map_err(store_error)?
    };
    let mut document = summary
        .document
        .ok_or_else(|| error(ErrorCode::LayoutInvalid, "Create a trim sheet first."))?;
    let max_edge = request.max_edge.unwrap_or(1024).clamp(512, 1024);
    // Topology identifies the compositing surface. Appearance edits deliberately reuse the
    // previous pixels and repaint only their dirty region.
    let settled_key = (
        hash_hex(document.topology.topology_hash),
        request.map_view,
        max_edge,
    );
    let base_pixels = if request.region_id.is_some() {
        preview_service
            .settled_previews
            .lock()
            .map_err(|_| poisoned())?
            .get(&settled_key)
            .map(|preview| preview.pixels.clone())
    } else {
        None
    };
    // A dirty preview is valid only when it composites onto a known-complete surface. If this
    // topology/map has no settled base yet, render the whole low-resolution sheet once instead
    // of promoting a one-region image with a black background into the settled cache.
    let effective_dirty_region = request.region_id.filter(|_| base_pixels.is_some());
    if let (Some(region_id), Some(projection)) = (request.region_id, request.projection) {
        let binding = document
            .region_bindings
            .get_mut(&region_id)
            .ok_or_else(|| {
                error(
                    ErrorCode::InvalidInput,
                    "The preview region no longer exists.",
                )
            })?;
        binding.mapping.projection = projection;
    }
    let maps = summary
        .sources
        .iter()
        .map(|source| registered_map_cached(source, &preview_service))
        .collect::<Result<Vec<_>, _>>()?;
    let compiled = compile_preview_map_incremental(
        &document,
        &maps,
        request.map_view,
        max_edge,
        base_pixels,
        effective_dirty_region,
        || preview_service.latest_draft_id.load(Ordering::Acquire) != request.draft_id,
    )
    .map_err(|compile| match compile {
        hot_trimmer_sheet_compiler::SheetCompileError::Cancelled => error(
            ErrorCode::OperationCancelled,
            "A newer preview superseded this draft.",
        ),
        _ => error(
            ErrorCode::LayoutInvalid,
            &format!("Preview failed: {compile}"),
        ),
    })?;
    if preview_service.latest_draft_id.load(Ordering::Acquire) != request.draft_id {
        return Err(error(
            ErrorCode::OperationCancelled,
            "A newer preview superseded this draft.",
        ));
    }
    {
        let mut cache = preview_service
            .settled_previews
            .lock()
            .map_err(|_| poisoned())?;
        if cache.len() >= 16 {
            cache.clear();
        }
        cache.insert(settled_key, compiled.clone());
    }
    Ok(PreviewSheetProjection {
        draft_id: request.draft_id,
        document_revision: compiled.document_revision,
        topology_hash: hash_hex(compiled.topology_hash),
        appearance_hash: hash_hex(compiled.appearance_hash),
        width: compiled.dimensions.width,
        height: compiled.dimensions.height,
        map_view: request.map_view,
        data_url: png_data_url(
            compiled.dimensions.width,
            compiled.dimensions.height,
            compiled.pixels,
        )?,
        regions: compiled.regions,
    })
}

#[tauri::command]
pub fn save_project(
    request: FoundationStatusRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectProjection, UserFacingError> {
    request.validate().map_err(UserFacingError::from)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    session
        .store
        .as_ref()
        .ok_or_else(no_project)?
        .save()
        .map_err(store_error)?;
    session.dirty = false;
    remember_open_project_best_effort(&session);
    project_projection(&session)
}

#[tauri::command]
pub fn save_project_as(
    request: ProjectPathRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    let next = session
        .store
        .as_ref()
        .ok_or_else(no_project)?
        .save_as(Path::new(&request.path))
        .map_err(store_error)?;
    session.store = Some(next);
    session.dirty = false;
    session.is_draft = false;
    remember_open_project_best_effort(&session);
    project_projection(&session)
}

#[tauri::command]
pub fn list_recent_projects(
    request: FoundationStatusRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<Vec<RecentProject>, UserFacingError> {
    request.validate().map_err(UserFacingError::from)?;
    let session = session.lock().map_err(|_| poisoned())?;
    read_recent_projects(&session.app_data_dir)
}

#[tauri::command]
pub fn close_project(
    request: CloseProjectRequest,
    session: State<'_, SharedProjectSession>,
    preview_service: State<'_, SharedPreviewService>,
) -> Result<(), UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    if request.save {
        if let Some(store) = &session.store {
            store.save().map_err(store_error)?;
        }
    } else if session.dirty {
        if let Some(baseline) = session.baseline.clone()
            && let Some(store) = &mut session.store
        {
            store.restore_from(&baseline).map_err(store_error)?;
        }
    }
    session.store = None;
    session
        .source_projection_cache
        .lock()
        .map_err(|_| poisoned())?
        .clear();
    session.preview_prepared_sources.clear();
    session.prepared_exemplars = PreparedExemplarCache::default();
    session.source_analysis_cache = SourceAnalysisCache::default();
    session.scale_orientation_cache = ScaleOrientationCache::default();
    preview_service.reset();
    if let Some(path) = session.baseline.take() {
        let _ = fs::remove_file(path);
    }
    session.dirty = false;
    Ok(())
}

#[tauri::command]
pub fn take_pending_project_path(
    request: FoundationStatusRequest,
    pending: State<'_, PendingProjectPath>,
) -> Result<Option<String>, UserFacingError> {
    request.validate().map_err(UserFacingError::from)?;
    Ok(pending.lock().map_err(|_| poisoned())?.take())
}

fn project_projection(session: &ProjectSession) -> Result<ProjectProjection, UserFacingError> {
    let store = session.store.as_ref().ok_or_else(no_project)?;
    let summary = store.summary().map_err(store_error)?;
    let material_sources = summary
        .source_sets
        .iter()
        .map(|material| {
            let channels = summary
                .sources
                .iter()
                .filter(|source| source.source_set_id.to_string() == material.id.to_string())
                .map(|source| session.source_projection_cached(source))
                .collect::<Result<Vec<_>, _>>()?;
            let registered_channels = if channels.is_empty() {
                None
            } else {
                let anchor = channels
                    .iter()
                    .find(|source| source.channel == SourceChannel::BaseColor)
                    .ok_or_else(|| {
                        error(
                            ErrorCode::ProjectInvalid,
                            "A registered channel set has no Base Color anchor.",
                        )
                    })?;
                Some(RegisteredChannelSetProjection {
                    oriented_size: anchor.oriented_size,
                    orientation: anchor.orientation,
                    channels,
                })
            };
            Ok(MaterialSourceProjection {
                id: material.id.to_string(),
                name: material.name.clone(),
                exemplar_group: material.exemplar_group.clone(),
                source_revision: material.source_revision,
                registration_digest: material.registration_digest.0.clone(),
                delighting: material.delighting.clone(),
                classification: material.classification,
                calibration: material.calibration,
                registered_channels,
            })
        })
        .collect::<Result<Vec<_>, UserFacingError>>()?;
    let feedback_authoring = feedback_authoring_projection(summary.document.as_ref())?;
    Ok(ProjectProjection {
        id: summary.id.to_string(),
        name: summary.name,
        path: summary.path.display().to_string(),
        schema_version: hot_trimmer_project_store::CURRENT_SCHEMA_VERSION,
        dirty: session.dirty,
        is_draft: session.is_draft,
        material_sources,
        patches: summary.patches,
        document: summary.document,
        legacy_layout_discarded: summary.legacy_layout_discarded,
        can_undo_document: store.can_undo_document_command(),
        can_redo_document: store.can_redo_document_command(),
        can_undo_patch: store.can_undo_patch_command(),
        can_redo_patch: store.can_redo_patch_command(),
        feedback_authoring,
    })
}

fn feedback_authoring_projection(
    document: Option<&TrimSheetDocument>,
) -> Result<FeedbackAuthoringProjection, UserFacingError> {
    let mut records = Vec::new();
    if let Some(document) = document {
        for binding in document
            .decorations
            .iter()
            .filter(|binding| is_feedback_detail_key(&binding.decoration_key))
        {
            let operation_id = binding
                .decoration_key
                .rsplit('.')
                .nth(1)
                .ok_or_else(|| error(ErrorCode::ProjectInvalid, "A persisted detail identity is malformed."))?
                .to_owned();
            validate_feedback_operation_id(&operation_id)?;
            records.push(FeedbackDetailRecordProjection {
                operation_id,
                enabled: !binding.decoration_key.starts_with("stage16.disabled."),
                intent: parse_feedback_detail(binding)?,
            });
        }
    }
    Ok(FeedbackAuthoringProjection {
        command_version: FEEDBACK_COMMAND_VERSION,
        records,
    })
}

fn source_projection(source: &StoredSource) -> Result<SourceProjection, UserFacingError> {
    let inspected = inspect_stored(source)?;
    Ok(SourceProjection {
        id: source.input.id.to_string(),
        channel: source.channel,
        display_name: source.input.origin_path.file_name().map_or_else(
            || source.input.origin_path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        ),
        original: OriginalSourceProjection {
            path: source.input.origin_path.display().to_string(),
            immutable_digest: source.input.sha256.clone(),
            encoded_bytes: source.input.encoded_bytes,
        },
        storage: SourceStorageProjection {
            ownership: source.input.ownership,
            external_path: source
                .input
                .external_path
                .as_ref()
                .map(|path| path.display().to_string()),
        },
        oriented_size: OrientedSizeProjection {
            width: inspected.info.width,
            height: inspected.info.height,
        },
        orientation: source.input.exif_orientation,
        interpretation: source.registration.interpretation,
        normal_convention: source.registration.normal_convention,
        assignment_provenance: source.registration.assignment_provenance,
        confidence_milli: source.registration.confidence_milli,
        thumbnail_data_url: format!(
            "data:image/png;base64,{}",
            STANDARD.encode(inspected.thumbnail_png)
        ),
    })
}

fn inspect_stored(source: &StoredSource) -> Result<InspectedImage, UserFacingError> {
    let bytes = source_bytes(source)?;
    let inspected =
        inspect_bytes_with_policy(bytes, DecodeLimits::default(), color_policy(source.channel))
            .map_err(image_error)?;
    if inspected.info.sha256 != source.input.sha256 {
        return Err(error(
            ErrorCode::ImageImportFailed,
            "An externally referenced source has changed.",
        ));
    }
    Ok(inspected)
}

fn source_bytes(source: &StoredSource) -> Result<Vec<u8>, UserFacingError> {
    match source.input.ownership {
        SourceOwnership::OwnedCopy => source
            .input
            .owned_bytes
            .clone()
            .ok_or_else(|| error(ErrorCode::ProjectInvalid, "Owned source bytes are missing.")),
        SourceOwnership::VerifiedExternalReference => {
            let path = source.input.external_path.as_ref().ok_or_else(|| {
                error(
                    ErrorCode::ProjectInvalid,
                    "External source path is missing.",
                )
            })?;
            fs::read(path).map_err(io_error)
        }
    }
}

fn preview_registered_channel_set(
    session: &ProjectSession,
    sources: &[StoredSource],
    source_set_id: Uuid,
) -> Result<(RegisteredChannelSet, BTreeMap<SourceId, Vec<u8>>), UserFacingError> {
    let mut selected: Vec<_> = sources
        .iter()
        .filter(|source| source.source_set_id == source_set_id)
        .collect();
    selected.sort_by_key(|source| source.registration.role);
    if !selected
        .iter()
        .any(|source| source.registration.role == MaterialChannelRole::BaseColor)
    {
        return Err(error(
            ErrorCode::ProjectInvalid,
            "A prepared patch requires Base Color.",
        ));
    }
    let mut encoded_sources = BTreeMap::new();
    let mut channels = Vec::with_capacity(selected.len());
    let mut oriented_size = None;
    for source in selected {
        let projection = session.source_projection_cached(source)?;
        let (_, payload) = projection
            .thumbnail_data_url
            .split_once(',')
            .ok_or_else(|| {
                error(
                    ErrorCode::Internal,
                    "The cached source preview is malformed.",
                )
            })?;
        let bytes = STANDARD.decode(payload).map_err(|failure| {
            error(
                ErrorCode::Internal,
                &format!("The cached source preview is invalid: {failure}"),
            )
        })?;
        let image = image::load_from_memory(&bytes).map_err(|failure| {
            error(
                ErrorCode::ImageImportFailed,
                &format!("The cached source preview could not be decoded: {failure}"),
            )
        })?;
        let dimensions = OrientedPixelSize {
            width: image.width(),
            height: image.height(),
        };
        if oriented_size.is_some_and(|expected| expected != dimensions) {
            return Err(error(
                ErrorCode::SourceRegistrationFailed,
                "Registered preview channels no longer share dimensions.",
            ));
        }
        oriented_size = Some(dimensions);
        channels.push(RegisteredChannel {
            source_id: source.input.id,
            registration: source.registration.clone(),
            oriented_size: dimensions,
            orientation: 1,
            original: OriginalAssetProvenance {
                original_path: source.input.origin_path.display().to_string(),
                immutable_digest: ContentDigest::sha256(&bytes),
                encoded_bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            },
            ownership: SourceOwnershipIntent::OwnedCopy,
        });
        encoded_sources.insert(source.input.id, bytes);
    }
    let registered = RegisteredChannelSet {
        oriented_size: oriented_size.ok_or_else(|| {
            error(
                ErrorCode::ProjectInvalid,
                "A prepared patch requires registered channels.",
            )
        })?,
        orientation: 1,
        channels,
    };
    Ok((registered, encoded_sources))
}

fn prepared_preview_projection(
    exemplar: &PreparedExemplar,
    base: &hot_trimmer_image_io::ImagePlane<hot_trimmer_image_io::LinearColor>,
    delighting_route: &str,
    delighting_strength_milli: u16,
    material_source_id: String,
    source_analysis: SourceInspectorProjection,
) -> Result<PreparedPatchPreviewProjection, UserFacingError> {
    let mask = exemplar.usable_mask.as_ref();
    let mut rgba = Vec::with_capacity(
        usize::try_from(u64::from(exemplar.width) * u64::from(exemplar.height) * 4)
            .map_err(|_| error(ErrorCode::Internal, "Prepared patch preview is too large."))?,
    );
    for y in 0..exemplar.height {
        for x in 0..exemplar.width {
            let pixel = base.pixel(x, y);
            let coverage = mask.map_or(1.0, |plane| plane.pixel(x, y).0);
            for value in pixel.rgb {
                let encoded = if value <= 0.003_130_8 {
                    value * 12.92
                } else {
                    1.055 * value.powf(1.0 / 2.4) - 0.055
                };
                rgba.push((encoded.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
            rgba.push((pixel.alpha.mul_add(coverage, 0.0).clamp(0.0, 1.0) * 255.0).round() as u8);
        }
    }
    Ok(PreparedPatchPreviewProjection {
        patch_id: exemplar
            .scope
            .patch_id
            .ok_or_else(|| error(ErrorCode::Internal, "Prepared patch scope is missing."))?,
        material_source_id,
        width: exemplar.width,
        height: exemplar.height,
        data_url: png_data_url(exemplar.width, exemplar.height, rgba)?,
        perspective_confidence_milli: exemplar.perspective_confidence_milli,
        delighting_route: delighting_route.to_owned(),
        delighting_strength_milli,
        source_analysis,
    })
}

fn encode_maps(
    width: u32,
    height: u32,
    maps: CompiledMapSet,
) -> Result<CompiledMapsProjection, UserFacingError> {
    Ok(CompiledMapsProjection {
        base_color: png_data_url(width, height, maps.base_color)?,
        normal: png_data_url(width, height, maps.normal)?,
        height: png_data_url(width, height, maps.height)?,
        roughness: png_data_url(width, height, maps.roughness)?,
        metallic: png_data_url(width, height, maps.metallic)?,
        ambient_occlusion: png_data_url(width, height, maps.ambient_occlusion)?,
        region_id: png_data_url(width, height, maps.region_id)?,
        material_id: png_data_url(width, height, maps.material_id)?,
    })
}

fn png_data_url(width: u32, height: u32, pixels: Vec<u8>) -> Result<String, UserFacingError> {
    let encoded = encode_preview_png(width, height, &pixels)?;
    Ok(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(encoded)
    ))
}

fn png_data_url_cancellable(
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    cancellation: &EngineCancellationToken,
    active: impl Fn() -> bool,
) -> Result<String, UserFacingError> {
    if cancellation.is_cancelled() || !active() {
        return Err(error(
            ErrorCode::OperationCancelled,
            "Preview publication was superseded.",
        ));
    }
    let encoded = encode_preview_png(width, height, &pixels)?;
    if cancellation.is_cancelled() || !active() {
        return Err(error(
            ErrorCode::OperationCancelled,
            "Preview publication was superseded.",
        ));
    }
    Ok(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(encoded)
    ))
}

fn encode_preview_png(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>, UserFacingError> {
    let expected = usize::try_from(u64::from(width) * u64::from(height) * 4).map_err(|_| {
        error(
            ErrorCode::Internal,
            "Compiled preview dimensions are invalid.",
        )
    })?;
    if pixels.len() != expected {
        return Err(error(ErrorCode::Internal, "Compiled pixels are invalid."));
    }
    // Preview publication optimizes latency, not file size. Export encoding remains separate.
    // The default adaptive PNG path is disproportionately expensive for high-entropy 2Kâ€“8K
    // material imagery, especially in development builds.
    let mut encoded = Vec::new();
    PngEncoder::new_with_quality(&mut encoded, CompressionType::Fast, FilterType::Sub)
        .write_image(pixels, width, height, ColorType::Rgba8.into())
        .map_err(|failure| error(ErrorCode::Internal, &failure.to_string()))?;
    Ok(encoded)
}

fn rgba_nontransparent_bounds(pixels: &[u8], width: u32, height: u32) -> String {
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for y in 0..height {
        for x in 0..width {
            let index = ((y * width + x) * 4 + 3) as usize;
            if pixels.get(index).copied().unwrap_or(0) == 0 {
                continue;
            }
            bounds = Some(match bounds {
                Some((left, top, right, bottom)) => {
                    (left.min(x), top.min(y), right.max(x + 1), bottom.max(y + 1))
                }
                None => (x, y, x + 1, y + 1),
            });
        }
    }
    bounds.map_or_else(
        || "none".into(),
        |(x, y, right, bottom)| format!("{x},{y} {}x{}", right - x, bottom - y),
    )
}

fn source_input(path: &Path, ownership: SourceOwnership, image: &InspectedImage) -> SourceInput {
    SourceInput {
        id: SourceId::new(),
        ownership,
        external_path: (ownership == SourceOwnership::VerifiedExternalReference)
            .then(|| path.to_path_buf()),
        origin_path: path.to_path_buf(),
        sha256: image.info.sha256.clone(),
        width: image.info.width,
        height: image.info.height,
        format: image.info.format.clone(),
        color_type: image.info.color_type.clone(),
        has_alpha: image.info.has_alpha,
        exif_orientation: image.info.exif_orientation,
        has_embedded_icc_profile: image.info.has_embedded_icc_profile,
        encoded_bytes: image.info.encoded_bytes,
        owned_bytes: (ownership == SourceOwnership::OwnedCopy).then(|| image.source_bytes.clone()),
    }
}

const fn color_policy(channel: SourceChannel) -> ColorPolicy {
    if matches!(channel, SourceChannel::BaseColor) {
        ColorPolicy::ConvertToSrgb
    } else {
        ColorPolicy::PreserveLinearData
    }
}

fn hash_hex(hash: hot_trimmer_domain::DocumentHash) -> String {
    hash.0.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod algorithm_stage_20a_feedback_workbench_tests {
    use super::*;

    fn test_session(root: &Path) -> ProjectSession {
        fs::create_dir_all(root).expect("test root");
        ProjectSession {
            store: Some(ProjectStore::create(&root.join("feedback.hottrimmer"), "Untitled").expect("empty draft")),
            dirty: false,
            is_draft: true,
            baseline: None,
            app_data_dir: root.join("app"),
            recovery_dir: root.join("recovery"),
            draft_dir: root.join("draft"),
            source_projection_cache: Mutex::new(HashMap::new()),
            preview_prepared_sources: HashMap::new(),
            prepared_exemplars: PreparedExemplarCache::default(),
            source_analysis_cache: SourceAnalysisCache::default(),
            scale_orientation_cache: ScaleOrientationCache::default(),
        }
    }

    fn feedback_request(
        revision: u64,
        region_id: RegionId,
        comparison_mode: FeedbackComparisonMode,
        selected_operation_id: Option<&str>,
    ) -> FeedbackQaTileRequest {
        FeedbackQaTileRequest {
            protocol_version: IPC_PROTOCOL_VERSION,
            command_version: FEEDBACK_COMMAND_VERSION,
            revision,
            generation: 77,
            region_id,
            all_regions: false,
            view: FeedbackContributionView::Stage16RegisteredMask,
            profile: PreviewProfile::Refinement1024,
            comparison_mode,
            selected_operation_id: selected_operation_id.map(str::to_owned),
        }
    }

    #[test]
    fn mvp_edge_wear_global_feedback_request_uses_the_full_atlas() {
        let region_id = RegionId::from_bytes([0x20; 16]);
        let mut request = feedback_request(1, region_id, FeedbackComparisonMode::After, None);
        assert!(matches!(
            feedback_preview_view_intent(&request),
            Some(PreviewViewIntent::ExactSelectedRegion)
        ));
        request.all_regions = true;
        assert!(feedback_preview_view_intent(&request).is_none());
    }

    fn solid_png(pixel: [u8; 4]) -> Vec<u8> {
        let rgba = pixel.repeat(16);
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&rgba, 4, 4, ColorType::Rgba8.into())
            .expect("test PNG");
        png
    }

    fn run_feedback_preview(
        session: &SharedProjectSession,
        service: &PreviewService,
        request: &FeedbackQaTileRequest,
    ) -> Result<IntermediateAtlasProjection, UserFacingError> {
        let map = feedback_map_for_view(request.view)?.expect("pixel view");
        let identity = feedback_preview_cache_identity(request, map)?;
        let job = service.latest_draft_id.fetch_add(1, Ordering::AcqRel).saturating_add(1);
        build_stage_14_preview(
            session,
            service,
            Stage14PreviewRequest {
                protocol_version: request.protocol_version,
                revision: request.revision,
                region_id: Some(request.region_id),
                transient_projection: None,
                draft_id: Some(request.generation),
                input_hash: Some(identity),
                profile: request.profile,
                view_intent: feedback_preview_view_intent(request),
                viewport_rect: None,
                requested_maps: vec![map],
                candidate_recipe: None,
                feedback_view: Some(request.view),
                feedback_comparison_mode: Some(request.comparison_mode),
                feedback_selected_operation_id: request.selected_operation_id.clone(),
            },
            job,
        )
    }

    #[test]
    fn algorithm_stage_20a_feedback_workbench_contract() {
        assert_eq!(FEEDBACK_COMMAND_VERSION, 1);
        assert_eq!(STAGE_15_20_DEBUG_SCHEMA_VERSION, 1);
        assert_eq!(
            structural_profile_for_feedback(
                hot_trimmer_effect_compiler::ProfileProgram::ConvexBevel,
                TemplateSlotRole::Planar,
            )
            .expect("planar bevel"),
            StructuralProfile::Bevel,
        );
        assert!(
            structural_profile_for_feedback(
                hot_trimmer_effect_compiler::ProfileProgram::Annulus,
                TemplateSlotRole::Planar,
            )
            .is_err(),
            "illegal role/profile pairs must remain typed failures",
        );
        assert_eq!(
            sanitize_debug_string(r"C:\Users\person\private.png"),
            "[redacted prohibited debug value]",
        );
        assert_eq!(
            sanitize_debug_string("stage16_cache_hit=true"),
            "stage16_cache_hit=true",
        );
        let states = [
            FeedbackExecutionState::InstalledNotRequested,
            FeedbackExecutionState::Requested,
            FeedbackExecutionState::Executed,
            FeedbackExecutionState::CacheHit,
            FeedbackExecutionState::SkippedBecauseUnused,
            FeedbackExecutionState::DeferredOnly,
            FeedbackExecutionState::Failed,
            FeedbackExecutionState::Cancelled,
            FeedbackExecutionState::Superseded,
            FeedbackExecutionState::NotInstalled,
        ];
        assert_eq!(states.len(), 10, "absence is represented by an explicit state");
    }

    #[test]
    fn algorithm_stage_20a_feedback_workbench_server_owns_decoration_keys() {
        let intent = FeedbackDetailIntent::Operation(hot_trimmer_effect_compiler::StampOperation {
            asset: hot_trimmer_effect_compiler::StampAssetRef {
                asset_id: "asset".into(),
                version: "1".into(),
                digest: hot_trimmer_domain::ContentDigest("a".repeat(64)),
                kind: "registered_stamp_channels".into(),
            },
            scope: hot_trimmer_effect_compiler::StampScope::AssetSpecificDeferred,
            target_region: "20152016-0000-4000-8000-000000000004".into(),
            physical_position_m: [0.1, 0.2],
            physical_size_m: [0.03, 0.04],
            pivot: [0.5, 0.5],
            rotation_degrees: 30.0,
            mirror: [false, true],
            opacity: 0.75,
            blend: hot_trimmer_effect_compiler::StampBlendPolicy::Add,
            clipping: hot_trimmer_effect_compiler::DetailFitPolicy::Contain,
            seed: 20,
            spacing_m: [0.01, 0.01],
            scatter: 0.1,
            jitter_m: [0.001, 0.002],
            layer_order: 3,
            occupancy: hot_trimmer_effect_compiler::OccupancyRelation::OnlyFlatCenter,
            channels: Vec::new(),
        });
        let decoration = feedback_decoration(
            "20152016-0000-4000-8000-000000000005",
            false,
            &intent,
        )
        .expect("typed decoration");
        assert!(decoration.decoration_key.starts_with("stage16.disabled.stamp.operation."));
        assert!(!decoration.value.contains("decoration_key"));
        assert!(matches!(parse_feedback_detail(&decoration), Ok(FeedbackDetailIntent::Operation(_))));
    }

    #[test]
    fn algorithm_stage_20a_feedback_workbench_sample_channels_are_distinct_and_content_addressed() {
        let root = std::env::temp_dir().join(format!("hot-trimmer-feedback-sample-{}", Uuid::new_v4()));
        let mut session = test_session(&root);
        let service = PreviewService::default();
        let projection = create_stage_15_16_feedback_sample_impl(&mut session, &service)
            .expect("bundled sample must be creatable from an empty draft");
        let material = projection.material_sources.iter()
            .find(|material| material.registered_channels.is_some())
            .expect("sample material source set");
        let channels = &material.registered_channels.as_ref().expect("registered channels").channels;
        let required = [SourceChannel::BaseColor, SourceChannel::Height, SourceChannel::MaterialId]
            .map(|role| channels.iter().find(|channel| channel.channel == role).expect("required sample channel"));
        assert_eq!(required.iter().map(|channel| channel.id.as_str()).collect::<BTreeSet<_>>().len(), 3);
        assert_eq!(required.iter().map(|channel| channel.original.immutable_digest.as_str()).collect::<BTreeSet<_>>().len(), 1);
        let records = &projection.feedback_authoring.records;
        let asset = records.iter().find_map(|record| match &record.intent {
            FeedbackDetailIntent::Operation(operation) => Some(&operation.asset),
            _ => None,
        }).expect("sample operation asset");
        assert_eq!(asset.asset_id, material.id);
        assert_eq!(asset.version, material.source_revision.to_string());
        assert_eq!(asset.digest.0, required[0].original.immutable_digest);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn algorithm_stage_20a_feedback_workbench_request_identity_and_real_cache_reuse() {
        let root = std::env::temp_dir().join(format!("hot-trimmer-feedback-cache-{}", Uuid::new_v4()));
        let mut project_session = test_session(&root);
        let service = PreviewService::default();
        let projection = create_stage_15_16_feedback_sample_impl(&mut project_session, &service).expect("sample");
        let document = projection.document.as_ref().expect("sample document");
        let region_id = document.topology.regions[0].id;
        let operation_id = projection.feedback_authoring.records.iter()
            .find(|record| matches!(&record.intent, FeedbackDetailIntent::Operation(_)))
            .expect("sample operation").operation_id.clone();
        let before = feedback_request(document.document_revision, region_id, FeedbackComparisonMode::Before, None);
        let after = feedback_request(document.document_revision, region_id, FeedbackComparisonMode::After, None);
        assert_ne!(
            feedback_preview_cache_identity(&before, MaterialMapKind::EdgeMask).unwrap(),
            feedback_preview_cache_identity(&after, MaterialMapKind::EdgeMask).unwrap(),
        );
        let isolated_a = feedback_request(document.document_revision, region_id, FeedbackComparisonMode::SelectedOperationIsolation, Some(&operation_id));
        let isolated_b = feedback_request(document.document_revision, region_id, FeedbackComparisonMode::SelectedOperationIsolation, Some("20152016-0000-4000-8000-000000000099"));
        assert_ne!(
            feedback_preview_cache_identity(&isolated_a, MaterialMapKind::EdgeMask).unwrap(),
            feedback_preview_cache_identity(&isolated_b, MaterialMapKind::EdgeMask).unwrap(),
        );
        assert_eq!(
            feedback_preview_cache_identity(&isolated_a, MaterialMapKind::EdgeMask).unwrap(),
            feedback_preview_cache_identity(&isolated_a, MaterialMapKind::EdgeMask).unwrap(),
        );
        let session = Arc::new(Mutex::new(project_session));
        let first = run_feedback_preview(&session, &service, &isolated_a).expect("first isolated GPU request");
        let repeated = run_feedback_preview(&session, &service, &isolated_a).expect("identical isolated GPU request");
        let first_execution = first.feedback_execution.expect("first execution identity");
        let repeated_execution = repeated.feedback_execution.expect("repeated execution identity");
        assert_eq!(first_execution.request_identity, repeated_execution.request_identity);
        assert!(repeated_execution.cache_reused);
        assert!(matches!(repeated_execution.outcome, FeedbackExecutionState::CacheHit));
        let published = repeated.tile_manifests.get("edgeMask").or(repeated.tile_manifest.as_ref()).expect("edge-mask tile");
        assert_eq!(repeated_execution.published_generation, published.manifest.generation);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn algorithm_stage_20a_feedback_workbench_mask_route_and_occupancy_are_typed() {
        assert_eq!(feedback_map_for_view(FeedbackContributionView::Stage16RegisteredMask).unwrap(), Some(MaterialMapKind::EdgeMask));
        let mut channels = vec![hot_trimmer_effect_compiler::DetailChannelContribution {
            channel: MaterialChannelRole::Height,
            amount: 0.25,
            blend: hot_trimmer_effect_compiler::StampBlendPolicy::Add,
            material_id: None,
            metallic_explicit: false,
        }];
        rewrite_feedback_channels(&mut channels, FeedbackContributionView::Stage16RegisteredMask);
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].channel, MaterialChannelRole::EdgeMask);
        assert!(channels.iter().all(|channel| channel.channel != MaterialChannelRole::BaseColor));
        for value in ["above_profile", "below_profile", "avoid_raised", "only_flat_center", "ignore"] {
            assert!(serde_json::from_str::<hot_trimmer_effect_compiler::OccupancyRelation>(&format!("\"{value}\"")).is_ok());
        }
        assert!(serde_json::from_str::<hot_trimmer_effect_compiler::OccupancyRelation>("\"unknown\"").is_err());
    }

    #[test]
    fn algorithm_stage_20a_feedback_workbench_normal_convention_survives_projection_and_preparation() {
        let root = std::env::temp_dir().join(format!("hot-trimmer-feedback-normal-{}", Uuid::new_v4()));
        let mut session = test_session(&root);
        let source_set_id = Uuid::new_v4();
        let base_bytes = solid_png([128, 128, 128, 255]);
        let normal_bytes = solid_png([128, 64, 255, 255]);
        let base_inspected = inspect_bytes_with_policy(base_bytes, DecodeLimits::default(), ColorPolicy::ConvertToSrgb).unwrap();
        let normal_inspected = inspect_bytes_with_policy(normal_bytes, DecodeLimits::default(), ColorPolicy::PreserveLinearData).unwrap();
        let base = source_input(Path::new("normal-test-base.png"), SourceOwnership::OwnedCopy, &base_inspected);
        let normal = source_input(Path::new("normal-test-directx.png"), SourceOwnership::OwnedCopy, &normal_inspected);
        let store = session.store.as_mut().expect("store");
        store.replace_registered_source_in_set(source_set_id, &base, ChannelRegistration {
            role: MaterialChannelRole::BaseColor,
            interpretation: MaterialChannelRole::BaseColor.required_interpretation(),
            normal_convention: NormalConvention::NotApplicable,
            assignment_provenance: AssignmentProvenance::UserAssigned,
            confidence_milli: 1000,
        }).unwrap();
        store.replace_registered_source_in_set(source_set_id, &normal, ChannelRegistration {
            role: MaterialChannelRole::Normal,
            interpretation: MaterialChannelRole::Normal.required_interpretation(),
            normal_convention: NormalConvention::DirectX,
            assignment_provenance: AssignmentProvenance::UserAssigned,
            confidence_milli: 1000,
        }).unwrap();
        let summary = store.summary().unwrap();
        let stored_normal = summary.sources.iter().find(|source| source.channel == SourceChannel::Normal).unwrap();
        assert_eq!(session.source_projection_cached(stored_normal).unwrap().normal_convention, NormalConvention::DirectX);
        let (registered, encoded) = preview_registered_channel_set(&session, &summary.sources, source_set_id).unwrap();
        assert_eq!(registered.channels.iter().find(|channel| channel.registration.role == MaterialChannelRole::Normal).unwrap().registration.normal_convention, NormalConvention::DirectX);
        let prepared = prepare_registered_channel_set(&registered, &encoded, &NormalizationSettings::default(), &CancellationToken::new()).unwrap();
        let prepared_normal = prepared.channels.iter().find_map(|channel| match channel {
            hot_trimmer_image_io::PreparedChannel::Normal { source_convention, canonical_convention, .. } => Some((*source_convention, *canonical_convention)),
            _ => None,
        }).expect("prepared normal channel");
        assert_eq!(prepared_normal, (NormalConvention::DirectX, NormalConvention::OpenGl));
        let _ = fs::remove_dir_all(root);
    }
}

#[cfg(any())]
mod algorithm_stage_14_preview_a_tests {
    use std::collections::BTreeMap;

    use hot_trimmer_domain::{
        AlgorithmProvenance, CancellationToken, CanonicalRect, CompiledTemplateSlot,
        CompiledTemplateTopology, ContentDigest, MaterialChannelRole, NormalConvention, PixelSize,
        QuarterTurn, RegionId, SamplingMode, SamplingPolicy, SourceSamplingMode, StageResult,
        TemplateIdentity, TemplateSlotRole,
    };
    use hot_trimmer_image_io::{ImagePlane, LinearColor, NormalAlphaPolicy, ResolvedAlphaMode};
    use hot_trimmer_material_synthesis::PreparedMaterialDomain;
    use hot_trimmer_placement_solver::{
        CandidateDescriptors, CandidateFamily, CandidateRoute, CandidateTransform, CropCandidate,
        EligibilityEvidence, MirrorTransform, PlacementObjectiveBreakdown, PlacementPlan,
        PlacementPlanQaView, PlacementValidationSummary, PositionStrategy, SamplingPlan,
        SliceGeometry, SourceCrop, StretchOverrideProvenance,
    };
    use hot_trimmer_render_core::{PreparedExemplarChannel, RenderCancellationToken};
    use hot_trimmer_sheet_compiler::{
        AlgorithmCompiler, IntermediateAtlasError, IntermediateAtlasRequest, IntermediateSlotInput,
        SlotSynthesisLimits, SlotSynthesisRequest, synthesize_slot_material,
    };

    fn domain(marker: f32, name: &[u8]) -> PreparedMaterialDomain {
        let colors = vec![
            LinearColor {
                rgb: [marker, marker * 0.5, 0.25],
                alpha: 1.0
            };
            16
        ];
        let channel = PreparedExemplarChannel::BaseColor {
            plane: ImagePlane::from_row_major(4, 4, 4, &colors).unwrap(),
            alpha_mode: ResolvedAlphaMode::Opaque,
        };
        PreparedMaterialDomain::from_registered_channels(
            ContentDigest::sha256([name, b"-domain"].concat().as_slice()),
            ContentDigest::sha256([name, b"-source"].concat().as_slice()),
            vec![channel],
        )
        .unwrap()
    }

    fn plan(id: u8, domain: &PreparedMaterialDomain) -> SamplingPlan {
        let slot_id = RegionId::from_bytes([id; 16]);
        SamplingPlan {
            slot_id,
            role: TemplateSlotRole::Planar,
            variation_group: format!("slot-{id}"),
            prepared_domain_dimensions: [4, 4],
            candidate: CropCandidate {
                candidate_id: ContentDigest::sha256(&[id, 1]),
                source_id: domain.prepared_source_digest.clone(),
                domain_id: domain.cache_key.clone(),
                slot_id,
                crop: Some(SourceCrop {
                    x: 0,
                    y: 0,
                    width: 4,
                    height: 4,
                }),
                transform: CandidateTransform {
                    rotation: QuarterTurn::Zero,
                    mirror: MirrorTransform::None,
                },
                isotropic_scale: 1.0,
                mapping_mode: SamplingMode::DirectCrop,
                family: CandidateFamily::PanelDirect,
                route: CandidateRoute::Direct,
                position_strategy: PositionStrategy::FeatureAware,
                period_pixels: None,
                seam_indices: Vec::new(),
                correspondence_reference: domain.cache_key.clone(),
                descriptors: CandidateDescriptors {
                    saliency_milli: 0,
                    stationarity_milli: 1000,
                    feature_strength_milli: 0,
                    usability_milli: 1000,
                },
                seed: 14,
                eligibility: EligibilityEvidence {
                    mapping_permitted: true,
                    transform_permitted: true,
                    isotropic_scale: true,
                    exact_aspect: true,
                    entire_crop_usable: Some(true),
                    cross_axis_preserved: None,
                    lattice_aligned: None,
                    direct_crop_applicable: true,
                    direct_crop_rejection: None,
                    reasons: Vec::new(),
                },
            },
            sampling_basis: hot_trimmer_placement_solver::SamplingBasis::SelectedCrop,
            slot_physical_size: [1.0, 1.0],
            source_pixels_per_physical_unit: 4.0,
            sampling_policy: SamplingPolicy {
                filter: SourceSamplingMode::Nearest,
                scale: 1.0,
                correct_tangent_normals: true,
            },
            stretch_override: StretchOverrideProvenance::NotAuthorized,
            slice_geometry: SliceGeometry::None,
            maximum_seam_cost_milli: 450,
            unary_cost: 0.0,
        }
    }

    fn placement(plans: Vec<SamplingPlan>) -> PlacementPlan {
        PlacementPlan {
            stage_result: StageResult::Executed {
                algorithm: AlgorithmProvenance {
                    algorithm_id: "hot-trimmer.stage-13.global-placement".into(),
                    version: "1.0.0".into(),
                },
                settings_hash: ContentDigest::sha256(b"preview-placement"),
                diagnostics: Vec::new(),
            },
            solver: AlgorithmProvenance {
                algorithm_id: "hot-trimmer.stage-13.global-placement".into(),
                version: "1.0.0".into(),
            },
            seed: 14,
            placements: plans,
            objective: PlacementObjectiveBreakdown {
                unary_cost: 0.0,
                pairwise_cost: 0.0,
                pairwise_lambda: 1.0,
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
                slot_count: 2,
            },
            qa_views: vec![
                PlacementPlanQaView::SelectedPlacements,
                PlacementPlanQaView::Validation,
            ],
        }
    }

    #[test]
    fn algorithm_stage_14_preview_a() {
        let first_domain = domain(0.2, b"persisted-authored-patch-a");
        let second_domain = domain(0.8, b"persisted-authored-patch-b");
        let first_plan = plan(1, &first_domain);
        let second_plan = plan(2, &second_domain);
        let first = synthesize_slot_material(
            SlotSynthesisRequest {
                plan: &first_plan,
                domain: &first_domain,
                output_dimensions: [4, 4],
                limits: SlotSynthesisLimits::default(),
            },
            &RenderCancellationToken::new(),
        )
        .unwrap();
        let second = synthesize_slot_material(
            SlotSynthesisRequest {
                plan: &second_plan,
                domain: &second_domain,
                output_dimensions: [4, 4],
                limits: SlotSynthesisLimits::default(),
            },
            &RenderCancellationToken::new(),
        )
        .unwrap();
        let topology = CompiledTemplateTopology {
            identity: TemplateIdentity {
                template_id: "representative-persisted".into(),
                template_version: "1.0.0".into(),
                compatibility_key: "stage-14-preview-a".into(),
            },
            output_size: PixelSize {
                width: 8,
                height: 4,
            },
            slots: vec![
                CompiledTemplateSlot {
                    slot_key: "authored-patch-a".into(),
                    allocation: CanonicalRect {
                        x: 0,
                        y: 0,
                        width: 4,
                        height: 4,
                    },
                    hotspot: CanonicalRect {
                        x: 1,
                        y: 1,
                        width: 2,
                        height: 2,
                    },
                },
                CompiledTemplateSlot {
                    slot_key: "authored-patch-b".into(),
                    allocation: CanonicalRect {
                        x: 4,
                        y: 0,
                        width: 4,
                        height: 4,
                    },
                    hotspot: CanonicalRect {
                        x: 5,
                        y: 1,
                        width: 2,
                        height: 2,
                    },
                },
            ],
        };
        let placement = placement(vec![first_plan.clone(), second_plan.clone()]);
        let algorithms = (1..=14)
            .map(|stage| {
                (
                    stage,
                    AlgorithmProvenance {
                        algorithm_id: format!("installed-stage-{stage}"),
                        version: "1.0.0".into(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let request = IntermediateAtlasRequest {
            topology: &topology,
            placement_plan: &placement,
            slots: vec![
                IntermediateSlotInput {
                    region_id: first_plan.slot_id,
                    slot_key: "authored-patch-a",
                    display_name: "Authored Patch A",
                    required: true,
                    patch_id: Some("patch-a".into()),
                    domain: &first_domain,
                    plan: &first_plan,
                    result: &first,
                    grid_rect: None,
                    behavior: hot_trimmer_domain::RegionBehavior::default(),
                },
                IntermediateSlotInput {
                    region_id: second_plan.slot_id,
                    slot_key: "authored-patch-b",
                    display_name: "Authored Patch B",
                    required: true,
                    patch_id: Some("patch-b".into()),
                    domain: &second_domain,
                    plan: &second_plan,
                    result: &second,
                    grid_rect: None,
                    behavior: hot_trimmer_domain::RegionBehavior::default(),
                },
            ],
            revision: 7,
            algorithm_versions: algorithms,
            diagnostics: Vec::new(),
            regions: Vec::new(),
        };
        let compiler = AlgorithmCompiler::new();
        let artifact = compiler
            .compile_intermediate_atlas(&request, &CancellationToken::new(), || 7)
            .unwrap();
        assert_eq!(artifact.topology, topology);
        assert_eq!(artifact.incomplete_after_stage, 14);
        assert!(artifact.non_exportable);
        assert_eq!(artifact.slots.len(), 2);
        assert!(artifact.slots.iter().all(|slot| slot.patch_id.is_some()
            && slot.valid_pixel_count == 16
            && slot.mapping_mode == SamplingMode::DirectCrop));
        assert_eq!(artifact.channels.len(), 1);
        assert_eq!(artifact.channels[0].role, MaterialChannelRole::BaseColor);
        assert!(
            artifact
                .unavailable_channels
                .contains(&MaterialChannelRole::Height)
        );
        assert!(artifact.pending.contains(&"final PBR composition"));
        assert_ne!(
            &artifact.channels[0].rgba8[0..4],
            &artifact.channels[0].rgba8[4 * 4..4 * 4 + 4]
        );

        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert!(matches!(
            compiler.compile_intermediate_atlas(&request, &cancelled, || 7),
            Err(
                hot_trimmer_sheet_compiler::CompilerFacadeError::Intermediate(
                    IntermediateAtlasError::Cancelled
                )
            )
        ));
        assert!(matches!(
            compiler.compile_intermediate_atlas(&request, &CancellationToken::new(), || 8),
            Err(
                hot_trimmer_sheet_compiler::CompilerFacadeError::Intermediate(
                    IntermediateAtlasError::RevisionSuperseded
                )
            )
        ));

        let mut failed_placement = placement.clone();
        failed_placement.validation.required_slots_present = false;
        let failed = IntermediateAtlasRequest {
            placement_plan: &failed_placement,
            ..request
        };
        assert!(
            compiler
                .compile_intermediate_atlas(&failed, &CancellationToken::new(), || 7)
                .is_err()
        );

        let _normal_semantics_are_registered_not_color = NormalConvention::OpenGl;
        let _normal_alpha_is_preserved = NormalAlphaPolicy::Preserve;
    }
}

#[cfg(test)]
mod persisted_algorithm_stage_14_preview_a_tests {
    use std::{
        collections::{BTreeMap, HashSet},
        io::Cursor,
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    use hot_trimmer_domain::{
        Channel, ChannelBitDepth, ContentDigest, ContentReference, ManualRegionRole,
        MaterialBehaviorClass, MaterialClassificationCommand, NormalizedPoint, Patch, PatchCommand,
        PatchGeometry, PatchId, PatchProperties, PixelSize, QuarterTurn, RectificationSettings,
        RegionBehavior, RegionSampling, SourceId, TrimSheetDocumentCommand,
    };
    use hot_trimmer_project_store::{ProjectStore, SourceChannel, SourceInput, SourceOwnership};
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
    use uuid::Uuid;

    use super::*;

    fn encoded_source() -> Vec<u8> {
        let image = RgbaImage::from_fn(192, 128, |x, y| {
            let course = (y / 12) % 2;
            let joint = ((x + course * 18) / 36) % 2;
            Rgba([
                120 + (x % 70) as u8,
                45 + (y % 35) as u8,
                30 + (joint * 25) as u8,
                255,
            ])
        });
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    fn native_export_manifest(map: MaterialMapKind) -> CompiledAtlasTileManifest {
        let output_rect = OutputPixelRect(PixelBounds {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        });
        let pixel_format = native_export_source_pixel_format(map);
        let identity = hot_trimmer_sheet_compiler::CompiledAtlasTileIdentity {
            plan_hash: ContentDigest::sha256(b"native-export-plan"),
            map,
            pixel_format,
            mip_level: 0,
            output_rect,
            halo_px: 0,
            valid_rect: output_rect,
            profile: hot_trimmer_sheet_compiler::CompiledAtlasPreviewProfile::Authoritative,
            shader_version: "native-export-test".into(),
            structural_plan_id: ContentDigest::sha256(b"native-export-structure"),
            scale_microunits: 1_000_000,
            normal_convention: None,
            generation: 7,
        };
        CompiledAtlasTileManifest {
            identity,
            map,
            mip_level: 0,
            output_rect,
            valid_rect: output_rect,
            halo_px: 0,
            generation: 7,
            pixel_format,
            width: 2,
            height: 2,
            row_stride: 8,
            opaque_handle: "native-export-test".into(),
        }
    }

    fn native_export_test_document(root: &Path) -> TrimSheetDocument {
        let project_path = root.join(format!("export-doc-{}.hottrimmer", Uuid::new_v4()));
        let mut store = ProjectStore::create(&project_path, "Export Package").unwrap();
        let bytes = encoded_source();
        store
            .replace_source(
                SourceChannel::BaseColor,
                &SourceInput {
                    id: SourceId::new(),
                    ownership: SourceOwnership::OwnedCopy,
                    external_path: None,
                    origin_path: PathBuf::from("export-package.png"),
                    sha256: ContentDigest::sha256(&bytes).0,
                    width: 192,
                    height: 128,
                    format: "PNG".into(),
                    color_type: "Rgba8".into(),
                    has_alpha: true,
                    exif_orientation: 1,
                    has_embedded_icc_profile: false,
                    encoded_bytes: u64::try_from(bytes.len()).unwrap(),
                    owned_bytes: Some(bytes),
                },
            )
            .unwrap();
        store.create_source_frame_document().unwrap();
        store
            .execute_document_command(&TrimSheetDocumentCommand::SetOutputResolution {
                output_size: PixelSize {
                    width: 2,
                    height: 2,
                },
            })
            .unwrap();
        let document = store.summary().unwrap().document.unwrap();
        drop(store);
        document
    }

    #[test]
    fn gpu_tiled_export_native_stage14_plans_distinct_tile_completion_ids() {
        let planned = native_export_planned_tiles(
            OutputPixelRect(PixelBounds {
                x: 0,
                y: 0,
                width: 3,
                height: 2,
            }),
            2,
            &[MaterialMapKind::BaseColor, MaterialMapKind::Height],
        );
        let ids = planned
            .iter()
            .map(|tile| tile.id.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(planned.len(), 4);
        assert_eq!(ids.len(), 4);
        assert!(ids.contains("baseColor@0,0-2x2"));
        assert!(ids.contains("height@2,0-1x2"));
        assert_eq!(native_export_unique_rects(&planned).len(), 2);
    }

    #[test]
    fn gpu_tiled_export_native_stage14_package_streams_and_preserves_stale_output() {
        let root =
            std::env::temp_dir().join(format!("hot-trimmer-native-export-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let final_path = root.join("material.hottrim");
        let document = native_export_test_document(&root);
        let revisions = RevisionAuthority::new(7);
        let cancellation = EngineCancellationToken::new();
        let manifest = native_export_manifest(MaterialMapKind::BaseColor);
        let pixels = vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let package = write_native_stage14_export_package(
            &final_path,
            7,
            revisions,
            &cancellation,
            &document,
            vec![NativeExportTile {
                map: MaterialMapKind::BaseColor,
                manifest: &manifest,
                pixels: &pixels,
                render_ms: 11,
                readback_ms: 7,
            }],
            || true,
        )
        .expect("native package writes material-map files");
        assert!(final_path.is_dir());
        let manifest_bytes = fs::read(final_path.join(HOTTRIM_MANIFEST_FILE_NAME)).unwrap();
        assert!(manifest_bytes.starts_with(b"{"));
        let png_bytes = fs::read(final_path.join("maps/base_color.png")).unwrap();
        assert!(png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(
            package.bytes_written,
            (manifest_bytes.len() + png_bytes.len()) as u64
        );
        assert_eq!(package.outputs.len(), 1);
        assert_eq!(package.outputs[0].id, "baseColor");
        assert_eq!(package.outputs[0].map, "baseColor");
        assert_eq!(package.outputs[0].file_name, "maps/base_color.png");
        assert_eq!(
            package.outputs[0].checksum,
            ContentDigest::sha256(&png_bytes).0
        );
        assert_eq!(package.outputs[0].width, 2);
        assert_eq!(package.outputs[0].height, 2);
        assert_eq!(package.progress.len(), 2);
        assert_eq!(package.progress[0].map, "baseColor");
        assert_eq!(package.progress[0].completed_tiles, 1);
        assert_eq!(package.progress[0].total_tiles, 1);
        assert_eq!(package.progress[0].render_ms, 11);
        assert_eq!(package.progress[0].readback_ms, 7);
        assert_eq!(package.progress[0].bytes_written, 0);
        assert_eq!(package.progress[1].map, "baseColor");
        assert_eq!(package.progress[1].completed_tiles, 1);
        assert_eq!(package.progress[1].total_tiles, 1);
        assert_eq!(package.progress[1].bytes_written, png_bytes.len() as u64);

        let stale_path = root.join("stale.hottrim");
        fs::write(&stale_path, b"existing").unwrap();
        let stale = write_native_stage14_export_package(
            &stale_path,
            7,
            RevisionAuthority::new(7),
            &EngineCancellationToken::new(),
            &document,
            vec![NativeExportTile {
                map: MaterialMapKind::BaseColor,
                manifest: &manifest,
                pixels: &pixels,
                render_ms: 11,
                readback_ms: 7,
            }],
            || false,
        )
        .expect_err("stale export must not publish");
        assert!(matches!(stale, TiledExportError::StaleRevision));
        assert_eq!(fs::read(&stale_path).unwrap(), b"existing");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn gpu_tiled_export_native_stage14_failed_init_removes_temp_package() {
        let root = std::env::temp_dir().join(format!(
            "hot-trimmer-native-export-init-fail-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let final_path = root.join("failed.hottrim");
        let mut document = native_export_test_document(&root);
        document
            .render_settings
            .channels
            .get_mut(&Channel::Normal)
            .expect("normal channel policy")
            .bit_depth = ChannelBitDepth::Sixteen;
        let full_rect = OutputPixelRect(PixelBounds {
            x: 0,
            y: 0,
            width: document.render_settings.output_size.width,
            height: document.render_settings.output_size.height,
        });
        let planned = native_export_planned_tiles(full_rect, 2, &[MaterialMapKind::Normal]);
        let error = match begin_native_stage14_export_package(
            &final_path,
            7,
            RevisionAuthority::new(7),
            &EngineCancellationToken::new(),
            &document,
            full_rect,
            &[MaterialMapKind::Normal],
            &planned,
        ) {
            Ok(_) => panic!("unsupported 16-bit Normal must fail during writer initialization"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            TiledExportError::UnsupportedFeatureOrFormat(_)
        ));
        let leftovers = fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".failed.hottrim.tmp-")
            })
            .collect::<Vec<_>>();
        assert!(leftovers.is_empty(), "temporary package directories leaked");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn gpu_tiled_export_native_stage14_region_id_uses_manifest_palette() {
        let root = std::env::temp_dir().join(format!(
            "hot-trimmer-native-export-region-id-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let final_path = root.join("region-id.hottrim");
        let document = native_export_test_document(&root);
        let region_color = document.topology.regions[0].id_color.0;
        let manifest = native_export_manifest(MaterialMapKind::RegionId);
        let pixels = [0_u32, u32::MAX, 0, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        let package = write_native_stage14_export_package(
            &final_path,
            7,
            RevisionAuthority::new(7),
            &EngineCancellationToken::new(),
            &document,
            vec![NativeExportTile {
                map: MaterialMapKind::RegionId,
                manifest: &manifest,
                pixels: &pixels,
                render_ms: 5,
                readback_ms: 3,
            }],
            || true,
        )
        .expect("region ID package writes from compact IDs");
        assert_eq!(package.outputs[0].file_name, "maps/region_id.png");
        let decoded =
            image::load_from_memory(&fs::read(final_path.join("maps/region_id.png")).unwrap())
                .unwrap()
                .to_rgba8();
        assert_eq!(
            decoded.get_pixel(0, 0).0,
            [region_color[0], region_color[1], region_color[2], 255]
        );
        assert_eq!(decoded.get_pixel(1, 0).0, [0, 0, 0, 0]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn gpu_tiled_export_native_stage14_rejects_fake_sixteen_bit_normal() {
        let root = std::env::temp_dir().join(format!(
            "hot-trimmer-native-export-normal-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let final_path = root.join("normal.hottrim");
        let mut document = native_export_test_document(&root);
        document
            .render_settings
            .channels
            .get_mut(&Channel::Normal)
            .expect("normal channel policy")
            .bit_depth = ChannelBitDepth::Sixteen;
        let manifest = native_export_manifest(MaterialMapKind::Normal);
        let pixels = [128, 128, 255, 255].repeat(4);
        let error = write_native_stage14_export_package(
            &final_path,
            7,
            RevisionAuthority::new(7),
            &EngineCancellationToken::new(),
            &document,
            vec![NativeExportTile {
                map: MaterialMapKind::Normal,
                manifest: &manifest,
                pixels: &pixels,
                render_ms: 5,
                readback_ms: 3,
            }],
            || true,
        )
        .expect_err("16-bit Normal must not be widened from RGBA8");
        assert!(matches!(
            error,
            TiledExportError::UnsupportedFeatureOrFormat(_)
        ));
        assert!(!final_path.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn gpu_tiled_export_native_stage14_manifest_uses_authored_behavior_metadata() {
        let root = std::env::temp_dir().join(format!(
            "hot-trimmer-native-export-manifest-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let mut document = native_export_test_document(&root);
        assert!(
            document.topology.regions.len() >= 2,
            "source-frame fixture should expose multiple authored regions"
        );
        let radial_region = document.topology.regions[0].id;
        let loop_region = document.topology.regions[1].id;
        let mut radial = RegionBehavior::new(ManualRegionRole::Radial);
        radial.radial.as_mut().unwrap().center_x = 0.25;
        radial.radial.as_mut().unwrap().center_y = 0.75;
        radial.radial.as_mut().unwrap().inner_radius = 0.1;
        radial.radial.as_mut().unwrap().outer_radius = 0.6;
        radial.radial.as_mut().unwrap().falloff = 2.0;
        radial.synchronize_derived_fields();
        {
            let binding = document
                .region_bindings
                .get_mut(&radial_region)
                .expect("radial binding");
            binding.mapping.radial = radial.radial;
            binding.mapping.behavior = radial.clone();
        }
        let mut repeat = RegionBehavior::default();
        repeat.sampling = RegionSampling::LoopXy;
        repeat.period_pixels = Some([7, 11]);
        repeat.orientation = QuarterTurn::Ninety;
        repeat.synchronize_derived_fields();
        document
            .region_bindings
            .get_mut(&loop_region)
            .expect("loop binding")
            .mapping
            .behavior = repeat.clone();

        let manifest =
            native_export_manifest_from_document(&document, BTreeMap::new()).expect("manifest");
        let radial_slot = manifest
            .slots
            .iter()
            .find(|slot| slot.region_id == radial_region.to_string())
            .expect("radial slot");
        assert_eq!(radial_slot.uv_fit.kind, UvFitKind::Radial);
        assert_eq!(radial_slot.behavior_role.as_deref(), Some("radial"));
        assert_eq!(radial_slot.radial_parameters.unwrap().center_x, 0.25);
        assert_eq!(radial_slot.radial_mapping.unwrap().falloff, 2.0);
        let loop_slot = manifest
            .slots
            .iter()
            .find(|slot| slot.region_id == loop_region.to_string())
            .expect("loop slot");
        assert_eq!(loop_slot.sampling.as_deref(), Some("loop_xy"));
        assert_eq!(loop_slot.repeat_period_pixels, Some([7, 11]));
        assert_eq!(loop_slot.orientation.as_deref(), Some("ninety"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn gpu_tiled_export_native_stage14_empty_request_uses_enabled_exportable_maps() {
        let root =
            std::env::temp_dir().join(format!("hot-trimmer-native-export-maps-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let project_path = root.join("export-maps.hottrimmer");
        let mut store = ProjectStore::create(&project_path, "Export Maps").unwrap();
        let bytes = encoded_source();
        store
            .replace_source(
                SourceChannel::BaseColor,
                &SourceInput {
                    id: SourceId::new(),
                    ownership: SourceOwnership::OwnedCopy,
                    external_path: None,
                    origin_path: PathBuf::from("export-maps.png"),
                    sha256: ContentDigest::sha256(&bytes).0,
                    width: 192,
                    height: 128,
                    format: "PNG".into(),
                    color_type: "Rgba8".into(),
                    has_alpha: true,
                    exif_orientation: 1,
                    has_embedded_icc_profile: false,
                    encoded_bytes: u64::try_from(bytes.len()).unwrap(),
                    owned_bytes: Some(bytes),
                },
            )
            .unwrap();
        store.create_source_frame_document().unwrap();
        let document = store.summary().unwrap().document.unwrap();
        assert_eq!(
            document.render_settings.channels[&Channel::Normal].bit_depth,
            ChannelBitDepth::Eight
        );

        assert_eq!(
            native_export_enabled_maps(&document),
            vec![
                MaterialMapKind::BaseColor,
                MaterialMapKind::Height,
                MaterialMapKind::Normal,
                MaterialMapKind::Roughness,
                MaterialMapKind::Metallic,
                MaterialMapKind::AmbientOcclusion,
                MaterialMapKind::RegionId,
            ]
        );
        drop(document);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn algorithm_stage_14_preview_a() {
        let root = std::env::temp_dir().join(format!("hot-trimmer-preview-a-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let project_path = root.join("persisted.hottrimmer");
        let mut store = ProjectStore::create(&project_path, "Persisted Stage 14").unwrap();
        let supplied_path = std::env::var_os("HOT_TRIMMER_STAGE14_SOURCE").map(PathBuf::from);
        let bytes = supplied_path
            .as_ref()
            .map_or_else(encoded_source, |path| fs::read(path).unwrap());
        let decoded = image::load_from_memory(&bytes).unwrap();
        let source_width = decoded.width();
        let source_height = decoded.height();
        let base_color_source_id = SourceId::new();
        store
            .replace_source(
                SourceChannel::BaseColor,
                &SourceInput {
                    id: base_color_source_id,
                    ownership: SourceOwnership::OwnedCopy,
                    external_path: None,
                    origin_path: supplied_path
                        .clone()
                        .unwrap_or_else(|| PathBuf::from("authored-brick.png")),
                    sha256: ContentDigest::sha256(&bytes).0,
                    width: source_width,
                    height: source_height,
                    format: if supplied_path.is_some() {
                        "JPEG".into()
                    } else {
                        "PNG".into()
                    },
                    color_type: if supplied_path.is_some() {
                        "Rgb8".into()
                    } else {
                        "Rgba8".into()
                    },
                    has_alpha: supplied_path.is_none(),
                    exif_orientation: 1,
                    has_embedded_icc_profile: false,
                    encoded_bytes: u64::try_from(bytes.len()).unwrap(),
                    owned_bytes: Some(bytes),
                },
            )
            .unwrap();
        let patch_source_id = if supplied_path.is_some() {
            base_color_source_id
        } else {
            let roughness_bytes = encoded_source();
            let patch_source_id = SourceId::new();
            store
                .replace_source(
                    SourceChannel::Roughness,
                    &SourceInput {
                        id: patch_source_id,
                        ownership: SourceOwnership::OwnedCopy,
                        external_path: None,
                        origin_path: PathBuf::from("authored-brick-roughness.png"),
                        sha256: ContentDigest::sha256(&roughness_bytes).0,
                        width: 192,
                        height: 128,
                        format: "PNG".into(),
                        color_type: "Rgba8".into(),
                        has_alpha: true,
                        exif_orientation: 1,
                        has_embedded_icc_profile: false,
                        encoded_bytes: u64::try_from(roughness_bytes.len()).unwrap(),
                        owned_bytes: Some(roughness_bytes),
                    },
                )
                .unwrap();
            patch_source_id
        };
        let primary = store.summary().unwrap().source_sets[0].id;
        store
            .apply_material_classification_command(
                Uuid::from_bytes(primary.to_bytes()),
                MaterialClassificationCommand::Override {
                    class: MaterialBehaviorClass::AlreadyTileable,
                },
            )
            .unwrap();
        let point = |x, y| NormalizedPoint::new(x, y).unwrap();
        let patch = Patch {
            id: PatchId::new(),
            source_id: patch_source_id,
            name: "Authored brick field".into(),
            enabled: true,
            geometry: PatchGeometry {
                corners: [
                    point(0.05, 0.05),
                    point(0.95, 0.05),
                    point(0.95, 0.95),
                    point(0.05, 0.95),
                ],
                assistance_mask: None,
            },
            properties: PatchProperties::default(),
            rectification: RectificationSettings::default(),
        };
        store
            .execute_patch_command(
                &PatchCommand::Create {
                    patch: patch.clone(),
                    index: None,
                },
                None,
            )
            .unwrap();
        store
            .create_trim_sheet_document("ht.generic_architecture", "1.0.0")
            .unwrap();
        let patch_region_id = store.summary().unwrap().document.unwrap().topology.regions[0].id;
        store
            .execute_document_command(&TrimSheetDocumentCommand::SetRegionContent {
                region_id: patch_region_id,
                content: ContentReference::Patch(patch.id),
            })
            .unwrap();
        store
            .execute_document_command(&TrimSheetDocumentCommand::SetOutputResolution {
                output_size: PixelSize {
                    width: 128,
                    height: 128,
                },
            })
            .unwrap();
        let revision = store.summary().unwrap().document.unwrap().document_revision;
        let session: SharedProjectSession = Arc::new(Mutex::new(ProjectSession {
            store: Some(store),
            dirty: false,
            is_draft: false,
            baseline: None,
            app_data_dir: root.join("app"),
            recovery_dir: root.join("recovery"),
            draft_dir: root.join("draft"),
            source_projection_cache: Mutex::new(HashMap::new()),
            preview_prepared_sources: HashMap::new(),
            prepared_exemplars: PreparedExemplarCache::default(),
            source_analysis_cache: SourceAnalysisCache::default(),
            scale_orientation_cache: ScaleOrientationCache::default(),
        }));
        let service = PreviewService::default();
        let job = service
            .latest_draft_id
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        let artifact = build_stage_14_preview(
            &session,
            &service,
            Stage14PreviewRequest {
                protocol_version: IPC_PROTOCOL_VERSION,
                revision,
                region_id: None,
                transient_projection: None,
                draft_id: Some(job),
                input_hash: None,
                profile: PreviewProfile::Authoritative,
                view_intent: None,
                viewport_rect: None,
                requested_maps: Vec::new(),
                candidate_recipe: None,
                feedback_view: None,
                feedback_comparison_mode: None,
                feedback_selected_operation_id: None,
            },
            job,
        )
        .expect("persisted Stage 1-14 preview");
        assert_eq!(
            artifact.slots.len(),
            53,
            "Generic Architecture must publish every fixed-template slot"
        );
        assert_eq!(
            artifact
                .slots
                .iter()
                .map(|slot| slot.region_id)
                .collect::<HashSet<_>>()
                .len(),
            53,
            "fixed-template region identities must be stable and unique"
        );
        assert!(
            artifact
                .slots
                .iter()
                .all(|slot| slot.mapping_mode != "TextureSynthesis"
                    && !slot.candidate_id.is_empty()
                    && !slot.sampling_plan_id.is_empty()
                    && !slot.stage_14_result_id.is_empty()
                    && slot.isotropic_scale.is_finite()
                    && slot.isotropic_scale > 0.0
                    && slot.sampling_scale.is_finite()
                    && slot.sampling_scale > 0.0),
            "unsupported synthesis candidates must not reach the published artifact"
        );
        for (index, left) in artifact.slots.iter().enumerate() {
            for right in &artifact.slots[index + 1..] {
                let separated = left.allocation_bounds.x + left.allocation_bounds.width
                    <= right.allocation_bounds.x
                    || right.allocation_bounds.x + right.allocation_bounds.width
                        <= left.allocation_bounds.x
                    || left.allocation_bounds.y + left.allocation_bounds.height
                        <= right.allocation_bounds.y
                    || right.allocation_bounds.y + right.allocation_bounds.height
                        <= left.allocation_bounds.y;
                assert!(separated, "fixed-template atlas allocations overlap");
            }
        }
        assert_eq!(artifact.incomplete_after_stage, 14);
        assert!(
            artifact.non_exportable
                && !artifact.final_compile_available
                && !artifact.export_available
                && !artifact.blender_available
        );
        let expected_patch_id = patch.id.to_string();
        assert_eq!(
            artifact
                .slots
                .iter()
                .find(|slot| slot.region_id == patch_region_id)
                .unwrap()
                .patch_id
                .as_deref(),
            Some(expected_patch_id.as_str())
        );
        assert!(
            artifact
                .slots
                .iter()
                .filter(|slot| slot.region_id != patch_region_id)
                .all(|slot| slot.patch_id.is_none()),
            "inherited material regions must not claim patch lineage"
        );
        assert!(
            !artifact.maps.contains_key("baseColor"),
            "fixed-template GPU Stage 14 must not publish the CPU PNG fallback map"
        );
        assert!(
            artifact.tile_manifest.is_some()
                && artifact.tile_manifests.contains_key("baseColor")
                && artifact
                    .telemetry
                    .iter()
                    .any(|entry| entry.contains("executor=gpu")
                        && entry.contains("cpu_stage14_calls=0")),
            "fixed-template Stage 14 must publish through the GPU tile path"
        );
        let crops = artifact
            .slots
            .iter()
            .filter_map(|slot| slot.source_crop)
            .map(|crop| (crop.x, crop.y, crop.width, crop.height))
            .collect::<HashSet<_>>();
        assert!(
            crops.len() > 8,
            "Stage 11-13 must select purposeful spatially distinct crops across the persisted topology"
        );
        let unplaced_crops = artifact
            .slots
            .iter()
            .filter(|slot| slot.region_id != patch_region_id)
            .filter_map(|slot| slot.source_crop)
            .collect::<Vec<_>>();
        assert!(
            unplaced_crops
                .iter()
                .all(|crop| crop.width < source_width || crop.height < source_height),
            "unplaced Gate 1 slots must not consume the complete prepared source domain"
        );
        let cornice = artifact
            .slots
            .iter()
            .find(|slot| slot.slot_key == "cornice_long")
            .expect("Generic Architecture cornice slot");
        let cornice_crop = cornice
            .source_crop
            .expect("cornice must carry a selected crop");
        assert!(
            cornice_crop.width > 0
                && cornice_crop.height > 0
                && cornice.mapping_mode != "TextureSynthesis",
            "long strips must carry a selected executable source crop"
        );
        let detail = artifact
            .slots
            .iter()
            .find(|slot| slot.slot_key == "detail_cell_a")
            .expect("Generic Architecture detail slot");
        let detail_crop = detail
            .source_crop
            .expect("detail must carry a selected crop");
        assert!(
            detail_crop.width < source_width && detail_crop.height < source_height,
            "detail slots must carry a selected source detail, not the full source sheet"
        );
        let radial = artifact
            .slots
            .iter()
            .find(|slot| slot.slot_key == "radial_fixture_a")
            .expect("Generic Architecture radial slot");
        if let Some(crop) = radial.source_crop {
            let fabricated_width = source_width.saturating_mul(3) / 4;
            let fabricated_height = source_height.saturating_mul(3) / 4;
            let fabricated_x = (source_width.saturating_sub(fabricated_width)) / 2;
            let fabricated_y = (source_height.saturating_sub(fabricated_height)) / 2;
            assert_ne!(
                (crop.x, crop.y, crop.width, crop.height),
                (
                    fabricated_x,
                    fabricated_y,
                    fabricated_width,
                    fabricated_height
                ),
                "radial slots must not publish the old invented centered crop as authoritative Stage 11-13 data"
            );
        }
        assert!(
            artifact
                .slots
                .iter()
                .all(|slot| !slot.candidate_id.is_empty()
                    && !slot.sampling_plan_id.is_empty()
                    && !slot.stage_14_result_id.is_empty())
        );
        drop(session);
        let _ = fs::remove_dir_all(root);
    }
}

fn remember_open_project(session: &ProjectSession) -> Result<(), UserFacingError> {
    if session.is_draft {
        return Ok(());
    }
    let summary = session
        .store
        .as_ref()
        .ok_or_else(no_project)?
        .summary()
        .map_err(store_error)?;
    let mut recent = read_recent_projects(&session.app_data_dir)?;
    recent.retain(|entry| Path::new(&entry.path) != summary.path);
    recent.insert(
        0,
        RecentProject {
            name: summary.name,
            path: summary.path.display().to_string(),
            last_opened_unix: now_unix(),
            available: true,
        },
    );
    recent.truncate(MAX_RECENT_PROJECTS);
    write_recent_projects(&session.app_data_dir, &recent)
}

fn remember_open_project_best_effort(session: &ProjectSession) {
    if let Err(error_value) = remember_open_project(session) {
        tracing::warn!(
            code = ?error_value.code,
            message = %error_value.message,
            "recent project list update failed"
        );
    }
}

fn read_authored_layout_preset_library(
    paths: &AppPaths,
) -> Result<AuthoredLayoutPresetLibrary, UserFacingError> {
    let path = paths.app_data.join(USER_LAYOUT_PRESET_LIBRARY_FILE);
    if !path.exists() {
        return Ok(AuthoredLayoutPresetLibrary {
            schema_version: 1,
            presets: Vec::new(),
        });
    }
    let bytes = fs::read(&path).map_err(|reason| {
        error(
            ErrorCode::Internal,
            &format!("Could not read saved layout presets: {reason}"),
        )
    })?;
    let library: AuthoredLayoutPresetLibrary =
        serde_json::from_slice(&bytes).map_err(|reason| {
            error(
                ErrorCode::InvalidInput,
                &format!("Saved layout preset library is invalid: {reason}"),
            )
        })?;
    if library.schema_version != 1 || library.presets.len() > 1_024 {
        return Err(error(
            ErrorCode::InvalidInput,
            "Saved layout preset library has an unsupported version or size.",
        ));
    }
    for preset in &library.presets {
        if preset.preset_id.starts_with("builtin.") {
            return Err(error(
                ErrorCode::InvalidInput,
                "Saved layout preset library attempts to replace a built-in preset.",
            ));
        }
        hot_trimmer_domain::validate_authored_layout_preset(preset).map_err(|reason| {
            error(
                ErrorCode::InvalidInput,
                &format!(
                    "Saved layout preset {} is invalid: {reason}",
                    preset.preset_id
                ),
            )
        })?;
    }
    Ok(library)
}

fn write_authored_layout_preset_library(
    paths: &AppPaths,
    library: &AuthoredLayoutPresetLibrary,
) -> Result<(), UserFacingError> {
    let path = paths.app_data.join(USER_LAYOUT_PRESET_LIBRARY_FILE);
    let bytes = serde_json::to_vec_pretty(library).map_err(|reason| {
        error(
            ErrorCode::Internal,
            &format!("Could not encode saved layout presets: {reason}"),
        )
    })?;
    let mut file = fs::File::create(&path).map_err(|reason| {
        error(
            ErrorCode::Internal,
            &format!("Could not save layout presets: {reason}"),
        )
    })?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|reason| {
            error(
                ErrorCode::Internal,
                &format!("Could not finish saving layout presets: {reason}"),
            )
        })
}

fn read_recent_projects(app_data: &Path) -> Result<Vec<RecentProject>, UserFacingError> {
    let path = app_data.join("recent-projects.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = fs::read(&path).map_err(recent_error)?;
    let mut projects: Vec<RecentProject> = serde_json::from_slice(&bytes).map_err(|parse| {
        error(
            ErrorCode::ProjectInvalid,
            &format!("Recent Projects could not be read: {parse}"),
        )
    })?;
    for project in &mut projects {
        project.available = Path::new(&project.path).is_file();
    }
    Ok(projects)
}

fn write_recent_projects(
    app_data: &Path,
    projects: &[RecentProject],
) -> Result<(), UserFacingError> {
    fs::create_dir_all(app_data).map_err(recent_error)?;
    let path = app_data.join("recent-projects.json");
    let temporary = app_data.join(format!("recent-projects.{}.tmp", Uuid::new_v4()));
    let bytes = serde_json::to_vec_pretty(projects).map_err(|serialize| {
        error(
            ErrorCode::Internal,
            &format!("Recent Projects could not be updated: {serialize}"),
        )
    })?;
    fs::write(&temporary, bytes).map_err(recent_error)?;
    if path.exists() {
        fs::remove_file(&path).map_err(recent_error)?;
    }
    fs::rename(temporary, path).map_err(recent_error)
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
        })
}

fn validate_protocol(received: u16) -> Result<(), UserFacingError> {
    FoundationStatusRequest {
        protocol_version: received,
    }
    .validate()
    .map(|_| ())
    .map_err(UserFacingError::from)
}

fn store_error(error_value: StoreError) -> UserFacingError {
    error(ErrorCode::ProjectInvalid, &error_value.to_string())
}

fn source_registration_error(error_value: StoreError, channel: SourceChannel) -> UserFacingError {
    let Some(diagnostic) = error_value.registration_diagnostic(channel) else {
        return store_error(error_value);
    };
    let detail = serde_json::to_string(&diagnostic).ok();
    UserFacingError {
        code: ErrorCode::SourceRegistrationFailed,
        message: diagnostic.message,
        recovery: diagnostic
            .recovery_choices
            .iter()
            .map(|choice| format!("{choice:?}"))
            .collect::<Vec<_>>()
            .join(", "),
        detail,
    }
}

fn image_error(error_value: hot_trimmer_image_io::ImageIoError) -> UserFacingError {
    error(ErrorCode::ImageImportFailed, &error_value.to_string())
}

fn io_error(error_value: std::io::Error) -> UserFacingError {
    error(ErrorCode::RecoveryFailed, &error_value.to_string())
}

fn recent_error(error_value: std::io::Error) -> UserFacingError {
    error(ErrorCode::Internal, &error_value.to_string())
}

fn no_project() -> UserFacingError {
    error(ErrorCode::NoOpenProject, "Open or create a project first.")
}

fn poisoned() -> UserFacingError {
    error(ErrorCode::Internal, "The project session is unavailable.")
}

fn error(code: ErrorCode, message: &str) -> UserFacingError {
    UserFacingError {
        code,
        message: message.into(),
        recovery: "Correct the issue and retry.".into(),
        detail: None,
    }
}
