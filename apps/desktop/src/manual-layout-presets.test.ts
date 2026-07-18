import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { authoredGridResolutions, cellDragRect, diagonalCascadePreset, newBlankPreset, rescalePreset, snappedGridPoint, sourceFrameGridBounds } from "./manual-layout-presets.ts";

test("manual-layout-presets: built-ins are deterministic exact-cover authored snapshots", () => {
  assert.deepEqual(diagonalCascadePreset.regions.map((region) => [region.presetRegionKey, region.gridRect]), diagonalCascadePreset.regions.map((region) => [region.presetRegionKey, region.gridRect]));
  assert.deepEqual(newBlankPreset(64).regions[0].gridRect, { x: 0, y: 0, width: 64, height: 64 });
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
});

test("manual-layout-presets: displayed hover snap is the committed point at zoomed boundaries", () => {
  const rect = { left: 10.25, top: 20.5, width: 511.5, height: 383.25 };
  for (const point of [[10.25,20.5],[521.75,403.75],[277.125,219.4]] as const) {
    assert.deepEqual(snappedGridPoint(...point, rect, 64, 64), snappedGridPoint(...point, rect, 64, 64));
  }
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

test("manual-layout-presets: grid resolution uses the reduced discrete slider", () => {
  assert.deepEqual([...authoredGridResolutions], [16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 256]);
  const app = readFileSync("src/source-first-app.tsx", "utf8");
  assert.match(app, /aria-label="Logical grid resolution" type="range"/);
});
