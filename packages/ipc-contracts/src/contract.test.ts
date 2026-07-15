import assert from "node:assert/strict";
import test from "node:test";
import fixture from "../../../fixtures/contracts/foundation-status.json" with { type: "json" };
import {
  IPC_PROTOCOL_VERSION,
  type CloseProjectRequest,
  type CreateProjectRequest,
  type ImportSourceRequest,
  type ProjectSnapshot,
  type RecoverProjectRequest,
} from "./index.ts";

const requiredCapabilities = [
  "native_paths",
  "typed_ipc",
  "structured_diagnostics",
  "native_dialog",
];

test("foundation fixture uses the current protocol and complete response shape", () => {
  assert.equal(fixture.request.protocolVersion, IPC_PROTOCOL_VERSION);
  assert.equal(fixture.response.protocolVersion, IPC_PROTOCOL_VERSION);
  assert.equal(typeof fixture.response.appVersion, "string");
  assert.equal(typeof fixture.response.platform, "string");
  assert.deepEqual(fixture.response.capabilities, requiredCapabilities);
  assert.deepEqual(Object.keys(fixture.response.directories).sort(), ["appData", "cache", "logs", "recovery"]);
});

test("phase 1 project and import requests remain protocol versioned", () => {
  const create: CreateProjectRequest = {
    protocolVersion: IPC_PROTOCOL_VERSION,
    path: "<project>",
    name: "Brick",
  };
  const source: ImportSourceRequest = {
    protocolVersion: IPC_PROTOCOL_VERSION,
    path: "<source>",
    ownership: "owned_copy",
    channel: "base_color",
  };
  assert.equal(create.protocolVersion, IPC_PROTOCOL_VERSION);
  assert.equal(source.protocolVersion, IPC_PROTOCOL_VERSION);
  assert.equal(source.ownership, "owned_copy");
  assert.equal(source.channel, "base_color");
});

test("phase 1 lifecycle and snapshot contracts carry recovery and registration state", () => {
  const close: CloseProjectRequest = {
    protocolVersion: IPC_PROTOCOL_VERSION,
    disposition: "discard",
  };
  const recover: RecoverProjectRequest = {
    protocolVersion: IPC_PROTOCOL_VERSION,
    recoveryPath: "<recovery>",
    destinationPath: "<new-project>",
  };
  const snapshot: ProjectSnapshot = {
    id: "00000000-0000-0000-0000-000000000001",
    name: "Registered material",
    path: "<project>",
    schemaVersion: 2,
    dirty: true,
    staleLockRecovered: false,
    sources: [{
      id: "00000000-0000-0000-0000-000000000002",
      channel: "roughness",
      ownership: "verified_external_reference",
      displayName: "roughness.tif",
      width: 2048,
      height: 2048,
      format: "TIFF",
      colorType: "L8",
      hasAlpha: false,
      exifOrientation: 1,
      hasEmbeddedIccProfile: false,
      iccConvertedToSrgb: false,
      encodedBytes: 4096,
      thumbnailDataUrl: "data:image/png;base64,AA==",
      thumbnailMipmaps: [{ maxEdge: 320, dataUrl: "data:image/png;base64,AA==" }],
    }],
  };

  assert.equal(close.disposition, "discard");
  assert.notEqual(recover.recoveryPath, recover.destinationPath);
  assert.equal(snapshot.sources[0]?.channel, "roughness");
  assert.equal(snapshot.dirty, true);
});
