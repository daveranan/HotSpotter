import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
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
  const shader = read("crates/preview/src/gpu_edge_detail.wgsl");
  const baseColor = read("crates/preview/src/gpu_base_color.wgsl");
  const normal = read("crates/preview/src/gpu_normal_from_height.wgsl");
  const executor = read("crates/sheet-compiler/src/atlas_executor.rs");
  assert.match(shader, /var stage15_sdf: texture_2d<f32>/);
  assert.match(shader, /var stage15_semantics: texture_2d<f32>/);
  assert.match(shader, /fn value_noise/);
  assert.match(shader, /fn role_coordinates/);
  assert.match(shader, /Periodic angular embedding/);
  assert.match(shader, /fn srgb_to_linear/);
  assert.match(shader, /clamp\(local \+ vec2<i32>\(x, y\), stencil_min, stencil_max\)/);
  assert.match(shader, /transition_micro/);
  assert.match(shader, /cmd\.cap_major_axis == 1u/);
  assert.match(shader, /var core_out: texture_storage_2d<r32float/);
  assert.match(shader, /var transition_out: texture_storage_2d<r32float/);
  assert.match(shader, /var fade_out: texture_storage_2d<r32float/);
  assert.match(shader, /var combined_out: texture_storage_2d<r32float/);
  assert.match(shader, /var height_out: texture_storage_2d<r32float/);
  assert.match(shader, /sqrt\(max\(0\.0, 1\.0 - \(1\.0 - x\)/);
  assert.doesNotMatch(shader, /normal.*rgb.*height/i);
  assert.match(executor, /GpuAtlasPipelineKind::EdgeDetail/);
  assert.match(executor, /execute_or_load_edge_detail_fields/);
  assert.match(executor, /edge-detail\.core/);
  assert.match(executor, /edge-detail\.transition/);
  assert.match(executor, /edge-detail\.fade/);
  assert.match(executor, /edge-detail\.combined/);
  assert.match(executor, /edge-detail\.height/);
  assert.match(executor, /EdgeDetailSourceModulationRoute::RegisteredHeight => 1/);
  assert.match(executor, /EdgeDetailSourceModulationRoute::HighPassedLinearLuminance => 2/);
  assert.match(executor, /selected_source_routes=/);
  assert.match(executor, /lod_fallbacks=/);
  assert.match(executor, /stage15_sdf_identity=/);
  assert.match(executor, /bounded-resolution-cross-section/);
  assert.match(executor, /tile-origin overlap changed/);
  assert.match(baseColor, /var source_tex: texture_2d_array<f32>/, "ED-3 still owns final source composition");
  assert.match(executor, /execute_height_normal_gpu/);
  assert.match(normal, /textureLoad\(final_height_tex/);
  assert.doesNotMatch(normal, /mix\([^\n]*authored_decoded/);
});

test("mvp-edge-wear is eligibility gated, deterministic, and metallic explicit-only", () => {
  const shader = read("crates/preview/src/gpu_edge_detail.wgsl");
  const executor = read("crates/sheet-compiler/src/atlas_executor.rs");
  assert.match(shader, /edge_mask: u32/);
  assert.match(shader, /cmd\.seed/);
  assert.match(shader, /atlas_pixel = id\.xy \+ vec2<u32>\(header\.tile_x, header\.tile_y\)/);
  assert.match(shader, /inside_rect\(atlas_pixel/);
  assert.match(shader, /semantic_bits & 1u/);
  assert.match(executor, /commands\.is_empty\(\)/);
});

test("mvp-edge-wear runs the real native Edge Detail GPU fixture", () => {
  const result = spawnSync("cargo", ["test", "-p", "hot-trimmer-sheet-compiler", "native_edge_detail_gpu_fixture_covers_roles_masks_height_and_cache"], {
    cwd: new URL("../../..", import.meta.url), encoding: "utf8",
  });
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
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
  assert.match(focusedNative, /mvp_edge_wear_preview_scales_subpixel_detail_without_changing_authored_intent/);
  assert.match(focusedNative, /preview_scale_floor/);
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
