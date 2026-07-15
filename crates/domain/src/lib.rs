mod channel;
mod error;
mod id;
mod patch;
mod protocol;
mod units;

pub use channel::{Channel, ChannelDataKind};
pub use error::{DomainError, ErrorCode, UserFacingError};
pub use id::{LayerId, LayoutId, MapId, PatchId, ProjectId, SourceId};
pub use patch::{
    MapParticipation, Patch, PatchCommand, PatchCommandError, PatchEditOutcome, PatchGeometry,
    PatchProperties, PatchSet, RectificationSettings, RepeatMode,
};
pub use protocol::{FoundationStatusRequest, IPC_PROTOCOL_VERSION};
pub use units::{NormalizedPoint, NormalizedScalar};
