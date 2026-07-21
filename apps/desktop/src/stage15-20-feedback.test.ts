import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import {
  FEEDBACK_COMMAND_VERSION,
  STAGE_15_20_DEBUG_SCHEMA_VERSION,
  contributionDependencies,
  defaultFeedbackProfile,
  feedbackEvidenceForRequest,
  feedbackExecutionMatchesRequest,
  feedbackPixelRequestIdentity,
  legalProfilePrograms,
  occupancyRelationFromValue,
  occupancyRelations,
  selectFeedbackRegionWithoutPixelWork,
  selectedOperationAfterCommand,
  sourceFrameMaterialMapForCompiledView,
  unavailableStages,
  updateFeedbackOperationIntent,
  visibleMapDependency,
} from "./feedback-workbench-contract.ts";

test("feedback workbench exposes only legal Prompt 20A profile programs", () => {
  assert.deepEqual(legalProfilePrograms("radial"), ["flat", "radial_disc", "annulus"]);
  assert.deepEqual(legalProfilePrograms("planar"), ["flat", "convex_bevel", "rounded_bevel", "concave_groove", "panel_frame"]);
  const bevel = defaultFeedbackProfile("convex_bevel");
  assert.equal(bevel.firstWidth.unit, "meters");
  assert.equal(bevel.amplitude.unit, "meters");
  assert.equal(bevel.seed, 201520);
});

test("each pixel view requests one real map dependency and metadata QA dispatches none", () => {
  assert.equal(visibleMapDependency("stage16VectorNormal"), "normal");
  assert.equal(visibleMapDependency("stage15Occupancy"), "ambientOcclusion");
  assert.equal(visibleMapDependency("stage16RegisteredMask"), "edgeMask");
  assert.equal(visibleMapDependency("stage16MaterialId"), "materialId");
  assert.equal(visibleMapDependency("stage16MaterialIdValidity"), "materialId");
  assert.equal(visibleMapDependency("stage15ProfileRoute"), null);
  assert.equal(visibleMapDependency("stage16AssetResolution"), null);
  assert.equal(Object.keys(contributionDependencies).length, 19);
  assert.equal(sourceFrameMaterialMapForCompiledView("edgeMask"), "edge_mask");
});

test("region selection changes overlays only and preserves the active map and publication", () => {
  const publication = { manifest: { generation: 42 }, opaque: "published-tile" };
  const next = selectFeedbackRegionWithoutPixelWork({ selectedRegionId: "region-a", activeMap: "normal", publication }, "region-b");
  assert.equal(next.selectedRegionId, "region-b");
  assert.equal(next.activeMap, "normal");
  assert.equal(next.publication, publication);
  assert.equal(next.previewInvocations, 0);
});

test("comparison and isolation fields participate in exact pixel request identity", () => {
  const base = { revision: 9, regionId: "region-a", view: "stage16RegisteredMask", map: "edgeMask", profile: "preview2048", selectedOperationId: null } as const;
  const before = feedbackPixelRequestIdentity({ ...base, comparisonMode: "before" });
  const after = feedbackPixelRequestIdentity({ ...base, comparisonMode: "after" });
  const isolatedA = feedbackPixelRequestIdentity({ ...base, comparisonMode: "selectedOperationIsolation", selectedOperationId: "operation-a" });
  const isolatedB = feedbackPixelRequestIdentity({ ...base, comparisonMode: "selectedOperationIsolation", selectedOperationId: "operation-b" });
  assert.notEqual(before, after);
  assert.notEqual(isolatedA, isolatedB);
  assert.notEqual(after, feedbackPixelRequestIdentity({ ...base, revision: 10, comparisonMode: "after" }));
  assert.notEqual(after, feedbackPixelRequestIdentity({ ...base, regionId: "region-b", comparisonMode: "after" }));
  assert.notEqual(after, feedbackPixelRequestIdentity({ ...base, view: "stage16Height", map: "height", comparisonMode: "after" }));
  assert.notEqual(after, feedbackPixelRequestIdentity({ ...base, profile: "preview4096", comparisonMode: "after" }));
  assert.equal(isolatedA, feedbackPixelRequestIdentity({ ...base, comparisonMode: "selectedOperationIsolation", selectedOperationId: "operation-a" }));
});

test("occupancy controls expose exactly the Rust vocabulary and reject unknown UI values", () => {
  assert.deepEqual(occupancyRelations, ["above_profile", "below_profile", "avoid_raised", "only_flat_center", "ignore"]);
  for (const occupancy of occupancyRelations) assert.equal(occupancyRelationFromValue(occupancy), occupancy);
  assert.equal(occupancyRelationFromValue("any"), null);
  assert.equal(occupancyRelationFromValue("unknown"), null);
});

test("editing a StampStroke operation preserves physicalSamplesM byte-for-byte", () => {
  const physicalSamplesM = [[0.02, 0.02], [0.04, 0.03], [0.06, 0.02]] as const;
  const operation = {
    asset: { assetId: "asset", version: "4", digest: "d".repeat(64), kind: "registered_stamp_channels" },
    scope: "material_reusable_atlas", targetRegion: "region-a", physicalPositionM: [0.02, 0.02], physicalSizeM: [0.03, 0.03],
    pivot: [0.5, 0.5], rotationDegrees: 0, mirror: [false, false], opacity: 1, blend: "add", clipping: "contain", seed: 201520,
    spacingM: [0.015, 0.015], scatter: 0, jitterM: [0, 0], layerOrder: 1, occupancy: "only_flat_center", channels: [],
  } as const;
  const edited = updateFeedbackOperationIntent({ kind: "stroke", value: { operation, physicalSamplesM } }, { rotationDegrees: 35 });
  assert.equal(edited.kind, "stroke");
  if (edited.kind !== "stroke") throw new Error("expected stroke");
  assert.deepEqual(edited.value.physicalSamplesM, physicalSamplesM);
  assert.equal(edited.value.operation.rotationDegrees, 35);
});

test("metadata and stale requests cannot attach old pixel or publication evidence", () => {
  const request = { revision: 9, regionId: "region-a", view: "stage16RegisteredMask", map: "edgeMask", profile: "preview2048", comparisonMode: "after", selectedOperationId: null } as const;
  const execution = {
    requestIdentity: "native-id", clientGeneration: 7, publishedGeneration: 11, revision: 9, regionId: "region-a",
    view: "stage16RegisteredMask", requestedMap: "edgeMask", profile: "preview2048", comparisonMode: "after", outcome: "Executed", cacheReused: false,
  } as const;
  const tile = { manifest: { generation: 11 }, payload: "gpu" };
  const exact = feedbackEvidenceForRequest(request, execution, tile);
  assert.equal(feedbackExecutionMatchesRequest(execution, request), true);
  assert.equal(exact.tile, tile);
  assert.equal(exact.pixelDispatch, 1);
  assert.equal(execution.outcome, "Executed");
  const metadata = feedbackEvidenceForRequest(null, execution, tile);
  assert.deepEqual(metadata, { pixelDispatch: 0, tile: undefined, exactRequestEvidence: false });
  const changedRegion = feedbackEvidenceForRequest({ ...request, regionId: "region-b" }, execution, tile);
  assert.equal(changedRegion.tile, undefined);
  assert.equal(changedRegion.exactRequestEvidence, false);
});

test("reorder retains the selected UUID and deleting it clears selection", () => {
  assert.equal(selectedOperationAfterCommand({ type: "reorder_details", operationIds: ["operation-b", "operation-a"] }, "operation-a", "order-hash"), "operation-a");
  assert.equal(selectedOperationAfterCommand({ type: "delete_detail", operationId: "operation-a" }, "operation-a", "operation-a"), null);
  assert.equal(selectedOperationAfterCommand({ type: "delete_detail", operationId: "operation-b" }, "operation-a", "operation-b"), "operation-a");
});

test("unfinished stages are explicit and the schema is extendable in place", () => {
  assert.equal(FEEDBACK_COMMAND_VERSION, 1);
  assert.equal(STAGE_15_20_DEBUG_SCHEMA_VERSION, 1);
  assert.deepEqual(unavailableStages, { 18: "NotInstalled", 17: "NotInstalled", 19: "NotInstalled", 20: "NotInstalled" });
});

test("source-first shell extends the existing F2 copy action without adding material math", () => {
  const app = readFileSync(new URL("./source-first-app.tsx", import.meta.url), "utf8");
  const workbench = readFileSync(new URL("./feedback-workbench.tsx", import.meta.url), "utf8");
  assert.match(app, /Copy Stage 15-20 telemetry \+ debug/);
  assert.match(app, /event\.key !== "F2"/);
  assert.match(app, /preview_stage_15_16_feedback/);
  assert.match(workbench, /not a finished PBR material/);
  assert.match(workbench, /Screen coordinates are transient/);
  assert.doesNotMatch(workbench, /canvas|getImageData|putImageData|base64/i);
});
