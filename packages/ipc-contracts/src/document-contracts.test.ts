import assert from "node:assert/strict";
import test from "node:test";

import { IPC_PROTOCOL_VERSION, type HierarchicalLayoutRecipe, type PartitionRecipe, type TrimSheetDocumentCommand } from "./document-contracts.ts";

test("document commands keep the typed protocol boundary", () => {
  const command: TrimSheetDocumentCommand = {
    type: "set_output_resolution",
    outputSize: { width: 2048, height: 2048 },
  };
  assert.equal(IPC_PROTOCOL_VERSION, 2);
  assert.equal(command.type, "set_output_resolution");
});

test("source-frame resize carries the selected identity separately from draw", () => {
  const resize: TrimSheetDocumentCommand = {
    type: "resize_source_frame_region",
    regionId: "8cc27f1e-5caf-45a5-bdd4-b3a0bcff0433",
    gridRect: { x: 4, y: 6, width: 20, height: 12 },
  };
  const draw: TrimSheetDocumentCommand = { type: "draw_source_frame_region", gridRect: resize.gridRect };
  assert.equal(resize.regionId, "8cc27f1e-5caf-45a5-bdd4-b3a0bcff0433");
  assert.notEqual(resize.type, draw.type);
});

test("hierarchical layout recipes keep a versioned camel-case wire shape while legacy recipes remain optional", () => {
  const hierarchy: HierarchicalLayoutRecipe = {
    schemaVersion: 1, macroStyle: "mixed_hierarchy", recursivePolicy: "cascade",
    targetRegionMin: 30, targetRegionMax: 40,
    largeShareMilli: 580, mediumShareMilli: 200, smallShareMilli: 80, stripShareMilli: 110, radialShareMilli: 30,
    macroParentCount: 4, protectedParentCount: 2, subdividableParentCount: 2,
    hierarchyDepth: 3, scaleFalloffMilli: 500,
    allowedSplitRatios: ["half", "one_third", "two_third"], alignmentStrengthMilli: 900, variationMilli: 80,
    horizontalStripWeightMilli: 550, verticalStripWeightMilli: 450, stripThicknessLadder: [1, 1, 2, 2, 3, 4],
    radialCount: 2, radialMinDiameter: 6, radialMaxDiameter: 10,
    majorAspects: ["square", "wide2", "tall2"], mediumAspects: ["square", "wide2", "tall2", "wide4", "tall4"],
    detailAspects: ["square", "wide2", "tall2", "wide4", "tall4"],
  };
  const wire = JSON.parse(JSON.stringify(hierarchy)) as HierarchicalLayoutRecipe;
  assert.equal(wire.macroStyle, "mixed_hierarchy");
  assert.deepEqual(wire.allowedSplitRatios, ["half", "one_third", "two_third"]);
  assert.equal(wire.largeShareMilli + wire.mediumShareMilli + wire.smallShareMilli + wire.stripShareMilli + wire.radialShareMilli, 1_000);
  const legacy: Pick<PartitionRecipe, "hierarchical"> = {};
  assert.equal(legacy.hierarchical, undefined);
});
