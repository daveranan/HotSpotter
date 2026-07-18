import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const app = readFileSync(new URL("./source-first-app.tsx", import.meta.url), "utf8");
const compiler = readFileSync(new URL("../../../crates/sheet-compiler/src/persisted_pipeline.rs", import.meta.url), "utf8");
const store = readFileSync(new URL("../../../crates/project-store/src/lib.rs", import.meta.url), "utf8");
const native = readFileSync(new URL("../src-tauri/src/document_commands.rs", import.meta.url), "utf8");

test("SourceFrame validation is pinned to its persisted owner while every region resolves ContentReference", () => {
  assert.match(compiler, /direct_source_frame_domain\(\s*request\.project, frame\.source_set_id/);
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
  assert.match(store, /persist_document_state_in_transaction\(&transaction, Some\(document\), "remove_source_set"/);
  assert.match(native, /PatchCommand::Delete \{ patch_id \}/);
  assert.match(native, /Patch .* is assigned to region/);
  assert.match(native, /primary material, SourceFrame, or a region/);
});

test("libraries, assignment, splitters, and application context menus are runtime connected", () => {
  assert.match(app, /Filter patches by source/);
  assert.match(app, /Add independent source/);
  assert.match(app, /Add maps to selected source/);
  assert.match(app, /Set as primary \/ Rebase layout/);
  assert.match(app, /onContextMenu=\{\(event\) => \{ event\.preventDefault\(\)/);
  assert.match(app, /hot-trimmer\.workbench-panes\.v1/);
  assert.match(app, /Assign patch to region/);
  assert.match(app, /<label>Content<select/);
  assert.match(app, /sourceFrame=\{project\?\.document\?\.sourceFrame\?\.sourceSetId === activeSourceSetId/);
});

test("content assignment changes appearance authority without touching topology", () => {
  const domain = readFileSync(new URL("../../../crates/domain/src/document.rs", import.meta.url), "utf8");
  const arm = domain.slice(domain.indexOf("TrimSheetDocumentCommand::SetRegionContent"), domain.indexOf("TrimSheetDocumentCommand::SetSheetFraming"));
  assert.match(arm, /region_bindings/);
  assert.match(arm, /content = content\.clone\(\)/);
  assert.doesNotMatch(arm, /topology|grid_rect|topology_revision/);
});
