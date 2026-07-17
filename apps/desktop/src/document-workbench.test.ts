import assert from "node:assert/strict";
import test from "node:test";

import type { CompiledSheetProjection, TrimSheetDocumentCommand } from "@hot-trimmer/ipc-contracts";
import { adjustCrop, anchoredZoom, movePatch, normalizePatchToRectangle, resizePatch, resizePanes, rotatePatch } from "./source-workbench-geometry.ts";

test("document command wire shapes are typed and carry exact stable IDs", () => {
  const regionId = "764f7fc0-5091-47f8-878f-1e926b0c9f66";
  const materialId = "a829a53c-23cb-49f8-81cb-80e412a57386";
  const command: TrimSheetDocumentCommand = {
    type: "set_region_content",
    regionId,
    content: { type: "material_source", id: materialId },
  };
  assert.equal(command.regionId, regionId);
  assert.deepEqual(command.content, { type: "material_source", id: materialId });
});

test("compiled overlay bounds are read from the artifact rather than reconstructed", () => {
  const artifact = {
    width: 1024,
    height: 1024,
    regions: [{ allocationBounds: { x: 256, y: 128, width: 512, height: 256 } }],
  } as Pick<CompiledSheetProjection, "width" | "height" | "regions">;
  const bounds = artifact.regions[0]!.allocationBounds;
  assert.deepEqual(
    [bounds.x / artifact.width, bounds.y / artifact.height, bounds.width / artifact.width, bounds.height / artifact.height],
    [0.25, 0.125, 0.5, 0.25],
  );
});

test("source crop geometry moves and resizes inside normalized source space", () => {
  const crop = { x: 0.25, y: 0.2, width: 0.35, height: 0.3 };
  assert.deepEqual(adjustCrop(crop, "move", 0.5, -0.4), {
    x: 0.65,
    y: 0,
    width: 0.35,
    height: 0.3,
  });
  assertBoundsClose(adjustCrop(crop, "nw", -0.1, 0.05), {
    x: 0.15,
    y: 0.25,
    width: 0.45,
    height: 0.25,
  });
  assert.deepEqual(adjustCrop(crop, "se", 0.7, 0.8), {
    x: 0.25,
    y: 0.2,
    width: 0.75,
    height: 0.8,
  });
});

function assertBoundsClose(
  actual: { x: number; y: number; width: number; height: number },
  expected: { x: number; y: number; width: number; height: number },
) {
  assert.ok(Math.abs(actual.x - expected.x) < 0.000001);
  assert.ok(Math.abs(actual.y - expected.y) < 0.000001);
  assert.ok(Math.abs(actual.width - expected.width) < 0.000001);
  assert.ok(Math.abs(actual.height - expected.height) < 0.000001);
}

test("source viewport zoom is anchored to the cursor and pane splitters resize bounded panes", () => {
  const zoomed = anchoredZoom({ x: 10, y: 20, scale: 1 }, { x: 110, y: 220 }, -1);
  assert.ok(Math.abs(zoomed.scale - 1.12) < 0.000001);
  assert.ok(Math.abs(zoomed.x - -2) < 0.000001);
  assert.ok(Math.abs(zoomed.y - -4) < 0.000001);

  assert.deepEqual(resizePanes("source-sheet", { library: 220, source: 400, inspector: 320 }, 766, 100, 1400), {
    library: 220,
    source: 440,
    inspector: 320,
  });
  assert.deepEqual(resizePanes("sheet-inspector", { library: 220, source: 400, inspector: 320 }, 1000, 100, 1400), {
    library: 220,
    source: 400,
    inspector: 420,
  });
});

test("patch selection moves, resizes, rotates, and normalizes without leaving source bounds", () => {
  const rectangle = [
    { x: 0.2, y: 0.25 }, { x: 0.5, y: 0.25 },
    { x: 0.5, y: 0.55 }, { x: 0.2, y: 0.55 },
  ] as const;
  assert.deepEqual(movePatch(rectangle, 0.8, -0.5), [
    { x: 0.7, y: 0 }, { x: 1, y: 0 },
    { x: 1, y: 0.3 }, { x: 0.7, y: 0.3 },
  ]);
  assert.deepEqual(resizePatch(rectangle, 2, { x: 0.8, y: 0.75 }), [
    { x: 0.2, y: 0.25 }, { x: 0.8, y: 0.25 },
    { x: 0.8, y: 0.75 }, { x: 0.2, y: 0.75 },
  ]);
  const rotated = rotatePatch(rectangle, { x: 0.35, y: 0.4 }, Math.PI / 2, { width: 1000, height: 1000 });
  assert.ok(Math.abs(rotated[0].x - 0.5) < 0.000001);
  assert.ok(Math.abs(rotated[0].y - 0.25) < 0.000001);
  const normalized = normalizePatchToRectangle([
    { x: 0.2, y: 0.2 }, { x: 0.6, y: 0.22 },
    { x: 0.64, y: 0.7 }, { x: 0.18, y: 0.62 },
  ], { width: 1000, height: 1000 });
  const top = { x: normalized[1].x - normalized[0].x, y: normalized[1].y - normalized[0].y };
  const side = { x: normalized[3].x - normalized[0].x, y: normalized[3].y - normalized[0].y };
  assert.ok(Math.abs(top.x * side.x + top.y * side.y) < 0.000001);
});
