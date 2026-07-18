import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { constrainAspectBounds } from "./source-workbench-geometry.ts";
import { defaultPartitionRecipe, layoutTemplateOptions, layoutTemplateRecipe } from "./hierarchical-layout-templates.ts";

test("source-frame partition UI contract uses a centered largest square and user-counted regions", () => {
  const source = { width: 8000, height: 4000 };
  const frame = { x: (source.width - source.height) / 2, y: 0, width: source.height, height: source.height };
  assert.deepEqual(frame, { x: 2000, y: 0, width: 4000, height: 4000 });
  for (const target of [16, 63, 103]) {
    assert.ok(target >= 1 && target <= 256);
    assert.notEqual(target, 53, "53 is not a primary workflow contract");
  }
});

test("local topology edits retain preview scale after a region resize commits", () => {
  const app = readFileSync(new URL("./source-first-app.tsx", import.meta.url), "utf8");
  const retopology = app.slice(app.indexOf("function retopologizeArtifact"), app.indexOf("function stableRegionColor"));
  assert.match(retopology, /gridRectToPreviewBounds\(definition\.gridRect, document\.logicalGrid!, prior\)/);
  assert.doesNotMatch(retopology, /allocationBounds:\s*definition\.allocationRect/);
});

test("resize ownership preview transfers released strips atomically instead of cell staircases", () => {
  const app = readFileSync(new URL("./source-first-app.tsx", import.meta.url), "utf8");
  const preview = app.slice(app.indexOf("function previewResizeOwnershipTransfers"), app.indexOf("function logicalRectIntersection"));
  assert.match(preview, /transfers\.push\(\{ rect: piece, fromId: selectedId, toId: neighbor\.id \}\)/);
  assert.doesNotMatch(preview, /for \(let y = piece\.y/);
  assert.doesNotMatch(preview, /rectangularizeLogicalMask/);
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

test("source preview is atlas-selected and SourceFrame editing is explicit", () => {
  const app = readFileSync(new URL("./source-first-app.tsx", import.meta.url), "utf8");
  assert.match(app, /data-selection-surface=\"atlas\"/);
  assert.doesNotMatch(app, /data-selection-surface=\"source\"/);
  assert.doesNotMatch(app, /props\.onSelectRegion\(region\.regionId\)/);
  assert.match(app, /data-selection-surface=\"source-preview\"/);
  assert.match(app, /Edit Source Frame/);
  assert.match(app, /sourceFrameEditing \? \"auto\" : \"none\"/);
});

test("intentional-source-partition keeps candidate generation separate from acceptance", () => {
  const app = readFileSync(new URL("./source-first-app.tsx", import.meta.url), "utf8");
  assert.match(app, /Update now/);
  assert.match(app, /Discard/);
  assert.match(app, /Accept/);
  assert.match(app, /candidateRecipe/);
  assert.match(app, /candidateRecipe: recipe/);
  assert.match(app, /accept_source_frame_partition/);
  assert.match(app, /profile: "draft512"/);
  assert.doesNotMatch(app, /Regenerate \/ Accept/);
  assert.match(app, /layout-sidebar/);
  assert.match(app, /candidateState/);
  assert.match(app, /requestedFloor}–\${requestedMaximum} soft region range/);
  assert.match(app, /protected \/ \${hierarchical\.subdividableParentCount} hierarchical parents/);
  assert.match(app, /candidatePreviewHash !== partitionRecipeFingerprint\(recipe\)/);
  assert.match(app, /split_source_frame_region/);
  assert.match(app, /merge_source_frame_regions/);
  assert.match(app, /draw_source_frame_region/);
  assert.match(app, /resize_source_frame_region/);
  assert.match(app, /Draw region/);
  assert.match(app, /draw-region-preview/);
  assert.match(app, /resize-region-preview/);
  assert.match(app, /resizeHandles\.map/);
  assert.match(app, /previewResizeOwnershipTransfers/);
  const directGesture = app.match(/function finishDirectEdit[\s\S]*?\n  return <section/)?.[0] ?? "";
  assert.match(directGesture, /resize_source_frame_region/);
  assert.match(directGesture, /regionId: draft\.regionId/);
  assert.match(directGesture, /draw_source_frame_region/);
  assert.match(app, /adaptiveGridSteps/);
  assert.match(app, /aria-label="Composition preset"/);
  assert.match(app, /layoutTemplateOptions\.map/);
  assert.doesNotMatch(app, /Composition fixtures/);
  const directEdit = app.match(/async function editSourceFrameLayout[\s\S]*?\n  function discardPartitionCandidate/)?.[0] ?? "";
  assert.match(directEdit, /retopologizeArtifact/);
  assert.doesNotMatch(directEdit, /preview_through_stage_14/);
  assert.doesNotMatch(directEdit, /setArtifact\(null\)/);
  assert.doesNotMatch(directEdit, /setPreview\(null\)/);
});

test("six hierarchical composition presets expose valid, distinct product recipes", () => {
  assert.equal(layoutTemplateOptions.length, 6);
  const base = defaultPartitionRecipe();
  assert.equal(base.hierarchical?.macroStyle, "mixed_hierarchy");
  const recipes = layoutTemplateOptions.map((option) => layoutTemplateRecipe(base, option.id));
  assert.equal(new Set(recipes.map((recipe) => JSON.stringify(recipe))).size, 6);
  for (const recipe of recipes) {
    const hierarchy = recipe.hierarchical!;
    assert.equal(hierarchy.largeShareMilli + hierarchy.mediumShareMilli + hierarchy.smallShareMilli
      + hierarchy.stripShareMilli + hierarchy.radialShareMilli, 1_000);
    assert.ok(hierarchy.targetRegionMin <= hierarchy.targetRegionMax);
    assert.ok(hierarchy.protectedParentCount + hierarchy.subdividableParentCount <= hierarchy.macroParentCount);
    assert.ok(hierarchy.stripThicknessLadder.every((value) => value >= 1));
    assert.equal(recipe.targetRegionCount, hierarchy.targetRegionMax);
    assert.equal(recipe.schemaVersion, 3);
    assert.deepEqual(recipe.grid, { schemaVersion: 1, width: 64, height: 64 });
  }
  assert.equal(recipes[0]!.hierarchical!.largeShareMilli, 580, "mixed hierarchy keeps broad panels dominant");
  assert.equal(recipes[1]!.hierarchical!.recursivePolicy, "cascade", "panel cascade follows one continuation branch");
  assert.equal(recipes[2]!.hierarchical!.horizontalStripWeightMilli, 800, "horizontal template is band-dominant");
  assert.deepEqual(recipes[3]!.hierarchical!.allowedSplitRatios, ["half"], "facade template aligns exact halvings");
  assert.equal(recipes[4]!.hierarchical!.macroStyle, "classic_hotspot_basis", "classic template uses its authored basis grammar");
  assert.equal(recipes[5]!.hierarchical!.radialCount, 4, "mechanical template reserves four radial slots");
  assert.ok(recipes.every((recipe) => recipe.hierarchical!.symmetry === "identity"));
});

test("accepted and discarded recipes settle auto-preview so direct editing stays unlocked", () => {
  const app = readFileSync(new URL("./source-first-app.tsx", import.meta.url), "utf8");
  const accept = app.slice(app.indexOf("async function acceptPartitionCandidate"), app.indexOf("async function editSourceFrameLayout"));
  const discard = app.slice(app.indexOf("function discardPartitionCandidate"), app.indexOf("async function createDocumentAndCompile"));
  assert.match(accept, /setCandidatePreviewHash\(acceptedFingerprint\)/);
  assert.doesNotMatch(accept, /setCandidatePreviewHash\(null\)/);
  assert.match(discard, /setCandidatePreviewHash\(partitionRecipeFingerprint\(candidateRecipe\)\)/);
  assert.match(app, /Accept candidate and edit/);
});

test("layout editing keeps the texture stable, pans in draw mode, and auto-previews valid recipes", () => {
  const app = readFileSync(new URL("./source-first-app.tsx", import.meta.url), "utf8");
  assert.match(app, /const \[gridOpacity, setGridOpacity\] = useState\(10\)/);
  assert.match(app, /source-frame-texture/);
  assert.match(app, /Region colors/);
  assert.match(app, /viewport\.beginPan\(event\)/);
  assert.doesNotMatch(app, /layoutTool !== "draw" &&/);
  assert.match(app, /lastAutoPreviewFingerprint/);
  assert.match(app, /window\.setTimeout\(\(\) => \{[\s\S]*props\.previewCandidate\(props\.candidateRecipe\)/);
  assert.doesNotMatch(app, /Drag shared vertical boundary/);
  assert.doesNotMatch(app, /Reset boundary/);
  assert.match(app, /props\.mapView === "baseColor" \? !!continuousTexture : !!imageUrl/);
  assert.match(app, /will not display a partial Stage 14 map/);
  assert.match(app, /Complete Base Color unavailable/);
});

test("source-frame layout exposes the patch workbench and product-facing composition controls", () => {
  const app = readFileSync(new URL("./source-first-app.tsx", import.meta.url), "utf8");
  assert.match(app, /Show Source Workbench/);
  assert.match(app, /showSourceWorkspace \? <section className="source-workspace">/);
  assert.match(app, /Assign patch to region/);
  assert.match(app, /set_region_content/);
  assert.match(app, /assigned-patch-preview/);
  assert.match(app, /const gridResolutionOptions = \[16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256\]/);
  assert.match(app, /Composition preset/);
  assert.match(app, /Large panel share/);
  assert.match(app, /Strip share/);
  assert.match(app, /Radial slots/);
  assert.match(app, /Orientation/);
  assert.match(app, /Hierarchy depth/);
  assert.match(app, /Protected parents/);
  assert.match(app, /Split ratio palette/);
  assert.match(app, /Strip thickness ladder/);
  assert.match(app, /<summary>Advanced hierarchy<\/summary>/);
});
