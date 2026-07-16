export const IPC_PROTOCOL_VERSION = 1 as const;

export interface FoundationStatusRequest {
  protocolVersion: typeof IPC_PROTOCOL_VERSION;
}

export interface NativeDirectories {
  appData: string;
  cache: string;
  logs: string;
  recovery: string;
}

export interface FoundationStatus {
  protocolVersion: typeof IPC_PROTOCOL_VERSION;
  appVersion: string;
  platform: string;
  directories: NativeDirectories;
  capabilities: readonly ["native_paths", "typed_ipc", "structured_diagnostics", "native_dialog"];
}

export interface StartupStatus {
  previousShutdownClean: boolean;
}

export interface CommandFailure {
  code: string;
  message: string;
  recovery: string;
  detail?: string;
}

export type SourceOwnership = "owned_copy" | "verified_external_reference";
export type SourceChannel =
  | "base_color"
  | "normal"
  | "height"
  | "roughness"
  | "metallic"
  | "ambient_occlusion"
  | "specular"
  | "opacity"
  | "edge_mask"
  | "material_id";

export interface ProjectPathRequest {
  protocolVersion: typeof IPC_PROTOCOL_VERSION;
  path: string;
}

export interface CreateProjectRequest extends ProjectPathRequest {
  name: string;
}

export interface ProjectNameRequest extends FoundationStatusRequest {
  name: string;
}

export interface ImportSourceRequest extends ProjectPathRequest {
  ownership: SourceOwnership;
  channel: SourceChannel;
  sourceSetId: string;
}

export interface SourceSlotRequest extends FoundationStatusRequest {
  channel: SourceChannel;
  sourceSetId: string;
}

export interface CloseProjectRequest extends FoundationStatusRequest {
  disposition: "save" | "discard";
}

export interface RecoverProjectRequest extends FoundationStatusRequest {
  recoveryPath: string;
  destinationPath: string;
}

export interface ThumbnailMipmap {
  maxEdge: number;
  dataUrl: string;
}

export interface SourceSnapshot {
  id: string;
  sourceSetId: string;
  channel: SourceChannel;
  ownership: SourceOwnership;
  displayName: string;
  sourcePath: string;
  width: number;
  height: number;
  format: "PNG" | "JPEG" | "TIFF";
  colorType: string;
  hasAlpha: boolean;
  exifOrientation: number;
  hasEmbeddedIccProfile: boolean;
  iccConvertedToSrgb: boolean;
  encodedBytes: number;
  thumbnailDataUrl: string;
  thumbnailMipmaps: ThumbnailMipmap[];
}

export interface SourceSetSnapshot {
  id: string;
  name: string;
}

export interface NormalizedPoint {
  x: number;
  y: number;
}

export interface PatchGeometry {
  corners: [NormalizedPoint, NormalizedPoint, NormalizedPoint, NormalizedPoint];
  assistanceMask?: NormalizedPoint[];
}

export type RepeatMode = "repeat_x" | "repeat_y" | "tile_xy" | "stretch" | "unique";
export type MapParticipation = "all" | "base_color_only" | "excluded";

export interface PatchProperties {
  repeatMode: RepeatMode;
  trimCap: boolean;
  paddingPx: number;
  bleedPx: number;
  materialId?: number;
  mapParticipation: MapParticipation;
}

export interface RectificationSettings {
  aspectRatio?: number;
  scale: number;
}

export interface PatchSnapshot {
  id: string;
  sourceId: string;
  name: string;
  enabled: boolean;
  geometry: PatchGeometry;
  properties: PatchProperties;
  rectification: RectificationSettings;
}

export type PatchCommand =
  | { type: "create"; patch: PatchSnapshot; index?: number }
  | { type: "replace_geometry"; patchId: string; geometry: PatchGeometry }
  | { type: "rename"; patchId: string; name: string }
  | { type: "set_enabled"; patchId: string; enabled: boolean }
  | { type: "set_properties"; patchId: string; properties: PatchProperties }
  | { type: "set_rectification"; patchId: string; settings: RectificationSettings }
  | { type: "duplicate"; patchId: string; newId: string; name: string; index?: number }
  | { type: "reorder"; patchId: string; toIndex: number }
  | { type: "reassign_source"; fromSourceId: string; toSourceId: string }
  | { type: "delete"; patchId: string };

export interface PatchCommandRequest extends FoundationStatusRequest {
  command: PatchCommand;
  coalescingGroup?: number;
}

export interface PolygonAssistRequest extends FoundationStatusRequest {
  points: NormalizedPoint[];
  retainMask: boolean;
}

export interface PatchPreviewRequest extends FoundationStatusRequest {
  patchId: string;
  maxEdge: number;
}

export interface DraftPatchPreviewRequest extends FoundationStatusRequest {
  previewId: string;
  sourceId: string;
  geometry: PatchGeometry;
  rectification: RectificationSettings;
  maxEdge: number;
}

export interface PatchPreviewSnapshot {
  patchId: string;
  width: number;
  height: number;
  dataUrl: string;
}

export interface PatchPreviewProgress {
  patchId: string;
  stage: string;
  fraction: number;
}

export interface PatchStateSnapshot {
  patches: PatchSnapshot[];
  dirty: boolean;
  authoringRevision: number;
  canUndoPatch: boolean;
  canRedoPatch: boolean;
  canUndoProject: boolean;
  canRedoProject: boolean;
  warnings: ProjectWarning[];
}

export interface PixelSize {
  width: number;
  height: number;
}

export interface PixelBounds extends PixelSize {
  x: number;
  y: number;
}

export type LayoutPreset = "balanced" | "horizontal_trims" | "vertical_trims" | "modular_kit" | "atlas";
export type LayoutKind = "template" | "custom_atlas";

export interface TemplateIdentity {
  templateId: string;
  templateVersion: string;
  compatibilityKey: string;
}

export interface TemplateSnapshot {
  identity: TemplateIdentity;
  schemaVersion: number;
  canonicalWidth: number;
  canonicalHeight: number;
  snapshotJson: string;
  snapshotHash: string;
}

export interface SlotBinding {
  slotKey: string;
  itemKey: string;
  regionId: string;
  idColor: [number, number, number];
}

export interface DecorationBinding {
  decorationKey: string;
  value: string;
}

export interface StyleRecipe {
  recipeKey: string;
  decorations: DecorationBinding[];
}

export interface TemplateLayoutContract {
  snapshot?: TemplateSnapshot;
  slotBindings: SlotBinding[];
  styleRecipe: StyleRecipe;
  sourceFraming?: TemplateSourceTransform;
}
export type LayoutOrder = "input" | "largest_first" | "horizontal_first" | "vertical_first";
export type PackPriority = "balanced" | "horizontal_strips" | "vertical_strips";
export type FillBehavior = "horizontal_loop" | "vertical_loop" | "tile" | "stretch" | "unique_detail" | "trim_cap";
export type TrimAxis = "horizontal" | "vertical";

export type RegionFill =
  | { type: "whole_source_set"; sourceSetId: string }
  | { type: "rectified_patch"; sourceSetId: string; patchId: string }
  | { type: "simple_color"; rgba: [number, number, number, number] }
  | { type: "simple_data"; input: { channel: SourceChannel; value: number } };

export interface LayoutSettings {
  output: PixelSize;
  paddingPx: number;
  bleedPx: number;
  order: LayoutOrder;
  autoPack: { enabled: boolean; priority: PackPriority; seed: number };
  fixedSelectedSize?: { regionId: string; size: PixelSize };
}

export interface LayoutItem {
  key: string;
  fill: RegionFill;
  behavior: FillBehavior;
  trimCaps?: { axis: TrimAxis; leadingPx: number; trailingPx: number };
  naturalSize: PixelSize;
  enabled: boolean;
  participates: boolean;
  constraints: {
    fixedWidthPx?: number;
    fixedHeightPx?: number;
    templateBounds?: { x: number; y: number; width: number; height: number };
  };
  paddingPx?: number;
  bleedPx?: number;
  regionId?: string;
}

export interface LayoutRegion {
  id: string;
  itemKey: string;
  fill: RegionFill;
  behavior: FillBehavior;
  trimCaps?: { axis: TrimAxis; leadingPx: number; trailingPx: number };
  bounds: PixelBounds;
  paddingPx: number;
  bleedPx: number;
  orderIndex: number;
  locks: { position: boolean; width: boolean; height: boolean };
  idColor: [number, number, number];
}

export interface LayoutSnapshot {
  id: string;
  preset: LayoutPreset;
  settings: LayoutSettings;
  regions: LayoutRegion[];
}

export interface StoredLayout {
  layout: LayoutSnapshot;
  items: LayoutItem[];
  layoutKind: LayoutKind;
  template?: TemplateLayoutContract;
  /** Missing entries use the versioned whole-source default. */
  sourceLayers: Record<string, RegionSourceLayer>;
}

export type SourceMapping =
  | { type: "whole_source" }
  | { type: "bounds"; bounds: { x: number; y: number; width: number; height: number } }
  | { type: "perspective"; quad: [NormalizedPoint, NormalizedPoint, NormalizedPoint, NormalizedPoint] };

export type SourceRectificationMode = "none" | "perspective";
export type SourceSamplingMode = "nearest" | "linear" | "cubic";
export type SourceBlend = "replace" | "normal" | "multiply" | "overlay";

export type SourceWarp =
  | { type: "planar"; scaleX: number; scaleY: number; offsetX: number; offsetY: number }
  | { type: "perspective"; strength: number }
  | { type: "polar"; centerX: number; centerY: number; radius: number }
  | { type: "spiral_twirl"; centerX: number; centerY: number; radius: number; strength: number; iterations: number }
  | { type: "radial_lens"; centerX: number; centerY: number; radius: number; strength: number }
  | { type: "cylindrical_arc"; radius: number; arcDegrees: number };

/** Executable source UV recipe. Warp order is exactly the array order. */
export interface RegionSourceLayer {
  version: number;
  mapping: SourceMapping;
  rectification: { mode: SourceRectificationMode; maxIntermediateEdge: number };
  sampling: { mode: SourceSamplingMode; scale: number };
  rotationDegrees: number;
  mirrorX: boolean;
  mirrorY: boolean;
  blend: SourceBlend;
  opacity: number;
  variationOffset: [number, number];
  warps: SourceWarp[];
}

export interface LayoutRequest {
  layoutId: string;
  preset: LayoutPreset;
  settings: LayoutSettings;
  items: LayoutItem[];
  existingRegions: LayoutRegion[];
}

export type SourceFramingMode = "cover" | "stretch" | "repeat";

export interface TemplateSourceTransform {
  mode: SourceFramingMode;
  cropFocus: NormalizedPoint;
  cropBounds?: { x: number; y: number; width: number; height: number };
}

export type GenerateLayoutRequest =
  | (FoundationStatusRequest & {
    mode: "template";
    template: TemplateIdentity;
    sourceSetId: string;
    layoutId: string;
    settings: LayoutSettings;
    sourceTransform: TemplateSourceTransform;
    coalescingGroup?: number;
  })
  | (FoundationStatusRequest & {
    mode: "custom_atlas";
    request: LayoutRequest;
    coalescingGroup?: number;
  });

export type CompiledLayoutPreviewMap = "baseColor" | "height" | "normal" | "roughness" | "metallic" | "ambientOcclusion" | "regionId" | "materialId";

export interface CompiledLayoutPreview {
  width: number;
  height: number;
  dataUrl: string;
  maps?: Partial<Record<CompiledLayoutPreviewMap, string>>;
}

export interface GenerateLayoutResult {
  state: LayoutStateSnapshot;
  preview?: CompiledLayoutPreview;
}

export type LayoutCommand =
  | { type: "set_bounds"; regionId: string; bounds: PixelBounds }
  | { type: "set_fill"; regionId: string; fill: RegionFill }
  | { type: "set_source_layer"; regionId: string; sourceLayer: RegionSourceLayer }
  | { type: "set_locks"; regionId: string; locks: { position: boolean; width: boolean; height: boolean } }
  | { type: "reorder"; regionId: string; toIndex: number }
  | { type: "delete_simple"; regionId: string };

export interface LayoutCommandRequest extends FoundationStatusRequest {
  command: LayoutCommand;
  coalescingGroup?: number;
}

export interface LayoutStateSnapshot {
  layout: StoredLayout | null;
  dirty: boolean;
  authoringRevision: number;
  canUndoPatch: boolean;
  canRedoPatch: boolean;
  canUndoProject: boolean;
  canRedoProject: boolean;
  warnings: ProjectWarning[];
}

export interface AuthoringHistorySnapshot extends PatchStateSnapshot, LayoutStateSnapshot {}

export interface ProjectSnapshot {
  id: string;
  name: string;
  path: string;
  schemaVersion: number;
  dirty: boolean;
  staleLockRecovered: boolean;
  isDraft: boolean;
  authoringRevision: number;
  sources: SourceSnapshot[];
  sourceSets: SourceSetSnapshot[];
  patches: PatchSnapshot[];
  layout: StoredLayout | null;
  canUndoPatch: boolean;
  canRedoPatch: boolean;
  canUndoProject: boolean;
  canRedoProject: boolean;
  warnings: ProjectWarning[];
}

export interface ProjectWarning {
  code: string;
  message: string;
  recovery: string;
}

export interface RecentProject {
  name: string;
  path: string;
  lastOpenedUnix: number;
  available: boolean;
}

export interface RecoveryCandidate {
  projectId: string;
  projectName: string;
  path: string;
  modifiedUnix: number;
  sourceCount: number;
}
