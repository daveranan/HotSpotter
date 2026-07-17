import type {
  CompiledLayoutPreview,
  CompiledLayoutPreviewMap,
  FillBehavior,
  GenerateLayoutRequest,
  LayoutItem,
  LayoutOrder,
  LayoutPreset,
  LayoutRegion,
  LayoutRequest,
  LayoutSettings,
  NormalizedPoint,
  PackPriority,
  PatchGeometry,
  PatchSnapshot,
  PixelBounds,
  ProjectSnapshot,
  RegionSourceLayer,
  SourceFramingMode,
  SourceSnapshot,
  TemplateIdentity,
  TemplateSourceTransform,
} from "@hot-trimmer/ipc-contracts";
import { IPC_PROTOCOL_VERSION } from "@hot-trimmer/ipc-contracts";
import type { CSSProperties } from "react";

export interface LayoutDraftOptions {
  layoutId: string;
  preset: LayoutPreset;
  settings: LayoutSettings;
  selectedSourceSetIds: readonly string[];
  includePatches: boolean;
  items?: readonly LayoutItem[];
  existingRegions?: readonly LayoutRegion[];
}

export interface LayoutDrag {
  regionId: string;
  kind: "move" | "north" | "east" | "south" | "west" | "north-east" | "south-east" | "south-west" | "north-west";
  pointerId: number;
  start: { x: number; y: number };
  original: PixelBounds;
  preview: PixelBounds;
  coalescingGroup: number;
}

export interface LayoutAsyncState<T> {
  value: T;
  failure: { message: string; recovery: string } | null;
  busy: boolean;
  generation: number;
}

export interface LayoutPresentationState<T> {
  sourceSetId: string | null;
  layout: T;
}

const presetSettings: Record<LayoutPreset, { order: LayoutOrder; priority: PackPriority }> = {
  balanced: { order: "largest_first", priority: "balanced" },
  horizontal_trims: { order: "horizontal_first", priority: "horizontal_strips" },
  vertical_trims: { order: "vertical_first", priority: "vertical_strips" },
  modular_kit: { order: "largest_first", priority: "balanced" },
  atlas: { order: "input", priority: "balanced" },
};

export const layoutPresetDescriptions: Record<LayoutPreset, string> = {
  balanced: "A general-purpose mix of strips and unique details.",
  horizontal_trims: "Favors long horizontal bands for trim workflows.",
  vertical_trims: "Favors tall vertical bands and columns.",
  modular_kit: "Packs reusable architectural pieces by size.",
  atlas: "Keeps input order for a predictable hotspot atlas.",
};

export const genericArchitectureTemplate = {
  templateId: "ht.generic_architecture",
  templateVersion: "1.0.0",
  compatibilityKey: "ht.generic_architecture.topology.v1",
} as const;

/** Built-in template choices exposed by the trim-sheet workbench. */
export const templateOptions: ReadonlyArray<{ identity: TemplateIdentity; label: string; description: string }> = [
  { identity: genericArchitectureTemplate, label: "Generic Architecture", description: "Broad hotspot field with progressive architectural bands." },
  { identity: { templateId: "ht.horizontal_moulding", templateVersion: "1.0.0", compatibilityKey: "ht.horizontal_moulding.topology.v1" }, label: "Horizontal Moulding", description: "Long horizontal moulding strips and repeatable caps." },
  { identity: { templateId: "ht.vertical_trim", templateVersion: "1.0.0", compatibilityKey: "ht.vertical_trim.topology.v1" }, label: "Vertical Trim", description: "Tall jambs, stiles, reveals, and an end cap." },
  { identity: { templateId: "ht.wood_board_moulding", templateVersion: "1.0.0", compatibilityKey: "ht.wood_board_moulding.topology.v1" }, label: "Wood Board", description: "Board widths, grooves, and radial end-grain slots." },
  { identity: { templateId: "ht.detail_ribbon_microtrim", templateVersion: "1.0.0", compatibilityKey: "ht.detail_ribbon_microtrim.topology.v1" }, label: "Detail Ribbon", description: "Compact micro-trim and detail-ribbon topology." },
  { identity: { templateId: "ht.hard_surface_panel", templateVersion: "1.0.0", compatibilityKey: "ht.hard_surface_panel.topology.v1" }, label: "Hard Surface Panels", description: "Fixed panels, access details, and authored edge banks." },
  { identity: { templateId: "ht.detail_heavy_props", templateVersion: "1.0.0", compatibilityKey: "ht.detail_heavy_props.topology.v1" }, label: "Detail-heavy Props", description: "Pinned unique-detail cells for prop vocabulary." },
  { identity: { templateId: "ht.radial_accents", templateVersion: "1.0.0", compatibilityKey: "ht.radial_accents.topology.v1" }, label: "Radial Accents", description: "Square radial disc and annulus allocations." },
];

export const layoutPreviewMapOptions: ReadonlyArray<{ key: CompiledLayoutPreviewMap; label: string }> = [
  { key: "baseColor", label: "Base Color" },
  { key: "height", label: "Height" },
  { key: "normal", label: "Normal" },
  { key: "roughness", label: "Roughness" },
  { key: "metallic", label: "Metallic" },
  { key: "ambientOcclusion", label: "AO/Cavity" },
  { key: "regionId", label: "Region ID" },
  { key: "materialId", label: "Material ID" },
];

export function availableLayoutPreviewMaps(preview: CompiledLayoutPreview | null): CompiledLayoutPreviewMap[] {
  return layoutPreviewMapOptions.map((option) => option.key).filter((key) => key === "baseColor" || Boolean(preview?.maps?.[key]));
}

export function layoutPreviewDataUrl(preview: CompiledLayoutPreview | null, map: CompiledLayoutPreviewMap): string | undefined {
  return preview?.maps?.[map] ?? preview?.dataUrl;
}

export const defaultTemplateSourceTransform: TemplateSourceTransform = {
  mode: "cover",
  cropFocus: { x: 0.5, y: 0.5 },
  cropBounds: { x: 0, y: 0, width: 1, height: 1 },
};

export function defaultRegionSourceLayer(): RegionSourceLayer {
  return {
    version: 1,
    mapping: { type: "whole_source" },
    rectification: { mode: "none", maxIntermediateEdge: 2048 },
    sampling: { mode: "linear", scale: 1 },
    rotationDegrees: 0,
    mirrorX: false,
    mirrorY: false,
    blend: "replace",
    opacity: 1,
    variationOffset: [0, 0],
    warps: [],
  };
}

/** The editable source quadrilateral is independent from the trim-sheet region bounds. */
export function sourceLayerGeometry(sourceLayer: RegionSourceLayer): PatchGeometry {
  if (sourceLayer.mapping.type === "perspective") return { corners: sourceLayer.mapping.quad.map((point) => ({ ...point })) as PatchGeometry["corners"] };
  const bounds = sourceLayer.mapping.type === "bounds" ? sourceLayer.mapping.bounds : { x: 0, y: 0, width: 1, height: 1 };
  return { corners: [
    { x: bounds.x, y: bounds.y },
    { x: bounds.x + bounds.width, y: bounds.y },
    { x: bounds.x + bounds.width, y: bounds.y + bounds.height },
    { x: bounds.x, y: bounds.y + bounds.height },
  ] };
}

export function sourceLayerWithGeometry(sourceLayer: RegionSourceLayer, geometry: PatchGeometry): RegionSourceLayer {
  const [topLeft, topRight, bottomRight, bottomLeft] = geometry.corners;
  const epsilon = 1e-9;
  const rectangular = Math.abs(topLeft.y - topRight.y) < epsilon
    && Math.abs(bottomLeft.y - bottomRight.y) < epsilon
    && Math.abs(topLeft.x - bottomLeft.x) < epsilon
    && Math.abs(topRight.x - bottomRight.x) < epsilon;
  return {
    ...sourceLayer,
    mapping: rectangular
      ? { type: "bounds", bounds: { x: topLeft.x, y: topLeft.y, width: topRight.x - topLeft.x, height: bottomLeft.y - topLeft.y } }
      : { type: "perspective", quad: geometry.corners.map((point) => ({ ...point })) as [NormalizedPoint, NormalizedPoint, NormalizedPoint, NormalizedPoint] },
  };
}

export interface SourceFootprint {
  bounds: { x: number; y: number; width: number; height: number };
  wrapped: boolean;
}

/**
 * Maps a stable sheet region to the source area currently sampled by template framing.
 * Repeat framing deliberately returns every visible source segment so the authoring view
 * never represents a wrapped sample as one false rectangle.
 */
export function sourceFootprintsForRegion(
  bounds: PixelBounds,
  output: { width: number; height: number },
  framing: TemplateSourceTransform,
  source: { width: number; height: number },
): SourceFootprint[] {
  const crop = framing.cropBounds ?? { x: 0, y: 0, width: 1, height: 1 };
  const left = bounds.x / Math.max(1, output.width);
  const top = bounds.y / Math.max(1, output.height);
  const right = (bounds.x + bounds.width) / Math.max(1, output.width);
  const bottom = (bounds.y + bounds.height) / Math.max(1, output.height);
  const clamp = (value: number): number => Math.max(0, Math.min(1, value));
  const canonical = (value: number): number => Math.round(value * 1_000_000_000) / 1_000_000_000;
  const canonicalBounds = (value: SourceFootprint["bounds"]): SourceFootprint["bounds"] => ({
    x: canonical(value.x), y: canonical(value.y), width: canonical(value.width), height: canonical(value.height),
  });
  const framed = (u: number, v: number): { x: number; y: number } => {
    if (framing.mode === "stretch") return { x: crop.x + u * crop.width, y: crop.y + v * crop.height };
    const sourceAspect = (source.width * crop.width) / Math.max(1, source.height * crop.height);
    const sheetAspect = output.width / Math.max(1, output.height);
    if (sourceAspect > sheetAspect) {
      const visibleWidth = sheetAspect / sourceAspect;
      const offset = clamp(framing.cropFocus.x) * (1 - visibleWidth);
      return { x: crop.x + (offset + u * visibleWidth) * crop.width, y: crop.y + v * crop.height };
    }
    const visibleHeight = sourceAspect / sheetAspect;
    const offset = clamp(framing.cropFocus.y) * (1 - visibleHeight);
    return { x: crop.x + u * crop.width, y: crop.y + (offset + v * visibleHeight) * crop.height };
  };
  if (framing.mode !== "repeat") {
    const start = framed(left, top);
    const end = framed(right, bottom);
    return [{ bounds: canonicalBounds({ x: start.x, y: start.y, width: Math.max(0.001, end.x - start.x), height: Math.max(0.001, end.y - start.y) }), wrapped: false }];
  }

  const repeat = 2;
  const xStart = left * repeat;
  const xEnd = right * repeat;
  const yStart = top * repeat;
  const yEnd = bottom * repeat;
  const segments = (start: number, end: number): Array<{ start: number; end: number; wrapped: boolean }> => {
    if (end - start >= 1 - Number.EPSILON) return [{ start: 0, end: 1, wrapped: false }];
    const firstTile = Math.floor(start);
    const lastTile = Math.floor(Math.max(start, end - Number.EPSILON));
    const result = [];
    for (let tile = firstTile; tile <= lastTile; tile += 1) {
      result.push({
        start: Math.max(start, tile) - tile,
        end: Math.min(end, tile + 1) - tile,
        wrapped: tile !== firstTile,
      });
    }
    return result;
  };
  const footprints: SourceFootprint[] = [];
  for (const x of segments(xStart, xEnd)) {
    for (const y of segments(yStart, yEnd)) {
      const bounds = canonicalBounds({ x: crop.x + x.start * crop.width, y: crop.y + y.start * crop.height, width: Math.max(0.001, (x.end - x.start) * crop.width), height: Math.max(0.001, (y.end - y.start) * crop.height) });
      footprints.push({ bounds, wrapped: x.wrapped || y.wrapped });
    }
  }
  return footprints;
}

export function templateRegionName(region: LayoutRegion): string {
  const key = region.itemKey.replace(/^(template|slot|region)[:._-]*/i, "");
  if (key && !/^source[:._-]/i.test(key) && !/^item[:._-]?\d*$/i.test(key)) {
    return key.split(/[_:.\-]+/).filter(Boolean).map((word) => `${word.slice(0, 1).toUpperCase()}${word.slice(1)}`).join(" ");
  }
  if (region.bounds.width === region.bounds.height) return "Radial detail";
  return region.bounds.width > region.bounds.height ? "Horizontal trim" : "Vertical trim";
}

export function regionLabelDetail(zoom: number, selected: boolean, hovered: boolean): "hidden" | "compact" | "expanded" {
  if (selected || hovered) return zoom >= 1.35 ? "expanded" : "compact";
  return zoom >= 2.1 ? "compact" : "hidden";
}

export function templateSourceTransform(mode: SourceFramingMode, cropFocus = defaultTemplateSourceTransform.cropFocus, cropBounds = defaultTemplateSourceTransform.cropBounds): TemplateSourceTransform {
  return { mode, cropFocus: { x: cropFocus.x, y: cropFocus.y }, cropBounds: cropBounds ? { ...cropBounds } : undefined };
}

export function buildTemplateGenerateLayoutRequest(sourceSetId: string, layoutId: string, settings: LayoutSettings, sourceTransform: TemplateSourceTransform = defaultTemplateSourceTransform, coalescingGroup?: number, template: TemplateIdentity = genericArchitectureTemplate): GenerateLayoutRequest {
  return {
    protocolVersion: IPC_PROTOCOL_VERSION,
    mode: "template",
    template,
    sourceSetId,
    layoutId,
    settings,
    sourceTransform: templateSourceTransform(sourceTransform.mode, sourceTransform.cropFocus, sourceTransform.cropBounds),
    coalescingGroup,
  };
}

export function buildCustomAtlasGenerateLayoutRequest(request: LayoutRequest, coalescingGroup?: number): GenerateLayoutRequest {
  return { protocolVersion: IPC_PROTOCOL_VERSION, mode: "custom_atlas", request, coalescingGroup };
}
export function defaultLayoutSettings(preset: LayoutPreset = "balanced"): LayoutSettings {
  const intent = presetSettings[preset];
  return {
    output: { width: 2048, height: 2048 },
    paddingPx: 4,
    bleedPx: 8,
    order: intent.order,
    autoPack: { enabled: true, priority: intent.priority, seed: 42 },
  };
}

export function settingsForPreset(settings: LayoutSettings, preset: LayoutPreset): LayoutSettings {
  const intent = presetSettings[preset];
  return {
    ...settings,
    order: intent.order,
    autoPack: { ...settings.autoPack, enabled: true, priority: intent.priority },
  };
}

function patchBehavior(patch: PatchSnapshot): FillBehavior {
  if (patch.properties.trimCap) return "trim_cap";
  switch (patch.properties.repeatMode) {
    case "repeat_x": return "horizontal_loop";
    case "repeat_y": return "vertical_loop";
    case "tile_xy": return "tile";
    case "stretch": return "stretch";
    default: return "unique_detail";
  }
}

function naturalPatchSize(patch: PatchSnapshot, source: SourceSnapshot): { width: number; height: number } {
  const xs = patch.geometry.corners.map((point) => point.x);
  const ys = patch.geometry.corners.map((point) => point.y);
  let width = Math.max(1, Math.round((Math.max(...xs) - Math.min(...xs)) * source.width * patch.rectification.scale));
  let height = Math.max(1, Math.round((Math.max(...ys) - Math.min(...ys)) * source.height * patch.rectification.scale));
  if (patch.rectification.aspectRatio) width = Math.max(1, Math.round(height * patch.rectification.aspectRatio));
  return { width, height };
}

function withoutTemplateBounds(constraints: LayoutItem["constraints"]): LayoutItem["constraints"] {
  const { templateBounds: _templateBounds, ...atlasConstraints } = constraints;
  return atlasConstraints;
}

function patchItem(patch: PatchSnapshot, sourceSetId: string, source: SourceSnapshot, previous?: LayoutItem): LayoutItem {
  const behavior = previous?.behavior ?? patchBehavior(patch);
  const axis = patch.properties.repeatMode === "repeat_y" ? "vertical" : "horizontal";
  const naturalSize = naturalPatchSize(patch, source);
  const capSpan = axis === "horizontal" ? naturalSize.width : naturalSize.height;
  const capSize = Math.min(16, Math.max(0, Math.floor((capSpan - 1) / 2)));
  return {
    key: `patch:${patch.id}`,
    fill: { type: "rectified_patch", sourceSetId, patchId: patch.id },
    behavior,
    trimCaps: previous?.trimCaps ?? (patch.properties.trimCap ? { axis, leadingPx: capSize, trailingPx: capSize } : undefined),
    naturalSize,
    enabled: patch.enabled,
    participates: patch.properties.mapParticipation !== "excluded",
    constraints: withoutTemplateBounds(previous?.constraints ?? {}),
    paddingPx: patch.properties.paddingPx,
    bleedPx: patch.properties.bleedPx,
    regionId: previous?.regionId,
  };
}

export function buildLayoutRequest(project: ProjectSnapshot, options: LayoutDraftOptions): LayoutRequest {
  const selected = new Set(options.selectedSourceSetIds);
  const previousItems = new Map((options.items ?? project.layout?.items ?? []).map((item) => [item.key, item]));
  const items: LayoutItem[] = [];

  if (options.includePatches) for (const patch of project.patches) {
    const patchSource = project.sources.find((candidate) => candidate.id === patch.sourceId);
    if (!patchSource || !selected.has(patchSource.sourceSetId) || !patch.enabled || patch.properties.mapParticipation === "excluded") continue;
    items.push(patchItem(patch, patchSource.sourceSetId, patchSource, previousItems.get(`patch:${patch.id}`)));
  }
  for (const item of previousItems.values()) {
    if (item.fill.type === "simple_color" || item.fill.type === "simple_data") {
      items.push({ ...item, constraints: withoutTemplateBounds(item.constraints) });
    }
  }

  return {
    layoutId: options.layoutId,
    preset: options.preset,
    settings: options.settings,
    items,
    existingRegions: [...(options.existingRegions ?? project.layout?.layout.regions ?? [])],
  };
}

export function withUpdatedItem(items: readonly LayoutItem[], key: string, update: Partial<LayoutItem>): LayoutItem[] {
  return items.map((item) => item.key === key ? { ...item, ...update } : item);
}

export function cssBounds(bounds: PixelBounds, output: { width: number; height: number }): CSSProperties {
  return {
    left: `${bounds.x / output.width * 100}%`,
    top: `${bounds.y / output.height * 100}%`,
    width: `${bounds.width / output.width * 100}%`,
    height: `${bounds.height / output.height * 100}%`,
  };
}

export function sheetPointFromClient(
  client: { x: number; y: number },
  rect: { left: number; top: number; width: number; height: number },
  output: { width: number; height: number },
): { x: number; y: number } {
  return {
    x: (client.x - rect.left) / Math.max(1, rect.width) * output.width,
    y: (client.y - rect.top) / Math.max(1, rect.height) * output.height,
  };
}

export function pixelDeltaAtZoom(delta: number, zoom: number): number {
  return delta / Math.max(0.01, zoom);
}

export function beginLayoutDrag(
  region: LayoutRegion,
  kind: LayoutDrag["kind"],
  pointerId: number,
  start: { x: number; y: number },
  coalescingGroup: number,
): LayoutDrag {
  return { regionId: region.id, kind, pointerId, start, original: { ...region.bounds }, preview: { ...region.bounds }, coalescingGroup };
}

export function clampBoundsToClearance(bounds: PixelBounds, output: { width: number; height: number }, clearance: number): PixelBounds {
  const safeClearance = Math.max(0, Math.min(clearance, Math.floor((Math.min(output.width, output.height) - 1) / 2)));
  const maximumWidth = Math.max(1, output.width - safeClearance * 2);
  const maximumHeight = Math.max(1, output.height - safeClearance * 2);
  const width = Math.max(1, Math.min(bounds.width, maximumWidth));
  const height = Math.max(1, Math.min(bounds.height, maximumHeight));
  return {
    x: Math.max(safeClearance, Math.min(output.width - safeClearance - width, bounds.x)),
    y: Math.max(safeClearance, Math.min(output.height - safeClearance - height, bounds.y)),
    width,
    height,
  };
}

export function updateLayoutDrag(drag: LayoutDrag, point: { x: number; y: number }, output: { width: number; height: number }, locks = { position: false, width: false, height: false }, clearance = 0): LayoutDrag {
  const dx = Math.round(point.x - drag.start.x);
  const dy = Math.round(point.y - drag.start.y);
  let { x, y, width, height } = drag.original;
  if (drag.kind === "move") {
    if (!locks.position) { x += dx; y += dy; }
  } else {
    if (drag.kind.includes("west") && !locks.width && !locks.position) { x += dx; width -= dx; }
    if (drag.kind.includes("east") && !locks.width) width += dx;
    if (drag.kind.includes("north") && !locks.height && !locks.position) { y += dy; height -= dy; }
    if (drag.kind.includes("south") && !locks.height) height += dy;
  }
  return { ...drag, preview: clampBoundsToClearance({ x, y, width, height }, output, clearance) };
}

export function cancelLayoutDrag(drag: LayoutDrag): PixelBounds {
  return { ...drag.original };
}

export function keyboardBounds(bounds: PixelBounds, key: string, modifiers: { shift?: boolean; alt?: boolean }, output: { width: number; height: number }): PixelBounds {
  const amount = modifiers.alt ? 10 : 1;
  let next = { ...bounds };
  if (modifiers.shift) {
    if (key === "ArrowLeft") next.width -= amount;
    if (key === "ArrowRight") next.width += amount;
    if (key === "ArrowUp") next.height -= amount;
    if (key === "ArrowDown") next.height += amount;
  } else {
    if (key === "ArrowLeft") next.x -= amount;
    if (key === "ArrowRight") next.x += amount;
    if (key === "ArrowUp") next.y -= amount;
    if (key === "ArrowDown") next.y += amount;
  }
  next.width = Math.max(1, Math.min(next.width, output.width));
  next.height = Math.max(1, Math.min(next.height, output.height));
  next.x = Math.max(0, Math.min(output.width - next.width, next.x));
  next.y = Math.max(0, Math.min(output.height - next.height, next.y));
  return next;
}

export function regionsOverlap(a: LayoutRegion, b: LayoutRegion): boolean {
  return boundsOverlap(a.bounds, b.bounds);
}

export type LayoutRegionIssue = "content_overlap" | "clearance" | "sheet_edge";

export interface ActiveLayoutRegionBounds {
  regionId: string;
  bounds: PixelBounds;
}

function boundsOverlap(a: PixelBounds, b: PixelBounds): boolean {
  return a.x < b.x + b.width && a.x + a.width > b.x
    && a.y < b.y + b.height && a.y + a.height > b.y;
}

function separatedByClearance(a: PixelBounds, aClearance: number, b: PixelBounds, bClearance: number): boolean {
  const gap = aClearance + bClearance;
  return a.x + a.width + gap <= b.x || b.x + b.width + gap <= a.x
    || a.y + a.height + gap <= b.y || b.y + b.height + gap <= a.y;
}

/** Mirrors geometry's external padding-plus-bleed validation for live authoring previews. */
export function layoutRegionIssues(
  regions: readonly LayoutRegion[],
  output: { width: number; height: number },
  active?: ActiveLayoutRegionBounds,
): Map<string, Set<LayoutRegionIssue>> {
  const issues = new Map<string, Set<LayoutRegionIssue>>();
  const boundsFor = (region: LayoutRegion): PixelBounds => active?.regionId === region.id ? active.bounds : region.bounds;
  const add = (regionId: string, issue: LayoutRegionIssue): void => {
    const regionIssues = issues.get(regionId) ?? new Set<LayoutRegionIssue>();
    regionIssues.add(issue);
    issues.set(regionId, regionIssues);
  };
  for (let index = 0; index < regions.length; index += 1) {
    const region = regions[index]!;
    const bounds = boundsFor(region);
    const clearance = region.paddingPx + region.bleedPx;
    if (bounds.x < clearance || bounds.y < clearance
      || bounds.x + bounds.width + clearance > output.width
      || bounds.y + bounds.height + clearance > output.height) {
      add(region.id, "sheet_edge");
    }
    for (let compare = index + 1; compare < regions.length; compare += 1) {
      const other = regions[compare]!;
      const otherBounds = boundsFor(other);
      if (boundsOverlap(bounds, otherBounds)) {
        add(region.id, "content_overlap");
        add(other.id, "content_overlap");
      } else if (!separatedByClearance(bounds, clearance, otherBounds, other.paddingPx + other.bleedPx)) {
        add(region.id, "clearance");
        add(other.id, "clearance");
      }
    }
  }
  return issues;
}

/** Keeps direct manipulation valid by settling at the nearest valid point on the drag path. */
export function nearestValidLayoutBounds(
  regions: readonly LayoutRegion[],
  regionId: string,
  original: PixelBounds,
  requested: PixelBounds,
  output: { width: number; height: number },
): PixelBounds {
  const region = regions.find((candidate) => candidate.id === regionId);
  if (!region) return requested;
  const clearance = region.paddingPx + region.bleedPx;
  const target = clampBoundsToClearance(requested, output, clearance);
  if (!layoutRegionIssues(regions, output, { regionId, bounds: target }).has(regionId)) return target;
  let valid = clampBoundsToClearance(original, output, clearance);
  let low = 0;
  let high = 1;
  for (let iteration = 0; iteration < 14; iteration += 1) {
    const amount = (low + high) / 2;
    const candidate = clampBoundsToClearance({
      x: Math.round(original.x + (target.x - original.x) * amount),
      y: Math.round(original.y + (target.y - original.y) * amount),
      width: Math.round(original.width + (target.width - original.width) * amount),
      height: Math.round(original.height + (target.height - original.height) * amount),
    }, output, clearance);
    if (layoutRegionIssues(regions, output, { regionId, bounds: candidate }).has(regionId)) high = amount;
    else { low = amount; valid = candidate; }
  }
  return valid;
}

export function layoutRegionIssueLabel(issues: ReadonlySet<LayoutRegionIssue>): string {
  const labels: string[] = [];
  if (issues.has("content_overlap")) labels.push("Overlap");
  if (issues.has("clearance")) labels.push("Insufficient clearance");
  if (issues.has("sheet_edge")) labels.push("Out of bounds / insufficient resolution");
  return labels.join(" / ");
}

/** Expands an absolutely positioned guide outside authoritative content bounds. */
export function externalGuideStyle(bounds: PixelBounds, distance: number): CSSProperties {
  return {
    left: `${-distance / Math.max(1, bounds.width) * 100}%`,
    right: `${-distance / Math.max(1, bounds.width) * 100}%`,
    top: `${-distance / Math.max(1, bounds.height) * 100}%`,
    bottom: `${-distance / Math.max(1, bounds.height) * 100}%`,
  };
}

export function usedAreaRatio(regions: readonly LayoutRegion[], output: { width: number; height: number }): number {
  const area = regions.reduce((sum, region) => sum + region.bounds.width * region.bounds.height, 0);
  return Math.min(1, area / Math.max(1, output.width * output.height));
}

export function switchAuthoringSource<T>(state: LayoutPresentationState<T>, sourceSetId: string): LayoutPresentationState<T> {
  return { sourceSetId, layout: state.layout };
}

export function layoutRegionPresentation(regions: readonly LayoutRegion[], selectedPatchId: string | null): { regions: readonly LayoutRegion[]; highlightedRegionIds: Set<string> } {
  return {
    regions,
    highlightedRegionIds: new Set(regions.filter((region) => region.fill.type === "rectified_patch" && region.fill.patchId === selectedPatchId).map((region) => region.id)),
  };
}

export function reorderRegionPreview(regions: readonly LayoutRegion[], regionId: string, toIndex: number): LayoutRegion[] {
  const ordered = [...regions].sort((a, b) => a.orderIndex - b.orderIndex);
  const from = ordered.findIndex((region) => region.id === regionId);
  if (from < 0) return ordered;
  const [region] = ordered.splice(from, 1);
  if (!region) return ordered;
  ordered.splice(Math.max(0, Math.min(ordered.length, toIndex)), 0, region);
  return ordered.map((candidate, index) => ({ ...candidate, orderIndex: index }));
}

export class LayoutSolveSequencer {
  private generation = 0;
  begin(): number { this.generation += 1; return this.generation; }
  cancel(): void { this.generation += 1; }
  isCurrent(generation: number): boolean { return generation === this.generation; }
}

export function layoutAsyncFailure<T>(state: LayoutAsyncState<T>, generation: number, failure: { message: string; recovery: string }): LayoutAsyncState<T> {
  if (generation !== state.generation) return state;
  return { ...state, busy: false, failure };
}
