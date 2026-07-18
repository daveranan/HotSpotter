import React, { useEffect, useMemo, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  IPC_PROTOCOL_VERSION,
  type CommandFailure,
  type DelightingIntent,
  type MaterialBehaviorClass,
  type MaterialClassificationCommand,
  type MaterialCalibrationCommand,
  type CompiledMapView,
  type IntermediateAtlasProjection,
  type NormalizedBounds,
  type NormalConvention,
  type Patch,
  type PatchCommand,
  type PatchGeometry,
  type ProjectProjection,
  type PreviewSheetProjection,
  type RecentProject,
  type RegionMapping,
  type ResolvedRegion,
  type SourceChannel,
  type SourceProjection,
  type SourceFrame,
  type PartitionRecipe,
  type Stage14SlotProjection,
  type TrimSheetDocumentCommand,
} from "@hot-trimmer/ipc-contracts";
import { assignSourceFiles } from "./source-assignment";
import { adjustCrop, anchoredZoom, clamp01, constrainAspectBounds, fitSourceFrame, fitView, gridRectToPreviewBounds, movePatch, normalizePatchToRectangle, patchBounds, patchPointerAngle, resizeAspectLocked, resizePatch, resizePanes, rotatePatch, type CanvasView, type CropDragAction, type PaneDragKind, type PaneState, type PatchResizeHandle } from "./source-workbench-geometry";
import { SourceFramePreviewController } from "./source-frame-preview-controller";
import { defaultPartitionRecipe, layoutTemplateOptions, layoutTemplateRecipe, selectedLayoutTemplate, type LayoutTemplateId } from "./hierarchical-layout-templates";
import "./document-app.css";

const protocol = { protocolVersion: IPC_PROTOCOL_VERSION };
const gridResolutionOptions = [16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256] as const;

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
  { value: "base_color", label: "Base Color", short: "BC", tone: "color" },
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
  ["baseColor", "Base Color"],
  ["normal", "Normal"],
  ["height", "Height"],
  ["roughness", "Roughness"],
  ["metallic", "Metallic"],
  ["ambientOcclusion", "AO"],
  ["regionId", "Region ID"],
  ["materialId", "Material ID"],
];

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

type Activity = "starting" | "idle" | "importing" | "compiling" | "editing" | "saving" | "opening";
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

function App() {
  const native = isNativeRuntime();
  const [project, setProject] = useState<ProjectProjection | null>(null);
  const [artifact, setArtifact] = useState<IntermediateAtlasProjection | null>(null);
  const [preview, setPreview] = useState<PreviewSheetProjection | null>(null);
  const [previewClientTelemetry, setPreviewClientTelemetry] = useState<string[]>([]);
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
  const [mapView, setMapView] = useState<CompiledMapView>("baseColor");
  const [activity, setActivity] = useState<Activity>("starting");
  const [problem, setProblem] = useState<CommandFailure | null>(null);
  const [recentProjects, setRecentProjects] = useState<RecentProject[]>([]);
  const [showRecents, setShowRecents] = useState(false);
  const [panes, setPanes] = useState<PaneState>({ library: 220, source: 470, inspector: 278 });
  const [workbenchWidth, setWorkbenchWidth] = useState(1280);
  const [renaming, setRenaming] = useState(false);
  const [draftName, setDraftName] = useState("");
  const [activePatchId, setActivePatchId] = useState<string | null>(null);
  const [preparedPatchPreview, setPreparedPatchPreview] = useState<PreparedPatchPreviewProjection | null>(null);
  const [draftPatchPreview, setDraftPatchPreview] = useState<{ patchId: string; geometry: PatchGeometry } | null>(null);
  const [patchTool, setPatchTool] = useState<"rectangle" | "four-point" | null>(null);
  const [sourceWorkbenchOpen, setSourceWorkbenchOpen] = useState(true);
  const started = useRef(false);
  const previewDraftId = useRef(0);
  const dirtyPreviewRegion = useRef<string | null>(null);
  const suppressAutomaticPreviewRevision = useRef<number | null>(null);
  const lastAutomaticPreviewRevision = useRef<number | null>(null);
  const patchPreviewRequestId = useRef(0);
  const previewPublishStartedAt = useRef<number | null>(null);
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
  const workbenchRef = useRef<HTMLElement | null>(null);

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
  const primaryMaterial = project?.document?.primaryMaterial ?? activeSourceSetId;
  const selectedRegion = artifact?.regions.find((region) => region.regionId === selectedRegionId) ?? null;
  const selectedSlot = artifact?.slots.find((slot) => slot.regionId === selectedRegionId) ?? null;
  const selectedBinding = selectedRegionId ? project?.document?.regionBindings[selectedRegionId] ?? null : null;
  const selectedCrop = selectedBinding?.mapping.projection.type === "crop" ? selectedBinding.mapping.projection : null;
  const currentTopologyHash = project?.document ? hashBytes(project.document.topology.topologyHash) : null;
  const stale = !!project?.document && !!artifact && artifact.documentRevision !== project.document.documentRevision;
  const buildState = buildStatus(project, artifact, activity, problem, stale);
  const paneMode = paneLayoutMode(workbenchWidth);
  const sourceFrameLayout = !!project?.document?.sourceFrame;
  const showSourceWorkspace = paneMode !== "sheet-only" && (!sourceFrameLayout || sourceWorkbenchOpen);
  const workbenchColumns = sourceFrameLayout
    ? paneMode === "full" || paneMode === "without-inspector"
      ? sourceWorkbenchOpen ? `${Math.min(240, panes.library)}px 6px ${Math.min(430, panes.source)}px 6px minmax(0, 1fr)` : `${Math.min(260, panes.library)}px 6px minmax(0, 1fr)`
      : paneMode === "without-library" && sourceWorkbenchOpen ? `${Math.min(430, panes.source)}px 6px minmax(0, 1fr)` : "minmax(0, 1fr)"
    : paneMode === "full"
    ? `${panes.library}px 6px ${panes.source}px 6px minmax(0, 1fr) 6px ${panes.inspector}px`
    : paneMode === "without-inspector"
      ? `${panes.library}px 6px ${panes.source}px 6px minmax(0, 1fr)`
      : paneMode === "without-library"
        ? `${panes.source}px 6px minmax(0, 1fr)`
        : "minmax(0, 1fr)";

  useEffect(() => {
    previewDraftId.current += 1;
    dirtyPreviewRegion.current = null;
    setPreview(null);
  }, [currentTopologyHash]);

  useEffect(() => {
    if (!native || !project?.document) return;
    if (suppressAutomaticPreviewRevision.current === project.document.documentRevision) {
      suppressAutomaticPreviewRevision.current = null;
      lastAutomaticPreviewRevision.current = project.document.documentRevision;
      return;
    }
    if (lastAutomaticPreviewRevision.current === project.document.documentRevision) return;
    lastAutomaticPreviewRevision.current = project.document.documentRevision;
    const dirtyRegion = dirtyPreviewRegion.current;
    dirtyPreviewRegion.current = null;
    void requestPreview(dirtyRegion ?? undefined);
  }, [native, project?.document?.documentRevision]);

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
      await requestPreview(request.regionId, request.projection, "draft512", request.revision, false);
    });
    return () => controller.cancel();
  }, [native, project?.document?.documentRevision, draftPreviewFps]);

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
    setProject(next);
    setArtifact(null);
    setPreview(null);
    setProblem(null);
    setSelectedRegionId(null);
    setSelectedSourceSetId(next.document?.primaryMaterial ?? next.materialSources[0]?.id ?? "");
    setSelectedChannel("base_color");
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
        recovery: "Open an empty map slot directly, or include a Base Color image for a new material source.",
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
      setArtifact(null);
      setSelectedSourceSetId(sourceSetId);
      setSelectedChannel(assignments.at(-1)?.channel ?? "base_color");
      if (assignments.some((assignment) => assignment.channel === "base_color")) {
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
      setArtifact(null);
      setSelectedSourceSetId(sourceSetId);
      setSelectedChannel(channel);
      if (channel === "base_color") {
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

  async function build() {
    if (!project || !primaryMaterial || activity !== "idle") return;
    if (!project.document) {
      setProblem({
        code: "trim_sheet_missing",
        message: "No trim sheet document exists yet.",
        recovery: "Import a Base Color to create the source-to-sheet document, or open a legacy project and rebuild after confirming the preserved sources.",
      });
      return;
    }
    setActivity("compiling");
    setProblem(null);
    try {
      let current = project;
      if (current.document?.primaryMaterial !== primaryMaterial) {
        current = await applyCommand({ type: "set_primary_material", materialId: primaryMaterial });
        suppressAutomaticPreviewRevision.current = current.document!.documentRevision;
        setProject(current);
      }
      const compiled = await invoke<IntermediateAtlasProjection>("preview_through_stage_14", {
        request: { ...protocol, revision: current.document!.documentRevision, profile: "draft512" },
      });
      previewDraftId.current += 1;
      setPreview(null);
      setArtifact(compiled);
      setSelectedRegionId((selected) => compiled.regions.some((region) => region.regionId === selected) ? selected : null);
      window.setTimeout(() => void requestPreview(undefined, undefined, "refinement1024", current.document!.documentRevision), 120);
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
      window.setTimeout(() => void requestPreview(undefined, undefined, "refinement1024", current.document!.documentRevision), 120);
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
    setActivity("editing"); setProblem(null);
    try {
      const current = await applyCommand(commandValue);
      suppressAutomaticPreviewRevision.current = current.document!.documentRevision;
      setProject(current);
      setArtifact((prior) => retopologizeArtifact(prior, current));
      setSelectedRegionId((selected) => current.document!.topology.regions.some((region) => region.id === selected) ? selected : null);
      return current;
    } catch (reason) { setProblem(failure(reason)); return null; }
    finally { setActivity("idle"); }
  }

  function discardPartitionCandidate() {
    if (!candidatePreviewing) return;
    // Discard settles the current recipe too; it should not regenerate until a control changes.
    setCandidatePreviewing(false); setCandidatePreviewHash(partitionRecipeFingerprint(candidateRecipe)); setCandidatePreviewRecipe(null); setArtifact(null); void build();
  }

  async function createDocumentAndCompile(seed: ProjectProjection, materialId: string) {
    setActivity("importing");
    setProblem(null);
    try {
      let current = seed;
      if (!current.document) {
        current = await invoke<ProjectProjection>("create_source_frame_document", { request: protocol });
      }
      if (current.document?.primaryMaterial !== materialId) {
        current = await applyCommand({ type: "set_primary_material", materialId });
      }
      previewDraftId.current += 1;
      setPreview(null);
      suppressAutomaticPreviewRevision.current = current.document!.documentRevision;
      setProject(current);
      const compiled = await invoke<IntermediateAtlasProjection>("preview_through_stage_14", {
        request: { ...protocol, revision: current.document!.documentRevision, profile: "draft512" },
      });
      setArtifact(compiled);
      setSelectedRegionId(null);
      window.setTimeout(() => void requestPreview(undefined, undefined, "refinement1024", current.document!.documentRevision), 120);
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

  async function requestPreview(regionId?: string, projection?: CropProjection, profile: "draft512" | "refinement1024" = "draft512", revision?: number, scheduleRefinement = true) {
    const requestedRevision = revision ?? project?.document?.documentRevision;
    if (!native || requestedRevision === undefined) return;
    const draftId = ++previewDraftId.current;
    setProblem(null);
    previewPublishStartedAt.current = performance.now();
    try {
      const next = await invoke<IntermediateAtlasProjection>("preview_through_stage_14", {
        request: {
          ...protocol,
          revision: requestedRevision,
          regionId,
          transientProjection: projection,
          draftId,
          inputHash: JSON.stringify({ revision: requestedRevision, regionId, projection }),
          profile,
        },
      });
      setPreviewClientTelemetry([`profile=${profile}`, `artifact_dimensions=${next.width}x${next.height}`, `ipc_round_trip_ms=${Math.round(performance.now() - (previewPublishStartedAt.current ?? performance.now()))}`]);
      if (draftId === previewDraftId.current) {
        setArtifact(next);
        setPreview(null);
        setProblem(null);
        if (profile === "draft512" && scheduleRefinement) {
          window.setTimeout(() => {
            if (draftId === previewDraftId.current && requestedRevision === next.documentRevision) {
              void requestPreview(regionId, projection, "refinement1024", requestedRevision);
            }
          }, 120);
        }
      }
    } catch (reason) {
      if (failure(reason).code !== "operation_cancelled") setProblem(failure(reason));
    }
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

  async function history(redo: boolean) {
    try {
      const priorTopologyHash = project?.document ? hashBytes(project.document.topology.topologyHash) : null;
      const next = await invoke<ProjectProjection>(redo ? "redo_document_command" : "undo_document_command", { request: protocol });
      const nextTopologyHash = next.document ? hashBytes(next.document.topology.topologyHash) : null;
      if (next.document && priorTopologyHash !== nextTopologyHash) suppressAutomaticPreviewRevision.current = next.document.documentRevision;
      setProject(next);
      setPreview(null);
      setArtifact((prior) => priorTopologyHash !== nextTopologyHash ? retopologizeArtifact(prior, next) : null);
      setSelectedRegionId((selected) => next.document?.topology.regions.some((region) => region.id === selected) ? selected : null);
      setProblem(null);
    } catch (reason) {
      setProblem(failure(reason));
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

  async function closeToDraft() {
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
    const next = await invoke<ProjectProjection>("apply_patch_command", {
      request: { ...protocol, command, coalescingGroup },
    });
    setProject(next);
    setProblem(null);
    return next;
  }

  async function createPatch(geometry: PatchGeometry, fourPoint: boolean) {
    if (!selectedSource) return;
    const id = crypto.randomUUID();
    try {
      await patchCommand({
        type: "create",
        patch: {
          id, sourceId: selectedSource.id, name: fourPoint ? "Four Point Patch" : "Rectangle Patch", enabled: true, geometry,
          properties: { repeatMode: "unique", trimCap: false, paddingPx: 4, bleedPx: 8, mapParticipation: "all" },
          rectification: { scale: 1 },
        },
      });
      setActivePatchId(id);
      setPatchTool(null);
      if (selectedRegionId) {
        const next = await applyCommand({ type: "set_region_content", regionId: selectedRegionId, content: { type: "patch", id } });
        setProject(next);
      }
    } catch (reason) { setProblem(failure(reason)); }
  }

  async function assignPatchToRegion(patchId: string, regionId: string) {
    if (!project?.document || activity !== "idle") return;
    setProblem(null);
    try {
      dirtyPreviewRegion.current = regionId;
      const next = await applyCommand({ type: "set_region_content", regionId, content: { type: "patch", id: patchId } });
      setProject(next);
      setArtifact((prior) => retopologizeArtifact(prior, next));
      setSelectedRegionId(regionId);
    } catch (reason) {
      dirtyPreviewRegion.current = null;
      setProblem(failure(reason));
    }
  }

  async function deletePatch(patchId: string) {
    setDraftPatchPreview((draft) => draft?.patchId === patchId ? null : draft);
    setActivePatchId((active) => active === patchId ? null : active);
    setPatchTool(null);
    try {
      await patchCommand({ type: "delete", patchId });
    } catch (reason) {
      setActivePatchId(patchId);
      setProblem(failure(reason));
    }
  }

  async function replacePatchGeometry(patchId: string, geometry: PatchGeometry) {
    try { await patchCommand({ type: "replace_geometry", patchId, geometry }, Date.now()); }
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
    dirtyPreviewRegion.current = regionId;
    try { await command({ type: "set_region_radial", regionId, radial }); }
    catch (reason) { dirtyPreviewRegion.current = null; setProblem(failure(reason)); }
  }

  function chooseSource(sourceSetId: string, channel: SourceChannel) {
    setSelectedSourceSetId(sourceSetId);
    setSelectedChannel(channel);
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
        <nav className="workflow" aria-label="Workbench tabs">
          <button className="mode active">Workbench & Hotspot Sheet</button>
          {sourceFrameLayout ? <button className={`mode ${sourceWorkbenchOpen ? "active" : ""}`} onClick={() => setSourceWorkbenchOpen((open) => !open)}>{sourceWorkbenchOpen ? "Hide Source Workbench" : "Show Source Workbench"}</button> : null}
          <button className="mode" disabled title="Layer and map editing has no document command in this slice.">Layers & Maps</button>
        </nav>
        <span className="window-drag-space" data-tauri-drag-region />
        <div className="publish-actions">
          <button disabled title="Export requires the export document command.">Export</button>
          <button disabled title="Send to Blender requires publish and companion commands.">Send to Blender</button>
        </div>
        {native ? <div className="window-controls">
          <button aria-label="Minimize" onClick={() => void getCurrentWindow().minimize()}>-</button>
          <button aria-label="Maximize or restore" onClick={() => void getCurrentWindow().toggleMaximize()}>[]</button>
          <button aria-label="Close window" onClick={() => void getCurrentWindow().close()}>x</button>
        </div> : null}
      </header>

      <section ref={workbenchRef} className={`workbench pane-layout-${paneMode} ${sourceFrameLayout ? "source-frame-layout" : ""}`} style={{ gridTemplateColumns: workbenchColumns }}>
        {paneMode === "full" || paneMode === "without-inspector" ? <SourceLibrary
          project={project}
          activeSourceSetId={activeSourceSetId}
          selectedSource={selectedSource}
          onSelect={chooseSource}
          onAddSourceSet={() => void addSourceSet()}
          onSetExemplarGroup={(id, group) => void setExemplarGroup(id, group)}
          onSetDelightingIntent={(id, intent) => void setDelightingIntent(id, intent)}
        /> : null}
        {paneMode === "full" || paneMode === "without-inspector" ? <PaneSplitter kind="library-source" paneDrag={paneDrag} setPanes={setPanes} workbenchRef={workbenchRef} /> : null}
        {showSourceWorkspace ? <section className="source-workspace">
          <MapSlots
            sources={activeSources}
            selectedChannel={selectedChannel}
            onSelect={(channel) => setSelectedChannel(channel)}
            onOpen={(channel) => void chooseImages(channel)}
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
            sourceFrame={project?.document?.sourceFrame}
            logicalGrid={project?.document?.logicalGrid}
            partitionRegions={artifact?.regions ?? []}
            selectedSlot={selectedSlot}
            crop={selectedCrop}
            selectedRegion={selectedRegion}
            sourceFrameEditing={sourceFrameEditing}
            importing={activity === "importing"}
            onOpenBase={() => void chooseImages("base_color")}
            onCommitCrop={(bounds) => void setSelectedCrop(bounds)}
            onDraftCrop={previewSelectedCrop}
            onSetSourceFrame={(bounds) => void setSourceFrame(bounds)}
            patches={project?.patches.filter((patch) => patch.sourceId === selectedSource?.id) ?? []}
            activePatchId={activePatchId}
            onEditPatch={setActivePatchId}
            onCommitPatch={(patchId, geometry) => void replacePatchGeometry(patchId, geometry)}
            onDraftPatch={setDraftPatchPreview}
            onDeletePatch={(patchId) => void deletePatch(patchId)}
            onExitPatch={() => { setDraftPatchPreview(null); setActivePatchId(null); }}
            tool={patchTool}
            onCreatePatch={(geometry, fourPoint) => void createPatch(geometry, fourPoint)}
            onCancelTool={() => setPatchTool(null)}
          />
        </section> : null}
        {showSourceWorkspace ? <PaneSplitter kind="source-sheet" sourceOnly={paneMode === "without-library"} paneDrag={paneDrag} setPanes={setPanes} workbenchRef={workbenchRef} /> : null}
        <SheetWorkbench
          project={project}
          artifact={artifact}
          preview={preview}
          preparedPatchPreview={activePatchId ? preparedPatchPreview : null}
          activePatchId={activePatchId}
          mapView={mapView}
          selectedRegionId={selectedRegionId}
          setSelectedRegionId={setSelectedRegionId}
          buildState={buildState}
          problem={problem}
          templateId={templateId}
          setTemplateId={setTemplateId}
          primaryMaterial={primaryMaterial}
          build={build}
          activity={activity}
          setResolution={setResolution}
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
           previewClientTelemetry={previewClientTelemetry}
           onPreviewPaint={(dimensions) => {
             if (previewPublishStartedAt.current !== null) {
               setPreviewClientTelemetry((current) => [
                 ...current.filter((entry) => !entry.startsWith("paint_ms=") && !entry.startsWith("png_decoded_dimensions=")),
                 `png_decoded_dimensions=${dimensions.width}x${dimensions.height}`,
                 `paint_ms=${Math.round(performance.now() - previewPublishStartedAt.current!)}`,
               ]);
             }
           }}
         />
        {paneMode === "full" && !sourceFrameLayout ? <PaneSplitter kind="sheet-inspector" paneDrag={paneDrag} setPanes={setPanes} workbenchRef={workbenchRef} /> : null}
        {paneMode === "full" && !sourceFrameLayout ? <Inspector
          project={project}
          artifact={artifact}
          sourceAnalysis={activePatchId ? preparedPatchPreview : null}
          selectedRegion={selectedRegion}
          mapView={mapView}
          setMapView={setMapView}
          onUndo={() => void history(false)}
          onRedo={() => void history(true)}
          onClassify={(materialSourceId, classificationCommand) => void applyMaterialClassificationCommand(materialSourceId, classificationCommand)}
          onCalibrate={(materialSourceId, calibrationCommand) => void applyMaterialCalibrationCommand(materialSourceId, calibrationCommand)}
          onSetCrop={(regionId, bounds) => void setRegionCrop(regionId, bounds)}
          onSetRadial={(regionId, radial) => void setRegionRadial(regionId, radial)}
          onSetSourceFrame={(bounds) => void setSourceFrame(bounds)}
          sourceFrameEditing={sourceFrameEditing}
          onSetSourceFrameEditing={setSourceFrameEditing}
          onDetachSourceCell={(regionId) => void detachSourceCell(regionId)}
          onResetSourceCell={(regionId) => void resetSourceCell(regionId)}
        /> : null}
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

function SourceLibrary(props: {
  project: ProjectProjection | null;
  activeSourceSetId: string;
  selectedSource: SourceProjection | null;
  onSelect: (sourceSetId: string, channel: SourceChannel) => void;
  onAddSourceSet: () => void;
  onSetExemplarGroup: (materialSourceId: string, exemplarGroup: string | null) => void;
  onSetDelightingIntent: (materialSourceId: string, delighting: DelightingIntent) => void;
}) {
  const sourceSets = props.project?.materialSources ?? [];
  return <aside className="source-library">
    <header className="panel-title"><span>WORKPLACE</span></header>
    <section className="library-section"><div className="section-head"><span>SOURCES</span><b>{sourceSets.length}</b></div>
      {sourceSets.map((set) => {
        const channels = set.registeredChannels?.channels ?? [];
        const base = channels.find((source) => source.channel === "base_color");
        const count = channels.length;
        return <div key={set.id} className="source-set-entry">
          <button className={`source-set ${set.id === props.activeSourceSetId ? "active" : ""}`} onClick={() => props.onSelect(set.id, base?.channel ?? "base_color")}>
            <span className="thumb">{base ? <img src={base.thumbnailDataUrl} alt="" /> : "+"}</span>
            <span><strong>{base?.displayName ?? set.name}</strong><small>{count} map{count === 1 ? "" : "s"} · rev {set.sourceRevision}</small></span>
          </button>
          <input
            key={`${set.id}:${set.sourceRevision}`}
            className="exemplar-group"
            aria-label={`Exemplar group for ${set.name}`}
            defaultValue={set.exemplarGroup ?? ""}
            placeholder="Exemplar group"
            onBlur={(event) => {
              const value = event.currentTarget.value.trim() || null;
              if (value !== set.exemplarGroup) props.onSetExemplarGroup(set.id, value);
            }}
            onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); }}
          />
          <label className="delighting-control">
            <span>De-lighting</span>
            <select
              aria-label={`De-lighting route for ${set.name}`}
              value={set.delighting.route.route}
              onChange={(event) => {
                const route = event.currentTarget.value;
                const nextRoute: DelightingIntent["route"] = route === "classical_low_frequency"
                  ? { route: "classical_low_frequency" }
                  : route === "local_intrinsic_provider"
                    ? { route: "local_intrinsic_provider", provider_id: "local-intrinsic-v1", fallback: "none" }
                    : { route: "pass_through", reason: "user_disabled" };
                props.onSetDelightingIntent(set.id, { ...set.delighting, route: nextRoute });
              }}
            >
              <option value="pass_through">Off / PassThrough</option>
              <option value="classical_low_frequency">Classical low frequency</option>
              <option value="local_intrinsic_provider">Local intrinsic (unavailable)</option>
            </select>
          </label>
          <label className="delighting-strength">
            <span>Strength {Math.round(set.delighting.classical.strengthMilli / 10)}%</span>
            <input
              type="range" min="0" max="1000" step="10"
              value={set.delighting.classical.strengthMilli}
              disabled={set.delighting.route.route !== "classical_low_frequency"}
              onChange={(event) => props.onSetDelightingIntent(set.id, {
                ...set.delighting,
                classical: { ...set.delighting.classical, strengthMilli: Number(event.currentTarget.value) },
              })}
            />
          </label>
        </div>;
      })}
      <button className="new-source" onClick={props.onAddSourceSet}>+ New source</button>
    </section>
    <section className="library-section patches"><div className="section-head"><span>PATCHES</span><b>{props.project?.patches.length ?? 0}</b></div>
      <p>Choose Rectangle or Four Point, then author a patch directly on the source.</p>
    </section>
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
  return <div className="map-slots" onWheel={(event) => {
    if (Math.abs(event.deltaY) > Math.abs(event.deltaX)) event.currentTarget.scrollLeft += event.deltaY;
  }}>
    {channelOptions.map((option) => {
      const source = props.sources.find((candidate) => candidate.channel === option.value);
      const blocked = option.value !== "base_color" && !hasBase;
      return <button
        key={option.value}
        className={`map-slot ${props.selectedChannel === option.value ? "active" : ""} ${source ? "filled" : ""}`}
        disabled={blocked}
        title={blocked ? "Add Base Color to anchor this source set first." : source?.original.path ?? `Add ${option.label}`}
        onClick={() => {
          props.onSelect(option.value);
          if (!source) props.onOpen(option.value);
        }}
      >
        <span className={`channel-swatch ${option.tone}`}>{option.short}</span>
        <span><strong>{option.label}</strong><small>{source?.displayName ?? "+ Add map"}</small></span>
      </button>;
    })}
    <button className="map-slot add-maps" onClick={props.onOpenAll}>Add maps...</button>
  </div>;
}

function useViewportController(content: { width: number; height: number } | null) {
  const containerRef = useRef<HTMLElement | null>(null);
  const [view, setView] = useState<CanvasView>({ x: 0, y: 0, scale: 1 });
  const mode = useRef<"fit" | "manual">("fit");
  const pan = useRef<{ pointerId: number; x: number; y: number; origin: CanvasView } | null>(null);
  function fit() {
    const rect = containerRef.current?.getBoundingClientRect();
    if (!rect || !content) return;
    mode.current = "fit";
    setView(fitView({ width: rect.width, height: rect.height }, content));
  }
  useEffect(() => {
    const element = containerRef.current;
    if (!element || !content) return;
    const observer = new ResizeObserver(() => { if (mode.current === "fit") fit(); });
    observer.observe(element);
    fit();
    return () => observer.disconnect();
  }, [content?.width, content?.height]);
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
  sourceFrameEditing: boolean;
  importing: boolean;
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
  const viewport = useViewportController(props.source?.orientedSize ?? null);
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
  const [draftPatch, setDraftPatch] = useState<{ patchId: string; geometry: PatchGeometry } | null>(null);
  const draftPatchRef = useRef<{ patchId: string; geometry: PatchGeometry } | null>(null);
  const [draftRectangle, setDraftRectangle] = useState<PatchGeometry | null>(null);
  const [fourPointDraft, setFourPointDraft] = useState<Array<{ x: number; y: number }>>([]);
  const [pointEditPatchId, setPointEditPatchId] = useState<string | null>(null);
  const [loupePoint, setLoupePoint] = useState<{ x: number; y: number; corner: number; clientX: number; clientY: number } | null>(null);
  const [patchMenu, setPatchMenu] = useState<{ patchId: string; clientX: number; clientY: number } | null>(null);
  const committedCrop = props.selectedSlot?.mappingOrigin === "explicit_override"
    ? props.selectedSlot.sourceBounds ?? props.crop?.bounds ?? null
    : null;
  const effectiveCrop = draftCrop ?? committedCrop;
  const effectiveFrame = draftFrame ?? props.sourceFrame?.bounds ?? null;

  useEffect(() => {
    setDraftCrop(null);
    draftCropRef.current = null;
    setDraftFrame(null);
  }, [props.crop?.bounds.x, props.crop?.bounds.y, props.crop?.bounds.width, props.crop?.bounds.height, props.sourceFrame?.identity.join(",")]);

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
      {props.selectedRegion && props.selectedSlot?.mappingOrigin === "partition" ? <div className="partition-selection-status" data-selection-status="partition-owned">
        Partition-owned — Detach to adjust
      </div> : null}
    </div> : <div className="empty-source-canvas">
      <strong>Open or drop a Base Color</strong>
      <span>The source canvas is ready before the project has a save location.</span>
      <button className="primary" onClick={props.onOpenBase}>Open Base Color</button>
    </div>}
    {props.source ? <svg
      className="patch-overlay"
      style={{ left: viewport.view.x, top: viewport.view.y, width: props.source.orientedSize.width * viewport.view.scale, height: props.source.orientedSize.height * viewport.view.scale }}
      viewBox={`0 0 ${props.source.orientedSize.width} ${props.source.orientedSize.height}`}
      aria-label="Editable patch outlines"
    >
      {effectiveFrame ? <g
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
      {effectiveCrop ? <g className="patch-outline active source-crop-transform">
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
        return <g key={patch.id} className={`patch-outline ${active ? "active" : ""} ${pointEditing ? "point-editing" : ""}`} onContextMenu={(event) => openPatchMenu(event, patch)}>
          <polygon
            points={points}
            onPointerDown={(event) => beginPatchMove(event, patch)}
            onClick={(event) => { event.stopPropagation(); props.onEditPatch(patch.id); }}
            onDoubleClick={(event) => { event.stopPropagation(); props.onEditPatch(patch.id); setPointEditPatchId(patch.id); }}
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
    {props.importing ? <div className="canvas-state">Importing source...</div> : null}
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
  activePatchId: string | null;
  mapView: CompiledMapView;
  selectedRegionId: string | null;
  setSelectedRegionId: (id: string | null) => void;
  buildState: string;
  problem: CommandFailure | null;
  templateId: string;
  setTemplateId: (id: string) => void;
  primaryMaterial: string;
  build: () => void;
  activity: Activity;
  setResolution: (size: number) => void;
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
  previewClientTelemetry: readonly string[];
  onPreviewPaint: (dimensions: { width: number; height: number }) => void;
}) {
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
  const sheetMatchesDocument = !!sheet && sheet.topologyHash === topologyHash;
  const requestedFamilies = Object.values(props.candidateRecipe.composition).reduce((total, value) => total + (typeof value === "object" && "count" in value ? value.count : 0), 0);
  const requestedBudget = props.candidateRecipe.composition.broadPanels.count * props.candidateRecipe.composition.broadPanels.subdivisionBudget
    + props.candidateRecipe.composition.mediumBlocks.count * props.candidateRecipe.composition.mediumBlocks.subdivisionBudget
    + props.candidateRecipe.composition.smallDetails.count * props.candidateRecipe.composition.smallDetails.subdivisionBudget;
  const hierarchical = props.candidateRecipe.hierarchical;
  const requestedArea = hierarchical
    ? hierarchical.largeShareMilli + hierarchical.mediumShareMilli + hierarchical.smallShareMilli + hierarchical.stripShareMilli + hierarchical.radialShareMilli
    : props.candidateRecipe.composition.broadPanels.areaShareMilli + props.candidateRecipe.composition.mediumBlocks.areaShareMilli + props.candidateRecipe.composition.smallDetails.areaShareMilli;
  const requestedFloor = hierarchical ? hierarchical.targetRegionMin : requestedFamilies + requestedBudget + (requestedFamilies > 0 ? 1 : 0);
  const requestedMaximum = hierarchical?.targetRegionMax ?? props.candidateRecipe.targetRegionCount;
  const candidateValid = hierarchical
    ? requestedArea === 1000 && hierarchical.targetRegionMin >= 24 && hierarchical.targetRegionMin <= hierarchical.targetRegionMax
      && hierarchical.protectedParentCount + hierarchical.subdividableParentCount <= hierarchical.macroParentCount
      && hierarchical.allowedSplitRatios.length > 0 && hierarchical.stripThicknessLadder.length > 0
    : requestedFloor <= props.candidateRecipe.targetRegionCount && requestedArea <= 1000;
  const displayedGrid = props.candidatePreviewing ? props.candidatePreviewRecipe?.grid ?? props.candidateRecipe.grid : props.project?.document?.logicalGrid ?? props.candidateRecipe.grid;
  const candidateState = props.activity === "compiling" ? "Generating"
    : !candidateValid ? "Invalid"
    : props.candidatePreviewing && props.candidateIsCurrent ? "Candidate ready"
    : props.candidatePreviewing ? "Draft changed"
    : "Accepted";
  const [layoutMenu, setLayoutMenu] = useState<{ regionId: string; x: number; y: number } | null>(null);
  const [layoutTool, setLayoutTool] = useState<"select" | "draw">("select");
  const [gridVisible, setGridVisible] = useState(true);
  const [gridOpacity, setGridOpacity] = useState(10);
  const [textureVisible, setTextureVisible] = useState(true);
  const [regionFillVisible, setRegionFillVisible] = useState(true);
  const [resizeDraft, setResizeDraft] = useState<{ pointerId: number; regionId: string; handle: ResizeHandle; origin: LogicalRect; rect: LogicalRect } | null>(null);
  const [drawDraft, setDrawDraft] = useState<{ pointerId: number; startX: number; startY: number; endX: number; endY: number } | null>(null);
  const sheetRef = useRef<HTMLDivElement>(null);
  const displayRegions = sheet?.regions ?? [];
  const selectedGridRect = resizeDraft?.rect ?? displayRegions.find((region) => region.regionId === props.selectedRegionId)?.gridRect;
  const selectedContent = props.selectedRegionId ? props.project?.document?.regionBindings[props.selectedRegionId]?.content : null;
  const selectedPatchAssigned = !!props.activePatchId && selectedContent?.type === "patch" && selectedContent.id === props.activePatchId;
  const drawRect = drawDraft ? normalizedGridRect(drawDraft) : null;
  const resizeTransfers = useMemo(() => resizeDraft ? previewResizeOwnershipTransfers(displayRegions, resizeDraft.regionId, resizeDraft.origin, resizeDraft.rect, displayedGrid) : [], [displayRegions, resizeDraft, displayedGrid]);
  const resizeAffectedIds = useMemo(() => new Set(resizeTransfers.flatMap((transfer) => [transfer.fromId, transfer.toId])), [resizeTransfers]);
  const sourceFrame = props.project?.document?.sourceFrame;
  const sourceTexture = sourceFrame
    ? props.project?.materialSources.find((source) => source.id === sourceFrame.sourceSetId)?.registeredChannels?.channels.find((channel) => channel.channel === "base_color")?.thumbnailDataUrl
    : null;
  const continuousTexture = props.mapView === "baseColor" ? sourceTexture : null;
  const editorHasImage = props.mapView === "baseColor" ? !!continuousTexture : !!imageUrl;
  const sourceTextureProblem = props.mapView === "baseColor" && sourceFrame && !continuousTexture
    ? "The complete oriented Source Frame Base Color is unavailable. Layout editing will not display a partial Stage 14 map. Re-register Base Color for this Source Frame."
    : null;
  const candidateFingerprint = partitionRecipeFingerprint(props.candidateRecipe);
  const lastAutoPreviewFingerprint = useRef<string | null>(null);
  const workpieceSize = sheet
    ? { width: sheet.width, height: sheet.height }
    : props.preparedPatchPreview
      ? { width: props.preparedPatchPreview.width, height: props.preparedPatchPreview.height }
      : null;
  const viewport = useViewportController(workpieceSize);
  const gridSteps = adaptiveGridSteps(displayedGrid, sheet?.width ?? 1, sheet?.height ?? 1, viewport.view.scale);
  useEffect(() => {
    if (!props.project?.document || !candidateValid || props.activity !== "idle" || props.candidateIsCurrent) return;
    if (lastAutoPreviewFingerprint.current === candidateFingerprint) return;
    const timer = window.setTimeout(() => {
      lastAutoPreviewFingerprint.current = candidateFingerprint;
      props.previewCandidate(props.candidateRecipe);
    }, 250);
    return () => window.clearTimeout(timer);
  }, [candidateFingerprint, candidateValid, props.activity, props.candidateIsCurrent, props.project?.document]);
  function pointerGridPoint(clientX: number, clientY: number) {
    const bounds = sheetRef.current?.getBoundingClientRect();
    if (!bounds) return null;
    return {
      x: Math.max(0, Math.min(displayedGrid.width, Math.round((clientX - bounds.left) / bounds.width * displayedGrid.width))),
      y: Math.max(0, Math.min(displayedGrid.height, Math.round((clientY - bounds.top) / bounds.height * displayedGrid.height))),
    };
  }
  function moveDirectEdit(event: React.PointerEvent<HTMLElement>) {
    const point = pointerGridPoint(event.clientX, event.clientY);
    if (!point) return false;
    if (resizeDraft?.pointerId === event.pointerId) {
      setResizeDraft((current) => current ? { ...current, rect: resizeGridRect(current.origin, current.handle, point, displayedGrid) } : null);
      return true;
    }
    if (drawDraft?.pointerId === event.pointerId) {
      setDrawDraft((current) => current ? { ...current, endX: point.x, endY: point.y } : null);
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
      const rect = normalizedGridRect(drawDraft);
      if (cancel || rect.width === 0 || rect.height === 0) { setDrawDraft(null); return; }
      void props.onLayoutCommand({ type: "draw_source_frame_region", gridRect: rect }).then((next) => {
        const drawn = next?.document?.topology.regions.find((region) => region.gridRect && sameGridRect(region.gridRect, rect));
        if (drawn) props.setSelectedRegionId(drawn.id);
      }).finally(() => setDrawDraft(null));
    }
  }
  return <section className="sheet-workbench">
    <header className="sheet-header">
      <div><strong>HOTSPOT SHEET</strong></div>
      <span className={`build-status ${props.problem ? "error" : props.artifact ? "ready" : ""}`}>{props.buildState}</span>
    </header>
    <section className="layout-stage">
    <aside className="layout-sidebar" aria-label="Layout controls">
      <header><span>LAYOUT</span><strong className={`layout-state ${candidateState.toLowerCase().replaceAll(" ", "-")}`}>{candidateState}</strong></header>
      <div className="layout-tool-row" role="toolbar" aria-label="Atlas editing tools"><button className={layoutTool === "select" ? "active" : ""} onClick={() => { setLayoutTool("select"); setDrawDraft(null); }}>Select / resize</button><button className={layoutTool === "draw" ? "active" : ""} onClick={() => { setLayoutTool("draw"); setLayoutMenu(null); }}>Draw region</button></div>
      <p className="layout-help">Draw snapped rectangles or resize the selected box with its handles. Middle-drag pans in either tool; texture pixels never recompile for direct edits.</p>
      <div className="display-controls"><label><input type="checkbox" checked={textureVisible} onChange={(event) => setTextureVisible(event.target.checked)} /> Texture</label><label><input type="checkbox" checked={regionFillVisible} onChange={(event) => setRegionFillVisible(event.target.checked)} /> Region colors</label></div>
      <div className="grid-controls"><label><input type="checkbox" checked={gridVisible} onChange={(event) => setGridVisible(event.target.checked)} /> Grid</label><label>Opacity <input aria-label="Grid opacity" type="range" min={0} max={100} value={gridOpacity} onChange={(event) => setGridOpacity(Number(event.target.value))} /></label></div>
      {selectedGridRect ? <output className="selection-readout">Selected: x {selectedGridRect.x}, y {selectedGridRect.y} · {selectedGridRect.width} × {selectedGridRect.height}</output> : drawRect ? <output className="selection-readout draw">Drawing: x {drawRect.x}, y {drawRect.y} · {drawRect.width} × {drawRect.height}</output> : <output className="selection-readout">No region selected</output>}
      <p className={`layout-capacity ${candidateValid ? "" : "invalid"}`} aria-live="polite">{hierarchical ? `${requestedFloor}–${requestedMaximum} soft region range` : `${requestedFloor} minimum leaves / ${props.candidateRecipe.targetRegionCount} Count`} · {requestedArea / 10}% unified area</p>
      {props.problem ? <p className="layout-diagnostic" role="alert">{props.problem.message}<small>{props.problem.recovery}</small></p> : null}
      {sourceTextureProblem ? <p className="layout-diagnostic" role="alert">{sourceTextureProblem}</p> : null}
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
      <details className="layout-advanced material-preview-settings"><summary>Optional material preview</summary>
        <label>Output resolution<select value={props.project?.document?.renderSettings.outputSize.width ?? 2048} onChange={(event) => void props.setResolution(Number(event.target.value))} disabled={!props.project?.document}>
          <option value={1024}>1024</option><option value={2048}>2048</option><option value={4096}>4096</option>
        </select></label>
        <button onClick={props.build} disabled={!props.project?.document || props.activity !== "idle"}>Rebuild material maps</button>
        <small>Layout drawing and resizing never require this rebuild.</small>
      </details>
      <section className="layout-editing">
        <strong>Direct atlas editing</strong>
        <p>{props.candidatePreviewing
          ? "This generated candidate is read-only. Accept it to resize, draw, split, or merge its regions."
          : layoutTool === "draw" ? "Drag anywhere on the atlas to place an exact snapped rectangle." : "Select a region, drag an edge handle continuously, or right-click for split/merge."}</p>
        {props.candidatePreviewing ? <button className="primary" onClick={() => props.acceptCandidate(props.candidateRecipe)} disabled={!props.candidateIsCurrent || props.activity !== "idle"}>Accept candidate and edit</button> : null}
        <div className="layout-history"><button onClick={props.onUndo} disabled={!props.project?.canUndoDocument || props.activity !== "idle"}>Undo</button><button onClick={props.onRedo} disabled={!props.project?.canRedoDocument || props.activity !== "idle"}>Redo</button></div>
        <button onClick={() => props.selectedRegionId && props.onLayoutCommand({ type: "split_source_frame_region", regionId: props.selectedRegionId, axis: "horizontal" })} disabled={!props.selectedRegionId || props.candidatePreviewing || props.activity !== "idle"}>Split horizontally</button>
        <button onClick={() => props.selectedRegionId && props.onLayoutCommand({ type: "split_source_frame_region", regionId: props.selectedRegionId, axis: "vertical" })} disabled={!props.selectedRegionId || props.candidatePreviewing || props.activity !== "idle"}>Split vertically</button>
      </section>
    </aside>
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
      {!sheet || !editorHasImage ? sourceTextureProblem ? <div className="empty-sheet source-texture-error"><strong>Complete Base Color unavailable</strong><span>{sourceTextureProblem}</span></div> : props.preparedPatchPreview ? <div
        className="rectified-workpiece"
        style={{ width: props.preparedPatchPreview.width, height: props.preparedPatchPreview.height, transform: `translate(${viewport.view.x}px, ${viewport.view.y}px) scale(${viewport.view.scale})` }}
      >
        <img src={props.preparedPatchPreview.dataUrl} alt="Selected Stage 3 rectified patch" />
        <svg className="orientation-overlay" viewBox={`0 0 ${props.preparedPatchPreview.width} ${props.preparedPatchPreview.height}`} aria-label="Source-pixel orientation field">
          {props.preparedPatchPreview.sourceAnalysis.orientationOverlay.map((sample, index) => {
            if (sample.axisMillidegrees === null) return null;
            const x = sample.sourceXMilli / 1000;
            const y = sample.sourceYMilli / 1000;
            const radians = sample.axisMillidegrees / 1000 * Math.PI / 180;
            const radius = 7;
            return <line key={index} x1={x - Math.cos(radians) * radius} y1={y - Math.sin(radians) * radius} x2={x + Math.cos(radians) * radius} y2={y + Math.sin(radians) * radius} />;
          })}
        </svg>
        <span>Rectified patch</span>
      </div> : <div className="empty-sheet">
        <strong>{props.project?.legacyLayoutDiscarded ? "No trim sheet yet" : "No compiled sheet"}</strong>
        <span>{props.project?.legacyLayoutDiscarded ? "Sources, maps, and patches were preserved. Old layout state is not shown or converted." : "Build from the current Base Color when ready."}</span>
      </div> : <div
        ref={sheetRef}
        className="sheet"
        style={{ width: sheet.width, height: sheet.height, transform: `translate(${viewport.view.x}px, ${viewport.view.y}px) scale(${viewport.view.scale})` }}
        onPointerDown={(event) => {
          if (event.button !== 0) return;
          if (layoutTool === "draw" && !props.candidatePreviewing && props.activity === "idle") {
            const point = pointerGridPoint(event.clientX, event.clientY);
            if (!point) return;
            event.preventDefault();
            event.currentTarget.setPointerCapture(event.pointerId);
            props.setSelectedRegionId(null);
            setDrawDraft({ pointerId: event.pointerId, startX: point.x, startY: point.y, endX: point.x, endY: point.y });
          } else if (!(event.target as Element).closest(".region")) props.setSelectedRegionId(null);
        }}
      >
        {textureVisible && continuousTexture && sourceFrame ? <div className="source-frame-texture"><img src={continuousTexture} alt="Source Frame texture" style={sourceFrameTextureStyle(sourceFrame)} onLoad={() => props.onPreviewPaint({ width: sheet.width, height: sheet.height })} /></div> : textureVisible && imageUrl ? <img src={imageUrl} alt={`${props.mapView} trim sheet preview`} onLoad={(event) => props.onPreviewPaint({ width: event.currentTarget.naturalWidth, height: event.currentTarget.naturalHeight })} /> : null}
        {selectedPatchAssigned && props.preparedPatchPreview && selectedGridRect ? <div className="assigned-patch-preview" style={gridRectOverlayStyle(selectedGridRect, displayedGrid)}><img src={props.preparedPatchPreview.dataUrl} alt="Assigned patch preview" /></div> : null}
        {(sheetMatchesDocument || props.candidatePreviewing) && gridVisible ? <><div className="sheet-grid minor" style={{ backgroundSize: `${gridSteps.minorX * 100 / displayedGrid.width}% ${gridSteps.minorY * 100 / displayedGrid.height}%`, opacity: gridOpacity / 100 * .9 }} /><div className="sheet-grid major" style={{ backgroundSize: `${gridSteps.majorX * 100 / displayedGrid.width}% ${gridSteps.majorY * 100 / displayedGrid.height}%`, opacity: gridOpacity / 100 }} /></> : null}
        <div className={`overlays ${layoutTool === "draw" ? "drawing" : ""}`}>{displayRegions.map((region) => <button key={region.regionId}
          data-region-id={region.regionId}
          data-selection-surface="atlas"
          aria-label={`${region.displayName}${region.gridRect ? `, x ${region.gridRect.x}, y ${region.gridRect.y}, ${region.gridRect.width} by ${region.gridRect.height}` : ""}`}
          aria-pressed={region.regionId === props.selectedRegionId}
          className={`region ${region.regionId === props.selectedRegionId ? "selected" : ""} ${resizeDraft && (region.regionId === resizeDraft.regionId || resizeAffectedIds.has(region.regionId)) ? "affected" : ""}`}
          style={overlayStyle(region, sheet, viewport.view.scale, regionFillVisible ? 0.2 : 0)}
          onClick={(event) => { event.stopPropagation(); if (!props.candidatePreviewing && layoutTool !== "draw") props.setSelectedRegionId(region.regionId); }}
          onContextMenu={(event) => { event.preventDefault(); if (props.candidatePreviewing) return; event.stopPropagation(); props.setSelectedRegionId(region.regionId); setLayoutMenu({ regionId: region.regionId, x: event.clientX, y: event.clientY }); }}
        ><span className="region-label">{region.displayName}</span>{region.regionId === props.selectedRegionId && region.gridRect && layoutTool === "select" && !props.candidatePreviewing ? resizeHandles.map((handle) => <i key={handle} className={`selection-handle ${handle}`} aria-label={`Resize ${handle}`} onPointerDown={(event) => { event.preventDefault(); event.stopPropagation(); event.currentTarget.setPointerCapture(event.pointerId); setResizeDraft({ pointerId: event.pointerId, regionId: region.regionId, handle, origin: region.gridRect!, rect: region.gridRect! }); }} />) : null}</button>)}</div>
        {drawRect && drawRect.width > 0 && drawRect.height > 0 ? <div className="draw-region-preview" style={gridRectOverlayStyle(drawRect, displayedGrid)}><span>{drawRect.x}, {drawRect.y} · {drawRect.width} × {drawRect.height}</span></div> : null}
        {resizeTransfers.map((transfer, index) => { const owner = displayRegions.find((region) => region.regionId === transfer.toId); const from = displayRegions.find((region) => region.regionId === transfer.fromId); return <div key={`${transfer.fromId}-${transfer.toId}-${index}`} className={`ownership-transfer ${transfer.toId === resizeDraft?.regionId ? "gained" : "released"}`} style={{ ...gridRectOverlayStyle(transfer.rect, displayedGrid), borderColor: owner ? `rgb(${owner.idColor.join(" ")})` : undefined, backgroundColor: owner ? `rgb(${owner.idColor.join(" ")} / .38)` : undefined }}><span>{from?.displayName ?? "Region"} → {owner?.displayName ?? "Region"}</span></div>; })}
        {resizeDraft && !sameGridRect(resizeDraft.origin, resizeDraft.rect) ? <div className="resize-region-preview" style={gridRectOverlayStyle(resizeDraft.rect, displayedGrid)}><span>{resizeDraft.rect.x}, {resizeDraft.rect.y} / {resizeDraft.rect.width} x {resizeDraft.rect.height}</span></div> : null}
        {!props.candidatePreviewing && layoutMenu ? <div className="layout-menu" style={{ left: layoutMenu.x, top: layoutMenu.y }} role="menu"><button onClick={() => { props.onLayoutCommand({ type: "split_source_frame_region", regionId: layoutMenu.regionId, axis: "horizontal" }); setLayoutMenu(null); }}>Split horizontally</button><button onClick={() => { props.onLayoutCommand({ type: "split_source_frame_region", regionId: layoutMenu.regionId, axis: "vertical" }); setLayoutMenu(null); }}>Split vertically</button>{mergeCandidate(sheet.regions, layoutMenu.regionId) ? <button onClick={() => { const sibling = mergeCandidate(sheet.regions, layoutMenu.regionId)!; props.onLayoutCommand({ type: "merge_source_frame_regions", regionId: layoutMenu.regionId, siblingId: sibling.regionId }); setLayoutMenu(null); }}>Merge / Remove Divider</button> : null}</div> : null}
      </div>}
      {workpieceSize ? <div className="viewport-tools">
        <button onClick={() => viewport.zoom(0.8)}>-</button>
        <output>{Math.round(viewport.view.scale * 100)}%</output>
        <button onClick={() => viewport.zoom(1.25)}>+</button>
        <button onClick={viewport.fit}>Fit</button>
      </div> : null}
    </section>
    </section>
    {props.artifact ? <footer className="artifact-footer">
      <span>{props.artifact.width} x {props.artifact.height}</span>
      <span>{props.artifact.regions.length} regions</span>
      <span>{props.artifact.label}</span>
      <span>incomplete after Stage {props.artifact.incompleteAfterStage} · non-exportable</span>
      <span>pending: {props.artifact.pending.join(", ")}</span>
      {props.artifact.telemetry.length > 0 || props.previewClientTelemetry.length > 0 ? <details className="preview-telemetry">
        <summary>Preview telemetry</summary>
        <pre>{[...props.artifact.telemetry, ...props.previewClientTelemetry].join("\n")}</pre>
      </details> : null}
    </footer> : null}
  </section>;
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
  onSetCrop: (regionId: string, bounds: NormalizedBounds) => void;
  onSetRadial: (regionId: string, radial: NonNullable<RegionMapping["radial"]>) => void;
  onSetSourceFrame: (bounds: NormalizedBounds) => void;
  sourceFrameEditing: boolean;
  onSetSourceFrameEditing: (editing: boolean) => void;
  onDetachSourceCell: (regionId: string) => void;
  onResetSourceCell: (regionId: string) => void;
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
  return <aside className={`context-inspector ${layoutMode ? "layout-mode" : ""}`}>
    <header className="inspector-actions"><button onClick={props.onUndo} disabled={!props.project?.canUndoDocument}>Undo</button><button onClick={props.onRedo} disabled={!props.project?.canRedoDocument}>Redo</button></header>
    {layoutMode ? <section className="inspector-section layout-summary"><span>LAYOUT MODE</span><p>Composition, candidate state, and atlas editing are in the Layout sidebar. Undo and redo remain available here.</p></section> : null}
    <section className="inspector-section">
      <span>MAP VIEW</span>
      <div className="map-view-grid">{mapViews.map(([id, label]) => <button key={id} className={props.mapView === id ? "active" : ""} onClick={() => props.setMapView(id)} disabled={!props.artifact?.maps[id]} title={props.artifact && !props.artifact.maps[id] ? "Unavailable through Stage 14" : undefined}>{label}</button>)}</div>
    </section>
    {stage14Slot ? <section className="inspector-section">
      <span>AUTHORITATIVE STAGE 14 SLOT</span>
      <dl>
        <dt>Slot</dt><dd>{stage14Slot.displayName}</dd>
        <dt>Mapping</dt><dd>{stage14Slot.mappingMode}</dd>
        <dt>Validity</dt><dd>{stage14Slot.validity}</dd>
        <dt>Correspondence</dt><dd>{stage14Slot.correspondence}</dd>
        <dt>Patch</dt><dd>{stage14Slot.patchId ?? "whole registered source"}</dd>
        <dt>Domain</dt><dd>{stage14Slot.domainId.slice(0, 12)}</dd>
        <dt>Candidate</dt><dd>{stage14Slot.candidateId.slice(0, 12)}</dd>
        <dt>SamplingPlan</dt><dd>{stage14Slot.samplingPlanId.slice(0, 12)}</dd>
        <dt>Stage 14 result</dt><dd>{stage14Slot.stage14ResultId.slice(0, 12)}</dd>
      </dl>
    </section> : null}
    <section className="inspector-section">
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
    {props.project?.document?.sourceFrame ? <SourceFrameEditor
      frame={props.project.document.sourceFrame}
      onApply={props.onSetSourceFrame}
      editing={props.sourceFrameEditing}
      onEditingChange={props.onSetSourceFrameEditing}
    /> : null}
    <section className="inspector-section">
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
        {stage14Slot?.mappingOrigin === "partition" ? <button onClick={() => props.onDetachSourceCell(props.selectedRegion!.regionId)}>Detach Source Cell</button> : null}
        {stage14Slot?.mappingOrigin === "explicit_override" ? <button onClick={() => props.onResetSourceCell(props.selectedRegion!.regionId)}>Reset to Partition</button> : null}
        {overlapIds.length > 0 ? <p className="source-overlap-warning">Explicit override overlaps: {overlapIds.join(", ")}</p> : null}
        {stage14Slot?.mappingOrigin === "explicit_override" && binding?.mapping.projection.type === "crop" ? <CropEditor
          key={`${props.selectedRegion.regionId}-crop`}
          regionId={props.selectedRegion.regionId}
          bounds={binding.mapping.projection.bounds}
          aspect={props.project?.document?.sourceFrame ? sourceCropAspect(
            stage14Slot,
            props.project.document.sourceFrame.orientedDimensions.width,
            props.project.document.sourceFrame.orientedDimensions.height,
          ) : 1}
          onApply={props.onSetCrop}
        /> : null}
        {props.selectedRegion.role === "radial" && binding?.mapping.radial ? <RadialEditor
          key={`${props.selectedRegion.regionId}-radial`}
          regionId={props.selectedRegion.regionId}
          radial={binding.mapping.radial}
          onApply={props.onSetRadial}
        /> : null}
      </> : <p>Select a patch or create one on the source workbench.</p>}
    </section>
    <LockedSection title="Profiles & Weathering" reason="Generated-map recipes are not command-backed in this slice." />
    <LockedSection title="Decorations" reason="Decoration bindings require authored patch commands." />
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

function CropEditor(props: { regionId: string; bounds: NormalizedBounds; aspect: number; onApply: (regionId: string, bounds: NormalizedBounds) => void }) {
  const [bounds, setBounds] = useState(props.bounds);
  useEffect(() => setBounds(props.bounds), [props.bounds.x, props.bounds.y, props.bounds.width, props.bounds.height]);
  const set = (field: keyof NormalizedBounds, value: number) => setBounds((current) => constrainAspectBounds(
    { ...current, [field]: value }, props.aspect, field === "height" ? "height" : "width",
  ));
  return <div className="mapping-editor">
    <strong>SOURCE CROP</strong>
    <small>Aspect locked to the destination region.</small>
    {(["x", "y", "width", "height"] as const).map((field) => <label key={field}>{field}<input type="number" min={0} max={1} step={0.01} value={Number(bounds[field].toFixed(4))} onChange={(event) => set(field, Number(event.target.value))} /></label>)}
    <button onClick={() => props.onApply(props.regionId, constrainAspectBounds(bounds, props.aspect))}>Apply crop</button>
  </div>;
}

function RadialEditor(props: { regionId: string; radial: NonNullable<RegionMapping["radial"]>; onApply: (regionId: string, radial: NonNullable<RegionMapping["radial"]>) => void }) {
  const [radial, setRadial] = useState(props.radial);
  const fields: ReadonlyArray<[keyof typeof radial, string, number, number, number]> = [
    ["centerX", "Center X", 0, 1, 0.01], ["centerY", "Center Y", 0, 1, 0.01],
    ["innerRadius", "Inner", 0, 1.99, 0.01], ["outerRadius", "Outer", 0.01, 2, 0.01],
    ["falloff", "Falloff", 0.1, 4, 0.1],
  ];
  return <div className="mapping-editor radial-editor">
    <strong>RADIAL PROJECTION</strong>
    {fields.map(([field, label, min, max, step]) => <label key={field}>{label}<input type="number" min={min} max={max} step={step} value={Number(radial[field].toFixed(3))} onChange={(event) => setRadial((current) => ({ ...current, [field]: Number(event.target.value) }))} /></label>)}
    <button onClick={() => props.onApply(props.regionId, radial)}>Apply radial</button>
  </div>;
}

function LockedSection({ title, reason }: { title: string; reason: string }) {
  return <section className="locked"><strong>{title}</strong><span>{reason}</span></section>;
}

function buildStatus(project: ProjectProjection | null, artifact: IntermediateAtlasProjection | null, activity: Activity, problem: CommandFailure | null, stale: boolean) {
  if (activity === "importing") return "Importing";
  if (activity === "compiling") return `Compiling revision ${project?.document?.documentRevision ?? 1}`;
  if (activity === "editing") return "Committing layout metadata";
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
    return {
      regionId: definition.id,
      displayName: definition.displayName,
      allocationBounds: previewBounds,
      hotspotBounds: previewBounds,
      idColor: existing?.idColor ?? stableRegionColor(definition.id),
      materialId: existing?.materialId ?? document.primaryMaterial ?? fallback?.materialId ?? "unassigned",
      materialIdColor: existing?.materialIdColor ?? fallback?.materialIdColor ?? [128, 128, 128],
      mapping: binding?.mapping ?? existing?.mapping ?? fallback!.mapping,
      role: definition.role,
      gridRect: definition.gridRect,
      sourceCrop: existing?.sourceCrop,
      sourceBounds: existing?.sourceBounds,
      mappingOrigin: existing?.mappingOrigin ?? "partition",
    };
  });
  const regionById = new Map(regions.map((region) => [region.regionId, region]));
  return {
    ...prior,
    revision: document.documentRevision,
    documentRevision: document.documentRevision,
    topologyHash: hashBytes(document.topology.topologyHash),
    topology: document.topology,
    regions,
    slots: prior.slots.flatMap((slot) => {
      const region = regionById.get(slot.regionId);
      return region ? [{ ...slot, displayName: region.displayName, allocationBounds: region.allocationBounds, hotspotBounds: region.hotspotBounds, gridRect: region.gridRect }] : [];
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

function normalizedGridRect(draft: { startX: number; startY: number; endX: number; endY: number }): LogicalRect {
  return { x: Math.min(draft.startX, draft.endX), y: Math.min(draft.startY, draft.endY), width: Math.abs(draft.endX - draft.startX), height: Math.abs(draft.endY - draft.startY) };
}

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

function overlayStyle(region: ResolvedRegion, artifact: Pick<IntermediateAtlasProjection, "width" | "height">, scale = 1, fillOpacity = 0): React.CSSProperties {
  const bounds = region.allocationBounds;
  return {
    left: `${bounds.x / artifact.width * 100}%`,
    top: `${bounds.y / artifact.height * 100}%`,
    width: `${bounds.width / artifact.width * 100}%`,
    height: `${bounds.height / artifact.height * 100}%`,
    borderColor: `rgb(${region.idColor[0]} ${region.idColor[1]} ${region.idColor[2]})`,
    "--region-fill": `rgb(${region.idColor[0]} ${region.idColor[1]} ${region.idColor[2]} / ${fillOpacity})`,
    "--region-stroke": `${Math.min(3, Math.max(0.75, 1 / scale))}px`,
    "--region-label-size": `${Math.min(16, Math.max(7, 10 / scale))}px`,
  } as React.CSSProperties;
}

createRoot(document.getElementById("root")!).render(<React.StrictMode><App /></React.StrictMode>);
