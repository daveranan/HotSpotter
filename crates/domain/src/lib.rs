mod channel;
mod document;
mod error;
mod id;
mod layout;
mod patch;
mod protocol;
mod templates;
mod units;

pub use channel::{Channel, ChannelDataKind};
pub use document::{
    AcceptedTopology, AddressMode, AppearanceHashInputs, BlendPolicy, ChangeClassification,
    ChannelBitDepth, ChannelRenderPolicy, ContentReference, DocumentHash, FitAxis,
    GeneratorProvenance, LayoutGridSettings, MAX_MAPPING_MAGNITUDE, MappingTransform, MaterialMapContent,
    MaterialMapKind, MaterialSourceSet, ProceduralMaterial, Projection, QuarterTurn,
    RegionAppearanceHashInput, RegionBinding, RegionDefinition, RegionMapping, RegionOrientation,
    RegionTopologyHashInput, RenderSettings, SamplingPolicy, SheetFraming, SolidChannelValues,
    TRIM_SHEET_DOCUMENT_SCHEMA_VERSION, TopologyHashInputs, TopologyKind, TopologySnapshot,
    TreatmentLayer, TreatmentParameter, TrimSheetChange, TrimSheetDocument,
    TrimSheetDocumentCommand, TrimSheetDocumentError, TrimSheetId, UvFitKind, UvFitPolicy,
    WARP_OPERATION_VERSION, WarpOperation,
};
pub use error::{DomainError, ErrorCode, UserFacingError};
pub use id::{LayerId, LayoutId, MapId, PatchId, ProjectId, RegionId, SourceId, SourceSetId};
pub use layout::{
    AutoPackSettings, DecorationBinding, FillBehavior, FixedRegionSize, IdColor, Layout,
    LayoutContractError, LayoutItem, LayoutKind, LayoutOrder, LayoutPreset, LayoutRegion,
    LayoutRequest, LayoutSettings, MAX_LAYOUT_EDGE, MAX_LAYOUT_PIXELS, MAX_LAYOUT_REGIONS,
    MAX_REGION_INSET, MAX_REGION_KEY_BYTES, MAX_SOURCE_LAYER_WARPS, NormalizedBounds, PackPriority,
    PixelBounds, PixelSize, RegionConstraints, RegionFill, RegionLocks, RegionSourceLayer,
    SimpleDataInput, SlotBinding, SourceBlend, SourceFraming, SourceFramingMode, SourceLayerError,
    SourceMapping, SourceRectification, SourceRectificationMode, SourceSampling,
    SourceSamplingMode, SourceWarp, StyleRecipe, TemplateIdentity, TemplateLayoutContract,
    TemplateSnapshot, TrimAxis, TrimCaps,
};
pub use patch::{
    MapParticipation, Patch, PatchCommand, PatchCommandError, PatchEditOutcome, PatchGeometry,
    PatchProperties, PatchSet, RectificationSettings, RepeatMode,
};
pub use protocol::{FoundationStatusRequest, IPC_PROTOCOL_VERSION};
pub use templates::{
    CANONICAL_TEMPLATE_EDGE, CanonicalRect, Hotspot, RadialParameters, StructuralProfile,
    TemplateDefinition, TemplateRegistry, TemplateRegistryError, TemplateSlot, TemplateSlotRole,
    TemplatePackingIntent, TemplateSourceAddressMode, TemplateSourceMapping, TemplateSourceRect,
    WorldPlacement,
};
pub use units::{NormalizedPoint, NormalizedScalar};
