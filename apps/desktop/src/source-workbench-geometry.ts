import type { NormalizedBounds, PatchGeometry } from "@hot-trimmer/ipc-contracts";

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

export function patchBounds(corners: PatchGeometry["corners"]) {
  const xs = corners.map((corner) => corner.x);
  const ys = corners.map((corner) => corner.y);
  return { left: Math.min(...xs), right: Math.max(...xs), top: Math.min(...ys), bottom: Math.max(...ys) };
}

export function movePatch(corners: PatchGeometry["corners"], dx: number, dy: number): PatchGeometry["corners"] {
  const bounds = patchBounds(corners);
  const safeDx = Math.max(-bounds.left, Math.min(1 - bounds.right, dx));
  const safeDy = Math.max(-bounds.top, Math.min(1 - bounds.bottom, dy));
  return corners.map((corner) => ({ x: corner.x + safeDx, y: corner.y + safeDy })) as unknown as PatchGeometry["corners"];
}

export function normalizePatchToRectangle(
  corners: PatchGeometry["corners"],
  size: { width: number; height: number },
): PatchGeometry["corners"] {
  const points = corners.map((corner) => ({ x: corner.x * size.width, y: corner.y * size.height }));
  const edge = (from: typeof points[number], to: typeof points[number]) => ({ x: to.x - from.x, y: to.y - from.y });
  const length = (value: { x: number; y: number }) => Math.hypot(value.x, value.y);
  const unit = (value: { x: number; y: number }) => {
    const magnitude = length(value);
    return magnitude > 0.000001 ? { x: value.x / magnitude, y: value.y / magnitude } : null;
  };
  const top = edge(points[0]!, points[1]!);
  const bottom = edge(points[3]!, points[2]!);
  const left = edge(points[0]!, points[3]!);
  const right = edge(points[1]!, points[2]!);
  const topUnit = unit(top);
  const bottomUnit = unit(bottom);
  if (!topUnit || !bottomUnit) return corners;
  const horizontal = unit({ x: topUnit.x + bottomUnit.x, y: topUnit.y + bottomUnit.y }) ?? topUnit;
  let vertical = { x: -horizontal.y, y: horizontal.x };
  const sideDirection = { x: left.x + right.x, y: left.y + right.y };
  if (vertical.x * sideDirection.x + vertical.y * sideDirection.y < 0) vertical = { x: -vertical.x, y: -vertical.y };
  const width = (length(top) + length(bottom)) * 0.5;
  const height = (length(left) + length(right)) * 0.5;
  if (width < 1 || height < 1) return corners;
  const center = points.reduce((sum, point) => ({ x: sum.x + point.x / 4, y: sum.y + point.y / 4 }), { x: 0, y: 0 });
  const halfX = { x: horizontal.x * width * 0.5, y: horizontal.y * width * 0.5 };
  const halfY = { x: vertical.x * height * 0.5, y: vertical.y * height * 0.5 };
  const normalized = [
    { x: center.x - halfX.x - halfY.x, y: center.y - halfX.y - halfY.y },
    { x: center.x + halfX.x - halfY.x, y: center.y + halfX.y - halfY.y },
    { x: center.x + halfX.x + halfY.x, y: center.y + halfX.y + halfY.y },
    { x: center.x - halfX.x + halfY.x, y: center.y - halfX.y + halfY.y },
  ].map((point) => ({ x: point.x / size.width, y: point.y / size.height }));
  return normalized.some((corner) => corner.x < 0 || corner.x > 1 || corner.y < 0 || corner.y > 1)
    ? corners
    : normalized as unknown as PatchGeometry["corners"];
}

export type PatchResizeHandle = "nw" | "n" | "ne" | "e" | "se" | "s" | "sw" | "w";

export function resizePatch(
  corners: PatchGeometry["corners"],
  requestedHandle: PatchResizeHandle | number,
  pointer: { x: number; y: number },
  modifiers: { proportional?: boolean; fromCenter?: boolean } = {},
): PatchGeometry["corners"] {
  const handle = typeof requestedHandle === "number"
    ? (["nw", "ne", "se", "sw"] as const)[requestedHandle]!
    : requestedHandle;
  const bounds = patchBounds(corners);
  const center = { x: (bounds.left + bounds.right) * 0.5, y: (bounds.top + bounds.bottom) * 0.5 };
  const west = handle.includes("w");
  const east = handle.includes("e");
  const north = handle.includes("n");
  const south = handle.includes("s");
  const horizontal = west || east;
  const vertical = north || south;
  const movingX = west ? bounds.left : bounds.right;
  const movingY = north ? bounds.top : bounds.bottom;
  const anchorX = modifiers.fromCenter ? center.x : west ? bounds.right : bounds.left;
  const anchorY = modifiers.fromCenter ? center.y : north ? bounds.bottom : bounds.top;
  const minimumScale = 0.01;
  let scaleX = horizontal ? Math.max(minimumScale, (pointer.x - anchorX) / (movingX - anchorX)) : 1;
  let scaleY = vertical ? Math.max(minimumScale, (pointer.y - anchorY) / (movingY - anchorY)) : 1;

  if (modifiers.proportional) {
    const uniform = horizontal && vertical
      ? (Math.abs(scaleX - 1) >= Math.abs(scaleY - 1) ? scaleX : scaleY)
      : horizontal ? scaleX : scaleY;
    scaleX = uniform;
    scaleY = uniform;
  }

  const transformAnchor = {
    x: horizontal && !modifiers.fromCenter ? anchorX : center.x,
    y: vertical && !modifiers.fromCenter ? anchorY : center.y,
  };
  const resized = corners.map((corner) => ({
    x: transformAnchor.x + (corner.x - transformAnchor.x) * scaleX,
    y: transformAnchor.y + (corner.y - transformAnchor.y) * scaleY,
  }));
  return resized.some((corner) => corner.x < 0 || corner.x > 1 || corner.y < 0 || corner.y > 1)
    ? corners
    : resized as unknown as PatchGeometry["corners"];
}

export function patchPointerAngle(point: { x: number; y: number }, center: { x: number; y: number }, size: { width: number; height: number }) {
  return Math.atan2((point.y - center.y) * size.height, (point.x - center.x) * size.width);
}

export function rotatePatch(
  corners: PatchGeometry["corners"],
  center: { x: number; y: number },
  angle: number,
  size: { width: number; height: number },
): PatchGeometry["corners"] {
  const sine = Math.sin(angle);
  const cosine = Math.cos(angle);
  const rotated = corners.map((corner) => {
    const x = (corner.x - center.x) * size.width;
    const y = (corner.y - center.y) * size.height;
    return {
      x: center.x + (x * cosine - y * sine) / size.width,
      y: center.y + (x * sine + y * cosine) / size.height,
    };
  });
  return rotated.some((corner) => corner.x < 0 || corner.x > 1 || corner.y < 0 || corner.y > 1)
    ? corners
    : rotated as unknown as PatchGeometry["corners"];
}

export function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}
