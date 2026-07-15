import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import {
  IPC_PROTOCOL_VERSION,
  type CommandFailure,
  type FoundationStatusRequest,
  type NormalizedPoint,
  type PatchCommand,
  type PatchGeometry,
  type PatchPreviewSnapshot,
  type PatchProperties,
  type PatchSnapshot,
  type PatchStateSnapshot,
  type ProjectSnapshot,
  type RectificationSettings,
  type SourceChannel,
  type SourceSnapshot,
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
} from "./patch-authoring";
import { LiveRectifiedCanvas } from "./live-rectified-canvas";

type PatchTool = "select" | "four_point" | "rectangle" | "polygon";

interface PatchWorkspaceProps {
  hidden: boolean;
  project: ProjectSnapshot;
  onPatchState: (state: PatchStateSnapshot) => void;
  onFailure: (failure: CommandFailure | null) => void;
  onOpenSources: () => void;
  onOpenSourceChannel: (channel: SourceChannel) => void;
}

interface ViewTransform { x: number; y: number; scale: number }
interface ImageRect { left: number; top: number; width: number; height: number }
interface DraftState {
  mode: "four_point" | "rectangle" | "polygon";
  points: NormalizedPoint[];
  geometry?: PatchGeometry;
}
interface ContextMenuState { patchId: string; x: number; y: number }
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

function behaviorLabel(value: PatchProperties["repeatMode"]): string {
  return behaviorOptions.find((option) => option.value === value)?.label ?? value;
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

export function PatchWorkspace({
  hidden,
  project,
  onPatchState,
  onFailure,
  onOpenSources,
  onOpenSourceChannel,
}: PatchWorkspaceProps): React.JSX.Element {
  const [tool, setTool] = useState<PatchTool>("select");
  const [selectedPatchId, setSelectedPatchId] = useState<string | null>(project.patches[0]?.id ?? null);
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
  const [renamingPatchId, setRenamingPatchId] = useState<string | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
  const [draggedPatchId, setDraggedPatchId] = useState<string | null>(null);
  const [dropTargetPatchId, setDropTargetPatchId] = useState<string | null>(null);
  const [workingSourceId, setWorkingSourceId] = useState<string | null>(
    project.sources.find((candidate) => candidate.channel === "base_color")?.id ?? project.sources[0]?.id ?? null,
  );
  const imageRef = useRef<HTMLImageElement | null>(null);
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const manipulation = useRef<Manipulation | null>(null);
  const applySequence = useRef(0);
  const latestGeometryCommit = useRef(0);
  const previousSourceCount = useRef(project.sources.length);
  const source = project.sources.find((candidate) => candidate.id === workingSourceId)
    ?? project.sources.find((candidate) => candidate.channel === "base_color")
    ?? project.sources[0];
  const selectedPatch = project.patches.find((patch) => patch.id === selectedPatchId) ?? null;
  const selectedSource = source;
  const imageUrl = sourcePreviewUrl(selectedSource);
  const displayGeometry = liveGeometry ?? selectedPatch?.geometry;

  useEffect(() => {
    if (selectedPatchId && project.patches.some((patch) => patch.id === selectedPatchId)) return;
    setSelectedPatchId(project.patches[0]?.id ?? null);
    setEditingPatchId(null);
    setLiveGeometry(null);
  }, [project.patches, selectedPatchId]);

  useEffect(() => {
    if (project.sources.length > previousSourceCount.current) {
      setWorkingSourceId(project.sources.at(-1)?.id ?? null);
    } else if (workingSourceId && !project.sources.some((candidate) => candidate.id === workingSourceId)) {
      setWorkingSourceId(project.sources[0]?.id ?? null);
    }
    previousSourceCount.current = project.sources.length;
  }, [project.sources, workingSourceId]);

  useEffect(() => {
    if (!contextMenu) return;
    const close = () => setContextMenu(null);
    window.addEventListener("pointerdown", close);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("pointerdown", close);
      window.removeEventListener("blur", close);
    };
  }, [contextMenu]);

  useEffect(() => {
    if (!selectedPatch || !selectedSource || hidden) return;
    const geometry = liveGeometry ?? selectedPatch.geometry;
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
        if (result.patchId === selectedPatch.id) setPreview(result);
      }).catch((reason) => {
        const failure = failureFrom(reason, "Patch preview refinement failed.");
        if (failure.code !== "operation_cancelled") onFailure(failure);
      }).finally(() => setPreviewBusy(false));
    }, 80);
    return () => {
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

  async function apply(command: PatchCommand, label: string, coalescingGroup?: number): Promise<PatchStateSnapshot | null> {
    const sequence = ++applySequence.current;
    setBusy(label);
    onFailure(null);
    try {
      const state = await invoke<PatchStateSnapshot>("apply_patch_command", {
        request: { protocolVersion: IPC_PROTOCOL_VERSION, command, coalescingGroup },
      });
      if (sequence === applySequence.current) onPatchState(state);
      return state;
    } catch (reason) {
      onFailure(failureFrom(reason, `${label} failed.`));
      return null;
    } finally {
      if (sequence === applySequence.current) setBusy(null);
    }
  }

  async function history(redo: boolean): Promise<void> {
    applySequence.current += 1;
    setBusy(redo ? "Redo" : "Undo");
    try {
      const state = await invoke<PatchStateSnapshot>(redo ? "redo_patch_command" : "undo_patch_command", { request });
      onPatchState(state);
      setLiveGeometry(null);
      setDraft(null);
    } catch (reason) {
      onFailure(failureFrom(reason, `${redo ? "Redo" : "Undo"} failed.`));
    } finally {
      setBusy(null);
    }
  }

  function beginNew(mode: DraftState["mode"] = "four_point"): void {
    if (!source) {
      onOpenSources();
      return;
    }
    setTool(mode);
    setEditingPatchId(null);
    setLiveGeometry(null);
    setGeometryMessage(null);
    setDraft({ mode, points: [] });
  }

  async function createPatch(geometry: PatchGeometry): Promise<void> {
    if (!source) return;
    const error = validatePatchGeometry(geometry);
    if (error) {
      setGeometryMessage(error);
      return;
    }
    const patch = defaultPatch(source, geometry, project.patches.length);
    const state = await apply({ type: "create", patch }, "Create patch");
    if (!state) return;
    setSelectedPatchId(patch.id);
    setEditingPatchId(null);
    setDraft(null);
    setTool("select");
    setGeometryMessage(null);
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
        setView((current) => ({ ...current, scale: Math.min(8, current.scale * 1.25) }));
      } else if (event.key === "-") {
        setView((current) => ({ ...current, scale: Math.max(0.1, current.scale * 0.8) }));
      }
    }
    window.addEventListener("keydown", keyboard);
    return () => window.removeEventListener("keydown", keyboard);
  });

  function pointerDown(event: React.PointerEvent<HTMLDivElement>): void {
    if (!selectedSource) return;
    if (event.button === 1) {
      event.preventDefault();
      event.currentTarget.setPointerCapture(event.pointerId);
      manipulation.current = { kind: "pan", pointerId: event.pointerId, x: event.clientX, y: event.clientY, origin: view };
      return;
    }
    if (event.button !== 0) return;
    if (tool === "select") {
      setEditingPatchId(null);
      setGeometryMessage(null);
    }
    const point = normalizedAt(event.clientX, event.clientY);
    if (!point) return;
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
    const active = manipulation.current;
    if (!active || active.pointerId !== event.pointerId) return;
    if (active.kind === "pan") {
      setView({ ...active.origin, x: active.origin.x + event.clientX - active.x, y: active.origin.y + event.clientY - active.y });
      return;
    }
    const point = normalizedManipulationAt(event.clientX, event.clientY, active.kind === "rotate");
    if (!point) return;
    if (active.kind === "rectangle") {
      const geometry = rectangleGeometry(active.start, point);
      active.current = geometry;
      setDraft({ mode: "rectangle", points: geometry.corners, geometry });
      setGeometryMessage(validatePatchGeometry(geometry));
      return;
    }
    let geometry = active.current;
    if (active.kind === "corner" && active.corner !== undefined) {
      geometry = moveCorner(active.geometry, active.corner, point);
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
    setLiveGeometry(geometry);
    setGeometryMessage(validatePatchGeometry(geometry));
  }

  function pointerUp(event: React.PointerEvent<HTMLDivElement>): void {
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
    latestGeometryCommit.current = active.group;
    void apply({ type: "replace_geometry", patchId: active.patchId, geometry: active.current }, "Transform patch", active.group)
      .then(() => {
        if (latestGeometryCommit.current === active.group && manipulation.current === null) setLiveGeometry(null);
      });
  }

  function pointerCancel(event: React.PointerEvent<HTMLDivElement>): void {
    if (manipulation.current?.pointerId === event.pointerId) cancelManipulation();
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
  }

  function beginMove(event: React.PointerEvent<SVGPolygonElement>, patch: PatchSnapshot, geometry: PatchGeometry): void {
    if (event.button === 1) return;
    if (event.button !== 0) return;
    const point = normalizedAt(event.clientX, event.clientY);
    if (!point) return;
    event.stopPropagation();
    setSelectedPatchId(patch.id);
    setWorkingSourceId(patch.sourceId);
    if (editingPatchId !== patch.id) setEditingPatchId(null);
    manipulation.current = {
      kind: "move", pointerId: event.pointerId, patchId: patch.id, start: point,
      startClient: { x: event.clientX, y: event.clientY }, moved: false,
      geometry, current: geometry, group: Date.now(),
    };
  }

  function beginCorner(event: React.PointerEvent<SVGCircleElement>, corner: number): void {
    if (!displayGeometry || !selectedPatch || event.button !== 0) return;
    captureManipulationPointer(event);
    manipulation.current = {
      kind: "corner", pointerId: event.pointerId, patchId: selectedPatch.id, corner,
      geometry: displayGeometry, current: displayGeometry, moved: false, group: Date.now(),
    };
  }

  function beginScale(event: React.PointerEvent<SVGRectElement>, handle: 0 | 1 | 2 | 3): void {
    if (!displayGeometry || !selectedPatch || event.button !== 0) return;
    captureManipulationPointer(event);
    manipulation.current = {
      kind: "scale", pointerId: event.pointerId, patchId: selectedPatch.id, handle,
      geometry: displayGeometry, current: displayGeometry, moved: false, group: Date.now(),
    };
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
      geometry: displayGeometry, current: displayGeometry, moved: false, group: Date.now(),
    };
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
    if (!await confirm(`Delete ${patch.name}?`, { title: "Delete patch", kind: "warning" })) return;
    await apply({ type: "delete", patchId: patch.id }, "Delete patch");
  }

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

  function updateProperties(properties: PatchProperties): void {
    if (selectedPatch) void apply({ type: "set_properties", patchId: selectedPatch.id, properties }, "Update patch properties");
  }

  function updateRectification(settings: RectificationSettings): void {
    if (selectedPatch) void apply({ type: "set_rectification", patchId: selectedPatch.id, settings }, "Update rectification");
  }

  const overlayGeometries = project.patches.map((patch) => ({
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

  return (
    <>
      <section className="workspace patch-workspace" aria-labelledby="patch-workspace-title" hidden={hidden}>
        <header className="panel-title patch-title">
          <div>
            <span className="eyebrow">Material workbench → rectified patch preview</span>
            <strong id="patch-workspace-title">{selectedSource?.displayName ?? "Add a material source to begin"}</strong>
          </div>
          <div className="patch-history" aria-label="Patch edit history">
            <button onClick={() => void history(false)} disabled={!project.canUndoPatch || busy !== null} title="Undo (Ctrl+Z)">Undo</button>
            <button onClick={() => void history(true)} disabled={!project.canRedoPatch || busy !== null} title="Redo (Ctrl+Y)">Redo</button>
          </div>
        </header>

        <div className="authoring-split">
          <section className="source-workbench" aria-label="Patch source workbench">
            <header className="split-pane-title">
              <div><span>Workplace</span><strong>Material source</strong></div>
              <button onClick={onOpenSources} title="Add Normal, Roughness, Height, and other registered maps to Material 1">Add maps...</button>
            </header>
            <div className="source-set-strip">
              <div className="source-set-card"><strong>Material 1</strong><small>{project.sources.length} map{project.sources.length === 1 ? "" : "s"}</small></div>
              <div className="source-map-tabs" role="tablist" aria-label="Material map slots">
                  {channelOrder.map((channel) => {
                    const input = project.sources.find((candidate) => candidate.channel === channel);
                    const appearance = channelAppearance[channel];
                    const needsBaseColor = channel !== "base_color" && !project.sources.some((candidate) => candidate.channel === "base_color");
                    return <button
                      key={channel}
                      role="tab"
                      aria-selected={input?.id === selectedSource?.id}
                      className={`${input?.id === selectedSource?.id ? "active" : ""} ${input ? "filled" : "empty"}`}
                      disabled={needsBaseColor}
                      title={input ? `${appearance.label}: ${input.displayName} (${input.width} x ${input.height})` : needsBaseColor ? "Add Base Color first" : `Add a ${appearance.label} map`}
                      onClick={() => {
                        if (!input) { onOpenSourceChannel(channel); return; }
                        setWorkingSourceId(input.id); setEditingPatchId(null); setDraft(null); setTool("select");
                      }}
                    ><span className={`channel-swatch ${appearance.tone}`}>{appearance.short}</span><span><strong>{appearance.label}</strong><small>{input?.displayName ?? (needsBaseColor ? "Needs Base Color" : "+ Add map")}</small></span></button>;
                  })}
              </div>
            </div>
            <div
              ref={viewportRef}
              className={`viewport patch-viewport tool-${tool} ${selectedSource ? "has-source" : ""}`}
              onPointerDown={pointerDown}
              onPointerMove={pointerMove}
              onPointerUp={pointerUp}
              onPointerCancel={pointerCancel}
              onAuxClick={(event) => event.preventDefault()}
              onWheel={(event) => { event.preventDefault(); setView((current) => ({ ...current, scale: Math.min(8, Math.max(0.1, current.scale * (event.deltaY < 0 ? 1.1 : 0.9))) })); }}
            >
              {selectedSource && imageUrl ? <img ref={imageRef} src={imageUrl} alt={`Patch source ${selectedSource.displayName}`} draggable={false} onLoad={refreshImageRect} style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.scale})` }} /> : null}
              <svg className="patch-overlay" viewBox={`0 0 ${viewportSize.width} ${viewportSize.height}`} aria-label="Editable patch outlines">
                {overlayGeometries.map(({ patch, geometry }) => {
                  const points = geometry.corners.map(overlayPoint);
                  return <g key={patch.id} className={`${patch.id === selectedPatch?.id ? "selected" : ""} ${patch.enabled ? "" : "disabled"}`}>
                    <polygon
                      points={points.map((point) => `${point.x},${point.y}`).join(" ")}
                      aria-label={`${patch.name} patch outline`}
                      onPointerDown={(event) => beginMove(event, patch, geometry)}
                      onDoubleClick={(event) => { event.stopPropagation(); setSelectedPatchId(patch.id); setWorkingSourceId(patch.sourceId); setEditingPatchId(patch.id); }}
                      onContextMenu={(event) => openContextMenu(event, patch)}
                    />
                    {patch.id === editingPatchId ? points.map((point, index) => <g key={index} className="point-handle"><circle cx={point.x} cy={point.y} r={8} aria-label={`${patch.name} ${cornerLabels[index]} corner`} onPointerDown={(event) => beginCorner(event, index)} /><text x={point.x + 11} y={point.y - 10}>{cornerLabels[index]}</text></g>) : null}
                  </g>;
                })}
                {selectedPatch && transformCorners.length === 4 && editingPatchId !== selectedPatch.id ? <g className="transform-box">
                  {transformCorners.map((point, index) => <rect key={index} x={point.x - 6} y={point.y - 6} width={12} height={12} onPointerDown={(event) => beginScale(event, index as 0 | 1 | 2 | 3)} />)}
                  {rotationHandles.map((point, index) => <circle key={index} className="corner-rotate" cx={point.x} cy={point.y} r={13} onPointerDown={beginRotate} />)}
                </g> : null}
                {draftOverlay.length ? <g className="draft"><polyline points={draftOverlay.map((point) => `${point.x},${point.y}`).join(" ")} />{draftOverlay.map((point, index) => <g key={index}><circle cx={point.x} cy={point.y} r={7} /><text x={point.x + 10} y={point.y - 9}>{index + 1}</text></g>)}</g> : null}
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
              <div className="viewport-tools"><span>MMB pan · Wheel zoom</span><button onClick={() => setView({ x: 0, y: 0, scale: 1 })}>Fit</button><output>{Math.round(view.scale * 100)}%</output></div>
            </div>
          </section>

          <section className="hotspot-workpiece" aria-label="Hotspot sheet workpiece">
            <header className="split-pane-title">
              <div><span>Live preview</span><strong>Selected rectified patch</strong></div>
              <small>Phase 3 turns this into the layout canvas</small>
            </header>
            <div className="workpiece-canvas">
              {selectedPatch && selectedPreviewGeometry && imageUrl ? <>
                <LiveRectifiedCanvas geometry={selectedPreviewGeometry} imageUrl={imageUrl} label={`Real-time rectified preview of ${selectedPatch.name}`} />
                <div className="workpiece-selection-label"><strong>{selectedPatch.name}</strong><span>{previewBusy ? "Refining…" : preview ? `Verified ${preview.width} × ${preview.height}` : "Live"}</span></div>
              </> : <div className="workpiece-empty"><strong>Rectified patch preview</strong><span>Create or select a patch on the left. Phase 3 places enabled patches and patch-free regions into the actual trim layout.</span></div>}
            </div>
          </section>
        </div>
        {busy ? <div className="busy" role="status"><span /><strong>{busy}…</strong></div> : null}
      </section>

      <aside className="inspector patch-inspector" aria-label="Patch manager" hidden={hidden}>
        <header className="panel-title"><div><span className="eyebrow">Workbench</span><strong>Patches</strong></div><span className="panel-hint">Double-click to rename · right-click for actions</span></header>
        <section className="patch-list" aria-label="Authored patches">
          {project.patches.map((patch, index) => <div
            key={patch.id}
            className={`${patch.id === selectedPatch?.id ? "active" : ""} ${patch.id === dropTargetPatchId ? "drop-target" : ""}`}
            onDragEnter={() => setDropTargetPatchId(patch.id)}
            onDragOver={(event) => { event.preventDefault(); event.dataTransfer.dropEffect = "move"; }}
            onDragLeave={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDropTargetPatchId(null); }}
            onDrop={(event) => {
              event.preventDefault();
              const patchId = event.dataTransfer.getData("text/plain") || draggedPatchId;
              if (patchId && patchId !== patch.id) void apply({ type: "reorder", patchId, toIndex: index }, "Reorder patches");
              setDraggedPatchId(null); setDropTargetPatchId(null);
            }}
            onContextMenu={(event) => openContextMenu(event, patch)}
          >
            <input type="checkbox" checked={patch.enabled} aria-label={`Enable ${patch.name}`} onChange={(event) => void apply({ type: "set_enabled", patchId: patch.id, enabled: event.target.checked }, "Toggle patch")} />
            {renamingPatchId === patch.id ? <input className="patch-name-editor" autoFocus value={renameDraft} maxLength={255} onChange={(event) => setRenameDraft(event.target.value)} onBlur={() => commitRename(patch)} onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); else if (event.key === "Escape") { setRenamingPatchId(null); setRenameDraft(patch.name); } }} /> : <button className="patch-select" onClick={() => { setSelectedPatchId(patch.id); setWorkingSourceId(patch.sourceId); setEditingPatchId(null); }} onDoubleClick={() => beginRename(patch)}>{patch.name}<small>{behaviorLabel(patch.properties.repeatMode)}</small></button>}
            <span
              className="drag-grip"
              role="button"
              tabIndex={0}
              draggable
              aria-label={`Reorder ${patch.name}`}
              title="Drag to reorder"
              onDragStart={(event) => { setDraggedPatchId(patch.id); event.dataTransfer.effectAllowed = "move"; event.dataTransfer.setData("text/plain", patch.id); }}
              onDragEnd={() => { setDraggedPatchId(null); setDropTargetPatchId(null); }}
              onKeyDown={(event) => {
                const offset = event.key === "ArrowUp" ? -1 : event.key === "ArrowDown" ? 1 : 0;
                if (!offset) return;
                event.preventDefault();
                const toIndex = Math.min(project.patches.length - 1, Math.max(0, index + offset));
                if (toIndex !== index) void apply({ type: "reorder", patchId: patch.id, toIndex }, "Reorder patches");
              }}
            >::::</span>
          </div>)}
          {!project.patches.length ? <p>No patches. Capture one on the left; it will finish automatically.</p> : null}
        </section>
        {selectedPatch ? <section className="inspector-section patch-properties"><h2>Placement behavior</h2>
          <div className="behavior-options" role="group" aria-label="Patch placement behavior">{behaviorOptions.map((option) => <button
            key={option.value}
            className={selectedPatch.properties.repeatMode === option.value ? "active" : ""}
            title={option.title}
            aria-pressed={selectedPatch.properties.repeatMode === option.value}
            onClick={() => updateProperties({ ...selectedPatch.properties, repeatMode: option.value })}
          ><BehaviorIcon kind={option.value} /><span>{option.label}</span></button>)}</div>
          {(selectedPatch.properties.repeatMode === "repeat_x" || selectedPatch.properties.repeatMode === "repeat_y") ? <label className="check-field"><input type="checkbox" checked={selectedPatch.properties.trimCap} onChange={(event) => updateProperties({ ...selectedPatch.properties, trimCap: event.target.checked })} /> Add a non-repeating end cap</label> : null}
          <details className="advanced-properties">
            <summary>Output settings</summary>
            <p>These settings are consumed when Phase 3 builds the final sheet.</p>
            <div className="numeric-pair"><label>Safe edge (px)<input type="number" min={0} max={4096} value={selectedPatch.properties.paddingPx} onChange={(event) => updateProperties({ ...selectedPatch.properties, paddingPx: event.target.valueAsNumber })} /><small>Empty spacing kept around the packed region.</small></label><label>Texture bleed (px)<input type="number" min={0} max={4096} value={selectedPatch.properties.bleedPx} onChange={(event) => updateProperties({ ...selectedPatch.properties, bleedPx: event.target.valueAsNumber })} /><small>Pixels extended past the UV edge to prevent seams.</small></label></div>
            <label>Material ID<input type="number" min={0} max={65535} placeholder="None" value={selectedPatch.properties.materialId ?? ""} onChange={(event) => updateProperties({ ...selectedPatch.properties, materialId: event.target.value === "" ? undefined : event.target.valueAsNumber })} /></label>
            <label>Maps included<select value={selectedPatch.properties.mapParticipation} onChange={(event) => updateProperties({ ...selectedPatch.properties, mapParticipation: event.target.value as PatchProperties["mapParticipation"] })}><option value="all">Every registered map</option><option value="base_color_only">Base Color only</option><option value="excluded">Do not generate maps</option></select></label>
            <div className="numeric-pair"><label>Output aspect<input type="number" min={0.01} max={100} step={0.01} placeholder="Auto" value={selectedPatch.rectification.aspectRatio ?? ""} onChange={(event) => updateRectification({ ...selectedPatch.rectification, aspectRatio: event.target.value === "" ? undefined : event.target.valueAsNumber })} /></label><label>Resolution scale<input type="number" min={0.01} max={16} step={0.05} value={selectedPatch.rectification.scale} onChange={(event) => updateRectification({ ...selectedPatch.rectification, scale: event.target.valueAsNumber })} /></label></div>
          </details>
          <p className="hint">Drag inside to move. Drag a square corner to resize; hold Shift to keep proportions. Move just outside a corner for rotation. Double-click the patch to edit its four perspective points.</p>
        </section> : <section className="inspector-section"><p className="hint">Select a patch or create one on the source workbench.</p></section>}
      </aside>

      {contextMenu ? <div className="patch-context-menu" role="menu" style={{ left: contextMenu.x, top: contextMenu.y }} onPointerDown={(event) => event.stopPropagation()}>
        {(() => { const patch = project.patches.find((candidate) => candidate.id === contextMenu.patchId); return patch ? <>
          <button role="menuitem" onClick={() => { beginRename(patch); setContextMenu(null); }}>Rename</button>
          <button role="menuitem" onClick={() => { void duplicatePatch(patch); setContextMenu(null); }}>Duplicate</button>
          <button role="menuitem" onClick={() => { void apply({ type: "set_enabled", patchId: patch.id, enabled: !patch.enabled }, "Toggle patch"); setContextMenu(null); }}>{patch.enabled ? "Disable" : "Enable"}</button>
          <button role="menuitem" className="danger" onClick={() => { void deletePatch(patch); setContextMenu(null); }}>Delete…</button>
        </> : null; })()}
      </div> : null}
    </>
  );
}
