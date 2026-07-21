import React, { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  IPC_PROTOCOL_VERSION,
  type CommandFailure,
  type AuthoredLayoutPreset,
  type DelightingIntent,
  type MaterialBehaviorClass,
  type MaterialClassificationCommand,
  type MaterialCalibrationCommand,
  type CompiledMapView,
  type ContentReference,
  type IntermediateAtlasProjection,
  type NativeStage14ExportProgress,
  type NativeStage14ExportProgressEvent,
  type NativeStage14ExportProjection,
  type NormalizedBounds,
  type NormalConvention,
  type Patch,
  type PatchCommand,
  type PatchGeometry,
  type PixelBounds,
  type ProjectProjection,
  type PreviewSheetProjection,
  type RecentProject,
  type RegionMapping,
  type RegionBehavior,
  type ManualRegionRole,
  type RegionContinuity,
  type RegionSampling,
  type ResolvedRegion,
  type SourceChannel,
  type SourceProjection,
  type SourceFrame,
  type SourceFramePreviewMaterialMap,
  type PartitionRecipe,
  type Stage14SlotProjection,
  type TrimSheetDocumentCommand,
  type FeedbackComparisonMode,
  type FeedbackContributionView,
  type FeedbackExecutionState,
  type FeedbackPreviewProfile,
  type FeedbackPreviewExecution,
  type FeedbackWorkbenchCommand,
  type FeedbackWorkbenchCommandResult,
  type Stage15To20DebugPayload,
  type Stage15To20DebugRequest,
} from "@hot-trimmer/ipc-contracts";
import { assignSourceFiles } from "./source-assignment";
import { adjustCrop, anchoredZoom, clamp01, constrainAspectBounds, fitSourceFrame, fitView, gridRectToPreviewBounds, mapQuadToUnitSquare, mapUnitSquareToQuad, movePatch, normalizePatchToRectangle, patchBounds, patchPointerAngle, preserveViewAcrossContentResize, resizeAspectLocked, resizePatch, resizePanes, rotatePatch, type CanvasView, type CropDragAction, type PaneDragKind, type PaneState, type PatchResizeHandle } from "./source-workbench-geometry";
import { GpuTiledPreviewPainter, gpuTiledPreviewMapMatches, shouldDisplayGpuTiledPreview, SourceFramePreviewController, type GpuTiledPreviewPaintSummary } from "./source-frame-preview-controller";
import { defaultPartitionRecipe, layoutTemplateOptions, layoutTemplateRecipe, selectedLayoutTemplate, type LayoutTemplateId } from "./hierarchical-layout-templates";
import { authoredGridResolutions, cellDragRect, diagonalCascadePreset, newBlankPreset, rescalePreset, snapshotDocumentPreset, snappedGridPoint, sourceFrameGridBounds } from "./manual-layout-presets";
import { canInteractWithPatch, compiledMapViewForSourceChannel, sourceChannelForCompiledMapView, sourceSetIdForRegion } from "./workbench-interactions";
import { FeedbackWorkbench, ProcessingSidebar } from "./feedback-workbench";
import { MaterialPreviewCanvas, materialPreviewReady } from "./material-preview";
import {
  feedbackExecutionMatchesRequest,
  edgeDetailInspectionForView,
  feedbackEvidenceForRequest,
  feedbackPixelRequestIdentity,
  feedbackPreviewRegionAfterCommand,
  feedbackRequestIsCurrent,
  feedbackViewAfterCommand,
  selectFeedbackRegionWithoutPixelWork,
  selectedOperationAfterCommand,
  sourceFrameMaterialMapForCompiledView,
  visibleMapDependency,
  type FeedbackPixelRequestIdentity,
} from "./feedback-workbench-contract";
import "./document-app.css";

const protocol = { protocolVersion: IPC_PROTOCOL_VERSION };
const gridResolutionOptions = [16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256] as const;
type WorkspaceMode = "authoring" | "processing";
type AuthoringPane = "workbench" | "hotspotSheet";

// Recipe fields are all primitive or fixed-order records; this canonical fingerprint is the
// exact candidate identity accepted by the preview/accept gate.
function partitionRecipeFingerprint(recipe: PartitionRecipe) { return JSON.stringify(recipe); }

const templates = [
  ["ht.generic_architecture", "Generic Architecture"],
  ["ht.horizontal_moulding", "Horizontal Moulding"],
  ["ht.vertical_trim", "Vertical Trim"],
  ["ht.wood_board_moulding", "Wood Board & Moulding"],
  ["ht.detail_ribbon_microtrim", "Detail Ribbon & Microtrim"],
  ["ht.hard_surface_panel", "Hard Surface & Panels"],
  ["ht.detail_heavy_props", "Detail-heavy Props"],
  ["ht.radial_accents", "Radial Accents"],
] as const;

const channelOptions: ReadonlyArray<{ value: SourceChannel; label: string; short: string; tone: string }> = [
  { value: "base_color", label: "Diffuse", short: "D", tone: "color" },
  { value: "normal", label: "Normal", short: "N", tone: "normal" },
  { value: "height", label: "Height", short: "H", tone: "height" },
  { value: "roughness", label: "Roughness", short: "R", tone: "roughness" },
  { value: "metallic", label: "Metallic", short: "M", tone: "metallic" },
  { value: "ambient_occlusion", label: "Ambient Occlusion", short: "AO", tone: "ao" },
  { value: "specular", label: "Specular", short: "S", tone: "specular" },
  { value: "opacity", label: "Opacity", short: "O", tone: "opacity" },
  { value: "edge_mask", label: "Edge Mask", short: "E", tone: "edge" },
  { value: "material_id", label: "Material ID", short: "ID", tone: "id" },
];

const mapViews: readonly [CompiledMapView, string][] = [
  ["baseColor", "Diffuse"],
  ["normal", "Normal"],
  ["height", "Height"],
  ["roughness", "Roughness"],
  ["metallic", "Metallic"],
  ["ambientOcclusion", "AO"],
  ["regionId", "Region ID"],
  ["materialId", "Material ID"],
];

const materialMapForView = sourceFrameMaterialMapForCompiledView;

function requestedMaterialMapsForView(view: CompiledMapView): readonly SourceFramePreviewMaterialMap[] {
  return [materialMapForView(view)];
}

export const processingPreviewMaterialMaps: readonly SourceFramePreviewMaterialMap[] = [
  "base_color",
  "normal",
  "height",
  "roughness",
  "metallic",
  "ambient_occlusion",
  "edge_mask",
];

const exportableMaterialMaps: readonly SourceFramePreviewMaterialMap[] = [
  "base_color",
  "height",
  "normal",
  "roughness",
  "metallic",
  "ambient_occlusion",
  "edge_mask",
  "region_id",
];

function requestedMaterialMapsForExport(project: ProjectProjection | null): readonly SourceFramePreviewMaterialMap[] {
  const channels = project?.document?.renderSettings.channels;
  if (!channels) return ["base_color"];
  const requested = exportableMaterialMaps.filter((map) => channels[map]?.enabled);
  if (project?.document?.edgeDetail?.enabled) {
    for (const map of ["base_color", "height", "normal", "roughness", "metallic", "edge_mask"] as const) {
      if (!requested.includes(map)) requested.push(map);
    }
  }
  return requested.length > 0 ? requested : ["base_color"];
}

function gpuTilePublicationForView(artifact: IntermediateAtlasProjection | null | undefined, view: CompiledMapView) {
  if (!artifact) return undefined;
  return artifact.tileManifests?.[view]
    ?? (artifact.tileManifest && gpuTiledPreviewMapMatches(artifact.tileManifest.manifest.map, view) ? artifact.tileManifest : undefined);
}

function artifactMapAvailable(artifact: IntermediateAtlasProjection | null | undefined, view: CompiledMapView): boolean {
  return !!artifact && (!!artifact.maps[view] || !!gpuTilePublicationForView(artifact, view));
}

function materialMapRouteAvailable(view: CompiledMapView): boolean {
  return materialMapForView(view).length > 0;
}

const materialBehaviorOptions: readonly [MaterialBehaviorClass, string][] = [
  ["already_tileable", "Already tileable"],
  ["stochastic_isotropic", "Stochastic isotropic"],
  ["stochastic_directional", "Stochastic directional"],
  ["periodic_lattice_structured", "Periodic/lattice structured"],
  ["layered_banded", "Layered/banded"],
  ["organic_directional", "Organic directional"],
  ["manufactured_pattern", "Manufactured pattern"],
  ["unique_detail", "Unique detail"],
  ["radial_detail", "Radial detail"],
  ["mixed_unknown", "Mixed/Unknown"],
];

const manualRoleOptions: readonly [ManualRegionRole, string][] = [
  ["panel", "Panel"], ["horizontal_strip", "Horizontal strip"], ["vertical_strip", "Vertical strip"],
  ["unique", "Unique"], ["radial", "Radial"],
];
const continuityOptions: readonly [RegionContinuity, string][] = [["none", "None"], ["x", "X"], ["y", "Y"], ["xy", "XY"]];
const samplingOptions: readonly [RegionSampling, string][] = [["one_shot", "One-shot"], ["loop_x", "Loop X"], ["loop_y", "Loop Y"], ["loop_xy", "Loop XY"]];

function eligibleEdges(continuity: RegionContinuity) {
  return { left: continuity !== "x" && continuity !== "xy", right: continuity !== "x" && continuity !== "xy", top: continuity !== "y" && continuity !== "xy", bottom: continuity !== "y" && continuity !== "xy" };
}

function samplingPrerequisite(role: ManualRegionRole, sampling: RegionSampling): string | null {
  if (sampling === "one_shot" || role === "panel") return null;
  if (role === "horizontal_strip" && sampling === "loop_x") return null;
  if (role === "vertical_strip" && sampling === "loop_y") return null;
  if (role === "radial") return "Radial regions currently execute one-shot PlanarRadial only.";
  if (role === "unique") return "Unique regions are intentionally one-shot.";
  return role === "horizontal_strip" ? "Horizontal strips can loop only along X." : "Vertical strips can loop only along Y.";
}

function changedBehavior(current: RegionBehavior, patch: Partial<RegionBehavior>): RegionBehavior {
  const next = { ...current, ...patch };
  next.edgeEligibility = eligibleEdges(next.continuity);
  if (next.role === "radial") next.radial = next.radial ?? { centerX: .5, centerY: .5, innerRadius: 0, outerRadius: .5, falloff: 1, blendWidth: 0, seamBlendWidth: .03 };
  else delete next.radial;
  if (samplingPrerequisite(next.role, next.sampling)) next.sampling = "one_shot";
  return next;
}

function behaviorCueLabel(behavior: RegionBehavior) {
  const role = manualRoleOptions.find(([value]) => value === behavior.role)?.[1] ?? behavior.role;
  const continuity = behavior.continuity === "none" ? "no continuous edges" : `continuous ${behavior.continuity.toUpperCase()}`;
  const sampling = samplingOptions.find(([value]) => value === behavior.sampling)?.[1] ?? behavior.sampling;
  const orientation = { zero: "0°", ninety: "90°", one_eighty: "180°", two_seventy: "270°" }[behavior.orientation];
  return `${role}; ${sampling}; ${continuity}; orientation ${orientation}`;
}

function RegionBehaviorCue({ behavior }: { behavior: RegionBehavior }) {
  const loopGlyph = { one_shot: "1×", loop_x: "↔", loop_y: "↕", loop_xy: "⤢" }[behavior.sampling];
  const orientationGlyph = { zero: "→", ninety: "↓", one_eighty: "←", two_seventy: "↑" }[behavior.orientation];
  const continuous = behavior.continuity === "none" ? [] : behavior.continuity === "x" ? ["left", "right"] : behavior.continuity === "y" ? ["top", "bottom"] : ["left", "right", "top", "bottom"];
  return <span className="region-behavior-cue" aria-label={behaviorCueLabel(behavior)} title={behaviorCueLabel(behavior)}>
    {behavior.sampling !== "one_shot" ? <i className={`behavior-loop ${behavior.sampling}`}>{loopGlyph}</i> : null}<i className="behavior-orientation">{orientationGlyph}</i>
    {continuous.map((edge) => <i key={edge} className={`continuity-cue ${edge}`} />)}
  </span>;
}

type Activity = "starting" | "idle" | "importing" | "compiling" | "editing" | "saving" | "opening" | "exporting";
type CropProjection = Extract<RegionMapping["projection"], { type: "crop" }>;
type PaneLayoutMode = "full" | "without-inspector" | "without-library" | "sheet-only";

interface PreparedPatchPreviewProjection {
  patchId: string;
  materialSourceId: string;
  width: number;
  height: number;
  dataUrl: string;
  perspectiveConfidenceMilli: number;
  delightingRoute: string;
  delightingStrengthMilli: number;
  sourceAnalysis: {
    qualitySummary: string;
    analyzedClass: MaterialBehaviorClass;
    routedClass: MaterialBehaviorClass;
    confidencePercent: number;
    evidenceSummary: string;
    warningCount: number;
    scaleSummary: string;
    orientationSummary: string;
    worldScaleAvailable: boolean;
    orientationOverlay: readonly { sourceXMilli: number; sourceYMilli: number; axisMillidegrees: number | null; confidenceMilli: number }[];
  };
}

function paneLayoutMode(width: number): PaneLayoutMode {
  if (width >= 998) return "full";
  if (width >= 712) return "without-inspector";
  if (width >= 506) return "without-library";
  return "sheet-only";
}

function reconcilePanes(current: PaneState, width: number): PaneState {
  let library = Math.min(420, Math.max(160, current.library));
  let source = Math.min(650, Math.max(260, current.source));
  let inspector = Math.min(420, Math.max(230, current.inspector));
  const mode = paneLayoutMode(width);
  const available = mode === "full" ? width - 338 : mode === "without-inspector" ? width - 292 : width - 266;
  if (mode === "full") {
    let overflow = Math.max(0, library + source + inspector - available);
    const shrink = (value: number, minimum: number) => {
      const amount = Math.min(overflow, value - minimum);
      overflow -= amount;
      return value - amount;
    };
    source = shrink(source, 260);
    inspector = shrink(inspector, 230);
    library = shrink(library, 160);
  } else if (mode === "without-inspector") {
    let overflow = Math.max(0, library + source - available);
    const amount = Math.min(overflow, source - 260);
    source -= amount;
    overflow -= amount;
    library -= Math.min(overflow, library - 160);
  } else if (mode === "without-library") {
    source = Math.min(source, Math.max(240, available));
  }
  if (library === current.library && source === current.source && inspector === current.inspector) return current;
  return { library, source, inspector };
}

function isNativeRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

function failure(reason: unknown): CommandFailure {
  if (typeof reason === "object" && reason && "message" in reason) {
    const value = reason as Partial<CommandFailure>;
    return {
      code: value.code ?? "operation_failed",
      message: String(value.message),
      recovery: value.recovery ?? "Correct the issue and retry.",
      detail: value.detail,
    };
  }
  return { code: "operation_failed", message: String(reason), recovery: "Correct the issue and retry." };
}

type PreviewProfile = "draft512" | "refinement1024" | "preview2048" | "preview4096" | "preview8192" | "authoritative";
type InteractivePreviewProfile = Exclude<PreviewProfile, "draft512" | "authoritative">;
function isInteractivePreviewProfile(value: string | null): value is InteractivePreviewProfile {
  return value === "refinement1024"
    || value === "preview2048"
    || value === "preview4096"
    || value === "preview8192";
}

function automaticPreviewKey(revision: number, profile: PreviewProfile, view: CompiledMapView | "materialSet"): string {
  return `${revision}:${profile}:${view}`;
}

function previewProfileDimensions(profile: PreviewProfile, outputSize?: { width: number; height: number }): { width: number; height: number } {
  switch (profile) {
    case "draft512": return { width: 512, height: 512 };
    case "refinement1024": return { width: 1024, height: 1024 };
    case "preview2048": return { width: 2048, height: 2048 };
    case "preview4096": return { width: 4096, height: 4096 };
    case "preview8192": return { width: 8192, height: 8192 };
    case "authoritative": return outputSize ?? { width: 2048, height: 2048 };
  }
}

type PreviewProgress = {
  requestId: number;
  phase: "compiling" | "received" | "painted" | "failed";
  profile: PreviewProfile;
  requestedRevision: number;
  requestedMap: CompiledMapView;
  startedAt: number;
  elapsedMs?: number;
  targetDimensions?: { width: number; height: number };
  dimensions?: { width: number; height: number };
  terminalOutcome?: "published" | "failed" | "superseded";
  feedbackRequestIdentity?: string;
  feedbackRegionId?: string;
  feedbackView?: FeedbackContributionView;
  feedbackComparisonMode?: FeedbackComparisonMode;
  feedbackSelectedOperationId?: string | null;
};

function App() {
  const native = isNativeRuntime();
  const [project, setProject] = useState<ProjectProjection | null>(null);
  const [artifact, setArtifact] = useState<IntermediateAtlasProjection | null>(null);
  const [preview, setPreview] = useState<PreviewSheetProjection | null>(null);
  const [previewClientTelemetry, setPreviewClientTelemetry] = useState<string[]>([]);
  const [interactivePreviewProfile, setInteractivePreviewProfile] = useState<InteractivePreviewProfile>(() => {
    const saved = localStorage.getItem("hot-trimmer.interactive-preview-profile.v1");
    return isInteractivePreviewProfile(saved) ? saved : "refinement1024";
  });
  const [previewProgress, setPreviewProgress] = useState<PreviewProgress | null>(null);
  const [previewElapsedMs, setPreviewElapsedMs] = useState(0);
  const [templateId, setTemplateId] = useState<string>(templates[0][0]);
  const [targetRegionCount, setTargetRegionCount] = useState(63);
  const [candidateRecipe, setCandidateRecipe] = useState<PartitionRecipe>(defaultPartitionRecipe);
  const [candidatePreviewing, setCandidatePreviewing] = useState(false);
  const [candidatePreviewHash, setCandidatePreviewHash] = useState<string | null>(null);
  const [candidatePreviewRecipe, setCandidatePreviewRecipe] = useState<PartitionRecipe | null>(null);
  const [selectedSourceSetId, setSelectedSourceSetId] = useState<string>("");
  const [selectedChannel, setSelectedChannel] = useState<SourceChannel>("base_color");
  const [normalConvention, setNormalConvention] = useState<Extract<NormalConvention, "open_gl" | "direct_x">>("open_gl");
  const [draftPreviewFps, setDraftPreviewFps] = useState<10 | 30 | 60>(30);
  const [actualDraftPreviewFps, setActualDraftPreviewFps] = useState<number | null>(null);
  const [selectedRegionId, setSelectedRegionId] = useState<string | null>(null);
  const [sourceFrameEditing, setSourceFrameEditing] = useState(false);
  const [mapView, setMapViewState] = useState<CompiledMapView>("baseColor");
  const [activity, setActivity] = useState<Activity>("starting");
  const [problem, setProblem] = useState<CommandFailure | null>(null);
  const [importProgress, setImportProgress] = useState<{ stage: string; fraction: number } | null>(null);
  const [exportProgress, setExportProgress] = useState<NativeStage14ExportProgress | null>(null);
  const [recentProjects, setRecentProjects] = useState<RecentProject[]>([]);
  const [showRecents, setShowRecents] = useState(false);
  const [panes, setPanes] = useState<PaneState>(() => {
    try { return { library: 220, source: 470, inspector: 278, ...JSON.parse(localStorage.getItem("hot-trimmer.workbench-panes.v1") ?? "{}") }; }
    catch { return { library: 220, source: 470, inspector: 278 }; }
  });
  const [workbenchWidth, setWorkbenchWidth] = useState(1280);
  const [renaming, setRenaming] = useState(false);
  const [draftName, setDraftName] = useState("");
  const [activePatchId, setActivePatchId] = useState<string | null>(null);
  const [regionPatchEditId, setRegionPatchEditId] = useState<string | null>(null);
  const patchFallbackContent = useRef(new Map<string, ContentReference>());
  const patchGeometryEditDepth = useRef(new Map<string, number>());
  const patchGeometryRedoDepth = useRef(new Map<string, number>());
  const lastRegionPatchId = useRef<string | null>(null);
  const undoneRegionPatchConversion = useRef<string | null>(null);
  const [preparedPatchPreview, setPreparedPatchPreview] = useState<PreparedPatchPreviewProjection | null>(null);
  const [preparedPatchPreviews, setPreparedPatchPreviews] = useState<Record<string, PreparedPatchPreviewProjection>>({});
  const [draftPatchPreview, setDraftPatchPreview] = useState<{ patchId: string; geometry: PatchGeometry } | null>(null);
  const [patchTool, setPatchTool] = useState<"rectangle" | "four-point" | null>(null);
  const [workspaceMode, setWorkspaceMode] = useState<WorkspaceMode>("authoring");
  const [authoringPanes, setAuthoringPanes] = useState<ReadonlySet<AuthoringPane>>(() => new Set(["workbench", "hotspotSheet"]));
  const [debugOpen, setDebugOpen] = useState(false);
  const [processingSidebarTab, setProcessingSidebarTab] = useState<"layers" | "outputs">("layers");
  const [processingRequestedMap, setProcessingRequestedMap] = useState<CompiledMapView>("baseColor");
  const [feedbackView, setFeedbackView] = useState<FeedbackContributionView>("stage15Height");
  const [feedbackProfile, setFeedbackProfile] = useState<FeedbackPreviewProfile>("preview2048");
  const [feedbackComparisonMode, setFeedbackComparisonMode] = useState<FeedbackComparisonMode>("after");
  const [feedbackActiveTool, setFeedbackActiveTool] = useState<"select" | "profile" | "stamp" | "stroke">("select");
  const [feedbackSelectedOperationId, setFeedbackSelectedOperationId] = useState<string | null>(null);
  const [feedbackPreviewAllRegions, setFeedbackPreviewAllRegions] = useState(false);
  const [feedbackCommandBusy, setFeedbackCommandBusy] = useState(false);
  const [feedbackLastCommandResult, setFeedbackLastCommandResult] = useState<string | null>(null);
  const [feedbackError, setFeedbackError] = useState<CommandFailure | null>(null);
  const [edgeDetailError, setEdgeDetailError] = useState<CommandFailure | null>(null);
  const [feedbackExecution, setFeedbackExecution] = useState<FeedbackPreviewExecution | null>(null);
  const [sourceSheetShare, setSourceSheetShare] = useState(() => {
    const saved = Number(localStorage.getItem("hot-trimmer.source-sheet-share.v1") ?? "0.46");
    return Number.isFinite(saved) ? Math.max(0.2, Math.min(0.8, saved)) : 0.46;
  });
  const started = useRef(false);
  const documentHistoryBusy = useRef(false);
  const previewDraftId = useRef(0);
  const dirtyPreviewRegion = useRef<string | null>(null);
  const suppressAutomaticPreviewRevision = useRef<number | null>(null);
  const lastAutomaticPreviewKey = useRef<string | null>(null);
  const pendingAutomaticPreviewKey = useRef<string | null>(null);
  const patchPreviewRequestId = useRef(0);
  const radialCommitId = useRef(0);
  const previewPublishStartedAt = useRef<number | null>(null);
  const projectRef = useRef<ProjectProjection | null>(null);
  const mapViewRef = useRef<CompiledMapView>("baseColor");
  const lastTransientCompletionAt = useRef(0);
  const smoothedTransientFps = useRef(0);
  const transientPreviewController = useRef(new SourceFramePreviewController<{
    patchId: string;
    geometry?: PatchGeometry;
    maxEdge: number;
    requestId: number;
  }>());
  const sourceFramePreviewController = useRef(new SourceFramePreviewController<{
    regionId: string;
    projection: CropProjection;
    revision: number;
  }>());
  const paneDrag = useRef<{ kind: PaneDragKind; start: PaneState } | null>(null);
  useEffect(() => { localStorage.setItem("hot-trimmer.interactive-preview-profile.v1", interactivePreviewProfile); }, [interactivePreviewProfile]);
  useEffect(() => {
    if (previewProgress?.phase !== "compiling") return;
    const update = () => setPreviewElapsedMs(performance.now() - previewProgress.startedAt);
    update();
    const timer = window.setInterval(update, 100);
    return () => window.clearInterval(timer);
  }, [previewProgress?.requestId, previewProgress?.phase]);
  const workbenchRef = useRef<HTMLElement | null>(null);
  useEffect(() => { projectRef.current = project; }, [project]);
  useEffect(() => { mapViewRef.current = mapView; }, [mapView]);

  useEffect(() => {
    if (!native) return;
    let disposed = false;
    void listen<{ stage: string; fraction: number }>("import-progress", (event) => {
      if (!disposed) setImportProgress(event.payload.fraction >= 1 ? null : event.payload);
    }).then((unlisten) => { if (disposed) unlisten(); });
    return () => { disposed = true; };
  }, [native]);

  useEffect(() => {
    if (!native) return;
    let disposed = false;
    void listen<NativeStage14ExportProgressEvent>("stage-14-export-progress", (event) => {
      if (!disposed) setExportProgress(event.payload.progress);
    }).then((unlisten) => { if (disposed) unlisten(); });
    return () => { disposed = true; };
  }, [native]);

  const sourceSets = project?.materialSources ?? [];
  const activeSourceSetId = selectedSourceSetId || sourceSets[0]?.id || "";
  const activeSources = useMemo(
    () => project?.materialSources.find((source) => source.id === activeSourceSetId)?.registeredChannels?.channels ?? [],
    [project?.materialSources, activeSourceSetId],
  );
  const baseSources = project?.materialSources.flatMap((source) =>
    source.registeredChannels?.channels.filter((channel) => channel.channel === "base_color") ?? []) ?? [];
  const selectedSource = activeSources.find((source) => source.channel === selectedChannel)
    ?? activeSources.find((source) => source.channel === "base_color")
    ?? activeSources[0]
    ?? null;
  const primaryMaterial = project?.document?.primaryMaterial ?? "";
  const selectedRegion = artifact?.regions.find((region) => region.regionId === selectedRegionId) ?? null;
  const selectedSlot = artifact?.slots.find((slot) => slot.regionId === selectedRegionId) ?? null;
  const selectedBinding = selectedRegionId ? project?.document?.regionBindings[selectedRegionId] ?? null : null;
  const selectedBindingContent = selectedBinding?.content;
  const selectedRadialSourceGeometry = selectedBindingContent?.type === "patch" ? (() => {
    const patch = project?.patches.find((value) => value.id === selectedBindingContent.id);
    return patch?.geometry;
  })() : (() => {
    const bounds = selectedSlot?.sourceBounds ?? { x: 0, y: 0, width: 1, height: 1 };
    return { corners: [
      { x: bounds.x, y: bounds.y }, { x: bounds.x + bounds.width, y: bounds.y },
      { x: bounds.x + bounds.width, y: bounds.y + bounds.height }, { x: bounds.x, y: bounds.y + bounds.height },
    ] } as PatchGeometry;
  })();
  const selectedCrop = selectedBinding?.mapping.projection.type === "crop" ? selectedBinding.mapping.projection : null;
  const selectedCanvasCrop = selectedBindingContent?.type !== "patch" && selectedSlot?.sourceBounds ? {
    type: "crop" as const,
    bounds: selectedSlot.sourceBounds,
    focus: { x: selectedSlot.sourceBounds.x + selectedSlot.sourceBounds.width * 0.5, y: selectedSlot.sourceBounds.y + selectedSlot.sourceBounds.height * 0.5 },
  } : null;
  const currentTopologyHash = project?.document ? hashBytes(project.document.topology.topologyHash) : null;
  const stale = !!project?.document && !!artifact && artifact.documentRevision !== project.document.documentRevision;
  const buildState = buildStatus(project, artifact, activity, problem, stale);
  const paneMode = paneLayoutMode(workbenchWidth);
  const sourceFrameLayout = !!project?.document?.sourceFrame;
  const processingOpen = workspaceMode === "processing";
  const sourceWorkbenchOpen = workspaceMode === "authoring" && authoringPanes.has("workbench");
  const hotspotSheetOpen = workspaceMode === "authoring" && authoringPanes.has("hotspotSheet");
  const showLibrary = sourceWorkbenchOpen && (paneMode === "full" || paneMode === "without-inspector");
  const showSourceWorkspace = sourceWorkbenchOpen && paneMode !== "sheet-only";
  // The Layout sidebar is the single region inspector. Keep the former context
  // inspector out of the pane layout so it cannot duplicate map or region controls.
  const showInspector = false;
  const visibleTracks: string[] = [];
  if (showLibrary) visibleTracks.push(`${paneMode === "without-inspector" ? Math.min(240, panes.library) : panes.library}px`);
  if (showSourceWorkspace && hotspotSheetOpen) visibleTracks.push(`minmax(280px, ${sourceSheetShare}fr)`, `minmax(320px, ${1 - sourceSheetShare}fr)`);
  else if (showSourceWorkspace || hotspotSheetOpen) visibleTracks.push("minmax(0, 1fr)");
  if (showInspector) visibleTracks.push(`${panes.inspector}px`);
  const workbenchColumns = processingOpen
    ? "minmax(0, 1fr) minmax(360px, 430px)"
    : visibleTracks.length ? visibleTracks.join(" 6px ") : "minmax(0, 1fr)";

  useEffect(() => {
    // A topology command starts its replacement request before React commits this
    // effect. Do not advance the client generation here: doing so discards that
    // valid result and leaves the progress UI counting forever. The replacement
    // request itself advances the generation and native revision guards cancel
    // any genuinely older work.
    dirtyPreviewRegion.current = null;
    setPreview(null);
  }, [currentTopologyHash]);

  useEffect(() => {
    if (!native || !project?.document || processingOpen) return;
    const key = automaticPreviewKey(project.document.documentRevision, interactivePreviewProfile, mapView);
    if (suppressAutomaticPreviewRevision.current === project.document.documentRevision) {
      suppressAutomaticPreviewRevision.current = null;
      return;
    }
    if (lastAutomaticPreviewKey.current === key || pendingAutomaticPreviewKey.current === key) return;
    pendingAutomaticPreviewKey.current = key;
    dirtyPreviewRegion.current = null;
    // Persisted appearance edits are not transient crop requests. The compiler's content
    // hashes reuse every unchanged region while the request remains contract-valid.
    void requestPreview(undefined, undefined, interactivePreviewProfile, project.document.documentRevision, false, true);
  }, [native, project?.document?.documentRevision, interactivePreviewProfile, mapView, processingOpen]);

  useEffect(() => {
    const controller = transientPreviewController.current;
    controller.setMaxFps(draftPreviewFps);
    if (!native || !activePatchId) {
      controller.cancel();
      setPreparedPatchPreview(null);
      return;
    }
    const transient = draftPatchPreview?.patchId === activePatchId ? draftPatchPreview : null;
    const requestId = ++patchPreviewRequestId.current;
    controller.setExecutor(async (request) => {
      return invoke<PreparedPatchPreviewProjection>("prepare_patch_preview", {
        request: {
          ...protocol,
          patchId: request.patchId,
          maxEdge: request.maxEdge,
          geometry: request.geometry,
        },
      }).then((value) => {
        if (request.requestId === patchPreviewRequestId.current && value.patchId === request.patchId) {
          setPreparedPatchPreview(value);
          setPreparedPatchPreviews((current) => ({ ...current, [value.patchId]: value }));
          if (request.maxEdge === 256) {
            const completedAt = performance.now();
            if (lastTransientCompletionAt.current > 0) {
              const sample = 1000 / Math.max(1, completedAt - lastTransientCompletionAt.current);
              smoothedTransientFps.current = smoothedTransientFps.current
                ? smoothedTransientFps.current * 0.7 + sample * 0.3
                : sample;
              setActualDraftPreviewFps(Math.round(smoothedTransientFps.current));
            }
            lastTransientCompletionAt.current = completedAt;
          } else {
            setProblem(null);
          }
        }
      }).catch((reason) => {
        if (request.requestId === patchPreviewRequestId.current && request.maxEdge !== 256) setProblem(failure(reason));
      });
    });
    controller.enqueue({ patchId: activePatchId, maxEdge: transient ? 256 : 512, geometry: transient?.geometry, requestId });
  }, [native, activePatchId, project?.patches, draftPatchPreview, draftPreviewFps]);

  useEffect(() => {
    const controller = sourceFramePreviewController.current;
    controller.setMaxFps(draftPreviewFps);
    if (!native || project?.document?.documentRevision === undefined) {
      controller.cancel();
      return;
    }
    controller.setExecutor(async (request) => {
      await requestPreview(request.regionId, request.projection, interactivePreviewProfile, request.revision, false);
    });
    return () => controller.cancel();
  }, [native, project?.document?.documentRevision, draftPreviewFps, interactivePreviewProfile]);

  useEffect(() => {
    const element = workbenchRef.current;
    if (!element) return;
    const update = () => {
      const width = element.clientWidth;
      setWorkbenchWidth(width);
      setPanes((current) => reconcilePanes(current, width));
    };
    const observer = new ResizeObserver(update);
    observer.observe(element);
    update();
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    localStorage.setItem("hot-trimmer.workbench-panes.v1", JSON.stringify(panes));
  }, [panes]);
  useEffect(() => { localStorage.setItem("hot-trimmer.source-sheet-share.v1", String(sourceSheetShare)); }, [sourceSheetShare]);

  useEffect(() => {
    const revision = project?.document?.documentRevision;
    if (!processingOpen || revision === undefined) return;
    const key = automaticPreviewKey(revision, interactivePreviewProfile, "materialSet");
    if (lastAutomaticPreviewKey.current === key || pendingAutomaticPreviewKey.current === key) return;
    pendingAutomaticPreviewKey.current = key;
    setFeedbackExecution(null);
    setEdgeDetailError(null);
    void requestPreview(undefined, undefined, interactivePreviewProfile, revision, false, true, processingRequestedMap);
    // Processing commands publish their own replacement. A document revision
    // effect here would race that explicit request and supersede both outputs.
  }, [processingOpen, interactivePreviewProfile]);

  useEffect(() => {
    const content = selectedBinding?.content;
    if (!content) return;
    if (content.type === "material_source") {
      setSelectedSourceSetId(content.id); setActivePatchId(null);
    } else if (content.type === "patch") {
      const patch = project?.patches.find((value) => value.id === content.id);
      const owner = patch && project?.materialSources.find((set) => set.registeredChannels?.channels.some((source) => source.id === patch.sourceId));
      if (owner) { setSelectedSourceSetId(owner.id); setActivePatchId(content.id); }
    } else if (content.type === "inherit_primary_material" && project?.document?.primaryMaterial) {
      setSelectedSourceSetId(project.document.primaryMaterial); setActivePatchId(null);
    }
  }, [selectedRegionId]);

  useEffect(() => {
    setRegionPatchEditId((current) => current && current !== selectedRegionId ? null : current);
    if (selectedRegionId) setSourceFrameEditing(false);
  }, [selectedRegionId]);

  useEffect(() => {
    if (started.current) return;
    started.current = true;
    if (!native) {
      setActivity("idle");
      return;
    }
    void refreshRecents();
    void bootDraft();
  }, [native]);

  useEffect(() => {
    if (!native) return;
    let removeDrop: (() => void) | undefined;
    let removeRoute: (() => void) | undefined;
    void getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type !== "drop") return;
      const paths = event.payload.paths;
      const first = paths[0];
      if (!first) return;
      if (first.toLowerCase().endsWith(".hottrimmer")) {
        void openProjectAt(first);
      } else {
        void importImages(paths, activeSourceSetId);
      }
    }).then((unlisten) => { removeDrop = unlisten; });
    void listen<string>("open-project-requested", (event) => {
      void openProjectAt(event.payload);
    }).then((unlisten) => { removeRoute = unlisten; });
    return () => {
      removeDrop?.();
      removeRoute?.();
    };
  }, [native, activeSourceSetId, project]);

  useEffect(() => {
    function keydown(event: KeyboardEvent) {
      const target = event.target as HTMLElement | null;
      const typing = target?.tagName === "INPUT" || target?.tagName === "TEXTAREA" || target?.tagName === "SELECT" || Boolean(target?.isContentEditable);
      if (typing) return;
      if (event.key === "Escape") {
        setSelectedRegionId(null);
        setActivePatchId(null);
        setPatchTool(null);
      }
      if ((event.key === "Delete" || event.key === "Backspace") && activePatchId) {
        event.preventDefault();
        void deletePatch(activePatchId);
      }
    }
    window.addEventListener("keydown", keydown);
    return () => window.removeEventListener("keydown", keydown);
  }, [activePatchId]);

  async function bootDraft() {
    setActivity("starting");
    try {
      const path = await invoke<string | null>("take_pending_project_path", { request: protocol });
      if (path) {
        await openProjectAt(path);
      } else {
        await createDraft();
      }
    } catch (reason) {
      setProblem(failure(reason));
      setActivity("idle");
    }
  }

  async function refreshRecents() {
    if (!native) return;
    const recent = await invoke<RecentProject[]>("list_recent_projects", { request: protocol }).catch(() => []);
    setRecentProjects(recent);
  }

  async function createDraft() {
    setActivity("opening");
    setProblem(null);
    try {
      const next = await invoke<ProjectProjection>("create_draft_project", { request: protocol });
      acceptProject(next);
    } catch (reason) {
      setProblem(failure(reason));
    } finally {
      setActivity("idle");
    }
  }

  async function chooseProject() {
    const path = await open({
      multiple: false,
      title: "Open Hot Trimmer project",
      filters: [{ name: "Hot Trimmer", extensions: ["hottrimmer"] }],
    });
    if (typeof path === "string") await openProjectAt(path);
  }

  async function openProjectAt(path: string) {
    setActivity("opening");
    setProblem(null);
    try {
      const next = await invoke<ProjectProjection>("open_project", { request: { ...protocol, path } });
      acceptProject(next);
      void refreshRecents();
    } catch (reason) {
      setProblem(failure(reason));
    } finally {
      setActivity("idle");
    }
  }

  function acceptProject(next: ProjectProjection) {
    // Project replacement is a hard UI boundary. In-flight patch work belongs to the
    // previous project and must never be allowed to publish into the fresh draft.
    patchPreviewRequestId.current += 1;
    transientPreviewController.current.cancel();
    sourceFramePreviewController.current.cancel();
    patchFallbackContent.current.clear();
    setProject(next);
    setArtifact(null);
    setPreview(null);
    setProblem(null);
    setSelectedRegionId(null);
    setActivePatchId(null);
    setRegionPatchEditId(null);
    setPreparedPatchPreview(null);
    setPreparedPatchPreviews({});
    setDraftPatchPreview(null);
    setPatchTool(null);
    setSourceFrameEditing(false);
    dirtyPreviewRegion.current = null;
    suppressAutomaticPreviewRevision.current = null;
    lastAutomaticPreviewKey.current = null;
    pendingAutomaticPreviewKey.current = null;
    setSelectedSourceSetId(next.document?.primaryMaterial ?? next.materialSources[0]?.id ?? "");
    selectSourceChannel("base_color");
    setTemplateId(templates[0][0]);
    setShowRecents(false);
  }

  async function chooseImages(channel?: SourceChannel, sourceSetId = activeSourceSetId) {
    if (!project) return;
    const chosen = await open({
      multiple: channel ? false : true,
      title: channel ? `Add ${channelLabel(channel)}` : "Open source images",
      filters: [{ name: "Source images", extensions: ["png", "jpg", "jpeg", "tif", "tiff"] }],
    });
    if (!chosen) return;
    const paths = Array.isArray(chosen) ? chosen : [chosen];
    if (channel) {
      await importOne(paths[0]!, channel, sourceSetId);
    } else {
      await importImages(paths, sourceSetId);
    }
  }

  async function importImages(paths: string[], sourceSetId = activeSourceSetId) {
    if (!project || !sourceSetId) return;
    const occupied = project.materialSources
      .find((source) => source.id === sourceSetId)?.registeredChannels?.channels
      .map((source) => source.channel) ?? [];
    const assignments = assignSourceFiles(paths, occupied);
    if (!assignments.length) {
      setProblem({
        code: "source_map_not_identified",
        message: "No selected file matched an empty registered map slot.",
        recovery: "Open an empty map slot directly, or include a Diffuse texture for a new material source.",
      });
      return;
    }
    setActivity("importing");
    setProblem(null);
    try {
      let next = project;
      for (const assignment of assignments) {
        next = await invoke<ProjectProjection>("import_source", {
          request: {
            ...protocol,
            path: assignment.path,
            ownership: "owned_copy",
            channel: assignment.channel,
            sourceSetId,
            assignmentProvenance: assignment.assignmentProvenance,
            confidenceMilli: assignment.confidenceMilli,
            normalConvention: assignment.channel === "normal" ? normalConvention : "not_applicable",
          },
        });
      }
      setProject(next);
      setSelectedSourceSetId(sourceSetId);
      selectSourceChannel(assignments.at(-1)?.channel ?? "base_color");
      if (!next.document && assignments.some((assignment) => assignment.channel === "base_color")) {
        await createDocumentAndCompile(next, sourceSetId);
      }
    } catch (reason) {
      setProblem(failure(reason));
    } finally {
      setActivity("idle");
    }
  }

  async function importOne(path: string, channel: SourceChannel, sourceSetId: string) {
    if (!project || !sourceSetId) return;
    setActivity("importing");
    setProblem(null);
    try {
      const next = await invoke<ProjectProjection>("import_source", {
        request: {
          ...protocol, path, ownership: "owned_copy", channel, sourceSetId,
          assignmentProvenance: "user_assigned", confidenceMilli: 1000,
          normalConvention: channel === "normal" ? normalConvention : "not_applicable",
        },
      });
      setProject(next);
      setSelectedSourceSetId(sourceSetId);
      selectSourceChannel(channel);
      if (!next.document && channel === "base_color") {
        await createDocumentAndCompile(next, sourceSetId);
      }
    } catch (reason) {
      setProblem(failure(reason));
    } finally {
      setActivity("idle");
    }
  }

  async function addSourceSet() {
    const id = crypto.randomUUID();
    setSelectedSourceSetId(id);
    await chooseImages("base_color", id);
  }

  async function setExemplarGroup(materialSourceId: string, exemplarGroup: string | null) {
    if (!project) return;
    try {
      const next = await invoke<ProjectProjection>("set_exemplar_group", {
        request: { ...protocol, materialSourceId, exemplarGroup },
      });
      setProject(next);
      setArtifact(null);
      setProblem(null);
    } catch (reason) {
      setProblem(failure(reason));
    }
  }

  async function setDelightingIntent(materialSourceId: string, delighting: DelightingIntent) {
    if (!project) return;
    try {
      const next = await invoke<ProjectProjection>("set_delighting_intent", {
        request: { ...protocol, materialSourceId, delighting },
      });
      setProject(next);
      setArtifact(null);
      setProblem(null);
    } catch (reason) {
      setProblem(failure(reason));
    }
  }

  async function applyMaterialClassificationCommand(
    materialSourceId: string,
    classificationCommand: MaterialClassificationCommand,
  ) {
    if (!project) return;
    try {
      const next = await invoke<ProjectProjection>("apply_material_classification_command", {
        request: { ...protocol, materialSourceId, classificationCommand },
      });
      setProject(next);
      setArtifact(null);
      setProblem(null);
    } catch (reason) {
      setProblem(failure(reason));
    }
  }

  async function applyMaterialCalibrationCommand(
    materialSourceId: string,
    calibrationCommand: MaterialCalibrationCommand,
  ) {
    if (!project) return;
    try {
      const next = await invoke<ProjectProjection>("apply_material_calibration_command", {
        request: { ...protocol, materialSourceId, calibrationCommand },
      });
      setProject(next);
      setPreparedPatchPreview(null);
      setArtifact(null);
      setProblem(null);
    } catch (reason) {
      setProblem(failure(reason));
    }
  }

  async function command(commandValue: TrimSheetDocumentCommand): Promise<ProjectProjection> {
    const next = await applyCommand(commandValue);
    setProject(next);
    setProblem(null);
    return next;
  }

  async function applyCommand(commandValue: TrimSheetDocumentCommand): Promise<ProjectProjection> {
    return invoke<ProjectProjection>("apply_document_command", {
      request: { ...protocol, command: commandValue },
    });
  }

  async function applyFeedbackCommand(commandValue: FeedbackWorkbenchCommand): Promise<void> {
    if (!native || !project?.document || feedbackCommandBusy) return;
    setFeedbackCommandBusy(true);
    setFeedbackError(null);
    if (commandValue.type === "set_edge_detail") setEdgeDetailError(null);
    setProblem(null);
    try {
      const result = await invoke<FeedbackWorkbenchCommandResult>("apply_feedback_workbench_command", {
        request: { ...protocol, commandVersion: 1, command: commandValue },
      });
      const previewRegionId = feedbackPreviewRegionAfterCommand(
        commandValue,
        selectedRegionId,
        result.project.document?.topology.regions.map((region) => region.id) ?? [],
      );
      const previewAllRegions = commandValue.type === "set_edge_detail" && !commandValue.intent.targetRegion;
      setFeedbackPreviewAllRegions(previewAllRegions);
      if (!previewAllRegions && previewRegionId !== selectedRegionId) setSelectedRegionId(previewRegionId);
      const processingEdgeCommand = processingOpen && commandValue.type === "set_edge_detail";
      const nextFeedbackView = processingEdgeCommand ? feedbackView : feedbackViewAfterCommand(commandValue, feedbackView);
      if (nextFeedbackView !== feedbackView) setFeedbackView(nextFeedbackView);
      // The explicit Processing preview starts before React effects run. Keep
      // the imperative revision source synchronized with the command result so
      // a supersession retry cannot fall back to the preceding revision.
      projectRef.current = result.project;
      setProject(result.project);
      if (commandValue.type === "set_edge_detail") setEdgeDetailError(null);
      setFeedbackLastCommandResult(`${commandValue.type}:${result.committedIdentity}:${result.status}`);
      setFeedbackSelectedOperationId((current) => selectedOperationAfterCommand(commandValue, current, result.committedIdentity));
      const dependency = visibleMapDependency(nextFeedbackView);
      if (!processingEdgeCommand && dependency) {
        setMapViewState(dependency);
        mapViewRef.current = dependency;
      }
      if (commandValue.type === "set_edge_detail" && previewRegionId) {
        const revision = result.project.document?.documentRevision;
        if (revision !== undefined) {
          if (processingEdgeCommand) await requestPreview(undefined, undefined, interactivePreviewProfile, revision, false, false, processingRequestedMap);
          else await requestFeedbackTile(revision, previewRegionId, previewAllRegions, nextFeedbackView);
        }
      }
    } catch (reason) {
      const commandFailure = failure(reason);
      setFeedbackError(commandFailure);
      if (commandValue.type === "set_edge_detail") setEdgeDetailError(commandFailure);
      setProblem(commandFailure);
      setFeedbackLastCommandResult(`${commandValue.type}:failed:${commandFailure.code}`);
    } finally {
      setFeedbackCommandBusy(false);
    }
  }

  function requestFeedbackVisibleView(requestedView = feedbackView) {
    const dependency = visibleMapDependency(requestedView);
    const revision = project?.document?.documentRevision;
    if (!dependency || revision === undefined) return;
    const inspection = edgeDetailInspectionForView(requestedView);
    if (inspection && !project?.document?.edgeDetail?.enabled) {
      setEdgeDetailError({
        code: "edge_detail_not_applied",
        message: "Enable Edge Detail before inspecting its generated fields.",
        recovery: "Choose a preset or edit a control; the live editor commits at the next editing boundary.",
      });
      return;
    }
    setMapViewState(dependency);
    mapViewRef.current = dependency;
    setFeedbackView(requestedView);
    const edgeAllRegions = !!inspection && !project?.document?.edgeDetail?.targetRegion;
    const previewRegionId = selectedRegionId
      ?? project?.document?.edgeDetail?.targetRegion
      ?? ((feedbackPreviewAllRegions || edgeAllRegions) ? project?.document?.topology.regions[0]?.id ?? null : null);
    if (previewRegionId) void requestFeedbackTile(revision, previewRegionId, feedbackPreviewAllRegions || edgeAllRegions, requestedView);
  }

  async function requestFeedbackTile(
    revision: number,
    regionId: string,
    allRegions = false,
    requestedView = feedbackView,
    requestedProfile: FeedbackPreviewProfile | "authoritative" = feedbackProfile,
  ) {
    const dependency = visibleMapDependency(requestedView);
    if (!native || !dependency) return;
    const edgeDetailInspection = edgeDetailInspectionForView(requestedView);
    const profile = requestedProfile === "preview1024" ? "refinement1024" : requestedProfile;
    const generation = ++previewDraftId.current;
    const startedAt = performance.now();
    previewPublishStartedAt.current = startedAt;
    const targetDimensions = previewProfileDimensions(
      profile,
      projectRef.current?.document?.renderSettings.outputSize,
    );
    const requestDescriptor: FeedbackPixelRequestIdentity = {
      revision, regionId, allRegions, view: requestedView, map: dependency, profile: requestedProfile,
      comparisonMode: feedbackComparisonMode, selectedOperationId: feedbackSelectedOperationId,
    };
    const requestIdentity = feedbackPixelRequestIdentity(requestDescriptor);
    setFeedbackExecution(null);
    setPreviewProgress({ requestId: generation, phase: "compiling", profile, requestedRevision: revision, requestedMap: dependency, startedAt, targetDimensions, feedbackRequestIdentity: requestIdentity, feedbackRegionId: regionId, feedbackView: requestedView, feedbackComparisonMode, feedbackSelectedOperationId });
    setFeedbackError(null);
    if (edgeDetailInspection) setEdgeDetailError(null);
    try {
      const next = await invoke<IntermediateAtlasProjection>("preview_stage_15_16_feedback", {
        request: { ...protocol, commandVersion: 1, revision, generation, regionId, allRegions, view: requestedView, profile, comparisonMode: feedbackComparisonMode, selectedOperationId: feedbackSelectedOperationId ?? undefined, edgeDetailInspection: edgeDetailInspection ?? undefined },
      });
      if (generation !== previewDraftId.current) return;
      if (!next.feedbackExecution || next.feedbackExecution.clientGeneration !== generation || !feedbackExecutionMatchesRequest(next.feedbackExecution, requestDescriptor)) {
        throw new Error("The native preview result did not identify the exact current feedback request.");
      }
      setArtifact(next);
      setFeedbackExecution(next.feedbackExecution);
      setFeedbackError(null);
      if (edgeDetailInspection) setEdgeDetailError(null);
      setProblem(null);
      if (next.project) setProject(next.project);
      setPreview(null);
      setPreviewProgress({ requestId: generation, phase: "received", profile, requestedRevision: revision, requestedMap: dependency, startedAt, elapsedMs: performance.now() - startedAt, dimensions: { width: next.width, height: next.height }, terminalOutcome: "published", feedbackRequestIdentity: next.feedbackExecution.requestIdentity, feedbackRegionId: regionId, feedbackView: requestedView, feedbackComparisonMode, feedbackSelectedOperationId });
      setPreviewClientTelemetry([`request_identity=${next.feedbackExecution.requestIdentity}`, `feedback_view=${next.feedbackExecution.view}`, `profile=${next.feedbackExecution.profile}`, `requested_revision=${next.feedbackExecution.revision}`, `requested_map=${next.feedbackExecution.requestedMap}`, `artifact_revision=${next.documentRevision}`, `request_outcome=${next.feedbackExecution.outcome}`, `cache_reused=${next.feedbackExecution.cacheReused}`]);
    } catch (reason) {
      // A superseded native job may settle after a newer request has already
      // published. It must not erase that exact execution evidence or surface a
      // stale cancellation beside the layer.
      if (!feedbackRequestIsCurrent(generation, previewDraftId.current)) return;
      const commandFailure = failure(reason);
      const superseded = commandFailure.code === "operation_cancelled";
      setFeedbackExecution(null);
      setFeedbackError(commandFailure);
      if (edgeDetailInspection) setEdgeDetailError(commandFailure);
      if (!superseded) setProblem(commandFailure);
      setPreviewProgress({ requestId: generation, phase: "failed", profile, requestedRevision: revision, requestedMap: dependency, startedAt, elapsedMs: performance.now() - startedAt, terminalOutcome: superseded ? "superseded" : "failed", feedbackRequestIdentity: requestIdentity, feedbackRegionId: regionId, feedbackView: requestedView, feedbackComparisonMode, feedbackSelectedOperationId });
      setPreviewClientTelemetry([`request_identity=${requestIdentity}`, `feedback_view=${requestedView}`, `requested_revision=${revision}`, `requested_map=${dependency}`, `request_outcome=${superseded ? "superseded" : "failed"}`, `problem=${commandFailure.code}`]);
    }
  }

  function selectFeedbackRegion(regionId: string) {
    const transition = selectFeedbackRegionWithoutPixelWork({ selectedRegionId, activeMap: mapView, publication: artifact }, regionId);
    setSelectedRegionId(transition.selectedRegionId);
    setFeedbackPreviewAllRegions(false);
  }

  async function createFeedbackSample() {
    if (!native || feedbackCommandBusy) return;
    setFeedbackCommandBusy(true);
    setFeedbackError(null);
    try {
      const next = await invoke<ProjectProjection>("create_stage_15_16_feedback_sample", { request: protocol });
      setProject(next);
      const regionId = next.document?.topology.regions[0]?.id ?? null;
      setSelectedRegionId(regionId);
      setFeedbackPreviewAllRegions(false);
      setFeedbackSelectedOperationId(next.feedbackAuthoring.records.find((record) => record.intent.kind === "operation")?.operationId ?? null);
      setFeedbackLastCommandResult("create_feedback_sample:executed");
      if (next.document && regionId) {
        setMapViewState("height");
        mapViewRef.current = "height";
      }
    } catch (reason) {
      const commandFailure = failure(reason);
      setFeedbackError(commandFailure);
      setProblem(commandFailure);
    } finally {
      setFeedbackCommandBusy(false);
    }
  }

  async function build() {
    if (!project || !primaryMaterial || activity !== "idle") return;
    if (!project.document) {
      setProblem({
        code: "trim_sheet_missing",
        message: "No trim sheet document exists yet.",
        recovery: "Import a Diffuse texture to create the source-to-sheet document, or open a legacy project and rebuild after confirming the preserved sources.",
      });
      return;
    }
    setActivity("compiling");
    setProblem(null);
    try {
      const current = project;
      const compiled = await invoke<IntermediateAtlasProjection>("preview_through_stage_14", {
        request: { ...protocol, revision: current.document!.documentRevision, profile: "draft512" },
      });
      previewDraftId.current += 1;
      setPreview(null);
      setArtifact(compiled);
      setSelectedRegionId((selected) => compiled.regions.some((region) => region.regionId === selected) ? selected : null);
      window.setTimeout(() => void requestPreview(undefined, undefined, interactivePreviewProfile, current.document!.documentRevision), 120);
    } catch (reason) {
      setProblem(failure(reason));
    } finally {
      setActivity("idle");
    }
  }

  async function regenerateSourceFrame(target: number) {
    if (!native || !project?.document || activity !== "idle") return;
    const count = Math.max(1, Math.min(256, Math.round(target)));
    setActivity("compiling");
    setProblem(null);
    try {
      const current = await invoke<ProjectProjection>("regenerate_source_frame_partition", {
        request: { ...protocol, targetRegionCount: count },
      });
      setProject(current);
      setArtifact(null);
      const compiled = await invoke<IntermediateAtlasProjection>("preview_through_stage_14", {
        request: { ...protocol, revision: current.document!.documentRevision, profile: "draft512" },
      });
      setArtifact(compiled);
      setSelectedRegionId(null);
      window.setTimeout(() => void requestPreview(undefined, undefined, interactivePreviewProfile, current.document!.documentRevision), 120);
    } catch (reason) {
      setProblem(failure(reason));
    } finally {
      setActivity("idle");
    }
  }

  async function previewPartitionCandidate(recipe: PartitionRecipe) {
    if (!native || !project?.document || activity !== "idle") return;
    setActivity("compiling"); setProblem(null);
    try {
      const compiled = await invoke<IntermediateAtlasProjection>("preview_through_stage_14", {
        request: { ...protocol, revision: project.document.documentRevision, profile: "draft512", candidateRecipe: recipe },
      });
      setArtifact(compiled); setCandidatePreviewing(true); setCandidatePreviewHash(partitionRecipeFingerprint(recipe)); setCandidatePreviewRecipe(recipe); setSelectedRegionId(null);
    } catch (reason) { setProblem(failure(reason)); }
    finally { setActivity("idle"); }
  }

  async function acceptPartitionCandidate(recipe: PartitionRecipe) {
    if (!project?.document || activity !== "idle" || candidatePreviewHash !== partitionRecipeFingerprint(recipe)) return;
    const acceptedFingerprint = partitionRecipeFingerprint(recipe);
    setActivity("compiling"); setProblem(null);
    try {
      const current = await applyCommand({ type: "accept_source_frame_partition", recipe });
      // Keep the settled fingerprint after acceptance. Clearing it caused the auto-preview
      // effect to immediately recreate the same read-only candidate and lock direct editing.
      setProject(current); setCandidatePreviewing(false); setCandidatePreviewHash(acceptedFingerprint); setCandidatePreviewRecipe(null); setArtifact(null);
      const compiled = await invoke<IntermediateAtlasProjection>("preview_through_stage_14", {
        request: { ...protocol, revision: current.document!.documentRevision, profile: "draft512" },
      });
      setArtifact(compiled); setSelectedRegionId(null);
    } catch (reason) { setProblem(failure(reason)); }
    finally { setActivity("idle"); }
  }

  async function editSourceFrameLayout(commandValue: TrimSheetDocumentCommand): Promise<ProjectProjection | null> {
    if (!project?.document || activity !== "idle" || candidatePreviewing) return null;
    setActivity("editing"); setProblem(null); undoneRegionPatchConversion.current = null;
    try {
      const pixelRegionId = commandValue.type === "set_region_content"
        || commandValue.type === "set_region_behavior"
        || commandValue.type === "set_region_radial"
        || commandValue.type === "set_region_projection"
        ? commandValue.regionId : null;
      const topologyChanged = commandValue.type === "resize_source_frame_region"
        || commandValue.type === "draw_source_frame_region"
        || commandValue.type === "split_source_frame_region"
        || commandValue.type === "merge_source_frame_regions"
        || commandValue.type === "apply_authored_layout_preset";
      if (pixelRegionId) dirtyPreviewRegion.current = pixelRegionId;
      const current = await applyCommand(commandValue);
      setProject(current);
      // Region content and mapping behavior change pixels, not topology. Keep the last published
      // artifact at its truthful revision until Stage 14 publishes the requested mapping.
      if (!pixelRegionId) setArtifact((prior) => retopologizeArtifact(prior, current));
      setSelectedRegionId((selected) => current.document!.topology.regions.some((region) => region.id === selected) ? selected : null);
      if (pixelRegionId || topologyChanged) {
        dirtyPreviewRegion.current = null;
        pendingAutomaticPreviewKey.current = automaticPreviewKey(current.document!.documentRevision, interactivePreviewProfile, mapView);
        // Region identity without a projection is not a transient crop request. Compile the
        // persisted binding revision; the immediate overlay above covers publication latency.
        void requestPreview(undefined, undefined, interactivePreviewProfile, current.document!.documentRevision, false, true);
      }
      return current;
    } catch (reason) { setProblem(failure(reason)); return null; }
    finally { setActivity("idle"); }
  }

  function discardPartitionCandidate() {
    if (!candidatePreviewing) return;
    // Discard settles the current recipe too; it should not regenerate until a control changes.
    setCandidatePreviewing(false); setCandidatePreviewHash(partitionRecipeFingerprint(candidateRecipe)); setCandidatePreviewRecipe(null); setArtifact(null); void build();
  }

  async function createDocumentAndCompile(seed: ProjectProjection, _materialId: string) {
    setActivity("importing");
    setProblem(null);
    try {
      let current = seed;
      if (!current.document) {
        current = await invoke<ProjectProjection>("create_source_frame_document", { request: protocol });
      }
      previewDraftId.current += 1;
      setPreview(null);
      suppressAutomaticPreviewRevision.current = current.document!.documentRevision;
      setProject(current);
      const requestedMaps = requestedMaterialMapsForView(mapView);
      const compiled = await invoke<IntermediateAtlasProjection>("preview_through_stage_14", {
        request: { ...protocol, revision: current.document!.documentRevision, profile: "draft512", requestedMaps },
      });
      setArtifact(compiled);
      setSelectedRegionId(null);
      window.setTimeout(() => void requestPreview(undefined, undefined, interactivePreviewProfile, current.document!.documentRevision), 120);
    } catch (reason) {
      setProblem(failure(reason));
    } finally {
      setActivity("idle");
    }
  }

  async function setSelectedCrop(bounds: NormalizedBounds) {
    if (!selectedRegionId || !selectedCrop || activity !== "idle") return;
    const projection: CropProjection = {
      ...selectedCrop,
      bounds,
      focus: { x: bounds.x + bounds.width * 0.5, y: bounds.y + bounds.height * 0.5 },
    };
    setProblem(null);
    try {
      dirtyPreviewRegion.current = selectedRegionId;
      const next = await applyCommand({ type: "set_region_projection", regionId: selectedRegionId, projection });
      setProject(next);
    } catch (reason) {
      dirtyPreviewRegion.current = null;
      setProblem(failure(reason));
    } finally {
    }
  }

  async function requestPreview(regionId?: string, projection?: CropProjection, profile: PreviewProfile = "draft512", revision?: number, scheduleRefinement = true, automatic = false, requestedMapOverride?: CompiledMapView, supersessionRetry = 0): Promise<void> {
    const requestedRevision = revision ?? project?.document?.documentRevision;
    if (!native || requestedRevision === undefined) return;
    const requestedMapView = requestedMapOverride ?? mapViewRef.current;
    const requestedMaps = processingOpen
      ? processingPreviewMaterialMaps
      : requestedMaterialMapsForView(requestedMapView);
    const automaticKey = automatic
      ? automaticPreviewKey(requestedRevision, profile, processingOpen ? "materialSet" : requestedMapView)
      : null;
    const viewIntent: "completeDraft512" | "completeRefinement1024" | "exactViewport" | "exactSelectedRegion" | undefined = profile === "draft512"
      ? "completeDraft512"
      : profile === "refinement1024"
        ? "completeRefinement1024"
        : profile === "authoritative" && regionId
          ? "exactSelectedRegion"
          : undefined;
    const viewportRect = undefined;
    const previewRegionId = viewIntent === "exactSelectedRegion" ? regionId : undefined;
    const draftId = ++previewDraftId.current;
    const targetDimensions = previewProfileDimensions(profile, project?.document?.renderSettings.outputSize);
    setProblem(null);
    previewPublishStartedAt.current = performance.now();
    setPreviewElapsedMs(0);
    setPreviewProgress({ requestId: draftId, phase: "compiling", profile, requestedRevision, requestedMap: requestedMapView, startedAt: previewPublishStartedAt.current, targetDimensions });
    try {
      const next = await invoke<IntermediateAtlasProjection>("preview_through_stage_14", {
        request: {
          ...protocol,
          revision: requestedRevision,
          regionId: previewRegionId,
          transientProjection: projection,
          draftId,
          inputHash: JSON.stringify({ revision: requestedRevision, regionId, projection, requestedMaps }),
          profile,
          viewIntent,
          requestedMaps,
          viewportRect,
        },
      });
      if (next.project) {
        setProject(next.project);
      }
      setPreviewClientTelemetry([`profile=${profile}`, `requested_revision=${requestedRevision}`, `requested_map=${requestedMapView}`, `artifact_revision=${next.documentRevision}`, `artifact_dimensions=${next.width}x${next.height}`, `request_outcome=published`, `ipc_round_trip_ms=${Math.round(performance.now() - (previewPublishStartedAt.current ?? performance.now()))}`]);
      if (automaticKey && draftId !== previewDraftId.current && pendingAutomaticPreviewKey.current === automaticKey) {
        pendingAutomaticPreviewKey.current = null;
      }
      if (draftId === previewDraftId.current) {
        if (processingOpen && requestedMapOverride) {
          setMapViewState(requestedMapOverride);
          mapViewRef.current = requestedMapOverride;
        }
        setPreviewProgress({ requestId: draftId, phase: "received", profile, requestedRevision, requestedMap: requestedMapView, startedAt: previewPublishStartedAt.current, elapsedMs: performance.now() - previewPublishStartedAt.current, dimensions: { width: next.width, height: next.height }, terminalOutcome: "published" });
        setArtifact(next);
        setPreview(null);
        setProblem(null);
        if (automaticKey) {
          lastAutomaticPreviewKey.current = automaticKey;
          if (pendingAutomaticPreviewKey.current === automaticKey) pendingAutomaticPreviewKey.current = null;
        }
        if (profile === "draft512" && scheduleRefinement) {
          window.setTimeout(() => {
            if (draftId === previewDraftId.current && requestedRevision === next.documentRevision) {
              void requestPreview(regionId, projection, interactivePreviewProfile, requestedRevision);
            }
          }, 120);
        }
      }
    } catch (reason) {
      const failureReason = failure(reason);
      const elapsedMs = performance.now() - (previewPublishStartedAt.current ?? performance.now());
      if (automaticKey && pendingAutomaticPreviewKey.current === automaticKey) pendingAutomaticPreviewKey.current = null;
      if (failureReason.code !== "operation_cancelled") {
        setPreviewClientTelemetry([`profile=${profile}`, `requested_revision=${requestedRevision}`, `requested_map=${requestedMapView}`, `request_outcome=failed`, `problem=${failureReason.code}`]);
        setProblem(failureReason);
        if (draftId === previewDraftId.current) setPreviewProgress({ requestId: draftId, phase: "failed", profile, requestedRevision, requestedMap: requestedMapView, startedAt: previewPublishStartedAt.current ?? performance.now(), elapsedMs, terminalOutcome: "failed" });
      } else if (draftId === previewDraftId.current) {
        // A superseded native job is terminal for this request. Do not leave the footer
        // counting forever as though CPU/GPU work were still running, and immediately
        // ask for the settled latest revision instead.
        setPreviewClientTelemetry([`profile=${profile}`, `requested_revision=${requestedRevision}`, `requested_map=${requestedMapView}`, `request_outcome=superseded`]);
        setPreviewProgress({ requestId: draftId, phase: "failed", profile, requestedRevision, requestedMap: requestedMapView, startedAt: previewPublishStartedAt.current ?? performance.now(), elapsedMs, terminalOutcome: "superseded" });
        const observedRevision = projectRef.current?.document?.documentRevision;
        const latestRevision = observedRevision === undefined
          ? requestedRevision
          : Math.max(requestedRevision, observedRevision);
        // Native revision handoff can supersede either the explicit Apply
        // request or its automatic equivalent. Await one replacement request
        // here so publication cannot depend on a fire-and-forget timer.
        if (supersessionRetry === 0) {
          const latestKey = automaticPreviewKey(latestRevision, profile, processingOpen ? "materialSet" : requestedMapView);
          pendingAutomaticPreviewKey.current = latestKey;
          // Let already-queued revision/map effects enter the native latest-wins
          // service first. The one replacement below then owns the settled burst
          // instead of being immediately superseded by that queued work.
          await new Promise<void>((resolve) => window.setTimeout(resolve, 180));
          return requestPreview(regionId, projection, profile, latestRevision, scheduleRefinement, true, requestedMapView, 1);
        }
      }
    }
  }

  async function renderFullResolutionPreview() {
    const revision = project?.document?.documentRevision;
    if (!native || revision === undefined || activity !== "idle") return;
    setActivity("compiling");
    try {
      await requestPreview(undefined, undefined, processingOpen ? interactivePreviewProfile : "authoritative", revision, false, false, processingOpen ? processingRequestedMap : mapViewRef.current);
    }
    finally { setActivity("idle"); }
  }

  function previewSelectedCrop(bounds: NormalizedBounds) {
    const revision = project?.document?.documentRevision;
    if (!selectedRegionId || !selectedCrop || revision === undefined) return;
    const projection: CropProjection = {
      ...selectedCrop,
      bounds,
      focus: { x: bounds.x + bounds.width * 0.5, y: bounds.y + bounds.height * 0.5 },
    };
    sourceFramePreviewController.current.enqueue({ regionId: selectedRegionId, projection, revision });
  }

  function exitRegionPatchEdit() {
    setDraftPatchPreview(null);
    setPatchTool(null);
    setRegionPatchEditId(null);
  }

  async function history(redo: boolean) {
    if (documentHistoryBusy.current || activity !== "idle") return;
    documentHistoryBusy.current = true;
    setActivity("editing");
    try {
      const regionPatchId = lastRegionPatchId.current;
      const regionPatchExists = !!regionPatchId && !!project?.patches.some((patch) => patch.id === regionPatchId);
      if (regionPatchId && !regionPatchExists) {
        patchGeometryEditDepth.current.delete(regionPatchId);
        patchGeometryRedoDepth.current.delete(regionPatchId);
        lastRegionPatchId.current = null;
      }
      if (!redo && regionPatchId && regionPatchExists && (patchGeometryEditDepth.current.get(regionPatchId) ?? 0) > 0) {
        const patchNext = await invoke<ProjectProjection>("undo_patch_command", { request: protocol });
        patchGeometryEditDepth.current.set(regionPatchId, Math.max(0, (patchGeometryEditDepth.current.get(regionPatchId) ?? 1) - 1));
        patchGeometryRedoDepth.current.set(regionPatchId, (patchGeometryRedoDepth.current.get(regionPatchId) ?? 0) + 1);
        setProject(patchNext); setPreview(null); setArtifact(null); setProblem(null);
        return;
      }
      if (redo && regionPatchId && regionPatchExists && (patchGeometryRedoDepth.current.get(regionPatchId) ?? 0) > 0) {
        const patchNext = await invoke<ProjectProjection>("redo_patch_command", { request: protocol });
        patchGeometryRedoDepth.current.set(regionPatchId, Math.max(0, (patchGeometryRedoDepth.current.get(regionPatchId) ?? 1) - 1));
        patchGeometryEditDepth.current.set(regionPatchId, (patchGeometryEditDepth.current.get(regionPatchId) ?? 0) + 1);
        setProject(patchNext); setPreview(null); setArtifact(null); setProblem(null);
        return;
      }
      const restoringPatchId = redo && project?.canRedoPatch && project.canRedoDocument ? undoneRegionPatchConversion.current : null;
      if (redo && undoneRegionPatchConversion.current && !restoringPatchId) undoneRegionPatchConversion.current = null;
      if (restoringPatchId) await invoke<ProjectProjection>("redo_patch_command", { request: protocol });
      let next = await invoke<ProjectProjection>(redo ? "redo_document_command" : "undo_document_command", { request: protocol });
      if (!redo && regionPatchId && regionPatchExists && patchFallbackContent.current.has(regionPatchId)
        && !Object.values(next.document?.regionBindings ?? {}).some((binding) => binding.content.type === "patch" && binding.content.id === regionPatchId)) {
        next = await invoke<ProjectProjection>("undo_patch_command", { request: protocol });
        undoneRegionPatchConversion.current = regionPatchId;
        lastRegionPatchId.current = null;
        setActivePatchId(null); setRegionPatchEditId(null); setDraftPatchPreview(null);
      } else if (redo && restoringPatchId) {
        lastRegionPatchId.current = restoringPatchId;
        undoneRegionPatchConversion.current = null;
      }
      setProject(next);
      setPreview(null);
      setArtifact((prior) => retopologizeArtifact(prior, next));
      setSelectedRegionId((selected) => next.document?.topology.regions.some((region) => region.id === selected) ? selected : null);
      setProblem(null);
    } catch (reason) {
      setProblem(failure(reason));
    } finally {
      documentHistoryBusy.current = false;
      setActivity("idle");
    }
  }

  async function saveProject(): Promise<boolean> {
    if (!project) return false;
    if (project.isDraft) return saveProjectAs();
    setActivity("saving");
    try {
      setProject(await invoke<ProjectProjection>("save_project", { request: protocol }));
      setProblem(null);
      void refreshRecents();
      return true;
    } catch (reason) {
      setProblem(failure(reason));
      return false;
    } finally {
      setActivity("idle");
    }
  }

  async function saveProjectAs(): Promise<boolean> {
    const path = await save({
      title: "Save Hot Trimmer project",
      defaultPath: `${project?.name || "Untitled"}.hottrimmer`,
      filters: [{ name: "Hot Trimmer", extensions: ["hottrimmer"] }],
    });
    if (!path) return false;
    setActivity("saving");
    try {
      const next = await invoke<ProjectProjection>("save_project_as", { request: { ...protocol, path } });
      setProject(next);
      setProblem(null);
      void refreshRecents();
      return true;
    } catch (reason) {
      setProblem(failure(reason));
      return false;
    } finally {
      setActivity("idle");
    }
  }

  async function exportMaterialMaps(): Promise<boolean> {
    if (!native || !project?.document || activity !== "idle") return false;
    const path = await save({
      title: "Export material maps",
      defaultPath: `${project.name || "HotTrimmer"}.hottrim`,
      filters: [{ name: "Hot Trimmer Package", extensions: ["hottrim"] }],
    });
    if (!path) return false;
    setActivity("exporting");
    setExportProgress(null);
    setProblem(null);
    try {
      let current = project;
      const normalPolicy = current.document?.renderSettings.channels.normal;
      if (normalPolicy?.enabled && normalPolicy.bitDepth !== "eight") {
        current = await applyCommand({
          type: "set_channel_render_policy",
          channel: "normal",
          policy: { enabled: true, bitDepth: "eight" },
        });
        setProject(current);
        setArtifact(null);
      }
      const revision = current.document!.documentRevision;
      const requestedMaps = requestedMaterialMapsForExport(current);
      const exported = await invoke<NativeStage14ExportProjection>("export_stage_14_material_maps", {
        request: { ...protocol, revision, path, requestedMaps },
      });
      if (exported.project) {
        current = exported.project;
        setProject(current);
        setArtifact(null);
      }
      setPreviewClientTelemetry([
        `export_path=${exported.path}`,
        `export_revision=${exported.revision}`,
        `export_bytes=${exported.bytesWritten}`,
        `export_outputs=${exported.outputs.map((output) => `${output.fileName}:${output.map}:${output.width}x${output.height}:${output.bytes}:${output.checksum}`).join(",")}`,
        ...exported.telemetry,
      ]);
      setProblem(null);
      return true;
    } catch (reason) {
      setProblem(failure(reason));
      return false;
    } finally {
      setExportProgress(null);
      setActivity("idle");
    }
  }

  async function closeToDraft() {
    // Drop browser-held data URLs and decoded preview surfaces before the native project/cache
    // boundary. WebView allocators may retain committed pages, but no old project image remains reachable.
    previewDraftId.current += 1;
    setArtifact(null);
    setPreview(null);
    setPreparedPatchPreview(null);
    setPreparedPatchPreviews({});
    setPreviewProgress(null);
    setPreviewClientTelemetry([]);
    try {
      await invoke("close_project", { request: { ...protocol, save: false } });
    } catch {
      /* Closing an unsaved draft may have no durable work to release. */
    }
    await createDraft();
  }

  async function commitProjectName() {
    const name = draftName.trim();
    setRenaming(false);
    if (!name || name === project?.name) return;
    try {
      setProject(await invoke<ProjectProjection>("rename_project", { request: { ...protocol, name } }));
    } catch (reason) {
      setProblem(failure(reason));
    }
  }

  async function patchCommand(command: PatchCommand, coalescingGroup?: number) {
    undoneRegionPatchConversion.current = null;
    const next = await invoke<ProjectProjection>("apply_patch_command", {
      request: { ...protocol, command, coalescingGroup },
    });
    setProject(next);
    setProblem(null);
    return next;
  }

  function nextPatchName(sourceId: string) {
    const used = new Set((project?.patches ?? [])
      .filter((patch) => patch.sourceId === sourceId)
      .flatMap((patch) => /^Patch (\d+)$/.exec(patch.name)?.[1] ?? [])
      .map(Number));
    let index = 1;
    while (used.has(index)) index += 1;
    return `Patch ${index}`;
  }

  async function createPatch(geometry: PatchGeometry, fourPoint: boolean) {
    if (!selectedSource) return;
    const id = crypto.randomUUID();
    try {
      await patchCommand({
        type: "create",
        patch: {
          id, sourceId: selectedSource.id, name: nextPatchName(selectedSource.id), enabled: true, geometry,
          properties: { repeatMode: "unique", trimCap: false, paddingPx: 4, bleedPx: 8, mapParticipation: "all" },
          rectification: { scale: 1 },
        },
      });
      setActivePatchId(id);
      setPatchTool(null);
    } catch (reason) { setProblem(failure(reason)); }
  }

  async function editSelectedRegionAsPatch(authoredBounds?: NormalizedBounds) {
    const document = project?.document;
    const regionId = selectedRegionId;
    if (!document || !regionId || activity !== "idle") return;
    const binding = document.regionBindings[regionId];
    const definition = document.topology.regions.find((region) => region.id === regionId);
    if (!binding || !definition) return;
    const content = binding.content;
    if (content.type === "patch") {
      const patch = project?.patches.find((candidate) => candidate.id === content.id);
      const owner = patch && project?.materialSources.find((set) => set.registeredChannels?.channels.some((source) => source.id === patch.sourceId));
      if (patch && owner) {
        setSelectedSourceSetId(owner.id); selectSourceChannel("base_color"); setActivePatchId(patch.id);
        setRegionPatchEditId(regionId); setWorkspaceMode("authoring"); setAuthoringPanes((current) => new Set([...current, "workbench"])); setPatchTool(null);
      }
      return;
    }
    const sourceSetId = content.type === "material_source" ? content.id : document.primaryMaterial;
    const owner = project?.materialSources.find((set) => set.id === sourceSetId);
    const base = owner?.registeredChannels?.channels.find((source) => source.channel === "base_color");
    if (!owner || !base) {
      setProblem({ code: "source_missing", message: `The source for ${definition.displayName} is unavailable.`, recovery: "Assign a valid Diffuse source before converting this region to a patch." });
      return;
    }
    const bounds = authoredBounds ?? (binding.mapping.projection.type === "crop"
      ? binding.mapping.projection.bounds
      : document.sourceFrame?.sourceSetId === sourceSetId && document.logicalGrid && definition.gridRect
        ? sourceFrameGridBounds(document.sourceFrame.bounds, document.logicalGrid, definition.gridRect)
        : { x: 0, y: 0, width: 1, height: 1 });
    const patchId = crypto.randomUUID();
    patchFallbackContent.current.set(patchId, content);
    lastRegionPatchId.current = patchId;
    patchGeometryEditDepth.current.set(patchId, 0);
    patchGeometryRedoDepth.current.set(patchId, 0);
    try {
      await patchCommand({
        type: "create",
        patch: {
          id: patchId, sourceId: base.id, name: nextPatchName(base.id), enabled: true,
          geometry: { corners: [
            { x: bounds.x, y: bounds.y }, { x: bounds.x + bounds.width, y: bounds.y },
            { x: bounds.x + bounds.width, y: bounds.y + bounds.height }, { x: bounds.x, y: bounds.y + bounds.height },
          ] },
          properties: { repeatMode: "unique", trimCap: false, paddingPx: 4, bleedPx: 8, mapParticipation: "all" },
          rectification: { scale: 1 },
        },
      });
      await assignPatchToRegion(patchId, regionId);
      setSelectedSourceSetId(owner.id); selectSourceChannel("base_color"); setActivePatchId(patchId);
      setRegionPatchEditId(regionId); setWorkspaceMode("authoring"); setAuthoringPanes((current) => new Set([...current, "workbench"])); setPatchTool(null);
    } catch (reason) { patchFallbackContent.current.delete(patchId); setProblem(failure(reason)); }
  }

  async function assignPatchToRegion(patchId: string, regionId: string) {
    if (!project?.document || activity !== "idle") return;
    setProblem(null);
    try {
      dirtyPreviewRegion.current = regionId;
      const next = await applyCommand({ type: "set_region_content", regionId, content: { type: "patch", id: patchId } });
      setProject(next);
      setSelectedRegionId(regionId);
    } catch (reason) {
      dirtyPreviewRegion.current = null;
      setProblem(failure(reason));
    }
  }

  async function assignContentToRegion(regionId: string, content: ContentReference) {
    if (!project?.document || activity !== "idle") return;
    dirtyPreviewRegion.current = regionId;
    try {
      const next = await applyCommand({ type: "set_region_content", regionId, content });
      setProject(next);
    } catch (reason) {
      dirtyPreviewRegion.current = null;
      setProblem(failure(reason));
    }
  }

  async function setRegionBehavior(regionId: string, behavior: RegionBehavior) {
    if (!project?.document || activity !== "idle") return;
    try { await command({ type: "set_region_behavior", regionId, behavior }); }
    catch (reason) { setProblem(failure(reason)); }
  }

  async function setPrimaryMaterialExplicit(sourceSetId: string) {
    if (!project?.document || project.document.primaryMaterial === sourceSetId) return;
    const affected = Object.values(project.document.regionBindings)
      .filter((binding) => binding.content.type === "inherit_primary_material").length;
    if (!window.confirm(`Rebase primary material to the selected source?\n\n${affected} inherited region binding(s) will resolve through it. Explicit whole-source and patch bindings will not change. The SourceFrame remains owned by ${project.document.sourceFrame?.sourceSetId ?? "its persisted source"}.`)) return;
    try { await command({ type: "set_primary_material", materialId: sourceSetId }); }
    catch (reason) { setProblem(failure(reason)); }
  }

  function replaceBaseWithPreflight(sourceSetId: string) {
    if (!project?.document) return;
    const set = project.materialSources.find((source) => source.id === sourceSetId);
    if (!set) return;
    const sourceIds = new Set(set.registeredChannels?.channels.map((channel) => channel.id) ?? []);
    const ownedPatchIds = new Set(project.patches.filter((patch) => sourceIds.has(patch.sourceId)).map((patch) => patch.id));
    const affectedRegions = Object.values(project.document.regionBindings).filter((binding) =>
      (binding.content.type === "material_source" && binding.content.id === sourceSetId)
      || (binding.content.type === "patch" && ownedPatchIds.has(binding.content.id))).length;
    const ownsFrame = project.document.sourceFrame?.sourceSetId === sourceSetId;
    if (!window.confirm(`Replace Diffuse texture for ${set.name}?\n\n${ownedPatchIds.size} owned patch(es) and ${affectedRegions} explicit region binding(s) will keep their stable IDs.${ownsFrame ? " The SourceFrame will be revalidated against the replacement dimensions." : ""} Incompatible companion-map dimensions will be rejected before the project is changed.`)) return;
    void chooseImages("base_color", sourceSetId);
  }

  async function removeSourceSet(sourceSetId: string) {
    if (!project?.document) return;
    const dependentRegions = Object.values(project.document.regionBindings).filter((binding) => {
      const content = binding.content;
      return (content.type === "material_source" && content.id === sourceSetId)
        || (content.type === "patch" && project.patches.some((patch) => patch.id === content.id
          && project.materialSources.find((set) => set.id === sourceSetId)?.registeredChannels?.channels.some((channel) => channel.id === patch.sourceId)));
    });
    const set = project.materialSources.find((source) => source.id === sourceSetId);
    if (!set) return;
    if (project.document.sourceFrame?.sourceSetId === sourceSetId || project.document.primaryMaterial === sourceSetId || dependentRegions.length) {
      setProblem({ code: "source_in_use", message: `Source is referenced by the layout${dependentRegions.length ? ` (${dependentRegions.length} region binding(s))` : ""}.`, recovery: "Assign an explicit fallback source or patch to every dependent region, then remove it." });
      return;
    }
    if (project.patches.some((patch) => set.registeredChannels?.channels.some((channel) => channel.id === patch.sourceId))) {
      setProblem({ code: "source_in_use", message: "Source owns authored patches.", recovery: "Remove or reassign its patches explicitly before removing the source." });
      return;
    }
    if (!window.confirm(`Remove ${set.name}? This removes its registered maps as one requested operation.`)) return;
    try {
      const next = await invoke<ProjectProjection>("remove_source_set", { request: { ...protocol, sourceSetId } });
      setProject(next);
      setSelectedSourceSetId(next.document?.primaryMaterial ?? next.materialSources[0]?.id ?? "");
    } catch (reason) { setProblem(failure(reason)); }
  }

  async function deletePatch(patchId: string) {
    const dependent = (project?.document && Object.values(project.document.regionBindings)
      .filter((binding) => binding.content.type === "patch" && binding.content.id === patchId)
      .map((binding) => binding.regionId)) ?? [];
    for (const regionId of dependent) {
      const fallback = patchFallbackContent.current.get(patchId) ?? { type: "inherit_primary_material" };
      await command({ type: "set_region_content", regionId, content: fallback });
    }
    setDraftPatchPreview((draft) => draft?.patchId === patchId ? null : draft);
    setActivePatchId((active) => active === patchId ? null : active);
    setPatchTool(null);
    try {
      await patchCommand({ type: "delete", patchId });
      patchFallbackContent.current.delete(patchId);
    } catch (reason) {
      setActivePatchId(patchId);
      setProblem(failure(reason));
    }
  }

  async function renamePatch(patchId: string) {
    const patch = project?.patches.find((value) => value.id === patchId);
    const name = patch && window.prompt("Rename patch", patch.name)?.trim();
    if (!name || name === patch?.name) return;
    try { await patchCommand({ type: "rename", patchId, name }); }
    catch (reason) { setProblem(failure(reason)); }
  }

  async function renameSourceSet(sourceSetId: string) {
    const source = project?.materialSources.find((value) => value.id === sourceSetId);
    const name = source && window.prompt("Rename source", source.name)?.trim();
    if (!name || name === source?.name) return;
    try {
      const next = await invoke<ProjectProjection>("rename_source_set", { request: { ...protocol, sourceSetId, name } });
      setProject(next); setProblem(null);
    } catch (reason) { setProblem(failure(reason)); }
  }

  async function replacePatchGeometry(patchId: string, geometry: PatchGeometry) {
    try {
      undoneRegionPatchConversion.current = null;
      const next = await invoke<ProjectProjection>("apply_patch_command", {
        request: { ...protocol, command: { type: "replace_geometry", patchId, geometry }, coalescingGroup: Date.now() },
      });
      const assignedToRegion = Object.values(next.document?.regionBindings ?? {}).some((binding) =>
        binding.content.type === "patch" && binding.content.id === patchId);
      if (assignedToRegion && next.document) {
        const revision = next.document.documentRevision;
        // An assigned patch is part of the persisted Stage 14 appearance. Keep the
        // previous artifact visible until the selected preview size publishes, so radial
        // or patch edits do not flash a lower-quality square intermediate.
        pendingAutomaticPreviewKey.current = automaticPreviewKey(revision, interactivePreviewProfile, mapView);
        dirtyPreviewRegion.current = null;
        // Set the suppression token before publishing the new projection. This closes the
        // race where React's revision effect could start a second native job first.
        setProject(next);
        setProblem(null);
        void requestPreview(undefined, undefined, interactivePreviewProfile, revision, false, true);
      } else {
        setProject(next);
        setProblem(null);
      }
      if (patchFallbackContent.current.has(patchId)) {
        patchGeometryEditDepth.current.set(patchId, (patchGeometryEditDepth.current.get(patchId) ?? 0) + 1);
        patchGeometryRedoDepth.current.set(patchId, 0);
      }
    }
    catch (reason) { setProblem(failure(reason)); }
    finally { setDraftPatchPreview((draft) => draft?.patchId === patchId ? null : draft); }
  }

  async function setResolution(size: number) {
    if (!project?.document) return;
    try {
      await command({ type: "set_output_resolution", outputSize: { width: size, height: size } });
    } catch (reason) {
      setProblem(failure(reason));
    }
  }

  async function setAtlasPadding(paddingPx: number) {
    if (!project?.document) return;
    try {
      await command({ type: "set_atlas_padding", paddingPx: Math.max(0, Math.min(4096, Math.round(paddingPx))) });
    } catch (reason) {
      setProblem(failure(reason));
    }
  }

  async function setRegionCrop(regionId: string, bounds: NormalizedBounds) {
    dirtyPreviewRegion.current = regionId;
    try {
      await command({
        type: "set_region_projection",
        regionId,
        projection: { type: "crop", bounds, focus: { x: bounds.x + bounds.width * 0.5, y: bounds.y + bounds.height * 0.5 } },
      });
    } catch (reason) {
      dirtyPreviewRegion.current = null;
      setProblem(failure(reason));
    }
  }

  async function setSourceFrame(bounds: NormalizedBounds) {
    try { await command({ type: "set_source_frame", bounds }); }
    catch (reason) { setProblem(failure(reason)); }
  }

  async function detachSourceCell(regionId: string) {
    try { await command({ type: "detach_source_cell", regionId }); }
    catch (reason) { setProblem(failure(reason)); }
  }

  async function resetSourceCell(regionId: string) {
    try { await command({ type: "reset_source_cell", regionId }); }
    catch (reason) { setProblem(failure(reason)); }
  }

  async function setRegionRadial(regionId: string, radial: NonNullable<RegionMapping["radial"]>) {
    const commitId = ++radialCommitId.current;
    dirtyPreviewRegion.current = regionId;
    setActivity("editing");
    try {
      const next = await applyCommand({ type: "set_region_radial", regionId, radial });
      // Numeric inputs can commit faster than native document commands settle.
      // Only the newest radial intent may update the client or publish pixels.
      if (commitId !== radialCommitId.current) return;
      const revision = next.document!.documentRevision;
      // Source-side radial gestures bypass the Layout command helper, so publish their
      // persisted revision explicitly. Keep the previous artifact visible until the
      // selected preview size is ready rather than flashing a lower-quality square pass.
      pendingAutomaticPreviewKey.current = automaticPreviewKey(revision, interactivePreviewProfile, mapView);
      dirtyPreviewRegion.current = null;
      projectRef.current = next;
      setProject(next);
      await requestPreview(undefined, undefined, interactivePreviewProfile, revision, false, true);
    }
    catch (reason) { dirtyPreviewRegion.current = null; setProblem(failure(reason)); }
    finally { if (commitId === radialCommitId.current) setActivity("idle"); }
  }

  function chooseSource(sourceSetId: string, channel: SourceChannel) {
    const selectedRegionSourceSetId = sourceSetIdForRegion({
      content: selectedBinding?.content,
      primarySourceSetId: project?.document?.primaryMaterial,
      patches: project?.patches ?? [],
      sourceSets: project?.materialSources.map((sourceSet) => ({
        id: sourceSet.id,
        sourceIds: sourceSet.registeredChannels?.channels.map((source) => source.id) ?? [],
      })) ?? [],
    });
    if (selectedRegionId && selectedRegionSourceSetId !== sourceSetId) {
      setSelectedRegionId(null);
      setActivePatchId(null);
      setRegionPatchEditId(null);
      setDraftPatchPreview(null);
      setSourceFrameEditing(false);
    }
    setSelectedSourceSetId(sourceSetId);
    selectSourceChannel(channel);
  }

  function selectSourceChannel(channel: SourceChannel) {
    setSelectedChannel(channel);
    const view = compiledMapViewForSourceChannel(channel);
    if (view && materialMapRouteAvailable(view)) setMapViewState(view);
  }

  function selectCompiledMapView(view: CompiledMapView) {
    if (processingOpen) {
      setProcessingRequestedMap(view);
      setMapViewState(view);
      mapViewRef.current = view;
      return;
    }
    setMapViewState(view);
    mapViewRef.current = view;
    setProcessingRequestedMap(view);
    const channel = sourceChannelForCompiledMapView(view);
    if (channel) setSelectedChannel(channel);
  }

  function toggleAuthoringPane(pane: AuthoringPane) {
    if (workspaceMode === "processing") { setWorkspaceMode("authoring"); return; }
    setAuthoringPanes((current) => {
      const next = new Set(current);
      if (next.has(pane) && next.size > 1) next.delete(pane); else next.add(pane);
      return next;
    });
  }

  function selectPatchAndLinkedRegion(patchId: string, sourceSetId?: string) {
    const patch = project?.patches.find((candidate) => candidate.id === patchId);
    const owner = sourceSetId
      ? project?.materialSources.find((source) => source.id === sourceSetId)
      : patch && project?.materialSources.find((source) => source.registeredChannels?.channels.some((channel) => channel.id === patch.sourceId));
    if (owner) chooseSource(owner.id, "base_color");
    setActivePatchId(patchId);
    setSourceFrameEditing(false);
    const linkedRegionIds = project?.document?.topology.regions
      .filter((region) => {
        const content = project.document!.regionBindings[region.id]?.content;
        return content?.type === "patch" && content.id === patchId;
      })
      .map((region) => region.id) ?? [];
    setSelectedRegionId((current) => current && linkedRegionIds.includes(current) ? current : linkedRegionIds[0] ?? null);
  }

  return (
    <main className="app-shell" aria-label="Hot Trimmer source-first workbench">
      <header className="topbar" data-tauri-drag-region>
        <strong className="brand" data-tauri-drag-region>Hot Trimmer</strong>
        <nav className="project-actions" aria-label="Project commands">
          <button onClick={() => void closeToDraft()} disabled={!native || activity !== "idle"}>New</button>
          <button onClick={() => void chooseProject()} disabled={!native || activity !== "idle"}>Open</button>
          <span className="menu-anchor">
            <button onClick={() => { setShowRecents((shown) => !shown); void refreshRecents(); }} disabled={!native || activity !== "idle"}>Recent</button>
            {showRecents ? <span className="popup-menu">
              {recentProjects.some((recent) => recent.available)
                ? recentProjects.filter((recent) => recent.available).map((recent) => <button key={recent.path} onClick={() => void openProjectAt(recent.path)}><strong>{recent.name}</strong><small>{recent.path}</small></button>)
                : <span>No recent projects</span>}
            </span> : null}
          </span>
          <button onClick={() => void saveProject()} disabled={!project || activity !== "idle"}>Save</button>
          <button onClick={() => void saveProjectAs()} disabled={!project || activity !== "idle"}>Save As</button>
          <button onClick={() => void closeToDraft()} disabled={!project || activity !== "idle"}>Close</button>
          <button onClick={() => void revealItemInDir(project?.path ?? "")} disabled={!project || project.isDraft}>Reveal</button>
        </nav>
        <div className="project-context">
          {renaming ? <input autoFocus value={draftName} onChange={(event) => setDraftName(event.target.value)} onBlur={() => void commitProjectName()} onKeyDown={(event) => {
            if (event.key === "Enter") void commitProjectName();
            if (event.key === "Escape") setRenaming(false);
          }} /> : <strong onDoubleClick={() => { setDraftName(project?.name ?? "Untitled"); setRenaming(true); }}>{project?.name ?? "Untitled"}</strong>}
          <span>{project?.isDraft ? "Draft" : project?.dirty ? "Unsaved" : "Saved"}</span>
        </div>
        <nav className="workflow" aria-label="Creative workspaces">
          <button className={`mode ${sourceWorkbenchOpen ? "active" : ""}`} aria-pressed={sourceWorkbenchOpen} onClick={() => toggleAuthoringPane("workbench")}>Workbench</button>
          <button className={`mode ${hotspotSheetOpen ? "active" : ""}`} aria-pressed={hotspotSheetOpen} onClick={() => toggleAuthoringPane("hotspotSheet")}>Hotspot Sheet</button>
          <button className={`mode ${processingOpen ? "active" : ""}`} aria-pressed={processingOpen} onClick={() => setWorkspaceMode("processing")}>Processing</button>
        </nav>
        <span className="window-drag-space" data-tauri-drag-region />
        <div className="publish-actions">
          <button className={debugOpen ? "active" : ""} aria-expanded={debugOpen} aria-controls="debug-drawer" onClick={() => setDebugOpen((open) => !open)}>Debug</button>
          <button onClick={() => void exportMaterialMaps()} disabled={!native || !project?.document || activity !== "idle"} title="Render and package every enabled material-map output for the current document revision.">Export All Maps</button>
          <button disabled title="Send to Blender requires publish and companion commands.">Send to Blender</button>
        </div>
        {native ? <div className="window-controls" aria-label="Window controls">
          <button className="window-minimize" aria-label="Minimize" title="Minimize" onClick={() => void getCurrentWindow().minimize()}><WindowControlIcon kind="minimize" /></button>
          <button className="window-maximize" aria-label="Maximize or restore" title="Maximize or restore" onClick={() => void getCurrentWindow().toggleMaximize()}><WindowControlIcon kind="maximize" /></button>
          <button className="window-close" aria-label="Close" title="Close" onClick={() => void getCurrentWindow().close()}><WindowControlIcon kind="close" /></button>
        </div> : null}
      </header>

      <section ref={workbenchRef} className={`workbench workspace-${workspaceMode} pane-layout-${paneMode} ${sourceFrameLayout ? "source-frame-layout" : ""}`} style={{ gridTemplateColumns: workbenchColumns }}>
        {showLibrary ? <SourceLibrary
          project={project}
          activeSourceSetId={activeSourceSetId}
          selectedSource={selectedSource}
          activePatchId={activePatchId}
          onSelect={chooseSource}
          onSelectPatch={(patchId, sourceSetId) => selectPatchAndLinkedRegion(patchId, sourceSetId)}
          onAddSourceSet={() => void addSourceSet()}
          onAddMaps={(id) => void chooseImages(undefined, id)}
          onSetPrimary={(id) => void setPrimaryMaterialExplicit(id)}
          onRemove={(id) => void removeSourceSet(id)}
          onRenameSource={(id) => void renameSourceSet(id)}
          onRenamePatch={(id) => void renamePatch(id)}
          onDeletePatch={(id) => void deletePatch(id)}
        /> : null}
        {showLibrary && showSourceWorkspace ? <PaneSplitter kind="library-source" paneDrag={paneDrag} setPanes={setPanes} workbenchRef={workbenchRef} /> : null}
        {showSourceWorkspace ? <section className={`source-workspace ${regionPatchEditId ? "region-patch-isolation" : ""}`}>
          <MapSlots
            sources={activeSources}
            selectedChannel={selectedChannel}
            onSelect={selectSourceChannel}
            onOpen={(channel) => {
              const replacingDiffuse = channel === "base_color" && activeSources.some((source) => source.channel === "base_color");
              if (replacingDiffuse) replaceBaseWithPreflight(activeSourceSetId);
              else void chooseImages(channel);
            }}
            onOpenAll={() => void chooseImages()}
          />
          <div className="patch-toolbar">
            <button className={patchTool === "rectangle" ? "active" : ""} onClick={() => setPatchTool((tool) => tool === "rectangle" ? null : "rectangle")} disabled={!selectedSource}>Rectangle</button>
            <button className={patchTool === "four-point" ? "active" : ""} onClick={() => setPatchTool((tool) => tool === "four-point" ? null : "four-point")} disabled={!selectedSource}>Four Point</button>
            <button onClick={() => activePatchId && void deletePatch(activePatchId)} disabled={!activePatchId}>Delete Patch</button>
            <button onClick={() => activePatchId && selectedRegionId && void assignPatchToRegion(activePatchId, selectedRegionId)} disabled={!activePatchId || !selectedRegionId || activity !== "idle"}>Assign patch to region</button>
            <label className="normal-setting" title="Applied explicitly when importing or replacing a tangent-space Normal map.">
              Normal convention
              <select value={normalConvention} onChange={(event) => setNormalConvention(event.currentTarget.value as "open_gl" | "direct_x")}>
                <option value="open_gl">OpenGL (+Y)</option>
                <option value="direct_x">DirectX (-Y)</option>
              </select>
            </label>
            <label title="Maximum transient rectification rate. Actual rate is limited by available CPU time.">
              Draft preview
              <select value={draftPreviewFps} onChange={(event) => setDraftPreviewFps(Number(event.currentTarget.value) as 10 | 30 | 60)}>
                <option value={10}>10 FPS</option>
                <option value={30}>30 FPS</option>
                <option value={60}>60 FPS</option>
              </select>
              <output className="draft-preview-actual">{actualDraftPreviewFps ? `~${actualDraftPreviewFps} actual` : "idle"}</output>
            </label>
          </div>
          <SourceCanvas
            source={selectedSource}
            sourceFrame={project?.document?.sourceFrame?.sourceSetId === activeSourceSetId ? project?.document?.sourceFrame : undefined}
            logicalGrid={project?.document?.sourceFrame?.sourceSetId === activeSourceSetId ? project?.document?.logicalGrid : undefined}
            partitionRegions={project?.document?.sourceFrame?.sourceSetId === activeSourceSetId ? artifact?.regions ?? [] : []}
            selectedSlot={selectedSlot}
            crop={selectedCanvasCrop ?? selectedCrop}
            selectedRegion={selectedRegion}
            radialBehavior={selectedBinding?.mapping.behavior}
            radialSourceGeometry={selectedRadialSourceGeometry}
            onCommitRadial={(radial) => selectedRegionId && void setRegionRadial(selectedRegionId, radial)}
            onCommitBehavior={(behavior) => selectedRegionId && void setRegionBehavior(selectedRegionId, behavior)}
            sourceFrameEditing={sourceFrameEditing}
            importing={activity === "importing"}
            importProgress={importProgress}
            onOpenBase={() => void chooseImages("base_color")}
            onCommitCrop={(bounds) => void editSelectedRegionAsPatch(bounds)}
            onDraftCrop={(bounds) => { if (selectedBindingContent?.type === "patch") previewSelectedCrop(bounds); }}
            onSetSourceFrame={(bounds) => void setSourceFrame(bounds)}
            patches={project?.patches.filter((patch) => patch.sourceId === selectedSource?.id) ?? []}
            activePatchId={activePatchId}
            onEditPatch={selectPatchAndLinkedRegion}
            onCommitPatch={(patchId, geometry) => void replacePatchGeometry(patchId, geometry)}
            onDraftPatch={setDraftPatchPreview}
            onDeletePatch={(patchId) => void deletePatch(patchId)}
            onExitPatch={() => { setDraftPatchPreview(null); setActivePatchId(null); setRegionPatchEditId(null); setSelectedRegionId(null); }}
            tool={patchTool}
            onCreatePatch={(geometry, fourPoint) => void createPatch(geometry, fourPoint)}
            onCancelTool={() => setPatchTool(null)}
          />
          {regionPatchEditId ? <div className="region-edit-toast" role="status" aria-live="polite"><strong>Editing region source</strong><span>Preview stays pinned until the selected size is ready.</span><button onClick={exitRegionPatchEdit}>Cancel</button><button onClick={exitRegionPatchEdit}>Done</button></div> : null}
        </section> : null}
        {showSourceWorkspace && hotspotSheetOpen ? <PaneSplitter kind="source-sheet" proportional libraryVisible={showLibrary} inspectorVisible={showInspector} onSourceShareChange={setSourceSheetShare} paneDrag={paneDrag} setPanes={setPanes} workbenchRef={workbenchRef} /> : null}
        {(hotspotSheetOpen || processingOpen) ? <SheetWorkbench
          project={project}
          artifact={artifact}
          preview={preview}
          preparedPatchPreview={activePatchId ? preparedPatchPreview : null}
          preparedPatchPreviews={preparedPatchPreviews}
          activePatchId={activePatchId}
          mapView={mapView}
          processingRequestedMap={processingRequestedMap}
          setMapView={selectCompiledMapView}
          selectedRegionId={selectedRegionId}
          setSelectedRegionId={(id) => { setSelectedRegionId(id); if (!id) { setActivePatchId(null); setRegionPatchEditId(null); setDraftPatchPreview(null); setSourceFrameEditing(false); } }}
          sourceFrameEditing={sourceFrameEditing}
          onEditSourceFrame={() => { const frame = project?.document?.sourceFrame; if (!frame) return; setSelectedRegionId(null); setActivePatchId(null); setRegionPatchEditId(null); setDraftPatchPreview(null); setSelectedSourceSetId(frame.sourceSetId); selectSourceChannel("base_color"); setWorkspaceMode("authoring"); setAuthoringPanes((current) => new Set([...current, "workbench"])); setSourceFrameEditing(true); }}
          buildState={buildState}
          problem={problem}
          templateId={templateId}
          setTemplateId={setTemplateId}
          primaryMaterial={primaryMaterial}
          build={build}
          renderFullResolutionPreview={renderFullResolutionPreview}
          interactivePreviewProfile={interactivePreviewProfile}
          setInteractivePreviewProfile={setInteractivePreviewProfile}
          previewProgress={previewProgress}
          previewElapsedMs={previewElapsedMs}
          exportProgress={exportProgress}
          activity={activity}
          setResolution={setResolution}
          setAtlasPadding={setAtlasPadding}
           targetRegionCount={targetRegionCount}
           setTargetRegionCount={setTargetRegionCount}
           regenerateSourceFrame={regenerateSourceFrame}
           candidateRecipe={candidateRecipe}
           setCandidateRecipe={setCandidateRecipe}
           candidatePreviewing={candidatePreviewing}
           candidateIsCurrent={candidatePreviewHash === partitionRecipeFingerprint(candidateRecipe)}
           candidatePreviewRecipe={candidatePreviewRecipe}
           previewCandidate={previewPartitionCandidate}
           discardCandidate={discardPartitionCandidate}
           acceptCandidate={acceptPartitionCandidate}
           onLayoutCommand={editSourceFrameLayout}
           onUndo={() => void history(false)}
           onRedo={() => void history(true)}
           presentation={processingOpen ? "processing" : "layout"}
           previewClientTelemetry={previewClientTelemetry}
           feedbackDebug={debugOpen ? {
             view: feedbackView,
             profile: feedbackProfile,
             comparisonMode: feedbackComparisonMode,
             selectedOperationId: feedbackSelectedOperationId,
             activeTool: feedbackActiveTool,
             lastCommandResult: feedbackLastCommandResult,
             error: feedbackError,
             execution: feedbackExecution,
             allRegions: feedbackPreviewAllRegions,
           } : null}
           onPreviewPaint={(dimensions) => {
             if (previewProgress?.feedbackRequestIdentity && dimensions.generation !== undefined && dimensions.generation !== feedbackExecution?.publishedGeneration) return;
             if (previewPublishStartedAt.current !== null) {
               setPreviewProgress((current) => current ? { ...current, phase: "painted", elapsedMs: performance.now() - current.startedAt, dimensions } : current);
               setPreviewClientTelemetry((current) => [
                 ...current.filter((entry) => !entry.startsWith("paint_ms=") && !entry.startsWith("png_decoded_dimensions=")),
                 `png_decoded_dimensions=${dimensions.width}x${dimensions.height}`,
                 `paint_ms=${Math.round(performance.now() - previewPublishStartedAt.current!)}`,
               ]);
             }
           }}
         /> : null}
        {showInspector && (showSourceWorkspace || hotspotSheetOpen) ? <PaneSplitter kind="sheet-inspector" paneDrag={paneDrag} setPanes={setPanes} workbenchRef={workbenchRef} /> : null}
        {showInspector ? <Inspector
          project={project}
          artifact={artifact}
          sourceAnalysis={activePatchId ? preparedPatchPreview : null}
          selectedRegion={selectedRegion}
          mapView={mapView}
          setMapView={selectCompiledMapView}
          onUndo={() => void history(false)}
          onRedo={() => void history(true)}
          onClassify={(materialSourceId, classificationCommand) => void applyMaterialClassificationCommand(materialSourceId, classificationCommand)}
          onCalibrate={(materialSourceId, calibrationCommand) => void applyMaterialCalibrationCommand(materialSourceId, calibrationCommand)}
          onSetRadial={(regionId, radial) => void setRegionRadial(regionId, radial)}
          onResizeRegion={(regionId, gridRect) => void editSourceFrameLayout({ type: "resize_source_frame_region", regionId, gridRect })}
          onSetSourceFrame={(bounds) => void setSourceFrame(bounds)}
          sourceFrameEditing={sourceFrameEditing}
          onSetSourceFrameEditing={setSourceFrameEditing}
          selectedSourceSetId={activeSourceSetId}
          onSetExemplarGroup={(id, group) => void setExemplarGroup(id, group)}
          onSetDelightingIntent={(id, intent) => void setDelightingIntent(id, intent)}
          onSetRegionContent={(regionId, content) => void assignContentToRegion(regionId, content)}
          onSetRegionBehavior={(regionId, behavior) => void setRegionBehavior(regionId, behavior)}
        /> : null}
        {processingOpen ? <ProcessingSidebar
          project={project}
          artifact={artifact}
          selectedRegionId={selectedRegionId}
          commandBusy={feedbackCommandBusy}
          edgeDetailError={edgeDetailError}
          activeTab={processingSidebarTab}
          onActiveTab={setProcessingSidebarTab}
          onCommand={applyFeedbackCommand}
          onRender={() => void renderFullResolutionPreview()}
          onUndo={() => void history(false)}
          onRedo={() => void history(true)}
        /> : null}
        {debugOpen ? <div id="debug-drawer" className="debug-drawer" role="dialog" aria-label="Debug and fixtures">
          <button className="debug-drawer-close" onClick={() => setDebugOpen(false)}>Close Debug</button>
          <FeedbackWorkbench
          project={project}
          artifact={artifact}
          selectedRegionId={selectedRegionId}
          selectedOperationId={feedbackSelectedOperationId}
          view={feedbackView}
          profile={feedbackProfile}
          comparisonMode={feedbackComparisonMode}
          activeTool={feedbackActiveTool}
          commandBusy={feedbackCommandBusy}
          edgeDetailError={edgeDetailError}
          onSelectRegion={selectFeedbackRegion}
          onSelectOperation={setFeedbackSelectedOperationId}
          onView={setFeedbackView}
          onInspectView={requestFeedbackVisibleView}
          onProfile={setFeedbackProfile}
          onComparisonMode={setFeedbackComparisonMode}
          onActiveTool={setFeedbackActiveTool}
          onCommand={applyFeedbackCommand}
          onRequestVisibleView={requestFeedbackVisibleView}
          onCreateSample={() => void createFeedbackSample()}
          onUndo={() => void history(false)}
          onRedo={() => void history(true)}
          />
        </div> : null}
      </section>
      <footer className="statusbar">
        <span>{project?.name ?? "Untitled"}</span>
        <span>{buildState}</span>
        <span>{selectedSource ? `${channelLabel(selectedSource.channel)} / ${selectedSource.orientedSize.width} x ${selectedSource.orientedSize.height}` : "No source selected"}</span>
      </footer>
      {activity === "starting" ? <div className="busy-corner">Starting untitled draft...</div> : null}
      {problem ? <div className="global-error" role="alert"><strong>{problem.message}</strong><span>{problem.recovery}</span></div> : null}
      {activePatchId && preview && selectedRegion ? <PatchPreview preview={preview} region={selectedRegion} /> : null}
    </main>
  );
}

function WindowControlIcon(props: { kind: "minimize" | "maximize" | "close" }) {
  return <svg viewBox="0 0 12 12" aria-hidden="true" focusable="false">
    {props.kind === "minimize" ? <path d="M2 8.5h8" /> : null}
    {props.kind === "maximize" ? <rect x="2.25" y="2.25" width="7.5" height="7.5" /> : null}
    {props.kind === "close" ? <path d="m2.5 2.5 7 7m0-7-7 7" /> : null}
  </svg>;
}

function SourceLibrary(props: {
  project: ProjectProjection | null;
  activeSourceSetId: string;
  selectedSource: SourceProjection | null;
  activePatchId: string | null;
  onSelect: (sourceSetId: string, channel: SourceChannel) => void;
  onSelectPatch: (patchId: string, sourceSetId: string) => void;
  onAddSourceSet: () => void;
  onAddMaps: (sourceSetId: string) => void;
  onSetPrimary: (sourceSetId: string) => void;
  onRemove: (sourceSetId: string) => void;
  onRenameSource: (sourceSetId: string) => void;
  onRenamePatch: (patchId: string) => void;
  onDeletePatch: (patchId: string) => void;
}) {
  const sourceSets = props.project?.materialSources ?? [];
  const [sourceMenu, setSourceMenu] = useState<{ id: string; x: number; y: number } | null>(null);
  const [patchMenu, setPatchMenu] = useState<{ id: string; x: number; y: number } | null>(null);
  useEffect(() => {
    if (!sourceMenu && !patchMenu) return;
    const dismiss = (event: PointerEvent) => {
      if (!(event.target as Element | null)?.closest(".source-context-menu, .library-patch-context-menu")) {
        setSourceMenu(null);
        setPatchMenu(null);
      }
    };
    const dismissBlur = () => { setSourceMenu(null); setPatchMenu(null); };
    window.addEventListener("pointerdown", dismiss, true);
    window.addEventListener("blur", dismissBlur);
    return () => {
      window.removeEventListener("pointerdown", dismiss, true);
      window.removeEventListener("blur", dismissBlur);
    };
  }, [sourceMenu, patchMenu]);
  const sourceSetForPatch = (patch: Patch) => sourceSets.find((set) => set.registeredChannels?.channels.some((source) => source.id === patch.sourceId));
  const patches = (props.project?.patches ?? []).filter((patch) => sourceSetForPatch(patch)?.id === props.activeSourceSetId);
  return <aside className="source-library">
    <header className="panel-title"><span>WORKBENCH</span></header>
    <section className="library-section source-list"><div className="section-head"><span>SOURCES</span><b>{sourceSets.length}</b></div>
      {sourceSets.map((set) => {
        const channels = set.registeredChannels?.channels ?? [];
        const base = channels.find((source) => source.channel === "base_color");
        const count = channels.length;
        const readiness = base ? "Ready" : "Missing Diffuse";
        return <div key={set.id} className="source-set-entry">
          <button className={`source-set ${set.id === props.activeSourceSetId ? "active" : ""}`} onClick={() => props.onSelect(set.id, base?.channel ?? "base_color")} onContextMenu={(event) => { event.preventDefault(); event.stopPropagation(); setSourceMenu({ id: set.id, x: event.clientX, y: event.clientY }); }}>
            <span className="thumb">{base ? <img src={base.thumbnailDataUrl} alt="" /> : "+"}</span>
            <span><strong>{set.name}</strong><small>{count} map{count === 1 ? "" : "s"} · {base ? `${base.orientedSize.width}×${base.orientedSize.height}` : "missing Diffuse"}</small><small>{readiness} · rev {set.sourceRevision}{props.project?.document?.primaryMaterial === set.id ? " · PRIMARY" : ""}</small></span>
          </button>
        </div>;
      })}
      <button className="new-source" onClick={props.onAddSourceSet}>+ Add independent source…</button>
      <button className="new-source" title="Add Normal, Height, Roughness, AO, and other texture maps to this source group." onClick={() => props.activeSourceSetId && props.onAddMaps(props.activeSourceSetId)} disabled={!props.activeSourceSetId}>+ Add maps…</button>
    </section>
    <section className="library-section patches"><div className="section-head"><span>PATCHES · SELECTED SOURCE</span><b>{patches.length}</b></div>
      <div className="patch-list">{patches.map((patch) => {
        const owner = sourceSetForPatch(patch); const base = owner?.registeredChannels?.channels.find((source) => source.channel === "base_color");
        const xs = patch.geometry.corners.map((point) => point.x); const ys = patch.geometry.corners.map((point) => point.y);
        const dimensions = base ? `${Math.round((Math.max(...xs) - Math.min(...xs)) * base.orientedSize.width)}×${Math.round((Math.max(...ys) - Math.min(...ys)) * base.orientedSize.height)}` : "unknown";
        const assigned = props.project?.document && Object.values(props.project.document.regionBindings).some((binding) => binding.content.type === "patch" && binding.content.id === patch.id);
        const [a, b, c, d] = patch.geometry.corners;
        const shape = a.y === b.y && b.x === c.x && c.y === d.y && d.x === a.x ? "Rectangle" : "Four point";
        return <button key={patch.id} className={`patch-row ${props.activePatchId === patch.id ? "active" : ""}`} onClick={() => owner && props.onSelectPatch(patch.id, owner.id)} onContextMenu={(event) => { event.preventDefault(); event.stopPropagation(); setPatchMenu({ id: patch.id, x: event.clientX, y: event.clientY }); }}><span className="thumb">{base ? <img src={base.thumbnailDataUrl} alt="" /> : "?"}</span><span><strong>{patch.name}</strong><small>{owner?.name ?? "Missing source"} · {shape} · {dimensions}</small><small>{patch.enabled ? "Enabled" : "Disabled"}{assigned ? " · assigned" : ""}</small></span></button>;
      })}</div>
      {!patches.length ? <p>Choose Rectangle or Four Point, then author patches on the selected source. Selecting another source switches this list.</p> : null}
    </section>
    {sourceMenu ? createPortal(<div className="patch-context-menu source-context-menu" role="menu" style={{ left: sourceMenu.x, top: sourceMenu.y }} onContextMenu={(event) => event.preventDefault()}><button role="menuitem" onClick={() => { props.onRenameSource(sourceMenu.id); setSourceMenu(null); }}>Rename…</button><button role="menuitem" onClick={() => { props.onSetPrimary(sourceMenu.id); setSourceMenu(null); }}>Set as primary / Rebase layout…</button><button role="menuitem" onClick={() => { const base = sourceSets.find((set) => set.id === sourceMenu.id)?.registeredChannels?.channels.find((source) => source.channel === "base_color"); if (base) void revealItemInDir(base.original.path); setSourceMenu(null); }}>Reveal source</button><button role="menuitem" className="danger" onClick={() => { props.onRemove(sourceMenu.id); setSourceMenu(null); }}>Remove…</button></div>, document.body) : null}
    {patchMenu ? createPortal(<div className="patch-context-menu library-patch-context-menu" role="menu" style={{ left: patchMenu.x, top: patchMenu.y }} onContextMenu={(event) => event.preventDefault()}><button onClick={() => { props.onRenamePatch(patchMenu.id); setPatchMenu(null); }}>Rename…</button><button className="danger" onClick={() => { props.onDeletePatch(patchMenu.id); setPatchMenu(null); }}>Remove…</button></div>, document.body) : null}
  </aside>;
}

function MapSlots(props: {
  sources: readonly SourceProjection[];
  selectedChannel: SourceChannel;
  onSelect: (channel: SourceChannel) => void;
  onOpen: (channel: SourceChannel) => void;
  onOpenAll: () => void;
}) {
  const hasBase = props.sources.some((source) => source.channel === "base_color");
  const [channelMenu, setChannelMenu] = useState<{ channel: SourceChannel; x: number; y: number; filled: boolean } | null>(null);
  useEffect(() => {
    if (!channelMenu) return;
    const dismiss = (event: PointerEvent) => {
      if (!(event.target as Element | null)?.closest(".channel-context-menu")) setChannelMenu(null);
    };
    const dismissBlur = () => setChannelMenu(null);
    window.addEventListener("pointerdown", dismiss, true);
    window.addEventListener("blur", dismissBlur);
    return () => {
      window.removeEventListener("pointerdown", dismiss, true);
      window.removeEventListener("blur", dismissBlur);
    };
  }, [channelMenu]);
  return <><div className="map-slots" onContextMenu={(event) => event.preventDefault()} onWheel={(event) => {
    if (Math.abs(event.deltaY) > Math.abs(event.deltaX)) event.currentTarget.scrollLeft += event.deltaY;
  }}>
    <button className="map-slot add-maps" onClick={props.onOpenAll}>Add maps…</button>
    {channelOptions.map((option) => {
      const source = props.sources.find((candidate) => candidate.channel === option.value);
      const blocked = option.value !== "base_color" && !hasBase;
      return <button
        key={option.value}
        className={`map-slot ${props.selectedChannel === option.value ? "active" : ""} ${source ? "filled" : ""}`}
        disabled={blocked}
        title={blocked ? "Add a Diffuse texture to anchor this source group first." : source?.original.path ?? `Add ${option.label}`}
        onClick={() => {
          props.onSelect(option.value);
          if (!source) props.onOpen(option.value);
        }}
        onContextMenu={(event) => {
          event.preventDefault();
          event.stopPropagation();
          setChannelMenu({ channel: option.value, x: event.clientX, y: event.clientY, filled: !!source });
        }}
      >
        <span className={`channel-swatch ${option.tone}`}>{option.short}</span>
        <span><strong>{option.label}</strong><small>{source?.displayName ?? "+ Add map"}</small></span>
      </button>;
    })}
  </div>{channelMenu ? createPortal(<div className="patch-context-menu channel-context-menu" role="menu" style={{ left: channelMenu.x, top: channelMenu.y }} onContextMenu={(event) => event.preventDefault()}><button role="menuitem" onClick={() => { props.onOpen(channelMenu.channel); setChannelMenu(null); }}>{channelMenu.filled ? "Replace texture…" : "Add texture…"}</button></div>, document.body) : null}</>;
}

function useViewportController(content: { width: number; height: number } | null, contentKey = "default") {
  const containerRef = useRef<HTMLElement | null>(null);
  const [view, setView] = useState<CanvasView>({ x: 0, y: 0, scale: 1 });
  const mode = useRef<"fit" | "manual">("fit");
  const previousContent = useRef<{ key: string; width: number; height: number } | null>(null);
  const pan = useRef<{ pointerId: number; x: number; y: number; origin: CanvasView } | null>(null);
  function fit() {
    const rect = containerRef.current?.getBoundingClientRect();
    if (!rect || !content) return;
    mode.current = "fit";
    setView(fitView({ width: rect.width, height: rect.height }, content));
  }
  useLayoutEffect(() => {
    const element = containerRef.current;
    if (!element || !content) return;
    const rect = element.getBoundingClientRect();
    const previous = previousContent.current;
    if (!previous || previous.key !== contentKey) {
      mode.current = "fit";
      setView(fitView({ width: rect.width, height: rect.height }, content));
    } else if (previous.width !== content.width || previous.height !== content.height) {
      if (mode.current === "fit") {
        setView(fitView({ width: rect.width, height: rect.height }, content));
      } else {
        setView((current) => preserveViewAcrossContentResize(current, previous, content, { width: rect.width, height: rect.height }));
      }
    }
    previousContent.current = { key: contentKey, width: content.width, height: content.height };
    const observer = new ResizeObserver(() => { if (mode.current === "fit") fit(); });
    observer.observe(element);
    return () => observer.disconnect();
  }, [contentKey, content?.width, content?.height]);
  function wheel(event: React.WheelEvent<HTMLElement>) {
    if (!content) return;
    event.preventDefault();
    const rect = event.currentTarget.getBoundingClientRect();
    mode.current = "manual";
    setView((current) => anchoredZoom(current, { x: event.clientX - rect.left, y: event.clientY - rect.top }, event.deltaY, { min: 0.02, max: 8 }));
  }
  function beginPan(event: React.PointerEvent<HTMLElement>) {
    if (event.button !== 1) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    mode.current = "manual";
    pan.current = { pointerId: event.pointerId, x: event.clientX, y: event.clientY, origin: view };
  }
  function movePan(event: React.PointerEvent<HTMLElement>) {
    const active = pan.current;
    if (active?.pointerId === event.pointerId) setView({ ...active.origin, x: active.origin.x + event.clientX - active.x, y: active.origin.y + event.clientY - active.y });
  }
  function endPan(event: React.PointerEvent<HTMLElement>) {
    if (pan.current?.pointerId === event.pointerId) pan.current = null;
  }
  function zoom(multiplier: number) {
    mode.current = "manual";
    setView((current) => ({ ...current, scale: Math.min(8, Math.max(0.02, current.scale * multiplier)) }));
  }
  return { containerRef, view, fit, wheel, beginPan, movePan, endPan, zoom };
}

function SourceCanvas(props: {
  source: SourceProjection | null;
  sourceFrame?: SourceFrame;
  logicalGrid?: { schemaVersion: number; width: number; height: number };
  partitionRegions: readonly ResolvedRegion[];
  selectedSlot: Stage14SlotProjection | null;
  crop: CropProjection | null;
  selectedRegion: ResolvedRegion | null;
  radialBehavior?: RegionBehavior;
  radialSourceGeometry?: PatchGeometry;
  onCommitRadial: (radial: NonNullable<RegionBehavior["radial"]>) => void;
  onCommitBehavior: (behavior: RegionBehavior) => void;
  sourceFrameEditing: boolean;
  importing: boolean;
  importProgress: { stage: string; fraction: number } | null;
  onOpenBase: () => void;
  onCommitCrop: (bounds: NormalizedBounds) => void;
  onDraftCrop: (bounds: NormalizedBounds) => void;
  onSetSourceFrame: (bounds: NormalizedBounds) => void;
  patches: readonly Patch[];
  activePatchId: string | null;
  onEditPatch: (patchId: string) => void;
  onCommitPatch: (patchId: string, geometry: PatchGeometry) => void;
  onDraftPatch: (draft: { patchId: string; geometry: PatchGeometry } | null) => void;
  onDeletePatch: (patchId: string) => void;
  onExitPatch: () => void;
  tool: "rectangle" | "four-point" | null;
  onCreatePatch: (geometry: PatchGeometry, fourPoint: boolean) => void;
  onCancelTool: () => void;
}) {
  const stageRef = useRef<HTMLDivElement | null>(null);
  const viewport = useViewportController(props.source?.orientedSize ?? null, props.source?.id ?? "no-source");
  const cropDrag = useRef<{ pointerId: number; action: CropDragAction; origin: NormalizedBounds; x: number; y: number } | null>(null);
  const frameDrag = useRef<{ pointerId: number; action: CropDragAction; origin: NormalizedBounds; x: number; y: number } | null>(null);
  const [draftCrop, setDraftCrop] = useState<NormalizedBounds | null>(null);
  const [draftFrame, setDraftFrame] = useState<NormalizedBounds | null>(null);
  const draftCropRef = useRef<NormalizedBounds | null>(null);
  const patchDrag = useRef<
    | { kind: "corner"; pointerId: number; patchId: string; corner: number; corners: PatchGeometry["corners"] }
    | { kind: "move"; pointerId: number; patchId: string; start: { x: number; y: number }; corners: PatchGeometry["corners"] }
    | { kind: "resize"; pointerId: number; patchId: string; handle: PatchResizeHandle; corners: PatchGeometry["corners"] }
    | { kind: "rotate"; pointerId: number; patchId: string; center: { x: number; y: number }; lastAngle: number; lastValid: PatchGeometry["corners"]; corners: PatchGeometry["corners"] }
    | null
  >(null);
  const patchCreate = useRef<{ pointerId: number; start: { x: number; y: number } } | null>(null);
  const radialDrag = useRef<{ pointerId: number; kind: "center" | "inner" | "outer" | "seam" | "seam_blend"; lastRadial: NonNullable<RegionBehavior["radial"]>; lastOrientation: RegionBehavior["orientation"] } | null>(null);
  const [draftRadial, setDraftRadial] = useState<NonNullable<RegionBehavior["radial"]> | null>(null);
  const [draftRadialOrientation, setDraftRadialOrientation] = useState<RegionBehavior["orientation"] | null>(null);
  const [draftPatch, setDraftPatch] = useState<{ patchId: string; geometry: PatchGeometry } | null>(null);
  const draftPatchRef = useRef<{ patchId: string; geometry: PatchGeometry } | null>(null);
  const [draftRectangle, setDraftRectangle] = useState<PatchGeometry | null>(null);
  const [fourPointDraft, setFourPointDraft] = useState<Array<{ x: number; y: number }>>([]);
  const [pointEditPatchId, setPointEditPatchId] = useState<string | null>(null);
  const [loupePoint, setLoupePoint] = useState<{ x: number; y: number; corner: number; clientX: number; clientY: number } | null>(null);
  const [patchMenu, setPatchMenu] = useState<{ patchId: string; clientX: number; clientY: number } | null>(null);
  const committedCrop = props.crop?.bounds ?? null;
  const effectiveCrop = draftCrop ?? committedCrop;
  const effectiveFrame = draftFrame ?? props.sourceFrame?.bounds ?? null;
  const patchEditing = !!props.activePatchId;
  const effectiveRadialSourceGeometry = draftPatch?.patchId === props.activePatchId
    ? draftPatch.geometry
    : patchEditing
      ? props.radialSourceGeometry
      : effectiveCrop
        ? { corners: [
            { x: effectiveCrop.x, y: effectiveCrop.y },
            { x: effectiveCrop.x + effectiveCrop.width, y: effectiveCrop.y },
            { x: effectiveCrop.x + effectiveCrop.width, y: effectiveCrop.y + effectiveCrop.height },
            { x: effectiveCrop.x, y: effectiveCrop.y + effectiveCrop.height },
          ] as PatchGeometry["corners"] }
        : props.radialSourceGeometry;
  const radialEditing = props.radialBehavior?.role === "radial" && !!props.radialBehavior.radial && !!effectiveRadialSourceGeometry;

  useEffect(() => {
    setDraftCrop(null);
    draftCropRef.current = null;
    setDraftFrame(null);
  }, [props.crop?.bounds.x, props.crop?.bounds.y, props.crop?.bounds.width, props.crop?.bounds.height, props.sourceFrame?.identity.join(",")]);

  useEffect(() => {
    radialDrag.current = null;
    setDraftRadial(null);
    setDraftRadialOrientation(null);
  }, [props.selectedRegion?.regionId, props.radialBehavior?.radial?.centerX, props.radialBehavior?.radial?.centerY, props.radialBehavior?.radial?.innerRadius, props.radialBehavior?.radial?.outerRadius, props.radialBehavior?.orientation]);

  useEffect(() => {
    setDraftRectangle(null);
    setFourPointDraft([]);
  }, [props.tool]);

  useEffect(() => {
    if (pointEditPatchId !== props.activePatchId) setPointEditPatchId(null);
  }, [props.activePatchId]);

  useEffect(() => {
    function keyDown(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      if (patchMenu) setPatchMenu(null);
      else if (pointEditPatchId) setPointEditPatchId(null);
      else {
        setDraftPatch(null);
        draftPatchRef.current = null;
        setDraftCrop(null);
        draftCropRef.current = null;
        props.onDraftPatch(null);
        props.onExitPatch();
      }
    }
    window.addEventListener("keydown", keyDown);
    return () => window.removeEventListener("keydown", keyDown);
  }, [patchMenu, pointEditPatchId, props.onExitPatch]);

  useEffect(() => {
    if (!patchMenu) return;
    function dismiss(event: PointerEvent) {
      if (!(event.target as Element | null)?.closest(".patch-context-menu")) setPatchMenu(null);
    }
    function dismissBlur() { setPatchMenu(null); }
    window.addEventListener("pointerdown", dismiss);
    window.addEventListener("blur", dismissBlur);
    return () => {
      window.removeEventListener("pointerdown", dismiss);
      window.removeEventListener("blur", dismissBlur);
    };
  }, [patchMenu]);

  function point(event: React.PointerEvent): { x: number; y: number } {
    const rect = stageRef.current?.getBoundingClientRect();
    if (!rect || rect.width <= 0 || rect.height <= 0) return { x: 0, y: 0 };
    return {
      x: clamp01((event.clientX - rect.left) / rect.width),
      y: clamp01((event.clientY - rect.top) / rect.height),
    };
  }

  function movePointer(event: React.PointerEvent<HTMLElement>) {
    const activeRadial = radialDrag.current;
    if (activeRadial?.pointerId === event.pointerId && effectiveRadialSourceGeometry) {
      const target = point(event);
      const local = mapQuadToUnitSquare(effectiveRadialSourceGeometry.corners, target);
      if (activeRadial.kind === "seam") {
        const dx = local.x - activeRadial.lastRadial.centerX, dy = local.y - activeRadial.lastRadial.centerY;
        const quarter = ((Math.round(Math.atan2(dy, dx) / (Math.PI / 2)) % 4) + 4) % 4;
        activeRadial.lastOrientation = (["zero", "ninety", "one_eighty", "two_seventy"] as const)[quarter]!;
        setDraftRadialOrientation(activeRadial.lastOrientation);
      } else if (activeRadial.kind === "seam_blend") {
        const dx = local.x - activeRadial.lastRadial.centerX, dy = local.y - activeRadial.lastRadial.centerY;
        const seamAngle = ({ zero: 0, ninety: Math.PI / 2, one_eighty: Math.PI, two_seventy: Math.PI * 1.5 } as const)[activeRadial.lastOrientation];
        const delta = Math.abs(Math.atan2(Math.sin(Math.atan2(dy, dx) - seamAngle), Math.cos(Math.atan2(dy, dx) - seamAngle)));
        activeRadial.lastRadial = { ...activeRadial.lastRadial, seamBlendWidth: Math.min(0.25, delta / (Math.PI * 2)) };
        setDraftRadial(activeRadial.lastRadial);
      } else {
        const next = { ...activeRadial.lastRadial };
        if (activeRadial.kind === "center") { next.centerX = local.x; next.centerY = local.y; }
        else {
          const radius = Math.hypot(local.x - next.centerX, local.y - next.centerY);
          if (activeRadial.kind === "inner") next.innerRadius = Math.max(0, Math.min(next.outerRadius - 0.001, radius));
          if (activeRadial.kind === "outer") next.outerRadius = Math.max(next.innerRadius + 0.001, Math.min(2, radius));
        }
        if (next.outerRadius > next.innerRadius) activeRadial.lastRadial = next;
        setDraftRadial(activeRadial.lastRadial);
      }
      return;
    }
    const creating = patchCreate.current;
    if (creating?.pointerId === event.pointerId) {
      const end = point(event);
      setDraftRectangle(rectangleGeometry(creating.start, end));
      return;
    }
    const activeFrame = frameDrag.current;
    if (activeFrame?.pointerId === event.pointerId) {
      const target = point(event);
      const dx = target.x - activeFrame.x;
      const dy = target.y - activeFrame.y;
      const next = activeFrame.action === "move"
        ? adjustCrop(activeFrame.origin, "move", dx, dy)
        : resizeAspectLocked(activeFrame.origin, activeFrame.action, dx, dy, frameAspect(props.sourceFrame!, props.source!));
      setDraftFrame(next);
      return;
    }
    const activePoint = patchDrag.current;
    if (activePoint?.pointerId === event.pointerId) {
      const target = point(event);
      let corners: PatchGeometry["corners"];
      if (activePoint.kind === "corner") {
        const next = [...activePoint.corners] as unknown as [typeof target, typeof target, typeof target, typeof target];
        next[activePoint.corner] = target;
        corners = next;
        setLoupePoint({ ...target, corner: activePoint.corner, clientX: event.clientX, clientY: event.clientY });
      } else if (activePoint.kind === "move") {
        corners = movePatch(activePoint.corners, target.x - activePoint.start.x, target.y - activePoint.start.y);
      } else if (activePoint.kind === "resize") {
        corners = resizePatch(activePoint.corners, activePoint.handle, target, {
          proportional: event.shiftKey,
          fromCenter: event.altKey,
        });
      } else {
        const angle = patchPointerAngle(target, activePoint.center, props.source!.orientedSize);
        const candidate = rotatePatch(activePoint.lastValid, activePoint.center, angle - activePoint.lastAngle, props.source!.orientedSize);
        if (candidate !== activePoint.lastValid) {
          activePoint.lastValid = candidate;
          activePoint.lastAngle = angle;
        }
        corners = activePoint.lastValid;
      }
      const draft = { patchId: activePoint.patchId, geometry: { corners } };
      draftPatchRef.current = draft;
      setDraftPatch(draft);
      props.onDraftPatch(draft);
      return;
    }
    const activeCrop = cropDrag.current;
    if (activeCrop?.pointerId === event.pointerId) {
      const target = point(event);
      const dx = target.x - activeCrop.x;
      const dy = target.y - activeCrop.y;
      const next = activeCrop.action === "move"
        ? adjustCrop(activeCrop.origin, "move", dx, dy)
        : resizeAspectLocked(activeCrop.origin, activeCrop.action, dx, dy, sourceCropAspect(props.selectedSlot, props.source!.orientedSize.width, props.source!.orientedSize.height));
      draftCropRef.current = next;
      setDraftCrop(next);
      props.onDraftCrop(next);
      return;
    }
    viewport.movePan(event);
  }

  function endPointer(event: React.PointerEvent<HTMLElement>) {
    if (radialDrag.current?.pointerId === event.pointerId) {
      const completed = radialDrag.current;
      radialDrag.current = null;
      if (completed.kind === "seam" && props.radialBehavior) props.onCommitBehavior(changedBehavior(props.radialBehavior, { orientation: completed.lastOrientation }));
      else props.onCommitRadial(completed.lastRadial);
      setDraftRadial(null);
      setDraftRadialOrientation(null);
    }
    if (patchCreate.current?.pointerId === event.pointerId) {
      const start = patchCreate.current.start;
      patchCreate.current = null;
      const geometry = rectangleGeometry(start, point(event));
      setDraftRectangle(null);
      if (rectangleArea(geometry) > 0.0004) props.onCreatePatch(geometry, false);
    }
    if (frameDrag.current?.pointerId === event.pointerId) {
      frameDrag.current = null;
      if (draftFrame) props.onSetSourceFrame(draftFrame);
      setDraftFrame(null);
    }
    if (patchDrag.current?.pointerId === event.pointerId) {
      const patchId = patchDrag.current.patchId;
      patchDrag.current = null;
      setLoupePoint(null);
      if (draftPatchRef.current?.patchId === patchId) props.onCommitPatch(patchId, draftPatchRef.current.geometry);
      draftPatchRef.current = null;
    }
    const activeCrop = cropDrag.current;
    if (activeCrop?.pointerId === event.pointerId) {
      cropDrag.current = null;
      if (draftCropRef.current) props.onCommitCrop(draftCropRef.current);
    }
    viewport.endPan(event);
  }

  function beginCrop(event: React.PointerEvent<Element>, action: CropDragAction) {
    if (!effectiveCrop || event.button !== 0) return;
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    const start = point(event);
    cropDrag.current = { pointerId: event.pointerId, action, origin: effectiveCrop, x: start.x, y: start.y };
  }

  function beginRadial(event: React.PointerEvent<Element>, kind: "center" | "inner" | "outer" | "seam" | "seam_blend") {
    const radial = props.radialBehavior?.radial;
    if (!radial || event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    radialDrag.current = { pointerId: event.pointerId, kind, lastRadial: { ...radial }, lastOrientation: props.radialBehavior!.orientation };
  }

  function beginFrame(event: React.PointerEvent<SVGElement>, action: CropDragAction) {
    if (!props.sourceFrameEditing || !effectiveFrame || event.button !== 0) return;
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    const start = point(event);
    frameDrag.current = { pointerId: event.pointerId, action, origin: effectiveFrame, x: start.x, y: start.y };
  }

  function beginPatchPoint(event: React.PointerEvent<Element>, patch: Patch, corner: number) {
    if (event.button !== 0) return;
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    const geometry = draftPatch?.patchId === patch.id ? draftPatch.geometry : patch.geometry;
    patchDrag.current = { kind: "corner", pointerId: event.pointerId, patchId: patch.id, corner, corners: geometry.corners };
    setLoupePoint({ ...geometry.corners[corner]!, corner, clientX: event.clientX, clientY: event.clientY });
  }

  function beginPatchMove(event: React.PointerEvent<Element>, patch: Patch) {
    if (event.button !== 0 || props.tool) return;
    event.stopPropagation();
    props.onEditPatch(patch.id);
    event.currentTarget.setPointerCapture(event.pointerId);
    const geometry = draftPatch?.patchId === patch.id ? draftPatch.geometry : patch.geometry;
    patchDrag.current = { kind: "move", pointerId: event.pointerId, patchId: patch.id, start: point(event), corners: geometry.corners };
  }

  function beginPatchResize(event: React.PointerEvent<Element>, patch: Patch, handle: PatchResizeHandle) {
    if (event.button !== 0) return;
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    const geometry = draftPatch?.patchId === patch.id ? draftPatch.geometry : patch.geometry;
    patchDrag.current = { kind: "resize", pointerId: event.pointerId, patchId: patch.id, handle, corners: geometry.corners };
  }

  function beginPatchRotate(event: React.PointerEvent<Element>, patch: Patch, center: { x: number; y: number }) {
    if (event.button !== 0) return;
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    const geometry = draftPatch?.patchId === patch.id ? draftPatch.geometry : patch.geometry;
    patchDrag.current = {
      kind: "rotate", pointerId: event.pointerId, patchId: patch.id, center,
      lastAngle: patchPointerAngle(point(event), center, props.source!.orientedSize), lastValid: geometry.corners, corners: geometry.corners,
    };
  }

  function beginPatchCreate(event: React.PointerEvent<HTMLDivElement>) {
    if (event.button !== 0 || !props.tool) return;
    event.stopPropagation();
    const start = point(event);
    if (props.tool === "rectangle") {
      event.currentTarget.setPointerCapture(event.pointerId);
      patchCreate.current = { pointerId: event.pointerId, start };
      setDraftRectangle(rectangleGeometry(start, start));
      return;
    }
    setFourPointDraft((current) => {
      const next = [...current, start];
      if (next.length === 4) {
        props.onCreatePatch({ corners: [next[0]!, next[1]!, next[2]!, next[3]!] }, true);
        return [];
      }
      return next;
    });
  }

  function openPatchMenu(event: React.MouseEvent<Element>, patch: Patch) {
    event.preventDefault();
    event.stopPropagation();
    props.onEditPatch(patch.id);
    setPointEditPatchId(null);
    setPatchMenu({ patchId: patch.id, clientX: event.clientX, clientY: event.clientY });
  }

  function normalizePatch(patchId: string) {
    const patch = props.patches.find((candidate) => candidate.id === patchId);
    if (!patch || !props.source) return;
    const geometry = draftPatch?.patchId === patch.id ? draftPatch.geometry : patch.geometry;
    const normalizedGeometry = { corners: normalizePatchToRectangle(geometry.corners, props.source.orientedSize) };
    const normalizedDraft = { patchId: patch.id, geometry: normalizedGeometry };
    setPatchMenu(null);
    draftPatchRef.current = normalizedDraft;
    setDraftPatch(normalizedDraft);
    props.onDraftPatch(normalizedDraft);
    props.onCommitPatch(patch.id, normalizedGeometry);
  }

  const previewGeometry = props.tool === "four-point" && fourPointDraft.length
    ? { corners: [...fourPointDraft, ...Array.from({ length: 4 - fourPointDraft.length }, () => fourPointDraft.at(-1)!)] as unknown as PatchGeometry["corners"] }
    : draftRectangle;

  return <section
    ref={viewport.containerRef}
    className="source-canvas"
    onWheel={viewport.wheel}
    onPointerDown={viewport.beginPan}
    onPointerMove={movePointer}
    onPointerUp={endPointer}
    onPointerCancel={endPointer}
    onContextMenu={(event) => event.preventDefault()}
    onClick={(event) => { if (event.target === event.currentTarget && !props.tool) { setPointEditPatchId(null); props.onExitPatch(); } }}
  >
    {props.source ? <div
      ref={stageRef}
      className="source-stage"
      style={{ width: props.source.orientedSize.width, height: props.source.orientedSize.height, transform: `translate(${viewport.view.x}px, ${viewport.view.y}px) scale(${viewport.view.scale})` }}
      onPointerDown={beginPatchCreate}
    >
      <img
        src={props.source.thumbnailDataUrl}
        alt={`${channelLabel(props.source.channel)} source ${props.source.displayName}`}
        draggable={false}
        onClick={() => { if (!props.tool) { setPointEditPatchId(null); props.onExitPatch(); } }}
      />
      {props.sourceFrame && props.logicalGrid && props.partitionRegions.length > 0 ? <svg
        className="source-partition-overlay"
        viewBox={`0 0 ${props.source.orientedSize.width} ${props.source.orientedSize.height}`}
        aria-label="SourceFrame and accepted logical partition"
      >
        {props.partitionRegions.map((region) => region.sourceBounds ? <rect
          key={region.regionId}
          data-region-id={region.regionId}
          data-selection-surface="source-preview"
          aria-label={`Preview ${region.displayName}`}
          x={region.sourceBounds.x * props.source!.orientedSize.width}
          y={region.sourceBounds.y * props.source!.orientedSize.height}
          width={region.sourceBounds.width * props.source!.orientedSize.width}
          height={region.sourceBounds.height * props.source!.orientedSize.height}
          style={{ pointerEvents: "none" }}
          className={`source-region-boundary ${region.regionId === props.selectedRegion?.regionId ? "selected" : ""}`}
        /> : null)}
      </svg> : null}
      {props.selectedRegion && props.selectedSlot?.mappingOrigin === "partition" && !radialEditing && !patchEditing ? <div className="partition-selection-status" data-selection-status="partition-owned">
        Drag the crop to create a region patch
      </div> : null}
    </div> : <div className="empty-source-canvas">
      <strong>Open or drop a Diffuse texture</strong>
      <span>The source canvas is ready before the project has a save location.</span>
      <button className="primary" onClick={props.onOpenBase}>Open Diffuse</button>
    </div>}
    {props.source ? <svg
      className="patch-overlay"
      style={{ left: viewport.view.x, top: viewport.view.y, width: props.source.orientedSize.width * viewport.view.scale, height: props.source.orientedSize.height * viewport.view.scale }}
      viewBox={`0 0 ${props.source.orientedSize.width} ${props.source.orientedSize.height}`}
      aria-label="Editable patch outlines"
    >
      {props.sourceFrameEditing && effectiveFrame ? <g
        className={`patch-outline source-frame-transform ${props.sourceFrameEditing ? "active" : "source-frame-preview"}`}
        style={{ pointerEvents: props.sourceFrameEditing ? "auto" : "none" }}
      >
        <polygon
          points={`${effectiveFrame.x * props.source.orientedSize.width},${effectiveFrame.y * props.source.orientedSize.height} ${(effectiveFrame.x + effectiveFrame.width) * props.source.orientedSize.width},${effectiveFrame.y * props.source.orientedSize.height} ${(effectiveFrame.x + effectiveFrame.width) * props.source.orientedSize.width},${(effectiveFrame.y + effectiveFrame.height) * props.source.orientedSize.height} ${effectiveFrame.x * props.source.orientedSize.width},${(effectiveFrame.y + effectiveFrame.height) * props.source.orientedSize.height}`}
          aria-label={props.sourceFrameEditing ? "Move SourceFrame" : "SourceFrame preview"}
          onPointerDown={props.sourceFrameEditing ? (event) => beginFrame(event, "move") : undefined}
        />
        {props.sourceFrameEditing ? <g className="patch-transform">
          <rect
            className="rotation-guide"
            x={effectiveFrame.x * props.source.orientedSize.width - 15 / viewport.view.scale}
            y={effectiveFrame.y * props.source.orientedSize.height - 15 / viewport.view.scale}
            width={effectiveFrame.width * props.source.orientedSize.width + 30 / viewport.view.scale}
            height={effectiveFrame.height * props.source.orientedSize.height + 30 / viewport.view.scale}
          />
          {(["nw", "n", "ne", "e", "se", "s", "sw", "w"] as const).map((action) => <rect
            key={action}
            x={frameHandlePosition(effectiveFrame, action).x * props.source!.orientedSize.width - 5 / viewport.view.scale}
            y={frameHandlePosition(effectiveFrame, action).y * props.source!.orientedSize.height - 5 / viewport.view.scale}
            width={10 / viewport.view.scale}
            height={10 / viewport.view.scale}
            className={`resize-handle resize-${action}`}
            onPointerDown={(event) => beginFrame(event, action)}
          />)}
        </g> : null}
      </g> : null}
      {effectiveCrop && !patchEditing ? <g className="patch-outline active source-crop-transform">
        <polygon
          points={`${effectiveCrop.x * props.source.orientedSize.width},${effectiveCrop.y * props.source.orientedSize.height} ${(effectiveCrop.x + effectiveCrop.width) * props.source.orientedSize.width},${effectiveCrop.y * props.source.orientedSize.height} ${(effectiveCrop.x + effectiveCrop.width) * props.source.orientedSize.width},${(effectiveCrop.y + effectiveCrop.height) * props.source.orientedSize.height} ${effectiveCrop.x * props.source.orientedSize.width},${(effectiveCrop.y + effectiveCrop.height) * props.source.orientedSize.height}`}
          aria-label={`Move source crop for ${props.selectedRegion?.displayName ?? "selected region"}`}
          onPointerDown={(event) => beginCrop(event, "move")}
        />
        <g className="patch-transform">
          <rect
            className="rotation-guide"
            x={effectiveCrop.x * props.source.orientedSize.width - 15 / viewport.view.scale}
            y={effectiveCrop.y * props.source.orientedSize.height - 15 / viewport.view.scale}
            width={effectiveCrop.width * props.source.orientedSize.width + 30 / viewport.view.scale}
            height={effectiveCrop.height * props.source.orientedSize.height + 30 / viewport.view.scale}
          />
          {(["nw", "n", "ne", "e", "se", "s", "sw", "w"] as const).map((action) => <rect
            key={action}
            x={frameHandlePosition(effectiveCrop, action).x * props.source!.orientedSize.width - 5 / viewport.view.scale}
            y={frameHandlePosition(effectiveCrop, action).y * props.source!.orientedSize.height - 5 / viewport.view.scale}
            width={10 / viewport.view.scale}
            height={10 / viewport.view.scale}
            className={`resize-handle resize-${action}`}
            onPointerDown={(event) => beginCrop(event, action)}
          />)}
        </g>
      </g> : null}
      {props.radialBehavior?.role === "radial" && props.radialBehavior.radial && effectiveRadialSourceGeometry ? (() => {
        const radial = draftRadial ?? props.radialBehavior!.radial!;
        const orientation = draftRadialOrientation ?? props.radialBehavior!.orientation;
        const geometry = effectiveRadialSourceGeometry;
        const sourcePoint = (point: { x: number; y: number }) => { const mapped = mapUnitSquareToQuad(geometry.corners, point); return { x: mapped.x * props.source!.orientedSize.width, y: mapped.y * props.source!.orientedSize.height }; };
        const center = sourcePoint({ x: radial.centerX, y: radial.centerY });
        const angle = ({ zero: 0, ninety: Math.PI / 2, one_eighty: Math.PI, two_seventy: Math.PI * 1.5 } as const)[orientation];
        const ring = (radius: number) => Array.from({ length: 65 }, (_, index) => { const theta = index / 64 * Math.PI * 2; return sourcePoint({ x: radial.centerX + Math.cos(theta) * radius, y: radial.centerY + Math.sin(theta) * radius }); }).map((point) => `${point.x},${point.y}`).join(" ");
        const innerHandle = sourcePoint({ x: radial.centerX + radial.innerRadius, y: radial.centerY });
        const outerHandle = sourcePoint({ x: radial.centerX + radial.outerRadius, y: radial.centerY });
        const seamBlendAngle = radial.seamBlendWidth * Math.PI * 2;
        const seamBlendHandle = sourcePoint({ x: radial.centerX + Math.cos(angle + seamBlendAngle) * radial.outerRadius, y: radial.centerY + Math.sin(angle + seamBlendAngle) * radial.outerRadius });
        const seamBlendMirror = sourcePoint({ x: radial.centerX + Math.cos(angle - seamBlendAngle) * radial.outerRadius, y: radial.centerY + Math.sin(angle - seamBlendAngle) * radial.outerRadius });
        const seamRadius = radial.outerRadius + Math.max(0.035, 14 / Math.max(props.source!.orientedSize.width, props.source!.orientedSize.height) / viewport.view.scale);
        const seamHandle = sourcePoint({ x: radial.centerX + Math.cos(angle) * seamRadius, y: radial.centerY + Math.sin(angle) * seamRadius });
        const handle = 7 / viewport.view.scale;
        return <g className="radial-gizmo" aria-label="Selected region radial source gizmo">
          <polyline className="radial-ring inner" points={ring(radial.innerRadius)} />
          <polyline className="radial-ring outer" points={ring(radial.outerRadius)} />
          <line className="radial-seam" x1={center.x} y1={center.y} x2={seamHandle.x} y2={seamHandle.y} />
          {radial.seamBlendWidth > 0 ? <><line className="radial-seam feather" x1={center.x} y1={center.y} x2={seamBlendHandle.x} y2={seamBlendHandle.y} /><line className="radial-seam feather" x1={center.x} y1={center.y} x2={seamBlendMirror.x} y2={seamBlendMirror.y} /></> : null}
          <circle className="radial-hit center" cx={center.x} cy={center.y} r={handle * 1.7} onPointerDown={(event) => beginRadial(event, "center")}><title>Move radial center</title></circle>
          <circle className="radial-handle center" cx={center.x} cy={center.y} r={handle} />
          <circle className="radial-hit" cx={innerHandle.x} cy={innerHandle.y} r={handle * 1.7} onPointerDown={(event) => beginRadial(event, "inner")}><title>Adjust inner radius</title></circle>
          <circle className="radial-handle" cx={innerHandle.x} cy={innerHandle.y} r={handle} />
          <circle className="radial-hit" cx={outerHandle.x} cy={outerHandle.y} r={handle * 1.7} onPointerDown={(event) => beginRadial(event, "outer")}><title>Adjust outer radius</title></circle>
          <circle className="radial-handle" cx={outerHandle.x} cy={outerHandle.y} r={handle} />
          <circle className="radial-hit seam-blend" cx={seamBlendHandle.x} cy={seamBlendHandle.y} r={handle * 1.7} onPointerDown={(event) => beginRadial(event, "seam_blend")}><title>Adjust seam blend width</title></circle>
          <circle className="radial-handle seam-blend" cx={seamBlendHandle.x} cy={seamBlendHandle.y} r={handle} />
          <circle className="radial-hit seam" cx={seamHandle.x} cy={seamHandle.y} r={handle * 1.9} onPointerDown={(event) => beginRadial(event, "seam")}><title>Rotate seam in exact quarter turns</title></circle>
        </g>;
      })() : null}
      {props.patches.map((patch) => {
        const geometry = draftPatch?.patchId === patch.id ? draftPatch.geometry : patch.geometry;
        const active = props.activePatchId === patch.id;
        const pointEditing = pointEditPatchId === patch.id;
        const points = geometry.corners.map((corner) => `${corner.x * props.source!.orientedSize.width},${corner.y * props.source!.orientedSize.height}`).join(" ");
        const handleRadius = 8 / viewport.view.scale;
        const hitRadius = 15 / viewport.view.scale;
        const bounds = patchBounds(geometry.corners);
        const box = {
          left: bounds.left * props.source!.orientedSize.width,
          right: bounds.right * props.source!.orientedSize.width,
          top: bounds.top * props.source!.orientedSize.height,
          bottom: bounds.bottom * props.source!.orientedSize.height,
        };
        const boxHandles: ReadonlyArray<{ handle: PatchResizeHandle; x: number; y: number }> = [
          { handle: "nw", x: box.left, y: box.top },
          { handle: "n", x: (box.left + box.right) * 0.5, y: box.top },
          { handle: "ne", x: box.right, y: box.top },
          { handle: "e", x: box.right, y: (box.top + box.bottom) * 0.5 },
          { handle: "se", x: box.right, y: box.bottom },
          { handle: "s", x: (box.left + box.right) * 0.5, y: box.bottom },
          { handle: "sw", x: box.left, y: box.bottom },
          { handle: "w", x: box.left, y: (box.top + box.bottom) * 0.5 },
        ];
        const transformHandle = 10 / viewport.view.scale;
        const rotationInset = 15 / viewport.view.scale;
        const center = { x: (bounds.left + bounds.right) * 0.5, y: (bounds.top + bounds.bottom) * 0.5 };
        const patchInteractionEnabled = canInteractWithPatch(pointEditPatchId, patch.id);
        return <g key={patch.id} className={`patch-outline ${active ? "active" : ""} ${pointEditing ? "point-editing" : ""}`} style={patchInteractionEnabled ? undefined : { pointerEvents: "none" }} onContextMenu={patchInteractionEnabled ? (event) => openPatchMenu(event, patch) : undefined}>
          <polygon
            points={points}
            style={radialEditing && active ? { pointerEvents: "none" } : undefined}
            onPointerDown={!patchInteractionEnabled || (radialEditing && active) ? undefined : (event) => beginPatchMove(event, patch)}
            onClick={patchInteractionEnabled ? (event) => { event.stopPropagation(); props.onEditPatch(patch.id); } : undefined}
            onDoubleClick={patchInteractionEnabled ? (event) => { event.stopPropagation(); props.onEditPatch(patch.id); setPointEditPatchId(patch.id); } : undefined}
          />
          {active && !pointEditing ? <g className="patch-transform">
            <rect className="rotation-guide" x={box.left - rotationInset} y={box.top - rotationInset} width={box.right - box.left + rotationInset * 2} height={box.bottom - box.top + rotationInset * 2} />
            <rect className="rotation-hit" x={box.left - rotationInset} y={box.top - rotationInset} width={box.right - box.left + rotationInset * 2} height={box.bottom - box.top + rotationInset * 2} onPointerDown={(event) => beginPatchRotate(event, patch, center)}>
              <title>Drag the outer frame to rotate</title>
            </rect>
            {boxHandles.map(({ handle, x, y }) => <rect
              key={handle}
              x={x - transformHandle * 0.5}
              y={y - transformHandle * 0.5}
              width={transformHandle}
              height={transformHandle}
              className={`resize-handle resize-${handle}`}
              onPointerDown={(event) => beginPatchResize(event, patch, handle)}
            />)}
          </g> : null}
          {pointEditing ? geometry.corners.map((corner, index) => {
            const cx = corner.x * props.source!.orientedSize.width;
            const cy = corner.y * props.source!.orientedSize.height;
            return <g key={index} className="patch-point">
              <circle className="patch-point-hit" cx={cx} cy={cy} r={hitRadius} onPointerDown={(event) => beginPatchPoint(event, patch, index)} />
              <circle className="patch-point-visible" cx={cx} cy={cy} r={handleRadius} />
              <line className="patch-point-crosshair" x1={cx - handleRadius * 1.5} y1={cy} x2={cx + handleRadius * 1.5} y2={cy} />
              <line className="patch-point-crosshair" x1={cx} y1={cy - handleRadius * 1.5} x2={cx} y2={cy + handleRadius * 1.5} />
            </g>;
          }) : null}
        </g>;
      })}
      {previewGeometry ? <polygon className="patch-outline draft" points={previewGeometry.corners.map((corner) => `${corner.x * props.source!.orientedSize.width},${corner.y * props.source!.orientedSize.height}`).join(" ")} /> : null}
    </svg> : null}
    {props.source && loupePoint ? <div
      className="corner-loupe"
      style={cornerLoupeStyle(props.source, loupePoint, viewport.view.scale)}
      aria-live="off"
    >
      <span>Corner {loupePoint.corner + 1}</span>
      <i /><b />
    </div> : null}
    {patchMenu ? <div
      className="patch-context-menu"
      role="menu"
      style={{ left: Math.max(8, Math.min(patchMenu.clientX, window.innerWidth - 194)), top: Math.max(8, Math.min(patchMenu.clientY, window.innerHeight - 92)) }}
    >
      <button role="menuitem" onClick={() => normalizePatch(patchMenu.patchId)}>
        <span>Normalize to rectangle</span><small>Make edges square</small>
      </button>
      <hr />
      <button role="menuitem" className="danger" onClick={() => { const id = patchMenu.patchId; setPatchMenu(null); props.onDeletePatch(id); }}>
        <span>Delete patch</span><small>Delete</small>
      </button>
    </div> : null}
    {props.importing ? <div className="canvas-state"><strong>{props.importProgress?.stage ?? "Preparing source"}</strong><progress max={1} value={props.importProgress?.fraction ?? 0} /><button onClick={() => void invoke("cancel_import", { request: protocol })}>Cancel import</button></div> : null}
    {props.source ? <div className="viewport-tools">
      <button onClick={() => viewport.zoom(0.8)}>-</button>
      <output>{Math.round(viewport.view.scale * 100)}%</output>
      <button onClick={() => viewport.zoom(1.25)}>+</button>
      <button onClick={viewport.fit}>Fit</button>
    </div> : null}
  </section>;
}

function PatchPreview(props: { preview: PreviewSheetProjection; region: ResolvedRegion }) {
  const bounds = props.region.hotspotBounds;
  const scale = Math.min(220 / bounds.width, 150 / bounds.height);
  return <aside className="patch-preview">
    <header>Patch Preview</header>
    <div><img src={props.preview.dataUrl} alt="Selected patch draft render" style={{ width: props.preview.width * scale, height: props.preview.height * scale, transform: `translate(${-bounds.x * scale}px, ${-bounds.y * scale}px)` }} /></div>
  </aside>;
}

function GpuTiledPreviewCanvas(props: { artifact: IntermediateAtlasProjection; mapView: CompiledMapView; retainPayload?: boolean; onPaint: (dimensions: { width: number; height: number; generation?: number }) => void; onDebugSummary: (summary: GpuTiledPreviewPaintSummary | null) => void }) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const painter = useRef(new GpuTiledPreviewPainter());
  const onPaint = useRef(props.onPaint);
  const publication = gpuTilePublicationForView(props.artifact, props.mapView);
  const manifest = publication?.manifest;

  onPaint.current = props.onPaint;

  useEffect(() => () => painter.current.dispose(), []);
  useEffect(() => {
    const surface = canvasRef.current;
    if (!surface || !publication || !manifest || !gpuTiledPreviewMapMatches(manifest.map, props.mapView)) return;
    painter.current.beginGeneration(manifest.generation);
    void painter.current.paint(surface, publication, {
      getPayload: (request) => invoke<Uint8Array>("get_gpu_tiled_preview_payload", { request }),
      releasePayload: async (request) => { await invoke("release_gpu_tiled_preview_payload", { request }); },
    }, IPC_PROTOCOL_VERSION, !props.retainPayload).then((painted) => {
      props.onDebugSummary(painter.current.lastSummary());
      if (painted) onPaint.current({ width: props.artifact.width, height: props.artifact.height, generation: manifest.generation });
    });
  }, [manifest?.generation, manifest?.opaqueHandle, manifest?.map, props.artifact.height, props.artifact.width, props.mapView, props.retainPayload, publication, props.onDebugSummary]);

  return <canvas
    ref={canvasRef}
    data-gpu-preview-canvas="true"
    width={props.artifact.width}
    height={props.artifact.height}
    aria-label={`${props.mapView} trim sheet preview`}
  />;
}

async function writeClipboardText(text: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const textArea = document.createElement("textarea");
  textArea.value = text;
  textArea.style.position = "fixed";
  textArea.style.left = "-9999px";
  document.body.appendChild(textArea);
  textArea.focus();
  textArea.select();
  try {
    if (!document.execCommand("copy")) throw new Error("Clipboard copy failed");
  } finally {
    textArea.remove();
  }
}

function canvasPixelSummary(canvas: HTMLCanvasElement | null) {
  if (!canvas) return null;
  try {
    const context = canvas.getContext("2d", { willReadFrequently: true });
    if (!context) return { width: canvas.width, height: canvas.height, readable: false, reason: "2d context unavailable" };
    const data = context.getImageData(0, 0, canvas.width, canvas.height).data;
    let nonTransparent = 0;
    let nonZeroRgb = 0;
    for (let index = 0; index < data.length; index += 4) {
      if (data[index + 3] !== 0) nonTransparent += 1;
      if (data[index] !== 0 || data[index + 1] !== 0 || data[index + 2] !== 0) nonZeroRgb += 1;
    }
    return { width: canvas.width, height: canvas.height, readable: true, nonTransparent, nonZeroRgb };
  } catch (reason) {
    return { width: canvas.width, height: canvas.height, readable: false, reason: reason instanceof Error ? reason.message : String(reason) };
  }
}

function rectangleGeometry(start: { x: number; y: number }, end: { x: number; y: number }): PatchGeometry {
  const left = Math.min(start.x, end.x);
  const right = Math.max(start.x, end.x);
  const top = Math.min(start.y, end.y);
  const bottom = Math.max(start.y, end.y);
  return { corners: [{ x: left, y: top }, { x: right, y: top }, { x: right, y: bottom }, { x: left, y: bottom }] };
}

function rectangleArea(geometry: PatchGeometry): number {
  const [topLeft, topRight, bottomRight] = geometry.corners;
  return Math.abs((topRight.x - topLeft.x) * (bottomRight.y - topLeft.y));
}

function frameHandlePosition(bounds: NormalizedBounds, handle: CropDragAction) {
  return {
    x: handle.includes("w") ? bounds.x : handle.includes("e") ? bounds.x + bounds.width : bounds.x + bounds.width * 0.5,
    y: handle.includes("n") ? bounds.y : handle.includes("s") ? bounds.y + bounds.height : bounds.y + bounds.height * 0.5,
  };
}

function frameAspect(frame: SourceFrame, source: SourceProjection): number {
  return (frame.outputAspect[0] / Math.max(1, frame.outputAspect[1]))
    * source.orientedSize.height / Math.max(1, source.orientedSize.width);
}

function sourceCropAspect(slot: Stage14SlotProjection | null, sourceWidth: number, sourceHeight: number): number {
  const allocation = slot?.allocationBounds;
  if (!allocation || allocation.width <= 0 || allocation.height <= 0) return 1;
  return (allocation.width / allocation.height) * sourceHeight / Math.max(1, sourceWidth);
}

function cornerLoupeStyle(source: SourceProjection, point: { x: number; y: number; clientX: number; clientY: number }, viewportScale: number): React.CSSProperties {
  const magnification = Math.min(6, Math.max(1, viewportScale * 6));
  const width = source.orientedSize.width * magnification;
  const height = source.orientedSize.height * magnification;
  const left = point.clientX + 258 > window.innerWidth ? point.clientX - 258 : point.clientX + 18;
  const top = point.clientY + 198 > window.innerHeight ? point.clientY - 198 : point.clientY + 18;
  return {
    left: Math.max(8, left),
    top: Math.max(8, top),
    backgroundImage: `url(${source.thumbnailDataUrl})`,
    backgroundSize: `${width}px ${height}px`,
    backgroundPosition: `${120 - point.x * width}px ${90 - point.y * height}px`,
  };
}

function PaneSplitter(props: {
  kind: PaneDragKind;
  sourceOnly?: boolean;
  proportional?: boolean;
  libraryVisible?: boolean;
  inspectorVisible?: boolean;
  onSourceShareChange?: (share: number) => void;
  paneDrag: React.MutableRefObject<{ kind: PaneDragKind; start: PaneState } | null>;
  setPanes: (next: PaneState | ((current: PaneState) => PaneState)) => void;
  workbenchRef: React.RefObject<HTMLElement | null>;
}) {
  function down(event: React.PointerEvent<HTMLDivElement>) {
    event.currentTarget.setPointerCapture(event.pointerId);
    props.setPanes((current) => {
      props.paneDrag.current = { kind: props.kind, start: current };
      return current;
    });
  }
  function move(event: React.PointerEvent<HTMLDivElement>) {
    const active = props.paneDrag.current;
    if (!active || active.kind !== props.kind) return;
    const rect = props.workbenchRef.current?.getBoundingClientRect();
    if (!rect) return;
    if (props.proportional && props.onSourceShareChange) {
      const leftOffset = props.libraryVisible ? active.start.library + 6 : 0;
      const rightOffset = props.inspectorVisible ? active.start.inspector + 6 : 0;
      const available = Math.max(1, rect.width - leftOffset - rightOffset - 6);
      const share = Math.max(0.2, Math.min(0.8, (event.clientX - rect.left - leftOffset) / available));
      props.onSourceShareChange(share);
      return;
    }
    if (props.sourceOnly) {
      const maximum = Math.max(240, rect.width - 266);
      const source = Math.min(maximum, Math.max(240, event.clientX - rect.left));
      props.setPanes((current) => current.source === source ? current : { ...current, source });
      return;
    }
    props.setPanes(() => resizePanes(props.kind, active.start, event.clientX, rect.left, rect.width));
  }
  function up() {
    props.paneDrag.current = null;
  }
  return <div className="pane-splitter" onPointerDown={down} onPointerMove={move} onPointerUp={up} onPointerCancel={up} role="separator" aria-orientation="vertical" />;
}

function SheetWorkbench(props: {
  project: ProjectProjection | null;
  artifact: IntermediateAtlasProjection | null;
  preview: PreviewSheetProjection | null;
  preparedPatchPreview: PreparedPatchPreviewProjection | null;
  preparedPatchPreviews: Readonly<Record<string, PreparedPatchPreviewProjection>>;
  activePatchId: string | null;
  mapView: CompiledMapView;
  processingRequestedMap: CompiledMapView;
  setMapView: (view: CompiledMapView) => void;
  selectedRegionId: string | null;
  setSelectedRegionId: (id: string | null) => void;
  sourceFrameEditing: boolean;
  onEditSourceFrame: () => void;
  buildState: string;
  problem: CommandFailure | null;
  templateId: string;
  setTemplateId: (id: string) => void;
  primaryMaterial: string;
  build: () => void;
  renderFullResolutionPreview: () => void;
  interactivePreviewProfile: InteractivePreviewProfile;
  setInteractivePreviewProfile: (profile: InteractivePreviewProfile) => void;
  previewProgress: PreviewProgress | null;
  previewElapsedMs: number;
  exportProgress: NativeStage14ExportProgress | null;
  activity: Activity;
  setResolution: (size: number) => void;
  setAtlasPadding: (paddingPx: number) => void;
  targetRegionCount: number;
  setTargetRegionCount: (count: number) => void;
  regenerateSourceFrame: (count: number) => void;
  candidateRecipe: PartitionRecipe;
  setCandidateRecipe: React.Dispatch<React.SetStateAction<PartitionRecipe>>;
  candidatePreviewing: boolean;
  candidateIsCurrent: boolean;
  candidatePreviewRecipe: PartitionRecipe | null;
  previewCandidate: (recipe: PartitionRecipe) => void;
  discardCandidate: () => void;
  acceptCandidate: (recipe: PartitionRecipe) => void;
  onLayoutCommand: (command: TrimSheetDocumentCommand) => Promise<ProjectProjection | null>;
  onUndo: () => void;
  onRedo: () => void;
  presentation: "layout" | "processing";
  previewClientTelemetry: readonly string[];
  feedbackDebug: null | {
    view: FeedbackContributionView;
    profile: FeedbackPreviewProfile;
    comparisonMode: FeedbackComparisonMode;
    selectedOperationId: string | null;
    activeTool: "select" | "profile" | "stamp" | "stroke";
    lastCommandResult: string | null;
    error: CommandFailure | null;
    execution: FeedbackPreviewExecution | null;
    allRegions: boolean;
  };
  onPreviewPaint: (dimensions: { width: number; height: number; generation?: number }) => void;
}) {
  const processing = props.presentation === "processing";
  const authoringOverlaysVisible = !processing;
  const artifact = props.artifact;
  const topologyHash = props.project?.document ? hashBytes(props.project.document.topology.topologyHash) : null;
  const validPreview = props.preview
    && props.project?.document
    && props.preview.documentRevision === props.project.document.documentRevision
    && props.preview.topologyHash === topologyHash
    && props.preview.mapView === props.mapView
      ? props.preview
      : null;
  const sheet = validPreview ?? artifact;
  const imageUrl = validPreview?.dataUrl ?? artifact?.maps[props.mapView];
  const activeTilePublication = gpuTilePublicationForView(artifact, props.mapView);
  const activeTileManifest = activeTilePublication?.manifest;
  const tiledManifestMatchesMap = gpuTiledPreviewMapMatches(activeTileManifest?.map, props.mapView);
  const sheetMatchesDocument = !!sheet && sheet.topologyHash === topologyHash;
  const artifactRevisionMatchesDocument = !!props.project?.document
    && !!props.artifact
    && props.artifact.documentRevision === props.project.document.documentRevision;
  const displayedGrid = props.project?.document?.logicalGrid ?? diagonalCascadePreset.logicalGrid;
  // Generator state is retained only for legacy-document migration code below; it has no
  // visible product authority and no automatic preview effect.
  const hierarchical = props.candidateRecipe.hierarchical;
  const requestedFamilies = 0, requestedBudget = 0, requestedArea = 0, requestedFloor = 0, requestedMaximum = 0;
  const candidateValid = false;
  const candidateState = props.activity === "editing" ? "Editing" : "Authored";
  const [layoutMenu, setLayoutMenu] = useState<{ regionId: string; x: number; y: number } | null>(null);
  const [layoutSubmenu, setLayoutSubmenu] = useState<"source" | "settings" | null>(null);
  const [layoutTool, setLayoutTool] = useState<"select" | "draw">("select");
  const [gridVisible, setGridVisible] = useState(true);
  const [gridOpacity, setGridOpacity] = useState(10);
  const [textureVisible, setTextureVisible] = useState(true);
  const [processingMaterialView, setProcessingMaterialView] = useState(true);
  const [regionFillVisible, setRegionFillVisible] = useState(true);
  const [regionBordersVisible, setRegionBordersVisible] = useState(true);
  const [edgeEligibilityVisible, setEdgeEligibilityVisible] = useState(false);
  const [debugCopyStatus, setDebugCopyStatus] = useState<"idle" | "copied" | "failed">("idle");
  const [gpuPaintSummary, setGpuPaintSummary] = useState<GpuTiledPreviewPaintSummary | null>(null);
  const [hoverSnap, setHoverSnap] = useState<ReturnType<typeof snappedGridPoint> | null>(null);
  const [pendingGridChange, setPendingGridChange] = useState<{ preset: AuthoredLayoutPreset; size: number } | null>(null);
  const [userPresets, setUserPresets] = useState<AuthoredLayoutPreset[]>([]);
  const [presetLibraryProblem, setPresetLibraryProblem] = useState<string | null>(null);
  const nativePresetLibraryAvailable = useRef(true);
  const [resizeDraft, setResizeDraft] = useState<{ pointerId: number; regionId: string; handle: ResizeHandle; origin: LogicalRect; rect: LogicalRect } | null>(null);
  const [drawDraft, setDrawDraft] = useState<{ pointerId: number; startCellX: number; startCellY: number; endCellX: number; endCellY: number } | null>(null);
  useEffect(() => { if (processing) setProcessingMaterialView(true); }, [processing]);
  useEffect(() => {
    if (processing && props.interactivePreviewProfile === "preview8192") {
      props.setInteractivePreviewProfile("preview4096");
    }
  }, [processing, props.interactivePreviewProfile]);
  const sheetRef = useRef<HTMLDivElement>(null);
  const displayRegions = sheet?.regions ?? [];
  const selectedLayoutRegion = props.project?.document?.topology.regions.find((region) => region.id === props.selectedRegionId);
  const selectedLayoutBinding = props.selectedRegionId ? props.project?.document?.regionBindings[props.selectedRegionId] : undefined;
  const selectedLayoutSlot = props.selectedRegionId ? props.artifact?.slots.find((slot) => slot.regionId === props.selectedRegionId) : undefined;
  const selectedGridRect = resizeDraft?.rect ?? displayRegions.find((region) => region.regionId === props.selectedRegionId)?.gridRect;
  const pendingPatchRegions = props.artifact?.documentRevision !== props.project?.document?.documentRevision
    ? displayRegions.flatMap((region) => {
        const content = props.project?.document?.regionBindings[region.regionId]?.content;
        const patchPreview = content?.type === "patch" ? props.preparedPatchPreviews[content.id] : null;
        return patchPreview && region.gridRect ? [{ region, patchPreview }] : [];
      })
    : [];
  const drawRect = drawDraft ? cellDragRect(drawDraft.startCellX, drawDraft.startCellY, drawDraft.endCellX, drawDraft.endCellY) : null;
  const resizeTransfers = useMemo(() => resizeDraft ? previewResizeOwnershipTransfers(displayRegions, resizeDraft.regionId, resizeDraft.origin, resizeDraft.rect, displayedGrid) : [], [displayRegions, resizeDraft, displayedGrid]);
  const resizeAffectedIds = useMemo(() => new Set(resizeTransfers.flatMap((transfer) => [transfer.fromId, transfer.toId])), [resizeTransfers]);
  const sourceFrame = props.project?.document?.sourceFrame;
  const sourceTexture = sourceFrame
    ? props.project?.materialSources.find((source) => source.id === sourceFrame.sourceSetId)?.registeredChannels?.channels.find((channel) => channel.channel === "base_color")?.thumbnailDataUrl
    : null;
  const localTopologyPending = props.artifact?.telemetry.at(-1)?.startsWith("local topology edit:") ?? false;
  // Do not substitute Source Frame pixels for Stage 14. During local edits, keep the last
  // painted Stage 14 sheet visible until native publishes replacement pixels.
  const hasTransientSourceFallback = false;
  const currentTileGeneration = activeTileManifest?.generation;
  const gpuTilePaintedBlank = gpuPaintSummary?.generation === currentTileGeneration
    && !!gpuPaintSummary?.painted
    && gpuPaintSummary.validPayload
    && gpuPaintSummary.payloadNonTransparent === 0
    && gpuPaintSummary.payloadNonZeroRgb === 0;
  const continuousTexture = null;
  const displayGpuTiles = !!activeTilePublication && shouldDisplayGpuTiledPreview(activeTileManifest?.map, props.mapView, hasTransientSourceFallback);
  const litMaterialReady = processing
    && materialPreviewReady(props.artifact, props.project?.document?.documentRevision);
  const editorHasImage = processingMaterialView && processing
    ? litMaterialReady
    : !!continuousTexture || displayGpuTiles || !!imageUrl;
  const previewInFlight = props.previewProgress?.phase === "compiling" || props.previewProgress?.phase === "received";
  const previewPixelsProblem = !!sheet && !editorHasImage && !previewInFlight
    ? processingMaterialView && processing
      ? "The complete Base Color, Normal, Height, Roughness, Metallic, and AO set is not ready for material preview. Click Refresh to render it."
      : "The compiled preview published metadata, but no paintable tile was available for this map view."
    : null;
  const workpieceSize = sheet
    ? { width: sheet.width, height: sheet.height }
    : null;
  const viewport = useViewportController(workpieceSize, props.project?.document?.id ?? "no-document");
  useEffect(() => setGpuPaintSummary(null), [currentTileGeneration, activeTileManifest?.opaqueHandle, props.mapView]);
  const gridSteps = adaptiveGridSteps(displayedGrid, sheet?.width ?? 1, sheet?.height ?? 1, viewport.view.scale);
  useEffect(() => {
    let disposed = false;
    if (!isNativeRuntime()) {
      try { setUserPresets(JSON.parse(localStorage.getItem("hot-trimmer.authored-layout-presets.v1") ?? "[]") as AuthoredLayoutPreset[]); }
      catch { setUserPresets([]); }
      return;
    }
    void (async () => {
      try {
        let presets = await invoke<AuthoredLayoutPreset[]>("list_authored_layout_presets", { request: protocol });
        const legacy = JSON.parse(localStorage.getItem("hot-trimmer.authored-layout-presets.v1") ?? "[]") as AuthoredLayoutPreset[];
        for (const preset of legacy) if (!presets.some((saved) => saved.presetId === preset.presetId)) {
          presets = await invoke<AuthoredLayoutPreset[]>("save_authored_layout_preset", { request: { ...protocol, preset } });
        }
        localStorage.removeItem("hot-trimmer.authored-layout-presets.v1");
        if (!disposed) { setUserPresets(presets); setPresetLibraryProblem(null); }
      } catch (reason) {
        const message = failure(reason).message;
        if (/list_authored_layout_presets.*not found|command .*not found/i.test(message)) {
          nativePresetLibraryAvailable.current = false;
          try { if (!disposed) setUserPresets(JSON.parse(localStorage.getItem("hot-trimmer.authored-layout-presets.v1") ?? "[]") as AuthoredLayoutPreset[]); }
          catch { if (!disposed) setUserPresets([]); }
          if (!disposed) setPresetLibraryProblem(null);
        } else if (!disposed) setPresetLibraryProblem(message);
      }
    })();
    return () => { disposed = true; };
  }, []);
  async function saveUserPreset(preset: AuthoredLayoutPreset) {
    if (!isNativeRuntime() || !nativePresetLibraryAvailable.current) {
      const next = [...userPresets.filter((saved) => saved.presetId !== preset.presetId), preset];
      setUserPresets(next); localStorage.setItem("hot-trimmer.authored-layout-presets.v1", JSON.stringify(next)); return true;
    }
    try {
      setUserPresets(await invoke<AuthoredLayoutPreset[]>("save_authored_layout_preset", { request: { ...protocol, preset } }));
      setPresetLibraryProblem(null); return true;
    } catch (reason) { setPresetLibraryProblem(failure(reason).message); return false; }
  }
  async function deleteUserPreset(presetId: string) {
    if (!isNativeRuntime() || !nativePresetLibraryAvailable.current) {
      const next = userPresets.filter((saved) => saved.presetId !== presetId);
      setUserPresets(next); localStorage.setItem("hot-trimmer.authored-layout-presets.v1", JSON.stringify(next)); return true;
    }
    try {
      setUserPresets(await invoke<AuthoredLayoutPreset[]>("delete_authored_layout_preset", { request: { ...protocol, presetId } }));
      setPresetLibraryProblem(null); return true;
    } catch (reason) { setPresetLibraryProblem(failure(reason).message); return false; }
  }
  function applyPreset(preset: AuthoredLayoutPreset) {
    const instanceId = props.project?.document?.authoredLayoutInstanceId ?? props.project?.document?.id ?? crypto.randomUUID();
    void props.onLayoutCommand({ type: "apply_authored_layout_preset", preset, instanceId });
  }
  function changeGridResolution(size: number) {
    const snapshot = props.project?.document ? snapshotDocumentPreset(props.project.document, "embedded.grid-change", "Grid change") : diagonalCascadePreset;
    try {
      const quantized = rescalePreset(snapshot, size);
      if (quantized.exact) { setPendingGridChange(null); applyPreset(quantized.preset); }
      else setPendingGridChange({ preset: quantized.preset, size });
    } catch (reason) {
      setPendingGridChange(null);
      window.alert(reason instanceof Error ? reason.message : "The requested grid cannot represent this authored layout.");
    }
  }
  function pointerGridPoint(clientX: number, clientY: number) {
    const bounds = sheetRef.current?.getBoundingClientRect();
    if (!bounds) return null;
    return snappedGridPoint(clientX, clientY, bounds, displayedGrid.width, displayedGrid.height);
  }
  function moveDirectEdit(event: React.PointerEvent<HTMLElement>) {
    const point = pointerGridPoint(event.clientX, event.clientY);
    if (!point) return false;
    if (resizeDraft?.pointerId === event.pointerId) {
      setResizeDraft((current) => current ? { ...current, rect: resizeGridRect(current.origin, current.handle, point, displayedGrid) } : null);
      return true;
    }
    if (drawDraft?.pointerId === event.pointerId) {
      setDrawDraft((current) => current ? { ...current, endCellX: point.cellX, endCellY: point.cellY } : null);
      return true;
    }
    return false;
  }
  function finishDirectEdit(pointerId: number, cancel = false) {
    if (resizeDraft?.pointerId === pointerId) {
      const draft = resizeDraft;
      if (cancel || sameGridRect(draft.origin, draft.rect)) { setResizeDraft(null); return; }
      void props.onLayoutCommand({ type: "resize_source_frame_region", regionId: draft.regionId, gridRect: draft.rect })
        .then((next) => { if (next?.document?.topology.regions.some((region) => region.id === draft.regionId)) props.setSelectedRegionId(draft.regionId); })
        .finally(() => setResizeDraft(null));
      return;
    }
    if (drawDraft?.pointerId === pointerId) {
      const rect = cellDragRect(drawDraft.startCellX, drawDraft.startCellY, drawDraft.endCellX, drawDraft.endCellY);
      if (cancel) { setDrawDraft(null); return; }
      void props.onLayoutCommand({ type: "draw_source_frame_region", gridRect: rect }).then((next) => {
        const drawn = next?.document?.topology.regions.find((region) => region.gridRect && sameGridRect(region.gridRect, rect));
        if (drawn) props.setSelectedRegionId(drawn.id);
      }).finally(() => setDrawDraft(null));
    }
  }
  useEffect(() => {
    const cancel = (event: KeyboardEvent) => { if (event.key === "Escape") { setDrawDraft(null); setResizeDraft(null); setLayoutMenu(null); setLayoutSubmenu(null); } };
    window.addEventListener("keydown", cancel); return () => window.removeEventListener("keydown", cancel);
  }, []);
  useEffect(() => {
    const historyShortcut = (event: KeyboardEvent) => {
      if (!event.ctrlKey || event.altKey) return;
      const target = event.target as HTMLElement | null;
      if (target?.matches("input, textarea, select, [contenteditable=true]")) return;
      const key = event.key.toLowerCase();
      const undo = key === "z" && !event.shiftKey;
      const redo = key === "y" || (key === "z" && event.shiftKey);
      if (!undo && !redo) return;
      if (props.activity !== "idle" || (undo && !props.project?.canUndoDocument && !props.project?.canUndoPatch) || (redo && !props.project?.canRedoDocument && !props.project?.canRedoPatch)) return;
      event.preventDefault();
      if (undo) props.onUndo(); else props.onRedo();
    };
    window.addEventListener("keydown", historyShortcut);
    return () => window.removeEventListener("keydown", historyShortcut);
  }, [props.activity, props.onUndo, props.onRedo, props.project?.canRedoDocument, props.project?.canRedoPatch, props.project?.canUndoDocument, props.project?.canUndoPatch]);
  useEffect(() => {
    if (!layoutMenu) return;
    const dismiss = (event: PointerEvent) => {
      if (!(event.target as Element | null)?.closest(".layout-menu")) { setLayoutMenu(null); setLayoutSubmenu(null); }
    };
    const dismissBlur = () => setLayoutMenu(null);
    window.addEventListener("pointerdown", dismiss);
    window.addEventListener("blur", dismissBlur);
    return () => {
      window.removeEventListener("pointerdown", dismiss);
      window.removeEventListener("blur", dismissBlur);
    };
  }, [layoutMenu]);
  useEffect(() => setLayoutSubmenu(null), [layoutMenu?.x, layoutMenu?.y, layoutMenu?.regionId]);
  function currentVisibleAtlasRect(): PixelBounds | null {
    if (!sheet) return null;
    const bounds = viewport.containerRef.current?.getBoundingClientRect();
    if (!bounds || bounds.width <= 0 || bounds.height <= 0 || viewport.view.scale <= 0) return null;
    const left = Math.max(0, Math.floor((0 - viewport.view.x) / viewport.view.scale));
    const top = Math.max(0, Math.floor((0 - viewport.view.y) / viewport.view.scale));
    const right = Math.min(sheet.width, Math.ceil((bounds.width - viewport.view.x) / viewport.view.scale));
    const bottom = Math.min(sheet.height, Math.ceil((bounds.height - viewport.view.y) / viewport.view.scale));
    return right > left && bottom > top ? { x: left, y: top, width: right - left, height: bottom - top } : null;
  }
  async function copyPreviewDebugInfo() {
    const canvas = sheetRef.current?.querySelector<HTMLCanvasElement>('canvas[data-gpu-preview-canvas="true"]') ?? null;
    const manifest = activeTileManifest;
    const telemetry = [...(props.artifact?.telemetry ?? []), ...props.previewClientTelemetry];
    if (props.feedbackDebug && props.project?.document) {
      const dependency = visibleMapDependency(props.feedbackDebug.view);
      const requestRegionId = props.selectedRegionId
        ?? (props.feedbackDebug.allRegions ? props.project.document.topology.regions[0]?.id ?? null : null);
      const currentRequest: FeedbackPixelRequestIdentity | null = dependency && requestRegionId ? {
        revision: props.project.document.documentRevision,
        regionId: requestRegionId,
        allRegions: props.feedbackDebug.allRegions,
        view: props.feedbackDebug.view,
        map: dependency,
        profile: props.feedbackDebug.profile,
        comparisonMode: props.feedbackDebug.comparisonMode,
        selectedOperationId: props.feedbackDebug.selectedOperationId,
      } : null;
      const currentRequestIdentity = currentRequest ? feedbackPixelRequestIdentity(currentRequest) : null;
      const evidence = feedbackEvidenceForRequest(currentRequest, props.feedbackDebug.execution, activeTilePublication);
      const exactExecution = evidence.exactRequestEvidence;
      const progressIsCurrent = !!currentRequestIdentity && props.previewProgress?.feedbackRequestIdentity === currentRequestIdentity;
      const evidenceTile = evidence.tile;
      const evidencePaint = evidenceTile && gpuPaintSummary?.generation === evidenceTile.manifest.generation
        ? gpuPaintSummary
        : undefined;
      const previewState: FeedbackExecutionState = !dependency ? "InstalledNotRequested"
        : exactExecution ? props.feedbackDebug.execution!.outcome
          : progressIsCurrent && (props.previewProgress?.phase === "compiling" || props.previewProgress?.phase === "received") ? "Requested"
            : progressIsCurrent && props.previewProgress?.terminalOutcome === "superseded" ? "Superseded"
              : progressIsCurrent && props.feedbackDebug.error?.code === "operation_cancelled" ? "Cancelled"
                : progressIsCurrent && (props.feedbackDebug.error || props.previewProgress?.phase === "failed") ? "Failed"
                  : "InstalledNotRequested";
      const requestIdentity = exactExecution ? props.feedbackDebug.execution!.requestIdentity
        : currentRequestIdentity ?? JSON.stringify(["stage15-16-feedback-metadata-v1", props.project.document.documentRevision, props.selectedRegionId, props.feedbackDebug.allRegions, props.feedbackDebug.view, props.feedbackDebug.profile, props.feedbackDebug.comparisonMode, props.feedbackDebug.selectedOperationId]);
      const pixelDispatchCount = !dependency ? 0 : exactExecution || progressIsCurrent ? 1 : 0;
      const selectedInspection = props.artifact?.slots.find((slot) => slot.regionId === props.selectedRegionId);
      const request: Stage15To20DebugRequest = {
        ...protocol,
        schemaVersion: 1,
        selectedRegionId: props.selectedRegionId ?? undefined,
        requestedView: props.feedbackDebug.view,
        previewProfile: props.feedbackDebug.profile,
        comparisonMode: props.feedbackDebug.comparisonMode,
        selectedOperationId: props.feedbackDebug.selectedOperationId ?? undefined,
        activeTool: props.feedbackDebug.activeTool,
        previewState,
        requestIdentity,
        pixelDispatchCount,
        executionOutcome: previewState,
        previewError: props.feedbackDebug.error ?? undefined,
        lastCommandResult: props.feedbackDebug.lastCommandResult ?? undefined,
        paintSummary: evidencePaint,
        tile: evidenceTile,
        compiledInspection: selectedInspection ? { compiledProfile: selectedInspection.compiledProfile, compiledDetails: selectedInspection.compiledDetails } : undefined,
        workbenchState: {
          dirty: props.project.dirty,
          undoAvailable: props.project.canUndoDocument,
          redoAvailable: props.project.canRedoDocument,
          selectedOperationId: props.feedbackDebug.selectedOperationId,
          draftOperationId: null,
          committedOperationIds: props.project.feedbackAuthoring.records.map((record) => record.operationId),
          comparisonMode: props.feedbackDebug.comparisonMode,
          displayGates: { sheetMatchesDocument, artifactRevisionMatchesDocument, displayGpuTiles, gpuTilePaintedBlank, rawManifestMapMatchesMapView: tiledManifestMatchesMap, exactRequestEvidence: exactExecution, pixelDispatch: dependency ? (progressIsCurrent || exactExecution ? 1 : 0) : 0 },
        },
        boundedTelemetry: exactExecution ? telemetry : props.previewClientTelemetry,
      };
      try {
        const debug = await invoke<Stage15To20DebugPayload>("stage_15_20_debug_payload", { request });
        await writeClipboardText([debug.summary, JSON.stringify({ schema: debug.schema, schemaVersion: debug.schemaVersion, ...(debug.payload as Record<string, unknown>) }, null, 2)].join("\n\n"));
        setDebugCopyStatus("copied");
        window.setTimeout(() => setDebugCopyStatus("idle"), 1600);
      } catch {
        setDebugCopyStatus("failed");
      }
      return;
    }
    const debug = {
      capturedAt: new Date().toISOString(),
      activity: props.activity,
      mapView: props.mapView,
      interactivePreviewProfile: props.interactivePreviewProfile,
      progress: props.previewProgress,
      request: props.previewProgress ? {
        profile: props.previewProgress.profile,
        requestedRevision: props.previewProgress.requestedRevision,
        requestedMap: props.previewProgress.requestedMap,
        terminalOutcome: props.previewProgress.terminalOutcome ?? null,
      } : null,
      problem: props.problem,
      project: props.project?.document ? {
        documentRevision: props.project.document.documentRevision,
        topologyHash,
        renderSettings: props.project.document.renderSettings,
        sourceFrame: props.project.document.sourceFrame ? {
          sourceSetId: props.project.document.sourceFrame.sourceSetId,
          orientedDimensions: props.project.document.sourceFrame.orientedDimensions,
          sourceRevision: props.project.document.sourceFrame.sourceRevision,
        } : null,
      } : null,
      artifact: props.artifact ? {
        width: props.artifact.width,
        height: props.artifact.height,
        documentRevision: props.artifact.documentRevision,
        topologyHash: props.artifact.topologyHash,
        appearanceHash: props.artifact.appearanceHash,
        regions: props.artifact.regions.length,
        slots: props.artifact.slots.length,
        pending: props.artifact.pending,
        mapKeys: Object.keys(props.artifact.maps),
        selectedRegionId: props.selectedRegionId,
        selectedGridRect,
        selectedSlot: selectedLayoutSlot ? {
          allocationBounds: selectedLayoutSlot.allocationBounds,
          mappingMode: selectedLayoutSlot.mappingMode,
          sourceCrop: selectedLayoutSlot.sourceCrop,
          sourceBounds: selectedLayoutSlot.sourceBounds,
          addressMode: selectedLayoutSlot.addressMode,
        } : null,
      } : null,
      displayGate: {
        sheetPresent: !!sheet,
        validPreviewPresent: !!validPreview,
        sheetMatchesDocument,
        artifactRevisionMatchesDocument,
        textureVisible,
        regionFillVisible,
        regionBordersVisible,
        gridVisible,
        hasTransientSourceFallback,
        localTopologyPending,
        continuousTexturePresent: !!continuousTexture,
        gpuTilePaintedBlank,
        gpuPaintSummary,
        sourceTextureThumbnailPresent: !!sourceTexture,
        imageUrlPresent: !!imageUrl,
        imageUrlLength: imageUrl?.length ?? 0,
        displayGpuTiles,
        editorHasImage,
        previewPixelsProblem,
        rawManifestMapMatchesMapView: tiledManifestMatchesMap,
        normalizedManifestMapMatchesMapView: gpuTiledPreviewMapMatches(manifest?.map, props.mapView),
      },
      viewport: {
        view: viewport.view,
        visibleAtlasRect: currentVisibleAtlasRect(),
        container: viewport.containerRef.current ? {
          width: Math.round(viewport.containerRef.current.getBoundingClientRect().width),
          height: Math.round(viewport.containerRef.current.getBoundingClientRect().height),
        } : null,
      },
      gpuTile: activeTilePublication ?? null,
      canvas: canvasPixelSummary(canvas),
      telemetry,
    };
    const text = [
      "Hot Trimmer preview debug",
      JSON.stringify(debug, null, 2),
      "",
      "Telemetry",
      telemetry.join("\n"),
    ].join("\n");
    try {
      await writeClipboardText(text);
      setDebugCopyStatus("copied");
      window.setTimeout(() => setDebugCopyStatus("idle"), 1600);
    } catch {
      setDebugCopyStatus("failed");
    }
  }
  useEffect(() => {
    const copyShortcut = (event: KeyboardEvent) => {
      if (event.key !== "F2" || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
      event.preventDefault();
      void copyPreviewDebugInfo();
    };
    window.addEventListener("keydown", copyShortcut);
    return () => window.removeEventListener("keydown", copyShortcut);
  });
  const fullResolutionPreviewBusy = props.previewProgress?.profile === "authoritative"
    && (props.previewProgress.phase === "compiling" || props.previewProgress.phase === "received");
  const fullResolutionPreviewElapsedMs = props.previewProgress?.phase === "compiling"
    ? props.previewElapsedMs
    : props.previewProgress?.elapsedMs ?? props.previewElapsedMs;
  const fullResolutionPreviewSeconds = Math.max(0, Math.round(fullResolutionPreviewElapsedMs / 100) / 10);
  const fullResolutionPreviewStatus = props.previewProgress?.profile === "authoritative" && props.previewProgress.terminalOutcome
    ? props.previewProgress.terminalOutcome === "published"
      ? "Full-resolution preview succeeded"
      : props.previewProgress.terminalOutcome === "superseded"
        ? "Full-resolution preview superseded"
        : "Full-resolution preview failed"
    : null;
  return <section className={`sheet-workbench ${processing ? "processing-canvas" : "layout-canvas"}`}>
    <header className="sheet-header">
      <div><strong>{processing ? "PROCESSING" : "HOTSPOT SHEET"}</strong></div>
      <span className={`build-status ${props.problem ? "error" : props.artifact ? "ready" : ""}`}>{props.buildState}</span>
    </header>
    {processing ? <div className="processing-toolbar">
      <div className="processing-map-tabs" role="tablist" aria-label="Material maps"><button role="tab" aria-selected={processingMaterialView} className={processingMaterialView ? "active" : ""} onClick={() => setProcessingMaterialView(true)} title="Real-time lit preview composed from the current material maps.">Material</button>{([['baseColor','Base Color'],['normal','Normal'],['height','Height'],['roughness','Roughness'],['metallic','Metallic'],['ambientOcclusion','AO'],['edgeMask','Edge Mask']] as const).map(([view, label]) => <button key={view} role="tab" aria-selected={!processingMaterialView && props.processingRequestedMap === view} className={!processingMaterialView && props.processingRequestedMap === view ? "active" : ""} onClick={() => { setProcessingMaterialView(false); props.setMapView(view); }}>{label}</button>)}</div>
      <div className="processing-inspection-controls"><select aria-label="Before or After" value={props.feedbackDebug?.comparisonMode ?? "after"} disabled><option value="before">Before</option><option value="after">After</option></select><select aria-label="Preview resolution" title="Interactive complete-material previews are capped at 4K; Export All Maps uses the project output resolution." value={props.interactivePreviewProfile} onChange={(event) => props.setInteractivePreviewProfile(event.currentTarget.value as InteractivePreviewProfile)}><option value="refinement1024">1K</option><option value="preview2048">2K</option><option value="preview4096">4K</option></select><button onClick={props.renderFullResolutionPreview} disabled={!props.project?.document || props.activity !== "idle"}>Refresh All Maps</button></div>
    </div> : null}
    <section className="layout-stage">
    {!processing ? <aside className="layout-sidebar" aria-label="Layout controls">
      <header><span>LAYOUT</span><strong className={`layout-state ${candidateState.toLowerCase().replaceAll(" ", "-")}`}>{candidateState}</strong></header>
      {props.project?.document?.sourceFrame ? <fieldset className="trim-sheet-source-settings"><legend>Trim-sheet source</legend><p>Controls the source frame used by the entire authored layout.</p><button className={`source-frame-layout-action ${props.sourceFrameEditing ? "active" : ""}`} onClick={props.onEditSourceFrame}>{props.sourceFrameEditing ? "Editing entire trim-sheet source" : "Edit entire trim-sheet source position"}</button></fieldset> : null}
      <div className="layout-tool-row" role="toolbar" aria-label="Atlas editing tools"><button className={layoutTool === "select" ? "active" : ""} onClick={() => { setLayoutTool("select"); setDrawDraft(null); }}>Select / resize</button><button className={layoutTool === "draw" ? "active" : ""} onClick={() => { setLayoutTool("draw"); setLayoutMenu(null); }}>Draw region</button></div>
      <p className="layout-help">Draw snapped rectangles or resize the selected box with its handles. Middle-drag pans in either tool; pointer movement stays local and release publishes one refreshed artifact.</p>
      <div className="display-controls"><label><input type="checkbox" checked={textureVisible} onChange={(event) => setTextureVisible(event.target.checked)} /> Texture</label><label><input type="checkbox" checked={regionFillVisible} onChange={(event) => setRegionFillVisible(event.target.checked)} /> Region colors</label><label><input type="checkbox" checked={regionBordersVisible} onChange={(event) => setRegionBordersVisible(event.target.checked)} /> Region borders</label><label><input type="checkbox" checked={edgeEligibilityVisible} onChange={(event) => setEdgeEligibilityVisible(event.target.checked)} /> Edge eligibility</label></div>
      <div className="grid-controls"><label><input type="checkbox" checked={gridVisible} onChange={(event) => setGridVisible(event.target.checked)} /> Grid</label><label>Opacity <input aria-label="Grid opacity" type="range" min={0} max={100} value={gridOpacity} onChange={(event) => setGridOpacity(Number(event.target.value))} /></label></div>
      <fieldset className="preview-resolution-settings"><legend>Preview resolution</legend>
        <label>Size<select value={props.project?.document?.renderSettings.outputSize.width ?? 2048} onChange={(event) => void props.setResolution(Number(event.target.value))} disabled={!props.project?.document || props.activity !== "idle"}>
          <option value={1024}>1K · 1024</option><option value={2048}>2K · 2048</option><option value={4096}>4K · 4096</option><option value={8192}>8K · 8192</option>
        </select></label>
        <label>Preview<select value={props.interactivePreviewProfile} onChange={(event) => props.setInteractivePreviewProfile(event.currentTarget.value as InteractivePreviewProfile)} disabled={!props.project?.document || props.activity !== "idle"}>
          <option value="refinement1024">1K · 1024</option><option value="preview2048">2K · 2048</option><option value="preview4096">4K · 4096</option><option value="preview8192">8K · 8192</option>
        </select></label>
        <button className={`render-preview-button ${fullResolutionPreviewBusy ? "busy" : ""}`} onClick={props.renderFullResolutionPreview} disabled={!props.project?.document || props.activity !== "idle" || fullResolutionPreviewBusy} aria-busy={fullResolutionPreviewBusy}>{fullResolutionPreviewBusy ? `Rendering full-res... ${fullResolutionPreviewSeconds.toFixed(1)}s` : "Render full-resolution preview"}</button>
        {fullResolutionPreviewStatus ? <output className={`preview-terminal-status ${props.previewProgress?.terminalOutcome ?? ""}`}>{fullResolutionPreviewStatus}</output> : null}
        <small>Displayed now: {sheet ? `${sheet.width} × ${sheet.height}` : "not rendered"}. Edits keep the last painted sheet until the selected preview size is ready.</small>
      </fieldset>
      {selectedGridRect ? <output className="selection-readout">Selected: x {selectedGridRect.x}, y {selectedGridRect.y} · {selectedGridRect.width} × {selectedGridRect.height}</output> : drawRect ? <output className="selection-readout draw">Drawing: x {drawRect.x}, y {drawRect.y} · {drawRect.width} × {drawRect.height}</output> : <output className="selection-readout">No region selected</output>}
      <fieldset className="map-view-section hotspot-map-view"><legend>Map view</legend>
        <div className="map-view-grid">{mapViews.filter(([id]) => id !== "materialId").map(([id, label]) => {
          const available = materialMapRouteAvailable(id);
          const published = artifactMapAvailable(props.artifact, id);
          return <button key={id} className={props.mapView === id ? "active" : ""} onClick={() => props.setMapView(id)} disabled={!available} title={!available ? "Unavailable through Stage 14" : published ? `${label} published` : "Render this material map"}>{label}</button>;
        })}</div>
      </fieldset>
      {selectedLayoutRegion && selectedLayoutBinding ? <fieldset className="layout-region-settings"><legend>Selected region</legend>
        <strong>{selectedLayoutRegion.displayName}</strong>
        <label>Replace source<select value={selectedLayoutBinding.content.type === "material_source" ? `source:${selectedLayoutBinding.content.id}` : selectedLayoutBinding.content.type === "patch" ? `patch:${selectedLayoutBinding.content.id}` : selectedLayoutBinding.content.type === "solid" ? "solid" : "inherit"} onChange={(event) => { const value = event.currentTarget.value; const content: ContentReference = value === "inherit" ? { type: "inherit_primary_material" } : value === "solid" ? { type: "solid", id: { baseColor: [128, 128, 128, 255] } } : value.startsWith("source:") ? { type: "material_source", id: value.slice(7) } : { type: "patch", id: value.slice(6) }; void props.onLayoutCommand({ type: "set_region_content", regionId: selectedLayoutRegion.id, content }); }}><option value="inherit">Primary source</option><option value="solid">Solid gray</option>{props.project?.materialSources.map((source) => <optgroup key={source.id} label={source.name}><option value={`source:${source.id}`}>Whole source</option>{props.project?.patches.filter((patch) => patch.enabled && source.registeredChannels?.channels.some((channel) => channel.id === patch.sourceId)).map((patch) => <option key={patch.id} value={`patch:${patch.id}`}>{patch.name}</option>)}</optgroup>)}</select></label>
        <div className="layout-pair"><label>Role<select value={selectedLayoutBinding.mapping.behavior.role} onChange={(event) => void props.onLayoutCommand({ type: "set_region_behavior", regionId: selectedLayoutRegion.id, behavior: changedBehavior(selectedLayoutBinding.mapping.behavior, { role: event.currentTarget.value as ManualRegionRole }) })}>{manualRoleOptions.map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label><label>Continuity<select value={selectedLayoutBinding.mapping.behavior.continuity} onChange={(event) => void props.onLayoutCommand({ type: "set_region_behavior", regionId: selectedLayoutRegion.id, behavior: changedBehavior(selectedLayoutBinding.mapping.behavior, { continuity: event.currentTarget.value as RegionContinuity }) })}>{continuityOptions.map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label></div>
        <div className="layout-pair"><label>Sampling<select value={selectedLayoutBinding.mapping.behavior.sampling} onChange={(event) => void props.onLayoutCommand({ type: "set_region_behavior", regionId: selectedLayoutRegion.id, behavior: changedBehavior(selectedLayoutBinding.mapping.behavior, { sampling: event.currentTarget.value as RegionSampling }) })}>{samplingOptions.map(([value, label]) => <option key={value} value={value} disabled={!!samplingPrerequisite(selectedLayoutBinding.mapping.behavior.role, value)}>{label}</option>)}</select></label><label>Orientation<select value={selectedLayoutBinding.mapping.behavior.orientation} onChange={(event) => void props.onLayoutCommand({ type: "set_region_behavior", regionId: selectedLayoutRegion.id, behavior: changedBehavior(selectedLayoutBinding.mapping.behavior, { orientation: event.currentTarget.value as RegionBehavior["orientation"] }) })}><option value="zero">0°</option><option value="ninety">90°</option><option value="one_eighty">180°</option><option value="two_seventy">270°</option></select></label></div>
        <small className="region-settings-summary">Crop {boundsLabel(selectedLayoutSlot?.sourceCrop)} · eligible edges {Object.entries(selectedLayoutBinding.mapping.behavior.edgeEligibility).filter(([, eligible]) => eligible).map(([edge]) => edge).join(", ") || "none"}</small>
        {selectedLayoutBinding.mapping.behavior.role === "radial" && selectedLayoutBinding.mapping.radial ? <RadialEditor regionId={selectedLayoutRegion.id} radial={selectedLayoutBinding.mapping.radial} onApply={(regionId, radial) => { void props.onLayoutCommand({ type: "set_region_radial", regionId, radial }); }} /> : null}
      </fieldset> : null}
      <section className="layout-presets" aria-label="Authored layout presets">
        <strong>Authored preset</strong>
        {presetLibraryProblem ? <p className="layout-diagnostic" role="alert">{presetLibraryProblem}</p> : null}
        <select aria-label="Authored layout preset" value={props.project?.document?.authoredLayoutPreset?.presetId ?? diagonalCascadePreset.presetId} onChange={(event) => {
          const preset = [diagonalCascadePreset, newBlankPreset(displayedGrid.width), ...userPresets].find((value) => value.presetId === event.target.value); if (preset) applyPreset(preset);
        }}><option value={diagonalCascadePreset.presetId}>Diagonal Cascade (Built-in)</option><option value="builtin.new-blank">New Blank</option>{props.project?.document?.authoredLayoutPreset?.presetId === "embedded.grid-change" ? <option value="embedded.grid-change">Edited layout (Project)</option> : null}{userPresets.map((preset) => <option key={preset.presetId} value={preset.presetId}>{preset.name}</option>)}</select>
        <label className="grid-resolution-slider">Grid resolution <output>{displayedGrid.width} × {displayedGrid.height}</output><input aria-label="Logical grid resolution" type="range" min={0} max={authoredGridResolutions.length - 1} step={1} value={Math.max(0, authoredGridResolutions.indexOf(displayedGrid.width as typeof authoredGridResolutions[number]))} onChange={(event) => changeGridResolution(authoredGridResolutions[Number(event.target.value)]!)} /></label>
        {pendingGridChange ? <div className="grid-change-confirm" role="alert"><strong>Previewing {pendingGridChange.size} × {pendingGridChange.size}</strong><span>Some authored boundaries moved to the nearest non-overlapping grid lines. Review the amber rectangles before applying.</span><div><button onClick={() => { applyPreset(pendingGridChange.preset); setPendingGridChange(null); }}>Apply quantized grid</button><button onClick={() => setPendingGridChange(null)}>Cancel</button></div></div> : null}
        <div className="layout-actions"><button onClick={() => applyPreset(newBlankPreset(displayedGrid.width))}>New</button><button onClick={() => {
          if (!props.project?.document) return; const name = window.prompt("Preset name", `${props.project.document.authoredLayoutPreset?.name ?? "Layout"} Copy`); if (!name) return;
          const preset = snapshotDocumentPreset(props.project.document, `user.${crypto.randomUUID()}`, name); void saveUserPreset(preset).then((saved) => { if (saved) applyPreset(preset); });
        }}>Duplicate / Save As</button><button onClick={() => {
          const active = props.project?.document?.authoredLayoutPreset; if (!active || active.presetId.startsWith("builtin.")) return; const name = window.prompt("Rename preset", active.name); if (!name) return;
          const renamed = { ...active, name }; void saveUserPreset(renamed).then((saved) => { if (saved) void props.onLayoutCommand({ type: "set_authored_layout_preset_snapshot", preset: renamed }); });
        }} disabled={!props.project?.document?.authoredLayoutPreset || props.project.document.authoredLayoutPreset.presetId.startsWith("builtin.")}>Rename</button><button onClick={() => {
          const active = props.project?.document?.authoredLayoutPreset; if (!active || active.presetId.startsWith("builtin.") || !props.project?.document) return;
          const saved = snapshotDocumentPreset(props.project.document, active.presetId, active.name); void saveUserPreset(saved).then((persisted) => { if (persisted) void props.onLayoutCommand({ type: "set_authored_layout_preset_snapshot", preset: saved }); });
        }} disabled={!props.project?.document?.authoredLayoutPreset || props.project.document.authoredLayoutPreset.presetId.startsWith("builtin.")}>Save</button><button onClick={() => {
          const active = props.project?.document?.authoredLayoutPreset; const saved = active && userPresets.find((preset) => preset.presetId === active.presetId); if (saved) applyPreset(saved);
        }} disabled={!userPresets.some((preset) => preset.presetId === props.project?.document?.authoredLayoutPreset?.presetId)}>Revert</button><button onClick={() => {
          const active = props.project?.document?.authoredLayoutPreset; if (!active || active.presetId.startsWith("builtin.") || !window.confirm(`Delete ${active.name}?`)) return;
          void deleteUserPreset(active.presetId).then((deleted) => { if (deleted) applyPreset(diagonalCascadePreset); });
        }} disabled={!userPresets.some((preset) => preset.presetId === props.project?.document?.authoredLayoutPreset?.presetId)}>Delete</button></div>
      </section>
      <fieldset className="legacy-generator-controls" hidden aria-hidden="true"><p className={`layout-capacity ${candidateValid ? "" : "invalid"}`} aria-live="polite">{hierarchical ? `${requestedFloor}–${requestedMaximum} soft region range` : `${requestedFloor} minimum leaves / ${props.candidateRecipe.targetRegionCount} Count`} · {requestedArea / 10}% unified area</p>
      {props.problem ? <p className="layout-diagnostic" role="alert">{props.problem.message}<small>{props.problem.recovery}</small></p> : null}
      {previewPixelsProblem ? <p className="layout-diagnostic" role="alert">{previewPixelsProblem}</p> : null}
      <span>SOURCE FRAME PARTITION</span>
      <strong>{props.candidatePreviewing && props.candidateIsCurrent ? `${props.artifact?.regions.length ?? 0} candidate ready` : props.candidatePreviewing ? "Draft changed — preview again" : props.artifact ? `${props.artifact.regions.length} accepted regions` : "Accepted layout"}</strong>
      <output aria-live="polite">{hierarchical ? `${hierarchical.protectedParentCount} protected / ${hierarchical.subdividableParentCount} hierarchical parents` : `${requestedFamilies} requested / ${props.candidateRecipe.targetRegionCount} available`}{candidateValid ? "" : " — correct the highlighted recipe constraints"}</output>
      <label>Grid resolution<select aria-label="Logical grid resolution" value={props.candidateRecipe.grid.width} onChange={(event) => props.setCandidateRecipe((recipe) => recipeWithGridSize(recipe, Number(event.target.value)))} disabled={!props.project?.document}>{gridResolutionOptions.map((size) => <option key={size} value={size}>{size} × {size}</option>)}</select><small>One-cell snapping; visible lines adapt with zoom.</small></label>
      <label>Composition preset<select aria-label="Composition preset" value={selectedLayoutTemplate(props.candidateRecipe)} onChange={(event) => props.setCandidateRecipe((recipe) => layoutTemplateRecipe(recipe, event.target.value as LayoutTemplateId))}>{layoutTemplateOptions.map((option) => <option key={option.id} value={option.id}>{option.label}</option>)}</select><small>Hierarchical macro-zone recipes; every field below remains editable.</small></label>
      {hierarchical ? <HierarchicalRecipeControls recipe={props.candidateRecipe} setRecipe={props.setCandidateRecipe} /> : <>
      <label>Exact region count<input aria-label="Total regions" type="number" min={1} max={256} value={props.candidateRecipe.targetRegionCount} onChange={(event) => props.setCandidateRecipe((recipe) => ({ ...recipe, targetRegionCount: Number(event.target.value) }))} disabled={!props.project?.document} /></label>
      <label>Major region count<input aria-label="Major region count" type="number" min={0} max={32} value={props.candidateRecipe.composition.broadPanels.count} onChange={(event) => props.setCandidateRecipe((recipe) => ({ ...recipe, composition: { ...recipe.composition, broadPanels: { ...recipe.composition.broadPanels, count: Number(event.target.value) } } }))} /></label>
      <label>Major region total area<input aria-label="Major region total area" type="range" min={0} max={90} step={5} value={props.candidateRecipe.composition.broadPanels.areaShareMilli / 10} onChange={(event) => props.setCandidateRecipe((recipe) => ({ ...recipe, composition: { ...recipe.composition, broadPanels: { ...recipe.composition.broadPanels, areaShareMilli: Number(event.target.value) * 10 } } }))} /><output>{props.candidateRecipe.composition.broadPanels.areaShareMilli / 10}% total{props.candidateRecipe.composition.broadPanels.count ? ` · ~${(props.candidateRecipe.composition.broadPanels.areaShareMilli / 10 / props.candidateRecipe.composition.broadPanels.count).toFixed(1)}% each` : ""}</output></label>
      <label className="inline-check"><input type="checkbox" checked={props.candidateRecipe.composition.broadPanels.subdivisionBudget === 0} onChange={(event) => props.setCandidateRecipe((recipe) => ({ ...recipe, composition: { ...recipe.composition, broadPanels: { ...recipe.composition.broadPanels, subdivisionBudget: event.target.checked ? 0 : Math.max(1, recipe.composition.broadPanels.subdivisionBudget) } } }))} /> Keep major regions whole</label>
      <div className="layout-pair"><label>Medium blocks<input aria-label="Medium block count" type="number" min={0} max={64} value={props.candidateRecipe.composition.mediumBlocks.count} onChange={(event) => props.setCandidateRecipe((recipe) => ({ ...recipe, composition: { ...recipe.composition, mediumBlocks: { ...recipe.composition.mediumBlocks, count: Number(event.target.value) } } }))} /></label><label>Medium total area %<input aria-label="Medium block total area" type="number" min={0} max={100} value={props.candidateRecipe.composition.mediumBlocks.areaShareMilli / 10} onChange={(event) => props.setCandidateRecipe((recipe) => ({ ...recipe, composition: { ...recipe.composition, mediumBlocks: { ...recipe.composition.mediumBlocks, areaShareMilli: Number(event.target.value) * 10 } } }))} /></label></div>
      <div className="layout-pair"><label>Horizontal bands<input aria-label="Horizontal band count" type="number" min={0} max={64} value={props.candidateRecipe.composition.horizontalStrips.count} onChange={(event) => props.setCandidateRecipe((recipe) => ({ ...recipe, composition: { ...recipe.composition, horizontalStrips: { ...recipe.composition.horizontalStrips, count: Number(event.target.value) } } }))} /></label><label>Vertical trims<input aria-label="Vertical trim count" type="number" min={0} max={64} value={props.candidateRecipe.composition.verticalStrips.count} onChange={(event) => props.setCandidateRecipe((recipe) => ({ ...recipe, composition: { ...recipe.composition, verticalStrips: { ...recipe.composition.verticalStrips, count: Number(event.target.value) } } }))} /></label></div>
      <label>Minimum strip thickness<input aria-label="Minimum strip thickness in cells" type="number" min={1} max={props.candidateRecipe.grid.width} value={props.candidateRecipe.composition.horizontalStrips.minimumThickness} onChange={(event) => props.setCandidateRecipe((recipe) => recipeWithStripMinimum(recipe, Number(event.target.value)))} /><small>Grid cells; one cell is legal.</small></label>
      <label>Detail density<input aria-label="Detail density" type="range" min={0} max={16} value={props.candidateRecipe.composition.smallDetails.count} onChange={(event) => props.setCandidateRecipe((recipe) => ({ ...recipe, composition: { ...recipe.composition, smallDetails: { ...recipe.composition.smallDetails, count: Number(event.target.value) } } }))} /><output>{props.candidateRecipe.composition.smallDetails.count}</output></label>
      <label>Radial slot reservations<input aria-label="Radial slot reservation count" type="number" min={0} max={16} value={props.candidateRecipe.composition.radialReservations.count} onChange={(event) => props.setCandidateRecipe((recipe) => ({ ...recipe, composition: { ...recipe.composition, radialReservations: { ...recipe.composition.radialReservations, count: Number(event.target.value) } } }))} /><small>Square allocation slots; circular rendering arrives in Prompt 5.</small></label>
      <div className="layout-pair"><label>Seed<input aria-label="Layout seed" type="number" min={0} value={props.candidateRecipe.seed} onChange={(event) => props.setCandidateRecipe((recipe) => ({ ...recipe, seed: Number(event.target.value) }))} /></label><label>Variation %<input aria-label="Layout variation percent" type="number" min={0} max={100} value={props.candidateRecipe.varianceMilli / 10} onChange={(event) => props.setCandidateRecipe((recipe) => ({ ...recipe, varianceMilli: Number(event.target.value) * 10 }))} /></label></div>
      <details className="layout-advanced"><summary>Advanced geometry</summary>
        <div className="layout-pair"><label>Major min W<input type="number" min={1} max={props.candidateRecipe.grid.width} value={props.candidateRecipe.composition.broadPanels.minimumWidth} onChange={(event) => props.setCandidateRecipe((recipe) => updateFamilyQuota(recipe, "broadPanels", { minimumWidth: Number(event.target.value) }))} /><small>{cellPercent(props.candidateRecipe.composition.broadPanels.minimumWidth, props.candidateRecipe.grid.width)}%</small></label><label>Major min H<input type="number" min={1} max={props.candidateRecipe.grid.height} value={props.candidateRecipe.composition.broadPanels.minimumHeight} onChange={(event) => props.setCandidateRecipe((recipe) => updateFamilyQuota(recipe, "broadPanels", { minimumHeight: Number(event.target.value) }))} /><small>{cellPercent(props.candidateRecipe.composition.broadPanels.minimumHeight, props.candidateRecipe.grid.height)}%</small></label></div>
        <div className="layout-pair"><label>Major max W<input type="number" min={1} max={props.candidateRecipe.grid.width} value={props.candidateRecipe.composition.broadPanels.maximumWidth} onChange={(event) => props.setCandidateRecipe((recipe) => updateFamilyQuota(recipe, "broadPanels", { maximumWidth: Number(event.target.value) }))} /><small>{cellPercent(props.candidateRecipe.composition.broadPanels.maximumWidth, props.candidateRecipe.grid.width)}%</small></label><label>Major max H<input type="number" min={1} max={props.candidateRecipe.grid.height} value={props.candidateRecipe.composition.broadPanels.maximumHeight} onChange={(event) => props.setCandidateRecipe((recipe) => updateFamilyQuota(recipe, "broadPanels", { maximumHeight: Number(event.target.value) }))} /><small>{cellPercent(props.candidateRecipe.composition.broadPanels.maximumHeight, props.candidateRecipe.grid.height)}%</small></label></div>
        <label>Major subdivision density<input type="number" min={0} max={16} value={props.candidateRecipe.composition.broadPanels.subdivisionBudget} onChange={(event) => props.setCandidateRecipe((recipe) => updateFamilyQuota(recipe, "broadPanels", { subdivisionBudget: Number(event.target.value) }))} /></label>
        <label>Maximum strip thickness<input type="number" min={1} max={props.candidateRecipe.grid.width} value={props.candidateRecipe.composition.horizontalStrips.maximumThickness} onChange={(event) => props.setCandidateRecipe((recipe) => recipeWithStripMaximum(recipe, Number(event.target.value)))} /></label>
        <label>Detail total area %<input type="number" min={0} max={100} value={props.candidateRecipe.composition.smallDetails.areaShareMilli / 10} onChange={(event) => props.setCandidateRecipe((recipe) => updateFamilyQuota(recipe, "smallDetails", { areaShareMilli: Number(event.target.value) * 10 }))} /></label>
        <label>Detail subdivision density<input type="number" min={0} max={16} value={props.candidateRecipe.composition.smallDetails.subdivisionBudget} onChange={(event) => props.setCandidateRecipe((recipe) => updateFamilyQuota(recipe, "smallDetails", { subdivisionBudget: Number(event.target.value) }))} /></label>
        <div className="layout-pair"><label>Radial min diameter<input type="number" min={1} max={props.candidateRecipe.grid.width} value={props.candidateRecipe.composition.radialReservations.allocationMinDiameter} onChange={(event) => props.setCandidateRecipe((recipe) => recipeWithRadialDiameter(recipe, Number(event.target.value), recipe.composition.radialReservations.allocationMaxDiameter))} /></label><label>Radial max diameter<input type="number" min={1} max={props.candidateRecipe.grid.width} value={props.candidateRecipe.composition.radialReservations.allocationMaxDiameter} onChange={(event) => props.setCandidateRecipe((recipe) => recipeWithRadialDiameter(recipe, recipe.composition.radialReservations.allocationMinDiameter, Number(event.target.value)))} /></label></div>
      </details>
      </>}
      <footer className="layout-actions">
        <button onClick={() => props.previewCandidate(props.candidateRecipe)} disabled={!props.project?.document || props.activity !== "idle" || !candidateValid}>Update now</button>
        <button onClick={props.discardCandidate} disabled={!props.candidatePreviewing || props.activity !== "idle"}>Discard</button>
        <button className="primary" onClick={() => props.acceptCandidate(props.candidateRecipe)} disabled={!props.candidatePreviewing || !props.candidateIsCurrent || props.activity !== "idle"}>Accept</button>
      </footer>
      </fieldset>
      <details className="layout-advanced material-preview-settings"><summary>Optional material preview</summary>
        <label>Atlas padding (output px)<input aria-label="Atlas padding in output pixels" type="number" min={0} max={4096} value={props.project?.document?.renderSettings.atlasPaddingPx ?? 0} onChange={(event) => void props.setAtlasPadding(Number(event.currentTarget.value))} disabled={!props.project?.document} /><small>{(() => { const output = props.project?.document?.renderSettings.outputSize.width ?? 2048; const padding = props.project?.document?.renderSettings.atlasPaddingPx ?? 0; return `Draft 512: ${padding ? Math.max(1, Math.ceil(padding * 512 / output)) : 0}px · Refinement 1024: ${padding ? Math.max(1, Math.ceil(padding * 1024 / output)) : 0}px`; })()}</small></label>
        <button onClick={props.build} disabled={!props.project?.document || props.activity !== "idle"}>Rebuild material maps</button>
        <small>Layout drawing and resizing never require this rebuild.</small>
      </details>
      <section className="layout-editing">
        <strong>Direct atlas editing</strong>
        <p>{props.candidatePreviewing
          ? "This generated candidate is read-only. Accept it to resize, draw, split, or merge its regions."
          : layoutTool === "draw" ? "Drag anywhere on the atlas to place an exact snapped rectangle." : "Select a region, drag an edge handle continuously, or right-click for split/merge."}</p>
        {props.candidatePreviewing ? <button className="primary" onClick={() => props.acceptCandidate(props.candidateRecipe)} disabled={!props.candidateIsCurrent || props.activity !== "idle"}>Accept candidate and edit</button> : null}
        <button onClick={() => props.selectedRegionId && props.onLayoutCommand({ type: "split_source_frame_region", regionId: props.selectedRegionId, axis: "horizontal" })} disabled={!props.selectedRegionId || props.candidatePreviewing || props.activity !== "idle"}>Split horizontally</button>
        <button onClick={() => props.selectedRegionId && props.onLayoutCommand({ type: "split_source_frame_region", regionId: props.selectedRegionId, axis: "vertical" })} disabled={!props.selectedRegionId || props.candidatePreviewing || props.activity !== "idle"}>Split vertically</button>
      </section>
    </aside> : null}
    <section
      ref={viewport.containerRef}
      className="sheet-canvas"
      onWheel={viewport.wheel}
      onPointerDown={(event) => {
        viewport.beginPan(event);
        if (event.button === 0 && event.target === event.currentTarget) props.setSelectedRegionId(null);
      }}
      onPointerMove={(event) => { if (!moveDirectEdit(event)) viewport.movePan(event); }}
      onPointerUp={(event) => { finishDirectEdit(event.pointerId); viewport.endPan(event); }}
      onPointerCancel={(event) => { finishDirectEdit(event.pointerId, true); viewport.endPan(event); }}
    >
      {!sheet || !editorHasImage ? previewPixelsProblem ? <div className="empty-sheet source-texture-error"><strong>Preview pixels unavailable</strong><span>{previewPixelsProblem}</span></div> : <div className="empty-sheet">
        <strong>{previewInFlight ? "Rendering preview" : props.project?.legacyLayoutDiscarded ? "No trim sheet yet" : "No compiled sheet"}</strong>
        <span>{previewInFlight ? "The source is imported. The hotspot sheet will appear when the compiled preview publishes." : props.project?.legacyLayoutDiscarded ? "Sources, maps, and patches were preserved. Old layout state is not shown or converted." : "Build from the current Diffuse texture when ready."}</span>
      </div> : <div
        ref={sheetRef}
        className="sheet"
        style={{ width: sheet.width, height: sheet.height, transform: `translate(${viewport.view.x}px, ${viewport.view.y}px) scale(${viewport.view.scale})` }}
        onPointerMove={(event) => { if (layoutTool === "draw" && !drawDraft) setHoverSnap(pointerGridPoint(event.clientX, event.clientY)); }}
        onPointerLeave={() => { if (!drawDraft) setHoverSnap(null); }}
        onPointerDown={(event) => {
          if (event.button !== 0) return;
          if (layoutTool === "draw" && !props.candidatePreviewing && props.activity === "idle") {
            const point = pointerGridPoint(event.clientX, event.clientY);
            if (!point) return;
            event.preventDefault();
            event.currentTarget.setPointerCapture(event.pointerId);
            props.setSelectedRegionId(null);
            setDrawDraft({ pointerId: event.pointerId, startCellX: point.cellX, startCellY: point.cellY, endCellX: point.cellX, endCellY: point.cellY });
          } else if (!(event.target as Element).closest(".region")) props.setSelectedRegionId(null);
        }}
      >
        {processing && processingMaterialView && artifact ? <MaterialPreviewCanvas artifact={artifact} onPaint={props.onPreviewPaint} /> : textureVisible && continuousTexture && sourceFrame ? <div className="source-frame-texture"><img src={continuousTexture} alt="Source Frame texture" style={sourceFrameTextureStyle(sourceFrame)} onLoad={() => props.onPreviewPaint({ width: sheet.width, height: sheet.height })} /></div> : textureVisible && displayGpuTiles && artifact ? <GpuTiledPreviewCanvas artifact={artifact} mapView={props.mapView} retainPayload={processing} onPaint={props.onPreviewPaint} onDebugSummary={setGpuPaintSummary} /> : textureVisible && imageUrl ? <img src={imageUrl} alt={`${props.mapView} trim sheet preview`} onLoad={(event) => props.onPreviewPaint({ width: event.currentTarget.naturalWidth, height: event.currentTarget.naturalHeight })} /> : null}
        {authoringOverlaysVisible ? pendingPatchRegions.map(({ region, patchPreview }) => <div key={`assigned-${region.regionId}`} className="assigned-patch-preview" style={gridRectOverlayStyle(region.gridRect!, displayedGrid)}><img src={patchPreview.dataUrl} alt={`Immediate assigned patch preview for ${region.displayName}`} /></div>) : null}
        {authoringOverlaysVisible && layoutTool === "draw" && hoverSnap && !drawDraft ? <div className="draw-hover-cell" style={gridRectOverlayStyle({ x: hoverSnap.cellX, y: hoverSnap.cellY, width: 1, height: 1 }, displayedGrid)}><div className="draw-snap-crosshair"><span>cell {hoverSnap.cellX}, {hoverSnap.cellY}</span></div></div> : null}
        {authoringOverlaysVisible && (sheetMatchesDocument || props.candidatePreviewing) && gridVisible ? <><div className="sheet-grid minor" style={{ backgroundSize: `${gridSteps.minorX * 100 / displayedGrid.width}% ${gridSteps.minorY * 100 / displayedGrid.height}%`, opacity: gridOpacity / 100 * .9 }} /><div className="sheet-grid major" style={{ backgroundSize: `${gridSteps.majorX * 100 / displayedGrid.width}% ${gridSteps.majorY * 100 / displayedGrid.height}%`, opacity: gridOpacity / 100 }} /></> : null}
        {authoringOverlaysVisible ? <div className={`overlays ${layoutTool === "draw" ? "drawing" : ""}`}>{displayRegions.map((region) => <button key={region.regionId}
          data-region-id={region.regionId}
          data-selection-surface="atlas"
          aria-label={`${region.displayName}${region.gridRect ? `, x ${region.gridRect.x}, y ${region.gridRect.y}, ${region.gridRect.width} by ${region.gridRect.height}` : ""}`}
          aria-pressed={region.regionId === props.selectedRegionId}
          className={`region ${region.regionId === props.selectedRegionId ? "selected" : ""} ${resizeDraft && (region.regionId === resizeDraft.regionId || resizeAffectedIds.has(region.regionId)) ? "affected" : ""}`}
          style={overlayStyle(region, sheet, viewport.view.scale, regionFillVisible ? 0.2 : 0, regionBordersVisible ? 0.92 : 0)}
          onClick={(event) => { event.stopPropagation(); if (!props.candidatePreviewing && layoutTool !== "draw") props.setSelectedRegionId(region.regionId); }}
          onContextMenu={(event) => { event.preventDefault(); if (props.candidatePreviewing) return; event.stopPropagation(); props.setSelectedRegionId(region.regionId); setLayoutMenu({ regionId: region.regionId, x: Math.min(event.clientX, window.innerWidth - 196), y: Math.min(event.clientY, window.innerHeight - 156) }); }}
        >{(() => { const behavior = props.project?.document?.regionBindings[region.regionId]?.mapping.behavior ?? region.behavior; return <><span className="region-label">{region.displayName}</span><RegionBehaviorCue behavior={behavior} />{edgeEligibilityVisible ? <span className="edge-eligibility-overlay" aria-label={`Eligible edges: ${Object.entries(behavior.edgeEligibility).filter(([, value]) => value).map(([key]) => key).join(", ") || "none"}`}>{Object.entries(behavior.edgeEligibility).map(([edge, eligible]) => <i key={edge} className={`${edge} ${eligible ? "eligible" : "continuous"}`} />)}</span> : null}</>; })()}{region.regionId === props.selectedRegionId && region.gridRect && layoutTool === "select" && !props.candidatePreviewing ? resizeHandles.map((handle) => <i key={handle} className={`selection-handle ${handle}`} aria-label={`Resize ${handle}`} onPointerDown={(event) => { event.preventDefault(); event.stopPropagation(); event.currentTarget.setPointerCapture(event.pointerId); setResizeDraft({ pointerId: event.pointerId, regionId: region.regionId, handle, origin: region.gridRect!, rect: region.gridRect! }); }} />) : null}</button>)}</div> : null}
        {drawRect && drawRect.width > 0 && drawRect.height > 0 ? <div className="draw-region-preview" style={gridRectOverlayStyle(drawRect, displayedGrid)}><span>{drawRect.x}, {drawRect.y} · {drawRect.width} × {drawRect.height}</span></div> : null}
        {resizeTransfers.map((transfer, index) => { const owner = displayRegions.find((region) => region.regionId === transfer.toId); const from = displayRegions.find((region) => region.regionId === transfer.fromId); return <div key={`${transfer.fromId}-${transfer.toId}-${index}`} className={`ownership-transfer ${transfer.toId === resizeDraft?.regionId ? "gained" : "released"}`} style={{ ...gridRectOverlayStyle(transfer.rect, displayedGrid), borderColor: owner ? `rgb(${owner.idColor.join(" ")})` : undefined, backgroundColor: owner ? `rgb(${owner.idColor.join(" ")} / .38)` : undefined }}><span>{from?.displayName ?? "Region"} → {owner?.displayName ?? "Region"}</span></div>; })}
        {resizeDraft && !sameGridRect(resizeDraft.origin, resizeDraft.rect) ? <div className="resize-region-preview" style={gridRectOverlayStyle(resizeDraft.rect, displayedGrid)}><span>{resizeDraft.rect.x}, {resizeDraft.rect.y} / {resizeDraft.rect.width} x {resizeDraft.rect.height}</span></div> : null}
        {pendingGridChange ? <div className="grid-change-preview" aria-label={`Quantized ${pendingGridChange.size} by ${pendingGridChange.size} layout preview`}>{pendingGridChange.preset.regions.map((region) => <i key={region.presetRegionKey} style={gridRectOverlayStyle(region.gridRect, pendingGridChange.preset.logicalGrid)} />)}</div> : null}
        {!props.candidatePreviewing && layoutMenu ? createPortal(<div className="layout-menu" style={{ left: Math.min(layoutMenu.x, window.innerWidth - 390), top: layoutMenu.y }} role="menu" onPointerDown={(event) => event.stopPropagation()} onContextMenu={(event) => event.preventDefault()} onWheel={(event) => event.stopPropagation()}><button onClick={() => { props.onLayoutCommand({ type: "split_source_frame_region", regionId: layoutMenu.regionId, axis: "horizontal" }); setLayoutMenu(null); }}>Split horizontal</button><button onClick={() => { props.onLayoutCommand({ type: "split_source_frame_region", regionId: layoutMenu.regionId, axis: "vertical" }); setLayoutMenu(null); }}>Split vertical</button>{mergeCandidate(sheet.regions, layoutMenu.regionId) ? <><button onClick={() => { const sibling = mergeCandidate(sheet.regions, layoutMenu.regionId)!; props.onLayoutCommand({ type: "merge_source_frame_regions", regionId: layoutMenu.regionId, siblingId: sibling.regionId }); setLayoutMenu(null); }}>Merge / Remove Divider</button><button onClick={() => { const neighbor = mergeCandidate(sheet.regions, layoutMenu.regionId)!; props.onLayoutCommand({ type: "merge_source_frame_regions", regionId: neighbor.regionId, siblingId: layoutMenu.regionId }); setLayoutMenu(null); }}>Delete / Return area to neighbor</button></> : <button disabled title="No legal ownership transfer: this region does not share one complete divider with a neighbor.">Delete unavailable — no legal neighbor</button>}<hr/><button className="submenu-trigger" onPointerEnter={() => setLayoutSubmenu("source")} onFocus={() => setLayoutSubmenu("source")}>Replace source <b>›</b></button><button className="submenu-trigger" onPointerEnter={() => setLayoutSubmenu("settings")} onFocus={() => setLayoutSubmenu("settings")}>Region settings <b>›</b></button>{layoutSubmenu === "source" ? <div className="layout-submenu region-content-menu"><strong>Replace source</strong><button onClick={() => { props.onLayoutCommand({ type: "set_region_content", regionId: layoutMenu.regionId, content: { type: "inherit_primary_material" } }); setLayoutMenu(null); }}>Inherit primary</button>{props.project?.materialSources.map((source) => {
          const base = source.registeredChannels?.channels.find((channel) => channel.channel === "base_color");
          const sourcePatches = props.project?.patches.filter((patch) => patch.enabled && source.registeredChannels?.channels.some((channel) => channel.id === patch.sourceId)) ?? [];
          return <div className="content-source-group" key={source.id}><strong>{base?.displayName ?? source.name}</strong><button onClick={() => { props.onLayoutCommand({ type: "set_region_content", regionId: layoutMenu.regionId, content: { type: "material_source", id: source.id } }); setLayoutMenu(null); }}>Whole source</button>{sourcePatches.map((patch) => <button key={patch.id} onClick={() => { props.onLayoutCommand({ type: "set_region_content", regionId: layoutMenu.regionId, content: { type: "patch", id: patch.id } }); setLayoutMenu(null); }}>{patch.name}</button>)}</div>;
        })}</div> : null}{layoutSubmenu === "settings" ? (() => { const behavior = props.project?.document?.regionBindings[layoutMenu.regionId]?.mapping.behavior; if (!behavior) return null; const apply = (next: RegionBehavior) => { void props.onLayoutCommand({ type: "set_region_behavior", regionId: layoutMenu.regionId, behavior: next }); setLayoutMenu(null); }; return <div className="layout-submenu region-behavior-menu"><strong>Region settings</strong><label>Role<select value={behavior.role} onChange={(event) => apply(changedBehavior(behavior, { role: event.currentTarget.value as ManualRegionRole }))}>{manualRoleOptions.map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label><label>Continuity<select value={behavior.continuity} onChange={(event) => apply(changedBehavior(behavior, { continuity: event.currentTarget.value as RegionContinuity }))}>{continuityOptions.map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label><label>Sampling<select value={behavior.sampling} onChange={(event) => apply(changedBehavior(behavior, { sampling: event.currentTarget.value as RegionSampling }))}>{samplingOptions.map(([value, label]) => <option key={value} value={value} disabled={!!samplingPrerequisite(behavior.role, value)}>{label}</option>)}</select></label><label>Orientation<select value={behavior.orientation} onChange={(event) => apply(changedBehavior(behavior, { orientation: event.currentTarget.value as RegionBehavior["orientation"] }))}><option value="zero">0°</option><option value="ninety">90°</option><option value="one_eighty">180°</option><option value="two_seventy">270°</option></select></label></div>; })() : null}</div>, document.body) : null}
      </div>}
      {fullResolutionPreviewBusy ? <div className="preview-busy-overlay" role="status" aria-live="polite" aria-atomic="true">
        <strong>Rendering full-resolution preview</strong>
        <span>{props.previewProgress?.phase === "received" ? "Decoding and painting" : "Compiling, rendering, and encoding"} - {fullResolutionPreviewSeconds.toFixed(1)}s</span>
        <progress aria-label="Full-resolution preview in progress" />
      </div> : null}
      {props.activity === "exporting" ? <div className="preview-busy-overlay" role="status" aria-live="polite" aria-atomic="true">
        <strong>Exporting material maps</strong>
        <span>{props.exportProgress ? `Rendering and encoding ${props.exportProgress.map} - tile ${props.exportProgress.completedTiles}/${Math.max(props.exportProgress.totalTiles, 1)}` : "Compiling, rendering, and encoding"}</span>
        {props.exportProgress ? <progress aria-label="Material map export in progress" value={props.exportProgress.completedTiles} max={Math.max(props.exportProgress.totalTiles, 1)} /> : <progress aria-label="Material map export in progress" />}
      </div> : null}
      {workpieceSize ? <div className="viewport-tools">
        <button onClick={() => viewport.zoom(0.8)}>-</button>
        <output>{Math.round(viewport.view.scale * 100)}%</output>
        <button onClick={() => viewport.zoom(1.25)}>+</button>
        <button onClick={viewport.fit}>Fit</button>
      </div> : null}
    </section>
    </section>
    <footer className="artifact-footer">
      <PreviewProgressStatus progress={props.previewProgress} elapsedMs={props.previewElapsedMs} />
      {processing ? props.artifact ? <><span>{props.artifact.width} × {props.artifact.height}</span><span>{processingMaterialView ? "material" : props.mapView}</span><span>{props.artifact.documentRevision === props.project?.document?.documentRevision ? "Current" : "Updating"}</span></> : <span>No preview rendered</span> : props.artifact ? <><span>{props.artifact.width} x {props.artifact.height}</span>
      <span>{props.artifact.regions.length} regions</span>
      <span>{props.artifact.label}</span>
      <span>incomplete after Stage {props.artifact.incompleteAfterStage} · non-exportable</span>
      <span>pending: {props.artifact.pending.join(", ")}</span>
      <button className="copy-debug-button" onClick={() => void copyPreviewDebugInfo()} title="Copy preview telemetry, tile manifest, display gates, and canvas pixel summary. Shortcut: F2.">
        {debugCopyStatus === "copied" ? "Copied debug" : debugCopyStatus === "failed" ? "Copy failed" : props.feedbackDebug ? "Copy Stage 15-20 telemetry + debug" : "Copy telemetry + debug"}
      </button>
      {props.artifact.telemetry.length > 0 || props.previewClientTelemetry.length > 0 ? <details className="preview-telemetry">
        <summary>Preview telemetry</summary>
        <pre>{[...props.artifact.telemetry, ...props.previewClientTelemetry].join("\n")}</pre>
      </details> : null}</> : <><span>No preview rendered</span>{props.feedbackDebug ? <button className="copy-debug-button" onClick={() => void copyPreviewDebugInfo()} title="Copy Stage 15-20 telemetry and typed error state. Shortcut: F2.">{debugCopyStatus === "copied" ? "Copied debug" : debugCopyStatus === "failed" ? "Copy failed" : "Copy Stage 15-20 telemetry + debug"}</button> : null}</>}
    </footer>
  </section>;
}

function PreviewProgressStatus(props: { progress: PreviewProgress | null; elapsedMs: number }) {
  const progress = props.progress;
  if (!progress) return <div className="preview-progress idle"><span>Preview idle</span><progress value={0} max={1} /></div>;
  const elapsed = Math.round((progress.phase === "compiling" ? props.elapsedMs : progress.elapsedMs ?? 0) / 100) / 10;
  const dimensions = progress.dimensions ? ` · ${progress.dimensions.width}×${progress.dimensions.height}` : "";
  const target = progress.targetDimensions ? ` · target ${progress.targetDimensions.width}×${progress.targetDimensions.height}` : "";
  const superseded = progress.terminalOutcome === "superseded";
  const label = progress.phase === "compiling" ? `Rendering / encoding preview${target} · ${elapsed.toFixed(1)}s`
    : progress.phase === "received" ? `Preview received, decoding${dimensions} · ${elapsed.toFixed(1)}s`
      : progress.phase === "painted" ? `Preview ready${dimensions} · ${elapsed.toFixed(1)}s`
        : superseded ? `Preview superseded · ${elapsed.toFixed(1)}s`
          : `Preview failed · ${elapsed.toFixed(1)}s`;
  return <div className={`preview-progress ${progress.phase}${superseded ? " superseded" : ""}`}><span>{label}</span>{progress.phase === "painted"
    ? <progress value={1} max={1} aria-label="Preview complete" />
    : superseded ? <progress value={1} max={1} aria-label="Preview superseded" />
      : progress.phase === "failed" ? <progress value={0} max={1} aria-label="Preview failed" />
      : <progress aria-label="Preview compiling" />}</div>;
}

function HierarchicalRecipeControls(props: { recipe: PartitionRecipe; setRecipe: React.Dispatch<React.SetStateAction<PartitionRecipe>> }) {
  const hierarchy = props.recipe.hierarchical!;
  const aspectOptions = ["square", "wide2", "tall2", "wide4", "tall4", "wide8", "tall8"] as const;
  const symmetryOptions = ["identity", "rotate90", "rotate180", "rotate270", "mirror_x", "mirror_y", "mirror_diagonal", "mirror_anti_diagonal"] as const;
  return <>
    <label>Complexity<input aria-label="Layout complexity" type="range" min={24} max={80} value={hierarchy.targetRegionMax} onChange={(event) => props.setRecipe((recipe) => updateHierarchicalComplexity(recipe, Number(event.target.value)))} /><output>{hierarchy.targetRegionMin}–{hierarchy.targetRegionMax} regions</output></label>
    <label>Large panel share<input aria-label="Large panel share" type="range" min={20} max={85} value={hierarchy.largeShareMilli / 10} onChange={(event) => props.setRecipe((recipe) => updateHierarchicalShare(recipe, "largeShareMilli", Number(event.target.value) * 10))} /><output>{hierarchy.largeShareMilli / 10}%</output></label>
    <label>Strip share<input aria-label="Strip share" type="range" min={0} max={40} value={hierarchy.stripShareMilli / 10} onChange={(event) => props.setRecipe((recipe) => updateHierarchicalShare(recipe, "stripShareMilli", Number(event.target.value) * 10))} /><output>{hierarchy.stripShareMilli / 10}%</output></label>
    <label>Radial slots<input aria-label="Radial slots" type="number" min={0} max={4} value={hierarchy.radialCount} onChange={(event) => props.setRecipe((recipe) => updateHierarchicalRadialSlots(recipe, Number(event.target.value)))} /></label>
    <label>Orientation<select aria-label="Layout orientation" value={hierarchy.symmetry} onChange={(event) => props.setRecipe((recipe) => updateHierarchicalRecipe(recipe, { symmetry: event.target.value as typeof hierarchy.symmetry }))}>{symmetryOptions.map((option) => <option key={option} value={option}>{option.replaceAll("_", " ")}</option>)}</select></label>
    <label>Variation<input aria-label="Hierarchical variation" type="range" min={0} max={100} value={hierarchy.variationMilli / 10} onChange={(event) => props.setRecipe((recipe) => updateHierarchicalRecipe(recipe, { variationMilli: Number(event.target.value) * 10 }, { varianceMilli: Number(event.target.value) * 10 }))} /><output>{hierarchy.variationMilli / 10}%</output></label>
    <details className="layout-advanced"><summary>Advanced hierarchy</summary>
      <div className="layout-pair"><label>Hierarchy depth<input aria-label="Hierarchy depth" type="number" min={1} max={8} value={hierarchy.hierarchyDepth} onChange={(event) => props.setRecipe((recipe) => updateHierarchicalRecipe(recipe, { hierarchyDepth: Number(event.target.value) }))} /></label><label>Scale falloff %<input aria-label="Scale falloff" type="number" min={10} max={90} value={hierarchy.scaleFalloffMilli / 10} onChange={(event) => props.setRecipe((recipe) => updateHierarchicalRecipe(recipe, { scaleFalloffMilli: Number(event.target.value) * 10 }))} /></label></div>
      <div className="layout-pair"><label>Protected parents<input aria-label="Protected parent count" type="number" min={0} max={hierarchy.macroParentCount} value={hierarchy.protectedParentCount} onChange={(event) => props.setRecipe((recipe) => updateHierarchicalRecipe(recipe, { protectedParentCount: Number(event.target.value) }))} /></label><label>Subdividable parents<input aria-label="Subdividable parent count" type="number" min={0} max={hierarchy.macroParentCount} value={hierarchy.subdividableParentCount} onChange={(event) => props.setRecipe((recipe) => updateHierarchicalRecipe(recipe, { subdividableParentCount: Number(event.target.value) }))} /></label></div>
      <label>Alignment strength<input aria-label="Alignment strength" type="range" min={0} max={100} value={hierarchy.alignmentStrengthMilli / 10} onChange={(event) => props.setRecipe((recipe) => updateHierarchicalRecipe(recipe, { alignmentStrengthMilli: Number(event.target.value) * 10 }))} /><output>{hierarchy.alignmentStrengthMilli / 10}%</output></label>
      <fieldset><legend>Split ratio palette</legend>{(["half", "one_third", "two_third"] as const).map((ratio) => <label className="inline-check" key={ratio}><input type="checkbox" checked={hierarchy.allowedSplitRatios.includes(ratio)} onChange={() => props.setRecipe((recipe) => toggleHierarchicalSplitRatio(recipe, ratio))} />{ratio === "half" ? "1/2" : ratio === "one_third" ? "1/3" : "2/3"}</label>)}</fieldset>
      <label>Strip thickness ladder<input aria-label="Strip thickness ladder" value={hierarchy.stripThicknessLadder.join(",")} onChange={(event) => props.setRecipe((recipe) => updateHierarchicalRecipe(recipe, { stripThicknessLadder: parseThicknessLadder(event.target.value) }))} /><small>Comma-separated logical cells.</small></label>
      <label>Major aspect palette<select multiple aria-label="Major aspect palette" value={hierarchy.majorAspects} onChange={(event) => props.setRecipe((recipe) => updateHierarchicalRecipe(recipe, { majorAspects: Array.from(event.target.selectedOptions, (option) => option.value as typeof hierarchy.majorAspects[number]) }))}>{aspectOptions.map((aspect) => <option key={aspect} value={aspect}>{aspect}</option>)}</select></label>
      <label>Medium aspect palette<select multiple aria-label="Medium aspect palette" value={hierarchy.mediumAspects} onChange={(event) => props.setRecipe((recipe) => updateHierarchicalRecipe(recipe, { mediumAspects: Array.from(event.target.selectedOptions, (option) => option.value as typeof hierarchy.mediumAspects[number]) }))}>{aspectOptions.map((aspect) => <option key={aspect} value={aspect}>{aspect}</option>)}</select></label>
      <label>Detail aspect palette<select multiple aria-label="Detail aspect palette" value={hierarchy.detailAspects} onChange={(event) => props.setRecipe((recipe) => updateHierarchicalRecipe(recipe, { detailAspects: Array.from(event.target.selectedOptions, (option) => option.value as typeof hierarchy.detailAspects[number]) }))}>{aspectOptions.map((aspect) => <option key={aspect} value={aspect}>{aspect}</option>)}</select></label>
      <div className="layout-pair"><label>Soft region minimum<input aria-label="Soft region minimum" type="number" min={24} max={hierarchy.targetRegionMax} value={hierarchy.targetRegionMin} onChange={(event) => props.setRecipe((recipe) => updateHierarchicalRecipe(recipe, { targetRegionMin: Number(event.target.value) }))} /></label><label>Soft region maximum<input aria-label="Soft region maximum" type="number" min={Math.max(24, hierarchy.targetRegionMin)} max={256} value={hierarchy.targetRegionMax} onChange={(event) => props.setRecipe((recipe) => updateHierarchicalRecipe(recipe, { targetRegionMax: Number(event.target.value) }, { targetRegionCount: Number(event.target.value) }))} /></label></div>
    </details>
  </>;
}

function updateHierarchicalRecipe(recipe: PartitionRecipe, patch: Partial<NonNullable<PartitionRecipe["hierarchical"]>>, recipePatch: Partial<PartitionRecipe> = {}): PartitionRecipe {
  if (!recipe.hierarchical) return recipe;
  return { ...recipe, ...recipePatch, hierarchical: { ...recipe.hierarchical, ...patch } };
}

function updateHierarchicalComplexity(recipe: PartitionRecipe, maximum: number): PartitionRecipe {
  const targetRegionMax = Math.max(24, Math.min(256, maximum));
  const targetRegionMin = Math.max(24, Math.min(targetRegionMax, Math.round(targetRegionMax * 0.75)));
  return updateHierarchicalRecipe(recipe, { targetRegionMin, targetRegionMax }, { targetRegionCount: targetRegionMax });
}

function updateHierarchicalShare(recipe: PartitionRecipe, field: "largeShareMilli" | "stripShareMilli", requested: number): PartitionRecipe {
  const hierarchy = recipe.hierarchical;
  if (!hierarchy) return recipe;
  const other = hierarchy.smallShareMilli + hierarchy.radialShareMilli + (field === "largeShareMilli" ? hierarchy.stripShareMilli : hierarchy.largeShareMilli);
  const value = Math.max(0, Math.min(1_000 - other, requested));
  return updateHierarchicalRecipe(recipe, { [field]: value, mediumShareMilli: 1_000 - other - value });
}

function updateHierarchicalRadialSlots(recipe: PartitionRecipe, count: number): PartitionRecipe {
  const hierarchy = recipe.hierarchical;
  if (!hierarchy) return recipe;
  const radialCount = Math.max(0, Math.min(4, count));
  if (radialCount === 0) return updateHierarchicalRecipe(recipe, { radialCount, radialShareMilli: 0, largeShareMilli: hierarchy.largeShareMilli + hierarchy.radialShareMilli });
  if (hierarchy.radialShareMilli > 0) return updateHierarchicalRecipe(recipe, { radialCount });
  const radialShareMilli = Math.min(100, hierarchy.mediumShareMilli);
  return updateHierarchicalRecipe(recipe, { radialCount, radialShareMilli, mediumShareMilli: hierarchy.mediumShareMilli - radialShareMilli });
}

function toggleHierarchicalSplitRatio(recipe: PartitionRecipe, ratio: NonNullable<PartitionRecipe["hierarchical"]>["allowedSplitRatios"][number]): PartitionRecipe {
  const hierarchy = recipe.hierarchical;
  if (!hierarchy) return recipe;
  const allowedSplitRatios = hierarchy.allowedSplitRatios.includes(ratio) ? hierarchy.allowedSplitRatios.filter((value) => value !== ratio) : [...hierarchy.allowedSplitRatios, ratio];
  return updateHierarchicalRecipe(recipe, { allowedSplitRatios });
}

function parseThicknessLadder(value: string) { return value.split(",").map((part) => Number(part.trim())).filter((part) => Number.isInteger(part) && part > 0); }

function Inspector(props: {
  project: ProjectProjection | null;
  artifact: IntermediateAtlasProjection | null;
  sourceAnalysis: PreparedPatchPreviewProjection | null;
  selectedRegion: ResolvedRegion | null;
  mapView: CompiledMapView;
  setMapView: (view: CompiledMapView) => void;
  onUndo: () => void;
  onRedo: () => void;
  onClassify: (materialSourceId: string, command: MaterialClassificationCommand) => void;
  onCalibrate: (materialSourceId: string, command: MaterialCalibrationCommand) => void;
  onSetRadial: (regionId: string, radial: NonNullable<RegionMapping["radial"]>) => void;
  onResizeRegion: (regionId: string, gridRect: LogicalRect) => void;
  onSetSourceFrame: (bounds: NormalizedBounds) => void;
  sourceFrameEditing: boolean;
  onSetSourceFrameEditing: (editing: boolean) => void;
  selectedSourceSetId: string;
  onSetExemplarGroup: (materialSourceId: string, exemplarGroup: string | null) => void;
  onSetDelightingIntent: (materialSourceId: string, delighting: DelightingIntent) => void;
  onSetRegionContent: (regionId: string, content: ContentReference) => void;
  onSetRegionBehavior: (regionId: string, behavior: RegionBehavior) => void;
}) {
  const binding = props.selectedRegion && props.project?.document?.regionBindings[props.selectedRegion.regionId];
  const stage14Slot = props.selectedRegion && props.artifact?.slots.find((slot) => slot.regionId === props.selectedRegion!.regionId);
  const overlapIds = stage14Slot?.mappingOrigin === "explicit_override" && props.selectedRegion?.sourceBounds
    ? props.artifact?.regions.filter((region) => region.regionId !== props.selectedRegion!.regionId && region.sourceBounds && normalizedBoundsOverlap(props.selectedRegion!.sourceBounds!, region.sourceBounds)).map((region) => region.regionId) ?? []
    : [];
  const analyzedSource = props.sourceAnalysis
    ? props.project?.materialSources.find((source) => source.id === props.sourceAnalysis!.materialSourceId)
    : undefined;
  const layoutMode = !!props.project?.document?.sourceFrame;
  const selectedMaterial = props.project?.materialSources.find((source) => source.id === props.selectedSourceSetId);
  const boundContent = binding?.content;
  const boundPatch = boundContent?.type === "patch" ? props.project?.patches.find((patch) => patch.id === boundContent.id) : undefined;
  const boundSource = boundContent?.type === "material_source"
    ? props.project?.materialSources.find((source) => source.id === boundContent.id)
    : boundContent?.type === "patch"
      ? props.project?.materialSources.find((source) => source.registeredChannels?.channels.some((channel) => channel.id === boundPatch?.sourceId))
      : props.project?.materialSources.find((source) => source.id === props.project?.document?.primaryMaterial);
  return <aside className={`context-inspector ${layoutMode ? "layout-mode" : ""}`}>
    <header className="inspector-actions"><button onClick={props.onUndo} disabled={!props.project?.canUndoDocument && !props.project?.canUndoPatch}>Undo</button><button onClick={props.onRedo} disabled={!props.project?.canRedoDocument && !props.project?.canRedoPatch}>Redo</button></header>
    {layoutMode && !props.selectedRegion ? <section className="inspector-section layout-summary"><span>REGION INSPECTOR</span><p>Select a region to replace its source and edit its mapping settings here.</p></section> : null}
    {props.selectedRegion ? <section className="inspector-section region-controls-primary">
      <span>REGION SETTINGS</span><h2>{props.selectedRegion.displayName}</h2>
      <label>Content source<select value={binding?.content.type === "material_source" ? `source:${binding.content.id}` : binding?.content.type === "patch" ? `patch:${binding.content.id}` : binding?.content.type === "solid" ? "solid" : "inherit"} onChange={(event) => { const value = event.currentTarget.value; if (value === "inherit") props.onSetRegionContent(props.selectedRegion!.regionId, { type: "inherit_primary_material" }); else if (value === "solid") props.onSetRegionContent(props.selectedRegion!.regionId, { type: "solid", id: { baseColor: [128, 128, 128, 255] } }); else if (value.startsWith("source:")) props.onSetRegionContent(props.selectedRegion!.regionId, { type: "material_source", id: value.slice(7) }); else if (value.startsWith("patch:")) props.onSetRegionContent(props.selectedRegion!.regionId, { type: "patch", id: value.slice(6) }); }}><option value="inherit">Primary source</option><option value="solid">Solid gray</option>{props.project?.materialSources.map((source) => <optgroup key={source.id} label={source.registeredChannels?.channels.find((channel) => channel.channel === "base_color")?.displayName ?? source.name}><option value={`source:${source.id}`}>Whole source</option>{props.project?.patches.filter((patch) => patch.enabled && source.registeredChannels?.channels.some((channel) => channel.id === patch.sourceId)).map((patch) => <option key={patch.id} value={`patch:${patch.id}`}>{patch.name}</option>)}</optgroup>)}</select></label>
      {props.selectedRegion.gridRect && props.project?.document?.logicalGrid ? <RegionGridRectEditor
        key={`${props.selectedRegion.regionId}:${props.selectedRegion.gridRect.x}:${props.selectedRegion.gridRect.y}:${props.selectedRegion.gridRect.width}:${props.selectedRegion.gridRect.height}`}
        regionId={props.selectedRegion.regionId}
        gridRect={props.selectedRegion.gridRect}
        grid={props.project.document.logicalGrid}
        onApply={props.onResizeRegion}
      /> : null}
      {binding ? <>
        <label>Role<select value={binding.mapping.behavior.role} onChange={(event) => props.onSetRegionBehavior(props.selectedRegion!.regionId, changedBehavior(binding.mapping.behavior, { role: event.currentTarget.value as ManualRegionRole }))}>{manualRoleOptions.map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label>
        <label>Continuity<select value={binding.mapping.behavior.continuity} onChange={(event) => props.onSetRegionBehavior(props.selectedRegion!.regionId, changedBehavior(binding.mapping.behavior, { continuity: event.currentTarget.value as RegionContinuity }))}>{continuityOptions.map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label>
        <label>Sampling<select value={binding.mapping.behavior.sampling} onChange={(event) => props.onSetRegionBehavior(props.selectedRegion!.regionId, changedBehavior(binding.mapping.behavior, { sampling: event.currentTarget.value as RegionSampling }))}>{samplingOptions.map(([value, label]) => { const reason = samplingPrerequisite(binding.mapping.behavior.role, value); return <option key={value} value={value} disabled={!!reason}>{label}{reason ? ` — ${reason}` : ""}</option>; })}</select></label>
        {binding.mapping.behavior.sampling !== "one_shot" ? <div className="layout-pair"><label>Period X (source px)<input type="number" min={1} max={stage14Slot?.sourceCrop?.width ?? undefined} value={binding.mapping.behavior.periodPixels?.[0] ?? stage14Slot?.sourceCrop?.width ?? 1} onChange={(event) => { const maximum = stage14Slot?.sourceCrop?.width ?? Number.MAX_SAFE_INTEGER; props.onSetRegionBehavior(props.selectedRegion!.regionId, changedBehavior(binding.mapping.behavior, { periodPixels: [Math.max(1, Math.min(maximum, Math.round(Number(event.currentTarget.value)))), binding.mapping.behavior.periodPixels?.[1] ?? stage14Slot?.sourceCrop?.height ?? 1] })); }} /></label><label>Period Y (source px)<input type="number" min={1} max={stage14Slot?.sourceCrop?.height ?? undefined} value={binding.mapping.behavior.periodPixels?.[1] ?? stage14Slot?.sourceCrop?.height ?? 1} onChange={(event) => { const maximum = stage14Slot?.sourceCrop?.height ?? Number.MAX_SAFE_INTEGER; props.onSetRegionBehavior(props.selectedRegion!.regionId, changedBehavior(binding.mapping.behavior, { periodPixels: [binding.mapping.behavior.periodPixels?.[0] ?? stage14Slot?.sourceCrop?.width ?? 1, Math.max(1, Math.min(maximum, Math.round(Number(event.currentTarget.value))))] })); }} /></label></div> : null}
        <label>Orientation<select value={binding.mapping.behavior.orientation} onChange={(event) => props.onSetRegionBehavior(props.selectedRegion!.regionId, changedBehavior(binding.mapping.behavior, { orientation: event.currentTarget.value as RegionBehavior["orientation"] }))}><option value="zero">0°</option><option value="ninety">90°</option><option value="one_eighty">180°</option><option value="two_seventy">270°</option></select></label>
      </> : null}
      <dl><dt>Points to</dt><dd>{boundPatch ? `${boundSource?.name ?? "Missing source"} / ${boundPatch.name}` : boundSource?.name ?? "Missing source"}</dd><dt>Exact crop</dt><dd>{boundsLabel(stage14Slot?.sourceCrop)}</dd><dt>Eligible structural edges</dt><dd>{binding ? Object.entries(binding.mapping.behavior.edgeEligibility).filter(([, eligible]) => eligible).map(([edge]) => edge).join(", ") || "none" : "-"}</dd></dl>
    </section> : null}
    <section className="inspector-section map-view-section">
      <span>MAP VIEW</span>
      <div className="map-view-grid">{mapViews.map(([id, label]) => {
        const available = materialMapRouteAvailable(id);
        const published = artifactMapAvailable(props.artifact, id);
        return <button key={id} className={props.mapView === id ? "active" : ""} onClick={() => props.setMapView(id)} disabled={!available} title={!available ? "Unavailable through Stage 14" : published ? undefined : "Render this material map"}>{label}</button>;
      })}</div>
    </section>
    {!layoutMode && selectedMaterial ? <section className="inspector-section source-inspector"><span>SOURCE INSPECTOR / ADVANCED MATERIAL PREPARATION</span>
      <h2>{selectedMaterial.name}</h2><dl><dt>Dimensions</dt><dd>{selectedMaterial.registeredChannels ? `${selectedMaterial.registeredChannels.orientedSize.width}×${selectedMaterial.registeredChannels.orientedSize.height}` : "-"}</dd><dt>Maps</dt><dd>{selectedMaterial.registeredChannels?.channels.length ?? 0}</dd><dt>Revision</dt><dd>{selectedMaterial.sourceRevision}</dd></dl>
      <label>Exemplar group<input key={`${selectedMaterial.id}:${selectedMaterial.sourceRevision}`} defaultValue={selectedMaterial.exemplarGroup ?? ""} placeholder="Optional group" onBlur={(event) => { const value = event.currentTarget.value.trim() || null; if (value !== selectedMaterial.exemplarGroup) props.onSetExemplarGroup(selectedMaterial.id, value); }} /></label>
      <label>De-lighting<select value={selectedMaterial.delighting.route.route} onChange={(event) => { const route = event.currentTarget.value; const nextRoute: DelightingIntent["route"] = route === "classical_low_frequency" ? { route: "classical_low_frequency" } : { route: "pass_through", reason: "user_disabled" }; props.onSetDelightingIntent(selectedMaterial.id, { ...selectedMaterial.delighting, route: nextRoute }); }}><option value="pass_through">Off / Pass through</option><option value="classical_low_frequency">Classical low frequency</option></select></label>
      {selectedMaterial.delighting.route.route === "classical_low_frequency" ? <label>Strength {Math.round(selectedMaterial.delighting.classical.strengthMilli / 10)}%<input type="range" min="0" max="1000" step="10" value={selectedMaterial.delighting.classical.strengthMilli} onChange={(event) => props.onSetDelightingIntent(selectedMaterial.id, { ...selectedMaterial.delighting, classical: { ...selectedMaterial.delighting.classical, strengthMilli: Number(event.currentTarget.value) } })} /></label> : null}
    </section> : null}
    <details className="inspector-section inspector-diagnostics"><summary>Advanced compile diagnostics</summary>
    {stage14Slot ? <section>
      <span>AUTHORITATIVE STAGE 14 SLOT</span>
      <dl>
        <dt>Slot</dt><dd>{stage14Slot.displayName}</dd>
        <dt>Mapping</dt><dd>{stage14Slot.mappingMode}</dd>
        <dt>Requested / executed</dt><dd>{stage14Slot.requestedSampling} / {stage14Slot.executedMode}</dd>
        <dt>Address mode</dt><dd>{stage14Slot.addressMode}</dd>
        <dt>Period</dt><dd>{stage14Slot.periodPixels?.join(" × ") ?? "none"}</dd>
        <dt>Validity</dt><dd>{stage14Slot.validity}</dd>
        <dt>Correspondence</dt><dd>{stage14Slot.correspondence}</dd>
        <dt>Patch</dt><dd>{stage14Slot.patchId ?? "whole registered source"}</dd>
        <dt>Domain</dt><dd>{stage14Slot.domainId.slice(0, 12)}</dd>
        <dt>Candidate</dt><dd>{stage14Slot.candidateId.slice(0, 12)}</dd>
        <dt>SamplingPlan</dt><dd>{stage14Slot.samplingPlanId.slice(0, 12)}</dd>
        <dt>Stage 14 result</dt><dd>{stage14Slot.stage14ResultId.slice(0, 12)}</dd>
      </dl>
    </section> : null}
    <section>
      <span>SOURCE QUALITY &amp; BEHAVIOR</span>
    {props.sourceAnalysis ? <>
        <dl>
          <dt>Analyzed</dt><dd>{materialBehaviorOptions.find(([id]) => id === props.sourceAnalysis!.sourceAnalysis.analyzedClass)?.[1]}</dd>
          <dt>Confidence</dt><dd>{props.sourceAnalysis.sourceAnalysis.confidencePercent}%</dd>
          <dt>Routed</dt><dd>{materialBehaviorOptions.find(([id]) => id === props.sourceAnalysis!.sourceAnalysis.routedClass)?.[1]}</dd>
          <dt>Warnings</dt><dd>{props.sourceAnalysis.sourceAnalysis.warningCount}</dd>
        </dl>
        <p>{props.sourceAnalysis.sourceAnalysis.qualitySummary}</p>
        <p>{props.sourceAnalysis.sourceAnalysis.evidenceSummary}</p>
        <p>{props.sourceAnalysis.sourceAnalysis.scaleSummary}</p>
        <p>{props.sourceAnalysis.sourceAnalysis.orientationSummary}</p>
        <div className="inspector-actions">
          <button onClick={() => props.onCalibrate(props.sourceAnalysis!.materialSourceId, { command: "reset_scale" })}>Reset scale</button>
          <button onClick={() => props.onCalibrate(props.sourceAnalysis!.materialSourceId, { command: "reset_orientation" })}>Reset axis</button>
        </div>
        <CalibrationEditor
          materialSourceId={props.sourceAnalysis.materialSourceId}
          onApply={props.onCalibrate}
        />
        <label>Routing intent<select
          value={analyzedSource?.classification.overrideClass ?? "analysis"}
          onChange={(event) => {
            const value = event.currentTarget.value;
            props.onClassify(
              props.sourceAnalysis!.materialSourceId,
              value === "analysis"
                ? { command: "reset_to_analysis" }
                : { command: "override", class: value as MaterialBehaviorClass },
            );
          }}
        >
          <option value="analysis">Use measured analysis</option>
          {materialBehaviorOptions.map(([id, label]) => <option key={id} value={id}>{label}</option>)}
        </select></label>
      </> : <p>Select a prepared patch to inspect Stage 5 evidence.</p>}
    </section>
    </details>
    {!layoutMode && props.project?.document?.sourceFrame ? <SourceFrameEditor
      frame={props.project.document.sourceFrame}
      onApply={props.onSetSourceFrame}
      editing={props.sourceFrameEditing}
      onEditingChange={props.onSetSourceFrameEditing}
    /> : null}
    {!layoutMode ? <section className="inspector-section">
      <span>SELECTED REGION</span>
      {props.selectedRegion ? <>
        <h2>{props.selectedRegion.displayName}</h2>
        <code>{props.selectedRegion.regionId}</code>
        <dl>
          <dt>Content</dt><dd>{contentLabel(binding?.content.type)}</dd>
          <dt>Projection</dt><dd>{stage14Slot?.mappingOrigin === "partition" ? "Partition crop" : binding?.mapping.projection.type ?? "-"}</dd>
          <dt>Source crop</dt><dd>{stage14Slot?.mappingOrigin === "partition" ? "Inapplicable — partition-owned" : boundsLabel(stage14Slot?.sourceCrop)}</dd>
          <dt>Mapping origin</dt><dd>{stage14Slot?.mappingOrigin ?? "-"}</dd>
          <dt>GridRect</dt><dd>{gridRectLabel(stage14Slot?.gridRect)}</dd>
          <dt>Source pixels</dt><dd>{boundsLabel(stage14Slot?.sourceCrop)}</dd>
          <dt>Source normalized</dt><dd>{normalizedBoundsLabel(stage14Slot?.sourceBounds)}</dd>
          <dt>Bounds</dt><dd>{boundsLabel(props.selectedRegion.allocationBounds)}</dd>
          <dt>Artifact revision</dt><dd>{props.artifact?.documentRevision ?? "-"}</dd>
          <dt>Material</dt><dd>{props.selectedRegion.materialId.slice(0, 8)}</dd>
        </dl>
        {overlapIds.length > 0 ? <p className="source-overlap-warning">Explicit override overlaps: {overlapIds.join(", ")}</p> : null}
        {binding?.mapping.behavior.role === "radial" && binding.mapping.radial ? <RadialEditor
          key={`${props.selectedRegion.regionId}-radial`}
          regionId={props.selectedRegion.regionId}
          radial={binding.mapping.radial}
          onApply={props.onSetRadial}
        /> : null}
      </> : <p>Select a patch or create one on the source workbench.</p>}
    </section> : null}
    {!layoutMode ? <><LockedSection title="Profiles & Weathering" reason="Generated-map recipes are not command-backed in this slice." /><LockedSection title="Decorations" reason="Decoration bindings require authored patch commands." /></> : null}
  </aside>;
}

function SourceFrameEditor(props: {
  frame: SourceFrame;
  onApply: (bounds: NormalizedBounds) => void;
  editing: boolean;
  onEditingChange: (editing: boolean) => void;
}) {
  const [bounds, setBounds] = useState<NormalizedBounds>(props.frame.bounds);
  useEffect(() => setBounds(props.frame.bounds), [props.frame.identity.join(",")]);
  const sourceSize = props.frame.orientedDimensions;
  const aspect = (props.frame.outputAspect[0] / Math.max(1, props.frame.outputAspect[1]))
    * sourceSize.height / Math.max(1, sourceSize.width);
  const set = (key: keyof NormalizedBounds, value: number) => setBounds((current) => constrainAspectBounds(
    { ...current, [key]: value }, aspect, key === "height" ? "height" : "width",
  ));
  const pixelValue = (key: keyof NormalizedBounds) => bounds[key] * (key === "x" || key === "width" ? sourceSize.width : sourceSize.height);
  const setPixels = (key: keyof NormalizedBounds, value: number) => set(key, value / (key === "x" || key === "width" ? sourceSize.width : sourceSize.height));
  function fit(mode: "center" | "width" | "height" | "largest") {
    const next = fitSourceFrame(props.frame.orientedDimensions, { width: props.frame.outputAspect[0], height: props.frame.outputAspect[1] }, mode === "center" ? "largest" : mode);
    setBounds(next);
    props.onApply(next);
  }
  function apply() {
    const next = constrainAspectBounds(bounds, aspect);
    setBounds(next);
    props.onApply(next);
  }
  return <section className="inspector-section source-frame-editor">
    <span>SOURCE FRAME</span>
    <dl><dt>Oriented source</dt><dd>{props.frame.orientedDimensions.width} × {props.frame.orientedDimensions.height}</dd><dt>Revision</dt><dd>{props.frame.sourceRevision}</dd></dl>
    <p className="source-frame-aspect-lock">Aspect ratio locked to {props.frame.outputAspect[0]}:{props.frame.outputAspect[1]}</p>
    <button className="primary source-frame-edit-toggle" onClick={() => props.onEditingChange(!props.editing)}>
      {props.editing ? "Done Editing" : "Edit Source Frame"}
    </button>
    {props.editing ? <>
    <div className="region-bounds-editor">
      {(["x", "y", "width", "height"] as const).map((key) => <label key={key}>{key}<input type="number" min={0} max={1} step={0.0001} value={bounds[key]} onChange={(event) => set(key, Number(event.currentTarget.value))} /></label>)}
    </div>
    <div className="region-bounds-editor source-frame-pixels">
      {(["x", "y", "width", "height"] as const).map((key) => <label key={key}>{key} px<input type="number" min={0} step={1} value={Math.round(pixelValue(key))} onChange={(event) => setPixels(key, Number(event.currentTarget.value))} /></label>)}
    </div>
    <div className="button-row">
      <button onClick={apply}>Apply</button>
      <button onClick={() => fit("center")}>Center</button>
      <button onClick={() => fit("width")}>Fit Width</button>
      <button onClick={() => fit("height")}>Fit Height</button>
      <button onClick={() => fit("largest")}>Largest Fit</button>
    </div>
    </> : <p className="source-frame-edit-hint">Preview mode. Choose Edit Source Frame to move or resize the crop.</p>}
  </section>;
}

function CalibrationEditor(props: {
  materialSourceId: string;
  onApply: (materialSourceId: string, command: MaterialCalibrationCommand) => void;
}) {
  type CalibrationValueKey = "x1" | "y1" | "x2" | "y2" | "distanceMm" | "motifWidthPx" | "motifHeightPx" | "motifWidthMm" | "motifHeightMm" | "ppmX" | "ppmY" | "confidence" | "orientationDegrees";
  const [mode, setMode] = useState<"measure" | "motif" | "imported" | "override" | "orientation">("measure");
  const [values, setValues] = useState<Record<CalibrationValueKey, number>>({
    x1: 0, y1: 0, x2: 100, y2: 0, distanceMm: 250,
    motifWidthPx: 100, motifHeightPx: 100, motifWidthMm: 250, motifHeightMm: 250,
    ppmX: 400, ppmY: 400, confidence: 100, orientationDegrees: 0,
  });
  const [provenance, setProvenance] = useState<"convention" | "prior_estimated">("convention");
  const set = (key: CalibrationValueKey, value: number) => setValues((current) => ({ ...current, [key]: value }));
  const positive = (...keys: CalibrationValueKey[]) => keys.every((key) => Number.isFinite(values[key]) && values[key] > 0);
  const confidenceMilli = Math.round(Math.min(100, Math.max(0, values.confidence)) * 10);
  const apply = () => {
    let command: MaterialCalibrationCommand | null = null;
    if (mode === "measure" && positive("distanceMm") && Number.isFinite(values.x1) && Number.isFinite(values.y1) && Number.isFinite(values.x2) && Number.isFinite(values.y2)) {
      command = {
        command: "measure_two_points",
        start: { x: Math.round(values.x1 * 1000), y: Math.round(values.y1 * 1000) },
        end: { x: Math.round(values.x2 * 1000), y: Math.round(values.y2 * 1000) },
        distance_micrometers: Math.round(values.distanceMm * 1000),
      };
    } else if (mode === "motif" && positive("motifWidthPx", "motifHeightPx", "motifWidthMm", "motifHeightMm")) {
      command = {
        command: "set_known_motif_size",
        motif_width_pixels_milli: Math.round(values.motifWidthPx * 1000),
        motif_height_pixels_milli: Math.round(values.motifHeightPx * 1000),
        motif_width_micrometers: Math.round(values.motifWidthMm * 1000),
        motif_height_micrometers: Math.round(values.motifHeightMm * 1000),
        confidence_milli: confidenceMilli,
      };
    } else if (mode === "imported" && positive("ppmX", "ppmY")) {
      command = {
        command: "set_imported_metadata",
        source_pixels_per_meter_x_milli: Math.round(values.ppmX * 1000),
        source_pixels_per_meter_y_milli: Math.round(values.ppmY * 1000),
        confidence_milli: confidenceMilli,
      };
    } else if (mode === "override" && positive("ppmX", "ppmY")) {
      command = {
        command: "override_scale",
        source_pixels_per_meter_x_milli: Math.round(values.ppmX * 1000),
        source_pixels_per_meter_y_milli: Math.round(values.ppmY * 1000),
        provenance,
        confidence_milli: confidenceMilli,
      };
    } else if (mode === "orientation" && Number.isFinite(values.orientationDegrees) && values.orientationDegrees >= 0 && values.orientationDegrees < 180) {
      command = { command: "override_orientation", axis_millidegrees: Math.round(values.orientationDegrees * 1000) };
    }
    if (command) props.onApply(props.materialSourceId, command);
  };
  const numeric = (key: CalibrationValueKey, label: string, min?: number, max?: number) => <label>{label}<input
    type="number" step="any" min={min} max={max} value={values[key]}
    onChange={(event) => set(key, event.currentTarget.valueAsNumber)}
  /></label>;
  return <div className="calibration-editor">
    <label>Calibration source<select value={mode} onChange={(event) => setMode(event.currentTarget.value as typeof mode)}>
      <option value="measure">Two-point measurement</option>
      <option value="motif">Known motif size</option>
      <option value="imported">Imported metadata</option>
      <option value="override">Convention / prior</option>
      <option value="orientation">Orientation override</option>
    </select></label>
    <div className="calibration-grid">
      {mode === "measure" ? <>
        {numeric("x1", "Start X (source px)")}{numeric("y1", "Start Y (source px)")}
        {numeric("x2", "End X (source px)")}{numeric("y2", "End Y (source px)")}
        {numeric("distanceMm", "Known distance (mm)", 0.001)}
      </> : null}
      {mode === "motif" ? <>
        {numeric("motifWidthPx", "Motif width (px)", 0.001)}{numeric("motifHeightPx", "Motif height (px)", 0.001)}
        {numeric("motifWidthMm", "Motif width (mm)", 0.001)}{numeric("motifHeightMm", "Motif height (mm)", 0.001)}
        {numeric("confidence", "Confidence (%)", 0, 100)}
      </> : null}
      {mode === "imported" || mode === "override" ? <>
        {numeric("ppmX", "Pixels per meter X", 0.001)}{numeric("ppmY", "Pixels per meter Y", 0.001)}
        {numeric("confidence", "Confidence (%)", 0, 100)}
        {mode === "override" ? <label>Provenance<select value={provenance} onChange={(event) => setProvenance(event.currentTarget.value as typeof provenance)}>
          <option value="convention">Texture-set convention</option><option value="prior_estimated">Class prior (not world accurate)</option>
        </select></label> : null}
      </> : null}
      {mode === "orientation" ? numeric("orientationDegrees", "Material axis (0-180°)", 0, 179.999) : null}
    </div>
    <button onClick={apply}>Apply calibration</button>
    <small>Coordinates are authoritative source pixels; viewport zoom is ignored.</small>
  </div>;
}

function RegionGridRectEditor(props: { regionId: string; gridRect: LogicalRect; grid: { width: number; height: number }; onApply: (regionId: string, gridRect: LogicalRect) => void }) {
  const [rect, setRect] = useState(props.gridRect);
  const set = (field: keyof LogicalRect, value: number) => setRect((current) => {
    const rounded = Math.round(Number.isFinite(value) ? value : current[field]);
    if (field === "x") return { ...current, x: Math.max(0, Math.min(props.grid.width - current.width, rounded)) };
    if (field === "y") return { ...current, y: Math.max(0, Math.min(props.grid.height - current.height, rounded)) };
    if (field === "width") return { ...current, width: Math.max(1, Math.min(props.grid.width - current.x, rounded)) };
    return { ...current, height: Math.max(1, Math.min(props.grid.height - current.y, rounded)) };
  });
  return <div className="mapping-editor region-grid-editor">
    <strong>REGION GEOMETRY</strong>
    <small>Exact logical-grid bounds. Applying uses the same ownership-preserving command as the sheet handles.</small>
    {(["x", "y", "width", "height"] as const).map((field) => <label key={field}>{field}<input type="number" min={field === "width" || field === "height" ? 1 : 0} max={field === "x" || field === "width" ? props.grid.width : props.grid.height} step={1} value={rect[field]} onChange={(event) => set(field, Number(event.currentTarget.value))} /></label>)}
    <button onClick={() => props.onApply(props.regionId, rect)} disabled={sameGridRect(rect, props.gridRect)}>Apply geometry</button>
  </div>;
}

function RadialEditor(props: { regionId: string; radial: NonNullable<RegionMapping["radial"]>; onApply: (regionId: string, radial: NonNullable<RegionMapping["radial"]>) => void }) {
  const [radial, setRadial] = useState(props.radial);
  useEffect(() => setRadial(props.radial), [props.radial.centerX, props.radial.centerY, props.radial.innerRadius, props.radial.outerRadius, props.radial.falloff, props.radial.blendWidth, props.radial.seamBlendWidth]);
  const fields: ReadonlyArray<[keyof typeof radial, string, number, number, number]> = [
    ["centerX", "Center X", 0, 1, 0.01], ["centerY", "Center Y", 0, 1, 0.01],
    ["innerRadius", "Protected center", 0, 1.999, 0.001], ["outerRadius", "Warp outer edge", 0.001, 2, 0.001],
    ["falloff", "Falloff", 0.1, 4, 0.05],
    ["seamBlendWidth", "Seam blend", 0, 0.25, 0.005],
  ];
  const update = (field: keyof typeof radial, value: number) => {
    if (!Number.isFinite(value)) return;
    let next = { ...radial, [field]: value };
    if (field === "innerRadius") next = { ...next, innerRadius: Math.max(0, Math.min(radial.outerRadius - 0.001, value)) };
    else if (field === "outerRadius") next = { ...next, outerRadius: Math.max(radial.innerRadius + 0.001, Math.min(2, value)) };
    else {
      const limits: readonly [number, number] = field === "falloff" ? [0.1, 4] : field === "seamBlendWidth" ? [0, 0.25] : [0, 1];
      next = { ...next, [field]: Math.max(limits[0], Math.min(limits[1], value)) };
    }
    setRadial(next);
    props.onApply(props.regionId, next);
  };
  return <div className="mapping-editor radial-editor">
    <strong>RADIAL PROJECTION</strong>
    {fields.map(([field, label, min, max, step]) => <label key={field}>{label}<input type="number" min={min} max={max} step={step} value={Number(radial[field].toFixed(3))} onChange={(event) => update(field, Number(event.target.value))} /></label>)}
    <small>The protected center stays planar. Falloff redistributes detail across the wrap band. Changes apply immediately.</small>
  </div>;
}

function LockedSection({ title, reason }: { title: string; reason: string }) {
  return <section className="locked"><strong>{title}</strong><span>{reason}</span></section>;
}

function buildStatus(project: ProjectProjection | null, artifact: IntermediateAtlasProjection | null, activity: Activity, problem: CommandFailure | null, stale: boolean) {
  if (activity === "importing") return "Importing";
  if (activity === "compiling") return `Compiling revision ${project?.document?.documentRevision ?? 1}`;
  if (activity === "editing") return "Committing layout metadata";
  if (activity === "exporting") return "Exporting";
  if (problem) return "Region error";
  if (!project?.materialSources.some((source) => source.registeredChannels?.channels.some((channel) => channel.channel === "base_color"))) return "Empty";
  if (!project.document) return "Ready";
  if (stale || !artifact) return "Stale";
  return `Intermediate Stage 14 rev ${artifact.documentRevision}`;
}

function channelLabel(channel: SourceChannel): string {
  return channelOptions.find((option) => option.value === channel)?.label ?? channel;
}

function hashBytes(bytes: readonly number[]): string {
  return bytes.map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

function retopologizeArtifact(prior: IntermediateAtlasProjection | null, project: ProjectProjection): IntermediateAtlasProjection | null {
  const document = project.document;
  if (!prior || !document?.sourceFrame || !document.logicalGrid) return null;
  const priorById = new Map(prior.regions.map((region) => [region.regionId, region]));
  const fallback = prior.regions[0];
  const regions = document.topology.regions.map((definition): ResolvedRegion => {
    const existing = priorById.get(definition.id);
    const binding = document.regionBindings[definition.id];
    const previewBounds = definition.gridRect
      ? gridRectToPreviewBounds(definition.gridRect, document.logicalGrid!, prior)
      : {
          x: Math.round(definition.allocationRect.x / document.renderSettings.outputSize.width * prior.width),
          y: Math.round(definition.allocationRect.y / document.renderSettings.outputSize.height * prior.height),
          width: Math.round(definition.allocationRect.width / document.renderSettings.outputSize.width * prior.width),
          height: Math.round(definition.allocationRect.height / document.renderSettings.outputSize.height * prior.height),
        };
    const partitionSourceBounds = definition.gridRect
      ? sourceFrameGridBounds(document.sourceFrame!.bounds, document.logicalGrid!, definition.gridRect)
      : undefined;
    return {
      regionId: definition.id,
      displayName: definition.displayName,
      semanticBounds: previewBounds,
      paddedBounds: previewBounds,
      atlasDestination: previewBounds,
      allocationBounds: previewBounds,
      hotspotBounds: previewBounds,
      idColor: existing?.idColor ?? stableRegionColor(definition.id),
      materialId: existing?.materialId ?? document.primaryMaterial ?? fallback?.materialId ?? "unassigned",
      materialIdColor: existing?.materialIdColor ?? fallback?.materialIdColor ?? [128, 128, 128],
      mapping: binding?.mapping ?? existing?.mapping ?? fallback!.mapping,
      role: definition.role,
      behavior: binding?.mapping.behavior ?? existing?.behavior ?? fallback!.behavior,
      gridRect: definition.gridRect,
      sourceCrop: existing?.sourceCrop,
      sourceBounds: existing?.mappingOrigin === "explicit_override" ? existing.sourceBounds : partitionSourceBounds,
      mappingOrigin: existing?.mappingOrigin ?? "partition",
    };
  });
  const regionById = new Map(regions.map((region) => [region.regionId, region]));
  const priorSlotById = new Map(prior.slots.map((slot) => [slot.regionId, slot]));
  const fallbackSlot = prior.slots[0];
  return {
    ...prior,
    // These pixels still belong to `prior` until Stage 14 publishes. Preserve
    // their revision/hash authority while using the current region rectangles as
    // a temporary editing overlay. Claiming the new revision here made a stale
    // tile pass the display gate and created mixed old/new split-region imagery.
    regions,
    slots: regions.flatMap((region) => {
      const slot = priorSlotById.get(region.regionId) ?? fallbackSlot;
      return slot ? [{ ...slot, regionId: region.regionId, slotKey: `authored:${region.regionId}`, displayName: region.displayName,
        allocationBounds: region.allocationBounds, hotspotBounds: region.hotspotBounds, gridRect: region.gridRect,
        sourceBounds: region.sourceBounds, mappingOrigin: region.mappingOrigin ?? "partition",
        behaviorVersion: region.behavior.version, role: region.behavior.role, continuity: region.behavior.continuity,
        requestedSampling: region.behavior.sampling, edgeEligibility: region.behavior.edgeEligibility,
        addressMode: region.behavior.sampling === "one_shot" ? "clamp" : region.behavior.sampling === "loop_x" ? "repeat_x" : region.behavior.sampling === "loop_y" ? "repeat_y" : "repeat_xy",
      }] : [];
    }),
    telemetry: [...prior.telemetry, "local topology edit: retained compiled map pixels; metadata committed asynchronously"],
  };
}

function stableRegionColor(id: string): readonly [number, number, number] {
  let hash = 2166136261;
  for (let index = 0; index < id.length; index += 1) hash = Math.imul(hash ^ id.charCodeAt(index), 16777619);
  return [72 + (hash & 127), 72 + ((hash >>> 8) & 127), 72 + ((hash >>> 16) & 127)];
}

function contentLabel(type?: string) {
  return type === "inherit_primary_material" ? "Primary material" : type?.replaceAll("_", " ") ?? "-";
}

function boundsLabel(bounds?: { x: number; y: number; width: number; height: number }) {
  return bounds ? `${bounds.x}, ${bounds.y} / ${bounds.width} x ${bounds.height}` : "-";
}

function normalizedBoundsLabel(bounds?: { x: number; y: number; width: number; height: number }) {
  return bounds ? `${bounds.x.toFixed(5)}, ${bounds.y.toFixed(5)} / ${bounds.width.toFixed(5)} x ${bounds.height.toFixed(5)}` : "-";
}

function gridRectLabel(rect?: { x: number; y: number; width: number; height: number }) {
  return rect ? `${rect.x}, ${rect.y} / ${rect.width} x ${rect.height}` : "-";
}

function normalizedBoundsOverlap(a: NormalizedBounds, b: NormalizedBounds) {
  return a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y;
}

type LogicalRect = { x: number; y: number; width: number; height: number };
type ResizeHandle = "nw" | "n" | "ne" | "e" | "se" | "s" | "sw" | "w";
const resizeHandles: readonly ResizeHandle[] = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];

function sameGridRect(first: LogicalRect, second: LogicalRect) {
  return first.x === second.x && first.y === second.y && first.width === second.width && first.height === second.height;
}

function resizeGridRect(origin: LogicalRect, handle: ResizeHandle, point: { x: number; y: number }, grid: { width: number; height: number }): LogicalRect {
  let left = origin.x;
  let top = origin.y;
  let right = origin.x + origin.width;
  let bottom = origin.y + origin.height;
  if (handle.includes("w")) left = Math.max(0, Math.min(right - 1, point.x));
  if (handle.includes("e")) right = Math.min(grid.width, Math.max(left + 1, point.x));
  if (handle.includes("n")) top = Math.max(0, Math.min(bottom - 1, point.y));
  if (handle.includes("s")) bottom = Math.min(grid.height, Math.max(top + 1, point.y));
  return { x: left, y: top, width: right - left, height: bottom - top };
}

type ResizeOwnershipTransfer = { rect: LogicalRect; fromId: string; toId: string };

function previewResizeOwnershipTransfers(regions: readonly ResolvedRegion[], selectedId: string, origin: LogicalRect, target: LogicalRect, grid: { width: number; height: number }): ResizeOwnershipTransfer[] {
  if (sameGridRect(origin, target)) return [];
  const transfers: ResizeOwnershipTransfer[] = [];
  for (const region of regions) {
    if (region.regionId === selectedId || !region.gridRect) continue;
    const gained = logicalRectIntersection(region.gridRect, target);
    if (gained) transfers.push({ rect: gained, fromId: region.regionId, toId: selectedId });
  }
  const retained = logicalRectIntersection(origin, target);
  if (!retained) return transfers;
  const released = subtractLogicalRect(origin, retained);
  if (released.length === 0) return transfers;
  const neighbors = regions.flatMap((region, ordinal) => region.regionId !== selectedId && region.gridRect && logicalRectsTouch(origin, region.gridRect)
    ? [{ ordinal, id: region.regionId, rect: region.gridRect }]
    : []);
  if (neighbors.length === 0) return transfers;
  for (const piece of released) {
    const neighbor = neighbors.reduce((best, candidate) => {
      const candidateScore = logicalTransferScore(piece, candidate.rect, candidate.ordinal);
      const bestScore = logicalTransferScore(piece, best.rect, best.ordinal);
      return compareNumberTuple(candidateScore, bestScore) < 0 ? candidate : best;
    });
    transfers.push({ rect: piece, fromId: selectedId, toId: neighbor.id });
  }
  return transfers;
}

function logicalRectIntersection(first: LogicalRect, second: LogicalRect): LogicalRect | null {
  const x = Math.max(first.x, second.x);
  const y = Math.max(first.y, second.y);
  const right = Math.min(first.x + first.width, second.x + second.width);
  const bottom = Math.min(first.y + first.height, second.y + second.height);
  return right > x && bottom > y ? { x, y, width: right - x, height: bottom - y } : null;
}

function subtractLogicalRect(rect: LogicalRect, cut: LogicalRect): LogicalRect[] {
  const pieces: LogicalRect[] = [];
  if (cut.y > rect.y) pieces.push({ x: rect.x, y: rect.y, width: rect.width, height: cut.y - rect.y });
  if (cut.y + cut.height < rect.y + rect.height) pieces.push({ x: rect.x, y: cut.y + cut.height, width: rect.width, height: rect.y + rect.height - cut.y - cut.height });
  if (cut.x > rect.x) pieces.push({ x: rect.x, y: cut.y, width: cut.x - rect.x, height: cut.height });
  if (cut.x + cut.width < rect.x + rect.width) pieces.push({ x: cut.x + cut.width, y: cut.y, width: rect.x + rect.width - cut.x - cut.width, height: cut.height });
  return pieces;
}

function logicalRectsTouch(first: LogicalRect, second: LogicalRect) {
  const vertical = (first.x + first.width === second.x || second.x + second.width === first.x)
    && first.y < second.y + second.height && first.y + first.height > second.y;
  const horizontal = (first.y + first.height === second.y || second.y + second.height === first.y)
    && first.x < second.x + second.width && first.x + first.width > second.x;
  return vertical || horizontal;
}

function logicalTransferScore(piece: LogicalRect, neighbor: LogicalRect, ordinal: number): readonly number[] {
  const mergeable = (piece.y === neighbor.y && piece.height === neighbor.height && (piece.x + piece.width === neighbor.x || neighbor.x + neighbor.width === piece.x))
    || (piece.x === neighbor.x && piece.width === neighbor.width && (piece.y + piece.height === neighbor.y || neighbor.y + neighbor.height === piece.y));
  const verticalTouch = piece.x + piece.width === neighbor.x || neighbor.x + neighbor.width === piece.x;
  const horizontalTouch = piece.y + piece.height === neighbor.y || neighbor.y + neighbor.height === piece.y;
  const touchSpan = verticalTouch
    ? Math.max(0, Math.min(piece.y + piece.height, neighbor.y + neighbor.height) - Math.max(piece.y, neighbor.y))
    : horizontalTouch ? Math.max(0, Math.min(piece.x + piece.width, neighbor.x + neighbor.width) - Math.max(piece.x, neighbor.x)) : 0;
  const dx = piece.x + piece.width <= neighbor.x ? neighbor.x - piece.x - piece.width
    : neighbor.x + neighbor.width <= piece.x ? piece.x - neighbor.x - neighbor.width : 0;
  const dy = piece.y + piece.height <= neighbor.y ? neighbor.y - piece.y - piece.height
    : neighbor.y + neighbor.height <= piece.y ? piece.y - neighbor.y - neighbor.height : 0;
  return [mergeable ? 0 : 1, -touchSpan, dx + dy, ordinal];
}

function compareNumberTuple(first: readonly number[], second: readonly number[]) {
  for (let index = 0; index < Math.min(first.length, second.length); index += 1) {
    if (first[index] !== second[index]) return first[index]! - second[index]!;
  }
  return first.length - second.length;
}

function adaptiveGridSteps(grid: { width: number; height: number }, width: number, height: number, scale: number) {
  const choose = (cells: number, pixels: number) => {
    const cellPixels = pixels * scale / Math.max(1, cells);
    return [1, 2, 4, 8, 16, 32, 64, 128, 256].find((step) => cellPixels * step >= 12) ?? 256;
  };
  const minorX = choose(grid.width, width);
  const minorY = choose(grid.height, height);
  return { minorX, minorY, majorX: Math.min(grid.width, minorX * 4), majorY: Math.min(grid.height, minorY * 4) };
}

function gridRectOverlayStyle(rect: LogicalRect, grid: { width: number; height: number }): React.CSSProperties {
  return { left: `${rect.x / grid.width * 100}%`, top: `${rect.y / grid.height * 100}%`, width: `${rect.width / grid.width * 100}%`, height: `${rect.height / grid.height * 100}%` };
}

function sourceFrameTextureStyle(frame: SourceFrame): React.CSSProperties {
  return {
    position: "absolute",
    left: `${-frame.bounds.x / frame.bounds.width * 100}%`,
    top: `${-frame.bounds.y / frame.bounds.height * 100}%`,
    width: `${100 / frame.bounds.width}%`,
    height: `${100 / frame.bounds.height}%`,
    maxWidth: "none",
    maxHeight: "none",
  };
}

function recipeWithGridSize(recipe: PartitionRecipe, size: number): PartitionRecipe {
  const previous = recipe.grid.width;
  const scale = (value: number, minimum = 1) => Math.max(minimum, Math.min(size, Math.round(value * size / Math.max(1, previous))));
  const scaleFamily = (family: PartitionRecipe["composition"]["broadPanels"]) => {
    const minimumWidth = scale(family.minimumWidth);
    const minimumHeight = scale(family.minimumHeight);
    return { ...family, minimumWidth, minimumHeight, maximumWidth: Math.max(minimumWidth, scale(family.maximumWidth)), maximumHeight: Math.max(minimumHeight, scale(family.maximumHeight)) };
  };
  const horizontalMinimum = scale(recipe.composition.horizontalStrips.minimumThickness);
  const verticalMinimum = scale(recipe.composition.verticalStrips.minimumThickness);
  const radialMinimum = scale(recipe.composition.radialReservations.allocationMinDiameter);
  const radialMaximum = Math.max(radialMinimum, Math.min(Math.max(1, size - 1), scale(recipe.composition.radialReservations.allocationMaxDiameter)));
  const hierarchical = recipe.hierarchical ? {
    ...recipe.hierarchical,
    stripThicknessLadder: recipe.hierarchical.stripThicknessLadder.map((value) => scale(value)),
    radialMinDiameter: scale(recipe.hierarchical.radialMinDiameter),
    radialMaxDiameter: Math.max(scale(recipe.hierarchical.radialMinDiameter), Math.min(Math.max(1, size - 1), scale(recipe.hierarchical.radialMaxDiameter))),
  } : undefined;
  return {
    ...recipe,
    grid: { ...recipe.grid, width: size, height: size },
    hierarchical,
    composition: {
      ...recipe.composition,
      broadPanels: scaleFamily(recipe.composition.broadPanels),
      mediumBlocks: scaleFamily(recipe.composition.mediumBlocks),
      smallDetails: scaleFamily(recipe.composition.smallDetails),
      horizontalStrips: { ...recipe.composition.horizontalStrips, minimumThickness: horizontalMinimum, maximumThickness: Math.max(horizontalMinimum, scale(recipe.composition.horizontalStrips.maximumThickness)) },
      verticalStrips: { ...recipe.composition.verticalStrips, minimumThickness: verticalMinimum, maximumThickness: Math.max(verticalMinimum, scale(recipe.composition.verticalStrips.maximumThickness)) },
      microStrips: { ...recipe.composition.microStrips, minimumThickness: scale(recipe.composition.microStrips.minimumThickness), maximumThickness: scale(recipe.composition.microStrips.maximumThickness) },
      radialReservations: { ...recipe.composition.radialReservations, allocationMinDiameter: radialMinimum, allocationMaxDiameter: radialMaximum },
    },
  };
}

function updateFamilyQuota<K extends "broadPanels" | "mediumBlocks" | "smallDetails">(recipe: PartitionRecipe, family: K, patch: Partial<PartitionRecipe["composition"][K]>): PartitionRecipe {
  return { ...recipe, composition: { ...recipe.composition, [family]: { ...recipe.composition[family], ...patch } } };
}

function recipeWithStripMinimum(recipe: PartitionRecipe, minimumThickness: number): PartitionRecipe {
  return { ...recipe, composition: { ...recipe.composition,
    horizontalStrips: { ...recipe.composition.horizontalStrips, minimumThickness, maximumThickness: Math.max(minimumThickness, recipe.composition.horizontalStrips.maximumThickness) },
    verticalStrips: { ...recipe.composition.verticalStrips, minimumThickness, maximumThickness: Math.max(minimumThickness, recipe.composition.verticalStrips.maximumThickness) },
  } };
}

function recipeWithStripMaximum(recipe: PartitionRecipe, maximumThickness: number): PartitionRecipe {
  return { ...recipe, composition: { ...recipe.composition,
    horizontalStrips: { ...recipe.composition.horizontalStrips, maximumThickness: Math.max(recipe.composition.horizontalStrips.minimumThickness, maximumThickness) },
    verticalStrips: { ...recipe.composition.verticalStrips, maximumThickness: Math.max(recipe.composition.verticalStrips.minimumThickness, maximumThickness) },
  } };
}

function recipeWithRadialDiameter(recipe: PartitionRecipe, minimum: number, maximum: number): PartitionRecipe {
  const allocationMinDiameter = Math.max(1, Math.min(recipe.grid.width - 1, minimum));
  const allocationMaxDiameter = Math.max(allocationMinDiameter, Math.min(recipe.grid.width - 1, maximum));
  return { ...recipe, composition: { ...recipe.composition, radialReservations: { ...recipe.composition.radialReservations, allocationMinDiameter, allocationMaxDiameter } } };
}

function cellPercent(cells: number, extent: number) { return (cells / Math.max(1, extent) * 100).toFixed(1); }

function mergeCandidate(regions: readonly ResolvedRegion[], regionId: string) {
  const region = regions.find((item) => item.regionId === regionId);
  if (!region?.gridRect) return null;
  const rect = region.gridRect;
  return regions.find((candidate) => {
    const other = candidate.gridRect;
    if (!other || candidate.regionId === regionId) return false;
    const vertical = rect.y === other.y && rect.height === other.height && (rect.x + rect.width === other.x || other.x + other.width === rect.x);
    const horizontal = rect.x === other.x && rect.width === other.width && (rect.y + rect.height === other.y || other.y + other.height === rect.y);
    return vertical || horizontal;
  }) ?? null;
}

function overlayStyle(region: ResolvedRegion, artifact: Pick<IntermediateAtlasProjection, "width" | "height">, scale = 1, fillOpacity = 0, borderOpacity = 1): React.CSSProperties {
  const bounds = region.allocationBounds;
  return {
    left: `${bounds.x / artifact.width * 100}%`,
    top: `${bounds.y / artifact.height * 100}%`,
    width: `${bounds.width / artifact.width * 100}%`,
    height: `${bounds.height / artifact.height * 100}%`,
    "--region-fill": `rgb(${region.idColor[0]} ${region.idColor[1]} ${region.idColor[2]} / ${fillOpacity})`,
    "--region-border": `rgb(${region.idColor[0]} ${region.idColor[1]} ${region.idColor[2]} / ${borderOpacity})`,
    "--region-stroke": `${Math.min(3, Math.max(0.75, 1 / scale))}px`,
    "--region-label-size": `${Math.min(16, Math.max(7, 10 / scale))}px`,
  } as React.CSSProperties;
}

createRoot(document.getElementById("root")!).render(<React.StrictMode><App /></React.StrictMode>);
