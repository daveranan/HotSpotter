import type { CompositionProfile, HierarchicalLayoutRecipe, PartitionRecipe } from "@hot-trimmer/ipc-contracts";

export type LayoutTemplateId = "mixed-hierarchy" | "panel-cascade" | "horizontal-trim-sheet" | "facade-halving" | "classic-source-hotspot" | "mechanical-radial";

export const layoutTemplateOptions: readonly { id: LayoutTemplateId; label: string }[] = [
  { id: "mixed-hierarchy", label: "Mixed Hierarchy" },
  { id: "panel-cascade", label: "Panel Cascade" },
  { id: "horizontal-trim-sheet", label: "Horizontal Trim Sheet" },
  { id: "facade-halving", label: "Facade Halving" },
  { id: "classic-source-hotspot", label: "Classic Source Hotspot" },
  { id: "mechanical-radial", label: "Mechanical / Radial" },
] as const;

function legacyComposition(): CompositionProfile {
  return {
    profileId: "hierarchical-mixed-hierarchy", version: 2,
    broadPanels: { count: 4, areaShareMilli: 580, minimumWidth: 8, minimumHeight: 8, maximumWidth: 64, maximumHeight: 64, minimumAspectMilli: 250, maximumAspectMilli: 4_000, subdivisionBudget: 0 },
    mediumBlocks: { count: 2, areaShareMilli: 200, minimumWidth: 4, minimumHeight: 4, maximumWidth: 32, maximumHeight: 32, minimumAspectMilli: 200, maximumAspectMilli: 5_000, subdivisionBudget: 0 },
    horizontalStrips: { count: 6, minimumThickness: 1, maximumThickness: 4 }, verticalStrips: { count: 5, minimumThickness: 1, maximumThickness: 4 },
    smallDetails: { count: 4, areaShareMilli: 80, minimumWidth: 1, minimumHeight: 1, maximumWidth: 12, maximumHeight: 12, minimumAspectMilli: 125, maximumAspectMilli: 8_000, subdivisionBudget: 0 },
    microStrips: { count: 2, minimumThickness: 1, maximumThickness: 1 }, radialReservations: { count: 2, allocationMinDiameter: 6, allocationMaxDiameter: 10 },
  };
}

function mixedHierarchy(): HierarchicalLayoutRecipe {
  return {
    schemaVersion: 1, macroStyle: "mixed_hierarchy", recursivePolicy: "cascade", targetRegionMin: 30, targetRegionMax: 40,
    largeShareMilli: 580, mediumShareMilli: 200, smallShareMilli: 80, stripShareMilli: 110, radialShareMilli: 30,
    macroParentCount: 4, protectedParentCount: 2, subdividableParentCount: 2, hierarchyDepth: 3, scaleFalloffMilli: 500,
    allowedSplitRatios: ["half", "one_third", "two_third"], alignmentStrengthMilli: 900, variationMilli: 80,
    horizontalStripWeightMilli: 550, verticalStripWeightMilli: 450, stripThicknessLadder: [1, 1, 2, 2, 3, 4],
    radialCount: 2, radialMinDiameter: 6, radialMaxDiameter: 10,
    majorAspects: ["square", "wide2", "tall2"], mediumAspects: ["square", "wide2", "tall2", "wide4", "tall4"],
    detailAspects: ["square", "wide2", "tall2", "wide4", "tall4"],
  };
}

function baseRecipe(): PartitionRecipe {
  const hierarchical = mixedHierarchy();
  return { schemaVersion: 3, recipeId: "source-frame-hierarchical-hotspot", recipeVersion: 3,
    grid: { schemaVersion: 1, width: 64, height: 64 }, targetRegionCount: hierarchical.targetRegionMax, seed: 0,
    horizontalSplitBiasMilli: 550, verticalSplitBiasMilli: 450, varianceMilli: hierarchical.variationMilli,
    minimumLogicalWidth: 1, minimumLogicalHeight: 1, minimumAspectMilli: 125, maximumAspectMilli: 8_000,
    workLimit: 4096, depthLimit: 32, composition: legacyComposition(), hierarchical };
}

function withShares(value: HierarchicalLayoutRecipe, large: number, medium: number, small: number, strips: number, radial: number) {
  return { ...value, largeShareMilli: large, mediumShareMilli: medium, smallShareMilli: small, stripShareMilli: strips, radialShareMilli: radial };
}

export function layoutTemplateRecipe(_current: PartitionRecipe, id: LayoutTemplateId): PartitionRecipe {
  const base = baseRecipe();
  let hierarchical = mixedHierarchy();
  switch (id) {
    case "mixed-hierarchy": break;
    case "panel-cascade": hierarchical = { ...withShares(hierarchical, 640, 180, 60, 120, 0), macroStyle: "panel_cascade", recursivePolicy: "cascade", targetRegionMin: 28, targetRegionMax: 38, hierarchyDepth: 3, protectedParentCount: 2, subdividableParentCount: 2, radialCount: 0, variationMilli: 80 }; break;
    case "horizontal-trim-sheet": hierarchical = { ...withShares(hierarchical, 480, 160, 60, 300, 0), macroStyle: "horizontal_trims", recursivePolicy: "balanced", targetRegionMin: 34, targetRegionMax: 48, horizontalStripWeightMilli: 800, verticalStripWeightMilli: 200, stripThicknessLadder: [1, 1, 1, 2, 2, 3, 4, 6], radialCount: 0, variationMilli: 60 }; break;
    case "facade-halving": hierarchical = { ...withShares(hierarchical, 720, 200, 40, 40, 0), macroStyle: "facade_halving", recursivePolicy: "balanced", targetRegionMin: 12, targetRegionMax: 22, hierarchyDepth: 2, allowedSplitRatios: ["half"], alignmentStrengthMilli: 1_000, variationMilli: 0, horizontalStripWeightMilli: 1_000, verticalStripWeightMilli: 0, radialCount: 0 }; break;
    case "classic-source-hotspot": hierarchical = { ...withShares(hierarchical, 540, 180, 60, 220, 0), macroStyle: "classic_source_hotspot", recursivePolicy: "cascade", targetRegionMin: 30, targetRegionMax: 46, horizontalStripWeightMilli: 545, verticalStripWeightMilli: 455, radialCount: 0, variationMilli: 40 }; break;
    case "mechanical-radial": hierarchical = { ...withShares(hierarchical, 480, 180, 100, 140, 100), macroStyle: "mechanical_radial", recursivePolicy: "balanced", targetRegionMin: 30, targetRegionMax: 44, radialCount: 4, radialMinDiameter: 6, radialMaxDiameter: 12, variationMilli: 60 }; break;
  }
  return { ...base, targetRegionCount: hierarchical.targetRegionMax, horizontalSplitBiasMilli: hierarchical.horizontalStripWeightMilli,
    verticalSplitBiasMilli: hierarchical.verticalStripWeightMilli, varianceMilli: hierarchical.variationMilli,
    composition: { ...base.composition, profileId: `hierarchical-${id}`,
      broadPanels: { ...base.composition.broadPanels, count: hierarchical.macroParentCount, areaShareMilli: hierarchical.largeShareMilli },
      mediumBlocks: { ...base.composition.mediumBlocks, count: hierarchical.subdividableParentCount, areaShareMilli: hierarchical.mediumShareMilli },
      smallDetails: { ...base.composition.smallDetails, areaShareMilli: hierarchical.smallShareMilli },
      radialReservations: { count: hierarchical.radialCount, allocationMinDiameter: hierarchical.radialMinDiameter, allocationMaxDiameter: hierarchical.radialMaxDiameter } }, hierarchical };
}

export function defaultPartitionRecipe(): PartitionRecipe { return layoutTemplateRecipe(baseRecipe(), "mixed-hierarchy"); }
export function selectedLayoutTemplate(recipe: PartitionRecipe): LayoutTemplateId {
  const id = recipe.composition.profileId.replace(/^hierarchical-/, "") as LayoutTemplateId;
  return layoutTemplateOptions.some((option) => option.id === id) ? id : "mixed-hierarchy";
}
