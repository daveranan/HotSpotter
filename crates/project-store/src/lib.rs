#![doc = "Project persistence boundary. `SQLite` implementation begins in Phase 1."]

pub const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceOwnership {
    OwnedCopy,
    VerifiedExternalReference,
}
