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
}

export interface SourceSlotRequest extends FoundationStatusRequest {
  channel: SourceChannel;
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
  canUndoPatch: boolean;
  canRedoPatch: boolean;
  warnings: ProjectWarning[];
}

export interface ProjectSnapshot {
  id: string;
  name: string;
  path: string;
  schemaVersion: number;
  dirty: boolean;
  staleLockRecovered: boolean;
  sources: SourceSnapshot[];
  patches: PatchSnapshot[];
  canUndoPatch: boolean;
  canRedoPatch: boolean;
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
