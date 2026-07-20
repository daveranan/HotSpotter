import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import {
  canInteractWithPatch,
  compiledMapViewForSourceChannel,
  sourceChannelForCompiledMapView,
  sourceSetIdForRegion,
} from "./workbench-interactions.ts";

test("source and compiled map selectors share every equivalent material map", () => {
  const pairs = [
    ["base_color", "baseColor"],
    ["normal", "normal"],
    ["height", "height"],
    ["roughness", "roughness"],
    ["metallic", "metallic"],
    ["ambient_occlusion", "ambientOcclusion"],
    ["material_id", "materialId"],
  ] as const;
  for (const [channel, view] of pairs) {
    assert.equal(compiledMapViewForSourceChannel(channel), view);
    assert.equal(sourceChannelForCompiledMapView(view), channel);
  }
  assert.equal(sourceChannelForCompiledMapView("regionId"), null);
  assert.equal(compiledMapViewForSourceChannel("specular"), null);
});

test("region ownership follows whole sources, primary inheritance, and patch owners", () => {
  const context = {
    primarySourceSetId: "material-1",
    patches: [{ id: "patch-2", sourceId: "image-2" }],
    sourceSets: [
      { id: "material-1", sourceIds: ["image-1"] },
      { id: "material-2", sourceIds: ["image-2"] },
    ],
  };
  assert.equal(sourceSetIdForRegion({ ...context, content: { type: "material_source", id: "material-2" } }), "material-2");
  assert.equal(sourceSetIdForRegion({ ...context, content: { type: "inherit_primary_material" } }), "material-1");
  assert.equal(sourceSetIdForRegion({ ...context, content: { type: "patch", id: "patch-2" } }), "material-2");
  assert.equal(sourceSetIdForRegion({ ...context, content: { type: "solid", id: { baseColor: [0, 0, 0, 255] } } }), null);
});

test("point editing isolates the active patch and the workbench wires the shared policies", () => {
  assert.equal(canInteractWithPatch(null, "patch-2"), true);
  assert.equal(canInteractWithPatch("patch-1", "patch-1"), true);
  assert.equal(canInteractWithPatch("patch-1", "patch-2"), false);

  const app = readFileSync(new URL("./source-first-app.tsx", import.meta.url), "utf8");
  assert.match(app, /onSelect=\{selectSourceChannel\}/);
  assert.match(app, /setMapView=\{selectCompiledMapView\}/);
  assert.match(app, /const showInspector = false/);
  assert.match(app, /className="preview-busy-overlay"[\s\S]*?<strong>Exporting material maps<\/strong>/);
  assert.doesNotMatch(app, /className="busy-corner">Exporting/);
});
