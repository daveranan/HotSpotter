import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { feedbackPreviewRegionAfterCommand, feedbackViewAfterCommand, sanitizeEdgeWearIntent } from "./feedback-workbench-contract.ts";

test("mvp-edge-wear corrects invalid numeric UI values before the native command", () => {
  const sanitized = sanitizeEdgeWearIntent({
    enabled: true, coverage: 5, strength: -2, edgeWidthM: 0,
    breakupScaleM: Number.NaN, breakupSeed: -7.8, heightAmplitudeM: Number.NaN,
    hueShiftDegrees: Number.POSITIVE_INFINITY, saturationMultiplier: -1,
    valueOffset: Number.NaN, roughnessOffset: Number.NaN,
    exposedMetalEnabled: false, metallicOffset: 0.9,
  });
  assert.equal(sanitized.coverage, 1);
  assert.equal(sanitized.strength, 0);
  assert.ok(sanitized.edgeWidthM > 0);
  assert.ok(sanitized.breakupScaleM > 0);
  assert.equal(sanitized.breakupSeed, 0);
  assert.equal(sanitized.saturationMultiplier, 0);
  assert.equal(sanitized.metallicOffset, 0);
  assert.ok(Object.values(sanitized).filter((value): value is number => typeof value === "number").every(Number.isFinite));
});

test("mvp-edge-wear apply acquires a deterministic preview region when none is selected", () => {
  const global = feedbackPreviewRegionAfterCommand(
    { type: "set_edge_wear", intent: {
      enabled: true, coverage: 0.55, strength: 0.8, edgeWidthM: 0.004,
      breakupScaleM: 0.012, breakupSeed: 201516, heightAmplitudeM: -0.00035,
      hueShiftDegrees: 0, saturationMultiplier: 0.55, valueOffset: 0.12,
      roughnessOffset: 0.18, exposedMetalEnabled: false, metallicOffset: 0,
    } },
    null,
    ["region-a", "region-b"],
  );
  assert.equal(global, "region-a");

  const targeted = feedbackPreviewRegionAfterCommand(
    { type: "set_edge_wear", intent: {
      enabled: true, targetRegion: "region-b", coverage: 1, strength: 1, edgeWidthM: 0.004,
      breakupScaleM: 0.012, breakupSeed: 7, heightAmplitudeM: -0.00035,
      hueShiftDegrees: 0, saturationMultiplier: 1, valueOffset: 0,
      roughnessOffset: 0, exposedMetalEnabled: false, metallicOffset: 0,
    } },
    null,
    ["region-a", "region-b"],
  );
  assert.equal(targeted, "region-b");
  assert.equal(feedbackViewAfterCommand({ type: "set_edge_wear", intent: {
    enabled: false, coverage: 0, strength: 0, edgeWidthM: 0.004,
    breakupScaleM: 0.012, breakupSeed: 0, heightAmplitudeM: 0,
    hueShiftDegrees: 0, saturationMultiplier: 1, valueOffset: 0,
    roughnessOffset: 0, exposedMetalEnabled: false, metallicOffset: 0,
  } }, "stage15Height"), "stage16BaseColor");
});

const root = new URL("../../..", import.meta.url);
const read = (path: string) => readFileSync(new URL(path, root), "utf8");

test("mvp-edge-wear stays on the compiled GPU requested-map path", () => {
  const shader = read("crates/preview/src/gpu_base_color.wgsl");
  const normal = read("crates/preview/src/gpu_normal_from_height.wgsl");
  const executor = read("crates/sheet-compiler/src/atlas_executor.rs");
  assert.match(shader, /fn edge_wear_mask/);
  assert.match(shader, /edge_wear_width_m/);
  assert.match(shader, /edge_wear_breakup_scale_m/);
  assert.match(shader, /fn edge_value_noise/);
  assert.match(shader, /fn edge_fbm/);
  assert.match(shader, /spatial_feather_m/);
  assert.doesNotMatch(shader, /let cell = vec2<u32>\(floor\(physical/);
  assert.match(shader, /header\.map_kind == 8u/);
  assert.match(executor, /MaterialMapKind::EdgeMask => 8/);
  assert.match(executor, /execute_height_normal_gpu/);
  assert.match(normal, /textureLoad\(final_height_tex/);
  assert.doesNotMatch(normal, /mix\([^\n]*authored_decoded/);
});

test("mvp-edge-wear is eligibility gated, deterministic, and metallic explicit-only", () => {
  const shader = read("crates/preview/src/gpu_base_color.wgsl");
  assert.match(shader, /edge_wear_flags & 2u/);
  assert.match(shader, /edge_wear_flags & 4u/);
  assert.match(shader, /edge_wear_flags & 8u/);
  assert.match(shader, /edge_wear_flags & 16u/);
  assert.match(shader, /edge_wear_seed/);
  assert.match(shader, /edge_wear_flags & 32u/);
  assert.match(shader, /select\(0\.0, wear \* cmd\.edge_wear_metallic_offset, explicit_metal\)/);
});

test("mvp-edge-wear UI is a right-docked ordered card with the five inspection routes", () => {
  const workbench = read("apps/desktop/src/feedback-workbench.tsx");
  const shell = read("apps/desktop/src/source-first-app.tsx");
  const css = read("apps/desktop/src/document-app.css");
  for (const label of ["Base Color", "Mask", "Height", "Normal", "Roughness"]) assert.match(workbench, new RegExp(`>${label}<`));
  assert.match(workbench, /className="layer-card-list"/);
  assert.match(shell, /!feedbackWorkbenchOpen/);
  assert.match(shell, /minmax\(360px, 430px\)/);
  assert.match(css, /\.feedback-workbench \{ position: relative/);
});
