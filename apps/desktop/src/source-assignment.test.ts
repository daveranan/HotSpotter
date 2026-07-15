import assert from "node:assert/strict";
import test from "node:test";
import { assignSourceFiles, suggestedChannel } from "./source-assignment.ts";

test("recognizes common texture-set suffixes", () => {
  assert.equal(suggestedChannel("Brick_BaseColor.png"), "base_color");
  assert.equal(suggestedChannel("Brick_NRM.tif"), "normal");
  assert.equal(suggestedChannel("Brick_AO.jpg"), "ambient_occlusion");
  assert.equal(suggestedChannel("Brick_Material_ID.png"), "material_id");
  assert.equal(suggestedChannel("T_Brick_D.png"), "base_color");
  assert.equal(suggestedChannel("T_Brick_N.png"), "normal");
});

test("imports Base Color first and auto-assigns named companions", () => {
  const assigned = assignSourceFiles([
    "Brick_Normal.png", "Brick_Roughness.png", "Brick_Albedo.png", "Brick_Metallic.png",
  ], []);
  assert.deepEqual(assigned.map(({ channel }) => channel), ["base_color", "normal", "roughness", "metallic"]);
});

test("does not silently replace or misclassify occupied slots", () => {
  const assigned = assignSourceFiles(["Second_Albedo.png", "Brick_Normal.png"], ["base_color"]);
  assert.deepEqual(assigned.map(({ channel }) => channel), ["normal"]);
  assert.deepEqual(assignSourceFiles(["another-random-photo.png"], ["base_color"]), []);
});
