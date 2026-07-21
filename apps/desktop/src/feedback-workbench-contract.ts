import type {
  CompiledMapView,
  EdgeWearIntent,
  FeedbackComparisonMode,
  FeedbackContributionView,
  FeedbackDetailIntent,
  FeedbackPreviewProfile,
  FeedbackProfileIntent,
  FeedbackPreviewExecution,
  FeedbackWorkbenchCommand,
  StampOperationIntent,
  SourceFramePreviewMaterialMap,
} from "@hot-trimmer/ipc-contracts";

const finiteOr = (value: number, fallback: number) => Number.isFinite(value) ? value : fallback;
const clamp = (value: number, minimum: number, maximum: number) => Math.min(maximum, Math.max(minimum, value));

export function sanitizeEdgeWearIntent(intent: EdgeWearIntent): EdgeWearIntent {
  const defaults = {
    coverage: 0.55, strength: 0.8, edgeWidthM: 0.004, breakupScaleM: 0.012,
    breakupSeed: 201516, heightAmplitudeM: -0.00035, hueShiftDegrees: 0,
    saturationMultiplier: 0.55, valueOffset: 0.12, roughnessOffset: 0.18,
    metallicOffset: 0,
  };
  const exposedMetalEnabled = !!intent.exposedMetalEnabled;
  return {
    ...intent,
    coverage: clamp(finiteOr(intent.coverage, defaults.coverage), 0, 1),
    strength: clamp(finiteOr(intent.strength, defaults.strength), 0, 1),
    edgeWidthM: Math.max(0.00001, finiteOr(intent.edgeWidthM, defaults.edgeWidthM)),
    breakupScaleM: Math.max(0.00001, finiteOr(intent.breakupScaleM, defaults.breakupScaleM)),
    breakupSeed: Math.max(0, Math.trunc(finiteOr(intent.breakupSeed, defaults.breakupSeed))),
    heightAmplitudeM: finiteOr(intent.heightAmplitudeM, defaults.heightAmplitudeM),
    hueShiftDegrees: finiteOr(intent.hueShiftDegrees, defaults.hueShiftDegrees),
    saturationMultiplier: Math.max(0, finiteOr(intent.saturationMultiplier, defaults.saturationMultiplier)),
    valueOffset: finiteOr(intent.valueOffset, defaults.valueOffset),
    roughnessOffset: finiteOr(intent.roughnessOffset, defaults.roughnessOffset),
    exposedMetalEnabled,
    metallicOffset: exposedMetalEnabled
      ? clamp(finiteOr(intent.metallicOffset, defaults.metallicOffset), 0, 1)
      : 0,
  };
}

export const FEEDBACK_WORKBENCH_VERSION = "20A.1" as const;
export const FEEDBACK_COMMAND_VERSION = 1 as const;
export const STAGE_15_20_DEBUG_SCHEMA_VERSION = 1 as const;

export const contributionDependencies: Readonly<Record<FeedbackContributionView, CompiledMapView | null>> = {
  stage15Occupancy: "ambientOcclusion",
  stage15Height: "height",
  stage15ProfileRoute: null,
  stage15Lod: null,
  stage15Fallback: null,
  stage16RegisteredMask: "edgeMask",
  stage16Height: "height",
  stage16VectorNormal: "normal",
  stage16ScalarRoughness: "roughness",
  stage16ScalarMetallic: "metallic",
  stage16ScalarAmbientOcclusion: "ambientOcclusion",
  stage16BaseColor: "baseColor",
  stage16MaterialId: "materialId",
  stage16MaterialIdValidity: "materialId",
  stage16Route: null,
  stage16Occupancy: null,
  stage16Lod: null,
  stage16Scope: null,
  stage16AssetResolution: null,
};

export function visibleMapDependency(view: FeedbackContributionView): CompiledMapView | null {
  return contributionDependencies[view];
}

export function sourceFrameMaterialMapForCompiledView(view: CompiledMapView): SourceFramePreviewMaterialMap {
  switch (view) {
    case "baseColor": return "base_color";
    case "normal": return "normal";
    case "height": return "height";
    case "roughness": return "roughness";
    case "metallic": return "metallic";
    case "ambientOcclusion": return "ambient_occlusion";
    case "regionId": return "region_id";
    case "materialId": return "material_id";
    case "edgeMask": return "edge_mask";
  }
}

export function defaultFeedbackProfile(program: FeedbackProfileIntent["program"] = "convex_bevel"): FeedbackProfileIntent {
  const radial = program === "radial_disc" || program === "annulus";
  return {
    program,
    firstWidth: { unit: "meters", value: radial ? 0.006 : 0.004 },
    secondWidth: { unit: "meters", value: radial ? 0.006 : 0.004 },
    minimumFlatCenter: { unit: "meters", value: 0.001 },
    amplitude: { unit: "meters", value: 0.002 },
    angleDegrees: 45,
    innerRadius: { unit: "meters", value: program === "annulus" ? 0.018 : 0 },
    outerRadius: { unit: "meters", value: radial ? 0.04 : 0 },
    legalityPolicy: "clamp",
    lodPolicy: "auto",
    maximumSupersampling: 8,
    seed: 201520,
    customCurve: [],
  };
}

export function legalProfilePrograms(role: string): readonly FeedbackProfileIntent["program"][] {
  return role === "radial"
    ? ["flat", "radial_disc", "annulus"]
    : ["flat", "convex_bevel", "rounded_bevel", "concave_groove", "panel_frame"];
}

export const unavailableStages = Object.freeze({ 18: "NotInstalled", 17: "NotInstalled", 19: "NotInstalled", 20: "NotInstalled" } as const);

export const occupancyRelations = [
  "above_profile",
  "below_profile",
  "avoid_raised",
  "only_flat_center",
  "ignore",
] as const satisfies readonly StampOperationIntent["occupancy"][];

export function occupancyRelationFromValue(value: string): StampOperationIntent["occupancy"] | null {
  switch (value) {
    case "above_profile":
    case "below_profile":
    case "avoid_raised":
    case "only_flat_center":
    case "ignore":
      return value;
    default:
      return null;
  }
}

export interface FeedbackPixelRequestIdentity {
  revision: number;
  regionId: string;
  allRegions: boolean;
  view: FeedbackContributionView;
  map: CompiledMapView;
  profile: FeedbackPreviewProfile;
  comparisonMode: FeedbackComparisonMode;
  selectedOperationId: string | null;
}

export function feedbackPixelRequestIdentity(request: FeedbackPixelRequestIdentity): string {
  return JSON.stringify([
    "stage15-16-feedback-v1",
    request.revision,
    request.regionId,
    request.allRegions,
    request.view,
    request.map,
    request.profile,
    request.comparisonMode,
    request.selectedOperationId,
  ]);
}

export function feedbackExecutionMatchesRequest(
  execution: FeedbackPreviewExecution | null | undefined,
  request: FeedbackPixelRequestIdentity | null,
): boolean {
  if (!execution || !request) return false;
  const nativeProfile = request.profile === "preview1024" ? "refinement1024" : request.profile;
  return execution.revision === request.revision
    && execution.regionId === request.regionId
    && execution.allRegions === request.allRegions
    && execution.view === request.view
    && execution.requestedMap === request.map
    && execution.profile === nativeProfile
    && execution.comparisonMode === request.comparisonMode
    && (execution.selectedOperationId ?? null) === request.selectedOperationId;
}

export function feedbackEvidenceForRequest<T extends { manifest: { generation: number } }>(
  request: FeedbackPixelRequestIdentity | null,
  execution: FeedbackPreviewExecution | null | undefined,
  tile: T | undefined,
): { pixelDispatch: 0 | 1; tile: T | undefined; exactRequestEvidence: boolean } {
  const exactRequestEvidence = feedbackExecutionMatchesRequest(execution, request);
  const exactTile = exactRequestEvidence && tile?.manifest.generation === execution?.publishedGeneration
    ? tile
    : undefined;
  return {
    pixelDispatch: exactRequestEvidence ? 1 : 0,
    tile: exactTile,
    exactRequestEvidence,
  };
}

export function selectFeedbackRegionWithoutPixelWork<T>(
  current: { selectedRegionId: string | null; activeMap: CompiledMapView; publication: T },
  selectedRegionId: string,
): { selectedRegionId: string; activeMap: CompiledMapView; publication: T; previewInvocations: 0 } {
  return { ...current, selectedRegionId, previewInvocations: 0 };
}

export function feedbackPreviewRegionAfterCommand(
  command: FeedbackWorkbenchCommand,
  currentRegionId: string | null,
  availableRegionIds: readonly string[],
): string | null {
  if (currentRegionId && availableRegionIds.includes(currentRegionId)) return currentRegionId;
  if (command.type === "set_edge_wear"
    && command.intent.targetRegion
    && availableRegionIds.includes(command.intent.targetRegion)) {
    return command.intent.targetRegion;
  }
  return availableRegionIds[0] ?? null;
}

export function feedbackViewAfterCommand(
  command: FeedbackWorkbenchCommand,
  currentView: FeedbackContributionView,
): FeedbackContributionView {
  return command.type === "set_edge_wear" ? "stage16BaseColor" : currentView;
}

export function updateFeedbackOperationIntent(
  intent: Extract<FeedbackDetailIntent, { kind: "operation" | "stroke" }>,
  patch: Partial<StampOperationIntent>,
): FeedbackDetailIntent {
  if (intent.kind === "stroke") {
    return { ...intent, value: { ...intent.value, operation: { ...intent.value.operation, ...patch } } };
  }
  return { ...intent, value: { ...intent.value, ...patch } };
}

export function selectedOperationAfterCommand(
  command: FeedbackWorkbenchCommand,
  current: string | null,
  committedIdentity: string,
): string | null {
  switch (command.type) {
    case "set_profile":
    case "set_edge_wear":
    case "reorder_details":
      return current;
    case "delete_detail":
      return current === command.operationId ? null : current;
    case "upsert_detail":
    case "duplicate_detail":
    case "set_detail_enabled":
      return committedIdentity;
  }
}
