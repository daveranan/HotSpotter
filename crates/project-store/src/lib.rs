#![doc = "Transactional `SQLite` project persistence, migration, locking, autosave, and recovery."]

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use hot_trimmer_domain::{
    Patch, PatchCommand, PatchCommandError, PatchEditOutcome, PatchSet, ProjectId, SourceId,
};
use hot_trimmer_geometry::Quadrilateral;
use rusqlite::{Connection, DatabaseName, OpenFlags, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessesToUpdate, System};
use thiserror::Error;
use uuid::Uuid;

pub const CURRENT_SCHEMA_VERSION: u32 = 5;
pub const MAX_RECOVERY_SNAPSHOTS: usize = 5;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceChannel {
    BaseColor,
    Normal,
    Height,
    Roughness,
    Metallic,
    AmbientOcclusion,
    Specular,
    Opacity,
    EdgeMask,
    MaterialId,
}

impl SourceChannel {
    pub const ALL: [Self; 10] = [
        Self::BaseColor,
        Self::Normal,
        Self::Height,
        Self::Roughness,
        Self::Metallic,
        Self::AmbientOcclusion,
        Self::Specular,
        Self::Opacity,
        Self::EdgeMask,
        Self::MaterialId,
    ];

    #[must_use]
    pub const fn as_db_value(self) -> &'static str {
        match self {
            Self::BaseColor => "base_color",
            Self::Normal => "normal",
            Self::Height => "height",
            Self::Roughness => "roughness",
            Self::Metallic => "metallic",
            Self::AmbientOcclusion => "ambient_occlusion",
            Self::Specular => "specular",
            Self::Opacity => "opacity",
            Self::EdgeMask => "edge_mask",
            Self::MaterialId => "material_id",
        }
    }

    fn from_db_value(value: &str) -> Result<Self, StoreError> {
        Self::ALL
            .into_iter()
            .find(|channel| channel.as_db_value() == value)
            .ok_or_else(|| StoreError::InvalidData(format!("unknown source channel: {value}")))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceOwnership {
    OwnedCopy,
    VerifiedExternalReference,
}

impl SourceOwnership {
    const fn as_db_value(self) -> &'static str {
        match self {
            Self::OwnedCopy => "owned_copy",
            Self::VerifiedExternalReference => "verified_external_reference",
        }
    }

    fn from_db_value(value: &str) -> Result<Self, StoreError> {
        match value {
            "owned_copy" => Ok(Self::OwnedCopy),
            "verified_external_reference" => Ok(Self::VerifiedExternalReference),
            _ => Err(StoreError::InvalidData(format!(
                "unknown source ownership value: {value}"
            ))),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceInput {
    pub id: SourceId,
    pub ownership: SourceOwnership,
    pub external_path: Option<PathBuf>,
    pub origin_path: PathBuf,
    pub sha256: String,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub color_type: String,
    pub has_alpha: bool,
    pub exif_orientation: u16,
    pub has_embedded_icc_profile: bool,
    pub encoded_bytes: u64,
    pub owned_bytes: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredSource {
    pub channel: SourceChannel,
    pub input: SourceInput,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectSummary {
    pub id: ProjectId,
    pub name: String,
    pub path: PathBuf,
    pub sources: Vec<StoredSource>,
    pub patches: Vec<Patch>,
    pub stale_lock_recovered: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutosaveEntry {
    pub sequence: u64,
    pub occurred_unix: i64,
    pub operation: String,
    pub payload_json: String,
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("the project file could not be accessed: {0}")]
    Io(#[from] std::io::Error),
    #[error("the project database operation failed: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("the project is already open in another Hot Trimmer process")]
    Locked,
    #[error("a project already exists at the selected location")]
    AlreadyExists,
    #[error(
        "the project schema version {found} is newer than this application supports ({supported})"
    )]
    NewerSchema { found: u32, supported: u32 },
    #[error("the project is missing required data: {0}")]
    InvalidData(String),
    #[error("the project failed its SQLite integrity check: {0}")]
    Integrity(String),
    #[error("a stored stable identifier is invalid: {0}")]
    InvalidId(String),
    #[error("import Base Color before assigning related PBR sources")]
    BaseColorRequired,
    #[error("remove companion material inputs before removing Base Color")]
    BaseColorInUse,
    #[error(
        "the {channel} source is {actual_width}x{actual_height}; registered sources must be {expected_width}x{expected_height}"
    )]
    RegistrationMismatch {
        channel: &'static str,
        expected_width: u32,
        expected_height: u32,
        actual_width: u32,
        actual_height: u32,
    },
    #[error("the source is used by one or more patches")]
    SourceInUseByPatches,
    #[error("the patch command is invalid: {0}")]
    PatchCommand(#[from] PatchCommandError),
    #[error("patch data could not be serialized: {0}")]
    PatchSerialization(#[from] serde_json::Error),
}

pub struct ProjectStore {
    connection: Connection,
    project_path: PathBuf,
    _lock: ProjectLock,
    stale_lock_recovered: bool,
    patch_set: PatchSet,
}

impl ProjectStore {
    /// Creates and locks a new project at a previously unused path.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the location is unavailable, already exists, is locked, or cannot be
    /// initialized transactionally.
    pub fn create(path: &Path, name: &str) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let (lock, stale_lock_recovered) = ProjectLock::acquire(path)?;
        if path.exists() {
            return Err(StoreError::AlreadyExists);
        }
        let mut connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        )?;
        configure(&connection)?;
        migrate(&mut connection)?;

        let project_id = ProjectId::new();
        let now = unix_timestamp()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO project (id, name, created_unix, modified_unix) VALUES (?1, ?2, ?3, ?3)",
            params![project_id.to_string(), name, now],
        )?;
        transaction.commit()?;
        checkpoint(&connection)?;

        Ok(Self {
            connection,
            project_path: path.to_path_buf(),
            _lock: lock,
            stale_lock_recovered,
            patch_set: PatchSet::default(),
        })
    }

    /// Opens, migrates, integrity-checks, and locks an existing project.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the project is unavailable, locked, corrupt, or newer than this schema.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let (lock, stale_lock_recovered) = ProjectLock::acquire(path)?;
        let mut connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        configure(&connection)?;
        migrate(&mut connection)?;
        verify_integrity(&connection)?;

        let patch_set = PatchSet::new(load_patches(&connection)?)?;
        let store = Self {
            connection,
            project_path: path.to_path_buf(),
            _lock: lock,
            stale_lock_recovered,
            patch_set,
        };
        store.summary()?;
        Ok(store)
    }

    /// Reads a project or recovery snapshot without taking ownership or migrating it.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the database is unavailable, invalid, or uses an unsupported schema.
    pub fn inspect(path: &Path) -> Result<ProjectSummary, StoreError> {
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > CURRENT_SCHEMA_VERSION {
            return Err(StoreError::NewerSchema {
                found: version,
                supported: CURRENT_SCHEMA_VERSION,
            });
        }
        if version < 1 {
            return Err(StoreError::InvalidData("project schema version".into()));
        }
        verify_integrity(&connection)?;
        summary_from_connection(&connection, path, false)
    }

    /// Reads authoritative project metadata and registered sources.
    ///
    /// # Errors
    ///
    /// Returns a typed error when required records or stable identifiers are invalid.
    pub fn summary(&self) -> Result<ProjectSummary, StoreError> {
        summary_from_connection(
            &self.connection,
            &self.project_path,
            self.stale_lock_recovered,
        )
    }

    /// Replaces one explicitly assigned source and journals the command in one transaction.
    ///
    /// # Errors
    ///
    /// Returns a typed error when ownership or registration is inconsistent or the transaction cannot commit.
    pub fn replace_source(
        &mut self,
        channel: SourceChannel,
        source: &SourceInput,
    ) -> Result<(), StoreError> {
        validate_source_ownership(source)?;
        self.validate_registration(channel, source.width, source.height)?;
        let previous_source_id = self
            .connection
            .query_row(
                "SELECT id FROM sources WHERE channel = ?1",
                [channel.as_db_value()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|value| {
                value
                    .parse::<SourceId>()
                    .map_err(|_| StoreError::InvalidId(value))
            })
            .transpose()?;
        let mut next_patch_set = self.patch_set.clone();
        if let Some(previous_source_id) = previous_source_id {
            next_patch_set.execute(
                PatchCommand::ReassignSource {
                    from_source_id: previous_source_id,
                    to_source_id: source.id,
                },
                None,
            )?;
        }
        let now = unix_timestamp()?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM sources WHERE channel = ?1",
            [channel.as_db_value()],
        )?;
        persist_patch_rows(&transaction, next_patch_set.patches())?;
        transaction.execute(
            "INSERT INTO sources (
                id, channel, ownership, external_path, sha256, width, height, format, color_type,
                has_alpha, exif_orientation, has_icc_profile, encoded_bytes, owned_bytes, origin_path
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                source.id.to_string(),
                channel.as_db_value(),
                source.ownership.as_db_value(),
                source
                    .external_path
                    .as_ref()
                    .map(|path| path.to_string_lossy()),
                source.sha256,
                i64::from(source.width),
                i64::from(source.height),
                source.format,
                source.color_type,
                source.has_alpha,
                i64::from(source.exif_orientation),
                source.has_embedded_icc_profile,
                i64::try_from(source.encoded_bytes).map_err(|_| {
                    StoreError::InvalidData(
                        "encoded byte count exceeds SQLite integer range".into(),
                    )
                })?,
                source.owned_bytes,
                source.origin_path.to_string_lossy(),
            ],
        )?;
        let payload = format!(
            "{{\"channel\":\"{}\",\"sourceId\":\"{}\",\"sha256\":\"{}\"}}",
            channel.as_db_value(),
            source.id,
            source.sha256
        );
        transaction.execute(
            "INSERT INTO autosave_journal (occurred_unix, operation, payload_json)
             VALUES (?1, 'replace_source', ?2)",
            params![now, payload],
        )?;
        transaction.execute("UPDATE project SET modified_unix = ?1", [now])?;
        transaction.commit()?;
        self.patch_set = next_patch_set;
        checkpoint(&self.connection)
    }

    /// Removes one material-input slot and journals the command transactionally.
    ///
    /// # Errors
    ///
    /// Returns a typed error when Base Color still anchors companion inputs or persistence fails.
    pub fn remove_source(&mut self, channel: SourceChannel) -> Result<(), StoreError> {
        if channel == SourceChannel::BaseColor {
            let companions: u32 = self.connection.query_row(
                "SELECT COUNT(*) FROM sources WHERE channel <> 'base_color'",
                [],
                |row| row.get(0),
            )?;
            if companions > 0 {
                return Err(StoreError::BaseColorInUse);
            }
        }
        let source_id = self
            .connection
            .query_row(
                "SELECT id FROM sources WHERE channel = ?1",
                [channel.as_db_value()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|value| {
                value
                    .parse::<SourceId>()
                    .map_err(|_| StoreError::InvalidId(value))
            })
            .transpose()?;
        if source_id.is_some_and(|id| {
            self.patch_set
                .patches()
                .iter()
                .any(|patch| patch.source_id == id)
        }) {
            return Err(StoreError::SourceInUseByPatches);
        }
        let now = unix_timestamp()?;
        let transaction = self.connection.transaction()?;
        let removed = transaction.execute(
            "DELETE FROM sources WHERE channel = ?1",
            [channel.as_db_value()],
        )?;
        if removed == 0 {
            return Err(StoreError::InvalidData(format!(
                "empty material input slot: {}",
                channel.as_db_value()
            )));
        }
        let payload = format!("{{\"channel\":\"{}\"}}", channel.as_db_value());
        transaction.execute(
            "INSERT INTO autosave_journal (occurred_unix, operation, payload_json)
             VALUES (?1, 'remove_source', ?2)",
            params![now, payload],
        )?;
        transaction.execute("UPDATE project SET modified_unix = ?1", [now])?;
        transaction.commit()?;
        checkpoint(&self.connection)
    }

    /// Renames the project and journals the edit transactionally.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the name is invalid or persistence cannot commit.
    pub fn rename_project(&mut self, name: &str) -> Result<(), StoreError> {
        let name = name.trim();
        if name.is_empty() || name.len() > 255 {
            return Err(StoreError::InvalidData(
                "project name must be 1 to 255 characters".into(),
            ));
        }
        let now = unix_timestamp()?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE project SET name = ?1, modified_unix = ?2",
            params![name, now],
        )?;
        transaction.execute(
            "INSERT INTO autosave_journal (occurred_unix, operation, payload_json)
             VALUES (?1, 'rename_project', json_object('name', ?2))",
            params![now, name],
        )?;
        transaction.commit()?;
        checkpoint(&self.connection)
    }

    #[must_use]
    pub fn patches(&self) -> &[Patch] {
        self.patch_set.patches()
    }

    #[must_use]
    pub fn can_undo_patch_command(&self) -> bool {
        self.patch_set.can_undo()
    }

    #[must_use]
    pub fn can_redo_patch_command(&self) -> bool {
        self.patch_set.can_redo()
    }

    /// Applies and durably journals one validated patch command in the same transaction as patch state.
    ///
    /// # Errors
    ///
    /// Returns a typed command, serialization, or database failure without changing in-memory state.
    pub fn execute_patch_command(
        &mut self,
        command: &PatchCommand,
        coalescing_group: Option<u64>,
    ) -> Result<PatchEditOutcome, StoreError> {
        let mut next = self.patch_set.clone();
        let outcome = next.execute(command.clone(), coalescing_group)?;
        let payload = serde_json::to_string(command)?;
        persist_patch_state(
            &mut self.connection,
            next.patches(),
            "patch_command",
            &payload,
        )?;
        self.patch_set = next;
        checkpoint(&self.connection)?;
        Ok(outcome)
    }

    /// Undoes the latest coalesced patch command and persists the restored state.
    ///
    /// # Errors
    ///
    /// Returns a typed failure without changing state when history is empty or persistence fails.
    pub fn undo_patch_command(&mut self) -> Result<PatchEditOutcome, StoreError> {
        let mut next = self.patch_set.clone();
        let outcome = next.undo()?;
        let payload = serde_json::to_string(&outcome.invalidated_patch_ids)?;
        persist_patch_state(&mut self.connection, next.patches(), "patch_undo", &payload)?;
        self.patch_set = next;
        checkpoint(&self.connection)?;
        Ok(outcome)
    }

    /// Redoes the next patch command and persists the restored state.
    ///
    /// # Errors
    ///
    /// Returns a typed failure without changing state when redo history is empty or persistence fails.
    pub fn redo_patch_command(&mut self) -> Result<PatchEditOutcome, StoreError> {
        let mut next = self.patch_set.clone();
        let outcome = next.redo()?;
        let payload = serde_json::to_string(&outcome.invalidated_patch_ids)?;
        persist_patch_state(&mut self.connection, next.patches(), "patch_redo", &payload)?;
        self.patch_set = next;
        checkpoint(&self.connection)?;
        Ok(outcome)
    }

    /// Returns the durable autosave command journal in commit order.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the journal cannot be read or contains invalid numeric data.
    pub fn autosave_journal(&self) -> Result<Vec<AutosaveEntry>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT sequence, occurred_unix, operation, payload_json
             FROM autosave_journal ORDER BY sequence",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(AutosaveEntry {
                sequence: row.get(0)?,
                occurred_unix: row.get(1)?,
                operation: row.get(2)?,
                payload_json: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Flushes committed WAL pages into the authoritative project database.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the checkpoint cannot complete.
    pub fn save(&self) -> Result<(), StoreError> {
        checkpoint(&self.connection)
    }

    /// Publishes an integrity-checked standalone copy at a previously unused generation path.
    ///
    /// # Errors
    ///
    /// Returns a typed error when backup, validation, synchronization, or publication fails.
    pub fn backup_atomic(&self, destination: &Path) -> Result<(), StoreError> {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = sibling_temporary_path(destination);
        if temporary.exists() {
            fs::remove_file(&temporary)?;
        }
        self.connection
            .backup(DatabaseName::Main, &temporary, None)?;
        validate_standalone_database(&temporary)?;
        sync_file(&temporary)?;
        pause_at_backup_publication_failpoint()?;
        atomic_publish_new(&temporary, destination)
    }

    /// Restores a validated snapshot into the open database using `SQLite`'s online backup API.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the snapshot is invalid or restore/checkpoint cannot complete.
    pub fn restore_from(&mut self, source: &Path) -> Result<(), StoreError> {
        validate_standalone_database(source)?;
        self.connection.restore(
            DatabaseName::Main,
            source,
            None::<fn(rusqlite::backup::Progress)>,
        )?;
        checkpoint(&self.connection)?;
        verify_integrity(&self.connection)?;
        self.patch_set = PatchSet::new(load_patches(&self.connection)?)?;
        Ok(())
    }

    /// Creates and locks a verified project copy at a new path.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the destination exists, is locked, or cannot be published and reopened.
    pub fn save_as(&self, destination: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let (lock, stale_lock_recovered) = ProjectLock::acquire(destination)?;
        if destination.exists() {
            return Err(StoreError::AlreadyExists);
        }
        let temporary = sibling_temporary_path(destination);
        self.connection
            .backup(DatabaseName::Main, &temporary, None)?;
        validate_standalone_database(&temporary)?;
        sync_file(&temporary)?;
        fs::rename(&temporary, destination)?;
        let connection =
            Connection::open_with_flags(destination, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        configure(&connection)?;
        verify_integrity(&connection)?;
        Ok(Self {
            connection,
            project_path: destination.to_path_buf(),
            _lock: lock,
            stale_lock_recovered,
            patch_set: self.patch_set.clone(),
        })
    }

    /// Creates a rotating recovery snapshot and returns its path.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the recovery directory, snapshot, or rotation cannot be updated safely.
    pub fn create_recovery_snapshot(&self, recovery_dir: &Path) -> Result<PathBuf, StoreError> {
        fs::create_dir_all(recovery_dir)?;
        let project_id = self.summary()?.id;
        let timestamp = unix_timestamp_millis()?;
        let path = recovery_dir.join(format!(
            "{project_id}.{timestamp}.{}.hottrimmer-recovery",
            Uuid::new_v4()
        ));
        self.backup_atomic(&path)?;
        rotate_recovery_snapshots(recovery_dir, project_id)?;
        Ok(path)
    }

    /// Returns a unique immutable baseline path used to implement discard.
    #[must_use]
    pub fn baseline_path(&self, recovery_dir: &Path) -> PathBuf {
        let id = self
            .summary()
            .map_or_else(|_| "unknown".to_owned(), |summary| summary.id.to_string());
        recovery_dir.join(format!(
            "{id}.baseline.{}.hottrimmer-recovery",
            Uuid::new_v4()
        ))
    }

    fn validate_registration(
        &self,
        channel: SourceChannel,
        width: u32,
        height: u32,
    ) -> Result<(), StoreError> {
        let base_dimensions = self
            .connection
            .query_row(
                "SELECT width, height FROM sources WHERE channel = 'base_color'",
                [],
                |row| Ok((row.get::<_, u32>(0)?, row.get::<_, u32>(1)?)),
            )
            .optional()?;
        if channel == SourceChannel::BaseColor {
            let mismatch: Option<String> = self
                .connection
                .query_row(
                    "SELECT channel FROM sources
                     WHERE channel <> 'base_color' AND (width <> ?1 OR height <> ?2) LIMIT 1",
                    params![width, height],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(mismatch) = mismatch {
                let (expected_width, expected_height) = base_dimensions.unwrap_or((width, height));
                return Err(StoreError::RegistrationMismatch {
                    channel: SourceChannel::from_db_value(&mismatch)?.as_db_value(),
                    expected_width,
                    expected_height,
                    actual_width: width,
                    actual_height: height,
                });
            }
            return Ok(());
        }
        let Some((expected_width, expected_height)) = base_dimensions else {
            return Err(StoreError::BaseColorRequired);
        };
        if (width, height) == (expected_width, expected_height) {
            Ok(())
        } else {
            Err(StoreError::RegistrationMismatch {
                channel: channel.as_db_value(),
                expected_width,
                expected_height,
                actual_width: width,
                actual_height: height,
            })
        }
    }
}

fn summary_from_connection(
    connection: &Connection,
    path: &Path,
    stale_lock_recovered: bool,
) -> Result<ProjectSummary, StoreError> {
    let (id_text, name): (String, String) = connection
        .query_row("SELECT id, name FROM project LIMIT 1", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .optional()?
        .ok_or_else(|| StoreError::InvalidData("project record".into()))?;
    let id = id_text
        .parse::<ProjectId>()
        .map_err(|_| StoreError::InvalidId(id_text.clone()))?;
    Ok(ProjectSummary {
        id,
        name,
        path: path.to_path_buf(),
        sources: load_sources(connection)?,
        patches: load_patches(connection)?,
        stale_lock_recovered,
    })
}

fn load_patches(connection: &Connection) -> Result<Vec<Patch>, StoreError> {
    let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version < 5 {
        return Ok(Vec::new());
    }
    let mut statement =
        connection.prepare("SELECT id, source_id, patch_json FROM patches ORDER BY ordinal")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut patches = Vec::new();
    for row in rows {
        let (id, source_id, json) = row?;
        let patch: Patch = serde_json::from_str(&json)?;
        if patch.id.to_string() != id || patch.source_id.to_string() != source_id {
            return Err(StoreError::InvalidData(
                "patch identifiers disagree with the indexed project data".into(),
            ));
        }
        Quadrilateral::new(patch.geometry.corners).map_err(|error| {
            StoreError::InvalidData(format!("invalid patch {} geometry: {error}", patch.id))
        })?;
        patches.push(patch);
    }
    PatchSet::new(patches.clone())?;
    Ok(patches)
}

fn load_sources(connection: &Connection) -> Result<Vec<StoredSource>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT id, channel, ownership, external_path, sha256, width, height, format, color_type,
                has_alpha, exif_orientation, has_icc_profile, encoded_bytes, owned_bytes, origin_path
         FROM sources ORDER BY CASE channel
            WHEN 'base_color' THEN 0 WHEN 'normal' THEN 1 WHEN 'height' THEN 2
            WHEN 'roughness' THEN 3 WHEN 'metallic' THEN 4 WHEN 'ambient_occlusion' THEN 5
            WHEN 'specular' THEN 6 WHEN 'opacity' THEN 7 WHEN 'edge_mask' THEN 8 ELSE 9 END",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, u32>(5)?,
            row.get::<_, u32>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, bool>(9)?,
            row.get::<_, u16>(10)?,
            row.get::<_, bool>(11)?,
            row.get::<_, u64>(12)?,
            row.get::<_, Option<Vec<u8>>>(13)?,
            row.get::<_, String>(14)?,
        ))
    })?;
    let mut sources = Vec::new();
    for row in rows {
        let (
            id_text,
            channel_text,
            ownership_text,
            external_path,
            sha256,
            width,
            height,
            format,
            color_type,
            has_alpha,
            exif_orientation,
            has_embedded_icc_profile,
            encoded_bytes,
            owned_bytes,
            origin_path,
        ) = row?;
        let input = SourceInput {
            id: id_text
                .parse::<SourceId>()
                .map_err(|_| StoreError::InvalidId(id_text))?,
            ownership: SourceOwnership::from_db_value(&ownership_text)?,
            external_path: external_path.map(PathBuf::from),
            origin_path: PathBuf::from(origin_path),
            sha256,
            width,
            height,
            format,
            color_type,
            has_alpha,
            exif_orientation,
            has_embedded_icc_profile,
            encoded_bytes,
            owned_bytes,
        };
        validate_source_ownership(&input)?;
        sources.push(StoredSource {
            channel: SourceChannel::from_db_value(&channel_text)?,
            input,
        });
    }
    Ok(sources)
}

fn configure(connection: &Connection) -> Result<(), StoreError> {
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "FULL")?;
    connection.busy_timeout(Duration::from_secs(2))?;
    Ok(())
}

fn persist_patch_rows(transaction: &Transaction<'_>, patches: &[Patch]) -> Result<(), StoreError> {
    transaction.execute("DELETE FROM patches", [])?;
    for (ordinal, patch) in patches.iter().enumerate() {
        let ordinal = i64::try_from(ordinal)
            .map_err(|_| StoreError::InvalidData("patch count exceeds SQLite limits".into()))?;
        let json = serde_json::to_string(patch)?;
        transaction.execute(
            "INSERT INTO patches (id, source_id, ordinal, patch_json) VALUES (?1, ?2, ?3, ?4)",
            params![
                patch.id.to_string(),
                patch.source_id.to_string(),
                ordinal,
                json
            ],
        )?;
    }
    Ok(())
}

fn persist_patch_state(
    connection: &mut Connection,
    patches: &[Patch],
    operation: &str,
    payload_json: &str,
) -> Result<(), StoreError> {
    let now = unix_timestamp()?;
    let transaction = connection.transaction()?;
    persist_patch_rows(&transaction, patches)?;
    transaction.execute(
        "INSERT INTO autosave_journal (occurred_unix, operation, payload_json) VALUES (?1, ?2, ?3)",
        params![now, operation, payload_json],
    )?;
    transaction.execute("UPDATE project SET modified_unix = ?1", [now])?;
    transaction.commit()?;
    Ok(())
}

fn migrate(connection: &mut Connection) -> Result<(), StoreError> {
    let mut version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > CURRENT_SCHEMA_VERSION {
        return Err(StoreError::NewerSchema {
            found: version,
            supported: CURRENT_SCHEMA_VERSION,
        });
    }
    if version == 0 {
        let transaction = connection.transaction()?;
        migrate_to_v1(&transaction)?;
        transaction.pragma_update(None, "user_version", 1_u32)?;
        transaction.commit()?;
        version = 1;
    }
    if version == 1 {
        let transaction = connection.transaction()?;
        migrate_to_v2(&transaction)?;
        transaction.pragma_update(None, "user_version", 2_u32)?;
        transaction.commit()?;
        version = 2;
    }
    if version == 2 {
        let transaction = connection.transaction()?;
        migrate_to_v3(&transaction)?;
        transaction.pragma_update(None, "user_version", 3_u32)?;
        transaction.commit()?;
        version = 3;
    }
    if version == 3 {
        let transaction = connection.transaction()?;
        migrate_to_v4(&transaction)?;
        transaction.pragma_update(None, "user_version", 4_u32)?;
        transaction.commit()?;
        version = 4;
    }
    if version == 4 {
        let transaction = connection.transaction()?;
        migrate_to_v5(&transaction)?;
        transaction.pragma_update(None, "user_version", 5_u32)?;
        transaction.commit()?;
    }
    Ok(())
}

fn migrate_to_v1(transaction: &Transaction<'_>) -> Result<(), StoreError> {
    transaction.execute_batch(include_str!("../../../fixtures/projects/schema-v1.sql"))?;
    Ok(())
}

fn migrate_to_v2(transaction: &Transaction<'_>) -> Result<(), StoreError> {
    transaction.execute_batch(include_str!(
        "../../../fixtures/projects/migrate-v1-to-v2.sql"
    ))?;
    Ok(())
}

fn migrate_to_v3(transaction: &Transaction<'_>) -> Result<(), StoreError> {
    transaction.execute_batch(include_str!(
        "../../../fixtures/projects/migrate-v2-to-v3.sql"
    ))?;
    Ok(())
}

fn migrate_to_v4(transaction: &Transaction<'_>) -> Result<(), StoreError> {
    transaction.execute_batch(include_str!(
        "../../../fixtures/projects/migrate-v3-to-v4.sql"
    ))?;
    Ok(())
}

fn migrate_to_v5(transaction: &Transaction<'_>) -> Result<(), StoreError> {
    transaction.execute_batch(include_str!(
        "../../../fixtures/projects/migrate-v4-to-v5.sql"
    ))?;
    Ok(())
}

fn verify_integrity(connection: &Connection) -> Result<(), StoreError> {
    let result: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if result == "ok" {
        Ok(())
    } else {
        Err(StoreError::Integrity(result))
    }
}

fn checkpoint(connection: &Connection) -> Result<(), StoreError> {
    connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    Ok(())
}

fn validate_standalone_database(path: &Path) -> Result<(), StoreError> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    verify_integrity(&connection)?;
    let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == CURRENT_SCHEMA_VERSION {
        Ok(())
    } else {
        Err(StoreError::InvalidData(format!(
            "snapshot schema {version} does not match {CURRENT_SCHEMA_VERSION}"
        )))
    }
}

fn validate_source_ownership(source: &SourceInput) -> Result<(), StoreError> {
    let valid = match source.ownership {
        SourceOwnership::OwnedCopy => {
            source.owned_bytes.is_some() && source.external_path.is_none()
        }
        SourceOwnership::VerifiedExternalReference => {
            source.owned_bytes.is_none() && source.external_path.is_some()
        }
    };
    if valid {
        Ok(())
    } else {
        Err(StoreError::InvalidData(
            "source bytes do not match the explicit ownership mode".into(),
        ))
    }
}

fn unix_timestamp() -> Result<i64, StoreError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| StoreError::InvalidData(error.to_string()))?;
    i64::try_from(duration.as_secs())
        .map_err(|_| StoreError::InvalidData("system timestamp exceeds SQLite range".into()))
}

fn unix_timestamp_millis() -> Result<u128, StoreError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .map_err(|error| StoreError::InvalidData(error.to_string()))
}

fn sibling_temporary_path(destination: &Path) -> PathBuf {
    let mut value = destination.as_os_str().to_owned();
    value.push(format!(".{}.tmp", Uuid::new_v4()));
    PathBuf::from(value)
}

fn sync_file(path: &Path) -> Result<(), StoreError> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?
        .sync_all()?;
    Ok(())
}

#[cfg(debug_assertions)]
fn pause_at_backup_publication_failpoint() -> Result<(), StoreError> {
    let Some(signal) = std::env::var_os("HOT_TRIMMER_BACKUP_PUBLICATION_SIGNAL") else {
        return Ok(());
    };
    fs::write(signal, b"ready-to-publish")?;
    std::thread::sleep(Duration::from_secs(60));
    Ok(())
}

#[cfg(not(debug_assertions))]
const fn pause_at_backup_publication_failpoint() -> Result<(), StoreError> {
    Ok(())
}

fn atomic_publish_new(temporary: &Path, destination: &Path) -> Result<(), StoreError> {
    if destination.exists() {
        return Err(StoreError::AlreadyExists);
    }
    #[cfg(windows)]
    fs::rename(temporary, destination)?;
    #[cfg(not(windows))]
    {
        fs::hard_link(temporary, destination)?;
        fs::remove_file(temporary)?;
    }
    Ok(())
}

fn rotate_recovery_snapshots(recovery_dir: &Path, project_id: ProjectId) -> Result<(), StoreError> {
    let prefix = format!("{project_id}.");
    let mut snapshots = fs::read_dir(recovery_dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name().is_some_and(|name| {
                let name = name.to_string_lossy();
                name.starts_with(&prefix)
                    && name.ends_with(".hottrimmer-recovery")
                    && !name.contains(".baseline.")
            })
        })
        .collect::<Vec<_>>();
    snapshots.sort();
    let remove_count = snapshots.len().saturating_sub(MAX_RECOVERY_SNAPSHOTS);
    for path in snapshots.into_iter().take(remove_count) {
        fs::remove_file(path)?;
    }
    Ok(())
}

struct ProjectLock {
    path: PathBuf,
    token: String,
}

impl ProjectLock {
    fn acquire(project_path: &Path) -> Result<(Self, bool), StoreError> {
        let path = lock_path(project_path);
        let mut stale_lock_recovered = false;
        if path.exists() {
            let active = fs::read_to_string(&path)
                .ok()
                .and_then(|contents| contents.split(':').next()?.parse::<u32>().ok())
                .is_some_and(process_is_running);
            if active {
                return Err(StoreError::Locked);
            }
            let stale = sibling_temporary_path(&path);
            fs::rename(&path, &stale).map_err(|_| StoreError::Locked)?;
            let _ = fs::remove_file(stale);
            stale_lock_recovered = true;
        }

        let token = format!("{}:{}", std::process::id(), Uuid::new_v4());
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(token.as_bytes())?;
        file.sync_all()?;
        Ok((Self { path, token }, stale_lock_recovered))
    }
}

impl Drop for ProjectLock {
    fn drop(&mut self) {
        let owns_lock = fs::read_to_string(&self.path).is_ok_and(|contents| contents == self.token);
        if owns_lock {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn process_is_running(pid: u32) -> bool {
    let mut system = System::new();
    let pid = Pid::from_u32(pid);
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    system.process(pid).is_some()
}

fn lock_path(project_path: &Path) -> PathBuf {
    let mut value = project_path.as_os_str().to_owned();
    value.push(".lock");
    PathBuf::from(value)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use hot_trimmer_domain::{
        NormalizedPoint, Patch, PatchCommand, PatchGeometry, PatchId, PatchProperties,
        RectificationSettings, SourceId,
    };
    use rusqlite::Connection;
    use uuid::Uuid;

    use super::{
        CURRENT_SCHEMA_VERSION, MAX_RECOVERY_SNAPSHOTS, ProjectStore, SourceChannel, SourceInput,
        SourceOwnership, StoreError, lock_path, migrate,
    };

    struct FixtureDir(PathBuf);

    impl FixtureDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("hot-trimmer-store-{}", Uuid::new_v4()));
            fs::create_dir_all(&path).expect("create test directory");
            Self(path)
        }
    }

    impl Drop for FixtureDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn source(width: u32, height: u32) -> SourceInput {
        SourceInput {
            id: SourceId::new(),
            ownership: SourceOwnership::OwnedCopy,
            external_path: None,
            origin_path: PathBuf::from("fixture.png"),
            sha256: "a".repeat(64),
            width,
            height,
            format: "PNG".into(),
            color_type: "Rgba8".into(),
            has_alpha: true,
            exif_orientation: 1,
            has_embedded_icc_profile: false,
            encoded_bytes: 3,
            owned_bytes: Some(vec![1, 2, 3]),
        }
    }

    fn patch(source_id: SourceId, name: &str) -> Patch {
        let point = |x, y| NormalizedPoint::new(x, y).expect("test point");
        Patch {
            id: PatchId::new(),
            source_id,
            name: name.into(),
            enabled: true,
            geometry: PatchGeometry {
                corners: [
                    point(0.1, 0.1),
                    point(0.9, 0.1),
                    point(0.9, 0.9),
                    point(0.1, 0.9),
                ],
                assistance_mask: None,
            },
            properties: PatchProperties::default(),
            rectification: RectificationSettings::default(),
        }
    }

    #[test]
    fn new_database_migrates_to_current_schema_and_reopens() {
        let fixture = FixtureDir::new();
        let path = fixture.0.join("project.hottrimmer");
        let project_id = {
            let store = ProjectStore::create(&path, "Brick Source").expect("create project");
            let summary = store.summary().expect("project summary");
            assert_eq!(summary.name, "Brick Source");
            summary.id
        };
        let connection = Connection::open(&path).expect("open SQLite fixture");
        let version: u32 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("schema version");
        assert_eq!(version, CURRENT_SCHEMA_VERSION);
        drop(connection);
        let reopened = ProjectStore::open(&path).expect("reopen project");
        assert_eq!(reopened.summary().expect("summary").id, project_id);
    }

    #[test]
    fn version_one_fixture_migrates_without_losing_project_or_source() {
        let fixture = FixtureDir::new();
        let path = fixture.0.join("v1.hottrimmer");
        let mut connection = Connection::open(&path).expect("create fixture");
        connection
            .execute_batch(include_str!("../../../fixtures/projects/schema-v1.sql"))
            .expect("v1 schema");
        connection
            .execute_batch(include_str!("../../../fixtures/projects/data-v1.sql"))
            .expect("v1 data");
        connection
            .pragma_update(None, "user_version", 1_u32)
            .expect("mark v1");
        migrate(&mut connection).expect("migrate v1");
        let source_count: u32 = connection
            .query_row("SELECT count(*) FROM sources", [], |row| row.get(0))
            .expect("source count");
        assert_eq!(source_count, 1);
        assert_eq!(
            connection
                .pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
                .expect("schema version"),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn version_two_fixture_migrates_to_material_input_slots() {
        let fixture = FixtureDir::new();
        let path = fixture.0.join("version-two.hottrimmer");
        let mut connection = Connection::open(&path).expect("create fixture");
        connection
            .execute_batch(include_str!("../../../fixtures/projects/schema-v2.sql"))
            .expect("v2 schema");
        connection
            .execute_batch(include_str!("../../../fixtures/projects/data-v1.sql"))
            .expect("v2 data");
        connection
            .pragma_update(None, "user_version", 2_u32)
            .expect("mark v2");
        migrate(&mut connection).expect("migrate v2");
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
                .expect("schema version"),
            CURRENT_SCHEMA_VERSION
        );
        drop(connection);

        let store = ProjectStore::open(&path).expect("open migrated project");
        assert_eq!(store.summary().expect("summary").sources.len(), 1);
        let mut channels = SourceChannel::ALL.into_iter();
        assert_eq!(channels.next(), Some(SourceChannel::BaseColor));
        assert_eq!(channels.count(), 9);
    }

    #[test]
    fn version_three_fixture_adds_source_provenance() {
        let fixture = FixtureDir::new();
        let path = fixture.0.join("version-three.hottrimmer");
        let mut connection = Connection::open(&path).expect("create fixture");
        connection
            .execute_batch(include_str!("../../../fixtures/projects/schema-v3.sql"))
            .expect("v3 schema");
        connection
            .execute_batch(include_str!("../../../fixtures/projects/data-v1.sql"))
            .expect("v3 data");
        connection
            .pragma_update(None, "user_version", 3_u32)
            .expect("mark v3");
        migrate(&mut connection).expect("migrate v3");
        let origin_path: String = connection
            .query_row("SELECT origin_path FROM sources LIMIT 1", [], |row| {
                row.get(0)
            })
            .expect("origin path");
        assert!(origin_path.is_empty());
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
                .expect("schema version"),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn version_four_fixture_adds_empty_patch_storage_transactionally() {
        let fixture = FixtureDir::new();
        let path = fixture.0.join("version-four.hottrimmer");
        let mut connection = Connection::open(&path).expect("create fixture");
        connection
            .execute_batch(include_str!("../../../fixtures/projects/schema-v4.sql"))
            .expect("v4 schema");
        connection
            .execute_batch(include_str!("../../../fixtures/projects/data-v1.sql"))
            .expect("v4 data");
        connection
            .pragma_update(None, "user_version", 4_u32)
            .expect("mark v4");
        migrate(&mut connection).expect("migrate v4");
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
                .expect("schema version"),
            CURRENT_SCHEMA_VERSION
        );
        let patch_count: u32 = connection
            .query_row("SELECT count(*) FROM patches", [], |row| row.get(0))
            .expect("patch count");
        assert_eq!(patch_count, 0);
    }

    #[test]
    fn patch_commands_undo_redo_and_reopen_without_geometry_loss() {
        let fixture = FixtureDir::new();
        let path = fixture.0.join("patches.hottrimmer");
        let mut store = ProjectStore::create(&path, "Patches").expect("create project");
        let base = source(1024, 512);
        let authored = patch(base.id, "Brick course");
        store
            .replace_source(SourceChannel::BaseColor, &base)
            .expect("base source");
        store
            .execute_patch_command(
                &PatchCommand::Create {
                    patch: authored.clone(),
                    index: None,
                },
                None,
            )
            .expect("create patch");
        assert_eq!(store.patches(), std::slice::from_ref(&authored));
        store.undo_patch_command().expect("undo patch");
        assert!(store.patches().is_empty());
        store.redo_patch_command().expect("redo patch");
        assert_eq!(store.patches(), std::slice::from_ref(&authored));
        assert_eq!(
            store
                .autosave_journal()
                .expect("journal")
                .last()
                .expect("entry")
                .operation,
            "patch_redo"
        );
        drop(store);

        let reopened = ProjectStore::open(&path).expect("reopen project");
        assert_eq!(reopened.summary().expect("summary").patches, vec![authored]);
    }

    #[test]
    fn replacing_a_source_reassociates_patches_and_removal_is_blocked() {
        let fixture = FixtureDir::new();
        let path = fixture.0.join("patch-source.hottrimmer");
        let mut store = ProjectStore::create(&path, "Patch source").expect("create project");
        let original = source(64, 64);
        store
            .replace_source(SourceChannel::BaseColor, &original)
            .expect("base source");
        store
            .execute_patch_command(
                &PatchCommand::Create {
                    patch: patch(original.id, "Patch"),
                    index: None,
                },
                None,
            )
            .expect("create patch");
        assert!(matches!(
            store.remove_source(SourceChannel::BaseColor),
            Err(StoreError::SourceInUseByPatches)
        ));
        let replacement = source(64, 64);
        store
            .replace_source(SourceChannel::BaseColor, &replacement)
            .expect("replace source");
        assert_eq!(store.patches()[0].source_id, replacement.id);
        drop(store);
        assert_eq!(
            ProjectStore::open(&path).expect("reopen").patches()[0].source_id,
            replacement.id
        );
    }

    #[test]
    fn failed_v1_migration_rolls_back_and_preserves_version() {
        let fixture = FixtureDir::new();
        let path = fixture.0.join("broken-v1.hottrimmer");
        let mut connection = Connection::open(&path).expect("create fixture");
        connection
            .execute_batch("CREATE TABLE project(id TEXT); PRAGMA user_version=1;")
            .expect("broken v1 fixture");
        assert!(migrate(&mut connection).is_err());
        let version: u32 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("schema version");
        assert_eq!(version, 1);
        let project_exists: u32 = connection
            .query_row("SELECT count(*) FROM project", [], |row| row.get(0))
            .expect("original table remains");
        assert_eq!(project_exists, 0);
    }

    #[test]
    fn concurrent_open_is_rejected_and_stale_lock_is_recovered() {
        let fixture = FixtureDir::new();
        let path = fixture.0.join("locked.hottrimmer");
        let store = ProjectStore::create(&path, "Locked").expect("create project");
        assert!(matches!(ProjectStore::open(&path), Err(StoreError::Locked)));
        drop(store);
        fs::write(lock_path(&path), "4294967295:dead").expect("stale lock");
        let reopened = ProjectStore::open(&path).expect("recover stale lock");
        assert!(reopened.summary().expect("summary").stale_lock_recovered);
    }

    #[test]
    fn pbr_sources_require_base_color_registration() {
        let fixture = FixtureDir::new();
        let path = fixture.0.join("registration.hottrimmer");
        let mut store = ProjectStore::create(&path, "Registered").expect("create project");
        assert!(matches!(
            store.replace_source(SourceChannel::Normal, &source(4, 6)),
            Err(StoreError::BaseColorRequired)
        ));
        store
            .replace_source(SourceChannel::BaseColor, &source(4, 6))
            .expect("base color");
        assert!(matches!(
            store.replace_source(SourceChannel::Normal, &source(8, 6)),
            Err(StoreError::RegistrationMismatch { .. })
        ));
        store
            .replace_source(SourceChannel::Normal, &source(4, 6))
            .expect("registered normal");
        for channel in SourceChannel::ALL
            .into_iter()
            .filter(|channel| !matches!(channel, SourceChannel::BaseColor | SourceChannel::Normal))
        {
            store
                .replace_source(channel, &source(4, 6))
                .expect("registered material input");
        }
        assert_eq!(store.summary().expect("summary").sources.len(), 10);
        assert_eq!(store.autosave_journal().expect("journal").len(), 10);
        assert!(matches!(
            store.remove_source(SourceChannel::BaseColor),
            Err(StoreError::BaseColorInUse)
        ));
        store
            .remove_source(SourceChannel::Specular)
            .expect("remove companion input");
        assert_eq!(store.summary().expect("summary").sources.len(), 9);
        let journal = store.autosave_journal().expect("journal");
        assert_eq!(journal.len(), 11);
        assert_eq!(
            journal.last().expect("removal entry").operation,
            "remove_source"
        );
        store
            .rename_project("Renamed Material")
            .expect("rename project");
        assert_eq!(store.summary().expect("summary").name, "Renamed Material");
        assert_eq!(
            store
                .autosave_journal()
                .expect("journal")
                .last()
                .expect("rename entry")
                .operation,
            "rename_project"
        );
    }

    #[test]
    fn backup_restore_and_save_as_keep_previous_valid_state() {
        let fixture = FixtureDir::new();
        let path = fixture.0.join("source.hottrimmer");
        let snapshot = fixture.0.join("baseline.hottrimmer-recovery");
        let copy = fixture.0.join("copy.hottrimmer");
        let mut store = ProjectStore::create(&path, "Source").expect("create project");
        store.backup_atomic(&snapshot).expect("baseline");
        store
            .replace_source(SourceChannel::BaseColor, &source(4, 6))
            .expect("persist source");
        assert_eq!(store.summary().expect("summary").sources.len(), 1);
        store.restore_from(&snapshot).expect("restore baseline");
        assert!(store.summary().expect("summary").sources.is_empty());
        let saved_as = store.save_as(&copy).expect("save as");
        assert_eq!(
            saved_as.summary().expect("copy summary").id,
            store.summary().expect("original summary").id
        );
    }

    #[test]
    fn recovery_snapshots_rotate_without_deleting_the_baseline() {
        let fixture = FixtureDir::new();
        let path = fixture.0.join("recovery.hottrimmer");
        let recovery = fixture.0.join("recovery");
        let store = ProjectStore::create(&path, "Recovery").expect("create project");
        let baseline = store.baseline_path(&recovery);
        store.backup_atomic(&baseline).expect("baseline");
        for _ in 0..(MAX_RECOVERY_SNAPSHOTS + 2) {
            store
                .create_recovery_snapshot(&recovery)
                .expect("recovery snapshot");
        }
        let snapshot_count = fs::read_dir(&recovery)
            .expect("recovery dir")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".hottrimmer-recovery")
            })
            .count();
        assert_eq!(snapshot_count, MAX_RECOVERY_SNAPSHOTS + 1);
        assert!(baseline.exists());
    }

    #[test]
    fn recovery_snapshot_contains_committed_patch_geometry() {
        let fixture = FixtureDir::new();
        let path = fixture.0.join("patch-recovery.hottrimmer");
        let recovery = fixture.0.join("recovery");
        let mut store = ProjectStore::create(&path, "Patch recovery").expect("create project");
        let base = source(1024, 1024);
        let authored = patch(base.id, "Recoverable patch");
        store
            .replace_source(SourceChannel::BaseColor, &base)
            .expect("base source");
        store
            .execute_patch_command(
                &PatchCommand::Create {
                    patch: authored.clone(),
                    index: None,
                },
                None,
            )
            .expect("create patch");
        let snapshot = store
            .create_recovery_snapshot(&recovery)
            .expect("recovery snapshot");
        drop(store);

        let recovered = ProjectStore::open(&snapshot).expect("open recovery snapshot");
        assert_eq!(
            recovered.summary().expect("summary").patches,
            vec![authored]
        );
    }
}
