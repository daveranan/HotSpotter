import type {
  CompiledMapView,
  EdgeDetailIntentV1,
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

export function sanitizeEdgeDetailIntent(intent: EdgeDetailIntentV1): EdgeDetailIntentV1 {
  const defaults: EdgeDetailIntentV1 = {
    schemaVersion: 1, enabled: true, wearAmount: 0.55, intensity: 0.8,
    edgeWidthM: 0.004, bevelRadiusM: 0.0025, edgeSoftness: 0.3,
    breakupAmount: 0.7, breakupScaleM: 0.012, microDetailAmount: 0.25,
    microDetailScaleM: 0.002, seed: 201516, sourceHeightInfluence: 0.65,
    sourceLuminanceInfluence: 0.2, heightAmplitudeM: -0.00035,
    normalDetailStrength: 1, hueShiftDegrees: 0, saturationMultiplier: 0.55,
    valueMultiplier: 1.12, roughnessOffset: 0.18, exposedMetalEnabled: false,
    metallicOffset: 0,
  };
  const exposedMetalEnabled = !!intent.exposedMetalEnabled;
  return {
    ...intent,
    schemaVersion: 1,
    wearAmount: clamp(finiteOr(intent.wearAmount, defaults.wearAmount), 0, 1),
    intensity: clamp(finiteOr(intent.intensity, defaults.intensity), 0, 1),
    edgeWidthM: Math.max(0.00001, finiteOr(intent.edgeWidthM, defaults.edgeWidthM)),
    bevelRadiusM: Math.max(0, finiteOr(intent.bevelRadiusM, defaults.bevelRadiusM)),
    edgeSoftness: clamp(finiteOr(intent.edgeSoftness, defaults.edgeSoftness), 0, 1),
    breakupAmount: clamp(finiteOr(intent.breakupAmount, defaults.breakupAmount), 0, 1),
    breakupScaleM: Math.max(0.00001, finiteOr(intent.breakupScaleM, defaults.breakupScaleM)),
    microDetailAmount: clamp(finiteOr(intent.microDetailAmount, defaults.microDetailAmount), 0, 1),
    microDetailScaleM: Math.max(0.00001, finiteOr(intent.microDetailScaleM, defaults.microDetailScaleM)),
    seed: Math.min(0xffff_ffff, Math.max(0, Math.trunc(finiteOr(intent.seed, defaults.seed)))),
    sourceHeightInfluence: clamp(finiteOr(intent.sourceHeightInfluence, defaults.sourceHeightInfluence), 0, 1),
    sourceLuminanceInfluence: clamp(finiteOr(intent.sourceLuminanceInfluence, defaults.sourceLuminanceInfluence), 0, 1),
    heightAmplitudeM: finiteOr(intent.heightAmplitudeM, defaults.heightAmplitudeM),
    normalDetailStrength: clamp(finiteOr(intent.normalDetailStrength, defaults.normalDetailStrength), 0, 2),
    hueShiftDegrees: clamp(finiteOr(intent.hueShiftDegrees, defaults.hueShiftDegrees), -180, 180),
    saturationMultiplier: clamp(finiteOr(intent.saturationMultiplier, defaults.saturationMultiplier), 0, 2),
    valueMultiplier: clamp(finiteOr(intent.valueMultiplier, defaults.valueMultiplier), 0, 3),
    roughnessOffset: clamp(finiteOr(intent.roughnessOffset, defaults.roughnessOffset), -1, 1),
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
  profile: FeedbackPreviewProfile | "authoritative";
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
  if (command.type === "set_edge_detail"
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
  // ED-2 publishes the combined mask and signed Height; ED-3 owns final composition.
  return command.type === "set_edge_detail" ? "stage16RegisteredMask" : currentView;
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
    case "set_edge_detail":
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
