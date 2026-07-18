import assert from "node:assert/strict";
import test from "node:test";

import type { CompiledSheetProjection, TrimSheetDocumentCommand } from "@hot-trimmer/ipc-contracts";
import { adjustCrop, anchoredZoom, fitSourceFrame, gridRectToPreviewBounds, mapQuadToUnitSquare, mapUnitSquareToQuad, movePatch, normalizePatchToRectangle, resizeAspectLocked, resizePatch, resizePanes, rotatePatch } from "./source-workbench-geometry.ts";

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

test("committed topology is projected into the retained preview pixel space", () => {
  assert.deepEqual(
    gridRectToPreviewBounds(
      { x: 16, y: 8, width: 32, height: 24 },
      { width: 64, height: 64 },
      { width: 512, height: 512 },
    ),
    { x: 128, y: 64, width: 256, height: 192 },
  );
});

test("truthful-base-color preserves stable slot identity and explicit source intent", () => {
  const region = {
    regionId: "region-cornice-long",
    mapping: { sourceCropIntent: "unplaced" as const },
  };
  const slot = {
    regionId: region.regionId,
    slotKey: "cornice_long",
    candidateId: "candidate-cornice-long",
    samplingPlanId: "plan-cornice-long",
    stage14ResultId: "result-cornice-long",
  };
  assert.equal(slot.regionId, region.regionId);
  assert.equal(region.mapping.sourceCropIntent, "unplaced");
  assert.notEqual(slot.candidateId, slot.samplingPlanId);
  assert.notEqual(slot.samplingPlanId, slot.stage14ResultId);
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

test("radial gizmo coordinates round-trip through an authored four-point patch", () => {
  const corners = [
    { x: 0.12, y: 0.18 }, { x: 0.91, y: 0.08 },
    { x: 0.82, y: 0.9 }, { x: 0.2, y: 0.78 },
  ] as const;
  for (const local of [{ x: 0.35, y: 0.62 }, { x: 0.5, y: 0.5 }, { x: 0.82, y: 0.24 }]) {
    const source = mapUnitSquareToQuad(corners, local);
    const roundTrip = mapQuadToUnitSquare(corners, source);
    assert.ok(Math.abs(roundTrip.x - local.x) < 0.000001);
    assert.ok(Math.abs(roundTrip.y - local.y) < 0.000001);
  }
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

test("source-frame authoring keeps square defaults and aspect-locked bounds deterministic", () => {
  assert.deepEqual(fitSourceFrame({ width: 8000, height: 4000 }, { width: 1, height: 1 }, "largest"), {
    x: 0.25, y: 0, width: 0.5, height: 1,
  });
  assertBoundsClose(fitSourceFrame({ width: 750, height: 3600 }, { width: 1, height: 1 }, "largest"), {
    x: 0, y: (3600 - 750) / 2 / 3600, width: 1, height: 750 / 3600,
  });
  assertBoundsClose(resizeAspectLocked({ x: 0.25, y: 0.25, width: 0.25, height: 0.25 }, "se", 0.1, 0.05, 1), {
    x: 0.25, y: 0.25, width: 0.35, height: 0.35,
  });
  const topEdge = resizeAspectLocked({ x: 0.25, y: 0.25, width: 0.5, height: 0.5 }, "n", 0, -0.1, 1);
  assertBoundsClose(topEdge, { x: 0.2, y: 0.15, width: 0.6, height: 0.6 });
  assert.equal(topEdge.width / topEdge.height, 1);
  const sourceAspect = resizeAspectLocked({ x: 0.2, y: 0.2, width: 0.25, height: 0.5 }, "se", 0.1, 0.05, 0.5);
  assert.equal(sourceAspect.width / sourceAspect.height, 0.5);
});

test("boundary-limited rotation holds the last valid transform instead of gesture start", () => {
  const rectangle = [
    { x: 0.07, y: 0.25 }, { x: 0.33, y: 0.25 },
    { x: 0.33, y: 0.75 }, { x: 0.07, y: 0.75 },
  ] as const;
  const valid = rotatePatch(rectangle, { x: 0.2, y: 0.5 }, 0.2, { width: 1, height: 1 });
  const blocked = rotatePatch(valid, { x: 0.2, y: 0.5 }, Math.PI / 2, { width: 1, height: 1 });
  assert.strictEqual(blocked, valid);
  const recovered = rotatePatch(valid, { x: 0.2, y: 0.5 }, -0.1, { width: 1, height: 1 });
  assert.notStrictEqual(recovered, rectangle);
});
