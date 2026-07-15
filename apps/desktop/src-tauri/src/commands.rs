use std::{
    cmp::Reverse,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::UNIX_EPOCH,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use hot_trimmer_domain::{
    ErrorCode, FoundationStatusRequest, IPC_PROTOCOL_VERSION, NormalizedPoint, Patch, PatchCommand,
    PatchGeometry, PatchId, SourceId, UserFacingError,
};
use hot_trimmer_geometry::{
    Quadrilateral, RectificationLimits, assist_polygon, rectified_dimensions,
};
use hot_trimmer_image_io::{
    CancellationToken, ColorPolicy, DecodeLimits, ImageIoError, InspectedImage,
    decode_rgba8_bytes_cancellable, inspect_bytes_with_policy, inspect_path_cancellable,
    inspect_path_with_policy,
};
use hot_trimmer_project_store::{
    ProjectStore, SourceChannel, SourceInput, SourceOwnership, StoreError, StoredSource,
};
use hot_trimmer_render_core::{
    RectificationRequest, RenderCancellationToken, RenderError, SampleSpace, SamplingFilter,
    rectify_rgba8_with_progress,
};
use image::{DynamicImage, ImageFormat, RgbaImage};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::paths::AppPaths;

const MAX_RECENT_PROJECTS: usize = 10;
const MAX_IPC_PATH_UTF16: usize = 32_767;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeDirectories {
    app_data: String,
    cache: String,
    logs: String,
    recovery: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundationStatus {
    protocol_version: u16,
    app_version: &'static str,
    platform: &'static str,
    directories: NativeDirectories,
    capabilities: [&'static str; 4],
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupStatus {
    previous_shutdown_clean: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct StartupState {
    pub previous_shutdown_clean: bool,
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
pub struct ProjectNameRequest {
    protocol_version: u16,
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSourceRequest {
    protocol_version: u16,
    path: String,
    ownership: SourceOwnership,
    channel: SourceChannel,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSlotRequest {
    protocol_version: u16,
    channel: SourceChannel,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchCommandRequest {
    protocol_version: u16,
    command: PatchCommand,
    coalescing_group: Option<u64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolygonAssistRequest {
    protocol_version: u16,
    points: Vec<NormalizedPoint>,
    retain_mask: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchPreviewRequest {
    protocol_version: u16,
    patch_id: PatchId,
    max_edge: u32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftPatchPreviewRequest {
    protocol_version: u16,
    preview_id: PatchId,
    source_id: SourceId,
    geometry: PatchGeometry,
    rectification: hot_trimmer_domain::RectificationSettings,
    max_edge: u32,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseDisposition {
    Save,
    Discard,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseProjectRequest {
    protocol_version: u16,
    disposition: CloseDisposition,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoverProjectRequest {
    protocol_version: u16,
    recovery_path: String,
    destination_path: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailMipmapSnapshot {
    max_edge: u32,
    data_url: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSnapshot {
    id: String,
    channel: SourceChannel,
    ownership: SourceOwnership,
    display_name: String,
    source_path: String,
    width: u32,
    height: u32,
    format: String,
    color_type: String,
    has_alpha: bool,
    exif_orientation: u16,
    has_embedded_icc_profile: bool,
    icc_converted_to_srgb: bool,
    encoded_bytes: u64,
    thumbnail_data_url: String,
    thumbnail_mipmaps: Vec<ThumbnailMipmapSnapshot>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)] // Flat IPC snapshot keeps independent UI states backward compatible.
pub struct ProjectSnapshot {
    id: String,
    name: String,
    path: String,
    schema_version: u32,
    dirty: bool,
    stale_lock_recovered: bool,
    sources: Vec<SourceSnapshot>,
    patches: Vec<Patch>,
    can_undo_patch: bool,
    can_redo_patch: bool,
    warnings: Vec<ProjectWarning>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchPreviewSnapshot {
    patch_id: PatchId,
    width: u32,
    height: u32,
    data_url: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchStateSnapshot {
    patches: Vec<Patch>,
    dirty: bool,
    can_undo_patch: bool,
    can_redo_patch: bool,
    warnings: Vec<ProjectWarning>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchPreviewProgress {
    patch_id: PatchId,
    stage: &'static str,
    fraction: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWarning {
    code: ErrorCode,
    message: String,
    recovery: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentProject {
    name: String,
    path: String,
    last_opened_unix: i64,
    available: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryCandidate {
    project_id: String,
    project_name: String,
    path: String,
    modified_unix: i64,
    source_count: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    stage: &'static str,
    fraction: f32,
}

pub struct ProjectSession {
    store: Option<ProjectStore>,
    dirty: bool,
    baseline: Option<PathBuf>,
    recovery_dir: PathBuf,
    app_data_dir: PathBuf,
}

impl ProjectSession {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            store: None,
            dirty: false,
            baseline: None,
            recovery_dir: paths.recovery.clone(),
            app_data_dir: paths.app_data.clone(),
        }
    }

    fn ensure_replaceable(&self) -> Result<(), UserFacingError> {
        if self.dirty {
            Err(dirty_project())
        } else {
            Ok(())
        }
    }

    fn adopt(&mut self, store: ProjectStore) -> Result<(), UserFacingError> {
        let baseline = store.baseline_path(&self.recovery_dir);
        store.backup_atomic(&baseline).map_err(store_error)?;
        if let Some(previous) = self.baseline.replace(baseline.clone()) {
            let _ = fs::remove_file(previous);
        }
        cleanup_stale_baselines(&self.recovery_dir, &baseline);
        self.store = Some(store);
        self.dirty = false;
        Ok(())
    }

    fn save(&mut self) -> Result<(), UserFacingError> {
        let store = self.store.as_ref().ok_or_else(no_open_project)?;
        store.save().map_err(store_error)?;
        let baseline = store.baseline_path(&self.recovery_dir);
        store.backup_atomic(&baseline).map_err(store_error)?;
        store
            .create_recovery_snapshot(&self.recovery_dir)
            .map_err(store_error)?;
        if let Some(previous) = self.baseline.replace(baseline.clone()) {
            if previous != baseline {
                let _ = fs::remove_file(previous);
            }
        }
        self.dirty = false;
        Ok(())
    }

    fn replace_source_and_refresh_recovery(
        &mut self,
        channel: SourceChannel,
        source: &SourceInput,
    ) -> Result<Option<UserFacingError>, UserFacingError> {
        self.store
            .as_mut()
            .ok_or_else(no_open_project)?
            .replace_source(channel, source)
            .map_err(store_error)?;
        self.dirty = true;
        Ok(self
            .store
            .as_ref()
            .ok_or_else(no_open_project)?
            .create_recovery_snapshot(&self.recovery_dir)
            .err()
            .map(recovery_refresh_warning))
    }

    fn remove_source_and_refresh_recovery(
        &mut self,
        channel: SourceChannel,
    ) -> Result<Option<UserFacingError>, UserFacingError> {
        self.store
            .as_mut()
            .ok_or_else(no_open_project)?
            .remove_source(channel)
            .map_err(store_error)?;
        self.dirty = true;
        Ok(self
            .store
            .as_ref()
            .ok_or_else(no_open_project)?
            .create_recovery_snapshot(&self.recovery_dir)
            .err()
            .map(recovery_refresh_warning))
    }

    fn rename_and_refresh_recovery(
        &mut self,
        name: &str,
    ) -> Result<Option<UserFacingError>, UserFacingError> {
        self.store
            .as_mut()
            .ok_or_else(no_open_project)?
            .rename_project(name)
            .map_err(store_error)?;
        self.dirty = true;
        Ok(self
            .store
            .as_ref()
            .ok_or_else(no_open_project)?
            .create_recovery_snapshot(&self.recovery_dir)
            .err()
            .map(recovery_refresh_warning))
    }

    fn apply_patch_and_refresh_recovery(
        &mut self,
        command: &PatchCommand,
        coalescing_group: Option<u64>,
    ) -> Result<Option<UserFacingError>, UserFacingError> {
        validate_patch_command_geometry(command)?;
        self.store
            .as_mut()
            .ok_or_else(no_open_project)?
            .execute_patch_command(command, coalescing_group)
            .map_err(store_error)?;
        self.dirty = true;
        Ok(self
            .store
            .as_ref()
            .ok_or_else(no_open_project)?
            .create_recovery_snapshot(&self.recovery_dir)
            .err()
            .map(recovery_refresh_warning))
    }

    fn patch_history_and_refresh_recovery(
        &mut self,
        redo: bool,
    ) -> Result<Option<UserFacingError>, UserFacingError> {
        let store = self.store.as_mut().ok_or_else(no_open_project)?;
        if redo {
            store.redo_patch_command()
        } else {
            store.undo_patch_command()
        }
        .map_err(store_error)?;
        self.dirty = true;
        Ok(self
            .store
            .as_ref()
            .ok_or_else(no_open_project)?
            .create_recovery_snapshot(&self.recovery_dir)
            .err()
            .map(recovery_refresh_warning))
    }

    fn close(&mut self, disposition: CloseDisposition) -> Result<(), UserFacingError> {
        if self.store.is_none() {
            return Ok(());
        }
        if self.dirty {
            match disposition {
                CloseDisposition::Save => self.save()?,
                CloseDisposition::Discard => {
                    let baseline = self.baseline.as_ref().ok_or_else(|| {
                        user_error(
                            ErrorCode::RecoveryFailed,
                            "The last saved project baseline is unavailable.",
                            "Save the project or keep it open; discard was not performed.",
                            None,
                        )
                    })?;
                    self.store
                        .as_mut()
                        .ok_or_else(no_open_project)?
                        .restore_from(baseline)
                        .map_err(store_error)?;
                }
            }
        }
        self.store = None;
        if let Some(baseline) = self.baseline.take() {
            let _ = fs::remove_file(baseline);
        }
        self.dirty = false;
        Ok(())
    }
}

pub type SharedProjectSession = Arc<Mutex<ProjectSession>>;
pub type PendingProjectPath = Arc<Mutex<Option<String>>>;
pub type SharedImportJob = Arc<Mutex<Option<CancellationToken>>>;

#[derive(Clone, Debug)]
pub struct PatchPreviewJob {
    id: Uuid,
    decode: CancellationToken,
    render: RenderCancellationToken,
}

impl PatchPreviewJob {
    fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            decode: CancellationToken::new(),
            render: RenderCancellationToken::new(),
        }
    }

    fn cancel(&self) {
        self.decode.cancel();
        self.render.cancel();
    }
}

pub type SharedPatchPreviewJob = Arc<Mutex<Option<PatchPreviewJob>>>;

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn foundation_status(
    request: FoundationStatusRequest,
    paths: State<'_, AppPaths>,
) -> Result<FoundationStatus, UserFacingError> {
    request.validate().map_err(UserFacingError::from)?;
    Ok(FoundationStatus {
        protocol_version: IPC_PROTOCOL_VERSION,
        app_version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
        directories: NativeDirectories {
            app_data: paths.app_data.display().to_string(),
            cache: paths.cache.display().to_string(),
            logs: paths.logs.display().to_string(),
            recovery: paths.recovery.display().to_string(),
        },
        capabilities: [
            "native_paths",
            "typed_ipc",
            "structured_diagnostics",
            "native_dialog",
        ],
    })
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri extracts managed state by value.
pub fn startup_status(
    request: FoundationStatusRequest,
    startup: State<'_, StartupState>,
) -> Result<StartupStatus, UserFacingError> {
    request.validate().map_err(UserFacingError::from)?;
    Ok(StartupStatus {
        previous_shutdown_clean: startup.previous_shutdown_clean,
    })
}

#[tauri::command]
pub async fn create_project(
    request: CreateProjectRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectSnapshot, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let path = validate_ipc_path(&request.path)?;
    let name = request.name.trim().to_owned();
    if name.is_empty() || name.len() > 255 {
        return Err(user_error(
            ErrorCode::InvalidInput,
            "Enter a project name between 1 and 255 characters.",
            "Edit the project name and retry.",
            None,
        ));
    }
    let shared = Arc::clone(&session);
    run_blocking(move || {
        let mut guard = shared.lock().map_err(|_| session_poisoned())?;
        guard.ensure_replaceable()?;
        let store = ProjectStore::create(&path, &name).map_err(store_error)?;
        guard.adopt(store)?;
        snapshot_adopted_session(&mut guard)
    })
    .await
}

#[tauri::command]
pub async fn open_project(
    request: ProjectPathRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectSnapshot, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let path = validate_ipc_path(&request.path)?;
    let shared = Arc::clone(&session);
    run_blocking(move || {
        let mut guard = shared.lock().map_err(|_| session_poisoned())?;
        guard.ensure_replaceable()?;
        let store = ProjectStore::open(&path).map_err(store_error)?;
        guard.adopt(store)?;
        snapshot_adopted_session(&mut guard)
    })
    .await
}

#[tauri::command]
pub async fn import_source(
    request: ImportSourceRequest,
    session: State<'_, SharedProjectSession>,
    import_job: State<'_, SharedImportJob>,
    app: AppHandle,
) -> Result<ProjectSnapshot, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let path = validate_ipc_path(&request.path)?;
    let ownership = request.ownership;
    let channel = request.channel;
    let shared = Arc::clone(&session);
    let jobs = Arc::clone(&import_job);
    let cancellation = CancellationToken::new();
    *jobs.lock().map_err(|_| session_poisoned())? = Some(cancellation.clone());
    let cleanup_token = cancellation.clone();
    let cleanup_jobs = Arc::clone(&jobs);
    let result = run_blocking(move || {
        emit_import_progress(&app, "Reading and decoding", 0.05);
        let inspected = inspect_path_cancellable(
            &path,
            DecodeLimits::default(),
            color_policy(channel),
            &cancellation,
        )
        .map_err(image_error)?;
        emit_import_progress(&app, "Validating registration", 0.82);
        if cancellation.is_cancelled() {
            return Err(cancelled());
        }
        let source = source_input(&path, ownership, &inspected);
        let source_id = source.id;
        let mut guard = shared.lock().map_err(|_| session_poisoned())?;
        emit_import_progress(&app, "Writing recovery snapshot", 0.92);
        let recovery_warning = guard.replace_source_and_refresh_recovery(channel, &source)?;
        emit_import_progress(&app, "Complete", 1.0);
        let mut snapshot = snapshot_session(&guard, Some((source_id, inspected)))?;
        if let Some(warning) = recovery_warning {
            snapshot.warnings.push(ProjectWarning {
                code: warning.code,
                message: warning.message,
                recovery: warning.recovery,
            });
        }
        Ok(snapshot)
    })
    .await;
    if let Ok(mut current) = cleanup_jobs.lock()
        && current
            .as_ref()
            .is_some_and(|token| token.same_job(&cleanup_token))
    {
        *current = None;
    }
    result
}

#[tauri::command]
pub async fn generate_draft_patch_preview(
    request: DraftPatchPreviewRequest,
    session: State<'_, SharedProjectSession>,
    preview_job: State<'_, SharedPatchPreviewJob>,
    app: AppHandle,
) -> Result<PatchPreviewSnapshot, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    if !(64..=2048).contains(&request.max_edge) {
        return Err(user_error(
            ErrorCode::InvalidInput,
            "Patch preview size is outside the supported range.",
            "Choose a preview size from 64 to 2048 pixels.",
            None,
        ));
    }
    let patch = Patch {
        id: request.preview_id,
        source_id: request.source_id,
        name: "Draft patch".into(),
        enabled: true,
        geometry: request.geometry,
        properties: hot_trimmer_domain::PatchProperties::default(),
        rectification: request.rectification,
    };
    if !patch.has_valid_metadata() {
        return Err(user_error(
            ErrorCode::InvalidInput,
            "Draft patch metadata is invalid.",
            "Use a valid source, patch identifier, and rectification settings.",
            None,
        ));
    }
    validate_patch_command_geometry(&PatchCommand::Create {
        patch: patch.clone(),
        index: None,
    })?;
    let source = {
        let guard = session.lock().map_err(|_| session_poisoned())?;
        guard
            .store
            .as_ref()
            .ok_or_else(no_open_project)?
            .summary()
            .map_err(store_error)?
            .sources
            .into_iter()
            .find(|source| source.input.id == patch.source_id)
            .ok_or_else(|| store_error(StoreError::InvalidData("draft patch source".into())))?
    };
    let job = PatchPreviewJob::new();
    {
        let mut current = preview_job.lock().map_err(|_| session_poisoned())?;
        if let Some(previous) = current.replace(job.clone()) {
            previous.cancel();
        }
    }
    let jobs = Arc::clone(&preview_job);
    let result_job = job.clone();
    let max_edge = request.max_edge;
    let result =
        run_blocking(move || render_patch_preview(&patch, &source, max_edge, &job, &app)).await;
    if let Ok(mut current) = jobs.lock()
        && current
            .as_ref()
            .is_some_and(|active| active.id == result_job.id)
    {
        *current = None;
    }
    result
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri extracts managed state by value.
pub fn cancel_import(
    request: FoundationStatusRequest,
    import_job: State<'_, SharedImportJob>,
) -> Result<(), UserFacingError> {
    request.validate().map_err(UserFacingError::from)?;
    if let Some(job) = import_job.lock().map_err(|_| session_poisoned())?.as_ref() {
        job.cancel();
    }
    Ok(())
}

#[tauri::command]
pub async fn remove_source(
    request: SourceSlotRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectSnapshot, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let shared = Arc::clone(&session);
    run_blocking(move || {
        let mut guard = shared.lock().map_err(|_| session_poisoned())?;
        let recovery_warning = guard.remove_source_and_refresh_recovery(request.channel)?;
        let mut snapshot = snapshot_session(&guard, None)?;
        if let Some(warning) = recovery_warning {
            snapshot.warnings.push(ProjectWarning {
                code: warning.code,
                message: warning.message,
                recovery: warning.recovery,
            });
        }
        Ok(snapshot)
    })
    .await
}

#[tauri::command]
pub async fn rename_project(
    request: ProjectNameRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectSnapshot, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let shared = Arc::clone(&session);
    run_blocking(move || {
        let mut guard = shared.lock().map_err(|_| session_poisoned())?;
        let recovery_warning = guard.rename_and_refresh_recovery(&request.name)?;
        let mut snapshot = snapshot_session(&guard, None)?;
        remember_open_project_best_effort(&guard);
        if let Some(warning) = recovery_warning {
            snapshot.warnings.push(ProjectWarning {
                code: warning.code,
                message: warning.message,
                recovery: warning.recovery,
            });
        }
        Ok(snapshot)
    })
    .await
}

#[tauri::command]
pub async fn apply_patch_command(
    request: PatchCommandRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<PatchStateSnapshot, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let shared = Arc::clone(&session);
    run_blocking(move || {
        let mut guard = shared.lock().map_err(|_| session_poisoned())?;
        let warning =
            guard.apply_patch_and_refresh_recovery(&request.command, request.coalescing_group)?;
        patch_state_snapshot(&guard, warning)
    })
    .await
}

#[tauri::command]
pub async fn undo_patch_command(
    request: FoundationStatusRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<PatchStateSnapshot, UserFacingError> {
    request.validate().map_err(UserFacingError::from)?;
    let shared = Arc::clone(&session);
    run_blocking(move || {
        let mut guard = shared.lock().map_err(|_| session_poisoned())?;
        let warning = guard.patch_history_and_refresh_recovery(false)?;
        patch_state_snapshot(&guard, warning)
    })
    .await
}

#[tauri::command]
pub async fn redo_patch_command(
    request: FoundationStatusRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<PatchStateSnapshot, UserFacingError> {
    request.validate().map_err(UserFacingError::from)?;
    let shared = Arc::clone(&session);
    run_blocking(move || {
        let mut guard = shared.lock().map_err(|_| session_poisoned())?;
        let warning = guard.patch_history_and_refresh_recovery(true)?;
        patch_state_snapshot(&guard, warning)
    })
    .await
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn fit_patch_polygon(request: PolygonAssistRequest) -> Result<PatchGeometry, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let assistance =
        assist_polygon(&request.points, request.retain_mask).map_err(UserFacingError::from)?;
    Ok(PatchGeometry {
        corners: assistance.quadrilateral.corners(),
        assistance_mask: assistance.mask,
    })
}

#[tauri::command]
#[allow(clippy::too_many_lines)] // Keeps job setup, cancellation ownership, and cleanup auditable together.
pub async fn generate_patch_preview(
    request: PatchPreviewRequest,
    session: State<'_, SharedProjectSession>,
    preview_job: State<'_, SharedPatchPreviewJob>,
    app: AppHandle,
) -> Result<PatchPreviewSnapshot, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    if !(64..=2048).contains(&request.max_edge) {
        return Err(user_error(
            ErrorCode::InvalidInput,
            "Patch preview size is outside the supported range.",
            "Choose a preview size from 64 to 2048 pixels.",
            None,
        ));
    }
    let (patch, source) = {
        let guard = session.lock().map_err(|_| session_poisoned())?;
        let summary = guard
            .store
            .as_ref()
            .ok_or_else(no_open_project)?
            .summary()
            .map_err(store_error)?;
        let patch = summary
            .patches
            .into_iter()
            .find(|patch| patch.id == request.patch_id)
            .ok_or_else(|| {
                user_error(
                    ErrorCode::InvalidInput,
                    "The selected patch no longer exists.",
                    "Select another patch or create a new one.",
                    None,
                )
            })?;
        let source = summary
            .sources
            .into_iter()
            .find(|source| source.input.id == patch.source_id)
            .ok_or_else(|| store_error(StoreError::InvalidData("patch source".into())))?;
        (patch, source)
    };
    let job = PatchPreviewJob::new();
    {
        let mut current = preview_job.lock().map_err(|_| session_poisoned())?;
        if let Some(previous) = current.replace(job.clone()) {
            previous.cancel();
        }
    }
    let jobs = Arc::clone(&preview_job);
    let result_job = job.clone();
    let result =
        run_blocking(move || render_patch_preview(&patch, &source, request.max_edge, &job, &app))
            .await;
    if let Ok(mut current) = jobs.lock()
        && current
            .as_ref()
            .is_some_and(|active| active.id == result_job.id)
    {
        *current = None;
    }
    result
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn cancel_patch_preview(
    request: FoundationStatusRequest,
    preview_job: State<'_, SharedPatchPreviewJob>,
) -> Result<(), UserFacingError> {
    request.validate().map_err(UserFacingError::from)?;
    if let Some(job) = preview_job.lock().map_err(|_| session_poisoned())?.as_ref() {
        job.cancel();
    }
    Ok(())
}

#[tauri::command]
pub async fn save_project(
    request: FoundationStatusRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectSnapshot, UserFacingError> {
    request.validate().map_err(UserFacingError::from)?;
    let shared = Arc::clone(&session);
    run_blocking(move || {
        let mut guard = shared.lock().map_err(|_| session_poisoned())?;
        guard.save()?;
        snapshot_session(&guard, None)
    })
    .await
}

#[tauri::command]
pub async fn save_project_as(
    request: ProjectPathRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectSnapshot, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let destination = validate_ipc_path(&request.path)?;
    let shared = Arc::clone(&session);
    run_blocking(move || {
        let mut guard = shared.lock().map_err(|_| session_poisoned())?;
        let copied = guard
            .store
            .as_ref()
            .ok_or_else(no_open_project)?
            .save_as(&destination)
            .map_err(store_error)?;
        guard.adopt(copied)?;
        snapshot_adopted_session(&mut guard)
    })
    .await
}

#[tauri::command]
pub async fn close_project(
    request: CloseProjectRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<(), UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let shared = Arc::clone(&session);
    run_blocking(move || {
        shared
            .lock()
            .map_err(|_| session_poisoned())?
            .close(request.disposition)
    })
    .await
}

#[tauri::command]
pub async fn list_recent_projects(
    request: FoundationStatusRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<Vec<RecentProject>, UserFacingError> {
    request.validate().map_err(UserFacingError::from)?;
    let shared = Arc::clone(&session);
    run_blocking(move || {
        let guard = shared.lock().map_err(|_| session_poisoned())?;
        read_recent_projects(&guard.app_data_dir)
    })
    .await
}

#[tauri::command]
pub async fn list_recovery_candidates(
    request: FoundationStatusRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<Vec<RecoveryCandidate>, UserFacingError> {
    request.validate().map_err(UserFacingError::from)?;
    let shared = Arc::clone(&session);
    run_blocking(move || {
        let guard = shared.lock().map_err(|_| session_poisoned())?;
        scan_recovery_candidates(&guard.recovery_dir)
    })
    .await
}

#[tauri::command]
pub async fn recover_project(
    request: RecoverProjectRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectSnapshot, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let source = validate_ipc_path(&request.recovery_path)?;
    let destination = validate_ipc_path(&request.destination_path)?;
    let shared = Arc::clone(&session);
    run_blocking(move || {
        let mut guard = shared.lock().map_err(|_| session_poisoned())?;
        guard.ensure_replaceable()?;
        publish_recovery_copy(&source, &destination)?;
        let store = ProjectStore::open(&destination).map_err(store_error)?;
        guard.adopt(store)?;
        snapshot_adopted_session(&mut guard)
    })
    .await
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri extracts managed state by value.
pub fn take_pending_project_path(
    request: FoundationStatusRequest,
    pending: State<'_, PendingProjectPath>,
) -> Result<Option<String>, UserFacingError> {
    request.validate().map_err(UserFacingError::from)?;
    pending
        .lock()
        .map(|mut path| path.take())
        .map_err(|_| session_poisoned())
}

async fn run_blocking<T, F>(task: F) -> Result<T, UserFacingError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, UserFacingError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|error| {
            user_error(
                ErrorCode::Internal,
                "The background project operation stopped unexpectedly.",
                "Retry the operation. Restart Hot Trimmer if it happens again.",
                Some(error.to_string()),
            )
        })?
}

fn snapshot_session(
    session: &ProjectSession,
    mut inspected_override: Option<(SourceId, InspectedImage)>,
) -> Result<ProjectSnapshot, UserFacingError> {
    let store = session.store.as_ref().ok_or_else(no_open_project)?;
    let summary = store.summary().map_err(store_error)?;
    let mut sources = Vec::with_capacity(summary.sources.len());
    for stored in &summary.sources {
        let inspected = if inspected_override
            .as_ref()
            .is_some_and(|(id, _)| *id == stored.input.id)
        {
            inspected_override
                .take()
                .map(|(_, image)| image)
                .ok_or_else(session_poisoned)?
        } else {
            inspect_stored(stored)?
        };
        sources.push(source_snapshot(stored, inspected));
    }
    Ok(ProjectSnapshot {
        id: summary.id.to_string(),
        name: summary.name,
        path: summary.path.display().to_string(),
        schema_version: hot_trimmer_project_store::CURRENT_SCHEMA_VERSION,
        dirty: session.dirty,
        stale_lock_recovered: summary.stale_lock_recovered,
        sources,
        patches: summary.patches,
        can_undo_patch: store.can_undo_patch_command(),
        can_redo_patch: store.can_redo_patch_command(),
        warnings: Vec::new(),
    })
}

fn patch_state_snapshot(
    session: &ProjectSession,
    warning: Option<UserFacingError>,
) -> Result<PatchStateSnapshot, UserFacingError> {
    let store = session.store.as_ref().ok_or_else(no_open_project)?;
    let warnings = warning
        .map(|warning| {
            vec![ProjectWarning {
                code: warning.code,
                message: warning.message,
                recovery: warning.recovery,
            }]
        })
        .unwrap_or_default();
    Ok(PatchStateSnapshot {
        patches: store.patches().to_vec(),
        dirty: session.dirty,
        can_undo_patch: store.can_undo_patch_command(),
        can_redo_patch: store.can_redo_patch_command(),
        warnings,
    })
}

fn validate_patch_command_geometry(command: &PatchCommand) -> Result<(), UserFacingError> {
    let geometry = match command {
        PatchCommand::Create { patch, .. } => Some(&patch.geometry),
        PatchCommand::ReplaceGeometry { geometry, .. } => Some(geometry),
        _ => None,
    };
    if let Some(geometry) = geometry {
        Quadrilateral::new(geometry.corners).map_err(UserFacingError::from)?;
        if let Some(mask) = &geometry.assistance_mask
            && !(4..=8).contains(&mask.len())
        {
            return Err(user_error(
                ErrorCode::PatchGeometryInvalid,
                "A polygon assistance mask must contain four through eight points.",
                "Refit the polygon with four through eight boundary points.",
                None,
            ));
        }
    }
    Ok(())
}

fn fit_preview_dimensions(width: u32, height: u32, max_edge: u32) -> (u32, u32) {
    if width <= max_edge && height <= max_edge {
        return (width, height);
    }
    let scale = f64::from(max_edge) / f64::from(width.max(height));
    let scaled_width = (f64::from(width) * scale).round().max(1.0);
    let scaled_height = (f64::from(height) * scale).round().max(1.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (scaled_width as u32, scaled_height as u32)
}

fn render_patch_preview(
    patch: &Patch,
    source: &StoredSource,
    max_edge: u32,
    job: &PatchPreviewJob,
    app: &AppHandle,
) -> Result<PatchPreviewSnapshot, UserFacingError> {
    emit_patch_preview_progress(app, patch.id, "Decoding source", 0.02);
    let inspected = inspect_stored(source)?;
    let decoded = decode_rgba8_bytes_cancellable(
        &inspected.source_bytes,
        DecodeLimits::default(),
        color_policy(source.channel),
        &job.decode,
    )
    .map_err(image_error)?;
    emit_patch_preview_progress(app, patch.id, "Rectifying patch", 0.35);
    let quadrilateral =
        Quadrilateral::new(patch.geometry.corners).map_err(UserFacingError::from)?;
    let natural = rectified_dimensions(
        quadrilateral,
        decoded.width,
        decoded.height,
        patch.rectification,
        RectificationLimits::default(),
    )
    .map_err(UserFacingError::from)?;
    let (output_width, output_height) =
        fit_preview_dimensions(natural.width, natural.height, max_edge);
    let rendered = rectify_rgba8_with_progress(
        RectificationRequest {
            source_rgba8: &decoded.pixels,
            source_width: decoded.width,
            source_height: decoded.height,
            quadrilateral,
            output_width,
            output_height,
            sampling: SamplingFilter::Bilinear,
            sample_space: if source.channel == SourceChannel::BaseColor {
                SampleSpace::SrgbColor
            } else {
                SampleSpace::LinearData
            },
        },
        &job.render,
        |fraction| {
            emit_patch_preview_progress(app, patch.id, "Rectifying patch", 0.35 + fraction * 0.58);
        },
    )
    .map_err(render_error)?;
    let image = RgbaImage::from_raw(rendered.width, rendered.height, rendered.rgba8)
        .ok_or_else(|| render_error(RenderError::OutputTooLarge))?;
    let mut encoded = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut encoded, ImageFormat::Png)
        .map_err(|error| {
            user_error(
                ErrorCode::Internal,
                "The rectified preview could not be encoded.",
                "Retry the preview. Restart Hot Trimmer if the problem continues.",
                Some(error.to_string()),
            )
        })?;
    emit_patch_preview_progress(app, patch.id, "Complete", 1.0);
    Ok(PatchPreviewSnapshot {
        patch_id: patch.id,
        width: rendered.width,
        height: rendered.height,
        data_url: format!(
            "data:image/png;base64,{}",
            STANDARD.encode(encoded.into_inner())
        ),
    })
}

fn snapshot_adopted_session(
    session: &mut ProjectSession,
) -> Result<ProjectSnapshot, UserFacingError> {
    match snapshot_session(session, None) {
        Ok(snapshot) => {
            remember_open_project_best_effort(session);
            Ok(snapshot)
        }
        Err(error) => {
            session.store = None;
            if let Some(baseline) = session.baseline.take() {
                let _ = fs::remove_file(baseline);
            }
            session.dirty = false;
            Err(error)
        }
    }
}

fn inspect_stored(source: &StoredSource) -> Result<InspectedImage, UserFacingError> {
    let input = &source.input;
    let policy = color_policy(source.channel);
    match input.ownership {
        SourceOwnership::OwnedCopy => inspect_bytes_with_policy(
            input
                .owned_bytes
                .clone()
                .ok_or_else(|| store_error(StoreError::InvalidData("owned source bytes".into())))?,
            DecodeLimits::default(),
            policy,
        )
        .map_err(image_error),
        SourceOwnership::VerifiedExternalReference => {
            let path = input.external_path.as_ref().ok_or_else(|| {
                store_error(StoreError::InvalidData("external source path".into()))
            })?;
            let inspected = inspect_path_with_policy(path, DecodeLimits::default(), policy)
                .map_err(image_error)?;
            if inspected.info.sha256 != input.sha256 {
                return Err(user_error(
                    ErrorCode::ImageImportFailed,
                    "An externally referenced source has changed.",
                    "Restore the original source or explicitly import the changed file again.",
                    Some(format!("channel={}", source.channel.as_db_value())),
                ));
            }
            Ok(inspected)
        }
    }
}

const fn color_policy(channel: SourceChannel) -> ColorPolicy {
    if matches!(channel, SourceChannel::BaseColor) {
        ColorPolicy::ConvertToSrgb
    } else {
        ColorPolicy::PreserveLinearData
    }
}

fn source_input(
    path: &Path,
    ownership: SourceOwnership,
    inspected: &InspectedImage,
) -> SourceInput {
    SourceInput {
        id: SourceId::new(),
        ownership,
        external_path: (ownership == SourceOwnership::VerifiedExternalReference)
            .then(|| path.to_path_buf()),
        origin_path: path.to_path_buf(),
        sha256: inspected.info.sha256.clone(),
        width: inspected.info.width,
        height: inspected.info.height,
        format: inspected.info.format.clone(),
        color_type: inspected.info.color_type.clone(),
        has_alpha: inspected.info.has_alpha,
        exif_orientation: inspected.info.exif_orientation,
        has_embedded_icc_profile: inspected.info.has_embedded_icc_profile,
        encoded_bytes: inspected.info.encoded_bytes,
        owned_bytes: (ownership == SourceOwnership::OwnedCopy)
            .then(|| inspected.source_bytes.clone()),
    }
}

fn source_snapshot(source: &StoredSource, inspected: InspectedImage) -> SourceSnapshot {
    let display_name = source.input.origin_path.file_name().map_or_else(
        || format!("Owned {}", channel_label(source.channel)),
        |name| name.to_string_lossy().into(),
    );
    let thumbnail_mipmaps = inspected
        .thumbnail_mipmaps
        .into_iter()
        .map(|mipmap| ThumbnailMipmapSnapshot {
            max_edge: mipmap.max_edge,
            data_url: format!("data:image/png;base64,{}", STANDARD.encode(mipmap.png)),
        })
        .collect();
    SourceSnapshot {
        id: source.input.id.to_string(),
        channel: source.channel,
        ownership: source.input.ownership,
        display_name,
        source_path: source.input.origin_path.display().to_string(),
        width: inspected.info.width,
        height: inspected.info.height,
        format: inspected.info.format,
        color_type: inspected.info.color_type,
        has_alpha: inspected.info.has_alpha,
        exif_orientation: inspected.info.exif_orientation,
        has_embedded_icc_profile: inspected.info.has_embedded_icc_profile,
        icc_converted_to_srgb: inspected.info.icc_converted_to_srgb,
        encoded_bytes: inspected.info.encoded_bytes,
        thumbnail_data_url: format!(
            "data:image/png;base64,{}",
            STANDARD.encode(inspected.thumbnail_png)
        ),
        thumbnail_mipmaps,
    }
}

const fn channel_label(channel: SourceChannel) -> &'static str {
    match channel {
        SourceChannel::BaseColor => "Base Color",
        SourceChannel::Normal => "Normal",
        SourceChannel::Height => "Height",
        SourceChannel::Roughness => "Roughness",
        SourceChannel::Metallic => "Metallic",
        SourceChannel::AmbientOcclusion => "AO",
        SourceChannel::Specular => "Specular",
        SourceChannel::Opacity => "Opacity",
        SourceChannel::EdgeMask => "Edge Mask",
        SourceChannel::MaterialId => "Material ID",
    }
}

fn remember_open_project(session: &ProjectSession) -> Result<(), UserFacingError> {
    let summary = session
        .store
        .as_ref()
        .ok_or_else(no_open_project)?
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
    if let Err(error) = remember_open_project(session) {
        tracing::warn!(
            code = ?error.code,
            message = %error.message,
            "recent project list update failed"
        );
    }
}

fn cleanup_stale_baselines(recovery_dir: &Path, current: &Path) {
    let Some(prefix) = current
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.split(".baseline.").next())
        .map(|value| format!("{value}.baseline."))
    else {
        return;
    };
    let Ok(entries) = fs::read_dir(recovery_dir) else {
        return;
    };
    for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
        if path != current
            && path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(&prefix))
        {
            let _ = fs::remove_file(path);
        }
    }
}

fn read_recent_projects(app_data: &Path) -> Result<Vec<RecentProject>, UserFacingError> {
    let path = app_data.join("recent-projects.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = fs::read(&path).map_err(recent_error)?;
    let mut projects: Vec<RecentProject> = serde_json::from_slice(&bytes).map_err(|error| {
        user_error(
            ErrorCode::ProjectInvalid,
            "Recent Projects could not be read.",
            "The list can be rebuilt by opening projects directly.",
            Some(error.to_string()),
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
    let bytes = serde_json::to_vec_pretty(projects).map_err(|error| {
        user_error(
            ErrorCode::Internal,
            "Recent Projects could not be updated.",
            "Open the project directly; its data is unaffected.",
            Some(error.to_string()),
        )
    })?;
    fs::write(&temporary, bytes).map_err(recent_error)?;
    if path.exists() {
        fs::remove_file(&path).map_err(recent_error)?;
    }
    fs::rename(temporary, path).map_err(recent_error)
}

fn scan_recovery_candidates(
    recovery_dir: &Path,
) -> Result<Vec<RecoveryCandidate>, UserFacingError> {
    let mut candidates = Vec::new();
    let entries = fs::read_dir(recovery_dir).map_err(recovery_error)?;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let name = path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        if !name.ends_with(".hottrimmer-recovery") || name.contains(".baseline.") {
            continue;
        }
        let Ok(summary) = ProjectStore::inspect(&path) else {
            continue;
        };
        let modified_unix = entry
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .and_then(|duration| i64::try_from(duration.as_secs()).ok())
            .unwrap_or(0);
        candidates.push(RecoveryCandidate {
            project_id: summary.id.to_string(),
            project_name: summary.name,
            path: path.display().to_string(),
            modified_unix,
            source_count: summary.sources.len(),
        });
    }
    candidates.sort_by_key(|candidate| Reverse(candidate.modified_unix));
    Ok(candidates)
}

fn publish_recovery_copy(source: &Path, destination: &Path) -> Result<(), UserFacingError> {
    ProjectStore::inspect(source).map_err(store_error)?;
    if destination.exists() {
        return Err(user_error(
            ErrorCode::InvalidInput,
            "A project already exists at the recovery destination.",
            "Choose a new filename. Recovery never overwrites an existing project.",
            None,
        ));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(recovery_error)?;
    }
    let temporary = destination.with_extension(format!("recovering-{}.tmp", Uuid::new_v4()));
    fs::copy(source, &temporary).map_err(recovery_error)?;
    fs::OpenOptions::new()
        .write(true)
        .open(&temporary)
        .and_then(|file| file.sync_all())
        .map_err(recovery_error)?;
    ProjectStore::inspect(&temporary).map_err(store_error)?;
    fs::rename(temporary, destination).map_err(recovery_error)?;
    Ok(())
}

fn validate_protocol(received: u16) -> Result<(), UserFacingError> {
    FoundationStatusRequest {
        protocol_version: received,
    }
    .validate()
    .map(|_| ())
    .map_err(UserFacingError::from)
}

fn validate_ipc_path(value: &str) -> Result<PathBuf, UserFacingError> {
    if value.is_empty() || value.encode_utf16().count() > MAX_IPC_PATH_UTF16 {
        return Err(user_error(
            ErrorCode::InvalidInput,
            "The selected path is empty or exceeds the Windows path limit.",
            "Choose a shorter local project or image path and retry.",
            None,
        ));
    }
    Ok(PathBuf::from(value))
}

fn emit_import_progress(app: &AppHandle, stage: &'static str, fraction: f32) {
    let _ = app.emit("import-progress", ImportProgress { stage, fraction });
}

fn emit_patch_preview_progress(
    app: &AppHandle,
    patch_id: PatchId,
    stage: &'static str,
    fraction: f64,
) {
    let _ = app.emit(
        "patch-preview-progress",
        PatchPreviewProgress {
            patch_id,
            stage,
            fraction,
        },
    );
}

#[allow(clippy::needless_pass_by_value)]
fn store_error(error: StoreError) -> UserFacingError {
    let (code, message, recovery) = match error {
        StoreError::Locked => (
            ErrorCode::ProjectLocked,
            "This project is already open.",
            "Close it in the other Hot Trimmer window, then retry.",
        ),
        StoreError::AlreadyExists => (
            ErrorCode::InvalidInput,
            "A project already exists at that location.",
            "Choose a different name or use Open Project.",
        ),
        StoreError::NewerSchema { .. } => (
            ErrorCode::ProjectInvalid,
            "This project was created by a newer version of Hot Trimmer.",
            "Open it with the newer application version. The file was not changed.",
        ),
        StoreError::BaseColorRequired | StoreError::RegistrationMismatch { .. } => (
            ErrorCode::SourceRegistrationFailed,
            "The PBR source cannot be registered to this project.",
            "Import Base Color first and use maps with exactly matching dimensions.",
        ),
        StoreError::BaseColorInUse => (
            ErrorCode::SourceRegistrationFailed,
            "Base Color still anchors other material inputs.",
            "Clear the companion slots first, then remove Base Color.",
        ),
        StoreError::SourceInUseByPatches => (
            ErrorCode::SourceRegistrationFailed,
            "This source is still used by authored patches.",
            "Delete those patches or replace the source while keeping its slot.",
        ),
        StoreError::PatchCommand(_) => (
            ErrorCode::InvalidInput,
            "The patch edit could not be applied.",
            "Review the selected patch and retry the edit.",
        ),
        StoreError::PatchSerialization(_) => (
            ErrorCode::ProjectInvalid,
            "The patch edit could not be stored safely.",
            "Keep the project open and retry Save. Use recovery if the error continues.",
        ),
        StoreError::Integrity(_) | StoreError::InvalidData(_) | StoreError::InvalidId(_) => (
            ErrorCode::ProjectInvalid,
            "The project is incomplete or damaged.",
            "Open a recovery snapshot or restore a known-good copy.",
        ),
        StoreError::Io(_) | StoreError::Database(_) => (
            ErrorCode::ProjectInvalid,
            "The project could not be read or written safely.",
            "Check the location and available disk space, then retry.",
        ),
    };
    user_error(code, message, recovery, Some(error.to_string()))
}

#[allow(clippy::needless_pass_by_value)]
fn recovery_refresh_warning(error: StoreError) -> UserFacingError {
    user_error(
        ErrorCode::RecoveryFailed,
        "Recovery snapshot could not be refreshed.",
        "The input is committed and Save-pending. Save explicitly after checking disk space and permissions.",
        Some(error.to_string()),
    )
}

#[allow(clippy::needless_pass_by_value)]
fn image_error(error: ImageIoError) -> UserFacingError {
    if matches!(error, ImageIoError::Cancelled) {
        return cancelled();
    }
    user_error(
        ErrorCode::ImageImportFailed,
        "The source image could not be imported safely.",
        "Choose a valid PNG, JPEG, or TIFF within the documented limits.",
        Some(error.to_string()),
    )
}

#[allow(clippy::needless_pass_by_value)] // Used as an owned map_err callback.
fn render_error(error: RenderError) -> UserFacingError {
    if matches!(error, RenderError::Cancelled) {
        return user_error(
            ErrorCode::OperationCancelled,
            "Patch preview was cancelled.",
            "Select the patch again to regenerate its preview.",
            None,
        );
    }
    user_error(
        ErrorCode::PatchGeometryInvalid,
        "The patch preview could not be generated safely.",
        "Adjust the corners or lower the rectified output scale, then retry.",
        Some(error.to_string()),
    )
}

fn cancelled() -> UserFacingError {
    user_error(
        ErrorCode::OperationCancelled,
        "Image import was cancelled.",
        "Choose the source again when you are ready. The project was not changed.",
        None,
    )
}

#[allow(clippy::needless_pass_by_value)] // Used as an owned map_err callback.
fn recent_error(error: std::io::Error) -> UserFacingError {
    user_error(
        ErrorCode::Internal,
        "Recent Projects could not be updated.",
        "Open projects directly; authoritative project data is unaffected.",
        Some(error.to_string()),
    )
}

#[allow(clippy::needless_pass_by_value)] // Used as an owned map_err callback.
fn recovery_error(error: std::io::Error) -> UserFacingError {
    user_error(
        ErrorCode::RecoveryFailed,
        "Recovery data could not be read or published safely.",
        "Keep the original project unchanged and retry to a writable location.",
        Some(error.to_string()),
    )
}

fn dirty_project() -> UserFacingError {
    user_error(
        ErrorCode::DirtyProject,
        "The open project has unsaved changes.",
        "Save, discard, or cancel before opening another project.",
        None,
    )
}

fn no_open_project() -> UserFacingError {
    user_error(
        ErrorCode::NoOpenProject,
        "No project is open.",
        "Create or open a project before continuing.",
        None,
    )
}

fn session_poisoned() -> UserFacingError {
    user_error(
        ErrorCode::Internal,
        "The open project session is unavailable.",
        "Restart Hot Trimmer. Your committed project data remains on disk.",
        None,
    )
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or(0)
}

fn user_error(
    code: ErrorCode,
    message: &str,
    recovery: &str,
    detail: Option<String>,
) -> UserFacingError {
    UserFacingError {
        code,
        message: message.into(),
        recovery: recovery.into(),
        detail,
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use hot_trimmer_domain::{ErrorCode, FoundationStatusRequest, IPC_PROTOCOL_VERSION, SourceId};
    use hot_trimmer_project_store::{ProjectStore, SourceChannel, SourceInput, SourceOwnership};
    use serde_json::{Value, json};

    use super::{
        CloseProjectRequest, CreateProjectRequest, FoundationStatus, ImportSourceRequest,
        MAX_IPC_PATH_UTF16, NativeDirectories, PatchCommandRequest, PatchPreviewRequest,
        PolygonAssistRequest, ProjectSession, ProjectSnapshot, ProjectWarning,
        RecoverProjectRequest, SourceSnapshot, ThumbnailMipmapSnapshot, validate_ipc_path,
    };

    #[test]
    fn rust_response_matches_the_cross_language_contract_fixture() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/contracts/foundation-status.json"
        ))
        .expect("valid contract fixture");
        let request = FoundationStatusRequest {
            protocol_version: IPC_PROTOCOL_VERSION,
        };
        assert_eq!(
            serde_json::to_value(request).expect("serializable request"),
            fixture["request"]
        );
        let response = FoundationStatus {
            protocol_version: IPC_PROTOCOL_VERSION,
            app_version: "0.1.0",
            platform: "windows",
            directories: NativeDirectories {
                app_data: PathBuf::from("<app-data>").display().to_string(),
                cache: PathBuf::from("<cache>").display().to_string(),
                logs: PathBuf::from("<logs>").display().to_string(),
                recovery: PathBuf::from("<recovery>").display().to_string(),
            },
            capabilities: [
                "native_paths",
                "typed_ipc",
                "structured_diagnostics",
                "native_dialog",
            ],
        };
        assert_eq!(
            serde_json::to_value(response).expect("serializable response"),
            fixture["response"]
        );
        assert_eq!(fixture["request"], json!({ "protocolVersion": 1 }));
    }

    #[test]
    fn rust_lifecycle_matches_the_phase_one_contract_fixture() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/contracts/phase-1-lifecycle.json"
        ))
        .expect("valid lifecycle fixture");
        let create: CreateProjectRequest =
            serde_json::from_value(fixture["createRequest"].clone()).expect("create request");
        let import: ImportSourceRequest =
            serde_json::from_value(fixture["importRequest"].clone()).expect("import request");
        let _: CloseProjectRequest =
            serde_json::from_value(fixture["closeRequest"].clone()).expect("close request");
        let _: RecoverProjectRequest =
            serde_json::from_value(fixture["recoverRequest"].clone()).expect("recover request");
        assert_eq!(create.protocol_version, IPC_PROTOCOL_VERSION);
        assert_eq!(create.name, "Brick");
        assert_eq!(import.channel, SourceChannel::Specular);

        let snapshot = ProjectSnapshot {
            id: "00000000-0000-4000-8000-000000000001".into(),
            name: "Brick".into(),
            path: "<project>".into(),
            schema_version: 5,
            dirty: true,
            stale_lock_recovered: false,
            sources: vec![SourceSnapshot {
                id: "00000000-0000-4000-8000-000000000002".into(),
                channel: SourceChannel::Specular,
                ownership: SourceOwnership::OwnedCopy,
                display_name: "Owned Specular".into(),
                source_path: String::new(),
                width: 2048,
                height: 2048,
                format: "PNG".into(),
                color_type: "L8".into(),
                has_alpha: false,
                exif_orientation: 1,
                has_embedded_icc_profile: false,
                icc_converted_to_srgb: false,
                encoded_bytes: 4096,
                thumbnail_data_url: "data:image/png;base64,AA==".into(),
                thumbnail_mipmaps: vec![ThumbnailMipmapSnapshot {
                    max_edge: 320,
                    data_url: "data:image/png;base64,AA==".into(),
                }],
            }],
            patches: Vec::new(),
            can_undo_patch: false,
            can_redo_patch: false,
            warnings: vec![ProjectWarning {
                code: ErrorCode::RecoveryFailed,
                message: "Recovery snapshot could not be refreshed.".into(),
                recovery: "Save explicitly to retry recovery publication.".into(),
            }],
        };
        assert_eq!(
            serde_json::to_value(snapshot).expect("serializable snapshot"),
            fixture["projectSnapshot"]
        );
    }

    #[test]
    fn rust_requests_match_the_phase_two_patch_contract_fixture() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/contracts/phase-2-patch-authoring.json"
        ))
        .expect("valid patch contract fixture");
        let command: PatchCommandRequest =
            serde_json::from_value(fixture["patchCommandRequest"].clone())
                .expect("patch command request");
        let assist: PolygonAssistRequest =
            serde_json::from_value(fixture["polygonAssistRequest"].clone())
                .expect("polygon assist request");
        let preview: PatchPreviewRequest =
            serde_json::from_value(fixture["previewRequest"].clone())
                .expect("patch preview request");
        assert_eq!(command.protocol_version, IPC_PROTOCOL_VERSION);
        assert_eq!(command.coalescing_group, Some(42));
        assert!(matches!(
            command.command,
            hot_trimmer_domain::PatchCommand::Create { .. }
        ));
        assert_eq!(assist.points.len(), 6);
        assert!(assist.retain_mask);
        assert_eq!(preview.max_edge, 768);
    }

    #[test]
    fn oversized_ipc_paths_are_rejected_before_file_access() {
        let oversized = "a".repeat(MAX_IPC_PATH_UTF16 + 1);
        let error = validate_ipc_path(&oversized).expect_err("reject oversized path");
        assert_eq!(error.code, ErrorCode::InvalidInput);
        assert!(validate_ipc_path("C:/project.hottrimmer").is_ok());
    }

    #[test]
    fn recovery_refresh_failure_keeps_committed_source_dirty_and_returns_warning() {
        let root =
            std::env::temp_dir().join(format!("hot-trimmer-warning-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("fixture directory");
        let project_path = root.join("warning.hottrimmer");
        let blocked_recovery = root.join("blocked-recovery");
        fs::write(&blocked_recovery, b"not a directory").expect("block recovery directory");
        let store = ProjectStore::create(&project_path, "Warning").expect("project");
        let mut session = ProjectSession {
            store: Some(store),
            dirty: false,
            baseline: None,
            recovery_dir: blocked_recovery,
            app_data_dir: root.clone(),
        };
        let source = SourceInput {
            id: SourceId::new(),
            ownership: SourceOwnership::OwnedCopy,
            external_path: None,
            origin_path: root.join("warning.png"),
            sha256: "a".repeat(64),
            width: 4,
            height: 4,
            format: "PNG".into(),
            color_type: "Rgba8".into(),
            has_alpha: true,
            exif_orientation: 1,
            has_embedded_icc_profile: false,
            encoded_bytes: 4,
            owned_bytes: Some(vec![0; 4]),
        };
        let warning = session
            .replace_source_and_refresh_recovery(SourceChannel::BaseColor, &source)
            .expect("authoritative import succeeds")
            .expect("recovery warning");
        assert!(session.dirty);
        assert_eq!(warning.code, ErrorCode::RecoveryFailed);
        assert_eq!(
            session
                .store
                .as_ref()
                .expect("open store")
                .summary()
                .expect("summary")
                .sources
                .len(),
            1
        );
        drop(session);
        fs::remove_dir_all(root).expect("remove fixture");
    }
}
