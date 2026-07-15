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
  | "ambient_occlusion";

export interface ProjectPathRequest {
  protocolVersion: typeof IPC_PROTOCOL_VERSION;
  path: string;
}

export interface CreateProjectRequest extends ProjectPathRequest {
  name: string;
}

export interface ImportSourceRequest extends ProjectPathRequest {
  ownership: SourceOwnership;
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

export interface ProjectSnapshot {
  id: string;
  name: string;
  path: string;
  schemaVersion: number;
  dirty: boolean;
  staleLockRecovered: boolean;
  sources: SourceSnapshot[];
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
