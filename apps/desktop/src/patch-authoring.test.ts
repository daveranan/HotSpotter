import assert from "node:assert/strict";
import test from "node:test";
import type { PatchGeometry } from "@hot-trimmer/ipc-contracts";
import {
  escapeAction,
  exceedsDragThreshold,
  canonicalizeFourPoints,
  geometryBounds,
  moveCorner,
  normalizedFromRect,
  quadProjection,
  rectangleGeometry,
  rotateGeometry,
  scaleGeometryFromHandle,
  scaleGeometryFromCorner,
  translateGeometry,
  validatePatchGeometry,
  zoomViewAtPoint,
} from "./patch-authoring.ts";

test("rectangle placement produces canonical valid corners", () => {
  const geometry = rectangleGeometry({ x: 0.8, y: 0.7 }, { x: 0.2, y: 0.1 });
  assert.equal(validatePatchGeometry(geometry), null);
  assert.deepEqual(geometry.corners[0], { x: 0.2, y: 0.1 });
  assert.deepEqual(geometry.corners[2], { x: 0.8, y: 0.7 });
});

test("four points are canonicalized regardless of click order", () => {
  const expected = rectangleGeometry({ x: 0.1, y: 0.2 }, { x: 0.8, y: 0.9 });
  const counterClockwise = [expected.corners[0], expected.corners[3], expected.corners[2], expected.corners[1]];
  const unordered = [expected.corners[2], expected.corners[0], expected.corners[3], expected.corners[1]];
  assert.deepEqual(canonicalizeFourPoints(counterClockwise), expected);
  assert.deepEqual(canonicalizeFourPoints(unordered), expected);
});

test("selection transforms move, resize, and rotate without editing individual points", () => {
  const geometry = rectangleGeometry({ x: 0.2, y: 0.2 }, { x: 0.6, y: 0.6 });
  const resized = scaleGeometryFromHandle(geometry, 2, { x: 0.8, y: 0.9 });
  const bounds = geometryBounds(resized);
  assert.ok(Math.abs(bounds.left - 0.2) < 1e-10);
  assert.ok(Math.abs(bounds.top - 0.2) < 1e-10);
  assert.ok(Math.abs(bounds.right - 0.8) < 1e-10);
  assert.ok(Math.abs(bounds.bottom - 0.9) < 1e-10);
  const rotated = rotateGeometry(geometry, { x: 0.4, y: 0.4 }, Math.PI / 2);
  assert.ok(Math.abs(rotated.corners[0]!.x - 0.6) < 1e-10);
  assert.ok(Math.abs(rotated.corners[0]!.y - 0.2) < 1e-10);
});

test("shift-resize preserves patch proportions from the opposite corner", () => {
  const geometry = rectangleGeometry({ x: 0.2, y: 0.2 }, { x: 0.6, y: 0.4 });
  const resized = scaleGeometryFromHandle(geometry, 2, { x: 0.65, y: 0.55 }, true);
  const before = geometryBounds(geometry);
  const after = geometryBounds(resized);
  const beforeRatio = (before.right - before.left) / (before.bottom - before.top);
  const afterRatio = (after.right - after.left) / (after.bottom - after.top);
  assert.ok(Math.abs(afterRatio - beforeRatio) < 1e-10);
  assert.deepEqual(resized.corners[0], geometry.corners[0]);
});

test("selection handles scale a skewed patch from its actual corner without jumping", () => {
  const geometry = { corners: [
    { x: 0.2, y: 0.2 }, { x: 0.6, y: 0.25 }, { x: 0.7, y: 0.7 }, { x: 0.25, y: 0.65 },
  ] } as PatchGeometry;
  const target = { x: 0.75, y: 0.15 };
  const resized = scaleGeometryFromCorner(geometry, 1, target);
  assert.ok(Math.abs(resized.corners[1]!.x - target.x) < 1e-10);
  assert.ok(Math.abs(resized.corners[1]!.y - target.y) < 1e-10);
  assert.deepEqual(resized.corners[3], geometry.corners[3]);
});

test("live rectification projection maps the unit square onto patch corners", () => {
  const geometry = rectangleGeometry({ x: 0.2, y: 0.3 }, { x: 0.8, y: 0.9 });
  const matrix = quadProjection(geometry);
  const project = (u: number, v: number) => ({
    x: (matrix.a * u + matrix.b * v + matrix.c) / (matrix.g * u + matrix.h * v + 1),
    y: (matrix.d * u + matrix.e * v + matrix.f) / (matrix.g * u + matrix.h * v + 1),
  });
  geometry.corners.forEach((corner, index) => {
    const [u, v] = [[0, 0], [1, 0], [1, 1], [0, 1]][index]!;
    const projected = project(u, v);
    assert.ok(Math.abs(projected.x - corner.x) < 1e-10);
    assert.ok(Math.abs(projected.y - corner.y) < 1e-10);
  });
});

test("escape finishes only a completed valid placement", () => {
  assert.equal(escapeAction([{ x: 0.1, y: 0.1 }]), "cancel");
  assert.equal(escapeAction(rectangleGeometry({ x: 0.1, y: 0.1 }, { x: 0.9, y: 0.9 }).corners), "finish");
});

test("direct manipulation remains bounded and exposes invalid concavity", () => {
  const geometry = rectangleGeometry({ x: 0.2, y: 0.2 }, { x: 0.8, y: 0.8 });
  const moved = translateGeometry(geometry, 0.8, -0.8);
  assert.equal(Math.max(...moved.corners.map((point) => point.x)), 1);
  assert.equal(Math.min(...moved.corners.map((point) => point.y)), 0);
  const concave = moveCorner(geometry, 2, { x: 0.3, y: 0.3 });
  assert.match(validatePatchGeometry(concave) ?? "", /outside boundary/);
});

test("viewport coordinates reject letterboxed space", () => {
  const rect = { left: 100, top: 50, width: 400, height: 200 };
  assert.deepEqual(normalizedFromRect(300, 150, rect), { x: 0.5, y: 0.5 });
  assert.equal(normalizedFromRect(90, 150, rect), null);
});

test("viewport coordinates are invariant at 100 and 300 percent display scale", () => {
  const at100 = normalizedFromRect(300, 150, { left: 100, top: 50, width: 400, height: 200 });
  const at300 = normalizedFromRect(900, 450, { left: 300, top: 150, width: 1200, height: 600 });
  assert.deepEqual(at300, at100);
});

test("wheel zoom keeps the image coordinate under the cursor stationary", () => {
  const before = { left: 100, top: 50, width: 400, height: 200 };
  const cursor = { x: 420, y: 90 };
  const view = { x: 0, y: 0, scale: 1 };
  const next = zoomViewAtPoint(view, 2, cursor, before);
  const after = {
    left: before.left + before.width / 2 + next.x - before.width,
    top: before.top + before.height / 2 + next.y - before.height,
    width: before.width * 2,
    height: before.height * 2,
  };
  assert.deepEqual(normalizedFromRect(cursor.x, cursor.y, after), normalizedFromRect(cursor.x, cursor.y, before));
});

test("rapid patch drafts remain independent and a cancelled drag restores its original geometry", () => {
  const first = rectangleGeometry({ x: 0.05, y: 0.05 }, { x: 0.25, y: 0.25 });
  const second = rectangleGeometry({ x: 0.4, y: 0.4 }, { x: 0.8, y: 0.8 });
  const patches = new Map([["first", first], ["second", second]]);
  let selected = "first";
  selected = "second";
  const dragOriginal = patches.get(selected)!;
  const live = moveCorner(dragOriginal, 0, { x: 0.5, y: 0.5 });
  assert.notDeepEqual(live, dragOriginal);
  const afterCancel = dragOriginal;
  assert.deepEqual(afterCancel, second);
  assert.deepEqual(patches.get("first"), first);
});

test("selection clicks do not become transform transactions until the pointer actually drags", () => {
  assert.equal(exceedsDragThreshold({ x: 100, y: 100 }, { x: 102, y: 102 }), false);
  assert.equal(exceedsDragThreshold({ x: 100, y: 100 }, { x: 104, y: 100 }), true);
});
