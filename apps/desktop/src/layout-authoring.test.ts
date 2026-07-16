import assert from "node:assert/strict";
import test from "node:test";
import type { LayoutRegion, ProjectSnapshot, SourceSnapshot } from "@hot-trimmer/ipc-contracts";
import {
  LayoutSolveSequencer,
  beginLayoutDrag,
  buildCustomAtlasGenerateLayoutRequest,
  buildLayoutRequest,
  buildTemplateGenerateLayoutRequest,
  cancelLayoutDrag,
  clampBoundsToClearance,
  defaultLayoutSettings,
  defaultTemplateSourceTransform,
  externalGuideStyle,
  keyboardBounds,
  layoutRegionIssues,
  layoutAsyncFailure,
  layoutRegionPresentation,
  availableLayoutPreviewMaps,
  layoutPreviewDataUrl,
  nearestValidLayoutBounds,
  pixelDeltaAtZoom,
  reorderRegionPreview,
  settingsForPreset,
  sheetPointFromClient,
  switchAuthoringSource,
  genericArchitectureTemplate,
  templateOptions,
  templateSourceTransform,
  updateLayoutDrag,
} from "./layout-authoring.ts";

function source(id: string, sourceSetId: string, width: number, height: number): SourceSnapshot {
  return {
    id, sourceSetId, channel: "base_color", ownership: "owned_copy", displayName: `${id}.png`, sourcePath: `${id}.png`,
    width, height, format: "PNG", colorType: "rgba8", hasAlpha: true, exifOrientation: 1,
    hasEmbeddedIccProfile: false, iccConvertedToSrgb: false, encodedBytes: 100, thumbnailDataUrl: "data:image/png;base64,AA==", thumbnailMipmaps: [],
  };
}

function project(): ProjectSnapshot {
  return {
    id: "project", name: "Test", path: "test.hottrimmer", schemaVersion: 6, dirty: false, staleLockRecovered: false, isDraft: false, authoringRevision: 1,
    sourceSets: [{ id: "set-b", name: "Metal" }, { id: "set-a", name: "Brick" }],
    sources: [source("source-a", "set-a", 1024, 512), source("source-b", "set-b", 2048, 2048)],
    patches: [
      { id: "patch-a", sourceId: "source-a", name: "Brick strip", enabled: true, geometry: { corners: [{ x: 0, y: 0 }, { x: 1, y: 0 }, { x: 1, y: 0.25 }, { x: 0, y: 0.25 }] }, properties: { repeatMode: "repeat_x", trimCap: true, paddingPx: 4, bleedPx: 8, mapParticipation: "all" }, rectification: { scale: 1 } },
      { id: "patch-b", sourceId: "source-b", name: "Metal tile", enabled: true, geometry: { corners: [{ x: 0, y: 0 }, { x: 0.5, y: 0 }, { x: 0.5, y: 0.5 }, { x: 0, y: 0.5 }] }, properties: { repeatMode: "tile_xy", trimCap: false, paddingPx: 2, bleedPx: 4, mapParticipation: "all" }, rectification: { scale: 1 } },
    ],
    layout: null, canUndoPatch: false, canRedoPatch: false, canUndoProject: false, canRedoProject: false, warnings: [],
  };
}

function region(id: string, itemKey: string, bounds = { x: 0, y: 0, width: 128, height: 128 }, orderIndex = 0): LayoutRegion {
  return {
    id, itemKey, fill: itemKey.startsWith("patch:") ? { type: "rectified_patch", sourceSetId: "set-a", patchId: itemKey.slice(6) } : { type: "whole_source_set", sourceSetId: "set-a" },
    behavior: "stretch", bounds, paddingPx: 4, bleedPx: 8, orderIndex, locks: { position: false, width: false, height: false }, idColor: [30, 80, 120],
  };
}

test("template generation request uses the Generic Architecture identity and default source framing without client geometry", () => {
  const settings = defaultLayoutSettings();
  assert.deepEqual(buildTemplateGenerateLayoutRequest("set-a", "layout", settings, undefined, 9), {
    protocolVersion: 1,
    mode: "template",
    template: genericArchitectureTemplate,
    sourceSetId: "set-a",
    layoutId: "layout",
    settings,
    sourceTransform: defaultTemplateSourceTransform,
    coalescingGroup: 9,
  });
});

test("template generation request includes selected whole-source framing", () => {
  const settings = defaultLayoutSettings();
  assert.deepEqual(buildTemplateGenerateLayoutRequest("set-a", "layout", settings, templateSourceTransform("repeat", { x: 0.25, y: 0.75 })), {
    protocolVersion: 1,
    mode: "template",
    template: genericArchitectureTemplate,
    sourceSetId: "set-a",
    layoutId: "layout",
    settings,
    sourceTransform: { mode: "repeat", cropFocus: { x: 0.25, y: 0.75 } },
    coalescingGroup: undefined,
  });
});

test("template generation accepts each built-in trim-sheet identity", () => {
  const settings = defaultLayoutSettings();
  for (const option of templateOptions) {
    const request = buildTemplateGenerateLayoutRequest("set-a", "layout", settings, undefined, undefined, option.identity);
    assert.deepEqual(request.template, option.identity);
  }
});

test("template map selection uses supplied maps and falls back to Base Color", () => {
  const preview = { width: 64, height: 64, dataUrl: "base", maps: { height: "height", normal: "normal" } };
  assert.deepEqual(availableLayoutPreviewMaps(preview), ["baseColor", "height", "normal"]);
  assert.equal(layoutPreviewDataUrl(preview, "height"), "height");
  assert.equal(layoutPreviewDataUrl(preview, "roughness"), "base");
  assert.deepEqual(availableLayoutPreviewMaps({ width: 64, height: 64, dataUrl: "base" }), ["baseColor"]);
});

test("custom Atlas generation request preserves the packed layout request", () => {
  const request = buildLayoutRequest(project(), {
    layoutId: "atlas", preset: "atlas", settings: defaultLayoutSettings("atlas"), selectedSourceSetIds: ["set-a"], includePatches: true,
  });
  assert.deepEqual(buildCustomAtlasGenerateLayoutRequest(request, 5), {
    protocolVersion: 1,
    mode: "custom_atlas",
    request,
    coalescingGroup: 5,
  });
});

test("Atlas contains only enabled participating patches and Atlas-local simple entries", () => {
  const active = project();
  active.patches[1]!.enabled = false;
  active.patches.push({ ...active.patches[0]!, id: "patch-excluded", properties: { ...active.patches[0]!.properties, mapParticipation: "excluded" } });
  const request = buildLayoutRequest(active, {
    layoutId: "atlas", preset: "atlas", settings: defaultLayoutSettings("atlas"), selectedSourceSetIds: ["set-a", "set-b"], includePatches: true,
    items: [
      { key: "source:set-a", fill: { type: "whole_source_set", sourceSetId: "set-a" }, behavior: "stretch", naturalSize: { width: 1024, height: 512 }, enabled: true, participates: true, constraints: {} },
      { key: "simple:color", fill: { type: "simple_color", rgba: [20, 40, 60, 255] }, behavior: "stretch", naturalSize: { width: 256, height: 256 }, enabled: true, participates: true, constraints: { templateBounds: { x: 0, y: 0, width: 1, height: 1 } } },
    ],
  });
  assert.deepEqual(request.items.map((item) => item.key), ["patch:patch-a", "simple:color"]);
  assert.equal(request.items.some((item) => item.fill.type === "whole_source_set"), false);
  assert.equal(request.items[1]?.constraints.templateBounds, undefined);
});
test("simple color/data items can create a patch-free and source-free layout request", () => {
  const request = buildLayoutRequest(project(), {
    layoutId: "simple-only", preset: "atlas", settings: defaultLayoutSettings("atlas"), selectedSourceSetIds: [], includePatches: false,
    items: [
      { key: "simple:color", fill: { type: "simple_color", rgba: [20, 40, 60, 255] }, behavior: "stretch", naturalSize: { width: 256, height: 256 }, enabled: true, participates: true, constraints: {} },
      { key: "simple:data", fill: { type: "simple_data", input: { channel: "roughness", value: 0.5 } }, behavior: "stretch", naturalSize: { width: 256, height: 256 }, enabled: true, participates: true, constraints: {} },
    ],
  });
  assert.deepEqual(request.items.map((item) => item.key), ["simple:color", "simple:data"]);
});

test("switching the authoring source preserves the same complete layout", () => {
  const layout = { id: "layout", regions: [region("one", "source:set-a"), region("two", "patch:patch-a")] };
  const changed = switchAuthoringSource({ sourceSetId: "set-a", layout }, "set-b");
  assert.equal(changed.layout, layout);
  assert.equal(changed.layout.regions.length, 2);
});

test("preset intent updates packing while locked existing regions remain in regeneration", () => {
  const locked = { ...region("locked", "patch:patch-a"), locks: { position: true, width: true, height: false } };
  const settings = settingsForPreset(defaultLayoutSettings(), "horizontal_trims");
  const request = buildLayoutRequest(project(), {
    layoutId: "layout", preset: "horizontal_trims", settings, selectedSourceSetIds: ["set-a"], includePatches: true, existingRegions: [locked],
  });
  assert.equal(request.preset, "horizontal_trims");
  assert.equal(request.settings.autoPack.priority, "horizontal_strips");
  assert.deepEqual(request.existingRegions[0]?.locks, locked.locks);
});

test("sheet coordinate transforms are normalized at 100 and 300 percent zoom", () => {
  assert.deepEqual(sheetPointFromClient({ x: 110, y: 70 }, { left: 10, top: 20, width: 200, height: 100 }, { width: 1000, height: 500 }), { x: 500, y: 250 });
  assert.equal(pixelDeltaAtZoom(15, 1), 15);
  assert.equal(pixelDeltaAtZoom(45, 3), 15);
});

test("live validation distinguishes overlap, external clearance, and sheet-edge resolution", () => {
  const direct = layoutRegionIssues([
    region("a", "source:set-a", { x: 20, y: 20, width: 40, height: 40 }),
    region("b", "patch:patch-a", { x: 50, y: 30, width: 40, height: 40 }),
  ], { width: 200, height: 200 });
  assert.deepEqual([...direct.get("a") ?? []], ["content_overlap"]);
  assert.deepEqual([...direct.get("b") ?? []], ["content_overlap"]);

  const clearanceOnly = layoutRegionIssues([
    region("a", "source:set-a", { x: 20, y: 20, width: 30, height: 30 }),
    region("b", "patch:patch-a", { x: 70, y: 20, width: 30, height: 30 }),
  ], { width: 200, height: 200 });
  assert.deepEqual([...clearanceOnly.get("a") ?? []], ["clearance"]);
  assert.deepEqual([...clearanceOnly.get("b") ?? []], ["clearance"]);

  const atEdge = layoutRegionIssues([
    region("edge", "source:set-a", { x: 5, y: 40, width: 50, height: 50 }),
  ], { width: 200, height: 200 });
  assert.deepEqual([...atEdge.get("edge") ?? []], ["sheet_edge"]);

  const valid = layoutRegionIssues([
    region("a", "source:set-a", { x: 12, y: 12, width: 30, height: 30 }),
    region("b", "patch:patch-a", { x: 66, y: 12, width: 30, height: 30 }),
  ], { width: 120, height: 80 });
  assert.equal(valid.size, 0);
});

test("active drag bounds drive validation immediately and stay invariant at 100 and 300 percent", () => {
  const active = region("active", "source:set-a", { x: 12, y: 12, width: 20, height: 20 });
  const obstacle = region("obstacle", "patch:patch-a", { x: 60, y: 12, width: 20, height: 20 });
  const output = { width: 100, height: 100 };
  const start100 = sheetPointFromClient({ x: 12, y: 12 }, { left: 0, top: 0, width: 100, height: 100 }, output);
  const point100 = sheetPointFromClient({ x: 45, y: 12 }, { left: 0, top: 0, width: 100, height: 100 }, output);
  const start300 = sheetPointFromClient({ x: 36, y: 36 }, { left: 0, top: 0, width: 300, height: 300 }, output);
  const point300 = sheetPointFromClient({ x: 135, y: 36 }, { left: 0, top: 0, width: 300, height: 300 }, output);
  const preview100 = updateLayoutDrag(beginLayoutDrag(active, "move", 1, start100, 1), point100, output).preview;
  const preview300 = updateLayoutDrag(beginLayoutDrag(active, "move", 1, start300, 1), point300, output).preview;
  assert.deepEqual(preview100, preview300);
  assert.equal(layoutRegionIssues([active, obstacle], output).size, 0, "persisted bounds remain valid");
  const issues100 = layoutRegionIssues([active, obstacle], output, { regionId: active.id, bounds: preview100 });
  const issues300 = layoutRegionIssues([active, obstacle], output, { regionId: active.id, bounds: preview300 });
  assert.deepEqual([...issues100.entries()].map(([id, issues]) => [id, [...issues]]), [...issues300.entries()].map(([id, issues]) => [id, [...issues]]));
  assert.deepEqual([...issues100.get("active") ?? []], ["content_overlap"]);
});

test("padding and bleed guides expand outside authoritative content on both axes", () => {
  assert.deepEqual(externalGuideStyle({ x: 0, y: 0, width: 100, height: 50 }, 10), {
    left: "-10%", right: "-10%", top: "-20%", bottom: "-20%",
  });
});

test("drag preview respects locks and Escape-style cancellation restores original bounds", () => {
  const original = region("drag", "source:set-a", { x: 10, y: 10, width: 100, height: 80 });
  const drag = beginLayoutDrag(original, "move", 7, { x: 20, y: 20 }, 91);
  const moved = updateLayoutDrag(drag, { x: 50, y: 60 }, { width: 512, height: 512 });
  assert.deepEqual(moved.preview, { x: 40, y: 50, width: 100, height: 80 });
  assert.equal(moved.coalescingGroup, 91);
  assert.deepEqual(cancelLayoutDrag(moved), original.bounds);
  const locked = updateLayoutDrag(drag, { x: 50, y: 60 }, { width: 512, height: 512 }, { position: true, width: false, height: false });
  assert.deepEqual(locked.preview, original.bounds);
});

test("direct manipulation respects bleed and settles before collisions", () => {
  assert.deepEqual(clampBoundsToClearance({ x: -20, y: 95, width: 90, height: 20 }, { width: 100, height: 100 }, 12), { x: 12, y: 68, width: 76, height: 20 });
  const moving = region("moving", "source:set-a", { x: 12, y: 12, width: 20, height: 20 });
  const obstacle = region("obstacle", "patch:patch-a", { x: 60, y: 12, width: 20, height: 20 }, 1);
  const repaired = nearestValidLayoutBounds([moving, obstacle], moving.id, moving.bounds, { x: 55, y: 12, width: 20, height: 20 }, { width: 120, height: 80 });
  assert.equal(layoutRegionIssues([moving, obstacle], { width: 120, height: 80 }, { regionId: moving.id, bounds: repaired }).has(moving.id), false);
  assert.ok(repaired.x > moving.bounds.x && repaired.x < 55);
});

test("numeric/keyboard bounds, reorder, and lock data stay exact", () => {
  const resized = keyboardBounds({ x: 2, y: 3, width: 10, height: 11 }, "ArrowRight", { shift: true }, { width: 64, height: 64 });
  assert.deepEqual(resized, { x: 2, y: 3, width: 11, height: 11 });
  const ordered = reorderRegionPreview([region("a", "source:set-a", undefined, 0), region("b", "patch:patch-a", undefined, 1)], "b", 0);
  assert.deepEqual(ordered.map((candidate) => [candidate.id, candidate.orderIndex]), [["b", 0], ["a", 1]]);
  const locked = { ...region("locked", "source:set-a"), locks: { position: true, width: false, height: true } };
  assert.deepEqual(locked.locks, { position: true, width: false, height: true });
});

test("impossible-fit failure remains visible and preserves the prior layout", () => {
  const previous = { id: "prior-layout" };
  const state = layoutAsyncFailure({ value: previous, failure: null, busy: true, generation: 4 }, 4, { message: "Regions do not fit", recovery: "Increase output resolution." });
  assert.equal(state.value, previous);
  assert.equal(state.failure?.message, "Regions do not fit");
  assert.equal(state.failure?.recovery, "Increase output resolution.");
});

test("cancelled and stale solve generations cannot overwrite current state", () => {
  const sequence = new LayoutSolveSequencer();
  const first = sequence.begin();
  const second = sequence.begin();
  assert.equal(sequence.isCurrent(first), false);
  assert.equal(sequence.isCurrent(second), true);
  sequence.cancel();
  assert.equal(sequence.isCurrent(second), false);
  const state = { value: "current", failure: null, busy: true, generation: 3 } as const;
  assert.equal(layoutAsyncFailure(state, 2, { message: "stale", recovery: "ignore" }), state);
});

test("selecting a patch highlights it without filtering the full sheet", () => {
  const all = [region("source", "source:set-a"), region("patch", "patch:patch-a")];
  const presentation = layoutRegionPresentation(all, "patch-a");
  assert.equal(presentation.regions, all);
  assert.equal(presentation.regions.length, 2);
  assert.deepEqual([...presentation.highlightedRegionIds], ["patch"]);
});
