#![doc = "Transactional `SQLite` project persistence, migration, locking, autosave, and recovery."]

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use hot_trimmer_domain::{
    AssignmentProvenance, AuthoredLayoutPreset, AuthoredLayoutPresetRegion, ChannelInterpretation,
    ChannelRegistration, ContentDigest, DelightingIntent, LayoutId, MaterialCalibrationCommand,
    MaterialCalibrationIntent, MaterialChannelRole, MaterialClassificationCommand,
    MaterialClassificationIntent, MaterialMapContent, MaterialMapKind, MaterialSourceSet,
    NormalConvention, NormalizedBounds, NormalizedPoint, NormalizedScalar, OrientedPixelSize,
    Patch, PatchCommand, PatchCommandError, PatchEditOutcome, PatchSet, ProjectId, Projection,
    RegionSourceOverride, RegistrationDiagnostic, RegistrationDiagnosticCode,
    RegistrationRecoveryChoice, SourceFrame, SourceId, SourceSetId, TemplateRegistry,
    TrimSheetDocument, TrimSheetDocumentCommand,
};
use hot_trimmer_geometry::Quadrilateral;
use hot_trimmer_image_io::{ColorPolicy, DecodeLimits, inspect_path_with_policy};
use rusqlite::{Connection, DatabaseName, OpenFlags, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessesToUpdate, System};
use thiserror::Error;
use uuid::Uuid;

pub const CURRENT_SCHEMA_VERSION: u32 = 14;
pub const MAX_RECOVERY_SNAPSHOTS: usize = 5;
const MAX_PROJECT_HISTORY: usize = 256;

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

    #[must_use]
    pub const fn material_role(self) -> MaterialChannelRole {
        match self {
            Self::BaseColor => MaterialChannelRole::BaseColor,
            Self::Normal => MaterialChannelRole::Normal,
            Self::Height => MaterialChannelRole::Height,
            Self::Roughness => MaterialChannelRole::Roughness,
            Self::Metallic => MaterialChannelRole::Metallic,
            Self::AmbientOcclusion => MaterialChannelRole::AmbientOcclusion,
            Self::Specular => MaterialChannelRole::Specular,
            Self::Opacity => MaterialChannelRole::Opacity,
            Self::EdgeMask => MaterialChannelRole::EdgeMask,
            Self::MaterialId => MaterialChannelRole::MaterialId,
        }
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
    pub source_set_id: Uuid,
    pub channel: SourceChannel,
    pub registration: ChannelRegistration,
    pub input: SourceInput,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSetSnapshot {
    pub id: SourceSetId,
    pub name: String,
    pub exemplar_group: Option<String>,
    pub source_revision: u64,
    pub registration_digest: ContentDigest,
    pub delighting: DelightingIntent,
    pub classification: MaterialClassificationIntent,
    pub calibration: MaterialCalibrationIntent,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectSummary {
    pub id: ProjectId,
    pub name: String,
    pub path: PathBuf,
    pub sources: Vec<StoredSource>,
    pub source_sets: Vec<SourceSetSnapshot>,
    pub patches: Vec<Patch>,
    pub document: Option<TrimSheetDocument>,
    pub legacy_layout_discarded: bool,
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
    #[error(
        "the {channel} source uses orientation {actual_orientation}; registered sources require orientation {expected_orientation}"
    )]
    OrientationMismatch {
        channel: &'static str,
        expected_orientation: u16,
        actual_orientation: u16,
    },
    #[error("the {channel} source has an invalid interpretation for its assigned role")]
    ChannelInterpretationMismatch { channel: &'static str },
    #[error("only a Normal channel may carry a tangent-space normal convention")]
    NormalConventionOnScalar,
    #[error("channel assignment confidence must be between 0 and 1000")]
    InvalidAssignmentConfidence,
    #[error("owned source bytes do not match their immutable SHA-256 digest")]
    ImmutableDigestMismatch,
    #[error("owned source byte count does not match the registered encoded byte count")]
    EncodedByteCountMismatch,
    #[error("the verified external source could not be re-inspected at registration: {0}")]
    ExternalSourceVerification(String),
    #[error("the verified external source changed between inspection and registration: {field}")]
    ExternalSourceChanged { field: &'static str },
    #[error("the source is used by one or more patches")]
    SourceInUseByPatches,
    #[error("the trim-sheet document command is invalid: {0}")]
    Document(String),
    #[error("the patch command is invalid: {0}")]
    PatchCommand(#[from] PatchCommandError),
    #[error("patch data could not be serialized: {0}")]
    PatchSerialization(#[from] serde_json::Error),
}

impl StoreError {
    /// Projects registration failures into the typed Stage 1 diagnostic contract.
    #[must_use]
    pub fn registration_diagnostic(
        &self,
        channel: SourceChannel,
    ) -> Option<RegistrationDiagnostic> {
        let (code, recovery_choices) = match self {
            Self::BaseColorRequired => (
                RegistrationDiagnosticCode::BaseColorRequired,
                vec![RegistrationRecoveryChoice::AssignBaseColor],
            ),
            Self::RegistrationMismatch { .. } => (
                RegistrationDiagnosticCode::OrientedDimensionMismatch,
                vec![RegistrationRecoveryChoice::ChooseMatchingDimensions],
            ),
            Self::OrientationMismatch { .. } => (
                RegistrationDiagnosticCode::OrientationMismatch,
                vec![RegistrationRecoveryChoice::ReorientCompanionExternally],
            ),
            Self::ChannelInterpretationMismatch { .. } => (
                RegistrationDiagnosticCode::ChannelInterpretationMismatch,
                vec![RegistrationRecoveryChoice::ReassignChannelRole],
            ),
            Self::NormalConventionOnScalar => (
                RegistrationDiagnosticCode::NormalConventionOnScalar,
                vec![RegistrationRecoveryChoice::ReassignChannelRole],
            ),
            Self::InvalidAssignmentConfidence => (
                RegistrationDiagnosticCode::InvalidConfidence,
                vec![RegistrationRecoveryChoice::ReassignChannelRole],
            ),
            _ => return None,
        };
        Some(RegistrationDiagnostic {
            code,
            channel: channel.material_role(),
            message: self.to_string(),
            recovery_choices,
        })
    }
}

pub struct ProjectStore {
    connection: Connection,
    project_path: PathBuf,
    _lock: ProjectLock,
    stale_lock_recovered: bool,
    patch_set: PatchSet,
    document: Option<TrimSheetDocument>,
    history: Vec<ProjectHistoryEntry>,
    history_cursor: usize,
    document_history: Vec<TrimSheetDocument>,
    document_history_cursor: usize,
}

#[derive(Clone, Debug, PartialEq)]
struct ProjectHistoryEntry {
    before_patches: Vec<Patch>,
    after_patches: Vec<Patch>,
    coalescing_group: Option<u64>,
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
        let default_delighting = serde_json::to_string(&DelightingIntent::default())?;
        let default_classification =
            serde_json::to_string(&MaterialClassificationIntent::default())?;
        let default_calibration = serde_json::to_string(&MaterialCalibrationIntent::default())?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO project (id, name, created_unix, modified_unix) VALUES (?1, ?2, ?3, ?3)",
            params![project_id.to_string(), name, now],
        )?;
        transaction.execute(
            "INSERT INTO source_sets (
                id, name, ordinal, exemplar_group, source_revision, registration_digest,
                delighting_json, classification_json, calibration_json
             ) VALUES (?1, 'Material 1', 0, NULL, 0, ?2, ?3, ?4, ?5)",
            params![
                project_id.to_string(),
                ContentDigest::sha256(b"empty-registered-channel-set").0,
                default_delighting,
                default_classification,
                default_calibration,
            ],
        )?;
        transaction.commit()?;
        checkpoint(&connection)?;

        Ok(Self {
            connection,
            project_path: path.to_path_buf(),
            _lock: lock,
            stale_lock_recovered,
            patch_set: PatchSet::default(),
            document: None,
            history: Vec::new(),
            history_cursor: 0,
            document_history: Vec::new(),
            document_history_cursor: 0,
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
        let mut document = load_document(&connection)?;
        if let Some(current) = document.as_mut()
            && snapshot_legacy_authored_layout(current)?
        {
            persist_document_state(
                &mut connection,
                Some(current),
                "migrate_accepted_topology_to_authored_preset",
            )?;
        }
        let store = Self {
            connection,
            project_path: path.to_path_buf(),
            _lock: lock,
            stale_lock_recovered,
            patch_set,
            document,
            history: Vec::new(),
            history_cursor: 0,
            document_history: Vec::new(),
            document_history_cursor: 0,
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
        if version != CURRENT_SCHEMA_VERSION {
            return Err(StoreError::InvalidData(format!(
                "project schema {version} is not the current Stage 1 schema {CURRENT_SCHEMA_VERSION}"
            )));
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

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.project_path
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
        let source_set_id = self.default_source_set_id()?;
        self.replace_source_in_set(source_set_id, channel, source)
    }

    /// Replaces a map in one independently registered material source set.
    ///
    /// # Errors
    ///
    /// Returns a typed error when ownership, registration, or persistence fails.
    pub fn replace_source_in_set(
        &mut self,
        source_set_id: Uuid,
        channel: SourceChannel,
        source: &SourceInput,
    ) -> Result<(), StoreError> {
        self.replace_registered_source_in_set(
            source_set_id,
            source,
            ChannelRegistration::explicit(channel.material_role()),
        )
    }

    /// Replaces a registered channel while preserving its explicit interpretation and provenance.
    pub fn replace_registered_source_in_set(
        &mut self,
        source_set_id: Uuid,
        source: &SourceInput,
        registration: ChannelRegistration,
    ) -> Result<(), StoreError> {
        let channel = SourceChannel::ALL
            .into_iter()
            .find(|candidate| candidate.material_role() == registration.role)
            .expect("every domain material role has a persisted source channel");
        validate_source_ownership(source)?;
        verify_external_source(source, channel)?;
        validate_channel_registration(channel, &registration)?;
        self.validate_registration(
            source_set_id,
            channel,
            source.width,
            source.height,
            source.exif_orientation,
        )?;
        let previous_source_id = self
            .connection
            .query_row(
                "SELECT id FROM sources WHERE source_set_id = ?1 AND channel = ?2",
                params![source_set_id.to_string(), channel.as_db_value()],
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
        let source_set_revision = self
            .connection
            .query_row(
                "SELECT source_revision FROM source_sets WHERE id = ?1",
                [source_set_id.to_string()],
                |row| row.get::<_, u64>(0),
            )
            .optional()?
            .unwrap_or(0)
            .saturating_add(1);
        let (next_document, rebind_diagnostic) = if channel == SourceChannel::BaseColor {
            self.document
                .as_ref()
                .filter(|document| {
                    document.source_frame.as_ref().is_some_and(|frame| {
                        frame.source_set_id == SourceSetId::from_bytes(*source_set_id.as_bytes())
                    })
                })
                .map(|document| {
                    rebind_source_frame(
                        document,
                        OrientedPixelSize {
                            width: source.width,
                            height: source.height,
                        },
                        source_set_revision,
                    )
                })
                .transpose()?
                .map_or((None, None), |(document, diagnostic)| {
                    (Some(document), Some(diagnostic))
                })
        } else {
            (None, None)
        };
        let now = unix_timestamp()?;
        let default_delighting = serde_json::to_string(&DelightingIntent::default())?;
        let default_classification =
            serde_json::to_string(&MaterialClassificationIntent::default())?;
        let default_calibration = serde_json::to_string(&MaterialCalibrationIntent::default())?;
        let transaction = self.connection.transaction()?;
        let next_ordinal: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(ordinal) + 1, 0) FROM source_sets",
            [],
            |row| row.get(0),
        )?;
        transaction.execute(
            "INSERT INTO source_sets (
                id, name, ordinal, exemplar_group, source_revision, registration_digest,
                delighting_json, classification_json, calibration_json
             ) VALUES (?1, ?2, ?3, NULL, 0, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO NOTHING",
            params![
                source_set_id.to_string(),
                source.origin_path.file_stem().map_or_else(
                    || "Material".into(),
                    |name| name.to_string_lossy().into_owned(),
                ),
                next_ordinal,
                ContentDigest::sha256(b"empty-registered-channel-set").0,
                default_delighting,
                default_classification,
                default_calibration,
            ],
        )?;
        transaction.execute(
            "DELETE FROM sources WHERE source_set_id = ?1 AND channel = ?2",
            params![source_set_id.to_string(), channel.as_db_value()],
        )?;
        persist_patch_rows(&transaction, next_patch_set.patches())?;
        transaction.execute(
            "INSERT INTO sources (
                id, source_set_id, channel, ownership, external_path, sha256, width, height, format, color_type,
                has_alpha, exif_orientation, has_icc_profile, encoded_bytes, owned_bytes, origin_path,
                interpretation, normal_convention, assignment_provenance, confidence_milli
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                ?17, ?18, ?19, ?20
             )",
            params![
                source.id.to_string(),
                source_set_id.to_string(),
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
                interpretation_db(registration.interpretation),
                normal_convention_db(registration.normal_convention),
                assignment_provenance_db(registration.assignment_provenance),
                i64::from(registration.confidence_milli),
            ],
        )?;
        advance_source_authority(&transaction, source_set_id)?;
        let payload = format!(
            "{{\"sourceSetId\":\"{}\",\"channel\":\"{}\",\"sourceId\":\"{}\",\"sha256\":\"{}\"{} }}",
            source_set_id,
            channel.as_db_value(),
            source.id,
            source.sha256,
            rebind_diagnostic
                .as_ref()
                .map_or_else(String::new, |diagnostic| format!(
                    ",\"sourceFrameRebind\":\"{}\"",
                    diagnostic.replace('"', "\\\"")
                ))
        );
        transaction.execute(
            "INSERT INTO autosave_journal (occurred_unix, operation, payload_json)
             VALUES (?1, 'replace_source', ?2)",
            params![now, payload],
        )?;
        transaction.execute("UPDATE project SET modified_unix = ?1", [now])?;
        if let Some(document) = next_document.as_ref() {
            persist_document_state_in_transaction(
                &transaction,
                Some(document),
                "replace_source_rebind",
                now,
            )?;
        }
        transaction.commit()?;
        self.patch_set = next_patch_set;
        if let Some(document) = next_document {
            self.document_history.clear();
            self.document_history_cursor = 0;
            self.document = Some(document);
        }
        self.clear_history();
        checkpoint(&self.connection)
    }

    /// Removes one material-input slot and journals the command transactionally.
    ///
    /// # Errors
    ///
    /// Returns a typed error when Base Color still anchors companion inputs or persistence fails.
    pub fn remove_source(&mut self, channel: SourceChannel) -> Result<(), StoreError> {
        let source_set_id = self.default_source_set_id()?;
        self.remove_source_in_set(source_set_id, channel)
    }

    /// Removes one map from one material source set.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the source is required or persistence fails.
    pub fn remove_source_in_set(
        &mut self,
        source_set_id: Uuid,
        channel: SourceChannel,
    ) -> Result<(), StoreError> {
        if channel == SourceChannel::BaseColor {
            let companions: u32 = self.connection.query_row(
                "SELECT COUNT(*) FROM sources WHERE source_set_id = ?1 AND channel <> 'base_color'",
                [source_set_id.to_string()],
                |row| row.get(0),
            )?;
            if companions > 0 {
                return Err(StoreError::BaseColorInUse);
            }
        }
        let source_id = self
            .connection
            .query_row(
                "SELECT id FROM sources WHERE source_set_id = ?1 AND channel = ?2",
                params![source_set_id.to_string(), channel.as_db_value()],
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
            "DELETE FROM sources WHERE source_set_id = ?1 AND channel = ?2",
            params![source_set_id.to_string(), channel.as_db_value()],
        )?;
        if removed == 0 {
            return Err(StoreError::InvalidData(format!(
                "empty material input slot: {}",
                channel.as_db_value()
            )));
        }
        advance_source_authority(&transaction, source_set_id)?;
        let payload = format!(
            "{{\"sourceSetId\":\"{}\",\"channel\":\"{}\"}}",
            source_set_id,
            channel.as_db_value()
        );
        transaction.execute(
            "INSERT INTO autosave_journal (occurred_unix, operation, payload_json)
             VALUES (?1, 'remove_source', ?2)",
            params![now, payload],
        )?;
        transaction.execute("UPDATE project SET modified_unix = ?1", [now])?;
        transaction.commit()?;
        self.clear_history();
        checkpoint(&self.connection)
    }

    /// Removes one independent, unreferenced material source set atomically.
    pub fn remove_source_set(&mut self, source_set_id: Uuid) -> Result<(), StoreError> {
        let exists: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM source_sets WHERE id = ?1)",
            [source_set_id.to_string()],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(StoreError::InvalidData(
                "material source does not exist".into(),
            ));
        }
        let source_ids = {
            let mut statement = self
                .connection
                .prepare("SELECT id FROM sources WHERE source_set_id = ?1")?;
            let rows =
                statement.query_map([source_set_id.to_string()], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        if self.patch_set.patches().iter().any(|patch| {
            source_ids
                .iter()
                .any(|id| id == &patch.source_id.to_string())
        }) {
            return Err(StoreError::SourceInUseByPatches);
        }
        let next_document = self
            .document
            .as_ref()
            .map(|document| {
                document.with_assets(
                    document
                        .materials
                        .iter()
                        .filter(|material| material.id.to_string() != source_set_id.to_string())
                        .cloned()
                        .collect(),
                    self.patch_set.patches().to_vec(),
                )
            })
            .transpose()
            .map_err(|error| StoreError::Document(error.to_string()))?;
        let now = unix_timestamp()?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM source_derived_cache WHERE source_set_id = ?1",
            [source_set_id.to_string()],
        )?;
        transaction.execute(
            "DELETE FROM sources WHERE source_set_id = ?1",
            [source_set_id.to_string()],
        )?;
        transaction.execute(
            "DELETE FROM source_sets WHERE id = ?1",
            [source_set_id.to_string()],
        )?;
        transaction.execute("UPDATE project SET modified_unix = ?1", [now])?;
        transaction.execute("INSERT INTO autosave_journal (occurred_unix, operation, payload_json) VALUES (?1, 'remove_source_set', json_object('sourceSetId', ?2))",
            params![now, source_set_id.to_string()])?;
        if let Some(document) = next_document.as_ref() {
            persist_document_state_in_transaction(
                &transaction,
                Some(document),
                "remove_source_set",
                now,
            )?;
        }
        transaction.commit()?;
        self.document = next_document;
        self.clear_history();
        checkpoint(&self.connection)
    }

    /// Groups independently registered exemplars without assigning a material behavior class.
    pub fn set_exemplar_group(
        &mut self,
        source_set_id: Uuid,
        exemplar_group: Option<&str>,
    ) -> Result<(), StoreError> {
        let exemplar_group = exemplar_group
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if exemplar_group.is_some_and(|value| value.len() > 255) {
            return Err(StoreError::InvalidData(
                "exemplar group must contain at most 255 bytes".into(),
            ));
        }
        let transaction = self.connection.transaction()?;
        let updated = transaction.execute(
            "UPDATE source_sets SET exemplar_group = ?2 WHERE id = ?1",
            params![source_set_id.to_string(), exemplar_group],
        )?;
        if updated == 0 {
            return Err(StoreError::InvalidData(
                "material source does not exist".into(),
            ));
        }
        advance_source_authority(&transaction, source_set_id)?;
        transaction.commit()?;
        self.clear_history();
        checkpoint(&self.connection)
    }

    /// Persists explicit Stage 4 route intent without deriving it from source metadata.
    pub fn set_delighting_intent(
        &mut self,
        source_set_id: Uuid,
        intent: &DelightingIntent,
    ) -> Result<(), StoreError> {
        let encoded = serde_json::to_string(intent)?;
        let transaction = self.connection.transaction()?;
        let updated = transaction.execute(
            "UPDATE source_sets SET delighting_json = ?2 WHERE id = ?1",
            params![source_set_id.to_string(), encoded],
        )?;
        if updated == 0 {
            return Err(StoreError::InvalidData(
                "material source does not exist".into(),
            ));
        }
        transaction.execute(
            "DELETE FROM source_derived_cache WHERE source_set_id = ?1",
            [source_set_id.to_string()],
        )?;
        transaction.commit()?;
        self.clear_history();
        checkpoint(&self.connection)
    }

    /// Persists a typed Stage 5 routing command without changing measured analysis evidence.
    pub fn apply_material_classification_command(
        &mut self,
        source_set_id: Uuid,
        command: MaterialClassificationCommand,
    ) -> Result<(), StoreError> {
        let encoded: String = self
            .connection
            .query_row(
                "SELECT classification_json FROM source_sets WHERE id = ?1",
                [source_set_id.to_string()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::InvalidData("material source does not exist".into()))?;
        let mut intent: MaterialClassificationIntent = serde_json::from_str(&encoded)?;
        intent.apply(command);
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE source_sets SET classification_json = ?2 WHERE id = ?1",
            params![source_set_id.to_string(), serde_json::to_string(&intent)?],
        )?;
        transaction.execute(
            "DELETE FROM source_derived_cache WHERE source_set_id = ?1",
            [source_set_id.to_string()],
        )?;
        transaction.commit()?;
        self.clear_history();
        checkpoint(&self.connection)
    }

    /// Persists a typed Stage 6 command and invalidates all downstream physical footprints.
    pub fn apply_material_calibration_command(
        &mut self,
        source_set_id: Uuid,
        command: MaterialCalibrationCommand,
    ) -> Result<(), StoreError> {
        let encoded: String = self
            .connection
            .query_row(
                "SELECT calibration_json FROM source_sets WHERE id = ?1",
                [source_set_id.to_string()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::InvalidData("material source does not exist".into()))?;
        let mut intent: MaterialCalibrationIntent = serde_json::from_str(&encoded)?;
        intent.apply(command).map_err(|failure| {
            StoreError::InvalidData(format!("invalid material calibration command: {failure:?}"))
        })?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE source_sets SET calibration_json = ?2 WHERE id = ?1",
            params![source_set_id.to_string(), serde_json::to_string(&intent)?],
        )?;
        transaction.execute(
            "DELETE FROM source_derived_cache WHERE source_set_id = ?1",
            [source_set_id.to_string()],
        )?;
        transaction.commit()?;
        self.clear_history();
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

    /// Renames one stable material source set without changing its identity or content revision.
    pub fn rename_source_set(&mut self, source_set_id: Uuid, name: &str) -> Result<(), StoreError> {
        let name = name.trim();
        if name.is_empty() || name.len() > 255 {
            return Err(StoreError::InvalidData(
                "source name must be 1 to 255 characters".into(),
            ));
        }
        let now = unix_timestamp()?;
        let transaction = self.connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE source_sets SET name = ?2 WHERE id = ?1",
            params![source_set_id.to_string(), name],
        )?;
        if changed != 1 {
            return Err(StoreError::InvalidData(
                "material source does not exist".into(),
            ));
        }
        transaction.execute("UPDATE project SET modified_unix = ?1", [now])?;
        transaction.execute("INSERT INTO autosave_journal (occurred_unix, operation, payload_json) VALUES (?1, 'rename_source_set', json_object('sourceSetId', ?2, 'name', ?3))",
            params![now, source_set_id.to_string(), name])?;
        transaction.commit()?;
        checkpoint(&self.connection)
    }

    #[must_use]
    pub fn patches(&self) -> &[Patch] {
        self.patch_set.patches()
    }

    #[must_use]
    pub const fn document(&self) -> Option<&TrimSheetDocument> {
        self.document.as_ref()
    }

    #[must_use]
    pub const fn can_undo_document_command(&self) -> bool {
        self.document_history_cursor > 0
    }

    #[must_use]
    pub fn can_redo_document_command(&self) -> bool {
        self.document_history_cursor < self.document_history.len()
    }

    /// Creates the one authoritative document directly from a registered template and assets.
    pub fn create_trim_sheet_document(
        &mut self,
        template_id: &str,
        template_version: &str,
    ) -> Result<&TrimSheetDocument, StoreError> {
        let registry = TemplateRegistry::built_in()
            .map_err(|error| StoreError::Document(error.to_string()))?;
        let template = registry
            .get(template_id, template_version)
            .ok_or_else(|| StoreError::Document("selected template is not registered".into()))?;
        let materials = load_material_catalog(&self.connection)?;
        if materials.is_empty() {
            return Err(StoreError::BaseColorRequired);
        }
        let document = TrimSheetDocument::from_template(
            LayoutId::new(),
            template,
            materials,
            self.patch_set.patches().to_vec(),
        )
        .map_err(|error| StoreError::Document(error.to_string()))?;
        persist_document_state(
            &mut self.connection,
            Some(&document),
            "create_trim_sheet_document",
        )?;
        self.document = Some(document);
        self.document_history.clear();
        self.document_history_cursor = 0;
        Ok(self.document.as_ref().expect("document just assigned"))
    }

    /// Creates the primary source-frame workflow. The template argument remains available to
    /// the compatibility command above, but new source-first documents never select a fixed
    /// template or infer source crops from destination rectangles.
    pub fn create_source_frame_document(&mut self) -> Result<&TrimSheetDocument, StoreError> {
        let materials = load_material_catalog(&self.connection)?;
        let primary = materials
            .iter()
            .find(|material| {
                material
                    .maps
                    .iter()
                    .any(|map| map.kind == MaterialMapKind::BaseColor)
            })
            .ok_or(StoreError::BaseColorRequired)?
            .id;
        let source = load_sources(&self.connection)?
            .into_iter()
            .find(|source| {
                source.source_set_id == Uuid::from_bytes(primary.to_bytes())
                    && source.channel == SourceChannel::BaseColor
            })
            .ok_or(StoreError::BaseColorRequired)?;
        let source_set = load_source_sets(&self.connection)?
            .into_iter()
            .find(|source_set| source_set.id == primary)
            .ok_or(StoreError::BaseColorRequired)?;
        let frame = SourceFrame::centered_largest(
            primary,
            OrientedPixelSize {
                width: source.input.width,
                height: source.input.height,
            },
            [1, 1],
            source_set.source_revision,
        );
        let document_id = LayoutId::new();
        let document = TrimSheetDocument::from_authored_layout_preset(
            document_id,
            frame,
            hot_trimmer_domain::diagonal_cascade_authored_preset(),
            document_id.to_string(),
            hot_trimmer_domain::PixelSize {
                width: 2_048,
                height: 2_048,
            },
            materials,
            self.patch_set.patches().to_vec(),
        )
        .map_err(|error| StoreError::Document(error.to_string()))?;
        persist_document_state(
            &mut self.connection,
            Some(&document),
            "create_source_frame_document",
        )?;
        self.document = Some(document);
        self.document_history.clear();
        self.document_history_cursor = 0;
        Ok(self.document.as_ref().expect("document just assigned"))
    }

    pub fn regenerate_source_frame_partition(
        &mut self,
        target_region_count: u32,
    ) -> Result<&TrimSheetDocument, StoreError> {
        let current = self
            .document
            .as_ref()
            .ok_or_else(|| StoreError::Document("create a source-frame document first".into()))?
            .clone();
        let frame = current.source_frame.clone().ok_or_else(|| {
            StoreError::Document("the current document is not source-frame authored".into())
        })?;
        let previous = current
            .partition_provenance
            .as_ref()
            .ok_or_else(|| StoreError::Document("source-frame provenance is missing".into()))?;
        let mut recipe = previous.recipe.clone();
        recipe.target_region_count = target_region_count;
        let mut next = TrimSheetDocument::from_source_frame(
            current.id,
            frame,
            recipe,
            current.render_settings.output_size,
            current.materials.clone(),
            current.patches.clone(),
        )
        .map_err(|error| StoreError::Document(error.to_string()))?;
        next.document_revision = current.document_revision.saturating_add(1);
        next.topology_revision = current.topology_revision.saturating_add(1);
        next.appearance_revision = next.document_revision;
        persist_document_state(
            &mut self.connection,
            Some(&next),
            "regenerate_source_frame_partition",
        )?;
        self.document = Some(next);
        self.document_history.clear();
        self.document_history_cursor = 0;
        Ok(self.document.as_ref().expect("document just assigned"))
    }

    /// Validates and persists one document command and one journal entry in a single transaction.
    pub fn execute_document_command(
        &mut self,
        command: &TrimSheetDocumentCommand,
    ) -> Result<&TrimSheetDocument, StoreError> {
        let before = self
            .document
            .as_ref()
            .ok_or_else(|| StoreError::Document("create a trim sheet first".into()))?
            .clone();
        let after = before
            .apply_command(command)
            .map_err(|error| StoreError::Document(error.to_string()))?;
        persist_document_state(
            &mut self.connection,
            Some(&after),
            document_operation(command),
        )?;
        self.document_history.truncate(self.document_history_cursor);
        self.document_history.push(before);
        self.document_history_cursor = self.document_history.len();
        self.document = Some(after);
        Ok(self.document.as_ref().unwrap())
    }

    pub fn refresh_document_assets(&mut self) -> Result<bool, StoreError> {
        let Some(current) = self.document.clone() else {
            return Ok(false);
        };
        let materials = load_material_catalog(&self.connection)?;
        let source_sets = load_source_sets(&self.connection)?;
        let sources = load_sources(&self.connection)?;
        let patches = self.patch_set.patches().to_vec();
        let mut next = current.clone();
        let mut changed = false;
        if next.materials != materials {
            next.materials = materials;
            changed = true;
        }
        if next.patches != patches {
            next.patches = patches;
            changed = true;
        }
        if let Some(frame) = next.source_frame.as_ref() {
            let owner = source_sets
                .iter()
                .find(|source_set| source_set.id == frame.source_set_id)
                .ok_or_else(|| {
                    StoreError::Document(format!(
                        "SourceFrame owner {} is missing",
                        frame.source_set_id
                    ))
                })?;
            if owner.source_revision != frame.source_revision {
                let source_set_id = Uuid::from_bytes(frame.source_set_id.to_bytes());
                let source = sources
                    .iter()
                    .find(|source| {
                        source.source_set_id == source_set_id
                            && source.channel == SourceChannel::BaseColor
                    })
                    .ok_or(StoreError::BaseColorRequired)?;
                let (rebound, _) = rebind_source_frame(
                    &next,
                    OrientedPixelSize {
                        width: source.input.width,
                        height: source.input.height,
                    },
                    owner.source_revision,
                )?;
                next = rebound;
                changed = true;
            }
        }
        if !changed {
            return Ok(false);
        }
        if next.document_revision == current.document_revision {
            next.document_revision = current.document_revision.saturating_add(1);
            next.appearance_revision = next.document_revision;
            next.validate()
                .map_err(|error| StoreError::Document(error.to_string()))?;
        }
        persist_document_state(&mut self.connection, Some(&next), "refresh_document_assets")?;
        self.document = Some(next);
        Ok(true)
    }

    pub fn undo_document_command(&mut self) -> Result<&TrimSheetDocument, StoreError> {
        if self.document_history_cursor == 0 {
            return Err(StoreError::Document("nothing to undo".into()));
        }
        let current = self
            .document
            .clone()
            .ok_or_else(|| StoreError::Document("create a trim sheet first".into()))?;
        self.document_history_cursor -= 1;
        let mut previous = self.document_history[self.document_history_cursor].clone();
        previous.document_revision = current.document_revision.saturating_add(1);
        previous.appearance_revision = previous.document_revision;
        if self.document_history_cursor + 1 == self.document_history.len() {
            self.document_history.push(current);
        }
        persist_document_state(
            &mut self.connection,
            Some(&previous),
            "undo_document_command",
        )?;
        self.document = Some(previous);
        Ok(self.document.as_ref().unwrap())
    }

    pub fn redo_document_command(&mut self) -> Result<&TrimSheetDocument, StoreError> {
        let next_index = self.document_history_cursor + 1;
        if next_index >= self.document_history.len() {
            return Err(StoreError::Document("nothing to redo".into()));
        }
        let mut next = self.document_history[next_index].clone();
        let current_revision = self
            .document
            .as_ref()
            .map_or(0, |document| document.document_revision);
        next.document_revision = current_revision.saturating_add(1);
        next.appearance_revision = next.document_revision;
        self.document_history_cursor = next_index;
        persist_document_state(&mut self.connection, Some(&next), "redo_document_command")?;
        self.document = Some(next);
        Ok(self.document.as_ref().unwrap())
    }

    pub fn record_compiled_artifact(
        &mut self,
        revision: u64,
        topology_hash: &str,
        appearance_hash: &str,
        renderer_version: &str,
    ) -> Result<(), StoreError> {
        let now = unix_timestamp()?;
        self.connection.execute(
            "INSERT INTO compiled_artifact_metadata (singleton, document_revision, topology_hash, appearance_hash, renderer_version, compiled_unix)
             VALUES (1, ?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(singleton) DO UPDATE SET document_revision=excluded.document_revision, topology_hash=excluded.topology_hash,
             appearance_hash=excluded.appearance_hash, renderer_version=excluded.renderer_version, compiled_unix=excluded.compiled_unix",
            params![i64::try_from(revision).unwrap_or(i64::MAX), topology_hash, appearance_hash, renderer_version, now],
        )?;
        Ok(())
    }

    #[must_use]
    pub fn can_undo_patch_command(&self) -> bool {
        self.history_cursor > 0
    }

    #[must_use]
    pub fn can_redo_patch_command(&self) -> bool {
        self.history_cursor < self.history.len()
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
        let before = self.patch_set.patches().to_vec();
        let mut next = PatchSet::new(before.clone())?;
        let outcome = next.execute(command.clone(), None)?;
        validate_patch_geometries(next.patches())?;
        let payload = serde_json::to_string(command)?;
        persist_patch_state(
            &mut self.connection,
            next.patches(),
            "patch_command",
            &payload,
        )?;
        let after = next.patches().to_vec();
        self.patch_set = next;
        if before != after {
            self.push_history(ProjectHistoryEntry {
                before_patches: before,
                after_patches: after,
                coalescing_group,
            });
        }
        checkpoint(&self.connection)?;
        Ok(PatchEditOutcome {
            can_undo: self.can_undo_patch_command(),
            can_redo: self.can_redo_patch_command(),
            ..outcome
        })
    }

    /// Undoes the latest coalesced patch command and persists the restored state.
    ///
    /// # Errors
    ///
    /// Returns a typed failure without changing state when history is empty or persistence fails.
    pub fn undo_patch_command(&mut self) -> Result<PatchEditOutcome, StoreError> {
        if self.history_cursor == 0 {
            return Err(StoreError::PatchCommand(PatchCommandError::NothingToUndo));
        }
        let entry = self.history[self.history_cursor - 1].clone();
        self.restore_patch_history(&entry, false)?;
        self.history_cursor -= 1;
        Ok(self.patch_history_outcome(&entry))
    }

    /// Redoes the next patch command and persists the restored state.
    ///
    /// # Errors
    ///
    /// Returns a typed failure without changing state when redo history is empty or persistence fails.
    pub fn redo_patch_command(&mut self) -> Result<PatchEditOutcome, StoreError> {
        let entry = self
            .history
            .get(self.history_cursor)
            .cloned()
            .ok_or(StoreError::PatchCommand(PatchCommandError::NothingToRedo))?;
        self.restore_patch_history(&entry, true)?;
        self.history_cursor += 1;
        Ok(self.patch_history_outcome(&entry))
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
        self.document = load_document(&self.connection)?;
        self.clear_history();
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
            patch_set: PatchSet::new(self.patch_set.patches().to_vec())?,
            document: self.document.clone(),
            history: Vec::new(),
            history_cursor: 0,
            document_history: Vec::new(),
            document_history_cursor: 0,
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

    fn default_source_set_id(&self) -> Result<Uuid, StoreError> {
        let value: String = self.connection.query_row(
            "SELECT id FROM source_sets ORDER BY ordinal LIMIT 1",
            [],
            |row| row.get(0),
        )?;
        Uuid::parse_str(&value).map_err(|_| StoreError::InvalidId(value))
    }

    fn validate_registration(
        &self,
        source_set_id: Uuid,
        channel: SourceChannel,
        width: u32,
        height: u32,
        orientation: u16,
    ) -> Result<(), StoreError> {
        let base_dimensions = self
            .connection
            .query_row(
                "SELECT width, height, exif_orientation FROM sources
                 WHERE source_set_id = ?1 AND channel = 'base_color'",
                [source_set_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, u32>(0)?,
                        row.get::<_, u32>(1)?,
                        row.get::<_, u16>(2)?,
                    ))
                },
            )
            .optional()?;
        if channel == SourceChannel::BaseColor {
            let mismatch: Option<(String, u32, u32, u16)> = self
                .connection
                .query_row(
                    "SELECT channel, width, height, exif_orientation FROM sources
                     WHERE source_set_id = ?1 AND channel <> 'base_color'
                       AND (width <> ?2 OR height <> ?3 OR exif_orientation <> ?4) LIMIT 1",
                    params![source_set_id.to_string(), width, height, orientation],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .optional()?;
            if let Some((mismatch, companion_width, companion_height, companion_orientation)) =
                mismatch
            {
                if companion_orientation != orientation {
                    return Err(StoreError::OrientationMismatch {
                        channel: SourceChannel::from_db_value(&mismatch)?.as_db_value(),
                        expected_orientation: companion_orientation,
                        actual_orientation: orientation,
                    });
                }
                return Err(StoreError::RegistrationMismatch {
                    channel: SourceChannel::from_db_value(&mismatch)?.as_db_value(),
                    expected_width: companion_width,
                    expected_height: companion_height,
                    actual_width: width,
                    actual_height: height,
                });
            }
            return Ok(());
        }
        let Some((expected_width, expected_height, expected_orientation)) = base_dimensions else {
            return Err(StoreError::BaseColorRequired);
        };
        if orientation != expected_orientation {
            return Err(StoreError::OrientationMismatch {
                channel: channel.as_db_value(),
                expected_orientation,
                actual_orientation: orientation,
            });
        }
        if (width, height) != (expected_width, expected_height) {
            return Err(StoreError::RegistrationMismatch {
                channel: channel.as_db_value(),
                expected_width,
                expected_height,
                actual_width: width,
                actual_height: height,
            });
        }
        Ok(())
    }

    fn push_history(&mut self, entry: ProjectHistoryEntry) {
        if self.history_cursor < self.history.len() {
            self.history.truncate(self.history_cursor);
        }
        let coalesces = entry.coalescing_group.is_some()
            && self
                .history
                .last()
                .is_some_and(|previous| previous.coalescing_group == entry.coalescing_group);
        if coalesces {
            if let Some(previous) = self.history.last_mut() {
                previous.after_patches = entry.after_patches;
            }
        } else {
            self.history.push(entry);
            if self.history.len() > MAX_PROJECT_HISTORY {
                self.history.remove(0);
            }
        }
        self.history_cursor = self.history.len();
    }

    fn clear_history(&mut self) {
        self.history.clear();
        self.history_cursor = 0;
    }

    fn restore_patch_history(
        &mut self,
        entry: &ProjectHistoryEntry,
        use_after: bool,
    ) -> Result<(), StoreError> {
        let patches = if use_after {
            &entry.after_patches
        } else {
            &entry.before_patches
        };
        let operation = if use_after {
            "patch_redo"
        } else {
            "patch_undo"
        };
        persist_patch_state(&mut self.connection, patches, operation, "{}")?;
        self.patch_set = PatchSet::new(patches.clone())?;
        checkpoint(&self.connection)
    }

    fn patch_history_outcome(&self, entry: &ProjectHistoryEntry) -> PatchEditOutcome {
        let invalidated_patch_ids = entry
            .before_patches
            .iter()
            .chain(&entry.after_patches)
            .map(|patch| patch.id)
            .collect();
        PatchEditOutcome {
            invalidated_patch_ids,
            dirty: true,
            can_undo: self.can_undo_patch_command(),
            can_redo: self.can_redo_patch_command(),
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
    let sources = load_sources(connection)?;
    let source_sets = load_source_sets(connection)?;
    let patches = load_patches(connection)?;
    let document = load_document(connection)?;
    let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let legacy_layout_discarded = if version >= 10 {
        connection
            .query_row(
                "SELECT legacy_layout_discarded FROM project_cutover WHERE singleton = 1",
                [],
                |row| row.get::<_, bool>(0),
            )
            .optional()?
            .unwrap_or(false)
    } else {
        false
    };
    Ok(ProjectSummary {
        id,
        name,
        path: path.to_path_buf(),
        sources,
        source_sets,
        patches,
        document,
        legacy_layout_discarded,
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
        patches.push(patch);
    }
    validate_patch_geometries(&patches)?;
    PatchSet::new(patches.clone())?;
    Ok(patches)
}

fn validate_patch_geometries(patches: &[Patch]) -> Result<(), StoreError> {
    for patch in patches {
        Quadrilateral::new(patch.geometry.corners).map_err(|error| {
            StoreError::InvalidData(format!("invalid patch {} geometry: {error}", patch.id))
        })?;
    }
    Ok(())
}

fn load_source_sets(connection: &Connection) -> Result<Vec<SourceSetSnapshot>, StoreError> {
    let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version < 6 {
        let value: String =
            connection.query_row("SELECT id FROM project LIMIT 1", [], |row| row.get(0))?;
        return Ok(vec![SourceSetSnapshot {
            id: value.parse().map_err(|_| StoreError::InvalidId(value))?,
            name: "Material 1".into(),
            exemplar_group: None,
            source_revision: 0,
            registration_digest: ContentDigest::sha256(b"legacy-source-set"),
            delighting: DelightingIntent::default(),
            classification: MaterialClassificationIntent::default(),
            calibration: MaterialCalibrationIntent::default(),
        }]);
    }
    let mut statement = connection.prepare(
        "SELECT id, name, exemplar_group, source_revision, registration_digest, delighting_json, classification_json, calibration_json
         FROM source_sets ORDER BY ordinal",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, u64>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
        ))
    })?;
    rows.map(|row| {
        let (
            id,
            name,
            exemplar_group,
            source_revision,
            registration_digest,
            delighting_json,
            classification_json,
            calibration_json,
        ) = row?;
        Ok(SourceSetSnapshot {
            id: id.parse().map_err(|_| StoreError::InvalidId(id))?,
            name,
            exemplar_group,
            source_revision,
            registration_digest: ContentDigest(registration_digest),
            delighting: serde_json::from_str(&delighting_json)?,
            classification: serde_json::from_str(&classification_json)?,
            calibration: serde_json::from_str(&calibration_json)?,
        })
    })
    .collect()
}

fn load_material_catalog(connection: &Connection) -> Result<Vec<MaterialSourceSet>, StoreError> {
    let sets = load_source_sets(connection)?;
    let sources = load_sources(connection)?;
    Ok(sets
        .into_iter()
        .filter_map(|set| {
            let maps: Vec<_> = sources
                .iter()
                .filter(|source| {
                    SourceSetId::from_bytes(*source.source_set_id.as_bytes()) == set.id
                })
                .map(|source| MaterialMapContent {
                    kind: material_map_kind(source.channel),
                    sha256: source.input.sha256.clone(),
                })
                .collect();
            (!maps.is_empty()).then_some(MaterialSourceSet {
                id: set.id,
                name: set.name,
                maps,
            })
        })
        .collect())
}

const fn material_map_kind(channel: SourceChannel) -> MaterialMapKind {
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

fn load_document(connection: &Connection) -> Result<Option<TrimSheetDocument>, StoreError> {
    let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version < 10 {
        return Ok(None);
    }
    let json = connection
        .query_row(
            "SELECT document_json FROM trim_sheet_documents WHERE singleton = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    json.map(|json| {
        let document: TrimSheetDocument = serde_json::from_str(&json)?;
        document
            .validate()
            .map_err(|error| StoreError::Document(error.to_string()))?;
        Ok(document)
    })
    .transpose()
}

fn snapshot_legacy_authored_layout(document: &mut TrimSheetDocument) -> Result<bool, StoreError> {
    if document.authored_layout_preset.is_some()
        || document.source_frame.is_none()
        || document.partition_provenance.is_none()
    {
        return Ok(false);
    }
    let grid = document.logical_grid.ok_or_else(|| {
        StoreError::Document(
            "accepted SourceFrame topology cannot migrate because its logical grid is missing"
                .into(),
        )
    })?;
    let regions = document.topology.regions.iter().map(|region| {
        Ok(AuthoredLayoutPresetRegion {
            preset_region_key: format!("migrated-{}", region.id),
            display_name: region.display_name.clone(),
            grid_rect: region.grid_rect.ok_or_else(|| StoreError::Document(format!(
                "accepted SourceFrame region {} cannot migrate because its GridRect is missing", region.id,
            )))?,
            role: region.role,
            orientation: region.orientation,
            uv_fit: region.uv_fit.clone(),
            structural_profile: region.structural_profile,
            default_behavior: document
                .region_bindings
                .get(&region.id)
                .map(|binding| binding.mapping.behavior.clone())
                .unwrap_or_default(),
        })
    }).collect::<Result<Vec<_>, StoreError>>()?;
    let preset = AuthoredLayoutPreset {
        preset_id: format!("embedded.migrated.{}", document.id),
        schema_version: hot_trimmer_domain::AUTHORED_LAYOUT_PRESET_SCHEMA_VERSION,
        name: "Migrated accepted layout".into(),
        logical_grid: grid,
        canonical_aspect: [
            document.render_settings.output_size.width,
            document.render_settings.output_size.height,
        ],
        regions,
        provenance: "migrated_accepted_topology_without_regeneration".into(),
    };
    hot_trimmer_domain::validate_authored_layout_preset(&preset).map_err(|reason| {
        StoreError::Document(format!("accepted topology cannot migrate: {reason}"))
    })?;
    document.authored_layout_preset = Some(preset);
    document
        .authored_layout_instance_id
        .get_or_insert_with(|| document.id.to_string());
    document
        .validate()
        .map_err(|reason| StoreError::Document(reason.to_string()))?;
    Ok(true)
}

fn load_sources(connection: &Connection) -> Result<Vec<StoredSource>, StoreError> {
    let sql = "SELECT sources.id, sources.source_set_id, sources.channel, sources.ownership,
                sources.external_path, sources.sha256, sources.width, sources.height, sources.format, sources.color_type,
                has_alpha, exif_orientation, has_icc_profile, encoded_bytes, owned_bytes, origin_path,
                interpretation, normal_convention, assignment_provenance, confidence_milli
         FROM sources JOIN source_sets ON source_sets.id = sources.source_set_id
         ORDER BY source_sets.ordinal, CASE channel
            WHEN 'base_color' THEN 0 WHEN 'normal' THEN 1 WHEN 'height' THEN 2
            WHEN 'roughness' THEN 3 WHEN 'metallic' THEN 4 WHEN 'ambient_occlusion' THEN 5
            WHEN 'specular' THEN 6 WHEN 'opacity' THEN 7 WHEN 'edge_mask' THEN 8 ELSE 9 END";
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, u32>(6)?,
            row.get::<_, u32>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, String>(9)?,
            row.get::<_, bool>(10)?,
            row.get::<_, u16>(11)?,
            row.get::<_, bool>(12)?,
            row.get::<_, u64>(13)?,
            row.get::<_, Option<Vec<u8>>>(14)?,
            row.get::<_, String>(15)?,
            row.get::<_, String>(16)?,
            row.get::<_, String>(17)?,
            row.get::<_, String>(18)?,
            row.get::<_, u16>(19)?,
        ))
    })?;
    let mut sources = Vec::new();
    for row in rows {
        let (
            id_text,
            source_set_id_text,
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
            interpretation,
            normal_convention,
            assignment_provenance,
            confidence_milli,
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
        let channel = SourceChannel::from_db_value(&channel_text)?;
        let registration = ChannelRegistration {
            role: channel.material_role(),
            interpretation: interpretation_from_db(&interpretation)?,
            normal_convention: normal_convention_from_db(&normal_convention)?,
            assignment_provenance: assignment_provenance_from_db(&assignment_provenance)?,
            confidence_milli,
        };
        validate_channel_registration(channel, &registration)?;
        sources.push(StoredSource {
            source_set_id: Uuid::parse_str(&source_set_id_text)
                .map_err(|_| StoreError::InvalidId(source_set_id_text))?,
            channel,
            registration,
            input,
        });
    }
    Ok(sources)
}

fn configure(connection: &Connection) -> Result<(), StoreError> {
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "journal_mode", "DELETE")?;
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

fn persist_document_state(
    connection: &mut Connection,
    document: Option<&TrimSheetDocument>,
    operation: &str,
) -> Result<(), StoreError> {
    let now = unix_timestamp()?;
    let transaction = connection.transaction()?;
    persist_document_state_in_transaction(&transaction, document, operation, now)?;
    transaction.commit()?;
    checkpoint(connection)
}

fn persist_document_state_in_transaction(
    transaction: &Transaction<'_>,
    document: Option<&TrimSheetDocument>,
    operation: &str,
    now: i64,
) -> Result<(), StoreError> {
    transaction.execute("DELETE FROM region_mapping_recipes", [])?;
    transaction.execute("DELETE FROM region_bindings", [])?;
    transaction.execute("DELETE FROM topology_regions", [])?;
    transaction.execute("DELETE FROM accepted_topology_snapshots", [])?;
    transaction.execute("DELETE FROM trim_sheet_documents", [])?;
    if let Some(document) = document {
        let document_json = serde_json::to_string(document)?;
        transaction.execute(
            "INSERT INTO trim_sheet_documents (singleton, id, document_revision, topology_revision, appearance_revision, document_json)
             VALUES (1, ?1, ?2, ?3, ?4, ?5)",
            params![
                document.id.to_string(),
                i64::try_from(document.document_revision).unwrap_or(i64::MAX),
                i64::try_from(document.topology_revision).unwrap_or(i64::MAX),
                i64::try_from(document.appearance_revision).unwrap_or(i64::MAX),
                document_json,
            ],
        )?;
        transaction.execute(
            "INSERT INTO accepted_topology_snapshots (document_id, topology_hash, compatibility_key, snapshot_json)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                document.id.to_string(),
                hash_hex(document.topology.topology_hash),
                document.topology.compatibility_key,
                serde_json::to_string(&document.topology.snapshot)?,
            ],
        )?;
        for (ordinal, region) in document.topology.regions.iter().enumerate() {
            transaction.execute(
                "INSERT INTO topology_regions (id, document_id, ordinal, region_json) VALUES (?1, ?2, ?3, ?4)",
                params![region.id.to_string(), document.id.to_string(), i64::try_from(ordinal).unwrap_or(i64::MAX), serde_json::to_string(region)?],
            )?;
            let binding = document
                .region_bindings
                .get(&region.id)
                .expect("validated binding");
            transaction.execute(
                "INSERT INTO region_bindings (region_id, document_id, binding_json) VALUES (?1, ?2, ?3)",
                params![region.id.to_string(), document.id.to_string(), serde_json::to_string(binding)?],
            )?;
            transaction.execute(
                "INSERT INTO region_mapping_recipes (region_id, document_id, mapping_json) VALUES (?1, ?2, ?3)",
                params![region.id.to_string(), document.id.to_string(), serde_json::to_string(&binding.mapping)?],
            )?;
        }
        transaction.execute(
            "INSERT INTO document_journal (occurred_unix, operation, document_revision, document_json) VALUES (?1, ?2, ?3, ?4)",
            params![now, operation, i64::try_from(document.document_revision).unwrap_or(i64::MAX), serde_json::to_string(document)?],
        )?;
    }
    transaction.execute(
        "INSERT INTO autosave_journal (occurred_unix, operation, payload_json) VALUES (?1, ?2, ?3)",
        params![
            now,
            operation,
            document.map_or_else(
                || "null".into(),
                |document| serde_json::to_string(&document.id).expect("stable ID serializes")
            )
        ],
    )?;
    transaction.execute("UPDATE project SET modified_unix = ?1", [now])?;
    Ok(())
}

fn document_operation(command: &TrimSheetDocumentCommand) -> &'static str {
    match command {
        TrimSheetDocumentCommand::ApplyAuthoredLayoutPreset { .. } => {
            "apply_authored_layout_preset"
        }
        TrimSheetDocumentCommand::SetAuthoredLayoutPresetSnapshot { .. } => {
            "set_authored_layout_preset_snapshot"
        }
        TrimSheetDocumentCommand::AcceptSourceFramePartition { .. } => {
            "accept_source_frame_partition"
        }
        TrimSheetDocumentCommand::SplitSourceFrameRegion { .. } => "split_source_frame_region",
        TrimSheetDocumentCommand::MergeSourceFrameRegions { .. } => "merge_source_frame_regions",
        TrimSheetDocumentCommand::MoveSourceFrameBoundary { .. } => "move_source_frame_boundary",
        TrimSheetDocumentCommand::DrawSourceFrameRegion { .. } => "draw_source_frame_region",
        TrimSheetDocumentCommand::ResizeSourceFrameRegion { .. } => "resize_source_frame_region",
        TrimSheetDocumentCommand::SetPrimaryMaterial { .. } => "set_primary_material",
        TrimSheetDocumentCommand::SetRegionContent { .. } => "set_region_content",
        TrimSheetDocumentCommand::SetRegionAddressMode { .. } => "set_region_address_mode",
        TrimSheetDocumentCommand::SetRegionBehavior { .. } => "set_region_behavior",
        TrimSheetDocumentCommand::SetRegionStructuralProfile { .. } => "set_region_structural_profile",
        TrimSheetDocumentCommand::SetFeedbackProfile { .. } => "set_feedback_profile",
        TrimSheetDocumentCommand::SetEdgeWearIntent { .. } => "set_edge_wear_intent",
        TrimSheetDocumentCommand::UpsertDecoration { .. } => "upsert_decoration",
        TrimSheetDocumentCommand::DeleteDecoration { .. } => "delete_decoration",
        TrimSheetDocumentCommand::ReplaceDecoration { .. } => "replace_decoration",
        TrimSheetDocumentCommand::ReorderDecorations { .. } => "reorder_decorations",
        TrimSheetDocumentCommand::SetSheetFraming { .. } => "set_sheet_framing",
        TrimSheetDocumentCommand::SetRegionProjection { .. } => "set_region_projection",
        TrimSheetDocumentCommand::SetRegionRadial { .. } => "set_region_radial",
        TrimSheetDocumentCommand::SetOutputResolution { .. } => "set_output_resolution",
        TrimSheetDocumentCommand::SetAtlasPadding { .. } => "set_atlas_padding",
        TrimSheetDocumentCommand::SetChannelRenderPolicy { .. } => "set_channel_render_policy",
        TrimSheetDocumentCommand::SetSourceFrame { .. } => "set_source_frame",
        TrimSheetDocumentCommand::DetachSourceCell { .. } => "detach_source_cell",
        TrimSheetDocumentCommand::ResetSourceCell { .. } => "reset_source_cell",
    }
}

fn hash_hex(hash: hot_trimmer_domain::DocumentHash) -> String {
    hash.0.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn rebind_source_frame(
    document: &TrimSheetDocument,
    oriented_dimensions: OrientedPixelSize,
    source_revision: u64,
) -> Result<(TrimSheetDocument, String), StoreError> {
    let frame = document
        .source_frame
        .as_ref()
        .ok_or_else(|| StoreError::Document("the current document has no SourceFrame".into()))?;
    let aspect_changed = u64::from(frame.oriented_dimensions.width)
        .saturating_mul(u64::from(oriented_dimensions.height))
        != u64::from(oriented_dimensions.width)
            .saturating_mul(u64::from(frame.oriented_dimensions.height));
    let mut next_frame = if aspect_changed {
        SourceFrame::centered_largest(
            frame.source_set_id,
            oriented_dimensions,
            frame.output_aspect,
            source_revision,
        )
    } else {
        let mut preserved = frame.clone();
        preserved.oriented_dimensions = oriented_dimensions;
        preserved.source_revision = source_revision;
        preserved.identity = preserved.compute_identity();
        preserved
    };
    next_frame.identity = next_frame.compute_identity();
    let mut next = document.clone();
    next.source_frame = Some(next_frame);
    let mut transformed_overrides = 0usize;
    if aspect_changed {
        let override_ids = next.source_overrides.keys().copied().collect::<Vec<_>>();
        for region_id in override_ids {
            let Some(region) = next
                .topology
                .regions
                .iter()
                .find(|region| region.id == region_id)
            else {
                return Err(StoreError::Document(format!(
                    "detached SourceFrame region {region_id} no longer exists"
                )));
            };
            let Some(override_value) = next.source_overrides.get(&region_id).copied() else {
                continue;
            };
            let bounds = rebind_override_bounds(
                override_value.source_bounds,
                region.allocation_rect.width,
                region.allocation_rect.height,
                oriented_dimensions,
            )?;
            next.source_overrides
                .insert(region_id, RegionSourceOverride::new(bounds));
            let binding = next.region_bindings.get_mut(&region_id).ok_or_else(|| {
                StoreError::Document(format!(
                    "detached SourceFrame region {region_id} has no binding"
                ))
            })?;
            let Projection::Crop {
                bounds: binding_bounds,
                focus,
            } = &mut binding.mapping.projection
            else {
                return Err(StoreError::Document(format!(
                    "detached SourceFrame region {region_id} has no crop projection"
                )));
            };
            *binding_bounds = bounds;
            *focus = NormalizedPoint::new(
                bounds.x.get() + bounds.width.get() * 0.5,
                bounds.y.get() + bounds.height.get() * 0.5,
            )
            .map_err(|_| {
                StoreError::Document(format!(
                    "detached SourceFrame region {region_id} produced invalid focus"
                ))
            })?;
            transformed_overrides += 1;
        }
    }
    next.document_revision = document.document_revision.saturating_add(1);
    next.appearance_revision = next.document_revision;
    next.validate()
        .map_err(|error| StoreError::Document(format!("SourceFrame rebind is invalid: {error}")))?;
    let diagnostic = if document.source_overrides.is_empty() {
        format!(
            "rebound SourceFrame to {}x{} at source revision {}{}",
            oriented_dimensions.width,
            oriented_dimensions.height,
            source_revision,
            if aspect_changed {
                " and refit Largest Fit"
            } else {
                ""
            }
        )
    } else if transformed_overrides > 0 {
        format!(
            "rebound SourceFrame to {}x{} at source revision {}; transformed {} detached override(s) to preserve destination aspect{}",
            oriented_dimensions.width,
            oriented_dimensions.height,
            source_revision,
            transformed_overrides,
            if aspect_changed {
                " and refit Largest Fit"
            } else {
                ""
            }
        )
    } else {
        format!(
            "rebound SourceFrame to {}x{} at source revision {}; preserved {} detached override(s) in normalized source space{}",
            oriented_dimensions.width,
            oriented_dimensions.height,
            source_revision,
            document.source_overrides.len(),
            if aspect_changed {
                " and refit Largest Fit"
            } else {
                ""
            }
        )
    };
    Ok((next, diagnostic))
}

fn rebind_override_bounds(
    bounds: NormalizedBounds,
    destination_width: u32,
    destination_height: u32,
    source_dimensions: OrientedPixelSize,
) -> Result<NormalizedBounds, StoreError> {
    if destination_width == 0
        || destination_height == 0
        || source_dimensions.width == 0
        || source_dimensions.height == 0
    {
        return Err(StoreError::Document(
            "detached SourceFrame region has invalid dimensions".into(),
        ));
    }
    let target_aspect = (f64::from(destination_width) / f64::from(destination_height))
        * f64::from(source_dimensions.height)
        / f64::from(source_dimensions.width);
    let width = bounds
        .width
        .get()
        .min(bounds.height.get() * target_aspect)
        .max(0.001)
        .min(1.0);
    let height = (width / target_aspect).min(1.0);
    let center_x = bounds.x.get() + bounds.width.get() * 0.5;
    let center_y = bounds.y.get() + bounds.height.get() * 0.5;
    let x = (center_x - width * 0.5).clamp(0.0, 1.0 - width);
    let y = (center_y - height * 0.5).clamp(0.0, 1.0 - height);
    Ok(NormalizedBounds {
        x: NormalizedScalar::new(x)
            .map_err(|_| StoreError::Document("detached override x is invalid".into()))?,
        y: NormalizedScalar::new(y)
            .map_err(|_| StoreError::Document("detached override y is invalid".into()))?,
        width: NormalizedScalar::new(width)
            .map_err(|_| StoreError::Document("detached override width is invalid".into()))?,
        height: NormalizedScalar::new(height)
            .map_err(|_| StoreError::Document("detached override height is invalid".into()))?,
    })
}

fn migrate(connection: &mut Connection) -> Result<(), StoreError> {
    let mut version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > CURRENT_SCHEMA_VERSION {
        return Err(StoreError::NewerSchema {
            found: version,
            supported: CURRENT_SCHEMA_VERSION,
        });
    }
    if version != 0 && version < CURRENT_SCHEMA_VERSION {
        return Err(StoreError::InvalidData(format!(
            "legacy project schema {version} is not supported by the Stage 1 source-contract cutover; create a new project"
        )));
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
        version = 5;
    }
    if version == 5 {
        let transaction = connection.transaction()?;
        migrate_to_v6(&transaction)?;
        transaction.pragma_update(None, "user_version", 6_u32)?;
        transaction.commit()?;
        version = 6;
    }
    if version == 6 {
        let transaction = connection.transaction()?;
        migrate_to_v7(&transaction)?;
        transaction.pragma_update(None, "user_version", 7_u32)?;
        transaction.commit()?;
        version = 7;
    }
    if version == 7 {
        let transaction = connection.transaction()?;
        migrate_to_v8(&transaction)?;
        transaction.pragma_update(None, "user_version", 8_u32)?;
        transaction.commit()?;
        version = 8;
    }
    if version == 8 {
        let transaction = connection.transaction()?;
        migrate_to_v9(&transaction)?;
        transaction.pragma_update(None, "user_version", 9_u32)?;
        transaction.commit()?;
        version = 9;
    }
    if version == 9 {
        let transaction = connection.transaction()?;
        migrate_to_v10(&transaction)?;
        transaction.pragma_update(None, "user_version", 10_u32)?;
        transaction.commit()?;
        version = 10;
    }
    if version == 10 {
        let transaction = connection.transaction()?;
        migrate_to_v11(&transaction)?;
        transaction.pragma_update(None, "user_version", 11_u32)?;
        transaction.commit()?;
        version = 11;
    }
    if version == 11 {
        let transaction = connection.transaction()?;
        migrate_to_v12(&transaction)?;
        transaction.pragma_update(None, "user_version", 12_u32)?;
        transaction.commit()?;
        version = 12;
    }
    if version == 12 {
        let transaction = connection.transaction()?;
        migrate_to_v13(&transaction)?;
        transaction.pragma_update(None, "user_version", 13_u32)?;
        transaction.commit()?;
        version = 13;
    }
    if version == 13 {
        let transaction = connection.transaction()?;
        migrate_to_v14(&transaction)?;
        transaction.pragma_update(None, "user_version", 14_u32)?;
        transaction.commit()?;
    }
    Ok(())
}

fn migrate_to_v14(transaction: &Transaction<'_>) -> Result<(), StoreError> {
    let default_intent = serde_json::to_string(&MaterialCalibrationIntent::default())?;
    transaction.execute(
        "ALTER TABLE source_sets ADD COLUMN calibration_json TEXT NOT NULL DEFAULT '{}'",
        [],
    )?;
    transaction.execute(
        "UPDATE source_sets SET calibration_json = ?1",
        [default_intent],
    )?;
    Ok(())
}

fn migrate_to_v13(transaction: &Transaction<'_>) -> Result<(), StoreError> {
    let default_intent = serde_json::to_string(&MaterialClassificationIntent::default())?;
    transaction.execute(
        "ALTER TABLE source_sets ADD COLUMN classification_json TEXT NOT NULL DEFAULT '{}'",
        [],
    )?;
    transaction.execute(
        "UPDATE source_sets SET classification_json = ?1",
        [default_intent],
    )?;
    Ok(())
}

fn migrate_to_v12(transaction: &Transaction<'_>) -> Result<(), StoreError> {
    let default_intent = serde_json::to_string(&DelightingIntent::default())?;
    transaction.execute(
        "ALTER TABLE source_sets ADD COLUMN delighting_json TEXT NOT NULL DEFAULT '{}'",
        [],
    )?;
    transaction.execute(
        "UPDATE source_sets SET delighting_json = ?1",
        [default_intent],
    )?;
    Ok(())
}

fn migrate_to_v11(transaction: &Transaction<'_>) -> Result<(), StoreError> {
    transaction.execute_batch(
        "ALTER TABLE source_sets ADD COLUMN exemplar_group TEXT;
         ALTER TABLE source_sets ADD COLUMN source_revision INTEGER NOT NULL DEFAULT 0 CHECK(source_revision >= 0);
         ALTER TABLE source_sets ADD COLUMN registration_digest TEXT NOT NULL DEFAULT '9f64a747e1b97f131fabb6b447296c9b6f0201e79fb3c5356e6c77e89b6a806a' CHECK(length(registration_digest) = 64);
         ALTER TABLE sources ADD COLUMN interpretation TEXT NOT NULL DEFAULT 'color_managed_base_color' CHECK(interpretation IN (
             'color_managed_base_color', 'tangent_space_normal', 'linear_scalar', 'linear_opacity', 'binary_mask', 'categorical_id'
         ));
         ALTER TABLE sources ADD COLUMN normal_convention TEXT NOT NULL DEFAULT 'not_applicable' CHECK(normal_convention IN (
             'not_applicable', 'open_gl', 'direct_x', 'unspecified'
         ));
         ALTER TABLE sources ADD COLUMN assignment_provenance TEXT NOT NULL DEFAULT 'user_assigned' CHECK(assignment_provenance IN (
             'user_assigned', 'filename_suggested', 'embedded_metadata'
         ));
         ALTER TABLE sources ADD COLUMN confidence_milli INTEGER NOT NULL DEFAULT 1000 CHECK(confidence_milli BETWEEN 0 AND 1000);
         CREATE TABLE source_derived_cache (
             source_set_id TEXT NOT NULL,
             registration_digest TEXT NOT NULL CHECK(length(registration_digest) = 64),
             cache_key TEXT NOT NULL,
             PRIMARY KEY(source_set_id, cache_key),
             FOREIGN KEY(source_set_id) REFERENCES source_sets(id) ON DELETE CASCADE
         );",
    )?;
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

fn migrate_to_v6(transaction: &Transaction<'_>) -> Result<(), StoreError> {
    transaction.execute_batch(include_str!(
        "../../../fixtures/projects/migrate-v5-to-v6.sql"
    ))?;
    Ok(())
}

fn migrate_to_v7(transaction: &Transaction<'_>) -> Result<(), StoreError> {
    transaction.execute_batch(include_str!(
        "../../../fixtures/projects/migrate-v6-to-v7.sql"
    ))?;
    Ok(())
}

fn migrate_to_v8(transaction: &Transaction<'_>) -> Result<(), StoreError> {
    transaction.execute_batch(include_str!(
        "../../../fixtures/projects/migrate-v7-to-v8.sql"
    ))?;
    Ok(())
}

fn migrate_to_v9(transaction: &Transaction<'_>) -> Result<(), StoreError> {
    transaction.execute_batch(
        "ALTER TABLE layouts ADD COLUMN source_layers_json TEXT NOT NULL DEFAULT '{}';",
    )?;
    Ok(())
}

fn migrate_to_v10(transaction: &Transaction<'_>) -> Result<(), StoreError> {
    let legacy_layout_discarded: bool =
        transaction.query_row("SELECT EXISTS(SELECT 1 FROM layouts)", [], |row| row.get(0))?;
    transaction.execute_batch(
        "DROP TABLE IF EXISTS layout_regions;
         DROP TABLE IF EXISTS layouts;
         CREATE TABLE project_cutover (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             legacy_layout_discarded INTEGER NOT NULL
         );
         CREATE TABLE trim_sheet_documents (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             id TEXT NOT NULL UNIQUE,
             document_revision INTEGER NOT NULL,
             topology_revision INTEGER NOT NULL,
             appearance_revision INTEGER NOT NULL,
             document_json TEXT NOT NULL
         );
         CREATE TABLE accepted_topology_snapshots (
             document_id TEXT PRIMARY KEY,
             topology_hash TEXT NOT NULL,
             compatibility_key TEXT NOT NULL,
             snapshot_json TEXT NOT NULL
         );
         CREATE TABLE topology_regions (
             id TEXT PRIMARY KEY,
             document_id TEXT NOT NULL,
             ordinal INTEGER NOT NULL,
             region_json TEXT NOT NULL
         );
         CREATE TABLE region_bindings (
             region_id TEXT PRIMARY KEY,
             document_id TEXT NOT NULL,
             binding_json TEXT NOT NULL
         );
         CREATE TABLE region_mapping_recipes (
             region_id TEXT PRIMARY KEY,
             document_id TEXT NOT NULL,
             mapping_json TEXT NOT NULL
         );
         CREATE TABLE document_journal (
             sequence INTEGER PRIMARY KEY AUTOINCREMENT,
             occurred_unix INTEGER NOT NULL,
             operation TEXT NOT NULL,
             document_revision INTEGER NOT NULL,
             document_json TEXT NOT NULL
         );
         CREATE TABLE compiled_artifact_metadata (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             document_revision INTEGER NOT NULL,
             topology_hash TEXT NOT NULL,
             appearance_hash TEXT NOT NULL,
             renderer_version TEXT NOT NULL,
             compiled_unix INTEGER NOT NULL
         );",
    )?;
    transaction.execute(
        "INSERT INTO project_cutover (singleton, legacy_layout_discarded) VALUES (1, ?1)",
        [legacy_layout_discarded],
    )?;
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
    if !valid {
        return Err(StoreError::InvalidData(
            "source bytes do not match the explicit ownership mode".into(),
        ));
    }
    if let Some(bytes) = &source.owned_bytes {
        if u64::try_from(bytes.len()).ok() != Some(source.encoded_bytes) {
            return Err(StoreError::EncodedByteCountMismatch);
        }
        if ContentDigest::sha256(bytes).0 != source.sha256 {
            return Err(StoreError::ImmutableDigestMismatch);
        }
    }
    Ok(())
}

fn verify_external_source(source: &SourceInput, channel: SourceChannel) -> Result<(), StoreError> {
    if source.ownership != SourceOwnership::VerifiedExternalReference {
        return Ok(());
    }
    let path = source.external_path.as_deref().ok_or_else(|| {
        StoreError::InvalidData("verified external source path is missing".into())
    })?;
    let policy = if channel == SourceChannel::BaseColor {
        ColorPolicy::ConvertToSrgb
    } else {
        ColorPolicy::PreserveLinearData
    };
    let inspected = inspect_path_with_policy(path, DecodeLimits::default(), policy)
        .map_err(|failure| StoreError::ExternalSourceVerification(failure.to_string()))?;
    let info = inspected.info;
    let changed = if info.sha256 != source.sha256 {
        Some("sha256")
    } else if (info.width, info.height) != (source.width, source.height) {
        Some("oriented_dimensions")
    } else if info.exif_orientation != source.exif_orientation {
        Some("orientation")
    } else if info.encoded_bytes != source.encoded_bytes {
        Some("encoded_bytes")
    } else if info.format != source.format {
        Some("format")
    } else if info.color_type != source.color_type {
        Some("color_type")
    } else if info.has_alpha != source.has_alpha {
        Some("alpha_metadata")
    } else if info.has_embedded_icc_profile != source.has_embedded_icc_profile {
        Some("icc_metadata")
    } else {
        None
    };
    changed.map_or(Ok(()), |field| {
        Err(StoreError::ExternalSourceChanged { field })
    })
}

fn validate_channel_registration(
    channel: SourceChannel,
    registration: &ChannelRegistration,
) -> Result<(), StoreError> {
    if registration.role != channel.material_role()
        || registration.interpretation != registration.role.required_interpretation()
    {
        return Err(StoreError::ChannelInterpretationMismatch {
            channel: channel.as_db_value(),
        });
    }
    if registration.confidence_milli > 1000 {
        return Err(StoreError::InvalidAssignmentConfidence);
    }
    if channel == SourceChannel::Normal {
        if registration.normal_convention == NormalConvention::NotApplicable {
            return Err(StoreError::ChannelInterpretationMismatch {
                channel: channel.as_db_value(),
            });
        }
    } else if registration.normal_convention != NormalConvention::NotApplicable {
        return Err(StoreError::NormalConventionOnScalar);
    }
    Ok(())
}

const fn interpretation_db(value: ChannelInterpretation) -> &'static str {
    match value {
        ChannelInterpretation::ColorManagedBaseColor => "color_managed_base_color",
        ChannelInterpretation::TangentSpaceNormal => "tangent_space_normal",
        ChannelInterpretation::LinearScalar => "linear_scalar",
        ChannelInterpretation::LinearOpacity => "linear_opacity",
        ChannelInterpretation::BinaryMask => "binary_mask",
        ChannelInterpretation::CategoricalId => "categorical_id",
    }
}

fn interpretation_from_db(value: &str) -> Result<ChannelInterpretation, StoreError> {
    match value {
        "color_managed_base_color" => Ok(ChannelInterpretation::ColorManagedBaseColor),
        "tangent_space_normal" => Ok(ChannelInterpretation::TangentSpaceNormal),
        "linear_scalar" => Ok(ChannelInterpretation::LinearScalar),
        "linear_opacity" => Ok(ChannelInterpretation::LinearOpacity),
        "binary_mask" => Ok(ChannelInterpretation::BinaryMask),
        "categorical_id" => Ok(ChannelInterpretation::CategoricalId),
        _ => Err(StoreError::InvalidData(format!(
            "unknown channel interpretation: {value}"
        ))),
    }
}

const fn normal_convention_db(value: NormalConvention) -> &'static str {
    match value {
        NormalConvention::NotApplicable => "not_applicable",
        NormalConvention::OpenGl => "open_gl",
        NormalConvention::DirectX => "direct_x",
        NormalConvention::Unspecified => "unspecified",
    }
}

fn normal_convention_from_db(value: &str) -> Result<NormalConvention, StoreError> {
    match value {
        "not_applicable" => Ok(NormalConvention::NotApplicable),
        "open_gl" => Ok(NormalConvention::OpenGl),
        "direct_x" => Ok(NormalConvention::DirectX),
        "unspecified" => Ok(NormalConvention::Unspecified),
        _ => Err(StoreError::InvalidData(format!(
            "unknown normal convention: {value}"
        ))),
    }
}

const fn assignment_provenance_db(value: AssignmentProvenance) -> &'static str {
    match value {
        AssignmentProvenance::UserAssigned => "user_assigned",
        AssignmentProvenance::FilenameSuggested => "filename_suggested",
        AssignmentProvenance::EmbeddedMetadata => "embedded_metadata",
    }
}

fn assignment_provenance_from_db(value: &str) -> Result<AssignmentProvenance, StoreError> {
    match value {
        "user_assigned" => Ok(AssignmentProvenance::UserAssigned),
        "filename_suggested" => Ok(AssignmentProvenance::FilenameSuggested),
        "embedded_metadata" => Ok(AssignmentProvenance::EmbeddedMetadata),
        _ => Err(StoreError::InvalidData(format!(
            "unknown assignment provenance: {value}"
        ))),
    }
}

fn advance_source_authority(
    transaction: &Transaction<'_>,
    source_set_id: Uuid,
) -> Result<(), StoreError> {
    let exemplar_group: Option<String> = transaction.query_row(
        "SELECT exemplar_group FROM source_sets WHERE id = ?1",
        [source_set_id.to_string()],
        |row| row.get(0),
    )?;
    let mut statement = transaction.prepare(
        "SELECT id, channel, sha256, width, height, exif_orientation, interpretation,
                normal_convention, assignment_provenance, confidence_milli, origin_path
         FROM sources WHERE source_set_id = ?1 ORDER BY CASE channel
            WHEN 'base_color' THEN 0 WHEN 'normal' THEN 1 WHEN 'height' THEN 2
            WHEN 'roughness' THEN 3 WHEN 'metallic' THEN 4 WHEN 'ambient_occlusion' THEN 5
            WHEN 'specular' THEN 6 WHEN 'opacity' THEN 7 WHEN 'edge_mask' THEN 8 ELSE 9 END",
    )?;
    let rows = statement.query_map([source_set_id.to_string()], |row| {
        Ok(format!(
            "{}|{}|{}|{}x{}|{}|{}|{}|{}|{}|{}",
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, u32>(3)?,
            row.get::<_, u32>(4)?,
            row.get::<_, u16>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, u16>(9)?,
            row.get::<_, String>(10)?,
        ))
    })?;
    let canonical = format!(
        "exemplar_group={};\n{}",
        exemplar_group.as_deref().unwrap_or(""),
        rows.collect::<Result<Vec<_>, _>>()?.join("\n"),
    );
    drop(statement);
    let digest = ContentDigest::sha256(canonical.as_bytes());
    transaction.execute(
        "DELETE FROM source_derived_cache WHERE source_set_id = ?1",
        [source_set_id.to_string()],
    )?;
    transaction.execute(
        "UPDATE source_sets
         SET source_revision = source_revision + 1, registration_digest = ?2
         WHERE id = ?1",
        params![source_set_id.to_string(), digest.0],
    )?;
    Ok(())
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
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
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
    use std::{
        collections::hash_map::DefaultHasher,
        hash::{Hash, Hasher},
    };
    let mut hasher = DefaultHasher::new();
    project_path
        .to_string_lossy()
        .to_lowercase()
        .hash(&mut hasher);
    std::env::temp_dir()
        .join("HotTrimmer")
        .join("locks")
        .join(format!("{:016x}.lock", hasher.finish()))
}

#[cfg(test)]
mod document_tests {
    use std::{fs, path::PathBuf};

    use hot_trimmer_domain::{
        LayoutId, LayoutSettings, NormalizedPoint, Patch, PatchGeometry, PatchId, PatchProperties,
        PixelSize, RectificationSettings, SourceId, TrimSheetDocumentCommand,
    };
    use rusqlite::{Connection, params};
    use uuid::Uuid;

    use super::{ProjectStore, SourceChannel, SourceInput, SourceOwnership, configure, migrate};

    #[test]
    fn trim_sheet_vertical_persists_document_hashes_and_atomic_history() {
        let root = std::env::temp_dir().join(format!("hot-trimmer-document-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("document.hottrimmer");
        let mut store = ProjectStore::create(&path, "Document").unwrap();
        store
            .replace_source(
                SourceChannel::BaseColor,
                &SourceInput {
                    id: SourceId::new(),
                    ownership: SourceOwnership::OwnedCopy,
                    external_path: None,
                    origin_path: PathBuf::from("concrete.png"),
                    sha256: "a".repeat(64),
                    width: 2,
                    height: 2,
                    format: "PNG".into(),
                    color_type: "Rgba8".into(),
                    has_alpha: true,
                    exif_orientation: 1,
                    has_embedded_icc_profile: false,
                    encoded_bytes: 4,
                    owned_bytes: Some(vec![0; 4]),
                },
            )
            .unwrap();
        store
            .create_trim_sheet_document("ht.generic_architecture", "1.0.0")
            .unwrap();
        store
            .execute_document_command(&TrimSheetDocumentCommand::SetOutputResolution {
                output_size: PixelSize {
                    width: 1024,
                    height: 1024,
                },
            })
            .unwrap();
        let expected = store.document().unwrap().clone();
        store.undo_document_command().unwrap();
        assert!(store.document().unwrap().document_revision > expected.document_revision);
        store.redo_document_command().unwrap();
        assert_eq!(
            store.document().unwrap().render_settings.output_size,
            expected.render_settings.output_size
        );
        let expected_hash = store.document().unwrap().appearance_hash().unwrap();
        let expected_inputs = store.document().unwrap().appearance_hash_inputs();
        drop(store);
        let reopened = ProjectStore::open(&path).unwrap();
        assert_eq!(
            reopened.document().unwrap().appearance_hash_inputs(),
            expected_inputs
        );
        assert_eq!(
            reopened.document().unwrap().appearance_hash().unwrap(),
            expected_hash
        );
        drop(reopened);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn trim_sheet_vertical_cutover_discards_layout_and_preserves_source_patch_assets() {
        let root = std::env::temp_dir().join(format!("hot-trimmer-cutover-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("legacy.hottrimmer");
        let mut connection = Connection::open(&path).unwrap();
        configure(&connection).unwrap();
        connection
            .execute_batch(include_str!("../../../fixtures/projects/schema-v7.sql"))
            .unwrap();
        connection
            .pragma_update(None, "user_version", 7_u32)
            .unwrap();
        let project_id = Uuid::new_v4();
        let source_id = SourceId::new();
        connection.execute("INSERT INTO project (id, name, created_unix, modified_unix) VALUES (?1, 'Legacy', 1, 1)", [project_id.to_string()]).unwrap();
        connection
            .execute(
                "INSERT INTO source_sets (id, name, ordinal) VALUES (?1, 'Concrete', 0)",
                [project_id.to_string()],
            )
            .unwrap();
        connection.execute(
            "INSERT INTO sources (id, source_set_id, channel, ownership, external_path, sha256, width, height, format, color_type, has_alpha, exif_orientation, has_icc_profile, encoded_bytes, owned_bytes, origin_path)
             VALUES (?1, ?2, 'base_color', 'owned_copy', NULL, ?3, 2, 2, 'PNG', 'Rgba8', 1, 1, 0, 4, ?4, 'concrete.png')",
            params![source_id.to_string(), project_id.to_string(), "b".repeat(64), vec![1_u8; 4]],
        ).unwrap();
        let patch = Patch {
            id: PatchId::new(),
            source_id,
            name: "Vent".into(),
            enabled: true,
            geometry: PatchGeometry {
                corners: [
                    NormalizedPoint::new(0.1, 0.1).unwrap(),
                    NormalizedPoint::new(0.9, 0.1).unwrap(),
                    NormalizedPoint::new(0.9, 0.9).unwrap(),
                    NormalizedPoint::new(0.1, 0.9).unwrap(),
                ],
                assistance_mask: None,
            },
            properties: PatchProperties::default(),
            rectification: RectificationSettings::default(),
        };
        connection
            .execute(
                "INSERT INTO patches (id, source_id, ordinal, patch_json) VALUES (?1, ?2, 0, ?3)",
                params![
                    patch.id.to_string(),
                    source_id.to_string(),
                    serde_json::to_string(&patch).unwrap()
                ],
            )
            .unwrap();
        connection.execute(
            "INSERT INTO layouts (singleton, id, preset, settings_json, items_json) VALUES (1, ?1, 'balanced', ?2, '[]')",
            params![LayoutId::new().to_string(), serde_json::to_string(&LayoutSettings::default()).unwrap()],
        ).unwrap();
        migrate(&mut connection).unwrap();
        let source_count: u32 = connection
            .query_row("SELECT count(*) FROM sources", [], |row| row.get(0))
            .unwrap();
        let patch_count: u32 = connection
            .query_row("SELECT count(*) FROM patches", [], |row| row.get(0))
            .unwrap();
        let layout_tables: u32 = connection.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('layouts','layout_regions')", [], |row| row.get(0)).unwrap();
        let discarded: bool = connection
            .query_row(
                "SELECT legacy_layout_discarded FROM project_cutover",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            (source_count, patch_count, layout_tables, discarded),
            (1, 1, 0, true)
        );
        drop(connection);
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(all(test, any()))]
mod tests {
    use std::{fs, path::PathBuf};

    use hot_trimmer_domain::{
        AutoPackSettings, FillBehavior, LayoutId, LayoutItem, LayoutOrder, LayoutPreset,
        LayoutRequest, LayoutSettings, NormalizedPoint, PackPriority, Patch, PatchCommand,
        PatchGeometry, PatchId, PatchProperties, PixelBounds, PixelSize, RectificationSettings,
        RegionConstraints, RegionFill, RegionLocks, RegionSourceLayer, SourceId, SourceLayerError,
        SourceMapping, SourceRectification, SourceRectificationMode, SourceSampling, SourceSetId,
        SourceWarp,
    };
    use rusqlite::{Connection, OptionalExtension};
    use uuid::Uuid;

    use super::{
        CURRENT_SCHEMA_VERSION, LayoutCommand, MAX_RECOVERY_SNAPSHOTS, ProjectStore, SourceChannel,
        SourceInput, SourceOwnership, StoreError, lock_path, migrate,
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
        let owned_bytes = vec![1, 2, 3];
        SourceInput {
            id: SourceId::new(),
            ownership: SourceOwnership::OwnedCopy,
            external_path: None,
            origin_path: PathBuf::from("fixture.png"),
            sha256: ContentDigest::sha256(&owned_bytes).0,
            width,
            height,
            format: "PNG".into(),
            color_type: "Rgba8".into(),
            has_alpha: true,
            exif_orientation: 1,
            has_embedded_icc_profile: false,
            encoded_bytes: 3,
            owned_bytes: Some(owned_bytes),
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

    fn source_item(source_set_id: SourceSetId, key: &str) -> LayoutItem {
        LayoutItem {
            key: key.into(),
            fill: RegionFill::WholeSourceSet { source_set_id },
            behavior: FillBehavior::Stretch,
            trim_caps: None,
            natural_size: PixelSize {
                width: 1024,
                height: 512,
            },
            enabled: true,
            participates: true,
            constraints: RegionConstraints::default(),
            padding_px: None,
            bleed_px: None,
            region_id: None,
        }
    }

    fn layout_request(items: Vec<LayoutItem>) -> LayoutRequest {
        LayoutRequest {
            layout_id: LayoutId::new(),
            preset: LayoutPreset::Balanced,
            settings: LayoutSettings {
                output: PixelSize {
                    width: 2048,
                    height: 2048,
                },
                padding_px: 4,
                bleed_px: 8,
                order: LayoutOrder::Input,
                auto_pack: AutoPackSettings {
                    enabled: true,
                    priority: PackPriority::Balanced,
                    seed: 17,
                },
                fixed_selected_size: None,
            },
            items,
            existing_regions: Vec::new(),
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
    fn version_five_fixture_groups_existing_maps_into_the_first_source_set() {
        let fixture = FixtureDir::new();
        let path = fixture.0.join("version-five.hottrimmer");
        let mut connection = Connection::open(&path).expect("create fixture");
        connection
            .execute_batch(include_str!("../../../fixtures/projects/schema-v4.sql"))
            .expect("v4 schema");
        connection
            .execute_batch(include_str!("../../../fixtures/projects/data-v1.sql"))
            .expect("v4 data");
        connection
            .execute_batch(include_str!(
                "../../../fixtures/projects/migrate-v4-to-v5.sql"
            ))
            .expect("v5 schema");
        connection
            .pragma_update(None, "user_version", 5_u32)
            .expect("mark v5");
        migrate(&mut connection).expect("migrate v5");
        let (project_id, source_set_id): (String, String) = connection
            .query_row(
                "SELECT project.id, sources.source_set_id FROM project JOIN sources LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("migrated source set");
        assert_eq!(source_set_id, project_id);
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
                .expect("schema version"),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn version_six_fixture_adds_empty_layout_storage_transactionally() {
        let fixture = FixtureDir::new();
        let path = fixture.0.join("version-six.hottrimmer");
        let mut connection = Connection::open(&path).expect("create fixture");
        connection
            .execute_batch(include_str!("../../../fixtures/projects/schema-v4.sql"))
            .expect("v4 schema");
        connection
            .execute_batch(include_str!("../../../fixtures/projects/data-v1.sql"))
            .expect("v4 data");
        connection
            .execute_batch(include_str!(
                "../../../fixtures/projects/migrate-v4-to-v5.sql"
            ))
            .expect("v5 schema");
        connection
            .execute_batch(include_str!(
                "../../../fixtures/projects/migrate-v5-to-v6.sql"
            ))
            .expect("v6 schema");
        connection
            .pragma_update(None, "user_version", 6_u32)
            .expect("mark v6");
        migrate(&mut connection).expect("migrate v6");
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
                .expect("schema version"),
            CURRENT_SCHEMA_VERSION
        );
        let layout_count: u32 = connection
            .query_row("SELECT count(*) FROM layouts", [], |row| row.get(0))
            .expect("layout count");
        assert_eq!(layout_count, 0);
    }

    #[test]
    fn version_seven_layout_migrates_to_custom_atlas_without_discarding_rows() {
        let fixture = FixtureDir::new();
        let path = fixture.0.join("version-seven.hottrimmer");
        let mut connection = Connection::open(&path).expect("create fixture");
        connection
            .execute_batch(include_str!("../../../fixtures/projects/schema-v7.sql"))
            .expect("v7 schema");
        connection
            .execute(
                "INSERT INTO layouts (singleton, id, preset, settings_json, items_json)
                 VALUES (1, ?1, ?2, ?3, ?4)",
                [
                    "00000000-0000-4000-8000-000000000007",
                    "atlas",
                    "{\"output\":{\"width\":256,\"height\":256}}",
                    "[]",
                ],
            )
            .expect("legacy layout");
        connection
            .pragma_update(None, "user_version", 7_u32)
            .expect("mark v7");

        migrate(&mut connection).expect("migrate v7");

        let row: (String, String, String, String, Option<String>) = connection
            .query_row(
                "SELECT id, preset, settings_json, items_json, template_json FROM layouts",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .expect("migrated layout");
        let kind: String = connection
            .query_row("SELECT layout_kind FROM layouts", [], |row| row.get(0))
            .expect("layout kind");
        assert_eq!(row.0, "00000000-0000-4000-8000-000000000007");
        assert_eq!(row.1, "atlas");
        assert_eq!(row.2, "{\"output\":{\"width\":256,\"height\":256}}");
        assert_eq!(row.3, "[]");
        assert_eq!(kind, "custom_atlas");
        assert_eq!(row.4, None);
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
                .expect("schema version"),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn version_eight_layout_storage_migrates_source_layers_transactionally() {
        let fixture = FixtureDir::new();
        let path = fixture.0.join("version-eight.hottrimmer");
        let mut connection = Connection::open(&path).expect("create fixture");
        connection
            .execute_batch(include_str!("../../../fixtures/projects/schema-v7.sql"))
            .expect("v7 schema");
        connection
            .execute_batch(include_str!(
                "../../../fixtures/projects/migrate-v7-to-v8.sql"
            ))
            .expect("v8 schema");
        connection
            .pragma_update(None, "user_version", 8_u32)
            .expect("mark v8");

        migrate(&mut connection).expect("migrate v8");

        let source_layers: String = connection
            .query_row(
                "SELECT source_layers_json FROM layouts LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .expect("source-layer column")
            .unwrap_or_else(|| "{}".into());
        assert_eq!(source_layers, "{}");
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
                .expect("schema version"),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn source_layer_edits_round_trip_without_changing_region_topology_and_reject_singular_quads() {
        let fixture = FixtureDir::new();
        let path = fixture.0.join("source-layer.hottrimmer");
        let mut store = ProjectStore::create(&path, "Source layer").expect("create project");
        let source_set = store.summary().expect("summary").source_sets[0].id;
        store
            .replace_source(SourceChannel::BaseColor, &source(64, 64))
            .expect("base source");
        store
            .solve_and_commit_layout(
                &layout_request(vec![source_item(source_set, "source:brick")]),
                None,
            )
            .expect("layout");
        let original = store.layout().expect("layout").layout.regions[0].clone();
        let point = |x, y| NormalizedPoint::new(x, y).expect("point");
        let source_layer = RegionSourceLayer {
            mapping: SourceMapping::Perspective {
                quad: [
                    point(0.1, 0.1),
                    point(0.9, 0.15),
                    point(0.85, 0.9),
                    point(0.15, 0.85),
                ],
            },
            rectification: SourceRectification {
                mode: SourceRectificationMode::Perspective,
                ..SourceRectification::default()
            },
            warps: vec![
                SourceWarp::Planar {
                    scale_x: 1.5,
                    scale_y: 0.75,
                    offset_x: 0.1,
                    offset_y: -0.1,
                },
                SourceWarp::SpiralTwirl {
                    center_x: 0.5,
                    center_y: 0.5,
                    radius: 0.8,
                    strength: 0.25,
                    iterations: 4,
                },
            ],
            ..RegionSourceLayer::default()
        };
        store
            .execute_layout_command(
                &LayoutCommand::SetSourceLayer {
                    region_id: original.id,
                    source_layer: source_layer.clone(),
                },
                None,
            )
            .expect("set source layer");
        let edited = store.layout().expect("edited layout");
        assert_eq!(edited.layout.regions[0], original);
        assert_eq!(edited.source_layer(original.id), source_layer);
        drop(store);

        let mut reopened = ProjectStore::open(&path).expect("reopen project");
        assert_eq!(
            reopened
                .layout()
                .expect("reopened layout")
                .source_layer(original.id),
            source_layer
        );
        let singular = RegionSourceLayer {
            mapping: SourceMapping::Perspective {
                quad: [
                    point(0.2, 0.2),
                    point(0.4, 0.4),
                    point(0.6, 0.6),
                    point(0.8, 0.8),
                ],
            },
            rectification: SourceRectification {
                mode: SourceRectificationMode::Perspective,
                ..SourceRectification::default()
            },
            ..RegionSourceLayer::default()
        };
        assert!(matches!(
            reopened.execute_layout_command(
                &LayoutCommand::SetSourceLayer { region_id: original.id, source_layer: singular },
                None,
            ),
            Err(StoreError::InvalidRegionSourceLayer {
                region_id,
                source: SourceLayerError::SingularPerspective,
            }) if region_id == original.id
        ));
        assert!(matches!(
            reopened.execute_layout_command(
                &LayoutCommand::SetSourceLayer {
                    region_id: original.id,
                    source_layer: RegionSourceLayer {
                        sampling: SourceSampling { scale: f64::NAN, ..SourceSampling::default() },
                        ..RegionSourceLayer::default()
                    },
                },
                None,
            ),
            Err(StoreError::InvalidRegionSourceLayer {
                region_id,
                source: SourceLayerError::SamplingScaleOutOfRange,
            }) if region_id == original.id
        ));
    }
    #[test]
    fn source_only_two_set_layout_reopens_with_explicit_set_names() {
        let fixture = FixtureDir::new();
        let path = fixture.0.join("two-sets.hottrimmer");
        let mut store = ProjectStore::create(&path, "Two sets").expect("create project");
        let first_set = store.summary().expect("summary").source_sets[0].id;
        let second_uuid = Uuid::new_v4();
        let second_set: SourceSetId = second_uuid.to_string().parse().expect("source set id");
        store
            .replace_source_in_set(
                Uuid::parse_str(&first_set.to_string()).expect("uuid"),
                SourceChannel::BaseColor,
                &source(1024, 512),
            )
            .expect("first source");
        store
            .replace_source_in_set(second_uuid, SourceChannel::BaseColor, &source(512, 1024))
            .expect("second source");
        let request = layout_request(vec![
            source_item(first_set, "source:first"),
            source_item(second_set, "source:second"),
        ]);
        store
            .solve_and_commit_layout(&request, None)
            .expect("source-only layout");
        drop(store);

        let reopened = ProjectStore::open(&path).expect("reopen");
        let summary = reopened.summary().expect("summary");
        assert_eq!(summary.source_sets.len(), 2);
        assert_eq!(summary.source_sets[0].name, "Material 1");
        assert_eq!(summary.source_sets[1].id, second_set);
        assert_eq!(summary.layout.expect("layout").layout.regions.len(), 2);
    }

    #[test]
    fn mixed_sets_and_optional_patches_are_one_to_one_in_recovery() {
        let fixture = FixtureDir::new();
        let path = fixture.0.join("mixed.hottrimmer");
        let recovery = fixture.0.join("recovery");
        let mut store = ProjectStore::create(&path, "Mixed").expect("create project");
        let first_set = store.summary().expect("summary").source_sets[0].id;
        let second_uuid = Uuid::new_v4();
        let second_set: SourceSetId = second_uuid.to_string().parse().expect("source set id");
        let first = source(1024, 512);
        let second = source(512, 1024);
        let authored = patch(first.id, "Optional patch");
        store
            .replace_source(SourceChannel::BaseColor, &first)
            .expect("first source");
        store
            .replace_source_in_set(second_uuid, SourceChannel::BaseColor, &second)
            .expect("second source");
        store
            .execute_patch_command(
                &PatchCommand::Create {
                    patch: authored.clone(),
                    index: None,
                },
                None,
            )
            .expect("patch");
        let mut patch_item = source_item(first_set, "patch:optional");
        patch_item.fill = RegionFill::RectifiedPatch {
            source_set_id: first_set,
            patch_id: authored.id,
        };
        let mut simple_item = source_item(first_set, "simple:accent");
        simple_item.fill = RegionFill::SimpleColor {
            rgba: [32, 64, 96, 255],
        };
        simple_item.behavior = FillBehavior::UniqueDetail;
        let request = layout_request(vec![
            source_item(first_set, "source:first"),
            source_item(second_set, "source:second"),
            patch_item,
            simple_item,
        ]);
        store
            .solve_and_commit_layout(&request, None)
            .expect("mixed layout");
        assert_eq!(store.layout().expect("layout").layout.regions.len(), 4);
        assert_eq!(
            store
                .layout()
                .expect("layout")
                .layout
                .regions
                .iter()
                .filter(|region| matches!(region.fill, RegionFill::RectifiedPatch { .. }))
                .count(),
            1
        );
        let simple_id = store
            .layout()
            .expect("layout")
            .layout
            .regions
            .iter()
            .find(|region| region.item_key == "simple:accent")
            .expect("simple region")
            .id;
        store
            .execute_layout_command(
                &LayoutCommand::DeleteSimple {
                    region_id: simple_id,
                },
                None,
            )
            .expect("delete simple");
        assert_eq!(store.layout().expect("layout").layout.regions.len(), 3);
        store.undo_project_command().expect("undo simple delete");
        assert_eq!(store.layout().expect("layout").layout.regions.len(), 4);
        let last_id = store.layout().expect("layout").layout.regions[3].id;
        store
            .execute_layout_command(
                &LayoutCommand::Reorder {
                    region_id: last_id,
                    to_index: 0,
                },
                None,
            )
            .expect("reorder");
        assert_eq!(
            store.layout().expect("layout").layout.regions[0].id,
            last_id
        );
        let snapshot = store
            .create_recovery_snapshot(&recovery)
            .expect("recovery snapshot");
        let inspected = ProjectStore::inspect(&snapshot).expect("inspect recovery");
        assert_eq!(inspected.sources.len(), 2);
        assert_eq!(inspected.patches, vec![authored]);
        assert_eq!(inspected.layout.expect("layout").layout.regions.len(), 4);
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn layout_regeneration_and_project_history_preserve_patch_rows_and_region_identity() {
        let fixture = FixtureDir::new();
        let path = fixture.0.join("layout-history.hottrimmer");
        let mut store = ProjectStore::create(&path, "Layout history").expect("create project");
        let source_set = store.summary().expect("summary").source_sets[0].id;
        let base = source(1024, 512);
        let authored = patch(base.id, "Trim");
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
            .expect("patch");
        let patch_json_before: String = store
            .connection
            .query_row("SELECT patch_json FROM patches", [], |row| row.get(0))
            .expect("patch json");
        let mut item = source_item(source_set, "patch:trim");
        item.fill = RegionFill::RectifiedPatch {
            source_set_id: source_set,
            patch_id: authored.id,
        };
        let request = layout_request(vec![item]);
        let initial = store
            .solve_and_commit_layout(&request, None)
            .expect("initial")
            .clone();
        let mut regeneration = initial.regeneration_request();
        regeneration.preset = LayoutPreset::Atlas;
        let regenerated = store
            .solve_and_commit_layout(&regeneration, None)
            .expect("regenerate")
            .clone();
        assert_eq!(
            initial.layout.regions[0].id,
            regenerated.layout.regions[0].id
        );
        assert_eq!(
            initial.layout.regions[0].id_color,
            regenerated.layout.regions[0].id_color
        );
        let patch_json_after: String = store
            .connection
            .query_row("SELECT patch_json FROM patches", [], |row| row.get(0))
            .expect("patch json");
        assert_eq!(patch_json_before, patch_json_after);

        let region_id = regenerated.layout.regions[0].id;
        let bounds = PixelBounds {
            x: 64,
            y: 64,
            width: 512,
            height: 256,
        };
        let history_before_drag = store.history.len();
        store
            .execute_layout_command(&LayoutCommand::SetBounds { region_id, bounds }, Some(44))
            .expect("manual resize");
        let dragged = PixelBounds {
            x: 96,
            y: 80,
            width: 480,
            height: 240,
        };
        store
            .execute_layout_command(
                &LayoutCommand::SetBounds {
                    region_id,
                    bounds: dragged,
                },
                Some(44),
            )
            .expect("coalesced drag");
        assert_eq!(store.history.len(), history_before_drag + 1);
        store
            .execute_layout_command(
                &LayoutCommand::SetLocks {
                    region_id,
                    locks: RegionLocks {
                        position: true,
                        width: true,
                        height: true,
                    },
                },
                None,
            )
            .expect("lock");
        store.undo_project_command().expect("undo lock");
        assert!(
            !store.layout().expect("layout").layout.regions[0]
                .locks
                .position
        );
        store.redo_project_command().expect("redo lock");
        assert!(
            store.layout().expect("layout").layout.regions[0]
                .locks
                .position
        );
    }

    #[test]
    fn impossible_layout_and_orphaning_patch_edit_do_not_commit() {
        let fixture = FixtureDir::new();
        let path = fixture.0.join("layout-reject.hottrimmer");
        let mut store = ProjectStore::create(&path, "Reject").expect("create project");
        let source_set = store.summary().expect("summary").source_sets[0].id;
        let base = source(64, 64);
        let authored = patch(base.id, "Used");
        store
            .replace_source(SourceChannel::BaseColor, &base)
            .expect("base");
        store
            .execute_patch_command(
                &PatchCommand::Create {
                    patch: authored.clone(),
                    index: None,
                },
                None,
            )
            .expect("patch");
        let mut impossible = source_item(source_set, "too-large");
        impossible.constraints.fixed_width_px = Some(4096);
        let mut request = layout_request(vec![impossible]);
        request.settings.output = PixelSize {
            width: 128,
            height: 128,
        };
        assert!(matches!(
            store.solve_and_commit_layout(&request, None),
            Err(StoreError::LayoutSolve(_))
        ));
        assert!(store.layout().is_none());

        let mut patch_item = source_item(source_set, "patch:used");
        patch_item.fill = RegionFill::RectifiedPatch {
            source_set_id: source_set,
            patch_id: authored.id,
        };
        store
            .solve_and_commit_layout(&layout_request(vec![patch_item]), None)
            .expect("valid layout");
        store
            .execute_patch_command(
                &PatchCommand::SetEnabled {
                    patch_id: authored.id,
                    enabled: false,
                },
                None,
            )
            .expect("disable restores source fallback");
        assert!(!store.patches()[0].enabled);
        assert!(matches!(
            store.layout().expect("layout").items[0].fill,
            RegionFill::WholeSourceSet { .. }
        ));

        store.undo_project_command().expect("undo fallback");
        assert!(store.patches()[0].enabled);
        assert!(matches!(
            store.layout().expect("layout").items[0].fill,
            RegionFill::RectifiedPatch { .. }
        ));
        store.redo_project_command().expect("redo fallback");
        assert!(!store.patches()[0].enabled);
        assert!(matches!(
            store.layout().expect("layout").items[0].fill,
            RegionFill::WholeSourceSet { .. }
        ));
    }

    #[test]
    fn current_schema_validation_rejects_dangling_layout_catalog_references() {
        let fixture = FixtureDir::new();
        let path = fixture.0.join("dangling-layout.hottrimmer");
        let mut store = ProjectStore::create(&path, "Dangling").expect("create project");
        let source_set = store.summary().expect("summary").source_sets[0].id;
        store
            .replace_source(SourceChannel::BaseColor, &source(64, 64))
            .expect("base");
        store
            .solve_and_commit_layout(
                &layout_request(vec![source_item(source_set, "source:valid")]),
                None,
            )
            .expect("layout");
        let mut items = store.layout().expect("layout").items.clone();
        let unknown: SourceSetId = "00000000-0000-4000-8000-000000009999"
            .parse()
            .expect("unknown source set");
        items[0].fill = RegionFill::WholeSourceSet {
            source_set_id: unknown,
        };
        drop(store);

        let connection = Connection::open(&path).expect("tamper fixture");
        connection
            .execute(
                "UPDATE layouts SET items_json = ?1",
                [serde_json::to_string(&items).expect("items json")],
            )
            .expect("tamper catalog reference");
        drop(connection);
        assert!(matches!(
            ProjectStore::inspect(&path),
            Err(StoreError::LayoutReference(_))
        ));
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
    fn invalid_patch_geometry_is_rejected_before_persistence() {
        let fixture = FixtureDir::new();
        let path = fixture.0.join("invalid-patch.hottrimmer");
        let mut store = ProjectStore::create(&path, "Invalid patch").expect("create project");
        let base = source(1024, 512);
        store
            .replace_source(SourceChannel::BaseColor, &base)
            .expect("base source");
        let mut invalid = patch(base.id, "Crossed patch");
        invalid.geometry.corners.swap(1, 2);
        let journal_before = store.autosave_journal().expect("journal before").len();

        assert!(matches!(
            store.execute_patch_command(
                &PatchCommand::Create {
                    patch: invalid,
                    index: None,
                },
                None,
            ),
            Err(StoreError::InvalidData(_))
        ));
        assert!(store.patches().is_empty());
        assert_eq!(
            store.autosave_journal().expect("journal after").len(),
            journal_before
        );
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
    fn project_folder_has_no_persistent_database_or_lock_sidecars() {
        let fixture = FixtureDir::new();
        let project = fixture.0.join("bundle.hottrimmer");
        let store = ProjectStore::create(&project, "Bundle").expect("create project");
        store.save().expect("save project");

        assert!(project.exists());
        assert!(!project.with_extension("hottrimmer-wal").exists());
        assert!(!project.with_extension("hottrimmer-shm").exists());
        assert!(!project.with_extension("hottrimmer.lock").exists());
        assert_ne!(lock_path(&project).parent(), project.parent());
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
    fn material_source_sets_keep_channels_and_registration_independent() {
        let fixture = FixtureDir::new();
        let path = fixture.0.join("multiple-sources.hottrimmer");
        let mut store = ProjectStore::create(&path, "Multiple sources").expect("create project");
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        store
            .replace_source_in_set(first, SourceChannel::BaseColor, &source(4, 6))
            .expect("first base color");
        store
            .replace_source_in_set(first, SourceChannel::Normal, &source(4, 6))
            .expect("first normal");
        store
            .replace_source_in_set(second, SourceChannel::BaseColor, &source(8, 10))
            .expect("second base color may use independent dimensions");
        store
            .replace_source_in_set(second, SourceChannel::Normal, &source(8, 10))
            .expect("second normal");
        let summary = store.summary().expect("summary");
        assert_eq!(summary.sources.len(), 4);
        assert_eq!(
            summary
                .sources
                .iter()
                .filter(|source| source.source_set_id == first)
                .count(),
            2
        );
        assert_eq!(
            summary
                .sources
                .iter()
                .filter(|source| source.source_set_id == second)
                .count(),
            2
        );
        assert!(matches!(
            store.remove_source_in_set(first, SourceChannel::BaseColor),
            Err(StoreError::BaseColorInUse)
        ));
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

#[cfg(test)]
mod algorithm_stage_01_tests {
    use std::{fs, path::PathBuf};

    use hot_trimmer_domain::{
        AssignmentProvenance, ChannelInterpretation, ChannelRegistration, ContentDigest, GridRect,
        MaterialChannelRole, NormalConvention, RegistrationDiagnosticCode,
        RegistrationRecoveryChoice, SourceId,
    };
    use uuid::Uuid;

    use super::{
        ProjectStore, SourceChannel, SourceInput, SourceOwnership, StoreError,
        persist_document_state,
    };
    use hot_trimmer_domain::TrimSheetDocumentCommand;

    fn input(label: u8, width: u32, height: u32, orientation: u16) -> SourceInput {
        let owned_bytes = vec![label, width as u8, height as u8, orientation as u8];
        SourceInput {
            id: SourceId::new(),
            ownership: SourceOwnership::OwnedCopy,
            external_path: None,
            origin_path: PathBuf::from(format!("capture-{label}.png")),
            sha256: ContentDigest::sha256(&owned_bytes).0,
            width,
            height,
            format: "PNG".into(),
            color_type: "Rgba8".into(),
            has_alpha: true,
            exif_orientation: orientation,
            has_embedded_icc_profile: false,
            encoded_bytes: owned_bytes.len() as u64,
            owned_bytes: Some(owned_bytes),
        }
    }

    #[test]
    fn algorithm_stage_01_registration() {
        let root = std::env::temp_dir().join(format!("hot-trimmer-stage-01-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create fixture directory");
        let path = root.join("registration.hottrimmer");
        let mut store = ProjectStore::create(&path, "Stage 1").expect("create current schema");
        let primary = Uuid::from_bytes(
            store.summary().expect("empty summary").source_sets[0]
                .id
                .to_bytes(),
        );

        let missing_base = store
            .replace_source_in_set(primary, SourceChannel::Normal, &input(1, 8, 4, 1))
            .expect_err("companions require Base Color");
        let diagnostic = missing_base
            .registration_diagnostic(SourceChannel::Normal)
            .expect("typed registration diagnostic");
        assert_eq!(
            diagnostic.code,
            RegistrationDiagnosticCode::BaseColorRequired
        );
        assert_eq!(
            diagnostic.recovery_choices,
            vec![RegistrationRecoveryChoice::AssignBaseColor]
        );

        let base = input(2, 8, 4, 1);
        let original_bytes = base.owned_bytes.clone().expect("owned bytes");
        store
            .replace_source_in_set(primary, SourceChannel::BaseColor, &base)
            .expect("Base-Color-only is valid");
        let base_only = store.summary().expect("base-only summary");
        assert_eq!(base_only.source_sets[0].source_revision, 1);
        assert_eq!(
            base_only.sources[0].input.owned_bytes.as_deref(),
            Some(original_bytes.as_slice())
        );
        assert_eq!(
            base_only.sources[0].input.sha256,
            ContentDigest::sha256(&original_bytes).0
        );
        assert_eq!(
            base_only.sources[0].registration.interpretation,
            ChannelInterpretation::ColorManagedBaseColor
        );

        let dimension_error = store
            .replace_source_in_set(primary, SourceChannel::Roughness, &input(3, 4, 8, 1))
            .expect_err("no implicit resize or rotate");
        assert!(matches!(
            dimension_error,
            StoreError::RegistrationMismatch { .. }
        ));
        assert_eq!(
            dimension_error
                .registration_diagnostic(SourceChannel::Roughness)
                .unwrap()
                .code,
            RegistrationDiagnosticCode::OrientedDimensionMismatch,
        );
        let orientation_error = store
            .replace_source_in_set(primary, SourceChannel::Roughness, &input(4, 8, 4, 6))
            .expect_err("orientation transforms cannot drift");
        assert!(matches!(
            orientation_error,
            StoreError::OrientationMismatch { .. }
        ));

        let invalid_normal = ChannelRegistration {
            role: MaterialChannelRole::Normal,
            interpretation: ChannelInterpretation::ColorManagedBaseColor,
            normal_convention: NormalConvention::OpenGl,
            assignment_provenance: AssignmentProvenance::UserAssigned,
            confidence_milli: 1000,
        };
        assert!(matches!(
            store.replace_registered_source_in_set(primary, &input(5, 8, 4, 1), invalid_normal),
            Err(StoreError::ChannelInterpretationMismatch { .. })
        ));

        for (index, channel) in SourceChannel::ALL.into_iter().enumerate().skip(1) {
            let mut registration = ChannelRegistration::explicit(channel.material_role());
            registration.assignment_provenance = if index % 2 == 0 {
                AssignmentProvenance::FilenameSuggested
            } else {
                AssignmentProvenance::UserAssigned
            };
            registration.confidence_milli =
                if registration.assignment_provenance == AssignmentProvenance::FilenameSuggested {
                    700
                } else {
                    1000
                };
            if channel == SourceChannel::Normal {
                registration.normal_convention = NormalConvention::OpenGl;
            }
            store
                .replace_registered_source_in_set(
                    primary,
                    &input(10 + index as u8, 8, 4, 1),
                    registration,
                )
                .expect("full PBR and auxiliary roles register through one path");
        }
        let full = store.summary().expect("full registered source");
        assert_eq!(
            full.sources
                .iter()
                .filter(|source| source.source_set_id == primary)
                .count(),
            10
        );
        assert_eq!(full.source_sets[0].source_revision, 10);
        assert!(
            full.sources
                .iter()
                .filter(|source| source.channel != SourceChannel::BaseColor)
                .all(|source| source.registration.interpretation
                    != ChannelInterpretation::ColorManagedBaseColor)
        );

        store.connection.execute(
            "INSERT INTO source_derived_cache (source_set_id, registration_digest, cache_key) VALUES (?1, ?2, 'stale')",
            rusqlite::params![primary.to_string(), full.source_sets[0].registration_digest.0],
        ).expect("seed derived cache");
        let before_replace = full.source_sets[0].clone();
        store
            .replace_source_in_set(primary, SourceChannel::Roughness, &input(40, 8, 4, 1))
            .expect("replace channel");
        let after_replace = store.summary().expect("replacement summary").source_sets[0].clone();
        assert_eq!(
            after_replace.source_revision,
            before_replace.source_revision + 1
        );
        assert_ne!(
            after_replace.registration_digest,
            before_replace.registration_digest
        );
        let stale_count: u32 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM source_derived_cache WHERE source_set_id = ?1",
                [primary.to_string()],
                |row| row.get(0),
            )
            .expect("cache count");
        assert_eq!(
            stale_count, 0,
            "replacement must invalidate all derived entries"
        );

        store.connection.execute(
            "INSERT INTO source_derived_cache (source_set_id, registration_digest, cache_key) VALUES (?1, ?2, 'stale-remove')",
            rusqlite::params![primary.to_string(), after_replace.registration_digest.0],
        ).expect("seed removal cache");
        store
            .remove_source_in_set(primary, SourceChannel::Specular)
            .expect("remove companion");
        let after_remove = store.summary().unwrap().source_sets[0].clone();
        assert_eq!(
            after_remove.source_revision,
            after_replace.source_revision + 1
        );
        let removal_cache_count: u32 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM source_derived_cache WHERE source_set_id = ?1",
                [primary.to_string()],
                |row| row.get(0),
            )
            .expect("removal cache count");
        assert_eq!(
            removal_cache_count, 0,
            "removal must invalidate all derived entries"
        );

        let exemplar = Uuid::new_v4();
        store
            .replace_source_in_set(exemplar, SourceChannel::BaseColor, &input(50, 16, 16, 1))
            .expect("independent exemplar");
        store.connection.execute(
            "INSERT INTO source_derived_cache (source_set_id, registration_digest, cache_key) VALUES (?1, ?2, 'stale-group')",
            rusqlite::params![primary.to_string(), after_remove.registration_digest.0],
        ).expect("seed grouping cache");
        store
            .set_exemplar_group(primary, Some("related-captures"))
            .expect("group primary exemplar");
        let grouping_cache_count: u32 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM source_derived_cache WHERE source_set_id = ?1",
                [primary.to_string()],
                |row| row.get(0),
            )
            .expect("grouping cache count");
        assert_eq!(
            grouping_cache_count, 0,
            "grouping must invalidate all derived entries"
        );
        store
            .set_exemplar_group(exemplar, Some("related-captures"))
            .expect("group second exemplar");
        let grouped = store.summary().expect("grouped exemplars");
        assert_eq!(
            grouped
                .source_sets
                .iter()
                .filter(|source| source.exemplar_group.as_deref() == Some("related-captures"))
                .count(),
            2
        );

        let mut tampered = input(60, 8, 4, 1);
        tampered.sha256 = "0".repeat(64);
        assert!(matches!(
            store.replace_source_in_set(primary, SourceChannel::Opacity, &tampered),
            Err(StoreError::ImmutableDigestMismatch)
        ));

        let external_path = root.join("external.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([12, 34, 56, 255]))
            .save(&external_path)
            .expect("write external fixture");
        let inspected = hot_trimmer_image_io::inspect_path(
            &external_path,
            hot_trimmer_image_io::DecodeLimits::default(),
        )
        .expect("desktop-style external inspection");
        let info = inspected.info;
        let external = SourceInput {
            id: SourceId::new(),
            ownership: SourceOwnership::VerifiedExternalReference,
            external_path: Some(external_path.clone()),
            origin_path: external_path.clone(),
            sha256: info.sha256,
            width: info.width,
            height: info.height,
            format: info.format,
            color_type: info.color_type,
            has_alpha: info.has_alpha,
            exif_orientation: info.exif_orientation,
            has_embedded_icc_profile: info.has_embedded_icc_profile,
            encoded_bytes: info.encoded_bytes,
            owned_bytes: None,
        };
        store
            .replace_source_in_set(Uuid::new_v4(), SourceChannel::BaseColor, &external)
            .expect("store re-verifies a stable external reference");

        let raced_path = root.join("external-race.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]))
            .save(&raced_path)
            .expect("write race fixture");
        let inspected = hot_trimmer_image_io::inspect_path(
            &raced_path,
            hot_trimmer_image_io::DecodeLimits::default(),
        )
        .expect("initial race inspection");
        let info = inspected.info;
        let raced = SourceInput {
            id: SourceId::new(),
            ownership: SourceOwnership::VerifiedExternalReference,
            external_path: Some(raced_path.clone()),
            origin_path: raced_path.clone(),
            sha256: info.sha256,
            width: info.width,
            height: info.height,
            format: info.format,
            color_type: info.color_type,
            has_alpha: info.has_alpha,
            exif_orientation: info.exif_orientation,
            has_embedded_icc_profile: info.has_embedded_icc_profile,
            encoded_bytes: info.encoded_bytes,
            owned_bytes: None,
        };
        image::RgbaImage::from_pixel(3, 2, image::Rgba([9, 8, 7, 255]))
            .save(&raced_path)
            .expect("mutate external before persistence");
        assert!(matches!(
            store.replace_source_in_set(Uuid::new_v4(), SourceChannel::BaseColor, &raced),
            Err(StoreError::ExternalSourceChanged { .. })
        ));

        drop(store);
        fs::remove_dir_all(root).expect("remove fixture directory");
    }

    #[test]
    fn replacing_base_color_rebinds_existing_source_frame_without_changing_grid_rects() {
        let root = std::env::temp_dir().join(format!(
            "hot-trimmer-source-frame-rebind-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("create fixture directory");
        let path = root.join("rebind.hottrimmer");
        let mut store = ProjectStore::create(&path, "SourceFrame rebind").expect("create project");
        let primary = Uuid::from_bytes(
            store.summary().expect("empty summary").source_sets[0]
                .id
                .to_bytes(),
        );
        store
            .replace_source_in_set(primary, SourceChannel::BaseColor, &input(70, 8000, 4000, 1))
            .expect("initial base color");
        store
            .create_source_frame_document()
            .expect("create source frame");
        let before = store
            .summary()
            .expect("before summary")
            .document
            .expect("document");
        let detached_region = before.topology.regions[0].id;
        store
            .execute_document_command(&TrimSheetDocumentCommand::DetachSourceCell {
                region_id: detached_region,
            })
            .expect("detach one region before replacement");
        let before = store
            .summary()
            .expect("before summary after detach")
            .document
            .expect("document");
        assert!(before.source_overrides.contains_key(&detached_region));
        let before_grid = before
            .topology
            .regions
            .iter()
            .map(|region| (region.id, region.grid_rect))
            .collect::<std::collections::BTreeMap<_, _>>();
        let before_revision = before.document_revision;

        store
            .replace_source_in_set(primary, SourceChannel::BaseColor, &input(71, 7952, 4016, 1))
            .expect("replacement rebinds source frame");
        let after = store
            .summary()
            .expect("after summary")
            .document
            .expect("document");
        let frame = after.source_frame.expect("rebound source frame");
        assert_eq!(
            (
                frame.oriented_dimensions.width,
                frame.oriented_dimensions.height
            ),
            (7952, 4016)
        );
        assert_eq!(frame.source_revision, 2);
        assert_eq!(frame.identity, frame.compute_identity());
        let rebound_override = after
            .source_overrides
            .get(&detached_region)
            .expect("detached override transformed");
        let rebound_region = after
            .topology
            .regions
            .iter()
            .find(|region| region.id == detached_region)
            .expect("rebound region");
        let source_aspect = rebound_override.source_bounds.width.get() * 7952.0
            / (rebound_override.source_bounds.height.get() * 4016.0);
        let destination_aspect = f64::from(rebound_region.allocation_rect.width)
            / f64::from(rebound_region.allocation_rect.height);
        assert!((source_aspect - destination_aspect).abs() < 1e-9);
        assert!(after.document_revision > before_revision);
        assert_eq!(
            after
                .topology
                .regions
                .iter()
                .map(|region| (region.id, region.grid_rect))
                .collect::<std::collections::BTreeMap<_, _>>(),
            before_grid,
        );
        let frame_width_px = frame.bounds.width.get() * 7952.0;
        let frame_height_px = frame.bounds.height.get() * 4016.0;
        assert!((frame_width_px - frame_height_px).abs() < 0.001);
        assert!(frame.bounds.x.get() > 0.0);
        assert_eq!(frame.bounds.y.get(), 0.0);
        drop(store);
        fs::remove_dir_all(root).expect("remove fixture directory");
    }

    #[test]
    fn refreshing_assets_advances_source_frame_revision_after_companion_map_import() {
        let root = std::env::temp_dir().join(format!(
            "hot-trimmer-source-frame-refresh-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("create fixture directory");
        let path = root.join("refresh.hottrimmer");
        let mut store =
            ProjectStore::create(&path, "SourceFrame companion refresh").expect("create project");
        let primary = Uuid::from_bytes(
            store.summary().expect("empty summary").source_sets[0]
                .id
                .to_bytes(),
        );
        store
            .replace_source_in_set(primary, SourceChannel::BaseColor, &input(80, 2048, 1024, 1))
            .expect("base color");
        store
            .create_source_frame_document()
            .expect("create source frame");
        let before = store
            .summary()
            .expect("before summary")
            .document
            .expect("document");
        let before_frame = before.source_frame.clone().expect("source frame");
        assert_eq!(before_frame.source_revision, 1);

        store
            .replace_source_in_set(primary, SourceChannel::Roughness, &input(81, 2048, 1024, 1))
            .expect("companion map bumps source set revision");
        let stale = store
            .summary()
            .expect("stale summary")
            .document
            .expect("document");
        assert_eq!(
            stale
                .source_frame
                .as_ref()
                .expect("source frame")
                .source_revision,
            1
        );
        assert!(store.refresh_document_assets().expect("refresh assets"));
        let refreshed = store
            .summary()
            .expect("refreshed summary")
            .document
            .expect("document");
        let refreshed_frame = refreshed.source_frame.expect("refreshed source frame");
        assert_eq!(refreshed_frame.source_revision, 2);
        assert_eq!(refreshed_frame.bounds, before_frame.bounds);
        assert_eq!(
            refreshed_frame.oriented_dimensions,
            before_frame.oriented_dimensions
        );
        assert_eq!(refreshed_frame.identity, refreshed_frame.compute_identity());
        assert!(refreshed.document_revision > before.document_revision);
        assert!(
            !store
                .refresh_document_assets()
                .expect("second refresh is idempotent")
        );

        drop(store);
        fs::remove_dir_all(root).expect("remove fixture directory");
    }

    #[test]
    fn direct_source_frame_rectangle_is_one_undoable_persisted_command() {
        let root =
            std::env::temp_dir().join(format!("hot-trimmer-source-frame-draw-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create fixture directory");
        let path = root.join("draw.hottrimmer");
        let mut store = ProjectStore::create(&path, "SourceFrame draw").expect("create project");
        let primary = Uuid::from_bytes(
            store.summary().expect("empty summary").source_sets[0]
                .id
                .to_bytes(),
        );
        store
            .replace_source_in_set(primary, SourceChannel::BaseColor, &input(72, 4096, 4096, 1))
            .expect("base color");
        store
            .create_source_frame_document()
            .expect("create source frame");
        let before = store.document().expect("document").topology.topology_hash;
        let drawn_rect = GridRect {
            x: 8,
            y: 8,
            width: 24,
            height: 16,
        };
        store
            .execute_document_command(&TrimSheetDocumentCommand::DrawSourceFrameRegion {
                grid_rect: drawn_rect,
            })
            .expect("draw rectangle");
        let drawn_hash = store
            .document()
            .expect("drawn document")
            .topology
            .topology_hash;
        assert_ne!(drawn_hash, before);
        assert!(
            store
                .document()
                .unwrap()
                .topology
                .regions
                .iter()
                .any(|region| region.grid_rect == Some(drawn_rect))
        );
        store.undo_document_command().expect("undo direct draw");
        assert_eq!(store.document().unwrap().topology.topology_hash, before);
        store.redo_document_command().expect("redo direct draw");
        assert_eq!(store.document().unwrap().topology.topology_hash, drawn_hash);
        assert!(
            store
                .document()
                .unwrap()
                .topology
                .regions
                .iter()
                .any(|region| region.grid_rect == Some(drawn_rect))
        );
        let drawn_id = store
            .document()
            .unwrap()
            .topology
            .regions
            .iter()
            .find(|region| region.grid_rect == Some(drawn_rect))
            .expect("drawn region")
            .id;
        let resized_rect = GridRect {
            x: 10,
            y: 8,
            width: 26,
            height: 18,
        };
        store
            .execute_document_command(&TrimSheetDocumentCommand::ResizeSourceFrameRegion {
                region_id: drawn_id,
                grid_rect: resized_rect,
            })
            .expect("resize selected rectangle");
        let resized_hash = store
            .document()
            .expect("resized document")
            .topology
            .topology_hash;
        assert_ne!(resized_hash, drawn_hash);
        assert_eq!(
            store
                .document()
                .unwrap()
                .topology
                .regions
                .iter()
                .find(|region| region.id == drawn_id)
                .expect("stable resized region")
                .grid_rect,
            Some(resized_rect)
        );
        store.undo_document_command().expect("undo direct resize");
        assert_eq!(store.document().unwrap().topology.topology_hash, drawn_hash);
        assert_eq!(
            store
                .document()
                .unwrap()
                .topology
                .regions
                .iter()
                .find(|region| region.id == drawn_id)
                .expect("restored region")
                .grid_rect,
            Some(drawn_rect)
        );
        store.redo_document_command().expect("redo direct resize");
        assert_eq!(
            store.document().unwrap().topology.topology_hash,
            resized_hash
        );
        assert_eq!(
            store
                .document()
                .unwrap()
                .topology
                .regions
                .iter()
                .find(|region| region.id == drawn_id)
                .expect("redone region")
                .grid_rect,
            Some(resized_rect)
        );
        drop(store);
        fs::remove_dir_all(root).expect("remove fixture directory");
    }

    #[test]
    fn opening_generated_source_frame_snapshots_exact_topology_without_regeneration() {
        let root =
            std::env::temp_dir().join(format!("hot-trimmer-authored-migration-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create fixture directory");
        let path = root.join("migration.hottrimmer");
        let mut store = ProjectStore::create(&path, "Authored migration").expect("create project");
        let primary = Uuid::from_bytes(
            store.summary().expect("summary").source_sets[0]
                .id
                .to_bytes(),
        );
        store
            .replace_source_in_set(primary, SourceChannel::BaseColor, &input(73, 4096, 4096, 1))
            .expect("base color");
        store.create_source_frame_document().expect("source frame");
        let mut legacy = store.document().expect("document").clone();
        legacy.authored_layout_preset = None;
        legacy.authored_layout_instance_id = None;
        let before = legacy
            .topology
            .regions
            .iter()
            .map(|region| (region.id, region.grid_rect))
            .collect::<Vec<_>>();
        persist_document_state(
            &mut store.connection,
            Some(&legacy),
            "legacy_generated_fixture",
        )
        .expect("persist fixture");
        drop(store);

        let reopened = ProjectStore::open(&path).expect("open migrates accepted topology");
        let migrated = reopened.document().expect("migrated document");
        assert_eq!(
            migrated
                .topology
                .regions
                .iter()
                .map(|region| (region.id, region.grid_rect))
                .collect::<Vec<_>>(),
            before
        );
        let preset = migrated
            .authored_layout_preset
            .as_ref()
            .expect("embedded authored snapshot");
        assert_eq!(
            preset
                .regions
                .iter()
                .map(|region| region.grid_rect)
                .collect::<Vec<_>>(),
            before
                .iter()
                .map(|(_, rect)| rect.expect("grid rect"))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            preset.provenance,
            "migrated_accepted_topology_without_regeneration"
        );
        drop(reopened);
        fs::remove_dir_all(root).expect("remove fixture directory");
    }
}
