mod algorithm_stack;
mod channel;
mod document;
mod error;
mod id;
mod layout;
mod material_source;
mod patch;
mod protocol;
mod source_frame;
mod templates;
mod units;

pub use algorithm_stack::*;
pub use channel::{Channel, ChannelDataKind};
pub use document::{
    AUTHORED_LAYOUT_PRESET_SCHEMA_VERSION, REGION_BEHAVIOR_VERSION, AcceptedTopology, AddressMode, AppearanceHashInputs,
    AuthoredLayoutPreset, AuthoredLayoutPresetRegion, BlendPolicy, ChangeClassification,
    ChannelBitDepth, ChannelRenderPolicy, ContentReference, DocumentHash, FitAxis,
    GeneratorProvenance, MAX_MAPPING_MAGNITUDE, MappingTransform, MaterialMapContent,
    MaterialMapKind, MaterialSourceSet, PartitionAxis, ProceduralMaterial, Projection, QuarterTurn,
    EdgeEligibility, ManualRegionRole, RadialMappingSettings, RegionAppearanceHashInput,
    RegionBehavior, RegionBinding, RegionContinuity, RegionDefinition, RegionMapping,
    RegionOrientation, RegionSampling, RegionTopologyHashInput, RenderSettings, SamplingPolicy,
    SheetFraming, SolidChannelValues, SourceCropIntent, TRIM_SHEET_DOCUMENT_SCHEMA_VERSION,
    TopologyHashInputs, TopologyKind, TopologySnapshot, TreatmentLayer, TreatmentParameter,
    TrimSheetChange, TrimSheetDocument, TrimSheetDocumentCommand, TrimSheetDocumentError,
    TrimSheetId, UvFitKind, UvFitPolicy, WARP_OPERATION_VERSION, WarpOperation,
    diagonal_cascade_authored_preset, new_blank_authored_preset, validate_authored_layout_preset,
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
pub use material_source::{
    AssignmentProvenance, ChannelInterpretation, ChannelRegistration, ClassicalDelightingSettings,
    DelightingIntent, DelightingPassThroughReason, DelightingRadius, DelightingRouteIntent,
    IntrinsicProviderFallback, MaterialBehaviorClass, MaterialCalibrationCommand,
    MaterialCalibrationError, MaterialCalibrationIntent, MaterialChannelRole,
    MaterialClassificationCommand, MaterialClassificationIntent, MaterialSource, NormalConvention,
    OrientedPixelSize, OriginalAssetProvenance, PhysicalScaleEvidence, RegisteredChannel,
    RegisteredChannelSet, RegistrationDiagnostic, RegistrationDiagnosticCode,
    RegistrationRecoveryChoice, ScaleProvenance, SourceOwnershipIntent, SourcePixelPointMilli,
    WorldScaleAvailability,
};
pub use patch::{
    MapParticipation, Patch, PatchCommand, PatchCommandError, PatchEditOutcome, PatchGeometry,
    PatchProperties, PatchSet, RectificationSettings, RepeatMode,
};
pub use protocol::{FoundationStatusRequest, IPC_PROTOCOL_VERSION};
pub use source_frame::{
    AspectClass, CompositionProfile, FamilyQuota, GridRect, HierarchicalLayoutRecipe,
    HierarchyZone, LOGICAL_GRID_SCHEMA_VERSION, LogicalGridSpec, MAX_LOGICAL_GRID_EDGE,
    MAX_PARTITION_REGIONS, MacroStyle, MappingOrigin, PARTITION_RECIPE_SCHEMA_VERSION,
    PartitionError, PartitionFamily, PartitionLineage, PartitionProvenance, PartitionRecipe,
    PartitionRegion, PartitionTreeNode, RadialQuota, RecursivePolicy, RegionSourceOverride,
    SOURCE_FRAME_SCHEMA_VERSION, SourceFrame, SplitRatio, StripQuota, generate_partition,
    region_id as source_frame_region_id, resolve_boundaries,
};
pub use templates::{
    CANONICAL_TEMPLATE_EDGE, CanonicalRect, CompiledTemplateSlot, CompiledTemplateTopology,
    RadialParameters, StructuralProfile, TemplateCompatibilityDiagnostic, TemplateDefinition,
    TemplateFitSemantics, TemplateRegistry, TemplateRegistryError, TemplateSlot, TemplateSlotRole,
    TemplateSourceAddressMode, TemplateSourceMapping, TemplateSourceRect, WeightedTemplateGrammar,
    WorldPlacement, compile_weighted_grammar,
};
pub use units::{NormalizedPoint, NormalizedScalar};
