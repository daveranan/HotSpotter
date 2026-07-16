mod channel;
mod error;
mod id;
mod layout;
mod patch;
mod protocol;
mod templates;
mod units;

pub use channel::{Channel, ChannelDataKind};
pub use error::{DomainError, ErrorCode, UserFacingError};
pub use id::{LayerId, LayoutId, MapId, PatchId, ProjectId, RegionId, SourceId, SourceSetId};
pub use layout::{
    AutoPackSettings, DecorationBinding, FillBehavior, FixedRegionSize, IdColor, Layout,
    LayoutContractError, LayoutItem, LayoutKind, LayoutOrder, LayoutPreset, LayoutRegion,
    LayoutRequest, LayoutSettings, MAX_LAYOUT_EDGE, MAX_LAYOUT_PIXELS, MAX_LAYOUT_REGIONS,
    MAX_REGION_INSET, MAX_REGION_KEY_BYTES, NormalizedBounds, PackPriority, PixelBounds, PixelSize,
    RegionConstraints, RegionFill, RegionLocks, SimpleDataInput, SlotBinding, SourceFraming,
    SourceFramingMode, StyleRecipe, TemplateIdentity, TemplateLayoutContract, TemplateSnapshot,
    TrimAxis, TrimCaps,
};
pub use patch::{
    MapParticipation, Patch, PatchCommand, PatchCommandError, PatchEditOutcome, PatchGeometry,
    PatchProperties, PatchSet, RectificationSettings, RepeatMode,
};
pub use protocol::{FoundationStatusRequest, IPC_PROTOCOL_VERSION};
pub use templates::{
    CanonicalRect, Hotspot, RadialParameters, StructuralProfile, TemplateDefinition,
    TemplateRegistry, TemplateRegistryError, TemplateSlot, TemplateSlotRole, WorldPlacement,
    CANONICAL_TEMPLATE_EDGE,
};
pub use units::{NormalizedPoint, NormalizedScalar};
