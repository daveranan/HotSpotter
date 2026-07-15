use std::{
    cmp::Reverse,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::UNIX_EPOCH,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use hot_trimmer_domain::{
    ErrorCode, FoundationStatusRequest, IPC_PROTOCOL_VERSION, SourceId, UserFacingError,
};
use hot_trimmer_image_io::{
    CancellationToken, ColorPolicy, DecodeLimits, ImageIoError, InspectedImage,
    inspect_bytes_with_policy, inspect_path_cancellable, inspect_path_with_policy,
};
use hot_trimmer_project_store::{
    ProjectStore, SourceChannel, SourceInput, SourceOwnership, StoreError, StoredSource,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::paths::AppPaths;

const MAX_RECENT_PROJECTS: usize = 10;

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
pub struct ImportSourceRequest {
    protocol_version: u16,
    path: String,
    ownership: SourceOwnership,
    channel: SourceChannel,
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
pub struct ProjectSnapshot {
    id: String,
    name: String,
    path: String,
    schema_version: u32,
    dirty: bool,
    stale_lock_recovered: bool,
    sources: Vec<SourceSnapshot>,
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
        self.store = Some(store);
        self.baseline = Some(baseline);
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
        self.baseline = Some(baseline);
        self.dirty = false;
        Ok(())
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
        self.baseline = None;
        self.dirty = false;
        Ok(())
    }
}

pub type SharedProjectSession = Arc<Mutex<ProjectSession>>;
pub type PendingProjectPath = Arc<Mutex<Option<String>>>;
pub type SharedImportJob = Arc<Mutex<Option<CancellationToken>>>;

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
    let path = PathBuf::from(request.path);
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
        remember_open_project(&guard)?;
        snapshot_session(&guard, None)
    })
    .await
}

#[tauri::command]
pub async fn open_project(
    request: ProjectPathRequest,
    session: State<'_, SharedProjectSession>,
) -> Result<ProjectSnapshot, UserFacingError> {
    validate_protocol(request.protocol_version)?;
    let path = PathBuf::from(request.path);
    let shared = Arc::clone(&session);
    run_blocking(move || {
        let mut guard = shared.lock().map_err(|_| session_poisoned())?;
        guard.ensure_replaceable()?;
        let store = ProjectStore::open(&path).map_err(store_error)?;
        guard.adopt(store)?;
        remember_open_project(&guard)?;
        snapshot_session(&guard, None)
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
    let path = PathBuf::from(request.path);
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
        let recovery_dir = guard.recovery_dir.clone();
        let store = guard.store.as_mut().ok_or_else(no_open_project)?;
        store
            .replace_source(channel, &source)
            .map_err(store_error)?;
        emit_import_progress(&app, "Writing recovery snapshot", 0.92);
        store
            .create_recovery_snapshot(&recovery_dir)
            .map_err(store_error)?;
        guard.dirty = true;
        emit_import_progress(&app, "Complete", 1.0);
        snapshot_session(&guard, Some((source_id, inspected)))
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
    let destination = PathBuf::from(request.path);
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
        remember_open_project(&guard)?;
        snapshot_session(&guard, None)
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
    let source = PathBuf::from(request.recovery_path);
    let destination = PathBuf::from(request.destination_path);
    let shared = Arc::clone(&session);
    run_blocking(move || {
        let mut guard = shared.lock().map_err(|_| session_poisoned())?;
        guard.ensure_replaceable()?;
        publish_recovery_copy(&source, &destination)?;
        let store = ProjectStore::open(&destination).map_err(store_error)?;
        guard.adopt(store)?;
        remember_open_project(&guard)?;
        snapshot_session(&guard, None)
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
    })
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
    let display_name = source
        .input
        .external_path
        .as_ref()
        .and_then(|path| path.file_name())
        .map_or_else(
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

fn emit_import_progress(app: &AppHandle, stage: &'static str, fraction: f32) {
    let _ = app.emit("import-progress", ImportProgress { stage, fraction });
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
    use std::path::PathBuf;

    use hot_trimmer_domain::{FoundationStatusRequest, IPC_PROTOCOL_VERSION};
    use serde_json::{Value, json};

    use super::{FoundationStatus, NativeDirectories};

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
}
