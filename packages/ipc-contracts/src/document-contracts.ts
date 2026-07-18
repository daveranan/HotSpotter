export const IPC_PROTOCOL_VERSION = 2 as const;

export type EngineAvailability =
  | { state: "unsupported_stage"; stage: number; diagnosticCode: "unsupported_stage" }
  | { state: "available"; compilerVersion: string };

export interface AlgorithmJobHeader {
  protocolVersion: typeof IPC_PROTOCOL_VERSION;
  revision: number;
  seed: number;
  requestKey: string;
}

export type AlgorithmJobEvent =
  | { type: "started"; header: AlgorithmJobHeader }
  | { type: "stage"; stage: number; state: "executed" | "pass_through" | "skipped_because_unused" | "failed_with_recovery" }
  | { type: "cancelled"; revision: number }
  | { type: "superseded"; revision: number; currentRevision: number }
  | { type: "completed"; revision: number; reportKey: string };

export type SourceChannel =
  | "base_color" | "normal" | "height" | "roughness" | "metallic"
  | "ambient_occlusion" | "specular" | "opacity" | "edge_mask" | "material_id";
export type ChannelInterpretation = "color_managed_base_color" | "tangent_space_normal" | "linear_scalar"
  | "linear_opacity" | "binary_mask" | "categorical_id";
export type NormalConvention = "not_applicable" | "open_gl" | "direct_x" | "unspecified";
export type SourceOwnership = "owned_copy" | "verified_external_reference";
export type AssignmentProvenance = "user_assigned" | "filename_suggested" | "embedded_metadata";

export interface PixelSize { width: number; height: number }
export interface PixelBounds { x: number; y: number; width: number; height: number }
export interface NormalizedBounds { x: number; y: number; width: number; height: number }
export interface NormalizedPoint { x: number; y: number }

export type ContentReference =
  | { type: "inherit_primary_material" }
  | { type: "material_source"; id: string }
  | { type: "patch"; id: string }
  | { type: "procedural"; id: string }
  | { type: "solid"; id: unknown };

export interface RegionMapping {
  projection: { type: "crop"; bounds: NormalizedBounds; focus: NormalizedPoint }
    | { type: "perspective"; quad: readonly NormalizedPoint[] };
  sourceCropIntent?: "unplaced" | "authored";
  warps: readonly unknown[];
  radial?: { centerX: number; centerY: number; innerRadius: number; outerRadius: number; falloff: number };
  transform: {
    scale: readonly [number, number]; rotationDegrees: number;
    mirrorX: boolean; mirrorY: boolean; offset: readonly [number, number];
  };
  addressMode: "clamp" | "repeat" | "mirrored_repeat";
}

export interface RegionBinding {
  regionId: string;
  content: ContentReference;
  mapping: RegionMapping;
}

export interface RegionDefinition {
  id: string;
  displayName: string;
  allocationRect: PixelBounds;
  hotspotRect: PixelBounds;
  role: string;
  orientation: string;
  structuralProfile: string;
  materialGroup: string;
  weatheringGroup: string;
  enabled: boolean;
  gridRect?: { x: number; y: number; width: number; height: number };
}

export interface AuthoredLayoutPresetRegion {
  presetRegionKey: string;
  displayName: string;
  gridRect: { x: number; y: number; width: number; height: number };
  role: string;
  orientation: string;
  uvFit: unknown;
  structuralProfile: string;
}

export interface AuthoredLayoutPreset {
  presetId: string;
  schemaVersion: number;
  name: string;
  logicalGrid: { schemaVersion: number; width: number; height: number };
  canonicalAspect: readonly [number, number];
  regions: readonly AuthoredLayoutPresetRegion[];
  provenance: string;
}

export interface SourceFrame {
  schemaVersion: number;
  sourceSetId: string;
  bounds: NormalizedBounds;
  orientedDimensions: PixelSize;
  sourceRevision: number;
  outputAspect: readonly [number, number];
  identity: readonly number[];
}

export interface FamilyQuota {
  count: number;
  areaShareMilli: number;
  minimumWidth: number;
  minimumHeight: number;
  maximumWidth: number;
  maximumHeight: number;
  minimumAspectMilli: number;
  maximumAspectMilli: number;
  subdivisionBudget: number;
}
export interface StripQuota { count: number; minimumThickness: number; maximumThickness: number; }
export interface RadialQuota { count: number; allocationMinDiameter: number; allocationMaxDiameter: number; }
export interface CompositionProfile {
  profileId: string;
  version: number;
  broadPanels: FamilyQuota;
  mediumBlocks: FamilyQuota;
  horizontalStrips: StripQuota;
  verticalStrips: StripQuota;
  smallDetails: FamilyQuota;
  microStrips: StripQuota;
  radialReservations: RadialQuota;
}
export type MacroStyle = "mixed_hierarchy" | "panel_cascade" | "horizontal_trims" | "vertical_trims" | "facade_halving" | "classic_source_hotspot" | "classic_hotspot_basis" | "mechanical_radial";
export type RecursivePolicy = "cascade" | "balanced";
export type SymmetryTransform = "identity" | "rotate90" | "rotate180" | "rotate270" | "mirror_x" | "mirror_y" | "mirror_diagonal" | "mirror_anti_diagonal";
export type SplitRatio = "half" | "one_third" | "two_third";
export type AspectClass = "square" | "wide2" | "tall2" | "wide4" | "tall4" | "wide8" | "tall8" | "wide16" | "tall16";
export interface HierarchicalLayoutRecipe {
  schemaVersion: number;
  macroStyle: MacroStyle;
  recursivePolicy: RecursivePolicy;
  targetRegionMin: number;
  targetRegionMax: number;
  largeShareMilli: number;
  mediumShareMilli: number;
  smallShareMilli: number;
  stripShareMilli: number;
  radialShareMilli: number;
  macroParentCount: number;
  protectedParentCount: number;
  subdividableParentCount: number;
  hierarchyDepth: number;
  scaleFalloffMilli: number;
  allowedSplitRatios: SplitRatio[];
  alignmentStrengthMilli: number;
  variationMilli: number;
  horizontalStripWeightMilli: number;
  verticalStripWeightMilli: number;
  stripThicknessLadder: number[];
  radialCount: number;
  radialMinDiameter: number;
  radialMaxDiameter: number;
  majorAspects: AspectClass[];
  mediumAspects: AspectClass[];
  detailAspects: AspectClass[];
  symmetry: SymmetryTransform;
}
export interface PartitionRecipe {
  schemaVersion: number;
  recipeId: string;
  recipeVersion: number;
  grid: { schemaVersion: number; width: number; height: number };
  targetRegionCount: number;
  seed: number;
  horizontalSplitBiasMilli: number;
  verticalSplitBiasMilli: number;
  varianceMilli: number;
  minimumLogicalWidth: number;
  minimumLogicalHeight: number;
  minimumAspectMilli: number;
  maximumAspectMilli: number;
  workLimit: number;
  depthLimit: number;
  composition: CompositionProfile;
  /** Missing means the version-2 Legacy Reserve + Remainder generator. */
  hierarchical?: HierarchicalLayoutRecipe;
}

export type MappingOrigin = "partition" | "explicit_override";
export interface RegionSourceOverride {
  schemaVersion: number;
  sourceBounds: NormalizedBounds;
  identity: readonly number[];
}

export interface TrimSheetDocument {
  id: string;
  documentRevision: number;
  topologyRevision: number;
  appearanceRevision: number;
  topology: {
    kind: string;
    topologyHash: readonly number[];
    compatibilityKey: string;
    regions: readonly RegionDefinition[];
  };
  primaryMaterial: string | null;
  materials: readonly { id: string; name: string; maps: readonly { kind: string; sha256: string }[] }[];
  regionBindings: Record<string, RegionBinding>;
  renderSettings: { outputSize: PixelSize; rendererVersion: string };
  sourceFrame?: SourceFrame;
  logicalGrid?: { schemaVersion: number; width: number; height: number };
  partitionProvenance?: unknown;
  authoredLayoutPreset?: AuthoredLayoutPreset;
  authoredLayoutInstanceId?: string;
  sourceOverrides?: Record<string, RegionSourceOverride>;
}

export interface SourceProjection {
  id: string;
  channel: SourceChannel;
  displayName: string;
  original: { path: string; immutableDigest: string; encodedBytes: number };
  storage: { ownership: SourceOwnership; externalPath: string | null };
  orientedSize: PixelSize;
  orientation: number;
  interpretation: ChannelInterpretation;
  normalConvention: NormalConvention;
  assignmentProvenance: AssignmentProvenance;
  confidenceMilli: number;
  thumbnailDataUrl: string;
}

export interface RegisteredChannelSetProjection {
  orientedSize: PixelSize;
  orientation: number;
  channels: readonly SourceProjection[];
}

export type DelightingPassThroughReason =
  | "default_new_or_unclassified"
  | "authored_texture_or_pbr_set"
  | "user_disabled";

export type DelightingRadius =
  | { pixels: number }
  | { relative_basis_points: number }
  | { physical_millimeters: { millimeters_milli: number; pixels_per_meter_milli: number } };

export type DelightingRouteIntent =
  | { route: "pass_through"; reason: DelightingPassThroughReason }
  | { route: "classical_low_frequency" }
  | { route: "local_intrinsic_provider"; provider_id: string; fallback: "none" | "pass_through" | "classical_low_frequency" };

export interface DelightingIntent {
  route: DelightingRouteIntent;
  classical: {
    strengthMilli: number;
    shadowRecoveryMilli: number;
    highlightRecoveryMilli: number;
    colorPreservationMilli: number;
    edgePreservationMilli: number;
    radius: DelightingRadius;
    analyzeMasks: boolean;
  };
}

export type MaterialBehaviorClass =
  | "already_tileable"
  | "stochastic_isotropic"
  | "stochastic_directional"
  | "periodic_lattice_structured"
  | "layered_banded"
  | "organic_directional"
  | "manufactured_pattern"
  | "unique_detail"
  | "radial_detail"
  | "mixed_unknown";

export interface MaterialClassificationIntent {
  overrideClass: MaterialBehaviorClass | null;
}

export type MaterialClassificationCommand =
  | { command: "override"; class: MaterialBehaviorClass }
  | { command: "reset_to_analysis" };

export type ScaleProvenance = "imported" | "user_measured" | "motif_derived" | "convention" | "prior_estimated" | "relative_only";
export interface MaterialCalibrationIntent {
  scale: {
    sourcePixelsPerMeterXMilli: number | null;
    sourcePixelsPerMeterYMilli: number | null;
    provenance: ScaleProvenance;
    confidenceMilli: number;
    worldScale: "available" | "unavailable_prior_estimate" | "unavailable_relative_only";
  };
  orientationOverrideMillidegrees: number | null;
  revision: number;
}
export type MaterialCalibrationCommand =
  | { command: "set_imported_metadata"; source_pixels_per_meter_x_milli: number; source_pixels_per_meter_y_milli: number; confidence_milli: number }
  | { command: "measure_two_points"; start: { x: number; y: number }; end: { x: number; y: number }; distance_micrometers: number }
  | { command: "set_known_motif_size"; motif_width_pixels_milli: number; motif_height_pixels_milli: number; motif_width_micrometers: number; motif_height_micrometers: number; confidence_milli: number }
  | { command: "override_scale"; source_pixels_per_meter_x_milli: number | null; source_pixels_per_meter_y_milli: number | null; provenance: ScaleProvenance; confidence_milli: number }
  | { command: "reset_scale" }
  | { command: "override_orientation"; axis_millidegrees: number }
  | { command: "reset_orientation" };

export interface MaterialSourceProjection {
  id: string;
  name: string;
  exemplarGroup: string | null;
  sourceRevision: number;
  registrationDigest: string;
  delighting: DelightingIntent;
  classification: MaterialClassificationIntent;
  calibration: MaterialCalibrationIntent;
  registeredChannels: RegisteredChannelSetProjection | null;
}

export interface SetExemplarGroupRequest {
  protocolVersion: typeof IPC_PROTOCOL_VERSION;
  materialSourceId: string;
  exemplarGroup: string | null;
}

export interface MaterialClassificationCommandRequest {
  protocolVersion: typeof IPC_PROTOCOL_VERSION;
  materialSourceId: string;
  classificationCommand: MaterialClassificationCommand;
}

export interface MaterialCalibrationCommandRequest {
  protocolVersion: typeof IPC_PROTOCOL_VERSION;
  materialSourceId: string;
  calibrationCommand: MaterialCalibrationCommand;
}

export interface ProjectProjection {
  id: string;
  name: string;
  path: string;
  schemaVersion: number;
  dirty: boolean;
  isDraft: boolean;
  materialSources: readonly MaterialSourceProjection[];
  patches: readonly Patch[];
  document: TrimSheetDocument | null;
  legacyLayoutDiscarded: boolean;
  canUndoDocument: boolean;
  canRedoDocument: boolean;
  canUndoPatch: boolean;
  canRedoPatch: boolean;
}

export interface PatchGeometry { corners: readonly [NormalizedPoint, NormalizedPoint, NormalizedPoint, NormalizedPoint]; assistanceMask?: readonly NormalizedPoint[] }
export interface Patch {
  id: string;
  sourceId: string;
  name: string;
  enabled: boolean;
  geometry: PatchGeometry;
  properties: { repeatMode: string; trimCap: boolean; paddingPx: number; bleedPx: number; materialId?: number; mapParticipation: string };
  rectification: { aspectRatio?: number; scale: number };
}

export type PatchCommand =
  | { type: "create"; patch: Patch; index?: number }
  | { type: "replace_geometry"; patchId: string; geometry: PatchGeometry }
  | { type: "rename"; patchId: string; name: string }
  | { type: "set_enabled"; patchId: string; enabled: boolean }
  | { type: "delete"; patchId: string };

export interface RecentProject {
  name: string;
  path: string;
  lastOpenedUnix: number;
  available: boolean;
}

export interface ResolvedRegion {
  regionId: string;
  displayName: string;
  allocationBounds: PixelBounds;
  hotspotBounds: PixelBounds;
  idColor: readonly [number, number, number];
  materialId: string;
  materialIdColor: readonly [number, number, number];
  mapping: RegionMapping;
  role: string;
  gridRect?: { x: number; y: number; width: number; height: number };
  sourceCrop?: PixelBounds;
  sourceBounds?: NormalizedBounds;
  mappingOrigin?: MappingOrigin;
}

export type CompiledMapView =
  | "baseColor" | "normal" | "height" | "roughness" | "metallic"
  | "ambientOcclusion" | "regionId" | "materialId";

export interface CompiledSheetProjection {
  documentRevision: number;
  topologyHash: string;
  appearanceHash: string;
  rendererVersion: string;
  width: number;
  height: number;
  maps: Record<CompiledMapView, string>;
  regions: readonly ResolvedRegion[];
}

export interface Stage14SlotProjection {
  regionId: string;
  slotKey: string;
  displayName: string;
  allocationBounds: PixelBounds;
  hotspotBounds: PixelBounds;
  mappingMode: string;
  sourceTransform: { rotation: string; mirror: string };
  isotropicScale: number;
  samplingScale: number;
  validity: string;
  correspondence: string;
  sourceId: string;
  patchId?: string;
  domainId: string;
  candidateId: string;
  samplingPlanId: string;
  stage14ResultId: string;
  sourceCrop?: PixelBounds;
  sourceBounds?: NormalizedBounds;
  mappingOrigin?: MappingOrigin;
  gridRect?: { x: number; y: number; width: number; height: number };
}

export interface IntermediateAtlasProjection {
  label: "Intermediate Stage 14 material-placement preview";
  nonExportable: true;
  incompleteAfterStage: 14;
  revision: number;
  documentRevision: number;
  topologyHash: string;
  appearanceHash: string;
  rendererVersion: "intermediate-stage-14";
  width: number;
  height: number;
  topology: unknown;
  placementPlanId: string;
  maps: Partial<Record<CompiledMapView, string>>;
  regions: readonly ResolvedRegion[];
  unavailableChannels: readonly string[];
  slots: readonly Stage14SlotProjection[];
  pending: readonly string[];
  telemetry: readonly string[];
  finalCompileAvailable: false;
  exportAvailable: false;
  blenderAvailable: false;
  sourceFrame?: SourceFrame;
}

/** Preview work profile; this changes requested output work only, never SourceFrame ownership. */
export type SourceFramePreviewProfile = "draft512" | "refinement1024" | "authoritative";

export interface Stage14PreviewRequest {
  protocolVersion: number;
  revision: number;
  regionId?: string;
  transientProjection?: RegionMapping["projection"];
  draftId?: number;
  inputHash?: string;
  profile?: SourceFramePreviewProfile;
}

export interface PreviewSheetProjection {
  draftId: number;
  documentRevision: number;
  topologyHash: string;
  appearanceHash: string;
  width: number;
  height: number;
  mapView: CompiledMapView;
  dataUrl: string;
  regions: readonly ResolvedRegion[];
}

export type TrimSheetDocumentCommand =
  | { type: "apply_authored_layout_preset"; preset: AuthoredLayoutPreset; instanceId: string }
  | { type: "accept_source_frame_partition"; recipe: PartitionRecipe }
  | { type: "split_source_frame_region"; regionId: string; axis: "horizontal" | "vertical" }
  | { type: "merge_source_frame_regions"; regionId: string; siblingId: string }
  | { type: "move_source_frame_boundary"; regionId: string; axis: "horizontal" | "vertical"; coordinate: number }
  | { type: "draw_source_frame_region"; gridRect: { x: number; y: number; width: number; height: number } }
  | { type: "resize_source_frame_region"; regionId: string; gridRect: { x: number; y: number; width: number; height: number } }
  | { type: "set_primary_material"; materialId: string }
  | { type: "set_region_content"; regionId: string; content: ContentReference }
  | { type: "set_sheet_framing"; framing: unknown }
  | { type: "set_region_projection"; regionId: string; projection: RegionMapping["projection"] }
  | { type: "set_region_radial"; regionId: string; radial: NonNullable<RegionMapping["radial"]> }
  | { type: "set_output_resolution"; outputSize: PixelSize }
  | { type: "set_source_frame"; bounds: NormalizedBounds }
  | { type: "detach_source_cell"; regionId: string }
  | { type: "reset_source_cell"; regionId: string };

export interface CommandFailure {
  code: string;
  message: string;
  recovery: string;
  detail?: string;
}
