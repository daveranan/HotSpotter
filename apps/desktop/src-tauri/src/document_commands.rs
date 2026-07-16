use std::{
    collections::HashMap,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, atomic::{AtomicU64, Ordering}},
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use hot_trimmer_domain::{
    ErrorCode, FoundationStatusRequest, IPC_PROTOCOL_VERSION, MaterialMapKind, PatchCommand,
    Projection, RegionId, SourceId,
    TrimSheetDocument, TrimSheetDocumentCommand, UserFacingError,
};
use hot_trimmer_image_io::{
    CancellationToken, ColorPolicy, DecodeLimits, InspectedImage, decode_rgba8_bytes_cancellable,
    inspect_bytes_with_policy, inspect_path_with_policy,
};
use hot_trimmer_project_store::{
    ProjectStore, SourceChannel, SourceInput, SourceOwnership, StoreError, StoredSource,
};
use hot_trimmer_sheet_compiler::{
    CompiledMapSet, PreviewMapKind, RegisteredMaterialMap, ResolvedRegion, compile_document,
    CompiledPreviewMap, compile_preview_map_incremental,
};
use image::{DynamicImage, ImageFormat, RgbaImage};
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
    source_projection_cache: Mutex<HashMap<(String, String), SourceProjection>>,
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
        let key = (source.input.id.to_string(), source.input.sha256.clone());
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
    settled_previews: Mutex<HashMap<(String, PreviewMapKind, u32), CompiledPreviewMap>>,
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
    source_set_id: String,
    channel: SourceChannel,
    display_name: String,
    source_path: String,
    width: u32,
    height: u32,
    thumbnail_data_url: String,
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
    sources: Vec<SourceProjection>,
    source_sets: Vec<hot_trimmer_project_store::SourceSetSnapshot>,
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
        .replace_source_in_set(request.source_set_id, request.channel, &input)
        .map_err(store_error)?;
    store.refresh_document_assets().map_err(store_error)?;
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
    let store = session.store.as_mut().ok_or_else(no_project)?;
    store.execute_patch_command(&request.command, request.coalescing_group).map_err(store_error)?;
    store.refresh_document_assets().map_err(store_error)?;
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
    let store = session.store.as_mut().ok_or_else(no_project)?;
    store.undo_patch_command().map_err(store_error)?;
    store.refresh_document_assets().map_err(store_error)?;
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
    let store = session.store.as_mut().ok_or_else(no_project)?;
    store.redo_patch_command().map_err(store_error)?;
    store.refresh_document_assets().map_err(store_error)?;
    session.mark_mutated();
    project_projection(&session)
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

fn compile_trim_sheet_document_impl(
    session: &SharedProjectSession,
    preview_service: &PreviewService,
) -> Result<CompiledSheetProjection, UserFacingError> {
    let summary = {
        let session = session.lock().map_err(|_| poisoned())?;
        session.store.as_ref().ok_or_else(no_project)?.summary().map_err(store_error)?
    };
    let document = summary.document.as_ref()
        .ok_or_else(|| error(ErrorCode::LayoutInvalid, "Create a trim sheet first."))?;
    let maps = summary
        .sources
        .iter()
        .map(|source| registered_map_cached(source, &preview_service))
        .collect::<Result<Vec<_>, _>>()?;
    let compiled = compile_document(document, &maps).map_err(|compile| {
        error(
            ErrorCode::LayoutInvalid,
            &format!("Build failed: {compile}"),
        )
    })?;
    let topology_hash = hash_hex(compiled.topology_hash);
    let appearance_hash = hash_hex(compiled.appearance_hash);
    let mut session = session.lock().map_err(|_| poisoned())?;
    let store = session.store.as_mut().ok_or_else(no_project)?;
    let current_revision = store.summary().map_err(store_error)?.document
        .map(|document| document.document_revision);
    if current_revision != Some(compiled.document_revision) {
        return Err(error(ErrorCode::OperationCancelled, "A newer edit superseded this build."));
    }
    store
        .record_compiled_artifact(
            compiled.document_revision,
            &topology_hash,
            &appearance_hash,
            &compiled.renderer_version,
        )
        .map_err(store_error)?;
    let width = compiled.dimensions.width;
    let height = compiled.dimensions.height;
    Ok(CompiledSheetProjection {
        document_revision: compiled.document_revision,
        topology_hash,
        appearance_hash,
        renderer_version: compiled.renderer_version,
        width,
        height,
        maps: encode_maps(width, height, compiled.maps)?,
        regions: compiled.regions,
    })
}

#[tauri::command]
pub async fn preview_trim_sheet_document(
    request: PreviewDocumentRequest,
    session: State<'_, SharedProjectSession>,
    preview_service: State<'_, SharedPreviewService>,
) -> Result<PreviewSheetProjection, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let session = Arc::clone(session.inner());
    let preview_service = Arc::clone(preview_service.inner());
    preview_service.latest_draft_id.store(request.draft_id, Ordering::Release);
    tauri::async_runtime::spawn_blocking(move || {
        preview_trim_sheet_document_impl(request, &session, &preview_service)
    })
    .await
    .map_err(|join| error(ErrorCode::Internal, &format!("Preview worker failed: {join}")))?
}

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
        request.region_id,
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
    Ok(ProjectProjection {
        id: summary.id.to_string(),
        name: summary.name,
        path: summary.path.display().to_string(),
        schema_version: hot_trimmer_project_store::CURRENT_SCHEMA_VERSION,
        dirty: session.dirty,
        is_draft: session.is_draft,
        sources: summary
            .sources
            .iter()
            .map(|source| session.source_projection_cached(source))
            .collect::<Result<Vec<_>, _>>()?,
        source_sets: summary.source_sets,
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
        source_set_id: source.source_set_id.to_string(),
        channel: source.channel,
        display_name: source.input.origin_path.file_name().map_or_else(
            || source.input.origin_path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        ),
        source_path: source.input.origin_path.display().to_string(),
        width: inspected.info.width,
        height: inspected.info.height,
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
