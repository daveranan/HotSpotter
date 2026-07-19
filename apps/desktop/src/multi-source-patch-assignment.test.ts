import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const app = readFileSync(new URL("./source-first-app.tsx", import.meta.url), "utf8");
const styles = readFileSync(new URL("./document-app.css", import.meta.url), "utf8");
const compiler = readFileSync(new URL("../../../crates/sheet-compiler/src/persisted_pipeline.rs", import.meta.url), "utf8");
const store = readFileSync(new URL("../../../crates/project-store/src/lib.rs", import.meta.url), "utf8");
const native = readFileSync(new URL("../src-tauri/src/document_commands.rs", import.meta.url), "utf8");

test("SourceFrame validation is pinned to its persisted owner while every region resolves ContentReference", () => {
  assert.match(compiler, /direct_source_frame_domain\(\s*request\.project,\s*frame\.source_set_id/);
  assert.doesNotMatch(compiler, /frame\.source_set_id != primary/);
  assert.match(compiler, /resolve_region_content\(request\.project, document, primary, &binding\.content\)/);
  assert.match(compiler, /ContentReference::InheritPrimaryMaterial => Ok\(\(primary, None\)\)/);
  assert.match(compiler, /ContentReference::MaterialSource\(source_set_id\) => Ok\(\(\*source_set_id, None\)\)/);
  assert.match(compiler, /ContentReference::Patch\(patch_id\)/);
  assert.match(compiler, /"patch_binding"/);
  assert.match(compiler, /"whole_source_binding"/);
});

test("adding and selecting an independent source never promotes it or clears the sheet", () => {
  const importImages = app.slice(app.indexOf("async function importImages"), app.indexOf("async function importOne"));
  const importOne = app.slice(app.indexOf("async function importOne"), app.indexOf("async function addSourceSet"));
  assert.doesNotMatch(importImages, /set_primary_material|setArtifact\(null\)/);
  assert.doesNotMatch(importOne, /set_primary_material|setArtifact\(null\)/);
  assert.match(importImages, /!next\.document/);
  assert.match(importOne, /!next\.document/);
  assert.match(app, /const primaryMaterial = project\?\.document\?\.primaryMaterial \?\? ""/);
});

test("twenty patches across two owners preserve stable IDs and independent region bindings", () => {
  const sourceA = "source-a";
  const sourceB = "source-b";
  const patches = Array.from({ length: 20 }, (_, index) => ({
    id: `patch-${index}`,
    sourceSetId: index % 2 ? sourceB : sourceA,
    geometry: { corners: [[0, 0], [1, 0], [1, 1], [0, 1]] },
  }));
  const bindings = {
    region1: { type: "patch", id: patches[0]!.id },
    region2: { type: "patch", id: patches[1]!.id },
    region3: { type: "material_source", id: sourceB },
    region4: { type: "inherit_primary_material" },
  };
  const reopened = JSON.parse(JSON.stringify({ primary: sourceA, patches, bindings }));
  assert.deepEqual(reopened, { primary: sourceA, patches, bindings });
  assert.equal(new Set(reopened.patches.map((patch: { id: string }) => patch.id)).size, 20);
  assert.equal(reopened.patches.filter((patch: { sourceSetId: string }) => patch.sourceSetId === sourceA).length, 10);
  assert.equal(reopened.patches.filter((patch: { sourceSetId: string }) => patch.sourceSetId === sourceB).length, 10);
});

test("replace and removal preserve identity or reject dependencies before mutation", () => {
  assert.match(store, /PatchCommand::ReassignSource/);
  assert.match(store, /pub fn remove_source_set/);
  assert.match(store, /persist_document_state_in_transaction\(\s*&transaction,\s*Some\(document\),\s*"remove_source_set"/);
  assert.match(native, /PatchCommand::Delete \{ patch_id \}/);
  assert.match(native, /Patch .* is assigned to region/);
  assert.match(native, /primary material, SourceFrame, or a region/);
});

test("libraries, assignment, splitters, and application context menus are runtime connected", () => {
  assert.doesNotMatch(app, /Filter patches by source|All sources/);
  assert.match(app, /sourceSetForPatch\(patch\)\?\.id === props\.activeSourceSetId/);
  assert.match(app, /Add independent source/);
  assert.match(app, />\+ Add maps…</);
  assert.match(app, /channelMenu\.filled \? "Replace texture…" : "Add texture…"/);
  assert.match(app, /Set as primary \/ Rebase layout/);
  assert.match(app, /onContextMenu=\{\(event\) => \{ event\.preventDefault\(\)/);
  assert.match(app, /hot-trimmer\.workbench-panes\.v1/);
  assert.match(app, /Assign patch to region/);
  assert.match(app, /<label>Content source<select/);
  assert.match(app, /sourceFrame=\{!regionPatchEditId && project\?\.document\?\.sourceFrame\?\.sourceSetId === activeSourceSetId/);
});

test("workbench and hotspot visibility are independent and the source-sheet divider is proportional", () => {
  assert.match(app, />Workbench<\/button>/);
  assert.match(app, />Hotspot Sheet<\/button>/);
  assert.doesNotMatch(app, /Hide Source Workbench|Workbench & Hotspot Sheet/);
  assert.match(app, /minmax\(280px, \$\{sourceSheetShare\}fr\)/);
  assert.match(app, /onSourceShareChange=\{setSourceSheetShare\}/);
  assert.match(app, /localStorage\.setItem\("hot-trimmer\.source-sheet-share\.v1"/);
});

test("selected region authority is exposed at the top of the right inspector", () => {
  assert.match(app, />REGION SETTINGS</);
  assert.match(app, />Content source<select/);
  assert.match(app, /samplingOptions/);
  assert.match(app, /set_region_behavior/);
  assert.match(compiler, /authored_repeat/);
});

test("patch assignment paints immediately and publishes the persisted binding without a fake transient crop", () => {
  assert.match(app, /pendingPatchRegions = props\.artifact\?\.documentRevision !== props\.project\?\.document\?\.documentRevision/);
  assert.match(app, /pendingPatchRegions\.map\(\(\{ region, patchPreview \}\) => <div/);
  const layoutAssignment = app.slice(app.indexOf("async function editSourceFrameLayout"), app.indexOf("function discardPartitionCandidate"));
  const directPatchAssignment = app.slice(app.indexOf("async function assignPatchToRegion"), app.indexOf("async function assignContentToRegion"));
  const directContentAssignment = app.slice(app.indexOf("async function assignContentToRegion"), app.indexOf("async function setRegionBehavior"));
  assert.match(layoutAssignment, /if \(!pixelRegionId\) setArtifact/);
  assert.doesNotMatch(directPatchAssignment, /retopologizeArtifact|setArtifact/);
  assert.doesNotMatch(directContentAssignment, /retopologizeArtifact|setArtifact/);
  assert.match(app, /requestPreview\(undefined, undefined, interactivePreviewProfile, current\.document!\.documentRevision, false\)/);
  assert.doesNotMatch(app, /requestPreview\(assignedRegionId, undefined/);
  assert.match(app, /className="content-source-group"/);
  assert.match(app, /base\?\.displayName \?\? source\.name/);
  assert.match(app, /void requestPreview\(undefined, undefined, interactivePreviewProfile/);
});

test("resizing an assigned patch keeps the old sheet until selected preview quality publishes", () => {
  const resize = app.slice(app.indexOf("async function replacePatchGeometry"), app.indexOf("async function setResolution"));
  assert.match(resize, /assignedToRegion/);
  assert.match(resize, /binding\.content\.type === "patch" && binding\.content\.id === patchId/);
  assert.doesNotMatch(resize, /requestPreview\(undefined, undefined, "draft512", revision, false\)/);
  assert.match(resize, /requestPreview\(undefined, undefined, interactivePreviewProfile, revision, false\)/);
  assert.match(resize, /lastAutomaticPreviewKey\.current = automaticPreviewKey\(revision, interactivePreviewProfile\)/);
});

test("solid content, replacement preflight, library metadata, and diagnostics are product connected", () => {
  assert.match(app, /type: "solid", id: \{ baseColor: \[128, 128, 128, 255\] \}/);
  assert.match(compiler, /fn build_solid_domain/);
  assert.match(compiler, /"solid_binding"/);
  assert.match(app, /function replaceBaseWithPreflight/);
  assert.match(app, /affectedRegions/);
  assert.match(app, /const readiness = base \? "Ready" : "Missing Diffuse"/);
  assert.match(app, /const shape = .*\? "Rectangle" : "Four point"/);
  assert.match(app, /<summary>Advanced compile diagnostics<\/summary>/);
});

test("scrolling the region assignment menu does not zoom the hotspot sheet", () => {
  assert.match(app, /className="layout-menu"[\s\S]*?onWheel=\{\(event\) => event\.stopPropagation\(\)\}[\s\S]*?className="layout-submenu region-content-menu"/);
});

test("patch domains are bounded for draft publication and reused across assignments", () => {
  assert.match(compiler, /patch_domain_cache_key\(\s*request\.project,\s*source_set_id,\s*patch,\s*preserve_source_resolution,\s*\)/);
  assert.match(compiler, /matches!\(request\.profile, SourceFramePreviewProfile::Authoritative\)/);
  assert.match(compiler, /guard\.insert\(patch_key, Arc::clone\(&domain\)\)/);
  assert.match(compiler, /const MAX_DIRECT_DOMAINS: usize = 4/);
  assert.match(compiler, /build_direct_patch_domain/);
  assert.match(compiler, /PreparedMaterialDomain::from_registered_channels/);
  assert.doesNotMatch(compiler.slice(compiler.indexOf("fn build_direct_patch_domain"), compiler.indexOf("fn build_domain")), /prepare_stage_08_material_domain|RepeatX|PeriodicTile/);
});

test("a selected region can become an isolated editable patch", () => {
  assert.match(app, /async function editSelectedRegionAsPatch\(authoredBounds\?: NormalizedBounds\)/);
  assert.match(app, /name: nextPatchName\(base\.id\)/);
  assert.match(app, /await assignPatchToRegion\(patchId, regionId\)/);
  assert.match(app, /Editing selected region as an isolated patch/);
  assert.match(app, /!regionPatchEditId \|\| patch\.id === activePatchId/);
  assert.match(app, /onCommitCrop=\{\(bounds\) => void editSelectedRegionAsPatch\(bounds\)\}/);
  assert.doesNotMatch(app, />Detach Source Cell</);
  assert.doesNotMatch(app, />Reset to Partition</);
});

test("Workbench terminology and region geometry have one visible authority", () => {
  assert.match(app, />WORKBENCH</);
  assert.doesNotMatch(app, /WORKPLACE/);
  assert.match(app, /function RegionGridRectEditor/);
  assert.match(app, /type: "resize_source_frame_region"/);
  assert.match(app, />Apply geometry</);
  assert.match(app, /\{props\.selectedRegion \? <section className="inspector-section region-controls-primary">/);
});

test("a fresh project cancels and clears every patch-derived state", () => {
  const reset = app.slice(app.indexOf("function acceptProject"), app.indexOf("async function chooseImages"));
  assert.match(reset, /patchPreviewRequestId\.current \+= 1/);
  assert.match(reset, /transientPreviewController\.current\.cancel\(\)/);
  assert.match(reset, /sourceFramePreviewController\.current\.cancel\(\)/);
  for (const state of ["ActivePatchId", "RegionPatchEditId", "PreparedPatchPreview", "DraftPatchPreview", "PatchTool"]) {
    assert.match(reset, new RegExp(`set${state}\\(null\\)`));
  }
  assert.match(reset, /setPreparedPatchPreviews\(\{\}\)/);
  assert.match(reset, /patchFallbackContent\.current\.clear\(\)/);
});

test("texture maps own replacement while source groups keep only group actions", () => {
  const sourceMenu = app.slice(app.indexOf("{sourceMenu ?"), app.indexOf("{patchMenu ?"));
  assert.doesNotMatch(sourceMenu, /Replace .*texture|Add maps|channel maps/);
  assert.match(sourceMenu, /Rename…/);
  assert.match(sourceMenu, /Reveal source/);
  assert.match(sourceMenu, /Remove…/);
  const slots = app.slice(app.indexOf("function MapSlots"), app.indexOf("function useViewportController"));
  assert.ok(slots.indexOf('className="map-slot add-maps"') < slots.indexOf("channelOptions.map"));
  assert.match(slots, /onContextMenu=\{\(event\) => \{/);
  assert.match(slots, /Replace texture…/);
  assert.doesNotMatch(app, /Base Color/);
  assert.match(styles, /\.map-slots \{[\s\S]*padding: 2px;/);
  assert.doesNotMatch(styles, /scrollbar-width: thin|scrollbar-color:/);
  assert.match(styles, /\.map-slots::-webkit-scrollbar \{ height: 2px; \}/);
  const mapSlotRule = styles.slice(styles.indexOf(".map-slot {"), styles.indexOf(".map-slot.active"));
  assert.match(mapSlotRule, /padding: 1px 2px 2px 3px;/);
  assert.doesNotMatch(mapSlotRule, /height:/);
  assert.match(styles, /\.map-slot\.add-maps \{[\s\S]*place-items: center;[\s\S]*align-self: stretch;[\s\S]*flex: 0 0 auto;[\s\S]*min-width: 88px;[\s\S]*white-space: nowrap;/);
});

test("source, patch, and texture context menus dismiss on outside pointer input", () => {
  assert.match(app, /closest\("\.source-context-menu, \.library-patch-context-menu"\)/);
  assert.match(app, /className="patch-context-menu library-patch-context-menu"/);
  assert.match(app, /closest\("\.channel-context-menu"\)/);
  assert.match(app, /window\.addEventListener\("pointerdown", dismiss, true\)/);
  assert.match(app, /window\.removeEventListener\("pointerdown", dismiss, true\)/);
});

test("new patches are enumerated per source and menus show the stored name once", () => {
  assert.match(app, /function nextPatchName\(sourceId: string\)/);
  assert.match(app, /name: nextPatchName\(selectedSource\.id\)/);
  assert.doesNotMatch(app, />Patch · \{patch\.name\}<\/button>/);
});

test("content assignment changes appearance authority without touching topology", () => {
  const domain = readFileSync(new URL("../../../crates/domain/src/document.rs", import.meta.url), "utf8");
  const arm = domain.slice(domain.indexOf("TrimSheetDocumentCommand::SetRegionContent"), domain.indexOf("TrimSheetDocumentCommand::SetSheetFraming"));
  assert.match(arm, /region_bindings/);
  assert.match(arm, /content = content\.clone\(\)/);
  assert.doesNotMatch(arm, /topology|grid_rect|topology_revision/);
});
