use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{NormalizedPoint, PatchId, SourceId};

/// The four editable source-space corners of a patch.
///
/// Corners use the canonical top-left, top-right, bottom-right, bottom-left order. Geometry code is
/// responsible for validating winding, convexity, area, and intersections before this value is persisted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchGeometry {
    pub corners: [NormalizedPoint; 4],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistance_mask: Option<Vec<NormalizedPoint>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepeatMode {
    RepeatX,
    RepeatY,
    TileXy,
    Stretch,
    Unique,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapParticipation {
    All,
    BaseColorOnly,
    Excluded,
}

/// Persistent patch behavior. Pixel-valued padding and bleed are interpreted in the eventual layout
/// resolution so source geometry remains resolution independent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchProperties {
    pub repeat_mode: RepeatMode,
    pub trim_cap: bool,
    pub padding_px: u32,
    pub bleed_px: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material_id: Option<u16>,
    pub map_participation: MapParticipation,
}

impl Default for PatchProperties {
    fn default() -> Self {
        Self {
            repeat_mode: RepeatMode::Unique,
            trim_cap: false,
            padding_px: 4,
            bleed_px: 8,
            material_id: None,
            map_participation: MapParticipation::All,
        }
    }
}

/// User-controlled rectified-output intent. An absent aspect ratio follows the measured patch shape and
/// scale is relative to its source-space pixel footprint.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RectificationSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<f64>,
    pub scale: f64,
}

impl Default for RectificationSettings {
    fn default() -> Self {
        Self {
            aspect_ratio: None,
            scale: 1.0,
        }
    }
}

impl RectificationSettings {
    #[must_use]
    pub fn is_valid(self) -> bool {
        self.scale.is_finite()
            && (0.01..=16.0).contains(&self.scale)
            && self
                .aspect_ratio
                .is_none_or(|ratio| ratio.is_finite() && (0.01..=100.0).contains(&ratio))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Patch {
    pub id: PatchId,
    pub source_id: SourceId,
    pub name: String,
    pub enabled: bool,
    pub geometry: PatchGeometry,
    pub properties: PatchProperties,
    pub rectification: RectificationSettings,
}

impl Patch {
    #[must_use]
    pub fn has_valid_metadata(&self) -> bool {
        let name = self.name.trim();
        !name.is_empty()
            && name.len() <= 255
            && self.properties.bleed_px <= 4096
            && self.properties.padding_px <= 4096
            && self.rectification.is_valid()
            && self
                .geometry
                .assistance_mask
                .as_ref()
                .is_none_or(|points| (4..=8).contains(&points.len()))
    }
}

pub const MAX_PATCH_COMMAND_HISTORY: usize = 256;

/// Every persistent patch edit is represented once in this domain command contract. A drag uses repeated
/// `ReplaceGeometry` commands with the same nonzero coalescing group.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum PatchCommand {
    Create {
        patch: Patch,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index: Option<usize>,
    },
    ReplaceGeometry {
        patch_id: PatchId,
        geometry: PatchGeometry,
    },
    Rename {
        patch_id: PatchId,
        name: String,
    },
    SetEnabled {
        patch_id: PatchId,
        enabled: bool,
    },
    SetProperties {
        patch_id: PatchId,
        properties: PatchProperties,
    },
    SetRectification {
        patch_id: PatchId,
        settings: RectificationSettings,
    },
    Duplicate {
        patch_id: PatchId,
        new_id: PatchId,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index: Option<usize>,
    },
    Reorder {
        patch_id: PatchId,
        to_index: usize,
    },
    ReassignSource {
        from_source_id: SourceId,
        to_source_id: SourceId,
    },
    Delete {
        patch_id: PatchId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatchEditOutcome {
    pub invalidated_patch_ids: BTreeSet<PatchId>,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct HistoryEntry {
    before: Vec<Patch>,
    after: Vec<Patch>,
    coalescing_group: Option<u64>,
    invalidated_patch_ids: BTreeSet<PatchId>,
}

/// Ordered patch state with bounded command history, save-point tracking, undo/redo, and drag coalescing.
#[derive(Clone, Debug, PartialEq)]
pub struct PatchSet {
    patches: Vec<Patch>,
    history: Vec<HistoryEntry>,
    cursor: usize,
    saved_cursor: Option<usize>,
}

impl Default for PatchSet {
    fn default() -> Self {
        Self::new(Vec::new()).expect("an empty patch set is valid")
    }
}

impl PatchSet {
    /// Restores an ordered patch collection without inventing undo history.
    ///
    /// # Errors
    ///
    /// Returns a typed failure when stable IDs repeat or persistent metadata is invalid.
    pub fn new(patches: Vec<Patch>) -> Result<Self, PatchCommandError> {
        validate_patch_collection(&patches)?;
        Ok(Self {
            patches,
            history: Vec::new(),
            cursor: 0,
            saved_cursor: Some(0),
        })
    }

    #[must_use]
    pub fn patches(&self) -> &[Patch] {
        &self.patches
    }

    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.saved_cursor != Some(self.cursor)
    }

    #[must_use]
    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    #[must_use]
    pub fn can_redo(&self) -> bool {
        self.cursor < self.history.len()
    }

    pub fn mark_saved(&mut self) {
        self.saved_cursor = Some(self.cursor);
    }

    /// Applies one command. Use a shared nonzero `coalescing_group` for all updates in one pointer drag.
    ///
    /// # Errors
    ///
    /// Returns a typed failure without changing state when the command targets missing/duplicate data or
    /// violates bounded patch metadata.
    pub fn execute(
        &mut self,
        command: PatchCommand,
        coalescing_group: Option<u64>,
    ) -> Result<PatchEditOutcome, PatchCommandError> {
        if coalescing_group == Some(0) {
            return Err(PatchCommandError::InvalidCoalescingGroup);
        }
        let before = self.patches.clone();
        let mut after = before.clone();
        apply_command(&mut after, command)?;
        validate_patch_collection(&after)?;
        if before == after {
            return Ok(self.outcome(BTreeSet::new()));
        }
        let invalidated_patch_ids = changed_patch_ids(&before, &after);
        self.patches = after;

        if self.cursor < self.history.len() {
            self.history.truncate(self.cursor);
            if self.saved_cursor.is_some_and(|saved| saved > self.cursor) {
                self.saved_cursor = None;
            }
        }
        let can_coalesce = coalescing_group.is_some()
            && self.cursor == self.history.len()
            && self.saved_cursor != Some(self.cursor)
            && self
                .history
                .last()
                .is_some_and(|entry| entry.coalescing_group == coalescing_group);
        if can_coalesce && let Some(entry) = self.history.last_mut() {
            entry.after.clone_from(&self.patches);
            entry
                .invalidated_patch_ids
                .extend(invalidated_patch_ids.iter().copied());
        } else {
            self.history.push(HistoryEntry {
                before,
                after: self.patches.clone(),
                coalescing_group,
                invalidated_patch_ids: invalidated_patch_ids.clone(),
            });
            self.cursor += 1;
            self.enforce_history_bound();
        }
        Ok(self.outcome(invalidated_patch_ids))
    }

    /// Restores the state before the most recent coalesced command.
    ///
    /// # Errors
    ///
    /// Returns [`PatchCommandError::NothingToUndo`] without changing state at the beginning of history.
    pub fn undo(&mut self) -> Result<PatchEditOutcome, PatchCommandError> {
        if self.cursor == 0 {
            return Err(PatchCommandError::NothingToUndo);
        }
        self.cursor -= 1;
        let entry = &self.history[self.cursor];
        self.patches.clone_from(&entry.before);
        Ok(self.outcome(entry.invalidated_patch_ids.clone()))
    }

    /// Reapplies the next command after undo.
    ///
    /// # Errors
    ///
    /// Returns [`PatchCommandError::NothingToRedo`] without changing state at the end of history.
    pub fn redo(&mut self) -> Result<PatchEditOutcome, PatchCommandError> {
        let entry = self
            .history
            .get(self.cursor)
            .ok_or(PatchCommandError::NothingToRedo)?;
        self.patches.clone_from(&entry.after);
        let invalidated = entry.invalidated_patch_ids.clone();
        self.cursor += 1;
        Ok(self.outcome(invalidated))
    }

    fn outcome(&self, invalidated_patch_ids: BTreeSet<PatchId>) -> PatchEditOutcome {
        PatchEditOutcome {
            invalidated_patch_ids,
            dirty: self.is_dirty(),
            can_undo: self.can_undo(),
            can_redo: self.can_redo(),
        }
    }

    fn enforce_history_bound(&mut self) {
        if self.history.len() <= MAX_PATCH_COMMAND_HISTORY {
            return;
        }
        let remove_count = self.history.len() - MAX_PATCH_COMMAND_HISTORY;
        self.history.drain(..remove_count);
        self.cursor = self.cursor.saturating_sub(remove_count);
        self.saved_cursor = self
            .saved_cursor
            .and_then(|saved| saved.checked_sub(remove_count));
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PatchCommandError {
    #[error("patch {0} does not exist")]
    NotFound(PatchId),
    #[error("patch {0} already exists")]
    DuplicateId(PatchId),
    #[error("patch name, output settings, mask, padding, or bleed is invalid")]
    InvalidMetadata,
    #[error("patch order index {index} exceeds the patch count {count}")]
    InvalidIndex { index: usize, count: usize },
    #[error("coalescing group zero is reserved")]
    InvalidCoalescingGroup,
    #[error("there is no patch edit to undo")]
    NothingToUndo,
    #[error("there is no patch edit to redo")]
    NothingToRedo,
}

fn validate_patch_collection(patches: &[Patch]) -> Result<(), PatchCommandError> {
    let mut ids = BTreeSet::new();
    for patch in patches {
        if !patch.has_valid_metadata() {
            return Err(PatchCommandError::InvalidMetadata);
        }
        if !ids.insert(patch.id) {
            return Err(PatchCommandError::DuplicateId(patch.id));
        }
    }
    Ok(())
}

fn changed_patch_ids(before: &[Patch], after: &[Patch]) -> BTreeSet<PatchId> {
    let before_position = |id| before.iter().position(|patch| patch.id == id);
    let after_position = |id| after.iter().position(|patch| patch.id == id);
    before
        .iter()
        .chain(after)
        .filter(|patch| {
            let before_patch = before.iter().find(|candidate| candidate.id == patch.id);
            let after_patch = after.iter().find(|candidate| candidate.id == patch.id);
            before_patch != after_patch || before_position(patch.id) != after_position(patch.id)
        })
        .map(|patch| patch.id)
        .collect()
}

fn apply_command(patches: &mut Vec<Patch>, command: PatchCommand) -> Result<(), PatchCommandError> {
    match command {
        PatchCommand::Create { patch, index } => {
            if patches.iter().any(|candidate| candidate.id == patch.id) {
                return Err(PatchCommandError::DuplicateId(patch.id));
            }
            if !patch.has_valid_metadata() {
                return Err(PatchCommandError::InvalidMetadata);
            }
            let index = index.unwrap_or(patches.len());
            if index > patches.len() {
                return Err(PatchCommandError::InvalidIndex {
                    index,
                    count: patches.len(),
                });
            }
            patches.insert(index, patch);
        }
        PatchCommand::ReplaceGeometry { patch_id, geometry } => {
            find_patch_mut(patches, patch_id)?.geometry = geometry;
        }
        PatchCommand::Rename { patch_id, name } => {
            name.trim()
                .clone_into(&mut find_patch_mut(patches, patch_id)?.name);
        }
        PatchCommand::SetEnabled { patch_id, enabled } => {
            find_patch_mut(patches, patch_id)?.enabled = enabled;
        }
        PatchCommand::SetProperties {
            patch_id,
            properties,
        } => {
            find_patch_mut(patches, patch_id)?.properties = properties;
        }
        PatchCommand::SetRectification { patch_id, settings } => {
            find_patch_mut(patches, patch_id)?.rectification = settings;
        }
        PatchCommand::Duplicate {
            patch_id,
            new_id,
            name,
            index,
        } => {
            if patches.iter().any(|candidate| candidate.id == new_id) {
                return Err(PatchCommandError::DuplicateId(new_id));
            }
            let source_index = patches
                .iter()
                .position(|patch| patch.id == patch_id)
                .ok_or(PatchCommandError::NotFound(patch_id))?;
            let mut duplicate = patches[source_index].clone();
            duplicate.id = new_id;
            name.trim().clone_into(&mut duplicate.name);
            if !duplicate.has_valid_metadata() {
                return Err(PatchCommandError::InvalidMetadata);
            }
            let index = index.unwrap_or(source_index + 1);
            if index > patches.len() {
                return Err(PatchCommandError::InvalidIndex {
                    index,
                    count: patches.len(),
                });
            }
            patches.insert(index, duplicate);
        }
        PatchCommand::Reorder { patch_id, to_index } => {
            if to_index >= patches.len() {
                return Err(PatchCommandError::InvalidIndex {
                    index: to_index,
                    count: patches.len(),
                });
            }
            let from_index = patches
                .iter()
                .position(|patch| patch.id == patch_id)
                .ok_or(PatchCommandError::NotFound(patch_id))?;
            let patch = patches.remove(from_index);
            patches.insert(to_index, patch);
        }
        PatchCommand::ReassignSource {
            from_source_id,
            to_source_id,
        } => {
            for patch in patches
                .iter_mut()
                .filter(|patch| patch.source_id == from_source_id)
            {
                patch.source_id = to_source_id;
            }
        }
        PatchCommand::Delete { patch_id } => {
            let index = patches
                .iter()
                .position(|patch| patch.id == patch_id)
                .ok_or(PatchCommandError::NotFound(patch_id))?;
            patches.remove(index);
        }
    }
    Ok(())
}

fn find_patch_mut(
    patches: &mut [Patch],
    patch_id: PatchId,
) -> Result<&mut Patch, PatchCommandError> {
    patches
        .iter_mut()
        .find(|patch| patch.id == patch_id)
        .ok_or(PatchCommandError::NotFound(patch_id))
}

#[cfg(test)]
mod tests {
    use super::{
        Patch, PatchCommand, PatchGeometry, PatchProperties, PatchSet, RectificationSettings,
    };
    use crate::{NormalizedPoint, PatchId, SourceId};

    fn point(x: f64, y: f64) -> NormalizedPoint {
        NormalizedPoint::new(x, y).expect("test point")
    }

    fn patch(name: &str) -> Patch {
        Patch {
            id: PatchId::new(),
            source_id: SourceId::new(),
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
    fn patch_contract_round_trips_through_json() {
        let patch = Patch {
            id: PatchId::new(),
            source_id: SourceId::new(),
            name: "Brick course".into(),
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
        };

        let encoded = serde_json::to_string(&patch).expect("serialize patch");
        let decoded: Patch = serde_json::from_str(&encoded).expect("deserialize patch");
        assert_eq!(decoded, patch);
        assert!(decoded.has_valid_metadata());
    }

    #[test]
    fn patch_command_json_matches_the_camel_case_ipc_contract() {
        let authored = patch("IPC patch");
        let value = serde_json::to_value(PatchCommand::Create {
            patch: authored,
            index: Some(2),
        })
        .expect("serialize command");
        assert_eq!(value["type"], "create");
        assert_eq!(value["index"], 2);
        assert!(value["patch"]["sourceId"].is_string());
        assert!(value["patch"]["properties"]["repeatMode"].is_string());
        assert!(value["patch"]["rectification"]["scale"].is_number());
        assert!(value["patch"].get("source_id").is_none());
    }

    #[test]
    fn rejects_unbounded_rectification_intent() {
        assert!(
            !RectificationSettings {
                aspect_ratio: Some(f64::INFINITY),
                scale: 1.0,
            }
            .is_valid()
        );
        assert!(
            !RectificationSettings {
                aspect_ratio: None,
                scale: 0.0,
            }
            .is_valid()
        );
    }

    #[test]
    fn command_history_coalesces_drag_and_tracks_save_point() {
        let original = patch("Course");
        let mut patches = PatchSet::new(vec![original.clone()]).expect("patch set");
        for x in [0.12, 0.14, 0.18] {
            let mut geometry = original.geometry.clone();
            geometry.corners[0] = point(x, 0.1);
            patches
                .execute(
                    PatchCommand::ReplaceGeometry {
                        patch_id: original.id,
                        geometry,
                    },
                    Some(17),
                )
                .expect("drag update");
        }
        assert!(patches.is_dirty());
        assert_eq!(patches.patches()[0].geometry.corners[0], point(0.18, 0.1));
        patches.undo().expect("one coalesced undo");
        assert_eq!(patches.patches()[0], original);
        assert!(!patches.is_dirty());
        patches.redo().expect("redo drag");
        assert_eq!(patches.patches()[0].geometry.corners[0], point(0.18, 0.1));
        patches.mark_saved();
        assert!(!patches.is_dirty());
    }

    #[test]
    fn create_duplicate_reorder_delete_are_undoable() {
        let first = patch("First");
        let second = patch("Second");
        let duplicate_id = PatchId::new();
        let mut patches = PatchSet::default();
        patches
            .execute(
                PatchCommand::Create {
                    patch: first.clone(),
                    index: None,
                },
                None,
            )
            .expect("create first");
        patches
            .execute(
                PatchCommand::Create {
                    patch: second.clone(),
                    index: None,
                },
                None,
            )
            .expect("create second");
        patches
            .execute(
                PatchCommand::Duplicate {
                    patch_id: first.id,
                    new_id: duplicate_id,
                    name: "First copy".into(),
                    index: None,
                },
                None,
            )
            .expect("duplicate");
        patches
            .execute(
                PatchCommand::Reorder {
                    patch_id: second.id,
                    to_index: 0,
                },
                None,
            )
            .expect("reorder");
        patches
            .execute(
                PatchCommand::Delete {
                    patch_id: duplicate_id,
                },
                None,
            )
            .expect("delete");
        assert_eq!(patches.patches()[0].id, second.id);
        assert_eq!(patches.patches().len(), 2);
        patches.undo().expect("undo delete");
        assert_eq!(patches.patches().len(), 3);
        patches.undo().expect("undo reorder");
        assert_eq!(patches.patches()[0].id, first.id);
    }
}
