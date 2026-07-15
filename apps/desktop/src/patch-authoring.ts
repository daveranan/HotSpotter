import type { NormalizedPoint, PatchGeometry } from "@hot-trimmer/ipc-contracts";

const MIN_AREA = 1e-6;

export function exceedsDragThreshold(
  start: { x: number; y: number },
  current: { x: number; y: number },
  threshold = 4,
): boolean {
  return Math.hypot(current.x - start.x, current.y - start.y) >= threshold;
}

export type DraftEscapeAction = "finish" | "cancel";

export function normalizedFromRect(
  clientX: number,
  clientY: number,
  rect: Pick<DOMRect, "left" | "top" | "width" | "height">,
): NormalizedPoint | null {
  if (rect.width <= 0 || rect.height <= 0) return null;
  const point = { x: (clientX - rect.left) / rect.width, y: (clientY - rect.top) / rect.height };
  return point.x >= 0 && point.x <= 1 && point.y >= 0 && point.y <= 1 ? point : null;
}

export function zoomViewAtPoint(
  view: { x: number; y: number; scale: number },
  nextScale: number,
  cursor: { x: number; y: number },
  renderedRect: { left: number; top: number; width: number; height: number },
): { x: number; y: number; scale: number } {
  if (view.scale <= 0 || renderedRect.width <= 0 || renderedRect.height <= 0) {
    return { ...view, scale: nextScale };
  }
  const ratio = nextScale / view.scale;
  const centerX = renderedRect.left + renderedRect.width / 2;
  const centerY = renderedRect.top + renderedRect.height / 2;
  return {
    x: view.x + (cursor.x - centerX) * (1 - ratio),
    y: view.y + (cursor.y - centerY) * (1 - ratio),
    scale: nextScale,
  };
}

export function rectangleGeometry(start: NormalizedPoint, end: NormalizedPoint): PatchGeometry {
  const left = Math.min(start.x, end.x);
  const right = Math.max(start.x, end.x);
  const top = Math.min(start.y, end.y);
  const bottom = Math.max(start.y, end.y);
  return { corners: [
    { x: left, y: top },
    { x: right, y: top },
    { x: right, y: bottom },
    { x: left, y: bottom },
  ] };
}

export function validatePatchGeometry(geometry: PatchGeometry): string | null {
  const points = geometry.corners;
  if (points.some((point) => !Number.isFinite(point.x) || !Number.isFinite(point.y)
    || point.x < 0 || point.x > 1 || point.y < 0 || point.y > 1)) {
    return "Move every corner inside the source image.";
  }
  const area = points.reduce((sum, point, index) => {
    const next = points[(index + 1) % points.length]!;
    return sum + point.x * next.y - next.x * point.y;
  }, 0) / 2;
  if (Math.abs(area) < MIN_AREA) return "Move the corners apart to enclose a visible area.";
  if (area < 0) return "Move the points until the patch boundary no longer crosses or folds over itself.";
  let sign = 0;
  for (let index = 0; index < 4; index += 1) {
    const origin = points[index]!;
    const first = points[(index + 1) % 4]!;
    const second = points[(index + 2) % 4]!;
    const cross = (first.x - origin.x) * (second.y - origin.y)
      - (first.y - origin.y) * (second.x - origin.x);
    if (Math.abs(cross) <= Number.EPSILON) return "Move the corners apart to avoid a flat edge.";
    if (sign === 0) sign = Math.sign(cross);
    else if (Math.sign(cross) !== sign) return "Move each corner to the outside boundary of the patch.";
  }
  return null;
}

export function escapeAction(points: NormalizedPoint[]): DraftEscapeAction {
  if (points.length !== 4) return "cancel";
  const geometry: PatchGeometry = { corners: points as PatchGeometry["corners"] };
  return validatePatchGeometry(geometry) === null ? "finish" : "cancel";
}

export function moveCorner(
  geometry: PatchGeometry,
  cornerIndex: number,
  point: NormalizedPoint,
): PatchGeometry {
  const corners = geometry.corners.map((corner) => ({ ...corner })) as PatchGeometry["corners"];
  corners[cornerIndex] = {
    x: Math.min(1, Math.max(0, point.x)),
    y: Math.min(1, Math.max(0, point.y)),
  };
  return { ...geometry, corners };
}

export function translateGeometry(
  geometry: PatchGeometry,
  deltaX: number,
  deltaY: number,
): PatchGeometry {
  const minX = Math.min(...geometry.corners.map((point) => point.x));
  const maxX = Math.max(...geometry.corners.map((point) => point.x));
  const minY = Math.min(...geometry.corners.map((point) => point.y));
  const maxY = Math.max(...geometry.corners.map((point) => point.y));
  const boundedX = Math.min(1 - maxX, Math.max(-minX, deltaX));
  const boundedY = Math.min(1 - maxY, Math.max(-minY, deltaY));
  return {
    ...geometry,
    corners: geometry.corners.map((point) => ({
      x: point.x + boundedX,
      y: point.y + boundedY,
    })) as PatchGeometry["corners"],
  };
}

export interface GeometryBounds {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export interface QuadProjection {
  a: number; b: number; c: number;
  d: number; e: number; f: number;
  g: number; h: number;
}

export function quadProjection(geometry: PatchGeometry): QuadProjection {
  const [topLeft, topRight, bottomRight, bottomLeft] = geometry.corners;
  const dx1 = topRight.x - bottomRight.x;
  const dx2 = bottomLeft.x - bottomRight.x;
  const dy1 = topRight.y - bottomRight.y;
  const dy2 = bottomLeft.y - bottomRight.y;
  const sx = topLeft.x - topRight.x + bottomRight.x - bottomLeft.x;
  const sy = topLeft.y - topRight.y + bottomRight.y - bottomLeft.y;
  const denominator = dx1 * dy2 - dx2 * dy1;
  const g = Math.abs(denominator) < Number.EPSILON ? 0 : (sx * dy2 - dx2 * sy) / denominator;
  const h = Math.abs(denominator) < Number.EPSILON ? 0 : (dx1 * sy - sx * dy1) / denominator;
  return {
    a: topRight.x - topLeft.x + g * topRight.x,
    b: bottomLeft.x - topLeft.x + h * bottomLeft.x,
    c: topLeft.x,
    d: topRight.y - topLeft.y + g * topRight.y,
    e: bottomLeft.y - topLeft.y + h * bottomLeft.y,
    f: topLeft.y,
    g,
    h,
  };
}

export function geometryBounds(geometry: PatchGeometry): GeometryBounds {
  return {
    left: Math.min(...geometry.corners.map((point) => point.x)),
    top: Math.min(...geometry.corners.map((point) => point.y)),
    right: Math.max(...geometry.corners.map((point) => point.x)),
    bottom: Math.max(...geometry.corners.map((point) => point.y)),
  };
}

export function canonicalizeFourPoints(points: NormalizedPoint[]): PatchGeometry | null {
  if (points.length !== 4) return null;
  const center = {
    x: points.reduce((sum, point) => sum + point.x, 0) / 4,
    y: points.reduce((sum, point) => sum + point.y, 0) / 4,
  };
  const ordered = [...points].sort((first, second) => (
    Math.atan2(first.y - center.y, first.x - center.x)
    - Math.atan2(second.y - center.y, second.x - center.x)
  ));
  const first = ordered.reduce((best, point, index) => (
    point.x + point.y < ordered[best]!.x + ordered[best]!.y ? index : best
  ), 0);
  const corners = [...ordered.slice(first), ...ordered.slice(0, first)] as PatchGeometry["corners"];
  const geometry = { corners };
  return validatePatchGeometry(geometry) === null ? geometry : null;
}

export function scaleGeometryFromHandle(
  geometry: PatchGeometry,
  handle: 0 | 1 | 2 | 3,
  point: NormalizedPoint,
  preserveAspect = false,
): PatchGeometry {
  const bounds = geometryBounds(geometry);
  const anchors = [
    { x: bounds.right, y: bounds.bottom },
    { x: bounds.left, y: bounds.bottom },
    { x: bounds.left, y: bounds.top },
    { x: bounds.right, y: bounds.top },
  ] as const;
  const anchor = anchors[handle];
  const oldWidth = Math.max(Number.EPSILON, bounds.right - bounds.left);
  const oldHeight = Math.max(Number.EPSILON, bounds.bottom - bounds.top);
  let width = Math.abs(point.x - anchor.x);
  let height = Math.abs(point.y - anchor.y);
  if (preserveAspect) {
    const scaleX = width / oldWidth;
    const scaleY = height / oldHeight;
    const scale = Math.abs(scaleX - 1) >= Math.abs(scaleY - 1) ? scaleX : scaleY;
    width = oldWidth * scale;
    height = oldHeight * scale;
  }
  const growsLeft = handle === 0 || handle === 3;
  const growsUp = handle === 0 || handle === 1;
  const left = Math.max(0, growsLeft ? anchor.x - width : anchor.x);
  const right = Math.min(1, growsLeft ? anchor.x : anchor.x + width);
  const top = Math.max(0, growsUp ? anchor.y - height : anchor.y);
  const bottom = Math.min(1, growsUp ? anchor.y : anchor.y + height);
  return {
    ...geometry,
    corners: geometry.corners.map((corner) => ({
      x: left + ((corner.x - bounds.left) / oldWidth) * (right - left),
      y: top + ((corner.y - bounds.top) / oldHeight) * (bottom - top),
    })) as PatchGeometry["corners"],
  };
}

function clampAxisScale(scale: number, anchor: number, deltas: number[]): number {
  let maximum = Number.POSITIVE_INFINITY;
  for (const delta of deltas) {
    if (delta > Number.EPSILON) maximum = Math.min(maximum, (1 - anchor) / delta);
    else if (delta < -Number.EPSILON) maximum = Math.min(maximum, -anchor / delta);
  }
  return Math.min(maximum, Math.max(0.01, Number.isFinite(scale) ? scale : 1));
}

export function scaleGeometryFromCorner(
  geometry: PatchGeometry,
  handle: 0 | 1 | 2 | 3,
  point: NormalizedPoint,
  preserveAspect = false,
): PatchGeometry {
  const anchor = geometry.corners[(handle + 2) % 4]!;
  const original = geometry.corners[handle]!;
  const deltasX = geometry.corners.map((corner) => corner.x - anchor.x);
  const deltasY = geometry.corners.map((corner) => corner.y - anchor.y);
  const rawX = Math.abs(original.x - anchor.x) <= Number.EPSILON
    ? 1
    : (point.x - anchor.x) / (original.x - anchor.x);
  const rawY = Math.abs(original.y - anchor.y) <= Number.EPSILON
    ? 1
    : (point.y - anchor.y) / (original.y - anchor.y);
  let scaleX = clampAxisScale(rawX, anchor.x, deltasX);
  let scaleY = clampAxisScale(rawY, anchor.y, deltasY);
  if (preserveAspect) {
    const requested = Math.abs(rawX - 1) >= Math.abs(rawY - 1) ? rawX : rawY;
    const uniform = Math.min(
      clampAxisScale(requested, anchor.x, deltasX),
      clampAxisScale(requested, anchor.y, deltasY),
    );
    scaleX = uniform;
    scaleY = uniform;
  }
  return {
    ...geometry,
    corners: geometry.corners.map((corner) => ({
      x: anchor.x + (corner.x - anchor.x) * scaleX,
      y: anchor.y + (corner.y - anchor.y) * scaleY,
    })) as PatchGeometry["corners"],
  };
}

export function rotateGeometry(
  geometry: PatchGeometry,
  center: NormalizedPoint,
  radians: number,
  imageAspect = 1,
): PatchGeometry {
  const cosine = Math.cos(radians);
  const sine = Math.sin(radians);
  return {
    ...geometry,
    corners: geometry.corners.map((corner) => {
      const x = (corner.x - center.x) * imageAspect;
      const y = corner.y - center.y;
      return {
        x: center.x + (x * cosine - y * sine) / imageAspect,
        y: center.y + x * sine + y * cosine,
      };
    }) as PatchGeometry["corners"],
  };
}

export function polygonPoints(geometry: PatchGeometry): string {
  return geometry.corners.map((point) => `${point.x * 100},${point.y * 100}`).join(" ");
}
