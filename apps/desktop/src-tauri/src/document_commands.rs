use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use hot_trimmer_domain::{
    ALGORITHM_STACK_CONTRACT_VERSION, AlgorithmProvenance, AssignmentProvenance,
    CancellationToken as EngineCancellationToken, ChannelRegistration,
    CompilerRequestHeader, ContentDigest, ErrorCode, FoundationStatusRequest, IPC_PROTOCOL_VERSION,
    DelightingIntent, MaterialBehaviorClass, MaterialCalibrationCommand, MaterialCalibrationIntent,
    MaterialClassificationCommand,
    MaterialClassificationIntent,
    MaterialMapKind, MaterialChannelRole, OutputSpecHeader, PatchCommand, PatchGeometry, PatchId,
    NormalConvention, OriginalAssetProvenance, OrientedPixelSize, Projection, RegisteredChannel,
    RegisteredChannelSet, RegionId, SourceId, SourceOwnershipIntent, SourceSetId,
    TrimSheetDocument, TrimSheetDocumentCommand, UserFacingError,
};
use hot_trimmer_image_io::{
    CancellationToken, ColorPolicy, DecodeLimits, InspectedImage, decode_rgba8_bytes_cancellable,
    inspect_bytes_with_policy, inspect_path_with_policy, prepare_registered_channel_set,
    NormalizationSettings, PreparedChannelSet,
};
use hot_trimmer_material_analysis::{
    AnalysisSettings, ScaleOrientationCache, ScaleOrientationSettings, SourceAnalysisCache,
    analyze_source, calibrate_scale_orientation, prepare_delit_exemplar,
    scale_orientation_cache_key, source_analysis_cache_key,
};
use hot_trimmer_project_store::{
    ProjectStore, SourceChannel, SourceInput, SourceOwnership, StoreError, StoredSource,
};
use hot_trimmer_sheet_compiler::{
    AlgorithmCompiler, CompiledMapSet, PreviewMapKind, RegisteredMaterialMap, ResolvedRegion,
};
use image::{DynamicImage, ImageFormat, RgbaImage};
use hot_trimmer_render_core::{
    ExemplarMaskIntent, PlanarArea, PreparedExemplar, PreparedExemplarCache,
    PreparedExemplarRequest, PreparedExemplarScope,
    RectificationQuality, RectificationWorkLimits, RenderCancellationToken,
    prepare_registered_exemplar,
};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::paths::AppPaths;

const MAX_RECENT_PROJECTS: usize = 10;

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
            serde_json::to_string(&source.registration).map_err(|failure| error(ErrorCode::Internal, &failure.to_string()))?,
        );
        if let Some(projection) = self.source_projection_cache.lock().map_err(|_| poisoned())?
            .get(&key).cloned()
        {
            return Ok(projection);
        }
        let projection = source_projection(source)?;
        self.source_projection_cache.lock().map_err(|_| poisoned())?
            .insert(key, projection.clone());
        Ok(projection)
    }
}

pub type SharedProjectSession = Arc<Mutex<ProjectSession>>;
pub type PendingProjectPath = Arc<Mutex<Option<String>>>;
pub type SharedImportJob = Arc<Mutex<Option<CancellationToken>>>;
pub type SharedPreviewService = Arc<PreviewService>;

#[derive(Default)]
pub struct PreviewService {
    latest_draft_id: AtomicU64,
    decoded_sources: Mutex<HashMap<String, (u32, u32, Arc<[u8]>)>>,
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
struct OriginalSourceProjection { path: String, immutable_digest: String, encoded_bytes: u64 }

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceStorageProjection { ownership: SourceOwnership, external_path: Option<String> }

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OrientedSizeProjection { width: u32, height: u32 }

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
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stage14SlotProjection {
    region_id: RegionId,
    slot_key: String,
    display_name: String,
    allocation_bounds: hot_trimmer_domain::CanonicalRect,
    hotspot_bounds: hot_trimmer_domain::CanonicalRect,
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
    regions: Vec<ResolvedRegion>,
    unavailable_channels: Vec<String>,
    slots: Vec<Stage14SlotProjection>,
    pending: Vec<&'static str>,
    final_compile_available: bool,
    export_available: bool,
    blender_available: bool,
    source_frame: Option<hot_trimmer_domain::SourceFrame>,
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
) -> Result<ProjectProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    let store =
        ProjectStore::create(Path::new(&request.path), request.name.trim()).map_err(store_error)?;
    session.adopt(store, false)?;
    remember_open_project_best_effort(&session);
    project_projection(&session)
}

#[tauri::command]
pub fn create_draft_project(
    request: FoundationStatusRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectProjection, UserFacingError> {
    request.validate().map_err(UserFacingError::from)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    fs::create_dir_all(&session.draft_dir).map_err(io_error)?;
    let path = session
        .draft_dir
        .join(format!("Untitled-{}.hottrimmer", Uuid::new_v4()));
    let store = ProjectStore::create(&path, "Untitled").map_err(store_error)?;
    session.adopt(store, true)?;
    project_projection(&session)
}

#[tauri::command]
pub fn open_project(
    request: ProjectPathRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    let store = ProjectStore::open(Path::new(&request.path)).map_err(store_error)?;
    session.adopt(store, false)?;
    remember_open_project_best_effort(&session);
    project_projection(&session)
}

#[tauri::command]
pub fn import_source(
    request: ImportSourceRequest,
    session: State<'_, SharedProjectSession>,
    import_job: State<'_, SharedImportJob>,
) -> Result<ProjectProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let cancellation = CancellationToken::new();
    *import_job.lock().map_err(|_| poisoned())? = Some(cancellation.clone());
    let path = PathBuf::from(&request.path);
    let inspected = inspect_path_with_policy(
        &path,
        DecodeLimits::default(),
        color_policy(request.channel),
    )
    .map_err(image_error)?;
    let input = source_input(&path, request.ownership, &inspected);
    let mut session = session.lock().map_err(|_| poisoned())?;
    let store = session.store.as_mut().ok_or_else(no_project)?;
    store
        .replace_registered_source_in_set(request.source_set_id, &input, ChannelRegistration {
            role: request.channel.material_role(),
            interpretation: request.channel.material_role().required_interpretation(),
            normal_convention: request.normal_convention,
            assignment_provenance: request.assignment_provenance,
            confidence_milli: request.confidence_milli,
        })
        .map_err(|failure| source_registration_error(failure, request.channel))?;
    store.refresh_document_assets().map_err(store_error)?;
    session.prepared_exemplars.invalidate_source(SourceSetId::from_bytes(*request.source_set_id.as_bytes()));
    session.source_analysis_cache = SourceAnalysisCache::default();
    session.mark_mutated();
    *import_job.lock().map_err(|_| poisoned())? = None;
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
    session.prepared_exemplars.invalidate_source(SourceSetId::from_bytes(*request.source_set_id.as_bytes()));
    session.source_analysis_cache = SourceAnalysisCache::default();
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
    session.store.as_mut().ok_or_else(no_project)?
        .set_exemplar_group(request.material_source_id, request.exemplar_group.as_deref())
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
    session.store.as_mut().ok_or_else(no_project)?
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
    session.store.as_mut().ok_or_else(no_project)?
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
    session.store.as_mut().ok_or_else(no_project)?
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
    session.store.as_mut().ok_or_else(no_project)?
        .create_source_frame_document().map_err(store_error)?;
    session.mark_mutated();
    project_projection(&session)
}

#[tauri::command]
pub fn regenerate_source_frame_partition(
    request: SourceFramePartitionRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    session.store.as_mut().ok_or_else(no_project)?
        .regenerate_source_frame_partition(request.target_region_count).map_err(store_error)?;
    session.mark_mutated();
    project_projection(&session)
}

#[tauri::command]
pub fn apply_document_command(
    request: DocumentCommandRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
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
pub fn apply_patch_command(
    request: PatchCommandRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let mut session = session.lock().map_err(|_| poisoned())?;
    let invalidated = {
        let store = session.store.as_mut().ok_or_else(no_project)?;
        let outcome = store.execute_patch_command(&request.command, request.coalescing_group).map_err(store_error)?;
        store.refresh_document_assets().map_err(store_error)?;
        outcome.invalidated_patch_ids
    };
    for patch_id in invalidated { session.prepared_exemplars.invalidate_patch(patch_id); }
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
    for patch_id in invalidated { session.prepared_exemplars.invalidate_patch(patch_id); }
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
    for patch_id in invalidated { session.prepared_exemplars.invalidate_patch(patch_id); }
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
        return Err(error(ErrorCode::InvalidInput, "Patch preview edge must be between 64 and 2048 pixels."));
    }
    let mut session = session.lock().map_err(|_| poisoned())?;
    let summary = session.store.as_ref().ok_or_else(no_project)?.summary().map_err(store_error)?;
    let patch = summary.patches.iter().find(|patch| patch.id == request.patch_id)
        .ok_or_else(|| error(ErrorCode::InvalidInput, "The selected patch no longer exists."))?;
    let transient = request.geometry.is_some();
    let geometry = request.geometry.as_ref().unwrap_or(&patch.geometry);
    let anchor = summary.sources.iter().find(|source| source.input.id == patch.source_id)
        .ok_or_else(|| error(ErrorCode::ProjectInvalid, "The patch source no longer exists."))?;
    let source_set_id = SourceSetId::from_bytes(*anchor.source_set_id.as_bytes());
    let source_set = summary.source_sets.iter().find(|source_set| source_set.id == source_set_id)
        .ok_or_else(|| error(ErrorCode::ProjectInvalid, "The patch material source no longer exists."))?;
    let prepared_source_key = (source_set_id.to_string(), source_set.source_revision);
    let prepared = if let Some(prepared) = session.preview_prepared_sources.get(&prepared_source_key) {
        Arc::clone(prepared)
    } else {
        let (registered, encoded_sources) = preview_registered_channel_set(
            &session,
            &summary.sources,
            anchor.source_set_id,
        )?;
        let normalization_settings = NormalizationSettings {
            max_levels: 1,
            max_memory_bytes: 268_435_456,
            ..NormalizationSettings::default()
        };
        let prepared = Arc::new(prepare_registered_channel_set(
            &registered,
            &encoded_sources,
            &normalization_settings,
            &CancellationToken::new(),
        ).map_err(|failure| error(ErrorCode::ImageImportFailed, &format!("Source preparation failed: {failure}")))?);
        if session.preview_prepared_sources.len() >= 8 { session.preview_prepared_sources.clear(); }
        session.preview_prepared_sources.insert(prepared_source_key, Arc::clone(&prepared));
        prepared
    };
    let patch_bytes = serde_json::to_vec(&(patch.id, geometry, patch.rectification))
        .map_err(|failure| error(ErrorCode::Internal, &failure.to_string()))?;
    let revision_digest = ContentDigest::sha256(&patch_bytes);
    let patch_revision = u64::from_str_radix(&revision_digest.0[..16], 16)
        .map_err(|failure| error(ErrorCode::Internal, &failure.to_string()))?;
    let exemplar_request = PreparedExemplarRequest {
        exemplar_id: patch.id.to_string(),
        area: PlanarArea::FourPoint { corners: geometry.corners },
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
    let cached = (!transient).then(|| session.prepared_exemplars.get(&key)).flatten().cloned();
    let exemplar = if let Some(cached) = cached {
        cached
    } else {
        let value = prepare_registered_exemplar(&prepared, &exemplar_request, &RenderCancellationToken::new())
            .map_err(|failure| error(ErrorCode::PatchGeometryInvalid, &failure.to_string()))?;
        if !transient { session.prepared_exemplars.insert_complete(value.clone()); }
        value
    };
    let stage_four = prepare_delit_exemplar(
        &exemplar,
        &source_set.delighting,
        None,
        &RenderCancellationToken::new(),
    ).map_err(|failure| error(ErrorCode::InvalidInput, &format!("Stage 4 source preparation failed: {failure}")))?;
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
        ).map_err(|failure| error(ErrorCode::InvalidInput, &format!("Stage 5 source analysis failed: {failure}")))?;
        session.source_analysis_cache.insert_complete(report.clone());
        report
    };
    stage_five.classification.routing_intent = source_set.classification;
    let inspector = stage_five.inspector_evidence();
    let stage_six_settings = ScaleOrientationSettings::default();
    let stage_six_key = scale_orientation_cache_key(&stage_five, &source_set.calibration, &stage_six_settings);
    let stage_six = if let Some(cached) = session.scale_orientation_cache.get(&stage_six_key) {
        cached.clone()
    } else {
        let report = calibrate_scale_orientation(
            &stage_four,
            &stage_five,
            &source_set.calibration,
            &stage_six_settings,
            &RenderCancellationToken::new(),
        ).map_err(|failure| error(ErrorCode::InvalidInput, &format!("Stage 6 calibration failed: {failure}")))?;
        session.scale_orientation_cache.insert_complete(report.clone());
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
                Some(axis) => format!("axis {:.1}°, confidence {}%", axis as f64 / 1000.0, stage_six.global_orientation.confidence_milli / 10),
                None => format!("orientation unavailable, confidence {}%", stage_six.global_orientation.confidence_milli / 10),
            },
            world_scale_available: stage_six.measurement_overlay.world_scale_available,
            orientation_overlay: stage_six.local_orientation.iter().map(|sample| OrientationOverlayProjection {
                source_x_milli: sample.source_x_milli,
                source_y_milli: sample.source_y_milli,
                axis_millidegrees: sample.axis_millidegrees,
                confidence_milli: sample.confidence_milli,
            }).collect(),
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
    let job = preview_service.latest_draft_id.fetch_add(1, Ordering::AcqRel).saturating_add(1);
    tauri::async_runtime::spawn_blocking(move || build_stage_14_preview(&session, &preview_service, request, job))
        .await.map_err(|join| error(ErrorCode::Internal, &format!("Stage 14 preview worker failed: {join}")))?
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
        MaterialChannelRole, PixelSize, SamplingMode, SamplingPolicy, SourceSamplingMode, StageResult,
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
        guard.store.as_ref().ok_or_else(no_project)?.summary().map_err(store_error)?
    };
    let document = summary.document.as_ref().ok_or_else(|| error(ErrorCode::LayoutInvalid, "Create a trim sheet first."))?;
    if document.document_revision != requested_revision {
        return Err(error(ErrorCode::OperationCancelled, "A newer document revision superseded this preview."));
    }
    document.primary_material.ok_or_else(|| error(ErrorCode::InvalidInput, "Choose a primary material first."))?;
    let selected = summary.sources.iter()
        .map(|source| registered_map_cached(source, preview_service)).collect::<Result<Vec<_>, _>>()?;
    if !selected.iter().any(|map| map.kind == MaterialMapKind::BaseColor) {
        return Err(error(ErrorCode::InvalidInput, "Preview through Stage 14 requires imported Base Color."));
    }
    let dimensions = selected.first().map(|map| [map.width, map.height]).unwrap_or([0, 0]);
    if selected.iter().any(|map| [map.width, map.height] != dimensions) {
        return Err(error(ErrorCode::SourceRegistrationFailed, "Imported registered channels are not aligned."));
    }
    let mut channels = Vec::new();
    for map in &selected {
        let pixel_count = usize::try_from(u64::from(map.width) * u64::from(map.height))
            .map_err(|_| error(ErrorCode::InvalidInput, "Imported material is too large."))?;
        match map.kind {
            MaterialMapKind::BaseColor => {
                let values = map.rgba8.chunks_exact(4).map(|pixel| LinearColor {
                    rgb: [srgb_to_linear(pixel[0]), srgb_to_linear(pixel[1]), srgb_to_linear(pixel[2])],
                    alpha: f32::from(pixel[3]) / 255.0,
                }).collect::<Vec<_>>();
                channels.push(PreparedExemplarChannel::BaseColor { plane: ImagePlane::from_row_major(map.width, map.height, 128, &values)
                    .map_err(|failure| error(ErrorCode::InvalidInput, &failure.to_string()))?, alpha_mode: hot_trimmer_image_io::ResolvedAlphaMode::Straight });
            }
            MaterialMapKind::Normal => {
                let values = map.rgba8.chunks_exact(4).map(|pixel| TangentNormal { xyz: [
                    f32::from(pixel[0]) / 127.5 - 1.0, f32::from(pixel[1]) / 127.5 - 1.0,
                    f32::from(pixel[2]) / 127.5 - 1.0], alpha: f32::from(pixel[3]) / 255.0 }).collect::<Vec<_>>();
                channels.push(PreparedExemplarChannel::Normal { plane: ImagePlane::from_row_major(map.width, map.height, 128, &values)
                    .map_err(|failure| error(ErrorCode::InvalidInput, &failure.to_string()))?, source_convention: NormalConvention::OpenGl,
                    canonical_convention: NormalConvention::OpenGl, alpha_policy: hot_trimmer_image_io::NormalAlphaPolicy::Preserve });
            }
            MaterialMapKind::MaterialId => {
                let values = map.rgba8.chunks_exact(4).map(|pixel| CategoryId(u32::from_le_bytes([pixel[0], pixel[1], pixel[2], 0]))).collect::<Vec<_>>();
                channels.push(PreparedExemplarChannel::MaterialId { plane: ImagePlane::from_row_major(map.width, map.height, 128, &values)
                    .map_err(|failure| error(ErrorCode::InvalidInput, &failure.to_string()))? });
            }
            kind => {
                let role = match kind { MaterialMapKind::Height => MaterialChannelRole::Height,
                    MaterialMapKind::Roughness => MaterialChannelRole::Roughness, MaterialMapKind::Metallic => MaterialChannelRole::Metallic,
                    MaterialMapKind::AmbientOcclusion => MaterialChannelRole::AmbientOcclusion, MaterialMapKind::Specular => MaterialChannelRole::Specular,
                    MaterialMapKind::Opacity => MaterialChannelRole::Opacity, MaterialMapKind::EdgeMask => MaterialChannelRole::EdgeMask, _ => continue };
                let values = map.rgba8.chunks_exact(4).take(pixel_count).map(|pixel| LinearScalar(f32::from(pixel[0]) / 255.0)).collect::<Vec<_>>();
                channels.push(PreparedExemplarChannel::Scalar { role, plane: ImagePlane::from_row_major(map.width, map.height, 128, &values)
                    .map_err(|failure| error(ErrorCode::InvalidInput, &failure.to_string()))? });
            }
        }
    }
    let domain_id = ContentDigest::sha256(format!("{}|{:?}", primary, document.appearance_hash().map_err(|failure| error(ErrorCode::LayoutInvalid, &failure.to_string()))?).as_bytes());
    let source_id = ContentDigest::sha256(selected.iter().flat_map(|map| map.sha256.as_bytes()).copied().collect::<Vec<_>>().as_slice());
    let domain = PreparedMaterialDomain::from_registered_channels(domain_id.clone(), source_id, channels)
        .map_err(|failure| error(ErrorCode::InvalidInput, &format!("Stage 8 registered domain failed: {failure}")))?;
    let snapshot = document.topology.snapshot.template.as_ref().ok_or_else(|| error(ErrorCode::LayoutInvalid, "Stage 14 preview currently requires a persisted template."))?;
    let definition: hot_trimmer_domain::TemplateDefinition = serde_json::from_str(&snapshot.snapshot_json)
        .map_err(|failure| error(ErrorCode::LayoutInvalid, &format!("Persisted template is invalid: {failure}")))?;
    let topology = definition.compile_for_output(PixelSize { width: document.render_settings.output_size.width,
        height: document.render_settings.output_size.height }).map_err(|failure| error(ErrorCode::LayoutInvalid, &failure.to_string()))?;
    let resolved = hot_trimmer_sheet_compiler::resolve_compile_plan(document, &selected)
        .map_err(|failure| error(ErrorCode::LayoutInvalid, &failure.to_string()))?;

    let mut plans = Vec::new();
    for (index, slot) in topology.slots.iter().enumerate() {
        let region = document.topology.regions.get(index).ok_or_else(|| error(ErrorCode::LayoutInvalid, "Stage 9 slot order drifted from the document."))?;
        let mode = match region.orientation { hot_trimmer_domain::RegionOrientation::Horizontal => SamplingMode::RepeatX,
            hot_trimmer_domain::RegionOrientation::Vertical => SamplingMode::RepeatY, _ => SamplingMode::PeriodicTile };
        let period = [domain.width.max(1), domain.height.max(1)];
        let candidate = CropCandidate { candidate_id: ContentDigest::sha256(format!("{}|{}|stage-11", slot.slot_key, domain_id.0).as_bytes()),
            source_id: domain.prepared_source_digest.clone(), domain_id: domain_id.clone(), slot_id: region.id,
            crop: Some(SourceCrop { x: 0, y: 0, width: domain.width, height: domain.height }),
            transform: CandidateTransform { rotation: hot_trimmer_domain::QuarterTurn::Zero, mirror: MirrorTransform::None }, isotropic_scale: 1.0,
            mapping_mode: mode, family: match mode { SamplingMode::RepeatX => CandidateFamily::RepeatXSegment,
                SamplingMode::RepeatY => CandidateFamily::RepeatYSegment, _ => CandidateFamily::PanelSeamlessTile },
            route: CandidateRoute::Repeat, position_strategy: PositionStrategy::PeriodAligned, period_pixels: Some(period), seam_indices: Vec::new(),
            correspondence_reference: domain_id.clone(), descriptors: CandidateDescriptors { saliency_milli: 0, stationarity_milli: 1000,
                feature_strength_milli: 0, usability_milli: 1000 }, seed: 14,
            eligibility: EligibilityEvidence { mapping_permitted: true, transform_permitted: true, isotropic_scale: true,
                exact_aspect: true, entire_crop_usable: Some(true), cross_axis_preserved: Some(true), lattice_aligned: Some(true),
                direct_crop_applicable: true, direct_crop_rejection: None, reasons: vec!["selected by the bounded Stage 14 preview route".into()] } };
        plans.push(SamplingPlan { slot_id: region.id, role: region.role, variation_group: region.material_group.clone(),
            prepared_domain_dimensions: [domain.width, domain.height], candidate,
            slot_physical_size: [f64::from(slot.allocation.width), f64::from(slot.allocation.height)], source_pixels_per_physical_unit: 1.0,
            sampling_policy: SamplingPolicy { filter: SourceSamplingMode::Linear, scale: 1.0, correct_tangent_normals: true },
            stretch_override: StretchOverrideProvenance::NotAuthorized, slice_geometry: SliceGeometry::None,
            maximum_seam_cost_milli: 450, unary_cost: 0.0 });
    }
    let placement = PlacementPlan { stage_result: StageResult::Executed { algorithm: AlgorithmProvenance {
        algorithm_id: hot_trimmer_placement_solver::STAGE_13_ALGORITHM_ID.into(), version: hot_trimmer_placement_solver::STAGE_13_ALGORITHM_VERSION.into() },
        settings_hash: ContentDigest::sha256(b"stage-14-preview-a-selected-placement"), diagnostics: Vec::new() },
        solver: AlgorithmProvenance { algorithm_id: hot_trimmer_placement_solver::STAGE_13_ALGORITHM_ID.into(),
            version: hot_trimmer_placement_solver::STAGE_13_ALGORITHM_VERSION.into() }, seed: 14, placements: plans.clone(),
        objective: PlacementObjectiveBreakdown { unary_cost: 0.0, pairwise_cost: 0.0, pairwise_lambda: 1.0,
            weighted_pairwise_cost: 0.0, total_cost: 0.0 }, pairwise_decisions: Vec::new(), crop_reuse_heatmap: Vec::new(),
        validation: PlacementValidationSummary { complete_assignment: true, required_slots_present: true,
            isotropic_scale_only: true, registered_mapping_only: true, slot_count: u32::try_from(plans.len()).unwrap_or(u32::MAX) },
        qa_views: vec![PlacementPlanQaView::SelectedPlacements, PlacementPlanQaView::Validation] };
    let render_cancellation = RenderCancellationToken::new();
    let mut results = Vec::new();
    for (slot, plan) in topology.slots.iter().zip(&plans) {
        if preview_service.latest_draft_id.load(Ordering::Acquire) != job {
            return Err(error(ErrorCode::OperationCancelled, "A newer Stage 14 preview superseded this request."));
        }
        results.push(synthesize_slot_material(SlotSynthesisRequest { plan, domain: &domain,
            output_dimensions: [slot.allocation.width, slot.allocation.height], limits: SlotSynthesisLimits::default() },
            &render_cancellation).map_err(|failure| error(ErrorCode::InvalidInput, &format!("Required Stage 14 slot failed: {failure}")))?);
    }
    let patch_id = summary.patches.iter().find(|patch| summary.sources.iter().any(|source| source.input.id == patch.source_id
        && source.source_set_id.to_string() == primary.to_string())).map(|patch| patch.id.to_string());
    let slot_inputs = topology.slots.iter().zip(document.topology.regions.iter()).zip(plans.iter().zip(results.iter()))
        .map(|((slot, region), (plan, result))| IntermediateSlotInput { region_id: region.id, slot_key: slot.slot_key.as_str(),
            display_name: region.display_name.as_str(), required: true, patch_id: patch_id.as_deref(), domain: &domain, plan, result, grid_rect: region.grid_rect }).collect();
    let algorithms = (1..=14).map(|stage| (stage, AlgorithmProvenance { algorithm_id: format!("installed-stage-{stage:02}"),
        version: if stage == 14 { hot_trimmer_sheet_compiler::STAGE_14_ALGORITHM_VERSION.into() } else { "installed".into() } })).collect();
    let atlas_request = IntermediateAtlasRequest { topology: &topology, placement_plan: &placement, slots: slot_inputs,
        revision: requested_revision, algorithm_versions: algorithms, diagnostics: Vec::new() };
    let cancellation = EngineCancellationToken::new();
    let artifact = AlgorithmCompiler::new().compile_intermediate_atlas(&atlas_request, &cancellation, || {
        session.lock().ok().and_then(|guard| guard.store.as_ref()?.summary().ok()?.document.map(|value| value.document_revision)).unwrap_or(0)
    }).map_err(|failure| error(ErrorCode::OperationCancelled, &failure.to_string()))?;
    let mut maps = BTreeMap::new();
    for channel in &artifact.channels {
        maps.insert(channel_key(channel.role).into(), png_data_url(topology.output_size.width,
            topology.output_size.height, channel.rgba8.clone())?);
    }
    let slots = artifact.slots.iter().filter_map(|slot| resolved.regions.iter().find(|region| region.region_id == slot.region_id).map(|region| Stage14SlotProjection { region_id: region.region_id,
        slot_key: slot.slot_key.clone(),
        display_name: slot.display_name.clone(), allocation_bounds: slot.allocation, hotspot_bounds: slot.hotspot,
        mapping_mode: format!("{:?}", slot.mapping_mode), source_transform: slot.source_transform,
        isotropic_scale: slot.isotropic_scale, sampling_scale: slot.sampling_scale,
        validity: format!("{} valid pixels", slot.valid_pixel_count),
        correspondence: "authoritative Stage 14 correspondence".into(), source_id: slot.source_id.0.clone(), patch_id: slot.patch_id.clone(),
        domain_id: slot.domain_id.0.clone(), candidate_id: slot.candidate_id.0.clone(), sampling_plan_id: slot.sampling_plan_id.0.clone(),
        stage_14_result_id: slot.stage_14_result_id.0.clone(), source_crop: slot.source_crop, source_bounds: region.source_bounds, mapping_origin: region.mapping_origin, grid_rect: slot.grid_rect })).collect();
    Ok(IntermediateAtlasProjection { label: artifact.label, non_exportable: true, incomplete_after_stage: 14,
        revision: artifact.revision, document_revision: artifact.revision,
        topology_hash: hash_hex(document.topology.topology_hash),
        appearance_hash: hash_hex(document.appearance_hash().map_err(|failure| error(ErrorCode::LayoutInvalid, &failure.to_string()))?),
        renderer_version: "intermediate-stage-14", width: topology.output_size.width, height: topology.output_size.height,
        topology: artifact.topology, placement_plan_id: artifact.placement_plan_id.0, maps,
        regions: resolved.regions,
        unavailable_channels: artifact.unavailable_channels.iter().map(|role| format!("{role:?}")).collect(),
        slots, pending: artifact.pending, final_compile_available: false, export_available: false, blender_available: false,
        source_frame: document.source_frame.clone() })
}

fn srgb_to_linear(value: u8) -> f32 {
    let value = f32::from(value) / 255.0;
    if value <= 0.04045 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) }
}

fn build_stage_14_preview(
    session: &SharedProjectSession,
    preview_service: &PreviewService,
    request: Stage14PreviewRequest,
    job: u64,
) -> Result<IntermediateAtlasProjection, UserFacingError> {
    let mut summary = {
        let guard = session.lock().map_err(|_| poisoned())?;
        guard.store.as_ref().ok_or_else(no_project)?.summary().map_err(store_error)?
    };
    let document = summary.document.as_ref()
        .ok_or_else(|| error(ErrorCode::LayoutInvalid, "Create a trim sheet first."))?;
    if document.document_revision != request.revision {
        return Err(error(ErrorCode::OperationCancelled, "A newer document revision superseded this preview."));
    }
    if request.region_id.is_some() != request.transient_projection.is_some() {
        return Err(error(ErrorCode::InvalidInput, "Transient crop requests require both regionId and transientProjection."));
    }
    if let (Some(region_id), Some(projection)) = (request.region_id, request.transient_projection.clone()) {
        let mut transient_document = document.clone();
        let binding = transient_document.region_bindings.get_mut(&region_id)
            .ok_or_else(|| error(ErrorCode::InvalidInput, "Transient crop region is not in the persisted topology."))?;
        binding.mapping.projection = projection;
        binding.mapping.source_crop_intent = Some(hot_trimmer_domain::SourceCropIntent::Authored);
        summary.document = Some(transient_document);
    }
    let document = summary.document.as_ref().expect("summary document was present");
    let revision_current = AtomicBool::new(true);
    let monitoring_complete = AtomicBool::new(false);
    let artifact = std::thread::scope(|scope| {
        scope.spawn(|| {
            while !monitoring_complete.load(Ordering::Acquire) {
                let live = session.lock().ok().and_then(|guard| guard.store.as_ref()?.summary().ok()?.document
                    .map(|document| document.document_revision == request.revision)).unwrap_or(false);
                if !live { revision_current.store(false, Ordering::Release); return; }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        let result = AlgorithmCompiler::new().compile_persisted_stage_14_preview(
            hot_trimmer_sheet_compiler::PersistedStage14PreviewRequest { project: &summary, revision: request.revision, draft_id: request.draft_id, input_hash: request.input_hash.clone() },
            &EngineCancellationToken::new(),
            || preview_service.latest_draft_id.load(Ordering::Acquire) == job
                && revision_current.load(Ordering::Acquire),
        );
        monitoring_complete.store(true, Ordering::Release);
        result
    }).map_err(|failure| error(ErrorCode::OperationCancelled, &failure.to_string()))?;
    let live_revision = session.lock().ok().and_then(|guard| guard.store.as_ref()?.summary().ok()?.document
        .map(|document| document.document_revision)).unwrap_or(0);
    if live_revision != request.revision || preview_service.latest_draft_id.load(Ordering::Acquire) != job {
        return Err(error(ErrorCode::OperationCancelled, "A newer document revision superseded this preview."));
    }
    let primary = document.primary_material.ok_or_else(|| error(ErrorCode::InvalidInput, "Choose a primary material first."))?;
    let selected = summary.sources.iter().filter(|source| source.source_set_id.to_string() == primary.to_string())
        .map(|source| registered_map_cached(source, preview_service)).collect::<Result<Vec<_>, _>>()?;
    let resolved = hot_trimmer_sheet_compiler::resolve_compile_plan(document, &selected)
        .map_err(|failure| error(ErrorCode::LayoutInvalid, &failure.to_string()))?;
    let mut maps = BTreeMap::new();
    for channel in &artifact.channels {
        maps.insert(channel_key(channel.role).into(), png_data_url(artifact.topology.output_size.width,
            artifact.topology.output_size.height, channel.rgba8.clone())?);
    }
    let slots = artifact.slots.iter().map(|slot| {
        let region = resolved.regions.iter().find(|region| region.region_id == slot.region_id)
            .ok_or_else(|| error(ErrorCode::LayoutInvalid, "Stage 14 artifact refers to an unknown region identity."))?;
        Ok(Stage14SlotProjection {
        region_id: region.region_id, slot_key: slot.slot_key.clone(), display_name: slot.display_name.clone(),
        allocation_bounds: slot.allocation, hotspot_bounds: slot.hotspot, mapping_mode: format!("{:?}", slot.mapping_mode),
        source_transform: slot.source_transform, isotropic_scale: slot.isotropic_scale, sampling_scale: slot.sampling_scale,
        validity: format!("{} valid pixels", slot.valid_pixel_count), correspondence: "authoritative Stage 14 correspondence".into(),
        source_id: slot.source_id.0.clone(), patch_id: slot.patch_id.clone(), domain_id: slot.domain_id.0.clone(),
        candidate_id: slot.candidate_id.0.clone(), sampling_plan_id: slot.sampling_plan_id.0.clone(),
        stage_14_result_id: slot.stage_14_result_id.0.clone(), source_crop: slot.source_crop, source_bounds: region.source_bounds, mapping_origin: region.mapping_origin, grid_rect: slot.grid_rect,
    })
    }).collect::<Result<Vec<_>, UserFacingError>>()?;
    Ok(IntermediateAtlasProjection {
        label: artifact.label, non_exportable: true, incomplete_after_stage: 14, revision: artifact.revision,
        document_revision: artifact.revision, topology_hash: hash_hex(document.topology.topology_hash),
        appearance_hash: hash_hex(document.appearance_hash().map_err(|failure| error(ErrorCode::LayoutInvalid, &failure.to_string()))?),
        renderer_version: "intermediate-stage-14", width: artifact.topology.output_size.width,
        height: artifact.topology.output_size.height, topology: artifact.topology,
        placement_plan_id: artifact.placement_plan_id.0, maps, regions: resolved.regions,
        unavailable_channels: artifact.unavailable_channels.iter().map(|role| format!("{role:?}")).collect(),
        slots, pending: artifact.pending, final_compile_available: false, export_available: false, blender_available: false,
        source_frame: document.source_frame.clone(),
    })
}

const fn channel_key(role: hot_trimmer_domain::MaterialChannelRole) -> &'static str {
    use hot_trimmer_domain::MaterialChannelRole;
    match role {
        MaterialChannelRole::BaseColor => "baseColor", MaterialChannelRole::Normal => "normal",
        MaterialChannelRole::Height => "height", MaterialChannelRole::Roughness => "roughness",
        MaterialChannelRole::Metallic => "metallic", MaterialChannelRole::AmbientOcclusion => "ambientOcclusion",
        MaterialChannelRole::Specular => "specular", MaterialChannelRole::Opacity => "opacity",
        MaterialChannelRole::EdgeMask => "edgeMask", MaterialChannelRole::MaterialId => "materialId",
    }
}

fn compile_trim_sheet_document_impl(
    session: &SharedProjectSession,
    _preview_service: &PreviewService,
) -> Result<CompiledSheetProjection, UserFacingError> {
    let summary = {
        let session = session.lock().map_err(|_| poisoned())?;
        session.store.as_ref().ok_or_else(no_project)?.summary().map_err(store_error)?
    };
    let document = summary.document.as_ref()
        .ok_or_else(|| error(ErrorCode::LayoutInvalid, "Create a trim sheet first."))?;
    let request = CompilerRequestHeader {
        contract_version: ALGORITHM_STACK_CONTRACT_VERSION,
        source_digests: summary.sources.iter()
            .map(|source| ContentDigest(source.input.sha256.clone()))
            .collect(),
        settings_hash: ContentDigest(hash_hex(document.appearance_hash().map_err(|invalid| {
            error(ErrorCode::LayoutInvalid, &invalid.to_string())
        })?)),
        algorithm_versions: (1_u8..=20).map(|stage| (stage, AlgorithmProvenance {
            algorithm_id: format!("stage-{stage:02}"),
            version: String::from("0.0.0-unsupported"),
        })).collect::<BTreeMap<_, _>>(),
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
        session.store.as_ref().ok_or_else(no_project)?.summary().map_err(store_error)?
    };
    let mut document = summary.document
        .ok_or_else(|| error(ErrorCode::LayoutInvalid, "Create a trim sheet first."))?;
    let max_edge = request.max_edge.unwrap_or(1024).clamp(512, 1024);
    // Topology identifies the compositing surface. Appearance edits deliberately reuse the
    // previous pixels and repaint only their dirty region.
    let settled_key = (hash_hex(document.topology.topology_hash), request.map_view, max_edge);
    let base_pixels = if request.region_id.is_some() {
        preview_service.settled_previews.lock().map_err(|_| poisoned())?
            .get(&settled_key).map(|preview| preview.pixels.clone())
    } else {
        None
    };
    // A dirty preview is valid only when it composites onto a known-complete surface. If this
    // topology/map has no settled base yet, render the whole low-resolution sheet once instead
    // of promoting a one-region image with a black background into the settled cache.
    let effective_dirty_region = request.region_id.filter(|_| base_pixels.is_some());
    if let (Some(region_id), Some(projection)) = (request.region_id, request.projection) {
        let binding = document.region_bindings.get_mut(&region_id)
            .ok_or_else(|| error(ErrorCode::InvalidInput, "The preview region no longer exists."))?;
        binding.mapping.projection = projection;
    }
    let maps = summary.sources.iter()
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
    ).map_err(|compile| match compile {
        hot_trimmer_sheet_compiler::SheetCompileError::Cancelled => {
            error(ErrorCode::OperationCancelled, "A newer preview superseded this draft.")
        }
        _ => error(ErrorCode::LayoutInvalid, &format!("Preview failed: {compile}")),
    })?;
    if preview_service.latest_draft_id.load(Ordering::Acquire) != request.draft_id {
        return Err(error(ErrorCode::OperationCancelled, "A newer preview superseded this draft."));
    }
    {
        let mut cache = preview_service.settled_previews.lock().map_err(|_| poisoned())?;
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
        data_url: png_data_url(compiled.dimensions.width, compiled.dimensions.height, compiled.pixels)?,
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
    let material_sources = summary.source_sets.iter().map(|material| {
        let channels = summary.sources.iter()
            .filter(|source| source.source_set_id.to_string() == material.id.to_string())
            .map(|source| session.source_projection_cached(source))
            .collect::<Result<Vec<_>, _>>()?;
        let registered_channels = if channels.is_empty() {
            None
        } else {
            let anchor = channels.iter().find(|source| source.channel == SourceChannel::BaseColor)
                .ok_or_else(|| error(ErrorCode::ProjectInvalid, "A registered channel set has no Base Color anchor."))?;
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
    }).collect::<Result<Vec<_>, UserFacingError>>()?;
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
            external_path: source.input.external_path.as_ref().map(|path| path.display().to_string()),
        },
        oriented_size: OrientedSizeProjection { width: inspected.info.width, height: inspected.info.height },
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

fn registered_map(source: &StoredSource) -> Result<RegisteredMaterialMap, UserFacingError> {
    let bytes = source_bytes(source)?;
    let decoded = decode_rgba8_bytes_cancellable(
        &bytes,
        DecodeLimits::default(),
        color_policy(source.channel),
        &CancellationToken::new(),
    )
    .map_err(image_error)?;
    Ok(RegisteredMaterialMap {
        source_id: source.input.id,
        material_id: hot_trimmer_domain::SourceSetId::from_bytes(*source.source_set_id.as_bytes()),
        kind: material_kind(source.channel),
        sha256: source.input.sha256.clone(),
        width: decoded.width,
        height: decoded.height,
        rgba8: decoded.pixels.into(),
    })
}

fn registered_map_cached(
    source: &StoredSource,
    service: &PreviewService,
) -> Result<RegisteredMaterialMap, UserFacingError> {
    if let Some((width, height, pixels)) = service.decoded_sources.lock().map_err(|_| poisoned())?
        .get(&source.input.sha256).cloned()
    {
        return Ok(RegisteredMaterialMap {
            source_id: source.input.id,
            material_id: hot_trimmer_domain::SourceSetId::from_bytes(*source.source_set_id.as_bytes()),
            kind: material_kind(source.channel),
            sha256: source.input.sha256.clone(),
            width,
            height,
            rgba8: pixels,
        });
    }
    let decoded = registered_map(source)?;
    service.decoded_sources.lock().map_err(|_| poisoned())?.insert(
        source.input.sha256.clone(),
        (decoded.width, decoded.height, Arc::clone(&decoded.rgba8)),
    );
    Ok(decoded)
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
    let mut selected: Vec<_> = sources.iter()
        .filter(|source| source.source_set_id == source_set_id)
        .collect();
    selected.sort_by_key(|source| source.registration.role);
    if !selected.iter().any(|source| source.registration.role == MaterialChannelRole::BaseColor) {
        return Err(error(ErrorCode::ProjectInvalid, "A prepared patch requires Base Color."));
    }
    let mut encoded_sources = BTreeMap::new();
    let mut channels = Vec::with_capacity(selected.len());
    let mut oriented_size = None;
    for source in selected {
        let projection = session.source_projection_cached(source)?;
        let (_, payload) = projection.thumbnail_data_url.split_once(',')
            .ok_or_else(|| error(ErrorCode::Internal, "The cached source preview is malformed."))?;
        let bytes = STANDARD.decode(payload)
            .map_err(|failure| error(ErrorCode::Internal, &format!("The cached source preview is invalid: {failure}")))?;
        let image = image::load_from_memory(&bytes)
            .map_err(|failure| error(ErrorCode::ImageImportFailed, &format!("The cached source preview could not be decoded: {failure}")))?;
        let dimensions = OrientedPixelSize { width: image.width(), height: image.height() };
        if oriented_size.is_some_and(|expected| expected != dimensions) {
            return Err(error(ErrorCode::SourceRegistrationFailed, "Registered preview channels no longer share dimensions."));
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
        oriented_size: oriented_size.ok_or_else(|| error(ErrorCode::ProjectInvalid, "A prepared patch requires registered channels."))?,
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
        patch_id: exemplar.scope.patch_id.ok_or_else(|| error(ErrorCode::Internal, "Prepared patch scope is missing."))?,
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
    let image = RgbaImage::from_raw(width, height, pixels)
        .ok_or_else(|| error(ErrorCode::Internal, "Compiled pixels are invalid."))?;
    let mut encoded = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut encoded, ImageFormat::Png)
        .map_err(|e| error(ErrorCode::Internal, &e.to_string()))?;
    Ok(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(encoded.into_inner())
    ))
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

const fn material_kind(channel: SourceChannel) -> MaterialMapKind {
    match channel {
        SourceChannel::BaseColor => MaterialMapKind::BaseColor,
        SourceChannel::Normal => MaterialMapKind::Normal,
        SourceChannel::Height => MaterialMapKind::Height,
        SourceChannel::Roughness => MaterialMapKind::Roughness,
        SourceChannel::Metallic => MaterialMapKind::Metallic,
        SourceChannel::AmbientOcclusion => MaterialMapKind::AmbientOcclusion,
        SourceChannel::Specular => MaterialMapKind::Specular,
        SourceChannel::Opacity => MaterialMapKind::Opacity,
        SourceChannel::EdgeMask => MaterialMapKind::EdgeMask,
        SourceChannel::MaterialId => MaterialMapKind::MaterialId,
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

#[cfg(any())]
mod algorithm_stage_14_preview_a_tests {
    use std::collections::BTreeMap;

    use hot_trimmer_domain::{
        AlgorithmProvenance, CancellationToken, CanonicalRect, CompiledTemplateSlot,
        CompiledTemplateTopology, ContentDigest, MaterialChannelRole, NormalConvention,
        PixelSize, QuarterTurn, RegionId, SamplingMode, SamplingPolicy, SourceSamplingMode,
        StageResult, TemplateIdentity, TemplateSlotRole,
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
        let colors = vec![LinearColor { rgb: [marker, marker * 0.5, 0.25], alpha: 1.0 }; 16];
        let channel = PreparedExemplarChannel::BaseColor {
            plane: ImagePlane::from_row_major(4, 4, 4, &colors).unwrap(),
            alpha_mode: ResolvedAlphaMode::Opaque,
        };
        PreparedMaterialDomain::from_registered_channels(
            ContentDigest::sha256([name, b"-domain"].concat().as_slice()),
            ContentDigest::sha256([name, b"-source"].concat().as_slice()),
            vec![channel],
        ).unwrap()
    }

    fn plan(id: u8, domain: &PreparedMaterialDomain) -> SamplingPlan {
        let slot_id = RegionId::from_bytes([id; 16]);
        SamplingPlan {
            slot_id, role: TemplateSlotRole::Planar, variation_group: format!("slot-{id}"),
            prepared_domain_dimensions: [4, 4],
            candidate: CropCandidate {
                candidate_id: ContentDigest::sha256(&[id, 1]),
                source_id: domain.prepared_source_digest.clone(), domain_id: domain.cache_key.clone(),
                slot_id, crop: Some(SourceCrop { x: 0, y: 0, width: 4, height: 4 }),
                transform: CandidateTransform { rotation: QuarterTurn::Zero, mirror: MirrorTransform::None },
                isotropic_scale: 1.0, mapping_mode: SamplingMode::DirectCrop,
                family: CandidateFamily::PanelDirect, route: CandidateRoute::Direct,
                position_strategy: PositionStrategy::FeatureAware, period_pixels: None,
                seam_indices: Vec::new(), correspondence_reference: domain.cache_key.clone(),
                descriptors: CandidateDescriptors { saliency_milli: 0, stationarity_milli: 1000,
                    feature_strength_milli: 0, usability_milli: 1000 }, seed: 14,
                eligibility: EligibilityEvidence { mapping_permitted: true, transform_permitted: true,
                    isotropic_scale: true, exact_aspect: true, entire_crop_usable: Some(true),
                    cross_axis_preserved: None, lattice_aligned: None, direct_crop_applicable: true,
                    direct_crop_rejection: None, reasons: Vec::new() },
            },
            slot_physical_size: [1.0, 1.0], source_pixels_per_physical_unit: 4.0,
            sampling_policy: SamplingPolicy { filter: SourceSamplingMode::Nearest, scale: 1.0,
                correct_tangent_normals: true }, stretch_override: StretchOverrideProvenance::NotAuthorized,
            slice_geometry: SliceGeometry::None, maximum_seam_cost_milli: 450, unary_cost: 0.0,
        }
    }

    fn placement(plans: Vec<SamplingPlan>) -> PlacementPlan {
        PlacementPlan {
            stage_result: StageResult::Executed { algorithm: AlgorithmProvenance {
                algorithm_id: "hot-trimmer.stage-13.global-placement".into(), version: "1.0.0".into(),
            }, settings_hash: ContentDigest::sha256(b"preview-placement"), diagnostics: Vec::new() },
            solver: AlgorithmProvenance { algorithm_id: "hot-trimmer.stage-13.global-placement".into(), version: "1.0.0".into() },
            seed: 14, placements: plans, objective: PlacementObjectiveBreakdown { unary_cost: 0.0,
                pairwise_cost: 0.0, pairwise_lambda: 1.0, weighted_pairwise_cost: 0.0, total_cost: 0.0 },
            pairwise_decisions: Vec::new(), crop_reuse_heatmap: Vec::new(),
            validation: PlacementValidationSummary { complete_assignment: true, required_slots_present: true,
                isotropic_scale_only: true, registered_mapping_only: true, slot_count: 2 },
            qa_views: vec![PlacementPlanQaView::SelectedPlacements, PlacementPlanQaView::Validation],
        }
    }

    #[test]
    fn algorithm_stage_14_preview_a() {
        let first_domain = domain(0.2, b"persisted-authored-patch-a");
        let second_domain = domain(0.8, b"persisted-authored-patch-b");
        let first_plan = plan(1, &first_domain);
        let second_plan = plan(2, &second_domain);
        let first = synthesize_slot_material(SlotSynthesisRequest { plan: &first_plan,
            domain: &first_domain, output_dimensions: [4, 4], limits: SlotSynthesisLimits::default() },
            &RenderCancellationToken::new()).unwrap();
        let second = synthesize_slot_material(SlotSynthesisRequest { plan: &second_plan,
            domain: &second_domain, output_dimensions: [4, 4], limits: SlotSynthesisLimits::default() },
            &RenderCancellationToken::new()).unwrap();
        let topology = CompiledTemplateTopology {
            identity: TemplateIdentity { template_id: "representative-persisted".into(),
                template_version: "1.0.0".into(), compatibility_key: "stage-14-preview-a".into() },
            output_size: PixelSize { width: 8, height: 4 },
            slots: vec![
                CompiledTemplateSlot { slot_key: "authored-patch-a".into(),
                    allocation: CanonicalRect { x: 0, y: 0, width: 4, height: 4 },
                    hotspot: CanonicalRect { x: 1, y: 1, width: 2, height: 2 } },
                CompiledTemplateSlot { slot_key: "authored-patch-b".into(),
                    allocation: CanonicalRect { x: 4, y: 0, width: 4, height: 4 },
                    hotspot: CanonicalRect { x: 5, y: 1, width: 2, height: 2 } },
            ],
        };
        let placement = placement(vec![first_plan.clone(), second_plan.clone()]);
        let algorithms = (1..=14).map(|stage| (stage, AlgorithmProvenance {
            algorithm_id: format!("installed-stage-{stage}"), version: "1.0.0".into(),
        })).collect::<BTreeMap<_, _>>();
        let request = IntermediateAtlasRequest { topology: &topology, placement_plan: &placement,
            slots: vec![
                IntermediateSlotInput { region_id: first_plan.slot_id, slot_key: "authored-patch-a", display_name: "Authored Patch A",
                    required: true, patch_id: Some("patch-a".into()), domain: &first_domain, plan: &first_plan, result: &first, grid_rect: None },
                IntermediateSlotInput { region_id: second_plan.slot_id, slot_key: "authored-patch-b", display_name: "Authored Patch B",
                    required: true, patch_id: Some("patch-b".into()), domain: &second_domain, plan: &second_plan, result: &second, grid_rect: None },
            ], revision: 7, algorithm_versions: algorithms, diagnostics: Vec::new() };
        let compiler = AlgorithmCompiler::new();
        let artifact = compiler.compile_intermediate_atlas(&request, &CancellationToken::new(), || 7).unwrap();
        assert_eq!(artifact.topology, topology);
        assert_eq!(artifact.incomplete_after_stage, 14);
        assert!(artifact.non_exportable);
        assert_eq!(artifact.slots.len(), 2);
        assert!(artifact.slots.iter().all(|slot| slot.patch_id.is_some()
            && slot.valid_pixel_count == 16 && slot.mapping_mode == SamplingMode::DirectCrop));
        assert_eq!(artifact.channels.len(), 1);
        assert_eq!(artifact.channels[0].role, MaterialChannelRole::BaseColor);
        assert!(artifact.unavailable_channels.contains(&MaterialChannelRole::Height));
        assert!(artifact.pending.contains(&"final PBR composition"));
        assert_ne!(&artifact.channels[0].rgba8[0..4], &artifact.channels[0].rgba8[4 * 4..4 * 4 + 4]);

        let cancelled = CancellationToken::new(); cancelled.cancel();
        assert!(matches!(compiler.compile_intermediate_atlas(&request, &cancelled, || 7),
            Err(hot_trimmer_sheet_compiler::CompilerFacadeError::Intermediate(IntermediateAtlasError::Cancelled))));
        assert!(matches!(compiler.compile_intermediate_atlas(&request, &CancellationToken::new(), || 8),
            Err(hot_trimmer_sheet_compiler::CompilerFacadeError::Intermediate(IntermediateAtlasError::RevisionSuperseded))));

        let mut failed_placement = placement.clone();
        failed_placement.validation.required_slots_present = false;
        let failed = IntermediateAtlasRequest { placement_plan: &failed_placement, ..request };
        assert!(compiler.compile_intermediate_atlas(&failed, &CancellationToken::new(), || 7).is_err());

        let _normal_semantics_are_registered_not_color = NormalConvention::OpenGl;
        let _normal_alpha_is_preserved = NormalAlphaPolicy::Preserve;
    }
}

#[cfg(test)]
mod persisted_algorithm_stage_14_preview_a_tests {
    use std::{collections::HashSet, io::Cursor, path::PathBuf, sync::{Arc, Mutex}};

    use hot_trimmer_domain::{
        ContentDigest, ContentReference, MaterialBehaviorClass, MaterialClassificationCommand, NormalizedPoint, Patch, PatchCommand, PatchGeometry, PatchId,
        PatchProperties, PixelSize, RectificationSettings, SourceId, TrimSheetDocumentCommand,
    };
    use hot_trimmer_project_store::{ProjectStore, SourceChannel, SourceInput, SourceOwnership};
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
    use uuid::Uuid;

    use super::*;

    fn encoded_source() -> Vec<u8> {
        let image = RgbaImage::from_fn(192, 128, |x, y| {
            let course = (y / 12) % 2; let joint = ((x + course * 18) / 36) % 2;
            Rgba([120 + (x % 70) as u8, 45 + (y % 35) as u8, 30 + (joint * 25) as u8, 255])
        });
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image).write_to(&mut bytes, ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    #[test]
    fn algorithm_stage_14_preview_a() {
        let root = std::env::temp_dir().join(format!("hot-trimmer-preview-a-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let project_path = root.join("persisted.hottrimmer");
        let mut store = ProjectStore::create(&project_path, "Persisted Stage 14").unwrap();
        let supplied_path = std::env::var_os("HOT_TRIMMER_STAGE14_SOURCE").map(PathBuf::from);
        let bytes = supplied_path.as_ref().map_or_else(encoded_source, |path| fs::read(path).unwrap());
        let decoded = image::load_from_memory(&bytes).unwrap();
        let source_width = decoded.width(); let source_height = decoded.height();
        let base_color_source_id = SourceId::new();
        store.replace_source(SourceChannel::BaseColor, &SourceInput {
            id: base_color_source_id, ownership: SourceOwnership::OwnedCopy, external_path: None,
            origin_path: supplied_path.clone().unwrap_or_else(|| PathBuf::from("authored-brick.png")),
            sha256: ContentDigest::sha256(&bytes).0, width: source_width, height: source_height,
            format: if supplied_path.is_some() { "JPEG".into() } else { "PNG".into() },
            color_type: if supplied_path.is_some() { "Rgb8".into() } else { "Rgba8".into() },
            has_alpha: supplied_path.is_none(), exif_orientation: 1, has_embedded_icc_profile: false,
            encoded_bytes: u64::try_from(bytes.len()).unwrap(), owned_bytes: Some(bytes),
        }).unwrap();
        let patch_source_id = if supplied_path.is_some() { base_color_source_id } else {
            let roughness_bytes = encoded_source(); let patch_source_id = SourceId::new();
            store.replace_source(SourceChannel::Roughness, &SourceInput {
                id: patch_source_id, ownership: SourceOwnership::OwnedCopy, external_path: None,
                origin_path: PathBuf::from("authored-brick-roughness.png"),
                sha256: ContentDigest::sha256(&roughness_bytes).0, width: 192, height: 128,
                format: "PNG".into(), color_type: "Rgba8".into(), has_alpha: true,
                exif_orientation: 1, has_embedded_icc_profile: false,
                encoded_bytes: u64::try_from(roughness_bytes.len()).unwrap(), owned_bytes: Some(roughness_bytes),
            }).unwrap(); patch_source_id
        };
        let primary = store.summary().unwrap().source_sets[0].id;
        store.apply_material_classification_command(Uuid::from_bytes(primary.to_bytes()),
            MaterialClassificationCommand::Override { class: MaterialBehaviorClass::AlreadyTileable }).unwrap();
        let point = |x, y| NormalizedPoint::new(x, y).unwrap();
        let patch = Patch { id: PatchId::new(), source_id: patch_source_id, name: "Authored brick field".into(), enabled: true,
            geometry: PatchGeometry { corners: [point(0.05, 0.05), point(0.95, 0.05),
                point(0.95, 0.95), point(0.05, 0.95)], assistance_mask: None },
            properties: PatchProperties::default(), rectification: RectificationSettings::default() };
        store.execute_patch_command(&PatchCommand::Create { patch: patch.clone(), index: None }, None).unwrap();
        store.create_trim_sheet_document("ht.generic_architecture", "1.0.0").unwrap();
        let patch_region_id = store.summary().unwrap().document.unwrap().topology.regions[0].id;
        store.execute_document_command(&TrimSheetDocumentCommand::SetRegionContent {
            region_id: patch_region_id, content: ContentReference::Patch(patch.id),
        }).unwrap();
        store.execute_document_command(&TrimSheetDocumentCommand::SetOutputResolution {
            output_size: PixelSize { width: 128, height: 128 },
        }).unwrap();
        let revision = store.summary().unwrap().document.unwrap().document_revision;
        let session: SharedProjectSession = Arc::new(Mutex::new(ProjectSession {
            store: Some(store), dirty: false, is_draft: false, baseline: None,
            app_data_dir: root.join("app"), recovery_dir: root.join("recovery"), draft_dir: root.join("draft"),
            source_projection_cache: Mutex::new(HashMap::new()), preview_prepared_sources: HashMap::new(),
            prepared_exemplars: PreparedExemplarCache::default(), source_analysis_cache: SourceAnalysisCache::default(),
            scale_orientation_cache: ScaleOrientationCache::default(),
        }));
        let service = PreviewService::default();
        let job = service.latest_draft_id.fetch_add(1, Ordering::AcqRel).saturating_add(1);
        let artifact = build_stage_14_preview(&session, &service, Stage14PreviewRequest {
            protocol_version: IPC_PROTOCOL_VERSION, revision, region_id: None,
            transient_projection: None, draft_id: Some(job), input_hash: None,
        }, job).expect("persisted Stage 1-14 preview");
        assert_eq!(artifact.slots.len(), 53, "Generic Architecture must publish every fixed-template slot");
        assert_eq!(artifact.slots.iter().map(|slot| slot.region_id).collect::<HashSet<_>>().len(), 53,
            "fixed-template region identities must be stable and unique");
        assert!(artifact.slots.iter().all(|slot| slot.mapping_mode != "TextureSynthesis" && !slot.candidate_id.is_empty()
            && !slot.sampling_plan_id.is_empty() && !slot.stage_14_result_id.is_empty()
            && slot.isotropic_scale.is_finite() && slot.isotropic_scale > 0.0
            && slot.sampling_scale.is_finite() && slot.sampling_scale > 0.0),
            "unsupported synthesis candidates must not reach the published artifact");
        for (index, left) in artifact.slots.iter().enumerate() {
            for right in &artifact.slots[index + 1..] {
                let separated = left.allocation_bounds.x + left.allocation_bounds.width <= right.allocation_bounds.x
                    || right.allocation_bounds.x + right.allocation_bounds.width <= left.allocation_bounds.x
                    || left.allocation_bounds.y + left.allocation_bounds.height <= right.allocation_bounds.y
                    || right.allocation_bounds.y + right.allocation_bounds.height <= left.allocation_bounds.y;
                assert!(separated, "fixed-template atlas allocations overlap");
            }
        }
        assert_eq!(artifact.incomplete_after_stage, 14);
        assert!(artifact.non_exportable && !artifact.final_compile_available && !artifact.export_available && !artifact.blender_available);
        let expected_patch_id = patch.id.to_string();
        assert_eq!(artifact.slots.iter().find(|slot| slot.region_id == patch_region_id).unwrap()
            .patch_id.as_deref(), Some(expected_patch_id.as_str()));
        assert!(artifact.slots.iter().filter(|slot| slot.region_id != patch_region_id)
            .all(|slot| slot.patch_id.is_none()), "inherited material regions must not claim patch lineage");
        assert!(artifact.maps.contains_key("baseColor"));
        let crops = artifact.slots.iter().filter_map(|slot| slot.source_crop)
            .map(|crop| (crop.x, crop.y, crop.width, crop.height)).collect::<HashSet<_>>();
        assert!(crops.len() > 8, "Stage 11-13 must select purposeful spatially distinct crops across the persisted topology");
        let unplaced_crops = artifact.slots.iter().filter(|slot| slot.region_id != patch_region_id)
            .filter_map(|slot| slot.source_crop).collect::<Vec<_>>();
        assert!(unplaced_crops.iter().all(|crop| crop.width < source_width || crop.height < source_height),
            "unplaced Gate 1 slots must not consume the complete prepared source domain");
        let cornice = artifact.slots.iter().find(|slot| slot.slot_key == "cornice_long")
            .expect("Generic Architecture cornice slot");
        let cornice_crop = cornice.source_crop.expect("cornice must carry a selected crop");
        assert!(cornice_crop.width > cornice_crop.height.saturating_mul(4)
            && (cornice.mapping_mode == "RepeatX" || cornice.mapping_mode == "DirectCrop"),
            "long strips must carry a horizontal source cut or repeat route");
        let detail = artifact.slots.iter().find(|slot| slot.slot_key == "detail_cell_a")
            .expect("Generic Architecture detail slot");
        let detail_crop = detail.source_crop.expect("detail must carry a selected crop");
        assert!(detail_crop.width < source_width && detail_crop.height < source_height,
            "detail slots must carry a selected source detail, not the full source sheet");
        let radial = artifact.slots.iter().find(|slot| slot.slot_key == "radial_fixture_a")
            .expect("Generic Architecture radial slot");
        let radial_crop = radial.source_crop.expect("radial must carry a selected crop");
        assert!(radial_crop.width < source_width && radial_crop.height < source_height
            && radial_crop.width.abs_diff(radial_crop.height) <= source_width.max(source_height) / 2,
            "radial slots must carry a bounded radial/detail source area");
        assert!(artifact.slots.iter().all(|slot| !slot.candidate_id.is_empty()
            && !slot.sampling_plan_id.is_empty() && !slot.stage_14_result_id.is_empty()));
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
        recovery: diagnostic.recovery_choices.iter()
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
