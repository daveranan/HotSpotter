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
  type Stage14SlotProjection,
  type TrimSheetDocumentCommand,
} from "@hot-trimmer/ipc-contracts";
import { assignSourceFiles } from "./source-assignment";
import { adjustCrop, anchoredZoom, clamp01, constrainAspectBounds, fitSourceFrame, fitView, movePatch, normalizePatchToRectangle, patchBounds, patchPointerAngle, resizeAspectLocked, resizePatch, resizePanes, rotatePatch, type CanvasView, type CropDragAction, type PaneDragKind, type PaneState, type PatchResizeHandle } from "./source-workbench-geometry";
import { SourceFramePreviewController } from "./source-frame-preview-controller";
import "./document-app.css";

const protocol = { protocolVersion: IPC_PROTOCOL_VERSION };

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

type Activity = "starting" | "idle" | "importing" | "compiling" | "saving" | "opening";
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
  const workbenchColumns = paneMode === "full"
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

  async function requestPreview(regionId?: string, projection?: CropProjection, profile: "draft512" | "refinement1024" = "draft512", revision?: number) {
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
        if (profile === "draft512") {
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
    if (!selectedRegionId || !selectedCrop) return;
    const projection: CropProjection = {
      ...selectedCrop,
      bounds,
      focus: { x: bounds.x + bounds.width * 0.5, y: bounds.y + bounds.height * 0.5 },
    };
    void requestPreview(selectedRegionId, projection);
  }

  async function history(redo: boolean) {
    try {
      const next = await invoke<ProjectProjection>(redo ? "redo_document_command" : "undo_document_command", { request: protocol });
      setProject(next);
      setArtifact(null);
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

      <section ref={workbenchRef} className={`workbench pane-layout-${paneMode}`} style={{ gridTemplateColumns: workbenchColumns }}>
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
        {paneMode !== "sheet-only" ? <section className="source-workspace">
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
        {paneMode !== "sheet-only" ? <PaneSplitter kind="source-sheet" sourceOnly={paneMode === "without-library"} paneDrag={paneDrag} setPanes={setPanes} workbenchRef={workbenchRef} /> : null}
        <SheetWorkbench
          project={project}
          artifact={artifact}
          preview={preview}
          preparedPatchPreview={activePatchId ? preparedPatchPreview : null}
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
        {paneMode === "full" ? <PaneSplitter kind="sheet-inspector" paneDrag={paneDrag} setPanes={setPanes} workbenchRef={workbenchRef} /> : null}
        {paneMode === "full" ? <Inspector
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
  const previewFrame = useRef<number | null>(null);
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
      if (previewFrame.current === null) {
        previewFrame.current = requestAnimationFrame(() => {
          previewFrame.current = null;
          if (draftCropRef.current) props.onDraftCrop(draftCropRef.current);
        });
      }
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
  const workpieceSize = sheet
    ? { width: sheet.width, height: sheet.height }
    : props.preparedPatchPreview
      ? { width: props.preparedPatchPreview.width, height: props.preparedPatchPreview.height }
      : null;
  const viewport = useViewportController(workpieceSize);
  return <section className="sheet-workbench">
    <header className="sheet-header">
      <div><strong>HOTSPOT SHEET</strong></div>
      <span className={`build-status ${props.problem ? "error" : props.artifact ? "ready" : ""}`}>{props.buildState}</span>
    </header>
    <section className="template-strip">
      <span>SOURCE FRAME PARTITION</span>
      <strong>{props.artifact ? `${props.artifact.regions.length} accepted regions` : "One canonical source frame"}</strong>
      <label>Target regions<input aria-label="Target regions" type="number" min={1} max={256} value={props.targetRegionCount} onChange={(event) => props.setTargetRegionCount(Number(event.target.value))} disabled={!props.project?.document} /></label>
      <button onClick={() => props.regenerateSourceFrame(props.targetRegionCount)} disabled={!props.project?.document || props.activity !== "idle"}>Regenerate / Accept</button>
      <select value={props.project?.document?.renderSettings.outputSize.width ?? 2048} onChange={(event) => void props.setResolution(Number(event.target.value))} disabled={!props.project?.document}>
        <option value={1024}>1024</option>
        <option value={2048}>2048</option>
        <option value={4096}>4096</option>
      </select>
      <button className="primary" onClick={props.build} disabled={!props.project?.document || props.activity !== "idle"}>
        Preview through Stage 14
      </button>
    </section>
    <section
      ref={viewport.containerRef}
      className="sheet-canvas"
      onWheel={viewport.wheel}
      onPointerDown={(event) => {
        viewport.beginPan(event);
        if (event.button === 0 && event.target === event.currentTarget) props.setSelectedRegionId(null);
      }}
      onPointerMove={viewport.movePan}
      onPointerUp={viewport.endPan}
      onPointerCancel={viewport.endPan}
    >
      {!sheet || !imageUrl ? props.preparedPatchPreview ? <div
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
        className="sheet"
        style={{ width: sheet.width, height: sheet.height, transform: `translate(${viewport.view.x}px, ${viewport.view.y}px) scale(${viewport.view.scale})` }}
        onPointerDown={(event) => {
          if (event.button === 0 && !(event.target as Element).closest(".region")) props.setSelectedRegionId(null);
        }}
      >
        <img src={imageUrl} alt={`${props.mapView} trim sheet preview`} onLoad={(event) => props.onPreviewPaint({ width: event.currentTarget.naturalWidth, height: event.currentTarget.naturalHeight })} />
        {sheetMatchesDocument ? <div
          className="sheet-grid"
          style={{
            backgroundSize: "6.25% 6.25%",
          }}
        /> : null}
        <div className="overlays">{sheet.regions.map((region) => <button
          key={region.regionId}
          data-region-id={region.regionId}
          data-selection-surface="atlas"
          aria-pressed={region.regionId === props.selectedRegionId}
          className={`region ${region.regionId === props.selectedRegionId ? "selected" : ""}`}
          style={overlayStyle(region, sheet, viewport.view.scale)}
          onClick={(event) => { event.stopPropagation(); props.setSelectedRegionId(region.regionId); }}
        ><span>{region.displayName}</span></button>)}</div>
      </div>}
      {workpieceSize ? <div className="viewport-tools">
        <button onClick={() => viewport.zoom(0.8)}>-</button>
        <output>{Math.round(viewport.view.scale * 100)}%</output>
        <button onClick={() => viewport.zoom(1.25)}>+</button>
        <button onClick={viewport.fit}>Fit</button>
      </div> : null}
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
  return <aside className="context-inspector">
    <header className="inspector-actions"><button onClick={props.onUndo} disabled={!props.project?.canUndoDocument}>Undo</button><button onClick={props.onRedo} disabled={!props.project?.canRedoDocument}>Redo</button></header>
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
  const [mode, setMode] = useState<"measure" | "motif" | "imported" | "override" | "orientation">("measure");
  const [values, setValues] = useState<Record<string, number>>({
    x1: 0, y1: 0, x2: 100, y2: 0, distanceMm: 250,
    motifWidthPx: 100, motifHeightPx: 100, motifWidthMm: 250, motifHeightMm: 250,
    ppmX: 400, ppmY: 400, confidence: 100, orientationDegrees: 0,
  });
  const [provenance, setProvenance] = useState<"convention" | "prior_estimated">("convention");
  const set = (key: string, value: number) => setValues((current) => ({ ...current, [key]: value }));
  const positive = (...keys: string[]) => keys.every((key) => Number.isFinite(values[key]) && values[key] > 0);
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
  const numeric = (key: string, label: string, min?: number, max?: number) => <label>{label}<input
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

function overlayStyle(region: ResolvedRegion, artifact: Pick<IntermediateAtlasProjection, "width" | "height">, scale = 1): React.CSSProperties {
  const bounds = region.allocationBounds;
  return {
    left: `${bounds.x / artifact.width * 100}%`,
    top: `${bounds.y / artifact.height * 100}%`,
    width: `${bounds.width / artifact.width * 100}%`,
    height: `${bounds.height / artifact.height * 100}%`,
    borderColor: `rgb(${region.idColor[0]} ${region.idColor[1]} ${region.idColor[2]})`,
    "--region-stroke": `${Math.min(3, Math.max(0.75, 1 / scale))}px`,
    "--region-label-size": `${Math.min(16, Math.max(7, 10 / scale))}px`,
  } as React.CSSProperties;
}

createRoot(document.getElementById("root")!).render(<React.StrictMode><App /></React.StrictMode>);
