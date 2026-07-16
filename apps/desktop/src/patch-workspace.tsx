import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  IPC_PROTOCOL_VERSION,
  type CommandFailure,
  type FoundationStatusRequest,
  type LayoutStateSnapshot,
  type AuthoringHistorySnapshot,
  type NormalizedPoint,
  type PatchCommand,
  type PatchGeometry,
  type PatchPreviewSnapshot,
  type PatchProperties,
  type PatchSnapshot,
  type PatchStateSnapshot,
  type ProjectSnapshot,
  type RectificationSettings,
  type RegionSourceLayer,
  type RegionFill,
  type SourceChannel,
  type SourceSnapshot,
  type TemplateSourceTransform,
} from "@hot-trimmer/ipc-contracts";
import {
  canonicalizeFourPoints,
  exceedsDragThreshold,
  geometryBounds,
  moveCorner,
  normalizedFromRect,
  rectangleGeometry,
  rotateGeometry,
  scaleGeometryFromCorner,
  translateGeometry,
  validatePatchGeometry,
  zoomViewAtPoint,
} from "./patch-authoring";
import { LiveRectifiedCanvas } from "./live-rectified-canvas";
import { defaultRegionSourceLayer, defaultTemplateSourceTransform, sourceFootprintsForRegion, sourceLayerGeometry, sourceLayerWithGeometry } from "./layout-authoring";
import { LayoutWorkspace, type WorkbenchRegionSelection } from "./layout-workspace";
import { SerialTaskQueue } from "./serial-task-queue";

type PatchTool = "select" | "four_point" | "rectangle" | "polygon";
type WorkspaceSelection =
  | { kind: "none" }
  | { kind: "patch"; patchId: string }
  | { kind: "region"; selection: WorkbenchRegionSelection };

interface PatchWorkspaceProps {
  hidden: boolean;
  project: ProjectSnapshot;
  onPatchState: (state: PatchStateSnapshot) => void;
  onLayoutState: (state: LayoutStateSnapshot) => void;
  onFailure: (failure: CommandFailure | null) => void;
  onAddSource: () => void;
  onOpenSources: (sourceSetId: string) => void;
  onOpenSourceChannel: (channel: SourceChannel, sourceSetId: string) => void;
}

interface ViewTransform { x: number; y: number; scale: number }
interface ImageRect { left: number; top: number; width: number; height: number }
interface DraftState {
  mode: "four_point" | "rectangle" | "polygon";
  points: NormalizedPoint[];
  geometry?: PatchGeometry;
}
interface ContextMenuState { patchId: string; x: number; y: number }
interface MapContextMenuState { channel: SourceChannel; x: number; y: number }
interface PatchReorderGesture {
  pointerId: number;
  patchId: string;
  start: { x: number; y: number };
  offset: { x: number; y: number };
  width: number;
  moved: boolean;
}
interface PatchDragGhost {
  patchId: string;
  name: string;
  enabled: boolean;
  left: number;
  top: number;
  width: number;
}
interface PaneResize {
  kind: "sources" | "workbench";
  pointerId: number;
  startX: number;
  startValue: number;
  containerWidth: number;
}
interface PatchManipulation {
  kind: "move" | "corner" | "scale" | "rotate";
  pointerId: number;
  patchId: string;
  geometry: PatchGeometry;
  current: PatchGeometry;
  start?: NormalizedPoint;
  corner?: number;
  handle?: 0 | 1 | 2 | 3;
  center?: NormalizedPoint;
  startAngle?: number;
  startClient?: { x: number; y: number };
  moved: boolean;
  group: number;
}
type Manipulation =
  | { kind: "pan"; pointerId: number; x: number; y: number; origin: ViewTransform }
  | { kind: "rectangle"; pointerId: number; start: NormalizedPoint; current?: PatchGeometry }
  | PatchManipulation;

const request = { protocolVersion: IPC_PROTOCOL_VERSION } satisfies FoundationStatusRequest;
const cornerLabels = ["TL", "TR", "BR", "BL"] as const;
const behaviorOptions: Array<{ value: PatchProperties["repeatMode"]; label: string; title: string }> = [
  { value: "unique", label: "Single", title: "Place this detail once in a layout region" },
  { value: "repeat_x", label: "Horizontal", title: "Loop continuously from left to right" },
  { value: "repeat_y", label: "Vertical", title: "Loop continuously from top to bottom" },
  { value: "tile_xy", label: "Tile", title: "Repeat across both axes" },
  { value: "stretch", label: "Stretch", title: "Stretch once to fill its layout region" },
];

const channelAppearance: Record<SourceChannel, { label: string; short: string; tone: string }> = {
  base_color: { label: "Base Color", short: "BC", tone: "color" },
  normal: { label: "Normal", short: "N", tone: "normal" },
  height: { label: "Height", short: "H", tone: "height" },
  roughness: { label: "Roughness", short: "R", tone: "roughness" },
  metallic: { label: "Metallic", short: "M", tone: "metallic" },
  ambient_occlusion: { label: "Ambient Occlusion", short: "AO", tone: "ao" },
  specular: { label: "Specular", short: "S", tone: "specular" },
  opacity: { label: "Opacity", short: "O", tone: "opacity" },
  edge_mask: { label: "Edge Mask", short: "E", tone: "edge" },
  material_id: { label: "Material ID", short: "ID", tone: "id" },
};
const channelOrder = Object.keys(channelAppearance) as SourceChannel[];

function BehaviorIcon({ kind }: { kind: PatchProperties["repeatMode"] }): React.JSX.Element {
  const common = { viewBox: "0 0 24 24", "aria-hidden": true } as const;
  if (kind === "repeat_x") return <svg {...common}><path d="M4 12h16M4 12l3-3M4 12l3 3M20 12l-3-3M20 12l-3 3" /><rect x="9" y="7" width="6" height="10" rx="1" /></svg>;
  if (kind === "repeat_y") return <svg {...common}><path d="M12 4v16M12 4L9 7M12 4l3 3M12 20l-3-3M12 20l3-3" /><rect x="7" y="9" width="10" height="6" rx="1" /></svg>;
  if (kind === "tile_xy") return <svg {...common}><rect x="4" y="4" width="7" height="7" /><rect x="13" y="4" width="7" height="7" /><rect x="4" y="13" width="7" height="7" /><rect x="13" y="13" width="7" height="7" /></svg>;
  if (kind === "stretch") return <svg {...common}><rect x="8" y="6" width="8" height="12" rx="1" /><path d="M8 12H3m0 0 3-3m-3 3 3 3M16 12h5m0 0-3-3m3 3-3 3" /></svg>;
  return <svg {...common}><rect x="5" y="5" width="14" height="14" rx="2" /><circle cx="12" cy="12" r="2" /></svg>;
}

function failureFrom(reason: unknown, fallback: string): CommandFailure {
  if (typeof reason === "object" && reason !== null) {
    const candidate = reason as Partial<CommandFailure>;
    if (typeof candidate.message === "string" && typeof candidate.recovery === "string") {
      return {
        code: typeof candidate.code === "string" ? candidate.code : "patch_command_failed",
        message: candidate.message,
        recovery: candidate.recovery,
        detail: candidate.detail,
      };
    }
  }
  return {
    code: "patch_command_failed",
    message: fallback,
    recovery: "Review the patch and retry the edit.",
    detail: reason instanceof Error ? reason.message : String(reason),
  };
}

function defaultPatch(source: SourceSnapshot, geometry: PatchGeometry, index: number): PatchSnapshot {
  return {
    id: crypto.randomUUID(),
    sourceId: source.id,
    name: `Patch ${index + 1}`,
    enabled: true,
    geometry,
    properties: {
      repeatMode: "unique",
      trimCap: false,
      paddingPx: 4,
      bleedPx: 8,
      mapParticipation: "all",
    },
    rectification: { scale: 1 },
  };
}

function sourcePreviewUrl(source: SourceSnapshot | undefined): string | undefined {
  return source?.thumbnailMipmaps.find((mipmap) => mipmap.maxEdge === 1280)?.dataUrl
    ?? source?.thumbnailDataUrl;
}
interface SourceCropBounds { x: number; y: number; width: number; height: number }
interface SourceCropDrag {
  pointerId: number;
  kind: "move" | "corner" | "rotate";
  corner?: 0 | 1 | 2 | 3;
  start: NormalizedPoint;
  original: PatchGeometry;
  current: PatchGeometry;
  sourceLayer?: RegionSourceLayer;
  regionId?: string;
  center?: NormalizedPoint;
  startAngle?: number;
  moved: boolean;
  group: number;
}

export function PatchWorkspace({
  hidden,
  project,
  onPatchState,
  onLayoutState,
  onFailure,
  onAddSource,
  onOpenSources,
  onOpenSourceChannel,
}: PatchWorkspaceProps): React.JSX.Element {
  const [tool, setTool] = useState<PatchTool>("select");
  const [selection, setSelection] = useState<WorkspaceSelection>({ kind: "none" });
  const [editingPatchId, setEditingPatchId] = useState<string | null>(null);
  const [view, setView] = useState<ViewTransform>({ x: 0, y: 0, scale: 1 });
  const [draft, setDraft] = useState<DraftState | null>(null);
  const [liveGeometry, setLiveGeometry] = useState<PatchGeometry | null>(null);
  const [geometryMessage, setGeometryMessage] = useState<string | null>(null);
  const [preview, setPreview] = useState<PatchPreviewSnapshot | null>(null);
  const [previewBusy, setPreviewBusy] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [imageRect, setImageRect] = useState<ImageRect | null>(null);
  const [viewportSize, setViewportSize] = useState({ width: 1, height: 1 });
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
  const [mapContextMenu, setMapContextMenu] = useState<MapContextMenuState | null>(null);
  const [renamingPatchId, setRenamingPatchId] = useState<string | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
  const [draggedPatchId, setDraggedPatchId] = useState<string | null>(null);
  const [dropTargetPatchId, setDropTargetPatchId] = useState<string | null>(null);
  const [patchDragGhost, setPatchDragGhost] = useState<PatchDragGhost | null>(null);
  const [precisionPoint, setPrecisionPoint] = useState<NormalizedPoint | null>(null);
  const [loupeZoom, setLoupeZoom] = useState<2 | 3 | 4>(3);
  const [sourceRailWidth, setSourceRailWidth] = useState(208);
  const [workbenchShare, setWorkbenchShare] = useState(55);
  const [sourceTransform, setSourceTransform] = useState<TemplateSourceTransform>(project.layout?.template?.sourceFraming ?? defaultTemplateSourceTransform);
  const [editingSourceRegion, setEditingSourceRegion] = useState(false);
  const [liveSourceGeometry, setLiveSourceGeometry] = useState<PatchGeometry | null>(null);
  const [workingSourceId, setWorkingSourceId] = useState<string | null>(
    project.sources.find((candidate) => candidate.channel === "base_color")?.id ?? project.sources[0]?.id ?? null,
  );
  function setSelectedPatchId(patchId: string | null): void {
    if (patchId) setEditingSourceRegion(false);
    setSelection(patchId ? { kind: "patch", patchId } : { kind: "none" });
  }
  const imageRef = useRef<HTMLImageElement | null>(null);
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const manipulation = useRef<Manipulation | null>(null);
  const commandQueue = useRef(new SerialTaskQueue());
  const previewSequence = useRef(0);
  const latestPatches = useRef(project.patches);
  const latestGeometryCommit = useRef(0);
  const latestSourceGeometryCommit = useRef(0);
  const nextManipulationGroup = useRef(0);
  const sourceLayerQueue = useRef(new SerialTaskQueue());
  const previousSourceIds = useRef(new Set(project.sources.map((candidate) => candidate.id)));
  const lastCursor = useRef<{ x: number; y: number } | null>(null);
  const authoringSplitRef = useRef<HTMLDivElement | null>(null);
  const sourceBodyRef = useRef<HTMLDivElement | null>(null);
  const paneResize = useRef<PaneResize | null>(null);
  const sourceCropDrag = useRef<SourceCropDrag | null>(null);
  const patchReorder = useRef<PatchReorderGesture | null>(null);
  const patchReorderCleanup = useRef<(() => void) | null>(null);
  const source = project.sources.find((candidate) => candidate.id === workingSourceId)
    ?? project.sources.find((candidate) => candidate.channel === "base_color")
    ?? project.sources[0];
  const selectedSource = source;
  const selectedSourceSetId = selectedSource?.sourceSetId ?? project.sourceSets[0]?.id ?? null;
  const selectedSetSources = selectedSourceSetId
    ? project.sources.filter((candidate) => candidate.sourceSetId === selectedSourceSetId)
    : [];
  const selectedSetSourceIds = new Set(selectedSetSources.map((candidate) => candidate.id));
  const sourcePatches = project.patches.filter((patch) => selectedSetSourceIds.has(patch.sourceId));
  const workbenchRegion = selection.kind === "region" ? selection.selection : null;
  const selectedPatchId = selection.kind === "patch" ? selection.patchId : editingPatchId;
  const sourceSets = project.sourceSets.map((sourceSet) => {
    const inputs = project.sources.filter((candidate) => candidate.sourceSetId === sourceSet.id);
    return { ...sourceSet, inputs, base: inputs.find((candidate) => candidate.channel === "base_color") ?? inputs[0] };
  });
  const imageUrl = sourcePreviewUrl(selectedSource);
  const workbenchRegionFill = workbenchRegion?.region.fill;
  const assignedRegionPatch = workbenchRegionFill?.type === "rectified_patch"
    ? project.patches.find((patch) => patch.id === workbenchRegionFill.patchId) ?? null
    : null;
  const selectedPatch = sourcePatches.find((patch) => patch.id === selectedPatchId) ?? assignedRegionPatch;
  const displayGeometry = liveGeometry ?? selectedPatch?.geometry;
  const sourceCropBounds = sourceTransform.cropBounds ?? { x: 0, y: 0, width: 1, height: 1 };
  const sourceCropGeometry = rectangleGeometry(
    { x: sourceCropBounds.x, y: sourceCropBounds.y },
    { x: sourceCropBounds.x + sourceCropBounds.width, y: sourceCropBounds.y + sourceCropBounds.height },
  );
  const persistedRegionSourceLayer = workbenchRegion ? project.layout?.sourceLayers[workbenchRegion.region.id] : undefined;
  const regionSourceLayer = workbenchRegion ? persistedRegionSourceLayer ?? defaultRegionSourceLayer() : null;
  const inferredRegionSourceGeometry = workbenchRegion && selectedSource && !persistedRegionSourceLayer
    ? (() => {
      const footprint = sourceFootprintsForRegion(workbenchRegion.region.bounds, workbenchRegion.output, sourceTransform, selectedSource)[0];
      return footprint ? rectangleGeometry(
        { x: footprint.bounds.x, y: footprint.bounds.y },
        { x: footprint.bounds.x + footprint.bounds.width, y: footprint.bounds.y + footprint.bounds.height },
      ) : null;
    })()
    : null;
  const regionSourceGeometry = regionSourceLayer
    ? liveSourceGeometry ?? inferredRegionSourceGeometry ?? sourceLayerGeometry(regionSourceLayer)
    : null;

  function selectSourceSet(sourceSetId: string): void {
    const inputs = project.sources.filter((candidate) => candidate.sourceSetId === sourceSetId);
    const nextSource = inputs.find((candidate) => candidate.channel === "base_color") ?? inputs[0];
    if (!nextSource) return;
    const sourceIds = new Set(inputs.map((candidate) => candidate.id));
    const nextPatch = project.patches.find((patch) => sourceIds.has(patch.sourceId));
    setWorkingSourceId(nextSource.id);
    if (!editingPatchId) setSelection({ kind: "none" });
    setEditingPatchId(null);
    setLiveGeometry(null);
    setDraft(null);
    manipulation.current = null;
    setPreview(null);
    setPrecisionPoint(null);
    setTool("select");
    setView({ x: 0, y: 0, scale: 1 });
  }

  const acceptRegionSelection = useCallback((selection: WorkbenchRegionSelection | null): void => {
    if (selection) setEditingSourceRegion(false);
    setSelection(selection ? { kind: "region", selection } : { kind: "none" });
    if (!selection) {
      setEditingPatchId(null);
      return;
    }
    const fill = selection.region.fill;
    const patch = fill.type === "rectified_patch"
      ? project.patches.find((candidate) => candidate.id === fill.patchId)
      : undefined;
    const sourceSetId = fill.type === "whole_source_set" || fill.type === "rectified_patch"
      ? fill.sourceSetId
      : null;
    const nextSource = patch
      ? project.sources.find((candidate) => candidate.id === patch.sourceId)
      : project.sources.find((candidate) => candidate.sourceSetId === sourceSetId && candidate.channel === "base_color")
        ?? project.sources.find((candidate) => candidate.sourceSetId === sourceSetId);
    if (nextSource) setWorkingSourceId(nextSource.id);
    setEditingPatchId(null);
    setLiveGeometry(null);
    setDraft(null);
    setTool("select");
  }, [project.patches, project.sources]);

  function clearSelectionFromEmpty(event: React.PointerEvent<HTMLElement>): void {
    if (event.button !== 0 || editingPatchId || tool !== "select") return;
    const target = event.target as Element;
    if (target.closest("button, input, select, textarea, label, summary, [role='button'], [role='menuitem'], [data-patch-id], [data-preserve-selection], .layout-region, .modal")) return;
    setSelection({ kind: "none" });
    setContextMenu(null);
    setMapContextMenu(null);
  }

  useEffect(() => {
    setSourceTransform(project.layout?.template?.sourceFraming ?? defaultTemplateSourceTransform);
    setSelection({ kind: "none" });
  }, [project.id]);

  useEffect(() => {
    latestPatches.current = project.patches;
    // A null selection is intentional while starting a capture or after clicking
    // empty canvas space. Only reconcile an id that has actually gone stale.
    if (selectedPatchId === null) return;

    if (selectedPatchId && sourcePatches.some((patch) => patch.id === selectedPatchId)) return;
    setSelectedPatchId(sourcePatches[0]?.id ?? null);
    setEditingPatchId(null);
    setLiveGeometry(null);
    setDraft(null);
    manipulation.current = null;
    setPreview(null);
  }, [project.patches, selectedPatchId, selectedSourceSetId]);

  useEffect(() => {
    const added = project.sources.find((candidate) => !previousSourceIds.current.has(candidate.id));
    if (added) {
      setWorkingSourceId(added?.id ?? null);
      const sourceIds = new Set(project.sources.filter((candidate) => candidate.sourceSetId === added.sourceSetId).map((candidate) => candidate.id));
      setSelectedPatchId(project.patches.find((patch) => sourceIds.has(patch.sourceId))?.id ?? null);
      setEditingPatchId(null);
      setLiveGeometry(null);
    } else if (workingSourceId && !project.sources.some((candidate) => candidate.id === workingSourceId)) {
      setWorkingSourceId(project.sources[0]?.id ?? null);
    }
    previousSourceIds.current = new Set(project.sources.map((candidate) => candidate.id));
  }, [project.sources, workingSourceId]);

  useEffect(() => {
    if (!contextMenu && !mapContextMenu) return;
    const close = (event: PointerEvent): void => {
      if (event.button !== 0 || (event.target as Element | null)?.closest(".patch-context-menu")) return;
      setContextMenu(null);
      setMapContextMenu(null);
    };
    const closeOnBlur = (): void => { setContextMenu(null); setMapContextMenu(null); };
    window.addEventListener("pointerdown", close, true);
    window.addEventListener("blur", closeOnBlur);
    return () => {
      window.removeEventListener("pointerdown", close, true);
      window.removeEventListener("blur", closeOnBlur);
    };
  }, [contextMenu, mapContextMenu]);

  useEffect(() => {
    if (!selectedPatch || !selectedSource || hidden) return;
    const sequence = ++previewSequence.current;
    const geometry = liveGeometry ?? selectedPatch.geometry;
    setPreview(null);
    const timer = window.setTimeout(() => {
      setPreviewBusy(true);
      void invoke<PatchPreviewSnapshot>("generate_draft_patch_preview", {
        request: {
          protocolVersion: IPC_PROTOCOL_VERSION,
          previewId: selectedPatch.id,
          sourceId: selectedSource.id,
          geometry,
          rectification: selectedPatch.rectification,
          maxEdge: 768,
        },
      }).then((result) => {
        if (sequence === previewSequence.current && result.patchId === selectedPatch.id) setPreview(result);
      }).catch((reason) => {
        const failure = failureFrom(reason, "Patch preview refinement failed.");
        if (sequence === previewSequence.current && failure.code !== "operation_cancelled") onFailure(failure);
      }).finally(() => {
        if (sequence === previewSequence.current) setPreviewBusy(false);
      });
    }, 80);
    return () => {
      if (sequence === previewSequence.current) previewSequence.current += 1;
      window.clearTimeout(timer);
      void invoke<void>("cancel_patch_preview", { request }).catch(() => undefined);
    };
  }, [hidden, liveGeometry, onFailure, selectedPatch, selectedSource]);

  function refreshImageRect(): void {
    const image = imageRef.current;
    const viewport = viewportRef.current;
    if (!image || !viewport) return;
    const imageBounds = image.getBoundingClientRect();
    const viewportBounds = viewport.getBoundingClientRect();
    setImageRect({
      left: imageBounds.left - viewportBounds.left,
      top: imageBounds.top - viewportBounds.top,
      width: imageBounds.width,
      height: imageBounds.height,
    });
    setViewportSize({ width: viewportBounds.width, height: viewportBounds.height });
  }

  useLayoutEffect(() => {
    refreshImageRect();
    const resize = new ResizeObserver(refreshImageRect);
    if (viewportRef.current) resize.observe(viewportRef.current);
    return () => resize.disconnect();
  }, [hidden, imageUrl, view]);

  function normalizedAt(clientX: number, clientY: number): NormalizedPoint | null {
    const image = imageRef.current;
    return image ? normalizedFromRect(clientX, clientY, image.getBoundingClientRect()) : null;
  }

  function zoomAtCursor(factor: number, cursor = lastCursor.current): void {
    const rect = imageRef.current?.getBoundingClientRect();
    if (!rect) return;
    const anchor = cursor ?? { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
    setView((current) => {
      const nextScale = Math.min(8, Math.max(0.1, current.scale * factor));
      return zoomViewAtPoint(current, nextScale, anchor, rect);
    });
  }

  function beginPaneResize(kind: PaneResize["kind"], event: React.PointerEvent<HTMLDivElement>): void {
    const container = kind === "sources" ? sourceBodyRef.current : authoringSplitRef.current;
    if (!container) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    paneResize.current = {
      kind,
      pointerId: event.pointerId,
      startX: event.clientX,
      startValue: kind === "sources" ? sourceRailWidth : workbenchShare,
      containerWidth: Math.max(1, container.clientWidth),
    };
  }

  function movePaneResize(event: React.PointerEvent<HTMLDivElement>): void {
    const active = paneResize.current;
    if (!active || active.pointerId !== event.pointerId) return;
    const delta = event.clientX - active.startX;
    if (active.kind === "sources") {
      setSourceRailWidth(Math.min(300, Math.max(144, active.startValue + delta)));
    } else {
      setWorkbenchShare(Math.min(68, Math.max(32, active.startValue + (delta / active.containerWidth) * 100)));
    }
  }

  function endPaneResize(event: React.PointerEvent<HTMLDivElement>): void {
    if (paneResize.current?.pointerId === event.pointerId) paneResize.current = null;
  }

  function normalizedManipulationAt(clientX: number, clientY: number, unbounded = false): NormalizedPoint | null {
    const image = imageRef.current;
    if (!image) return null;
    const rect = image.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return null;
    const point = { x: (clientX - rect.left) / rect.width, y: (clientY - rect.top) / rect.height };
    return unbounded ? point : {
      x: Math.min(1, Math.max(0, point.x)),
      y: Math.min(1, Math.max(0, point.y)),
    };
  }

  function overlayPoint(point: NormalizedPoint): { x: number; y: number } {
    if (!imageRect) return { x: 0, y: 0 };
    return { x: imageRect.left + point.x * imageRect.width, y: imageRect.top + point.y * imageRect.height };
  }

  function captureManipulationPointer(event: React.PointerEvent<SVGElement>): void {
    event.preventDefault();
    event.stopPropagation();
    viewportRef.current?.setPointerCapture(event.pointerId);
  }

  function enqueuePatchMutation<T>(operation: () => Promise<T>): Promise<T> {
    return commandQueue.current.run(operation);
  }

  function applyFactory(
    commandFactory: () => PatchCommand | null,
    label: string,
    coalescingGroup?: number,
  ): Promise<PatchStateSnapshot | null> {
    return enqueuePatchMutation(async () => {
      const command = commandFactory();
      if (!command) return null;
      setBusy(label);
      onFailure(null);
      try {
        const state = await invoke<PatchStateSnapshot>("apply_patch_command", {
          request: { protocolVersion: IPC_PROTOCOL_VERSION, command, coalescingGroup },
        });
        latestPatches.current = state.patches;
        onPatchState(state);
        return state;
      } catch (reason) {
        onFailure(failureFrom(reason, `${label} failed.`));
        return null;
      } finally {
        setBusy(null);
      }
    });
  }

  function apply(command: PatchCommand, label: string, coalescingGroup?: number): Promise<PatchStateSnapshot | null> {
    return applyFactory(() => command, label, coalescingGroup);
  }

  function history(redo: boolean): Promise<void> {
    return enqueuePatchMutation(async () => {
      setBusy(redo ? "Redo" : "Undo");
      try {
        const state = await invoke<AuthoringHistorySnapshot>(redo ? "redo_patch_command" : "undo_patch_command", { request });
        latestPatches.current = state.patches;
        onPatchState(state);
        setLiveGeometry(null);
        setDraft(null);
      } catch (reason) {
        onFailure(failureFrom(reason, `${redo ? "Redo" : "Undo"} failed.`));
      } finally {
        setBusy(null);
      }
    });
  }

  function beginNew(mode: DraftState["mode"] = "four_point", initialPoint?: NormalizedPoint): void {
    if (!source) {
      onAddSource();
      return;
    }
    setTool(mode);
    setSelectedPatchId(null);
    setEditingPatchId(null);
    setLiveGeometry(null);
    setGeometryMessage(null);
    setDraft({ mode, points: initialPoint ? [initialPoint] : [] });
  }

  async function createPatch(geometry: PatchGeometry): Promise<void> {
    if (!source) return;
    const error = validatePatchGeometry(geometry);
    if (error) {
      setGeometryMessage(error);
      return;
    }
    const patch = defaultPatch(source, geometry, sourcePatches.length);
    const state = await apply({ type: "create", patch }, "Create patch");
    if (!state) return;
    setSelectedPatchId(patch.id);
    setEditingPatchId(null);
    setDraft(null);
    setTool("select");
    setGeometryMessage(null);
  }

  function createManipulationGroup(): number {
    nextManipulationGroup.current += 1;
    return nextManipulationGroup.current;
  }

  async function editSelectedRegionSource(): Promise<void> {
    if (!workbenchRegion) return;
    setEditingPatchId(null);
    setLiveGeometry(null);
    setLiveSourceGeometry(null);
    setTool("select");
    setEditingSourceRegion(true);
  }

  async function assignSelectedRegionFill(fill: RegionFill): Promise<void> {
    if (!workbenchRegion) return;
    try {
      const layoutState = await invoke<LayoutStateSnapshot>("apply_layout_command", {
        request: {
          protocolVersion: IPC_PROTOCOL_VERSION,
          command: { type: "set_fill", regionId: workbenchRegion.region.id, fill },
          coalescingGroup: Date.now(),
        },
      });
      onLayoutState(layoutState);
      onFailure(null);
    } catch (reason) {
      onFailure(failureFrom(reason, "Assigning the region source failed."));
    }
  }

  function setSourceCropField(field: keyof SourceCropBounds, percent: number): void {
    if (!Number.isFinite(percent)) return;
    const value = percent / 100;
    setSourceTransform((current) => {
      const bounds = { ...(current.cropBounds ?? { x: 0, y: 0, width: 1, height: 1 }) };
      if (field === "x") bounds.x = Math.max(0, Math.min(1 - bounds.width, value));
      else if (field === "y") bounds.y = Math.max(0, Math.min(1 - bounds.height, value));
      else if (field === "width") bounds.width = Math.max(0.01, Math.min(1 - bounds.x, value));
      else bounds.height = Math.max(0.01, Math.min(1 - bounds.y, value));
      return { ...current, cropBounds: bounds };
    });
  }

  async function fitPolygonPoints(points: NormalizedPoint[]): Promise<void> {
    if (points.length < 4) return;
    try {
      const geometry = await invoke<PatchGeometry>("fit_patch_polygon", {
        request: { protocolVersion: IPC_PROTOCOL_VERSION, points, retainMask: true },
      });
      await createPatch(geometry);
    } catch (reason) {
      onFailure(failureFrom(reason, "Polygon fit failed."));
    }
  }

  async function fitPolygon(): Promise<void> {
    if (!draft || draft.mode !== "polygon") return;
    await fitPolygonPoints(draft.points);
  }

  function cancelManipulation(): boolean {
    if (sourceCropDrag.current) {
      sourceCropDrag.current = null;
      setLiveSourceGeometry(null);
      return true;
    }
    if (!manipulation.current) return false;
    const active = manipulation.current;
    manipulation.current = null;
    if (viewportRef.current?.hasPointerCapture(active.pointerId)) viewportRef.current.releasePointerCapture(active.pointerId);
    setLiveGeometry(null);
    setGeometryMessage(null);
    if (active.kind === "rectangle") {
      setDraft(null);
      setTool("select");
    } else if (active.kind === "pan") {
      setView(active.origin);
    }
    return true;
  }

  function beginSourceCropDrag(event: React.PointerEvent<SVGElement>, kind: SourceCropDrag["kind"], corner?: 0 | 1 | 2 | 3): void {
    if (!editingSourceRegion || event.button !== 0) return;
    const point = normalizedManipulationAt(event.clientX, event.clientY, kind === "rotate");
    if (!point) return;
    captureManipulationPointer(event);
    const geometry = regionSourceGeometry ?? sourceCropGeometry;
    const bounds = geometryBounds(geometry);
    const center = { x: (bounds.left + bounds.right) / 2, y: (bounds.top + bounds.bottom) / 2 };
    const startAngle = selectedSource ? Math.atan2((point.y - center.y) * selectedSource.height, (point.x - center.x) * selectedSource.width) : undefined;
    sourceCropDrag.current = {
      pointerId: event.pointerId, kind, corner, start: point, original: geometry, current: geometry,
      sourceLayer: regionSourceLayer ?? undefined, regionId: workbenchRegion?.region.id, center, startAngle,
      moved: false, group: createManipulationGroup(),
    };
    latestSourceGeometryCommit.current = sourceCropDrag.current.group;
  }

  function updateSourceCropDrag(point: NormalizedPoint): void {
    const active = sourceCropDrag.current;
    if (!active) return;
    if (active.kind === "move") {
      active.current = translateGeometry(active.original, point.x - active.start.x, point.y - active.start.y);
    } else if (active.kind === "rotate" && active.center && active.startAngle !== undefined && selectedSource) {
      const angle = Math.atan2((point.y - active.center.y) * selectedSource.height, (point.x - active.center.x) * selectedSource.width);
      active.current = rotateGeometry(active.original, active.center, angle - active.startAngle, selectedSource.width / selectedSource.height);
    } else if (active.corner !== undefined) {
      active.current = moveCorner(active.original, active.corner, point);
    }
    active.moved = true;
    setLiveSourceGeometry(active.current);
  }

  useEffect(() => {
    if (hidden) return;
    function keyboard(event: KeyboardEvent): void {
      const target = event.target as HTMLElement | null;
      if (target?.matches("input, select, textarea")) return;
      if (event.ctrlKey && event.key.toLowerCase() === "z") {
        event.preventDefault(); void history(event.shiftKey); return;
      }
      if (event.ctrlKey && event.key.toLowerCase() === "y") {
        event.preventDefault(); void history(true); return;
      }
      if (event.key.toLowerCase() === "n") { event.preventDefault(); beginNew(); }
      else if (event.key === "Escape") {
        event.preventDefault();
        if (!cancelManipulation()) {
          setDraft(null);
          setTool("select");
          setEditingPatchId(null);
          setGeometryMessage(null);
        }
      } else if (event.key === "Enter" && draft?.mode === "polygon") {
        event.preventDefault(); void fitPolygon();
      } else if (event.key === "0") {
        event.preventDefault(); setView({ x: 0, y: 0, scale: 1 });
      } else if (event.key === "+" || event.key === "=") {
        zoomAtCursor(1.25);
      } else if (event.key === "-") {
        zoomAtCursor(0.8);
      }
    }
    window.addEventListener("keydown", keyboard);
    return () => window.removeEventListener("keydown", keyboard);
  });

  function pointerDown(event: React.PointerEvent<HTMLDivElement>): void {
    if (!selectedSource) return;
    lastCursor.current = { x: event.clientX, y: event.clientY };
    if (event.button === 1) {
      event.preventDefault();
      event.currentTarget.setPointerCapture(event.pointerId);
      manipulation.current = { kind: "pan", pointerId: event.pointerId, x: event.clientX, y: event.clientY, origin: view };
      return;
    }
    if (event.button !== 0) return;
    if (tool === "select") {
      setSelectedPatchId(null);
      setEditingPatchId(null);
      setPrecisionPoint(null);
      setGeometryMessage(null);
    }
    const point = normalizedAt(event.clientX, event.clientY);
    if (!point) return;
    if (tool !== "select") setPrecisionPoint(point);
    if (tool === "four_point" && draft?.mode === "four_point") {
      const points = [...draft.points, point];
      if (points.length === 4) {
        const geometry = canonicalizeFourPoints(points);
        if (geometry) void createPatch(geometry);
        else {
          setDraft({ ...draft, points });
          setGeometryMessage("Those points do not enclose a valid patch. Escape and try four boundary corners again.");
        }
      } else {
        setDraft({ ...draft, points });
      }
    } else if (tool === "polygon" && draft?.mode === "polygon") {
      if (draft.points.length < 8) {
        const points = [...draft.points, point];
        if (points.length === 8) void fitPolygonPoints(points);
        else setDraft({ ...draft, points });
      }
    } else if (tool === "rectangle") {
      event.currentTarget.setPointerCapture(event.pointerId);
      manipulation.current = { kind: "rectangle", pointerId: event.pointerId, start: point };
      setDraft({ mode: "rectangle", points: [point] });
    }
  }

  function pointerMove(event: React.PointerEvent<HTMLDivElement>): void {
    lastCursor.current = { x: event.clientX, y: event.clientY };
    const active = manipulation.current;
    const activeSourceCrop = sourceCropDrag.current;
    const hoverPoint = activeSourceCrop
      ? normalizedManipulationAt(event.clientX, event.clientY, activeSourceCrop.kind === "rotate")
      : normalizedAt(event.clientX, event.clientY);
    if (activeSourceCrop?.pointerId === event.pointerId && hoverPoint) {
      updateSourceCropDrag(hoverPoint);
      return;
    }
    if ((draft || editingPatchId) && hoverPoint) setPrecisionPoint(hoverPoint);
    if (!active || active.pointerId !== event.pointerId) return;
    if (active.kind === "pan") {
      setView({ ...active.origin, x: active.origin.x + event.clientX - active.x, y: active.origin.y + event.clientY - active.y });
      return;
    }
    const point = normalizedManipulationAt(event.clientX, event.clientY, active.kind === "rotate");
    if (!point) return;
    if (active.kind === "rectangle") {
      setPrecisionPoint(point);
      const geometry = rectangleGeometry(active.start, point);
      active.current = geometry;
      setDraft({ mode: "rectangle", points: geometry.corners, geometry });
      setGeometryMessage(validatePatchGeometry(geometry));
      return;
    }
    let geometry = active.current;
    if (active.kind === "corner" && active.corner !== undefined) {
      geometry = moveCorner(active.geometry, active.corner, point);
      setPrecisionPoint(point);
    } else if (active.kind === "move" && active.start) {
      if (!active.moved && active.startClient) {
        if (!exceedsDragThreshold(active.startClient, { x: event.clientX, y: event.clientY })) return;
        if (!event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.setPointerCapture(event.pointerId);
      }
      geometry = translateGeometry(active.geometry, point.x - active.start.x, point.y - active.start.y);
    } else if (active.kind === "scale" && active.handle !== undefined) {
      geometry = scaleGeometryFromCorner(active.geometry, active.handle, point, event.shiftKey);
    } else if (active.kind === "rotate" && active.center && active.startAngle !== undefined && selectedSource) {
      const aspect = selectedSource.width / selectedSource.height;
      const angle = Math.atan2((point.y - active.center.y) * selectedSource.height, (point.x - active.center.x) * selectedSource.width);
      geometry = rotateGeometry(active.geometry, active.center, angle - active.startAngle, aspect);
    }
    active.moved = true;
    active.current = geometry;
    setPreview(null);
    setLiveGeometry(geometry);
    setGeometryMessage(validatePatchGeometry(geometry));
  }

  function pointerUp(event: React.PointerEvent<HTMLDivElement>): void {
    if (sourceCropDrag.current?.pointerId === event.pointerId) {
      const activeCrop = sourceCropDrag.current;
      sourceCropDrag.current = null;
      if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
      if (!activeCrop.moved) {
        setLiveSourceGeometry(null);
        return;
      }
      setLiveSourceGeometry(activeCrop.current);
      if (activeCrop.regionId && activeCrop.sourceLayer) {
        const sourceLayer = sourceLayerWithGeometry(activeCrop.sourceLayer, activeCrop.current);
        void sourceLayerQueue.current.run(async () => invoke<LayoutStateSnapshot>("apply_layout_command", {
          request: {
            protocolVersion: IPC_PROTOCOL_VERSION,
            command: { type: "set_source_layer", regionId: activeCrop.regionId, sourceLayer },
            coalescingGroup: activeCrop.group,
          },
        })).then((state) => {
          if (latestSourceGeometryCommit.current !== activeCrop.group) return;
          onLayoutState(state);
          onFailure(null);
          setLiveSourceGeometry(null);
        }).catch((reason) => {
          if (latestSourceGeometryCommit.current !== activeCrop.group) return;
          setLiveSourceGeometry(null);
          onFailure(failureFrom(reason, "Updating the region source transform failed."));
        });
      } else {
        const bounds = geometryBounds(activeCrop.current);
        setSourceTransform((current) => ({ ...current, cropBounds: { x: bounds.left, y: bounds.top, width: bounds.right - bounds.left, height: bounds.bottom - bounds.top } }));
      }
      return;
    }
    const active = manipulation.current;
    if (!active || active.pointerId !== event.pointerId) return;
    manipulation.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
    if (active.kind === "rectangle") {
      if (active.current && validatePatchGeometry(active.current) === null) void createPatch(active.current);
      else {
        setDraft(null);
        setTool("select");
      }
      return;
    }
    if (active.kind === "pan") return;
    if (!active.moved) {
      setLiveGeometry(null);
      return;
    }
    const error = validatePatchGeometry(active.current);
    if (error) {
      setGeometryMessage(error);
      setLiveGeometry(null);
      return;
    }
    void apply({ type: "replace_geometry", patchId: active.patchId, geometry: active.current }, "Transform patch", active.group)
      .then(() => {
        if (latestGeometryCommit.current === active.group && manipulation.current === null) setLiveGeometry(null);
      });
  }

  function pointerCancel(event: React.PointerEvent<HTMLDivElement>): void {
    if (sourceCropDrag.current?.pointerId === event.pointerId) {
      sourceCropDrag.current = null;
      setLiveSourceGeometry(null);
    }
    if (manipulation.current?.pointerId === event.pointerId) cancelManipulation();
    setPrecisionPoint(null);
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
  }

  function beginMove(event: React.PointerEvent<SVGPolygonElement>, patch: PatchSnapshot, geometry: PatchGeometry): void {
    if (tool !== "select") return;
    if (event.button === 1) return;
    if (event.button !== 0) return;
    const point = normalizedAt(event.clientX, event.clientY);
    if (!point) return;
    captureManipulationPointer(event);
    setSelectedPatchId(patch.id);
    setWorkingSourceId(patch.sourceId);
    if (editingPatchId !== patch.id) setEditingPatchId(null);
    manipulation.current = {
      kind: "move", pointerId: event.pointerId, patchId: patch.id, start: point,
      startClient: { x: event.clientX, y: event.clientY }, moved: false,
      geometry, current: geometry, group: createManipulationGroup(),
    };
    latestGeometryCommit.current = manipulation.current.group;
  }

  function beginCorner(event: React.PointerEvent<SVGCircleElement>, corner: number): void {
    if (!displayGeometry || !selectedPatch || event.button !== 0) return;
    captureManipulationPointer(event);
    setPrecisionPoint(displayGeometry.corners[corner] ?? null);
    manipulation.current = {
      kind: "corner", pointerId: event.pointerId, patchId: selectedPatch.id, corner,
      geometry: displayGeometry, current: displayGeometry, moved: false, group: createManipulationGroup(),
    };
    latestGeometryCommit.current = manipulation.current.group;
  }

  function beginScale(event: React.PointerEvent<SVGRectElement>, handle: 0 | 1 | 2 | 3): void {
    if (!displayGeometry || !selectedPatch || event.button !== 0) return;
    captureManipulationPointer(event);
    manipulation.current = {
      kind: "scale", pointerId: event.pointerId, patchId: selectedPatch.id, handle,
      geometry: displayGeometry, current: displayGeometry, moved: false, group: createManipulationGroup(),
    };
    latestGeometryCommit.current = manipulation.current.group;
  }

  function beginRotate(event: React.PointerEvent<SVGCircleElement>): void {
    if (!displayGeometry || !selectedPatch || !selectedSource || event.button !== 0) return;
    const bounds = geometryBounds(displayGeometry);
    const center = { x: (bounds.left + bounds.right) / 2, y: (bounds.top + bounds.bottom) / 2 };
    const point = normalizedManipulationAt(event.clientX, event.clientY, true);
    if (!point) return;
    captureManipulationPointer(event);
    const startAngle = Math.atan2((point.y - center.y) * selectedSource.height, (point.x - center.x) * selectedSource.width);
    manipulation.current = {
      kind: "rotate", pointerId: event.pointerId, patchId: selectedPatch.id, center, startAngle,
      geometry: displayGeometry, current: displayGeometry, moved: false, group: createManipulationGroup(),
    };
    latestGeometryCommit.current = manipulation.current.group;
  }

  function openContextMenu(event: React.MouseEvent, patch: PatchSnapshot): void {
    event.preventDefault();
    event.stopPropagation();
    setSelectedPatchId(patch.id);
    setWorkingSourceId(patch.sourceId);
    setContextMenu({ patchId: patch.id, x: event.clientX, y: event.clientY });
  }

  async function duplicatePatch(patch: PatchSnapshot): Promise<void> {
    const newPatch = {
      ...patch,
      id: crypto.randomUUID(),
      name: `${patch.name} Copy`,
      geometry: translateGeometry(patch.geometry, 0.03, 0.03),
    };
    const index = project.patches.findIndex((candidate) => candidate.id === patch.id) + 1;
    const state = await apply({ type: "create", patch: newPatch, index }, "Duplicate patch");
    if (state) setSelectedPatchId(newPatch.id);
  }

  async function deletePatch(patch: PatchSnapshot): Promise<void> {
    if (selectedPatchId === patch.id) setSelectedPatchId(null);
    if (editingPatchId === patch.id) setEditingPatchId(null);
    await apply({ type: "delete", patchId: patch.id }, "Delete patch");
  }

  function reorderPatch(patchId: string, targetPatchId: string): void {
    if (patchId === targetPatchId) return;
    const toIndex = project.patches.findIndex((candidate) => candidate.id === targetPatchId);
    if (toIndex >= 0) void apply({ type: "reorder", patchId, toIndex }, "Reorder patches");
  }

  function updatePatchDropTarget(clientX: number, clientY: number): void {
    const row = document.elementFromPoint(clientX, clientY)?.closest<HTMLElement>("[data-patch-id]");
    setDropTargetPatchId(row?.dataset.patchId ?? null);
  }

  function resetPatchReorder(): void {
    patchReorderCleanup.current?.();
    patchReorderCleanup.current = null;
    patchReorder.current = null;
    document.body.classList.remove("patch-reordering");
    setDraggedPatchId(null);
    setDropTargetPatchId(null);
    setPatchDragGhost(null);
  }

  function suppressPointerClick(): void {
    const suppress = (event: MouseEvent): void => {
      event.preventDefault();
      event.stopPropagation();
      event.stopImmediatePropagation();
    };
    document.addEventListener("click", suppress, { capture: true, once: true });
    window.setTimeout(() => document.removeEventListener("click", suppress, true), 100);
  }

  function beginPatchReorder(event: React.PointerEvent<HTMLElement>, patch: PatchSnapshot): void {
    if (tool !== "select" || event.button !== 0) return;
    resetPatchReorder();
    const bounds = event.currentTarget.getBoundingClientRect();
    const gesture: PatchReorderGesture = {
      pointerId: event.pointerId,
      patchId: patch.id,
      start: { x: event.clientX, y: event.clientY },
      offset: { x: event.clientX - bounds.left, y: event.clientY - bounds.top },
      width: bounds.width,
      moved: false,
    };
    patchReorder.current = gesture;

    const move = (pointerEvent: PointerEvent): void => {
      const active = patchReorder.current;
      if (!active || pointerEvent.pointerId !== active.pointerId) return;
      if (!active.moved && !exceedsDragThreshold(active.start, { x: pointerEvent.clientX, y: pointerEvent.clientY })) return;
      pointerEvent.preventDefault();
      if (!active.moved) {
        active.moved = true;
        document.body.classList.add("patch-reordering");
        setDraggedPatchId(active.patchId);
      }
      setPatchDragGhost({
        patchId: active.patchId,
        name: patch.name,
        enabled: patch.enabled,
        left: pointerEvent.clientX - active.offset.x,
        top: pointerEvent.clientY - active.offset.y,
        width: active.width,
      });
      updatePatchDropTarget(pointerEvent.clientX, pointerEvent.clientY);
    };
    const finish = (pointerEvent: PointerEvent): void => {
      const active = patchReorder.current;
      if (!active || pointerEvent.pointerId !== active.pointerId) return;
      const target = document.elementFromPoint(pointerEvent.clientX, pointerEvent.clientY)?.closest<HTMLElement>("[data-patch-id]")?.dataset.patchId ?? null;
      const moved = active.moved;
      const patchId = active.patchId;
      if (moved) {
        pointerEvent.preventDefault();
        suppressPointerClick();
      }
      resetPatchReorder();
      if (moved && target) reorderPatch(patchId, target);
    };
    const cancel = (pointerEvent: PointerEvent): void => {
      if (patchReorder.current?.pointerId === pointerEvent.pointerId) resetPatchReorder();
    };
    const cleanup = (): void => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", finish);
      window.removeEventListener("pointercancel", cancel);
    };
    patchReorderCleanup.current = cleanup;
    window.addEventListener("pointermove", move, { passive: false });
    window.addEventListener("pointerup", finish);
    window.addEventListener("pointercancel", cancel);
  }

  useEffect(() => () => resetPatchReorder(), []);

  function beginRename(patch: PatchSnapshot): void {
    setSelectedPatchId(patch.id);
    setRenamingPatchId(patch.id);
    setRenameDraft(patch.name);
  }

  function commitRename(patch: PatchSnapshot): void {
    const name = renameDraft.trim();
    setRenamingPatchId(null);
    if (name && name !== patch.name) void apply({ type: "rename", patchId: patch.id, name }, "Rename patch");
  }

  function updateProperties(changes: Partial<PatchProperties>): void {
    if (!selectedPatch) return;
    const patchId = selectedPatch.id;
    void applyFactory(() => {
      const current = latestPatches.current.find((patch) => patch.id === patchId);
      if (!current) return null;
      const properties = { ...current.properties, ...changes };
      const valid = Number.isInteger(properties.paddingPx) && properties.paddingPx >= 0 && properties.paddingPx <= 4096
        && Number.isInteger(properties.bleedPx) && properties.bleedPx >= 0 && properties.bleedPx <= 4096
        && (properties.materialId === undefined || (Number.isInteger(properties.materialId) && properties.materialId >= 0 && properties.materialId <= 65535));
      return valid ? { type: "set_properties", patchId, properties } : null;
    }, "Update patch properties");
  }

  function updateRectification(changes: Partial<RectificationSettings>): void {
    if (!selectedPatch) return;
    const patchId = selectedPatch.id;
    void applyFactory(() => {
      const current = latestPatches.current.find((patch) => patch.id === patchId);
      if (!current) return null;
      const settings = { ...current.rectification, ...changes };
      const valid = Number.isFinite(settings.scale) && settings.scale >= 0.01 && settings.scale <= 16
        && (settings.aspectRatio === undefined || (Number.isFinite(settings.aspectRatio) && settings.aspectRatio >= 0.01 && settings.aspectRatio <= 100));
      return valid ? { type: "set_rectification", patchId, settings } : null;
    }, "Update rectification");
  }

  function commitGeometry(patch: PatchSnapshot, geometry: PatchGeometry, label: string): void {
    const error = validatePatchGeometry(geometry);
    if (error) {
      setGeometryMessage(error);
      return;
    }
    setGeometryMessage(null);
    void apply({ type: "replace_geometry", patchId: patch.id, geometry }, label);
  }

  function patchKeyDown(event: React.KeyboardEvent<SVGPolygonElement>, patch: PatchSnapshot): void {
    const sourceForPatch = project.sources.find((candidate) => candidate.id === patch.sourceId) ?? selectedSource;
    const multiplier = event.shiftKey ? 10 : 1;
    const xStep = multiplier / Math.max(1, sourceForPatch?.width ?? 1);
    const yStep = multiplier / Math.max(1, sourceForPatch?.height ?? 1);
    const offset = event.key === "ArrowLeft" ? { x: -xStep, y: 0 }
      : event.key === "ArrowRight" ? { x: xStep, y: 0 }
        : event.key === "ArrowUp" ? { x: 0, y: -yStep }
          : event.key === "ArrowDown" ? { x: 0, y: yStep }
            : null;
    if (offset) {
      event.preventDefault();
      event.stopPropagation();
      void applyFactory(() => {
        const current = latestPatches.current.find((candidate) => candidate.id === patch.id);
        if (!current) return null;
        return { type: "replace_geometry", patchId: current.id, geometry: translateGeometry(current.geometry, offset.x, offset.y) };
      }, "Nudge patch");
    } else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      setSelectedPatchId(patch.id);
      setWorkingSourceId(patch.sourceId);
    } else if (event.key === "F2") {
      event.preventDefault();
      beginRename(patch);
    } else if (event.key === "Delete") {
      event.preventDefault();
      void deletePatch(patch);
    }
  }

  function cornerKeyDown(event: React.KeyboardEvent<SVGCircleElement>, corner: number): void {
    if (!selectedPatch || !displayGeometry || !selectedSource) return;
    const current = displayGeometry.corners[corner]!;
    const multiplier = event.shiftKey ? 10 : 1;
    const next = event.key === "ArrowLeft" ? { x: current.x - multiplier / selectedSource.width, y: current.y }
      : event.key === "ArrowRight" ? { x: current.x + multiplier / selectedSource.width, y: current.y }
        : event.key === "ArrowUp" ? { x: current.x, y: current.y - multiplier / selectedSource.height }
          : event.key === "ArrowDown" ? { x: current.x, y: current.y + multiplier / selectedSource.height }
            : null;
    if (!next) return;
    event.preventDefault();
    event.stopPropagation();
    const patchId = selectedPatch.id;
    const delta = { x: next.x - current.x, y: next.y - current.y };
    void applyFactory(() => {
      const patch = latestPatches.current.find((candidate) => candidate.id === patchId);
      if (!patch) return null;
      const point = patch.geometry.corners[corner]!;
      const geometry = moveCorner(patch.geometry, corner, { x: point.x + delta.x, y: point.y + delta.y });
      return validatePatchGeometry(geometry) === null ? { type: "replace_geometry", patchId, geometry } : null;
    }, "Nudge patch corner");
  }

  function commitCornerCoordinate(input: HTMLInputElement, corner: number, axis: "x" | "y"): void {
    if (!selectedPatch) return;
    const current = selectedPatch.geometry.corners[corner]!;
    const value = input.valueAsNumber / 100;
    if (!Number.isFinite(value)) {
      input.value = (current[axis] * 100).toFixed(3);
      return;
    }
    const patchId = selectedPatch.id;
    void applyFactory(() => {
      const patch = latestPatches.current.find((candidate) => candidate.id === patchId);
      if (!patch) return null;
      const point = patch.geometry.corners[corner]!;
      const geometry = moveCorner(patch.geometry, corner, { ...point, [axis]: value });
      const error = validatePatchGeometry(geometry);
      if (error) {
        setGeometryMessage(error);
        input.value = (point[axis] * 100).toFixed(3);
        return null;
      }
      setGeometryMessage(null);
      return { type: "replace_geometry", patchId, geometry };
    }, `Set ${cornerLabels[corner]} ${axis.toUpperCase()}`);
  }

  const overlayGeometries = project.patches.filter((patch) => selectedSetSourceIds.has(patch.sourceId)).map((patch) => ({
    patch,
    geometry: patch.id === selectedPatch?.id && displayGeometry ? displayGeometry : patch.geometry,
  }));
  const draftOverlay = (draft?.geometry?.corners ?? draft?.points ?? []).map(overlayPoint);
  const transformCorners = displayGeometry ? displayGeometry.corners.map(overlayPoint) : [];
  const transformCenter = transformCorners.length === 4 ? {
    x: transformCorners.reduce((sum, point) => sum + point.x, 0) / 4,
    y: transformCorners.reduce((sum, point) => sum + point.y, 0) / 4,
  } : null;
  const rotationHandles = transformCenter ? transformCorners.map((point) => {
    const dx = point.x - transformCenter.x;
    const dy = point.y - transformCenter.y;
    const distance = Math.max(1, Math.hypot(dx, dy));
    return { x: point.x + (dx / distance) * 20, y: point.y + (dy / distance) * 20 };
  }) : [];
  const selectedPreviewGeometry = displayGeometry ?? selectedPatch?.geometry;
  const loupeSize = { width: 176, height: 124 };
  const loupeImageSize = imageRect ? {
    width: imageRect.width * loupeZoom,
    height: imageRect.height * loupeZoom,
  } : { width: 1, height: 1 };

  return (
    <>
      <section className="workspace patch-workspace" aria-labelledby="patch-workspace-title" hidden={hidden}>
        <div ref={authoringSplitRef} className="authoring-split" style={{ gridTemplateColumns: `${workbenchShare}% 5px minmax(320px, 1fr)` }} onPointerDownCapture={clearSelectionFromEmpty}>
          <section className="source-workbench" aria-label="Patch source workbench">
            <header className="split-pane-title">
              <div><span>Workbench</span><strong id="patch-workspace-title">Material source</strong></div>
              <button onClick={() => selectedSourceSetId && onOpenSources(selectedSourceSetId)} disabled={!selectedSourceSetId} title="Auto-assign additional maps to the selected source">Add maps...</button>
            </header>
            <div ref={sourceBodyRef} className="source-workbench-body" style={{ gridTemplateColumns: `${sourceRailWidth}px 5px minmax(0, 1fr)` }}>
              <aside className="source-library" aria-label="Material sources">
                <div className="source-library-title"><span>Sources</span><button onClick={onAddSource} title="Add an independent image source">+</button></div>
                <div className="source-library-list">
                  {sourceSets.map((sourceSet, index) => <button key={sourceSet.id} className={sourceSet.id === selectedSourceSetId ? "active" : ""} onClick={() => selectSourceSet(sourceSet.id)} title={sourceSet.base?.displayName}>
                    {sourceSet.base ? <img src={sourcePreviewUrl(sourceSet.base)} alt="" /> : null}
                    <span><strong>{sourceSet.base?.displayName ?? `Source ${index + 1}`}</strong><small>{sourceSet.inputs.length} map{sourceSet.inputs.length === 1 ? "" : "s"}</small></span>
                  </button>)}
                  <button className="add-source-card" onClick={onAddSource}><span aria-hidden="true">+</span><strong>New source</strong></button>
                </div>
                <div className="source-library-title source-patch-title"><span>Patches</span><small>{sourcePatches.length}</small></div>
                <section className="patch-list source-patch-list" aria-label="Patches for selected source">
                  {sourcePatches.map((patch, index) => <div
                    key={patch.id}
                    data-patch-id={patch.id}
                    className={`${patch.id === selectedPatch?.id ? "active" : ""} ${patch.id === dropTargetPatchId ? "drop-target" : ""} ${patch.id === draggedPatchId ? "dragging" : ""}`}
                    onPointerDown={(event) => beginPatchReorder(event, patch)}
                    onClick={(event) => {
                      if ((event.target as Element).closest("button, input")) return;
                      setSelectedPatchId(patch.id);
                      setWorkingSourceId(patch.sourceId);
                      setEditingPatchId(null);
                    }}
                    onContextMenu={(event) => openContextMenu(event, patch)}
                    onKeyDown={(event) => {
                      const offset = event.altKey && event.key === "ArrowUp" ? -1 : event.altKey && event.key === "ArrowDown" ? 1 : 0;
                      if (offset) {
                        event.preventDefault();
                        const target = sourcePatches[Math.min(sourcePatches.length - 1, Math.max(0, index + offset))];
                        if (target) reorderPatch(patch.id, target.id);
                        return;
                      }
                      if (event.key !== "Delete" || tool !== "select") return;
                      event.preventDefault();
                      void deletePatch(patch);
                    }}
                  >
                    <input type="checkbox" checked={patch.enabled} aria-label={`Enable ${patch.name}`} onChange={(event) => void apply({ type: "set_enabled", patchId: patch.id, enabled: event.target.checked }, "Toggle patch")} />
                    {renamingPatchId === patch.id ? <input className="patch-name-editor" autoFocus value={renameDraft} maxLength={255} onChange={(event) => setRenameDraft(event.target.value)} onBlur={() => commitRename(patch)} onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); else if (event.key === "Escape") { setRenamingPatchId(null); setRenameDraft(patch.name); } }} /> : <button className="patch-select" disabled={tool !== "select"} onClick={() => { setSelectedPatchId(patch.id); setWorkingSourceId(patch.sourceId); setEditingPatchId(null); }} onDoubleClick={() => beginRename(patch)}>{patch.name}</button>}
                    <button
                      className="patch-menu-button"
                      aria-label={`Actions for ${patch.name}`}
                      title="Patch actions"
                      onClick={(event) => openContextMenu(event, patch)}
                    >...</button>
                  </div>)}
                  {!sourcePatches.length ? <p>No patches yet. Double-click the image to capture one.</p> : null}
                </section>
              </aside>
              <div className="pane-splitter source-splitter" role="separator" aria-label="Resize source list" aria-orientation="vertical" onPointerDown={(event) => beginPaneResize("sources", event)} onPointerMove={movePaneResize} onPointerUp={endPaneResize} onPointerCancel={endPaneResize} />
              <div className="source-canvas-column">
              <div className="source-map-tabs" role="tablist" aria-label="Material map slots" onWheel={(event) => {
                const movement = Math.abs(event.deltaY) >= Math.abs(event.deltaX) ? event.deltaY : event.deltaX;
                if (!movement) return;
                event.preventDefault();
                event.currentTarget.scrollLeft += movement;
              }}>
                  {channelOrder.map((channel) => {
                    const input = selectedSetSources.find((candidate) => candidate.channel === channel);
                    const appearance = channelAppearance[channel];
                    const needsBaseColor = channel !== "base_color" && !selectedSetSources.some((candidate) => candidate.channel === "base_color");
                    return <button
                      key={channel}
                      role="tab"
                      aria-selected={input?.id === selectedSource?.id}
                      className={`${input?.id === selectedSource?.id ? "active" : ""} ${input ? "filled" : "empty"}`}
                      disabled={needsBaseColor}
                      title={input ? `${appearance.label}: ${input.displayName} (${input.width} x ${input.height})` : needsBaseColor ? "Add Base Color first" : `Add a ${appearance.label} map`}
                      onContextMenu={(event) => {
                        event.preventDefault();
                        event.stopPropagation();
                        setMapContextMenu({ channel, x: event.clientX, y: event.clientY });
                      }}
                      onClick={() => {
                        if (!input) { if (selectedSourceSetId) onOpenSourceChannel(channel, selectedSourceSetId); return; }
                        if (!editingPatchId) setSelection({ kind: "none" });
                        setWorkingSourceId(input.id); setEditingPatchId(null); setLiveGeometry(null); setDraft(null); setPreview(null); setPrecisionPoint(null); manipulation.current = null; setTool("select");
                      }}
                    ><span className={`channel-swatch ${appearance.tone}`}>{appearance.short}</span><span><strong>{appearance.label}</strong><small>{input?.displayName ?? (needsBaseColor ? "Needs Base Color" : "+ Add map")}</small></span></button>;
                  })}
              </div>
            <div
              ref={viewportRef}
              className={`viewport patch-viewport tool-${tool} ${selectedSource ? "has-source" : ""}`}
              onPointerDown={pointerDown}
              onDoubleClick={(event) => {
                if (tool !== "select" || event.button !== 0) return;
                if ((event.target as Element).closest("button, polygon, circle, rect, input, .precision-loupe")) return;
                event.preventDefault();
                const point = normalizedAt(event.clientX, event.clientY);
                beginNew("four_point", point ?? undefined);
              }}
              onPointerMove={pointerMove}
              onPointerUp={pointerUp}
              onPointerCancel={pointerCancel}
              onPointerLeave={() => { if (!manipulation.current) setPrecisionPoint(null); }}
              onAuxClick={(event) => event.preventDefault()}
              onWheel={(event) => {
                event.preventDefault();
                lastCursor.current = { x: event.clientX, y: event.clientY };
                zoomAtCursor(event.deltaY < 0 ? 1.1 : 0.9, lastCursor.current);
              }}
            >
              {selectedSource && imageUrl ? <img ref={imageRef} src={imageUrl} alt={`Patch source ${selectedSource.displayName}`} draggable={false} onLoad={refreshImageRect} style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.scale})` }} /> : null}
              <svg className="patch-overlay" viewBox={`0 0 ${viewportSize.width} ${viewportSize.height}`} aria-label="Editable patch outlines">
                {overlayGeometries.map(({ patch, geometry }) => {
                  const points = geometry.corners.map(overlayPoint);
                  return <g key={patch.id} data-preserve-selection className={`${patch.id === selectedPatch?.id ? "selected" : ""} ${patch.enabled ? "" : "disabled"}`}>
                    <polygon
                      points={points.map((point) => `${point.x},${point.y}`).join(" ")}
                      aria-label={`${patch.name} patch outline`}
                      role="button"
                      tabIndex={patch.id === editingPatchId ? -1 : 0}
                      onPointerDown={(event) => beginMove(event, patch, geometry)}
                      onFocus={() => { setSelectedPatchId(patch.id); setWorkingSourceId(patch.sourceId); }}
                      onKeyDown={(event) => patchKeyDown(event, patch)}
                      onDoubleClick={(event) => { event.preventDefault(); event.stopPropagation(); event.currentTarget.blur(); setSelectedPatchId(patch.id); setWorkingSourceId(patch.sourceId); setEditingPatchId(patch.id); }}
                      onContextMenu={(event) => openContextMenu(event, patch)}
                    />
                    {patch.id === editingPatchId ? points.map((point, index) => <g key={index} className="point-handle">
                      <circle className="point-hit" cx={point.x} cy={point.y} r={14} role="button" tabIndex={0} aria-label={`${patch.name} ${cornerLabels[index]} corner; use arrow keys to move one pixel and Shift plus arrow for ten pixels`} onPointerDown={(event) => beginCorner(event, index)} onKeyDown={(event) => cornerKeyDown(event, index)} />
                      <circle className="point-ring" cx={point.x} cy={point.y} r={6} />
                      <path className="point-crosshair" d={`M ${point.x - 10} ${point.y} H ${point.x + 10} M ${point.x} ${point.y - 10} V ${point.y + 10}`} />
                      <text x={point.x + 11} y={point.y - 10}>{cornerLabels[index]}</text>
                    </g>) : null}
                  </g>;
                })}
                {editingSourceRegion ? (() => {
                  const geometry = regionSourceGeometry ?? sourceCropGeometry;
                  const points = geometry.corners.map(overlayPoint);
                  const center = points.reduce((total, point) => ({ x: total.x + point.x / points.length, y: total.y + point.y / points.length }), { x: 0, y: 0 });
                  return <g className="source-region-editor" data-preserve-selection>
                    <polygon points={points.map((point) => `${point.x},${point.y}`).join(" ")} onPointerDown={(event) => beginSourceCropDrag(event, "move")} />
                    {points.map((point, index) => <g key={index} className="source-region-handle">
                      <rect className="source-region-handle-hit" x={point.x - 18} y={point.y - 18} width={36} height={36} onPointerDown={(event) => beginSourceCropDrag(event, "corner", index as 0 | 1 | 2 | 3)} />
                      <rect className="source-region-handle-visible" x={point.x - 7} y={point.y - 7} width={14} height={14} />
                    </g>)}
                    {regionSourceLayer ? points.map((point, index) => <circle key={`rotate-${index}`} className="corner-rotate" cx={center.x + (point.x - center.x) * 1.2} cy={center.y + (point.y - center.y) * 1.2} r={13} onPointerDown={(event) => beginSourceCropDrag(event, "rotate")} />) : null}
                  </g>;
                })() : null}
                {regionSourceGeometry && !editingSourceRegion ? (() => {
                  const activate = (): void => { void editSelectedRegionSource(); };
                  const points = regionSourceGeometry.corners.map(overlayPoint);
                  return <g className="region-source-selection" data-preserve-selection>
                    <polygon points={points.map((point) => `${point.x},${point.y}`).join(" ")} aria-label={`${workbenchRegion?.label ?? "Selected region"} source layer; press Enter to edit`} role="button" tabIndex={0} onPointerDown={(event) => { event.preventDefault(); event.stopPropagation(); }} onClick={(event) => { event.preventDefault(); event.stopPropagation(); activate(); }} onDoubleClick={(event) => { event.preventDefault(); event.stopPropagation(); activate(); }} onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); activate(); } }} />
                    {points.map((point, index) => <g key={index}><circle cx={point.x} cy={point.y} r={6} /><path d={`M ${point.x - 10} ${point.y} H ${point.x + 10} M ${point.x} ${point.y - 10} V ${point.y + 10}`} /></g>)}
                  </g>;
                })() : null}
                {selectedPatch && !editingSourceRegion && transformCorners.length === 4 && editingPatchId !== selectedPatch.id ? <g className="transform-box" data-preserve-selection>
                  {transformCorners.map((point, index) => <rect key={index} x={point.x - 6} y={point.y - 6} width={12} height={12} onPointerDown={(event) => beginScale(event, index as 0 | 1 | 2 | 3)} />)}
                  {rotationHandles.map((point, index) => <circle key={index} className="corner-rotate" cx={point.x} cy={point.y} r={13} onPointerDown={beginRotate} />)}
                </g> : null}
                {draftOverlay.length ? <g className="draft"><polyline points={draftOverlay.map((point) => `${point.x},${point.y}`).join(" ")} />{draftOverlay.map((point, index) => <g key={index} className="draft-point"><circle cx={point.x} cy={point.y} r={6} /><path d={`M ${point.x - 9} ${point.y} H ${point.x + 9} M ${point.x} ${point.y - 9} V ${point.y + 9}`} /><text x={point.x + 10} y={point.y - 9}>{index + 1}</text></g>)}</g> : null}
              </svg>

              <div className="patch-toolbar" role="toolbar" aria-label="Patch capture tools" onPointerDown={(event) => event.stopPropagation()}>
                <button className={tool === "four_point" ? "active" : ""} onClick={() => beginNew("four_point")} title="Click any four boundary corners; completion is automatic">+ 4 Point</button>
                <button className={tool === "rectangle" ? "active" : ""} onClick={() => beginNew("rectangle")}>+ Rectangle</button>
                <button className={tool === "polygon" ? "active" : ""} onClick={() => beginNew("polygon")} title="Trace 8 points around an irregular boundary; Hot Trimmer automatically fits an editable four-corner patch">+ Outline Fit (8)</button>
                {tool !== "select" ? <button onClick={() => { setTool("select"); setDraft(null); setGeometryMessage(null); }}>Cancel</button> : null}
              </div>

              {draft ? <div className="capture-status" role="status">
                <strong>{draft.mode === "four_point" ? `${draft.points.length} / 4 points` : draft.mode === "polygon" ? `${draft.points.length} / 8 points` : "Drag rectangle"}</strong>
                <span>{draft.mode === "four_point" ? "Click corners in any order; auto-finishes at four" : draft.mode === "rectangle" ? "Release to create; hold Shift while resizing to keep proportions" : "Trace the visible boundary; auto-fits an editable quad at 8 points (Enter fits after 4+)"}</span>
              </div> : null}
              {geometryMessage ? <div className="geometry-toast" role="alert">{geometryMessage}</div> : null}
              {precisionPoint && imageUrl && (draft || editingPatchId) ? <div className="precision-loupe" aria-label={`${loupeZoom} times precision preview`} onPointerDown={(event) => event.stopPropagation()}>
                <div className="precision-loupe-image" style={{
                  backgroundImage: `url("${imageUrl}")`,
                  backgroundSize: `${loupeImageSize.width}px ${loupeImageSize.height}px`,
                  backgroundPosition: `${loupeSize.width / 2 - precisionPoint.x * loupeImageSize.width}px ${loupeSize.height / 2 - precisionPoint.y * loupeImageSize.height}px`,
                }}><span className="loupe-crosshair" /></div>
                <div className="loupe-controls">{([2, 3, 4] as const).map((zoom) => <button key={zoom} className={loupeZoom === zoom ? "active" : ""} onClick={() => setLoupeZoom(zoom)}>{zoom}×</button>)}</div>
              </div> : null}
              {selectedPatch && selectedPreviewGeometry && imageUrl ? <section className="patch-preview-card" aria-label="Patch authoring preview" onPointerDown={(event) => event.stopPropagation()}>
                <header><strong>Patch view · {selectedPatch.name}</strong><span>{previewBusy ? "Refining…" : preview ? `${preview.width} × ${preview.height}` : "Live"}</span></header>
                <div><LiveRectifiedCanvas geometry={selectedPreviewGeometry} imageUrl={imageUrl} label={`Rectified authoring preview of ${selectedPatch.name}`} aspectRatio={selectedPatch.rectification.aspectRatio} /></div>
                {previewBusy ? <progress aria-label="Patch preview refinement" /> : null}
              </section> : null}
              <div className="viewport-tools"><span>MMB pan · Wheel zoom</span><button onClick={() => setView({ x: 0, y: 0, scale: 1 })}>Fit</button><output>{Math.round(view.scale * 100)}%</output></div>
            </div>
              </div>
            </div>
          </section>

          <div className="pane-splitter workbench-splitter" role="separator" aria-label="Resize source and preview workspaces" aria-orientation="vertical" onPointerDown={(event) => beginPaneResize("workbench", event)} onPointerMove={movePaneResize} onPointerUp={endPaneResize} onPointerCancel={endPaneResize} />

          <LayoutWorkspace project={project} selectedPatchId={selection.kind === "patch" ? selection.patchId : null} selectedRegionId={workbenchRegion?.region.id ?? null} selectedSourceSetId={selectedSourceSetId} onLayoutState={onLayoutState} onFailure={onFailure} sourceTransform={sourceTransform} onRegionSelectionChange={acceptRegionSelection} />
        </div>
        {busy ? <div className="busy" role="status"><span /><strong>{busy}…</strong></div> : null}
      </section>

      <aside className="inspector patch-inspector" aria-label="Patch manager" hidden={hidden} onPointerDownCapture={clearSelectionFromEmpty}>
        {selection.kind === "none" && !editingPatchId ? <section className="inspector-section source-region-properties"><h2>Source framing</h2>
          <div className="source-framing-quick" role="group" aria-label="Source framing mode">
            {([{"mode":"cover","label":"Cover / crop"},{"mode":"stretch","label":"Stretch"},{"mode":"repeat","label":"Repeat"}] as const).map((option) => <button key={option.mode} className={sourceTransform.mode === option.mode ? "active" : ""} aria-pressed={sourceTransform.mode === option.mode} onClick={() => setSourceTransform((current) => ({ ...current, mode: option.mode }))}>{option.label}</button>)}
          </div>
          {sourceTransform.mode === "cover" ? <div className="inspector-focus-controls"><label>Focus X<input type="range" min={0} max={1} step={0.01} value={sourceTransform.cropFocus.x} onChange={(event) => setSourceTransform((current) => ({ ...current, cropFocus: { ...current.cropFocus, x: event.target.valueAsNumber } }))} /></label><label>Focus Y<input type="range" min={0} max={1} step={0.01} value={sourceTransform.cropFocus.y} onChange={(event) => setSourceTransform((current) => ({ ...current, cropFocus: { ...current.cropFocus, y: event.target.valueAsNumber } }))} /></label></div> : null}
          <button className={editingSourceRegion ? "active" : "primary"} onClick={() => {
            setEditingSourceRegion((current) => !current);
            setSelection({ kind: "none" });
            setEditingPatchId(null);
            setLiveGeometry(null);
          }}>{editingSourceRegion ? "Finish editing source framing" : "Edit Source Framing"}</button>
          <div className="source-region-bounds"><label>X (%)<input type="number" min={0} max={100} step={0.1} value={(sourceCropBounds.x * 100).toFixed(1)} onChange={(event) => setSourceCropField("x", event.target.valueAsNumber)} /></label><label>Y (%)<input type="number" min={0} max={100} step={0.1} value={(sourceCropBounds.y * 100).toFixed(1)} onChange={(event) => setSourceCropField("y", event.target.valueAsNumber)} /></label><label>Width (%)<input type="number" min={1} max={100} step={0.1} value={(sourceCropBounds.width * 100).toFixed(1)} onChange={(event) => setSourceCropField("width", event.target.valueAsNumber)} /></label><label>Height (%)<input type="number" min={1} max={100} step={0.1} value={(sourceCropBounds.height * 100).toFixed(1)} onChange={(event) => setSourceCropField("height", event.target.valueAsNumber)} /></label></div>
          <button onClick={() => setSourceTransform((current) => ({ ...current, cropBounds: { x: 0, y: 0, width: 1, height: 1 } }))}>Reset to full source</button>
          <p className="hint">Move or resize this crop directly on the source texture. Every whole-source trim region is sampled from it.</p>
        </section> : null}
        {workbenchRegion ? <section className="inspector-section region-properties"><h2>Selected region</h2>
          <p><strong>{workbenchRegion.label}</strong></p>
          <label>Source / patch<select value={workbenchRegion.region.fill.type === "rectified_patch" ? `patch:${workbenchRegion.region.fill.patchId}` : workbenchRegion.region.fill.type === "whole_source_set" ? `source:${workbenchRegion.region.fill.sourceSetId}` : ""} onChange={(event) => {
            const value = event.target.value;
            if (value.startsWith("source:")) void assignSelectedRegionFill({ type: "whole_source_set", sourceSetId: value.slice(7) });
            else if (value.startsWith("patch:")) {
              const patch = project.patches.find((candidate) => candidate.id === value.slice(6));
              const sourceForPatch = patch ? project.sources.find((candidate) => candidate.id === patch.sourceId) : undefined;
              if (patch && sourceForPatch) void assignSelectedRegionFill({ type: "rectified_patch", sourceSetId: sourceForPatch.sourceSetId, patchId: patch.id });
            }
          }}><option value="" disabled>Choose source</option>{project.sourceSets.map((sourceSet) => <option key={`source:${sourceSet.id}`} value={`source:${sourceSet.id}`}>{sourceSet.name} · whole source</option>)}{project.patches.filter((patch) => patch.enabled).map((patch) => <option key={`patch:${patch.id}`} value={`patch:${patch.id}`}>{patch.name} · patch</option>)}</select></label>
          {workbenchRegion.templateMode ? <label>Source framing<select value={sourceTransform.mode} onChange={(event) => setSourceTransform((current) => ({ ...current, mode: event.target.value as TemplateSourceTransform["mode"] }))}><option value="cover">Cover / crop</option><option value="stretch">Stretch</option><option value="repeat">Repeat</option></select></label> : null}
          {workbenchRegion.templateMode && sourceTransform.mode === "cover" ? <div className="inspector-focus-controls"><label>Focus X<input type="range" min={0} max={1} step={0.01} value={sourceTransform.cropFocus.x} onChange={(event) => setSourceTransform((current) => ({ ...current, cropFocus: { ...current.cropFocus, x: event.target.valueAsNumber } }))} /></label><label>Focus Y<input type="range" min={0} max={1} step={0.01} value={sourceTransform.cropFocus.y} onChange={(event) => setSourceTransform((current) => ({ ...current, cropFocus: { ...current.cropFocus, y: event.target.valueAsNumber } }))} /></label></div> : null}
          <button className="primary" onClick={() => void editSelectedRegionSource()}>Edit source area</button>
          {regionSourceGeometry ? <p className="hint">Source layer: {regionSourceGeometry.corners.map((point) => `${(point.x * 100).toFixed(1)}%, ${(point.y * 100).toFixed(1)}%`).join(" · ")}</p> : null}
          <p className="hint">Edit the highlighted source layer in place. Move, resize, rotate, or adjust perspective without changing this trim-sheet region’s bounds.</p>
        </section> : selectedPatch && (selection.kind === "patch" || editingPatchId === selectedPatch.id) ? <section className="inspector-section patch-properties"><h2>Placement behavior</h2>
          <div className="behavior-options" role="group" aria-label="Patch placement behavior">{behaviorOptions.map((option) => <button
            key={option.value}
            className={selectedPatch.properties.repeatMode === option.value ? "active" : ""}
            title={option.title}
            aria-pressed={selectedPatch.properties.repeatMode === option.value}
            onClick={() => updateProperties({ repeatMode: option.value })}
          ><BehaviorIcon kind={option.value} /><span>{option.label}</span></button>)}</div>
          {(selectedPatch.properties.repeatMode === "repeat_x" || selectedPatch.properties.repeatMode === "repeat_y") ? <label className="check-field"><input type="checkbox" checked={selectedPatch.properties.trimCap} onChange={(event) => updateProperties({ trimCap: event.target.checked })} /> Add a non-repeating end cap</label> : null}
          <details className="advanced-properties">
            <summary>Output settings</summary>
            <p>These settings are used when rebuilding the final trim sheet.</p>
            <div className="numeric-pair"><label>Safe edge (px)<input type="number" min={0} max={4096} value={selectedPatch.properties.paddingPx} onChange={(event) => updateProperties({ paddingPx: event.target.valueAsNumber })} /><small>Empty spacing kept around the packed region.</small></label><label>Texture bleed (px)<input type="number" min={0} max={4096} value={selectedPatch.properties.bleedPx} onChange={(event) => updateProperties({ bleedPx: event.target.valueAsNumber })} /><small>Pixels extended past the UV edge to prevent seams.</small></label></div>
            <label>Material ID<input type="number" min={0} max={65535} placeholder="None" value={selectedPatch.properties.materialId ?? ""} onChange={(event) => updateProperties({ materialId: event.target.value === "" ? undefined : event.target.valueAsNumber })} /></label>
            <label>Maps included<select value={selectedPatch.properties.mapParticipation} onChange={(event) => updateProperties({ mapParticipation: event.target.value as PatchProperties["mapParticipation"] })}><option value="all">Every registered map</option><option value="base_color_only">Base Color only</option><option value="excluded">Do not generate maps</option></select></label>
            <div className="numeric-pair"><label>Output aspect<input type="number" min={0.01} max={100} step={0.01} placeholder="Auto" value={selectedPatch.rectification.aspectRatio ?? ""} onChange={(event) => updateRectification({ aspectRatio: event.target.value === "" ? undefined : event.target.valueAsNumber })} /></label><label>Resolution scale<input type="number" min={0.01} max={16} step={0.05} value={selectedPatch.rectification.scale} onChange={(event) => updateRectification({ scale: event.target.valueAsNumber })} /></label></div>
            <fieldset className="corner-coordinate-editor"><legend>Corner coordinates (%)</legend>{selectedPatch.geometry.corners.map((point, index) => <div key={`${selectedPatch.id}-${index}`}><strong>{cornerLabels[index]}</strong><label>X<input key={`${selectedPatch.id}-${index}-x-${point.x}`} type="number" min={0} max={100} step={0.001} defaultValue={(point.x * 100).toFixed(3)} onBlur={(event) => commitCornerCoordinate(event.currentTarget, index, "x")} onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); else if (event.key === "Escape") { event.currentTarget.value = (point.x * 100).toFixed(3); event.currentTarget.blur(); } }} /></label><label>Y<input key={`${selectedPatch.id}-${index}-y-${point.y}`} type="number" min={0} max={100} step={0.001} defaultValue={(point.y * 100).toFixed(3)} onBlur={(event) => commitCornerCoordinate(event.currentTarget, index, "y")} onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); else if (event.key === "Escape") { event.currentTarget.value = (point.y * 100).toFixed(3); event.currentTarget.blur(); } }} /></label></div>)}</fieldset>
          </details>
          <p className="hint">Drag inside to move. Drag a square corner to resize; hold Shift to keep proportions. Move just outside a corner for rotation. Double-click the patch to edit its four perspective points.</p>
        </section> : <section className="inspector-section"><p className="hint">Select a patch or create one on the source workbench.</p></section>}
      </aside>

      {patchDragGhost ? <div
        className="patch-drag-ghost"
        aria-hidden="true"
        style={{ left: patchDragGhost.left, top: patchDragGhost.top, width: patchDragGhost.width }}
      >
        <span className={`patch-drag-check ${patchDragGhost.enabled ? "checked" : ""}`}>{patchDragGhost.enabled ? "✓" : ""}</span>
        <strong>{patchDragGhost.name}</strong>
        <span className="patch-drag-menu">...</span>
      </div> : null}

      {contextMenu ? <div className="patch-context-menu" role="menu" style={{ left: contextMenu.x, top: contextMenu.y }} onPointerDown={(event) => event.stopPropagation()}>
        {(() => { const patch = project.patches.find((candidate) => candidate.id === contextMenu.patchId); return patch ? <>
          <button role="menuitem" onClick={() => { beginRename(patch); setContextMenu(null); }}>Rename</button>
          <button role="menuitem" onClick={() => { void duplicatePatch(patch); setContextMenu(null); }}>Duplicate</button>
          <button role="menuitem" onClick={() => { void apply({ type: "set_enabled", patchId: patch.id, enabled: !patch.enabled }, "Toggle patch"); setContextMenu(null); }}>{patch.enabled ? "Disable" : "Enable"}</button>
          <button role="menuitem" className="danger" onClick={() => { void deletePatch(patch); setContextMenu(null); }}>Delete</button>
        </> : null; })()}
      </div> : null}
      {mapContextMenu && selectedSourceSetId ? <div className="patch-context-menu map-context-menu" role="menu" style={{ left: mapContextMenu.x, top: mapContextMenu.y }} onPointerDown={(event) => event.stopPropagation()}>
        <strong>{channelAppearance[mapContextMenu.channel].label}</strong>
        <button role="menuitem" onClick={() => { onOpenSourceChannel(mapContextMenu.channel, selectedSourceSetId); setMapContextMenu(null); }}>{selectedSetSources.some((source) => source.channel === mapContextMenu.channel) ? "Replace map…" : "Import map…"}</button>
        <button role="menuitem" disabled title="Map estimation is not available in this workspace">Generate from Base Color</button>
      </div> : null}
    </>
  );
}
