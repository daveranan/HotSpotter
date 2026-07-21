import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { feedbackPreviewRegionAfterCommand, feedbackViewAfterCommand, sanitizeEdgeDetailIntent } from "./feedback-workbench-contract.ts";

test("mvp-edge-wear corrects invalid numeric UI values before the native command", () => {
  const sanitized = sanitizeEdgeDetailIntent({
    schemaVersion: 1, enabled: true, wearAmount: 5, intensity: -2, edgeWidthM: 0,
    bevelRadiusM: -1, edgeSoftness: 2, breakupAmount: 2,
    breakupScaleM: Number.NaN, microDetailAmount: -1, microDetailScaleM: 0,
    seed: -7.8, sourceHeightInfluence: 2, sourceLuminanceInfluence: -1,
    heightAmplitudeM: Number.NaN, normalDetailStrength: 3,
    hueShiftDegrees: Number.POSITIVE_INFINITY, saturationMultiplier: -1,
    valueMultiplier: Number.NaN, roughnessOffset: Number.NaN,
    exposedMetalEnabled: false, metallicOffset: 0.9,
  });
  assert.equal(sanitized.schemaVersion, 1);
  assert.equal(sanitized.wearAmount, 1);
  assert.equal(sanitized.intensity, 0);
  assert.ok(sanitized.edgeWidthM > 0);
  assert.ok(sanitized.breakupScaleM > 0);
  assert.equal(sanitized.seed, 0);
  assert.equal(sanitized.saturationMultiplier, 0);
  assert.equal(sanitized.metallicOffset, 0);
  assert.ok(Object.values(sanitized).filter((value): value is number => typeof value === "number").every(Number.isFinite));
});

test("mvp-edge-wear apply acquires a deterministic preview region when none is selected", () => {
  const global = feedbackPreviewRegionAfterCommand(
    { type: "set_edge_detail", intent: edgeDetail() },
    null,
    ["region-a", "region-b"],
  );
  assert.equal(global, "region-a");

  const targeted = feedbackPreviewRegionAfterCommand(
    { type: "set_edge_detail", intent: edgeDetail({ targetRegion: "region-b", seed: 7 }) },
    null,
    ["region-a", "region-b"],
  );
  assert.equal(targeted, "region-b");
  assert.equal(feedbackViewAfterCommand({ type: "set_edge_detail", intent: edgeDetail({ enabled: false }) }, "stage15Height"), "stage16BaseColor");
});

const edgeDetail = (patch: Partial<ReturnType<typeof sanitizeEdgeDetailIntent>> = {}) => ({
  schemaVersion: 1 as const, enabled: true, wearAmount: 0.55, intensity: 0.8,
  edgeWidthM: 0.004, bevelRadiusM: 0.0025, edgeSoftness: 0.3,
  breakupAmount: 0.7, breakupScaleM: 0.012, microDetailAmount: 0.25,
  microDetailScaleM: 0.002, seed: 201516, sourceHeightInfluence: 0.65,
  sourceLuminanceInfluence: 0.2, heightAmplitudeM: -0.00035,
  normalDetailStrength: 1, hueShiftDegrees: 0, saturationMultiplier: 0.55,
  valueMultiplier: 1.12, roughnessOffset: 0.18, exposedMetalEnabled: false,
  metallicOffset: 0, ...patch,
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

test("mvp-edge-wear Edge Detail V1 fields agree with the native persisted contract", () => {
  const sharedFixture = JSON.parse(read("fixtures/edge-detail-intent-v1.json"));
  assert.deepEqual(edgeDetail(), sharedFixture, "TypeScript defaults match every shared V1 fixture value");
  const ts = read("packages/ipc-contracts/src/document-contracts.ts");
  const rust = read("crates/domain/src/document.rs");
  const fields = ["schemaVersion", "wearAmount", "intensity", "edgeWidthM", "bevelRadiusM", "edgeSoftness",
    "breakupAmount", "breakupScaleM", "microDetailAmount", "microDetailScaleM", "seed",
    "sourceHeightInfluence", "sourceLuminanceInfluence", "heightAmplitudeM", "normalDetailStrength",
    "hueShiftDegrees", "saturationMultiplier", "valueMultiplier", "roughnessOffset",
    "exposedMetalEnabled", "metallicOffset"];
  for (const field of fields) assert.match(ts, new RegExp(`${field}:`));
  for (const field of fields.map((field) => field.replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`))) {
    assert.match(rust, new RegExp(`pub ${field}:`));
  }
  assert.match(rust, /EDGE_DETAIL_INTENT_SCHEMA_VERSION: u16 = 1/);
  const focusedNative = read("crates/sheet-compiler/src/persisted_pipeline.rs");
  assert.match(focusedNative, /mvp_edge_wear_compiler_covers_global_target_reorder_and_authoritative_role_identity/);
  assert.match(focusedNative, /mvp_edge_wear_compiler_rejects_invalid_and_subpixel_intents_and_disables_cleanly/);
  const store = read("crates/project-store/src/lib.rs");
  assert.match(store, /mvp_edge_wear_project_save_reopen_migrates_once_to_edge_detail_v1/);
  assert.match(read("apps/desktop/src-tauri/src/document_commands.rs"), /"18": \{ "state": FeedbackExecutionState::NotInstalled/);
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
