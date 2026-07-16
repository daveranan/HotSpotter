import assert from "node:assert/strict";
import test from "node:test";
import fixture from "../../../fixtures/contracts/foundation-status.json" with { type: "json" };
import lifecycle from "../../../fixtures/contracts/phase-1-lifecycle.json" with { type: "json" };
import patchAuthoring from "../../../fixtures/contracts/phase-2-patch-authoring.json" with { type: "json" };
import layoutAuthoring from "../../../fixtures/contracts/phase-3-layout-authoring.json" with { type: "json" };
import {
  IPC_PROTOCOL_VERSION,
  type AuthoringHistorySnapshot,
  type CloseProjectRequest,
  type CreateProjectRequest,
  type ImportSourceRequest,
  type GenerateLayoutRequest,
  type LayoutCommandRequest,
  type LayoutStateSnapshot,
  type PatchCommandRequest,
  type PatchPreviewRequest,
  type PatchStateSnapshot,
  type PolygonAssistRequest,
  type ProjectSnapshot,
  type RecoverProjectRequest,
  type SourceSlotRequest,
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
    sourceSetId: "00000000-0000-4000-8000-000000000001",
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
    schemaVersion: 5,
    dirty: true,
    staleLockRecovered: false,
    isDraft: false,
    authoringRevision: 0,
    warnings: [],
    patches: [],
    canUndoPatch: false,
    canRedoPatch: false,
    canUndoProject: false,
    canRedoProject: false,
    layout: null,
    sourceSets: [{ id: "00000000-0000-4000-8000-000000000001", name: "Material 1" }],
    sources: [{
      id: "00000000-0000-0000-0000-000000000002",
      sourceSetId: "00000000-0000-4000-8000-000000000001",
      channel: "roughness",
      ownership: "verified_external_reference",
      displayName: "roughness.tif",
      sourcePath: "C:/textures/roughness.tif",
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
  assert.equal(snapshot.warnings.length, 0);
});

test("phase 1 fixture covers native lifecycle shapes and expanded material slots", () => {
  const clearSpecular: SourceSlotRequest = {
    protocolVersion: IPC_PROTOCOL_VERSION,
    channel: "specular",
    sourceSetId: "00000000-0000-4000-8000-000000000001",
  };
  assert.equal(lifecycle.createRequest.protocolVersion, IPC_PROTOCOL_VERSION);
  assert.equal(lifecycle.importRequest.channel, "specular");
  assert.equal(lifecycle.closeRequest.disposition, "discard");
  assert.notEqual(lifecycle.recoverRequest.recoveryPath, lifecycle.recoverRequest.destinationPath);
  assert.equal(lifecycle.projectSnapshot.schemaVersion, 7);
  assert.equal(lifecycle.projectSnapshot.sources[0]?.channel, "specular");
  assert.equal(lifecycle.projectSnapshot.warnings[0]?.code, "recovery_failed");
  assert.equal(clearSpecular.channel, lifecycle.importRequest.channel);
});

test("phase 2 fixtures preserve camel-case patch commands, assistance, preview, and history state", () => {
  const command = patchAuthoring.patchCommandRequest as PatchCommandRequest;
  const assist = patchAuthoring.polygonAssistRequest as PolygonAssistRequest;
  const preview = patchAuthoring.previewRequest as PatchPreviewRequest;
  const state = patchAuthoring.patchState as PatchStateSnapshot;
  assert.equal(command.protocolVersion, IPC_PROTOCOL_VERSION);
  assert.equal(command.command.type, "create");
  assert.equal(command.command.patch.properties.repeatMode, "repeat_x");
  assert.equal(command.command.patch.geometry.corners.length, 4);
  assert.equal(assist.points.length, 6);
  assert.equal(assist.retainMask, true);
  assert.equal(preview.maxEdge, 768);
  assert.equal(state.canUndoPatch, true);
  assert.equal(state.canRedoPatch, false);
});

test("phase 3 fixture preserves solver intent, stable references, manual commands, and project history", () => {
  const generate = layoutAuthoring.generateRequest as GenerateLayoutRequest;
  const command = layoutAuthoring.layoutCommandRequest as LayoutCommandRequest;
  const state = layoutAuthoring.layoutState as LayoutStateSnapshot;
  const history = layoutAuthoring.historyState as AuthoringHistorySnapshot;
  assert.equal(generate.protocolVersion, IPC_PROTOCOL_VERSION);
  assert.equal(generate.request.preset, "horizontal_trims");
  assert.equal(generate.request.items[0]?.fill.type, "whole_source_set");
  assert.equal(generate.request.items[1]?.fill.type, "rectified_patch");
  assert.equal(generate.request.existingRegions[0]?.locks.position, true);
  assert.equal(command.command.type, "set_bounds");
  assert.equal(command.coalescingGroup, 72);
  assert.equal(state.canUndoProject, true);
  assert.equal(state.canUndoPatch, true);
  assert.equal(state.layout, null);
  assert.deepEqual(history.patches, []);
  assert.equal(history.layout, null);
  assert.equal(history.canUndoPatch, history.canUndoProject);
  assert.equal(history.canRedoPatch, history.canRedoProject);
});
