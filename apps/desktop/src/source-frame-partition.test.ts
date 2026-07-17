import assert from "node:assert/strict";
import test from "node:test";
import { constrainAspectBounds } from "./source-workbench-geometry.ts";

test("source-frame partition UI contract uses a centered largest square and user-counted regions", () => {
  const source = { width: 8000, height: 4000 };
  const frame = { x: (source.width - source.height) / 2, y: 0, width: source.height, height: source.height };
  assert.deepEqual(frame, { x: 2000, y: 0, width: 4000, height: 4000 });
  for (const target of [16, 63, 103]) {
    assert.ok(target >= 1 && target <= 256);
    assert.notEqual(target, 53, "53 is not a primary workflow contract");
  }
});

test("source frame and detached crop field edits preserve pixel aspect", () => {
  const sourceAspect = 4000 / 8000;
  const frame = constrainAspectBounds({ x: 0.1, y: 0.1, width: 0.75, height: 0.5 }, sourceAspect);
  assert.equal(frame.width / frame.height, sourceAspect);
  assert.ok(frame.x + frame.width <= 1 && frame.y + frame.height <= 1);

  const detachedAspect = (4 / 1) * 4000 / 8000;
  const crop = constrainAspectBounds({ x: 0.2, y: 0.2, width: 0.4, height: 0.1 }, detachedAspect);
  assert.equal(crop.width / crop.height, detachedAspect);
  assert.ok(crop.x + crop.width <= 1 && crop.y + crop.height <= 1);
});
