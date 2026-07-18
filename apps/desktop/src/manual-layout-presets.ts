import type { AuthoredLayoutPreset, AuthoredLayoutPresetRegion, TrimSheetDocument } from "@hot-trimmer/ipc-contracts";

export const authoredGridResolutions = [16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256] as const;
export const DIAGONAL_CASCADE_PRESET_ID = "builtin.diagonal-cascade";
export const NEW_BLANK_PRESET_ID = "builtin.new-blank";

const diagonalRects = [
  [0,0,16,32],[16,0,16,16],[32,0,32,16],[16,16,16,16],[32,16,16,16],[48,16,16,16],
  [0,32,16,8],[16,32,8,16],[24,32,1,24],[25,32,1,24],[26,32,2,24],[28,32,4,24],
  [32,32,32,32],[0,40,8,8],[8,40,8,8],[0,48,8,8],[8,48,8,4],[16,48,4,8],
  [20,48,4,8],[8,52,8,4],[0,56,32,1],[0,57,32,1],[0,58,32,2],[0,60,32,4],
] as const;

function record(key: string, displayName: string, x: number, y: number, width: number, height: number): AuthoredLayoutPresetRegion {
  return {
    presetRegionKey: key, displayName, gridRect: { x, y, width, height }, role: "planar",
    orientation: width > height ? "horizontal" : height > width ? "vertical" : "unspecified",
    uvFit: { kind: "rectangular", fitAxis: "automatic", keepProportion: true, allowedRotations: ["zero"], mirrorAllowed: false, worldSizeMeters: [width, height], classificationTags: ["AUTHORED_LAYOUT"] },
    structuralProfile: "flat",
  };
}

export const diagonalCascadePreset: AuthoredLayoutPreset = {
  presetId: DIAGONAL_CASCADE_PRESET_ID, schemaVersion: 1, name: "Diagonal Cascade",
  logicalGrid: { schemaVersion: 1, width: 64, height: 64 }, canonicalAspect: [1, 1],
  regions: diagonalRects.map(([x,y,width,height], index) => record(`cascade-${String(index).padStart(2, "0")}`, `Region ${String(index + 1).padStart(3, "0")}`, x, y, width, height)),
  provenance: "checked_in_authored_fixture",
};

export function newBlankPreset(size = 64): AuthoredLayoutPreset {
  return { presetId: NEW_BLANK_PRESET_ID, schemaVersion: 1, name: "New Blank", logicalGrid: { schemaVersion: 1, width: size, height: size }, canonicalAspect: [1, 1], regions: [record("remainder", "Remainder", 0, 0, size, size)], provenance: "built_in_blank" };
}

export function snapshotDocumentPreset(document: TrimSheetDocument, presetId: string, name: string): AuthoredLayoutPreset {
  const grid = document.logicalGrid ?? { schemaVersion: 1, width: 64, height: 64 };
  // Topology commands sort regions spatially, so array position cannot carry a preset-local
  // identity. Exact unchanged rectangles retain their prior key; edited/new rectangles receive
  // a project-region-derived key which remains stable across subsequent Save operations.
  const priorKeys = new Map(document.authoredLayoutPreset?.regions.map((region) => [rectKey(region.gridRect), region.presetRegionKey]));
  return {
    presetId, schemaVersion: 1, name, logicalGrid: grid, canonicalAspect: [document.renderSettings.outputSize.width, document.renderSettings.outputSize.height],
    regions: document.topology.regions.flatMap((region, index) => region.gridRect ? [record(priorKeys.get(rectKey(region.gridRect)) ?? `authored-${region.id}`, region.displayName || `Region ${index + 1}`, region.gridRect.x, region.gridRect.y, region.gridRect.width, region.gridRect.height)] : []),
    provenance: "user_authored_snapshot",
  };
}

export function rescalePreset(preset: AuthoredLayoutPreset, size: number): { preset: AuthoredLayoutPreset; exact: boolean } {
  const oldWidth = preset.logicalGrid.width;
  const oldHeight = preset.logicalGrid.height;
  const x = quantizedAxis(preset.regions.flatMap((region) => [region.gridRect.x, region.gridRect.x + region.gridRect.width]), oldWidth, size);
  const y = quantizedAxis(preset.regions.flatMap((region) => [region.gridRect.y, region.gridRect.y + region.gridRect.height]), oldHeight, size);
  const regions = preset.regions.map((region) => {
    const left = x.positions.get(region.gridRect.x)!;
    const right = x.positions.get(region.gridRect.x + region.gridRect.width)!;
    const top = y.positions.get(region.gridRect.y)!;
    const bottom = y.positions.get(region.gridRect.y + region.gridRect.height)!;
    return { ...region, gridRect: { x: left, y: top, width: right - left, height: bottom - top } };
  });
  const result = { ...preset, logicalGrid: { schemaVersion: 1, width: size, height: size }, regions };
  if (!presetExactlyCoversGrid(result)) throw new Error("Quantized layout does not exactly cover the requested grid.");
  return { exact: x.exact && y.exact, preset: result };
}

export function presetExactlyCoversGrid(preset: AuthoredLayoutPreset): boolean {
  const { width, height } = preset.logicalGrid;
  if (width <= 0 || height <= 0) return false;
  const owners = new Uint8Array(width * height);
  for (const region of preset.regions) {
    const rect = region.gridRect;
    if (rect.width <= 0 || rect.height <= 0 || rect.x < 0 || rect.y < 0 || rect.x + rect.width > width || rect.y + rect.height > height) return false;
    for (let y = rect.y; y < rect.y + rect.height; y += 1) for (let x = rect.x; x < rect.x + rect.width; x += 1) {
      const index = y * width + x;
      owners[index] += 1;
      if (owners[index] !== 1) return false;
    }
  }
  return owners.every((owner) => owner === 1);
}

function quantizedAxis(boundaries: readonly number[], oldSize: number, newSize: number) {
  const ordered = [...new Set([0, oldSize, ...boundaries])].sort((left, right) => left - right);
  if (ordered.length > newSize + 1) throw new Error(`A ${newSize}-cell grid cannot preserve ${ordered.length - 1} authored intervals.`);
  const mapped = ordered.map((boundary, index) => index === 0 ? 0 : index === ordered.length - 1 ? newSize : Math.round(boundary * newSize / oldSize));
  // Keep the authored endpoints pinned. Only interior boundaries move while the
  // two passes guarantee at least one target cell for every authored interval.
  for (let index = 1; index < mapped.length - 1; index += 1) mapped[index] = Math.max(mapped[index]!, mapped[index - 1]! + 1);
  for (let index = mapped.length - 2; index > 0; index -= 1) mapped[index] = Math.min(mapped[index]!, mapped[index + 1]! - 1);
  const positions = new Map(ordered.map((boundary, index) => [boundary, mapped[index]!]));
  return { positions, exact: ordered.every((boundary) => boundary * newSize % oldSize === 0) };
}

function rectKey(rect: { x: number; y: number; width: number; height: number }) {
  return `${rect.x}:${rect.y}:${rect.width}:${rect.height}`;
}

export function snappedGridPoint(clientX: number, clientY: number, rect: Pick<DOMRect, "left"|"top"|"width"|"height">, width: number, height: number) {
  const gridX = Math.max(0, Math.min(width, (clientX - rect.left) / rect.width * width));
  const gridY = Math.max(0, Math.min(height, (clientY - rect.top) / rect.height * height));
  const cellX = Math.min(width - 1, Math.max(0, Math.floor(gridX)));
  const cellY = Math.min(height - 1, Math.max(0, Math.floor(gridY)));
  return { x: Math.round(gridX), y: Math.round(gridY), cellX, cellY, centerX: cellX + 0.5, centerY: cellY + 0.5 };
}

export function cellDragRect(startCellX: number, startCellY: number, endCellX: number, endCellY: number) {
  return {
    x: Math.min(startCellX, endCellX), y: Math.min(startCellY, endCellY),
    width: Math.abs(endCellX - startCellX) + 1, height: Math.abs(endCellY - startCellY) + 1,
  };
}

export function sourceFrameGridBounds(
  frame: { x: number; y: number; width: number; height: number },
  grid: { width: number; height: number },
  cell: { x: number; y: number; width: number; height: number },
) {
  return {
    x: frame.x + cell.x / grid.width * frame.width,
    y: frame.y + cell.y / grid.height * frame.height,
    width: cell.width / grid.width * frame.width,
    height: cell.height / grid.height * frame.height,
  };
}
