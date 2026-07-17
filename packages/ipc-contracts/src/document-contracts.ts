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
  layoutGrid: { columns: number; rows: number; padding: number };
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

export interface MaterialSourceProjection {
  id: string;
  name: string;
  exemplarGroup: string | null;
  sourceRevision: number;
  registrationDigest: string;
  delighting: DelightingIntent;
  registeredChannels: RegisteredChannelSetProjection | null;
}

export interface SetExemplarGroupRequest {
  protocolVersion: typeof IPC_PROTOCOL_VERSION;
  materialSourceId: string;
  exemplarGroup: string | null;
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
  | { type: "set_primary_material"; materialId: string }
  | { type: "set_region_content"; regionId: string; content: ContentReference }
  | { type: "set_sheet_framing"; framing: unknown }
  | { type: "set_region_projection"; regionId: string; projection: RegionMapping["projection"] }
  | { type: "set_region_radial"; regionId: string; radial: NonNullable<RegionMapping["radial"]> }
  | { type: "set_output_resolution"; outputSize: PixelSize }
  | { type: "set_layout_grid"; settings: { columns: number; rows: number; padding: number } }
  | { type: "set_region_destination"; regionId: string; allocationRect: PixelBounds; padding: number };

export interface CommandFailure {
  code: string;
  message: string;
  recovery: string;
  detail?: string;
}
