import type { NormalizedBounds } from "@hot-trimmer/ipc-contracts";

export type CanvasView = { x: number; y: number; scale: number };
export type CropDragAction = "move" | "nw" | "ne" | "sw" | "se";
export type PaneState = { library: number; source: number; inspector: number };
export type PaneDragKind = "library-source" | "source-sheet" | "sheet-inspector";

export function anchoredZoom(
  current: CanvasView,
  anchor: { x: number; y: number },
  deltaY: number,
  limits = { min: 0.25, max: 5 },
): CanvasView {
  const nextScale = clamp(current.scale * (deltaY < 0 ? 1.12 : 0.88), limits.min, limits.max);
  const ratio = nextScale / current.scale;
  return {
    scale: nextScale,
    x: anchor.x - (anchor.x - current.x) * ratio,
    y: anchor.y - (anchor.y - current.y) * ratio,
  };
}

export function adjustCrop(
  bounds: NormalizedBounds,
  action: CropDragAction,
  dx: number,
  dy: number,
): NormalizedBounds {
  const minSize = 0.025;
  if (action === "move") {
    return {
      ...bounds,
      x: clamp(bounds.x + dx, 0, 1 - bounds.width),
      y: clamp(bounds.y + dy, 0, 1 - bounds.height),
    };
  }
  if (action === "nw" || action === "ne" || action === "sw") {
    const fromLeft = action === "nw" || action === "sw";
    const fromTop = action === "nw" || action === "ne";
    const nextX = clamp(bounds.x + dx, 0, bounds.x + bounds.width - minSize);
    const nextY = clamp(bounds.y + dy, 0, bounds.y + bounds.height - minSize);
    return {
      x: fromLeft ? nextX : bounds.x,
      y: fromTop ? nextY : bounds.y,
      width: fromLeft ? bounds.width + bounds.x - nextX : clamp(bounds.width + dx, minSize, 1 - bounds.x),
      height: fromTop ? bounds.height + bounds.y - nextY : clamp(bounds.height + dy, minSize, 1 - bounds.y),
    };
  }
  const right = clamp(bounds.x + bounds.width + dx, bounds.x + minSize, 1);
  const bottom = clamp(bounds.y + bounds.height + dy, bounds.y + minSize, 1);
  return {
    ...bounds,
    width: right - bounds.x,
    height: bottom - bounds.y,
  };
}

export function resizePanes(kind: PaneDragKind, start: PaneState, pointerX: number, left: number, width: number): PaneState {
  const boundary = pointerX - left;
  if (kind === "library-source") return { ...start, library: clamp(boundary, 160, 420) };
  if (kind === "source-sheet") return { ...start, source: clamp(boundary - start.library - 6, 280, Math.max(280, width - start.library - start.inspector - 332)) };
  return { ...start, inspector: clamp(width - boundary, 230, 420) };
}

export function fitView(container: { width: number; height: number }, content: { width: number; height: number }, padding = 24): CanvasView {
  const availableWidth = Math.max(1, container.width - padding * 2);
  const availableHeight = Math.max(1, container.height - padding * 2);
  const scale = Math.min(availableWidth / content.width, availableHeight / content.height);
  return {
    scale,
    x: (container.width - content.width * scale) / 2,
    y: (container.height - content.height * scale) / 2,
  };
}

export function clamp01(value: number): number {
  return clamp(value, 0, 1);
}

export function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}
