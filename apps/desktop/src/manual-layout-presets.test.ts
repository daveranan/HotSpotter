import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import type { TrimSheetDocument } from "@hot-trimmer/ipc-contracts";
import { authoredGridResolutions, cellDragRect, diagonalCascadePreset, newBlankPreset, presetExactlyCoversGrid, rescalePreset, snapshotDocumentPreset, snappedGridPoint, sourceFrameGridBounds } from "./manual-layout-presets.ts";

test("manual-layout-presets: built-ins are deterministic exact-cover authored snapshots", () => {
  assert.equal(new Set(diagonalCascadePreset.regions.map((region) => region.presetRegionKey)).size, diagonalCascadePreset.regions.length);
  assert.equal(presetExactlyCoversGrid(diagonalCascadePreset), true);
  assert.equal(presetExactlyCoversGrid(newBlankPreset(64)), true);
  assert.equal(JSON.stringify(diagonalCascadePreset).includes("recipe"), false);
});

test("manual-layout-presets: Diagonal Cascade exactly matches the classic source hotspot golden", () => {
  const svg = readFileSync("../../target/hierarchical-goldens/hierarchical-classic-source-hotspot.golden.svg", "utf8");
  const golden = [...svg.matchAll(/<rect x="(\d+)" y="(\d+)" width="(\d+)" height="(\d+)"/g)]
    .map((match) => ({ x: Number(match[1]), y: Number(match[2]), width: Number(match[3]), height: Number(match[4]) }));
  assert.deepEqual(diagonalCascadePreset.regions.map((region) => region.gridRect), golden);
});

test("manual-layout-presets: grid changes preserve representable boundaries and flag quantization", () => {
  assert.equal(rescalePreset(diagonalCascadePreset, 128).exact, true);
  assert.equal(rescalePreset(diagonalCascadePreset, 24).exact, false);
  for (const size of authoredGridResolutions) assert.equal(presetExactlyCoversGrid(rescalePreset(diagonalCascadePreset, size).preset), true, `${size} must remain exact-cover`);
});

test("manual-layout-presets: displayed hover snap is the committed point at zoomed boundaries", () => {
  const rect = { left: 10.25, top: 20.5, width: 511.5, height: 383.25 };
  assert.deepEqual(snappedGridPoint(10.25, 20.5, rect, 64, 64), { x: 0, y: 0, cellX: 0, cellY: 0, centerX: .5, centerY: .5 });
  assert.deepEqual(snappedGridPoint(277.125, 219.4, rect, 64, 64), { x: 33, y: 33, cellX: 33, cellY: 33, centerX: 33.5, centerY: 33.5 });
  assert.deepEqual(snappedGridPoint(521.75, 403.75, rect, 64, 64), { x: 64, y: 64, cellX: 63, cellY: 63, centerX: 63.5, centerY: 63.5 });
  assert.deepEqual(snappedGridPoint(14, 24, rect, 64, 64), { x: 0, y: 1, cellX: 0, cellY: 0, centerX: .5, centerY: .5 });
  assert.deepEqual(cellDragRect(4, 7, 4, 7), { x: 4, y: 7, width: 1, height: 1 });
  assert.deepEqual(cellDragRect(4, 7, 2, 9), { x: 2, y: 7, width: 3, height: 3 });
});

test("manual-layout-presets: selected authored region retains its exact SourceFrame preview", () => {
  assert.deepEqual(sourceFrameGridBounds(
    { x: .1, y: .2, width: .8, height: .6 }, { width: 64, height: 64 }, { x: 16, y: 32, width: 16, height: 8 },
  ), { x: .30000000000000004, y: .5, width: .2, height: .075 });
});

test("manual-layout-presets: region context menu escapes the transformed sheet coordinate space", () => {
  const app = readFileSync("src/source-first-app.tsx", "utf8");
  assert.match(app, /createPortal\(<div className="layout-menu"[\s\S]*document\.body\)/);
  assert.match(app, /Math\.min\(event\.clientX, window\.innerWidth - 196\)/);
  assert.match(app, /closest\("\.layout-menu"\)\) setLayoutMenu\(null\)/);
  assert.match(app, /window\.addEventListener\("blur", dismissBlur\)/);
});

test("manual-layout-presets: hotspot history is keyboard-only", () => {
  const app = readFileSync("src/source-first-app.tsx", "utf8");
  assert.doesNotMatch(app, /hotspot-history/);
  assert.match(app, /key === "z" && !event\.shiftKey/);
  assert.match(app, /key === "y" \|\| \(key === "z" && event\.shiftKey\)/);
  assert.match(app, /input, textarea, select, \[contenteditable=true\]/);
  assert.match(app, /documentHistoryBusy\.current/);
  assert.match(app, /setArtifact\(\(prior\) => retopologizeArtifact\(prior, next\)\)/);
  assert.doesNotMatch(app, /priorTopologyHash !== nextTopologyHash \? retopologizeArtifact\(prior, next\) : null/);
});

test("manual-layout-presets: grid resolution uses every authored product stop", () => {
  assert.deepEqual([...authoredGridResolutions], [16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256]);
  const app = readFileSync("src/source-first-app.tsx", "utf8");
  assert.match(app, /aria-label="Logical grid resolution" type="range"/);
});

test("manual-layout-presets: Save As preserves keys by authored rectangle after topology reorder", () => {
  const document = {
    authoredLayoutPreset: { ...newBlankPreset(8), regions: [
      { ...newBlankPreset(8).regions[0]!, presetRegionKey: "left", gridRect: { x: 0, y: 0, width: 4, height: 8 } },
      { ...newBlankPreset(8).regions[0]!, presetRegionKey: "right", gridRect: { x: 4, y: 0, width: 4, height: 8 } },
    ] },
    logicalGrid: { schemaVersion: 1, width: 8, height: 8 },
    renderSettings: { outputSize: { width: 2048, height: 2048 } },
    topology: { regions: [
      { id: "region-right", displayName: "Right", gridRect: { x: 4, y: 0, width: 4, height: 8 } },
      { id: "region-left", displayName: "Left", gridRect: { x: 0, y: 0, width: 4, height: 8 } },
    ] },
  } as unknown as TrimSheetDocument;
  assert.deepEqual(snapshotDocumentPreset(document, "user.saved", "Saved").regions.map((region) => region.presetRegionKey), ["right", "left"]);
});

test("manual-layout-presets: settled Base Color and user preset storage use authoritative native paths", () => {
  const app = readFileSync("src/source-first-app.tsx", "utf8");
  assert.match(app, /localTopologyPending \|\| !imageUrl/);
  assert.match(app, /textureVisible && imageUrl \? <img src=\{imageUrl\}/);
  assert.match(app, /invoke<AuthoredLayoutPreset\[]>\("list_authored_layout_presets"/);
  assert.match(app, /invoke<AuthoredLayoutPreset\[]>\("save_authored_layout_preset"/);
  assert.match(app, /invoke<AuthoredLayoutPreset\[]>\("delete_authored_layout_preset"/);
  assert.match(app, /set_authored_layout_preset_snapshot/);
});
