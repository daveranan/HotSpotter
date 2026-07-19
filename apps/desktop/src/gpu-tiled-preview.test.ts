import assert from "node:assert/strict";
import test from "node:test";
import { GpuTiledPreviewPainter, gpuTiledPreviewPayloadBytes, isValidGpuTiledPreviewPayload, shouldDisplayGpuTiledPreview } from "./source-frame-preview-controller.ts";

const publication = (generation = 7) => ({
  manifest: {
    map: "baseColor",
    mipLevel: 0,
    outputRect: { x: 4, y: 8, width: 4, height: 2 },
    validRect: { x: 5, y: 8, width: 2, height: 2 },
    haloPx: 1,
    generation,
    pixelFormat: "rgba8UnormSrgb" as const,
    width: 4,
    height: 2,
    rowStride: 16,
    opaqueHandle: `tile-${generation}`,
  },
  telemetry: { generation, nativePublishMs: 1, rawIpcBytes: 32, rawIpcMs: 1 },
});

test("gpu-tiled-preview validates raw payload against declared stride and dimensions", () => {
  assert.equal(isValidGpuTiledPreviewPayload(publication(), 32), true);
  assert.equal(isValidGpuTiledPreviewPayload(publication(), 31), false);
  assert.equal(isValidGpuTiledPreviewPayload({ ...publication(), manifest: { ...publication().manifest, rowStride: 12 } }, 24), false);
});

test("gpu-tiled-preview accepts a clipped corner tile without a full-atlas payload", () => {
  const corner = publication();
  corner.manifest.outputRect = { x: 1022, y: 1022, width: 2, height: 2 };
  corner.manifest.validRect = { x: 1023, y: 1023, width: 1, height: 1 };
  corner.manifest.width = 2;
  corner.manifest.height = 2;
  corner.manifest.rowStride = 8;
  assert.equal(isValidGpuTiledPreviewPayload(corner, 16), true);
  assert.equal(isValidGpuTiledPreviewPayload(corner, 4 * 1024 * 1024), false);
});

test("gpu-tiled-preview coerces Tauri binary ArrayBuffer payloads", () => {
  const source = new Uint8Array([0, 24, 48, 255, 96, 120, 144, 255]);
  const bytes = gpuTiledPreviewPayloadBytes(source.buffer.slice(0));
  assert.equal(bytes.byteLength, source.byteLength);
  assert.deepEqual([...bytes], [...source]);
});

test("gpu-tiled-preview prefers a matching manifest over legacy image absence", () => {
  assert.equal(shouldDisplayGpuTiledPreview("baseColor", "baseColor", false), true);
  assert.equal(shouldDisplayGpuTiledPreview("base_color", "baseColor", false), true);
  assert.equal(shouldDisplayGpuTiledPreview("BaseColor", "baseColor", false), true);
  assert.equal(shouldDisplayGpuTiledPreview("baseColor", "baseColor", true), false);
  assert.equal(shouldDisplayGpuTiledPreview("baseColor", "normal", false), false);
});

test("gpu-tiled-preview rejects a stale generation and releases its cache handle", async () => {
  const painter = new GpuTiledPreviewPainter();
  painter.beginGeneration(8);
  const released: string[] = [];
  const painted = await painter.paint({} as HTMLCanvasElement, publication(7), {
    getPayload: async () => new Uint8Array(32),
    releasePayload: async (request) => { released.push(request.opaqueHandle); },
  }, 1);
  assert.equal(painted, false);
  assert.deepEqual(released, ["tile-7"]);
});
