import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { EDGE_DETAIL_PRESETS, edgeDetailInspectionForView, edgeDetailIntentFromPreset, edgeDetailPresetForIntent, feedbackPreviewRegionAfterCommand, feedbackRequestIsCurrent, feedbackViewAfterCommand, sanitizeEdgeDetailIntent } from "./feedback-workbench-contract.ts";

test("mvp-edge-wear ignores a stale cancellation after newer preview publication", () => {
  assert.equal(feedbackRequestIsCurrent(5, 6), false);
  assert.equal(feedbackRequestIsCurrent(6, 6), true);
});

test("mvp-edge-wear exposes four editable typed presets with distinct exact intents", () => {
  const names = ["Soft Worn Edge", "Chipped Paint", "Heavy Erosion", "Clean Bevel"] as const;
  assert.deepEqual(Object.keys(EDGE_DETAIL_PRESETS), names);
  const intents = names.map((name) => edgeDetailIntentFromPreset(name, edgeDetail({ targetRegion: "stable-region-uuid" })));
  assert.equal(new Set(intents.map((intent) => JSON.stringify(intent))).size, names.length);
  assert.ok(intents.every((intent) => intent.targetRegion === "stable-region-uuid"));
  assert.equal(intents[3].breakupAmount, 0);
  assert.equal(intents[3].microDetailAmount, 0);
  assert.ok(intents[0].edgeWidthM <= 0.006, "the default wear band stays narrow");
  assert.ok(intents[0].saturationMultiplier >= 0.9, "the default does not bleach Base Color");
  assert.ok(intents[0].valueMultiplier <= 1.06, "the default does not paint a bright halo");
  assert.deepEqual(intents.map(edgeDetailPresetForIntent), names);
  assert.equal(edgeDetailPresetForIntent({ ...intents[0], heightAmplitudeM: -0.0014 }), "Custom");
});

test("mvp-edge-wear maps every inspection route to one typed field", () => {
  const routes = [
    ["edgeDetailCoreMask", "coreMask"], ["edgeDetailTransitionMask", "transitionMask"],
    ["edgeDetailFadeMask", "fadeMask"], ["edgeDetailCombinedMask", "combinedMask"],
    ["edgeDetailHeightContribution", "heightContribution"], ["edgeDetailFinalHeight", "finalHeight"],
    ["edgeDetailFinalNormal", "finalNormal"], ["edgeDetailBaseColorContribution", "baseColorContribution"],
    ["edgeDetailRoughnessContribution", "roughnessContribution"], ["edgeDetailMetallicContribution", "metallicContribution"],
  ] as const;
  for (const [view, field] of routes) assert.equal(edgeDetailInspectionForView(view), field);
});

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
  assert.equal(feedbackViewAfterCommand({ type: "set_edge_detail", intent: edgeDetail({ enabled: false }) }, "stage15Height"), "edgeDetailCombinedMask");
});

const edgeDetail = (patch: Partial<ReturnType<typeof sanitizeEdgeDetailIntent>> = {}) => ({
  schemaVersion: 1 as const, enabled: true, wearAmount: 0.5, intensity: 0.72,
  edgeWidthM: 0.006, bevelRadiusM: 0.004, edgeSoftness: 0.2,
  breakupAmount: 0.78, breakupScaleM: 0.012, microDetailAmount: 0.35,
  microDetailScaleM: 0.0015, seed: 201516, sourceHeightInfluence: 0.55,
  sourceLuminanceInfluence: 0.16, heightAmplitudeM: -0.0008,
  normalDetailStrength: 1.1, hueShiftDegrees: 0, saturationMultiplier: 0.96,
  valueMultiplier: 1.03, roughnessOffset: 0.1, exposedMetalEnabled: false,
  metallicOffset: 0, ...patch,
});

const root = new URL("../../..", import.meta.url);
const read = (path: string) => readFileSync(new URL(path, root), "utf8");

test("mvp-edge-wear stays on the compiled GPU requested-map path", () => {
  const shader = read("crates/preview/src/gpu_edge_detail.wgsl");
  const baseColor = read("crates/preview/src/gpu_base_color.wgsl");
  const normal = read("crates/preview/src/gpu_normal_from_height.wgsl");
  const composition = read("crates/preview/src/gpu_edge_detail_composition.wgsl");
  const executor = read("crates/sheet-compiler/src/atlas_executor.rs");
  assert.match(shader, /var stage15_sdf: texture_2d<f32>/);
  assert.match(shader, /var stage15_semantics: texture_2d<f32>/);
  assert.match(shader, /fn value_noise/);
  assert.match(shader, /fn role_coordinates/);
  assert.match(shader, /Periodic angular embedding/);
  assert.match(shader, /fn srgb_to_linear/);
  assert.match(shader, /fn source_luminance_height/);
  assert.match(shader, /source_linear_luminance\(local\) - 0\.5/);
  assert.match(shader, /cmd\.source_height_range_m/);
  assert.match(shader, /surface_height \+ edge_height/);
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
  assert.match(executor, /edge-detail\.core\.display/);
  assert.match(executor, /edge-detail\.transition\.display/);
  assert.match(executor, /edge-detail\.fade\.display/);
  assert.match(read("crates/sheet-compiler/src/intermediate_atlas.rs"), /rendered_intermediate_tiles/);
  assert.match(read("apps/desktop/src-tauri/src/document_commands.rs"), /feedback_intermediate_display/);
  assert.match(executor, /edge-detail\.base-color-contribution\.display/);
  assert.match(executor, /edge-detail\.roughness-contribution\.display/);
  assert.match(executor, /edge-detail\.metallic-contribution\.display/);
  assert.match(executor, /EdgeDetailSourceModulationRoute::RegisteredHeight => 1/);
  assert.match(executor, /EdgeDetailSourceModulationRoute::HighPassedLinearLuminance => 2/);
  assert.match(executor, /selected_source_routes=/);
  assert.match(executor, /lod_fallbacks=/);
  assert.match(executor, /display_format=Rgba8UnormLinear/);
  assert.match(executor, /scalar-display-rgba8/);
  assert.match(executor, /stage15_sdf_identity=/);
  assert.match(executor, /bounded-resolution-cross-section/);
  assert.match(executor, /tile-origin overlap changed/);
  assert.match(baseColor, /var source_tex: texture_2d_array<f32>/, "ED-3 still owns final source composition");
  assert.doesNotMatch(baseColor, /edge_wear_mask|edge_wear_flags|edge_wear_coverage/);
  assert.match(composition, /var physical_height_m = authored_height_m \+ textureLoad\(stage15_height_tex/);
  assert.match(composition, /cmd\.source_height_range_m/);
  assert.match(composition, /base\.r != -1\.0/);
  assert.match(composition, /cmd\.normal_detail_strength - 1\.0/);
  assert.match(composition, /cmd\.normal_detail_strength/);
  assert.match(composition, /core_tex, p, 0\)\.r \* cmd\.intensity/);
  assert.match(composition, /textureLoad\(stage16_height_tex/);
  assert.match(composition, /textureLoad\(edge_height_tex/);
  assert.match(composition, /let core = clamp\(textureLoad\(core_tex/);
  assert.match(composition, /let transition = clamp\(textureLoad\(transition_tex/);
  assert.match(composition, /let fade = clamp\(textureLoad\(fade_tex/);
  assert.match(composition, /let mask = clamp\(textureLoad\(combined_tex/);
  assert.match(composition, /roughness = clamp\(base\.r \+ mask \* cmd\.roughness_offset/);
  assert.match(composition, /cmd\.exposed_metal_enabled != 0u/);
  assert.match(composition, /else if \(header\.inspection_mode != 0u\)[\s\S]*result = vec4<f32>\(0\.0\)/,
    "disabled exposed-metal inspection must publish zero contribution while preserving the base Metallic route");
  assert.match(composition, /Decode once[\s\S]*encode exactly once/);
  assert.match(executor, /execute_height_normal_gpu/);
  assert.match(executor, /compose_edge_detail_map/);
  assert.match(executor, /edge_detail_mask_identity=/);
  assert.match(executor, /SOURCE_HEIGHT_RANGE_M: f32 = 0\.002/);
  assert.match(executor, /scalar_display_from_composed_map/);
  assert.match(read("crates/preview/src/gpu_scalar_display.wgsl"), /var allocation_tex: texture_2d<f32>/);
  assert.match(normal, /textureLoad\(final_height_tex/);
  assert.match(normal, /Physical Scharr derivative/);
  assert.match(normal, /meters_per_pixel_x/);
  assert.match(normal, /meters_per_pixel_y/);
  assert.match(normal, /compose_rnm/);
  assert.match(normal, /center_h == center_h && center_h != -1\.0/);
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
  for (const name of ["ed4-combined-edge-mask-64.png", "ed4-edge-height-64.png", "ed4-final-normal-64.png", "ed4-final-base-color-64.png"]) {
    const bytes = readFileSync(new URL(`target/edge-detail-goldens/${name}`, root));
    assert.ok(bytes.length > 100, `${name} must be a bounded non-empty PNG golden`);
  }
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
  assert.match(focusedNative, /authoritative_scale_floor/);
  const app = read("apps/desktop/src/source-first-app.tsx");
  assert.match(app, /requestFeedbackTile\([\s\S]*"authoritative"/);
  const store = read("crates/project-store/src/lib.rs");
  assert.match(store, /mvp_edge_wear_project_save_reopen_migrates_once_to_edge_detail_v1/);
  assert.match(read("apps/desktop/src-tauri/src/document_commands.rs"), /"18": \{ "state": FeedbackExecutionState::NotInstalled/);
});

test("mvp-edge-wear authoring restores the default Workbench plus Hotspot split while Processing stays exclusive", () => {
  const workbench = read("apps/desktop/src/feedback-workbench.tsx");
  const shell = read("apps/desktop/src/source-first-app.tsx");
  const css = read("apps/desktop/src/document-app.css");
  assert.match(shell, /type WorkspaceMode = "authoring" \| "processing"/);
  assert.match(shell, /new Set\(\["workbench", "hotspotSheet"\]\)/);
  assert.match(shell, /sourceWorkbenchOpen = workspaceMode === "authoring" && authoringPanes\.has\("workbench"\)/);
  assert.match(shell, /hotspotSheetOpen = workspaceMode === "authoring" && authoringPanes\.has\("hotspotSheet"\)/);
  assert.match(shell, /if \(workspaceMode === "processing"\) \{ setWorkspaceMode\("authoring"\); return; \}/);
  assert.match(shell, /\(hotspotSheetOpen \|\| processingOpen\) \? <SheetWorkbench/);
  assert.match(shell, /processingOpen \? <ProcessingSidebar/);
  assert.match(shell, /debugOpen \? <div id="debug-drawer"/);
  assert.match(css, /\.debug-drawer \{ position: fixed/);
  assert.match(workbench, /Debug \/ Fixtures · Create bundled sample/);
});

test("mvp-edge-wear Processing switches retained map publications, keeps the prior atlas pending, and has no unclear Edit mode", () => {
  const shell = read("apps/desktop/src/source-first-app.tsx");
  for (const label of ["Material", "Base Color", "Normal", "Height", "Roughness", "Metallic", "AO", "Edge Mask"]) assert.match(shell, new RegExp(`'${label}'|>${label}<`));
  assert.match(shell, /authoringOverlaysVisible = !processing/);
  assert.match(shell, /authoringOverlaysVisible \? <div className=\{`overlays/);
  assert.doesNotMatch(shell, /processingCanvasMode|Atlas interaction mode/);
  assert.match(shell, /if \(processingOpen\) \{\s*setProcessingRequestedMap\(view\);\s*setMapViewState\(view\);\s*mapViewRef\.current = view;\s*return;\s*\}/);
  assert.match(shell, /requestPreview\(undefined, undefined, interactivePreviewProfile, revision, false, true, processingRequestedMap\)/);
  assert.doesNotMatch(shell, /processingViewForMap/);
  assert.match(shell, /previewProgress\?\.feedbackRequestIdentity && dimensions\.generation/);
  assert.match(shell, /Preview superseded/);
  assert.match(shell, /supersessionRetry === 0/);
  assert.match(shell, /window\.setTimeout\(resolve, 180\)/);
  assert.match(shell, /if \(draftId !== previewDraftId\.current\)/);
  assert.match(shell, /if \(failureReason\.code !== "operation_cancelled"\) \{[\s\S]*?if \(draftId !== previewDraftId\.current\) return;[\s\S]*?setProblem\(failureReason\)/);
  assert.match(shell, /pendingAutomaticPreviewKey\.current === latestKey/);
  assert.match(shell, /current_project_projection/);
  assert.match(shell, /projectRef\.current = refreshedProject/);
  assert.match(shell, /const settledRevision = Math\.max\(/);
  assert.match(shell, /return requestPreview\(regionId, projection, profile, settledRevision, scheduleRefinement, true, requestedMapView, 1\)/);
  assert.match(shell, /projectRef\.current = result\.project/);
  assert.match(shell, /Math\.max\(requestedRevision, observedRevision\)/);
  assert.match(shell, /const feedbackCommandInFlight = useRef\(false\)/);
  assert.match(shell, /feedbackCommandInFlight\.current\) return/);
  assert.match(shell, /feedbackCommandInFlight\.current = true/);
  assert.match(shell, /feedbackCommandInFlight\.current = false/);
  assert.match(shell, /onRender=\{\(\) => void renderFullResolutionPreview\(\)\}/);
  assert.match(shell, /\}, \[processingOpen, interactivePreviewProfile\]\)/);
  assert.doesNotMatch(shell, /feedbackCommandPreviewRevision/);
  assert.match(shell, /const radialCommitId = useRef\(0\)/);
  assert.match(shell, /const radialCommitTail = useRef<Promise<void>>\(Promise\.resolve\(\)\)/);
  assert.match(shell, /const stage14InvokeTail = useRef<Promise<void>>\(Promise\.resolve\(\)\)/);
  assert.match(shell, /function invokeStage14Serialized<T>/);
  assert.match(shell, /stage14InvokeTail\.current = queued\.then\(\(\) => undefined, \(\) => undefined\)/);
  assert.doesNotMatch(shell, /invoke<IntermediateAtlasProjection>\("preview_(?:through_stage_14|stage_15_16_feedback)"/);
  assert.match(shell, /const queued = radialCommitTail\.current\.then\(async \(\) =>/);
  assert.match(shell, /radialCommitTail\.current = queued\.catch\(\(\) => undefined\)/);
  assert.match(shell, /if \(commitId !== radialCommitId\.current\) return/);
  assert.match(shell, /await requestPreview\(undefined, undefined, interactivePreviewProfile, revision, false, true\)/);
});

test("mvp-edge-wear Processing makes Edge Detail application explicit and truthful", () => {
  const workbench = read("apps/desktop/src/feedback-workbench.tsx");
  assert.match(workbench, /persisted = props\.project\?\.document\?\.edgeDetail/);
  assert.match(workbench, /persisted \?\? \{ \.\.\.defaultEdgeWearIntent\(\), enabled: false \}/);
  assert.match(workbench, /targetRegion = committed\.targetRegion/);
  assert.match(workbench, /targetLabel = targetRegion \? `\$\{targetRegion\.displayName\} · \$\{targetRegion\.id\}` : "All regions"/);
  assert.match(workbench, /Selected region ·/);
  assert.match(workbench, /onBlur=\{finish\}/);
  assert.match(workbench, /onPointerUp=\{finish\}/);
  assert.match(workbench, /Apply Edge Detail/);
  assert.match(workbench, /Apply Changes/);
  assert.match(workbench, /Render Edge Detail/);
  assert.match(workbench, /if \(applied && !dirty\)/);
  assert.match(workbench, /if \(!artifactCurrent\) props\.onRender\(\)/);
  assert.match(workbench, /Edge Detail is not applied/);
  assert.match(workbench, /Edge Detail has unapplied changes/);
  assert.match(workbench, /sanitizeEdgeDetailIntent\(\{ \.\.\.draft, enabled: true \}\)/);
  assert.doesNotMatch(workbench, /Changes publish live/);
  assert.match(workbench, /edgeDetailIntentFromPreset\(event\.currentTarget\.value as EdgeDetailPresetName, \{ \.\.\.draft, enabled: true \}\)/);
  assert.doesNotMatch(workbench, /<strong>Structural Profile<\/strong>/);
  assert.doesNotMatch(workbench, /What feeds these maps\?/);
  for (const control of ["Wear Amount", "Intensity", "Edge Width", "Bevel Radius", "Breakup", "Height"]) assert.match(workbench, new RegExp(`label: "${control}"`));
  assert.match(workbench, /props\.value \* 1000/);
  for (const control of ["Edge Softness", "Microdetail Amount", "Source Height", "Base Color bump", "Normal detail"]) assert.match(workbench, new RegExp(control));
});

test("mvp-edge-wear ED-6 Outputs require current publication evidence", () => {
  const workbench = read("apps/desktop/src/feedback-workbench.tsx");
  assert.match(workbench, /artifactCurrent = revision !== undefined && props\.artifact\?\.documentRevision === revision/);
  for (const map of ["Base Color", "Edge Mask", "Height", "Normal", "Roughness", "Metallic", "AO"]) assert.match(workbench, new RegExp(`\\["${map}"`));
  assert.match(workbench, /ready \? "Ready" : "Not ready"/);
});

test("mvp-edge-wear publishes a complete material set, lights it without geometry, and exports every Edge Detail output", () => {
  const shell = read("apps/desktop/src/source-first-app.tsx");
  const css = read("apps/desktop/src/document-app.css");
  const materialPreview = read("apps/desktop/src/material-preview.tsx");
  const native = read("apps/desktop/src-tauri/src/document_commands.rs");
  for (const map of ["base_color", "normal", "height", "roughness", "metallic", "ambient_occlusion", "edge_mask"]) {
    assert.match(shell, new RegExp(`processingPreviewMaterialMaps[\\s\\S]*?"${map}"`));
  }
  assert.match(shell, /const requestedMaps = processingOpen\s*\? processingPreviewMaterialMaps/);
  assert.match(shell, /<MaterialPreviewCanvas artifact=\{artifact\}/);
  assert.doesNotMatch(shell, /role="tab" disabled[^>]*>Material/);
  assert.match(materialPreview, /uniform sampler2D u_base_color/);
  assert.match(materialPreview, /uniform sampler2D u_normal/);
  assert.match(materialPreview, /uniform sampler2D u_height/);
  assert.match(materialPreview, /height_toward_light/);
  assert.match(materialPreview, /onPointerMove/);
  assert.match(shell, />Export All Maps</);
  assert.match(shell, />Refresh All Maps</);
  assert.match(css, /\.processing-inspection-controls button \{[^}]*min-width: max-content;[^}]*max-width: none;[^}]*white-space: nowrap;/);
  assert.match(shell, /"metallic", "edge_mask"/);
  assert.match(native, /MaterialMapKind::AmbientOcclusion\s*\| MaterialMapKind::EdgeMask => \{/);
  assert.match(native, /"maps\/edge_mask\.png"/);
});
