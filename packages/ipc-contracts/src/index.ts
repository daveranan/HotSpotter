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

export interface CommandFailure {
  code: string;
  message: string;
  recovery: string;
  detail?: string;
}

