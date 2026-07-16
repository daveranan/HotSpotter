import React, { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  IPC_PROTOCOL_VERSION,
  type CommandFailure,
  type CompiledLayoutPreview,
  type CompiledLayoutPreviewMap,
  type FillBehavior,
  type GenerateLayoutResult,
  type LayoutCommand,
  type LayoutItem,
  type LayoutPreset,
  type LayoutRegion,
  type LayoutSettings,
  type LayoutStateSnapshot,
  type PixelBounds,
  type ProjectSnapshot,
  type RegionFill,
  type SourceChannel,
  type StoredLayout,
  type TemplateIdentity,
} from "@hot-trimmer/ipc-contracts";
import {
  LayoutSolveSequencer,
  beginLayoutDrag,
  availableLayoutPreviewMaps,
  buildCustomAtlasGenerateLayoutRequest,
  buildLayoutRequest,
  buildTemplateGenerateLayoutRequest,
  cancelLayoutDrag,
  cssBounds,
  defaultLayoutSettings,
  externalGuideStyle,
  keyboardBounds,
  layoutRegionIssueLabel,
  layoutRegionIssues,
  layoutRegionPresentation,
  layoutPreviewDataUrl,
  layoutPreviewMapOptions,
  nearestValidLayoutBounds,
  settingsForPreset,
  sheetPointFromClient,
  updateLayoutDrag,
  usedAreaRatio,
  withUpdatedItem,
  defaultTemplateSourceTransform,
  genericArchitectureTemplate,
  templateOptions,
  type LayoutDrag,
} from "./layout-authoring";
import { zoomViewAtPoint } from "./patch-authoring";
import { SerialTaskQueue } from "./serial-task-queue";

interface LayoutWorkspaceProps {
  project: ProjectSnapshot;
  selectedPatchId: string | null;
  selectedSourceSetId: string | null;
  onLayoutState: (state: LayoutStateSnapshot) => void;
  onFailure: (failure: CommandFailure | null) => void;
}

interface SheetView {
  x: number;
  y: number;
  scale: number;
}

interface SheetPanDrag {
  pointerId: number;
  x: number;
  y: number;
  origin: SheetView;
}

interface RegionContextMenu {
  regionId: string;
  x: number;
  y: number;
}

const genericArchitecturePreset: LayoutPreset = "balanced";
const behaviorOptions: ReadonlyArray<{ value: FillBehavior; label: string }> = [
  { value: "horizontal_loop", label: "Horizontal Loop" },
  { value: "vertical_loop", label: "Vertical Loop" },
  { value: "tile", label: "Tile" },
  { value: "stretch", label: "Stretch" },
  { value: "unique_detail", label: "Unique Detail" },
  { value: "trim_cap", label: "Trim Cap" },
];

const dataChannels: SourceChannel[] = ["height", "roughness", "metallic", "ambient_occlusion", "opacity", "material_id"];
const baseRequest = { protocolVersion: IPC_PROTOCOL_VERSION } as const;

function failureFrom(reason: unknown, fallback: string): CommandFailure {
  if (typeof reason === "object" && reason !== null) {
    const candidate = reason as Partial<CommandFailure>;
    if (typeof candidate.message === "string" && typeof candidate.recovery === "string") {
      return { code: candidate.code ?? "layout_command_failed", message: candidate.message, recovery: candidate.recovery, detail: candidate.detail };
    }
  }
  return {
    code: "layout_command_failed",
    message: fallback,
    recovery: "Keep the current sheet, review the highlighted constraints, then retry.",
    detail: reason instanceof Error ? reason.message : String(reason),
  };
}

function imageForRegion(project: ProjectSnapshot, region: LayoutRegion): string | undefined {
  const sourceSetId = region.fill.type === "whole_source_set" || region.fill.type === "rectified_patch" ? region.fill.sourceSetId : undefined;
  const source = sourceSetId ? project.sources.find((candidate) => candidate.sourceSetId === sourceSetId && candidate.channel === "base_color")
    ?? project.sources.find((candidate) => candidate.sourceSetId === sourceSetId) : undefined;
  return source?.thumbnailMipmaps.find((mipmap) => mipmap.maxEdge === 640)?.dataUrl ?? source?.thumbnailDataUrl;
}

function regionLabel(project: ProjectSnapshot, region: LayoutRegion): string {
  if (region.fill.type === "rectified_patch") {
    const patchId = region.fill.patchId;
    return project.patches.find((patch) => patch.id === patchId)?.name ?? "Patch";
  }
  if (region.fill.type === "whole_source_set") {
    const sourceSetId = region.fill.sourceSetId;
    return project.sourceSets.find((sourceSet) => sourceSet.id === sourceSetId)?.name ?? "Material source";
  }
  if (region.fill.type === "simple_color") return "Color region";
  return `${region.fill.input.channel.replaceAll("_", " ")} ${region.fill.input.value}`;
}

function colorHex(rgba: [number, number, number, number]): string {
  return `#${rgba.slice(0, 3).map((value) => value.toString(16).padStart(2, "0")).join("")}`;
}

function hexColor(value: string): [number, number, number, number] {
  return [Number.parseInt(value.slice(1, 3), 16), Number.parseInt(value.slice(3, 5), 16), Number.parseInt(value.slice(5, 7), 16), 255];
}

export function LayoutWorkspace({ project, selectedPatchId, selectedSourceSetId, onLayoutState, onFailure }: LayoutWorkspaceProps): React.JSX.Element {
  const initialLayout = project.layout;
  const [stored, setStored] = useState<StoredLayout | null>(initialLayout);
  const [preset, setPreset] = useState<LayoutPreset>(initialLayout?.layout.preset ?? "balanced");
  const [settings, setSettings] = useState<LayoutSettings>(initialLayout?.layout.settings ?? defaultLayoutSettings());
  const [items, setItems] = useState<LayoutItem[]>(initialLayout?.items ?? []);
  const [selectedSourceSetIds, setSelectedSourceSetIds] = useState<string[]>(project.sourceSets.map((sourceSet) => sourceSet.id));
  const [selectedRegionId, setSelectedRegionId] = useState<string | null>(initialLayout?.layout.regions[0]?.id ?? null);
  const [sheetView, setSheetView] = useState<SheetView>({ x: 0, y: 0, scale: 1 });
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<CommandFailure | null>(null);
  const [drag, setDrag] = useState<LayoutDrag | null>(null);
  const [simpleColor, setSimpleColor] = useState("#6987a5");
  const [simpleChannel, setSimpleChannel] = useState<SourceChannel>("roughness");
  const [simpleValue, setSimpleValue] = useState(0.5);
  const [compiledPreview, setCompiledPreview] = useState<CompiledLayoutPreview | null>(null);
  const [previewMap, setPreviewMap] = useState<CompiledLayoutPreviewMap>("baseColor");
  const [sourceTransform, setSourceTransform] = useState(defaultTemplateSourceTransform);
  const [templateIdentity, setTemplateIdentity] = useState<TemplateIdentity>(genericArchitectureTemplate);
  const [regionContextMenu, setRegionContextMenu] = useState<RegionContextMenu | null>(null);
  const sheetRef = useRef<HTMLDivElement | null>(null);
  const scrollportRef = useRef<HTMLDivElement | null>(null);
  const sheetPanDrag = useRef<SheetPanDrag | null>(null);
  const lastSheetCursor = useRef<{ x: number; y: number } | null>(null);
  const dragElement = useRef<HTMLElement | null>(null);
  const solveSequence = useRef(new LayoutSolveSequencer());
  const commandQueue = useRef(new SerialTaskQueue());
  const commandGroup = useRef(1);
  const autoGeneratedSources = useRef<string | null>(null);
  const rawRegions = stored?.layout.preset === preset ? stored.layout.regions : [];
  const presentation = useMemo(() => layoutRegionPresentation(rawRegions, selectedPatchId), [rawRegions, selectedPatchId]);
  const regions = presentation.regions;
  const output = stored?.layout.preset === preset ? stored.layout.settings.output : settings.output;
  const templateMode = preset !== "atlas";
  const availablePreviewMaps = useMemo(() => availableLayoutPreviewMaps(compiledPreview), [compiledPreview]);
  const compiledPreviewDataUrl = layoutPreviewDataUrl(compiledPreview, previewMap);
  const contentSignature = useMemo(() => {
    const selected = new Set(selectedSourceSetIds);
    if (preset === "atlas") {
      const eligible = project.patches.filter((patch) => {
        const source = project.sources.find((candidate) => candidate.id === patch.sourceId);
        return Boolean(source && selected.has(source.sourceSetId) && patch.enabled && patch.properties.mapParticipation !== "excluded");
      });
      return eligible.length ? `atlas|${JSON.stringify(eligible)}` : "";
    }
    const baseSources = project.sources.filter((source) => source.channel === "base_color" && selected.has(source.sourceSetId));
    if (!baseSources.length) return "";
    // Include every registered map in the content key: replacing Normal or Roughness
    // must refresh the compiled preview even when the Base Color/source-set identity is unchanged.
    const sourceRevision = project.sources
      .filter((source) => selected.has(source.sourceSetId))
      .map((source) => `${source.channel}:${source.id}:${source.width}x${source.height}:${source.encodedBytes}`)
      .sort()
      .join("|");
    return `${preset}|${templateIdentity.templateId}|${JSON.stringify(sourceTransform)}|${sourceRevision}`;
  }, [preset, project.patches, project.sources, selectedSourceSetIds, sourceTransform, templateIdentity]);
  const selectedRegion = regions.find((region) => region.id === selectedRegionId) ?? null;
  const selectedItem = selectedRegion ? items.find((item) => item.key === selectedRegion.itemKey) ?? null : null;
  const templateSourceSetId = selectedSourceSetIds.find((id) => project.sources.some((source) => source.sourceSetId === id && source.channel === "base_color"));
  const selectedTemplateOption = templateOptions.find((option) => option.identity.templateId === templateIdentity.templateId)
    ?? templateOptions[0]!;

  useEffect(() => {
    setStored(project.layout);
    if (project.layout) {
      setPreset(project.layout.layout.preset);
      setSettings(project.layout.layout.settings);
      setItems(project.layout.items);
      setTemplateIdentity(project.layout.template?.snapshot?.identity ?? genericArchitectureTemplate);
      setSelectedRegionId((current) => project.layout?.layout.regions.some((region) => region.id === current) ? current : project.layout?.layout.regions[0]?.id ?? null);
    }
  }, [project.layout]);

  useEffect(() => {
    const valid = new Set(project.sourceSets.map((sourceSet) => sourceSet.id));
    setSelectedSourceSetIds((current) => {
      const retained = current.filter((id) => valid.has(id));
      return retained.length ? retained : project.sourceSets.map((sourceSet) => sourceSet.id);
    });
  }, [project.sourceSets]);

  useEffect(() => {
    if (!availablePreviewMaps.includes(previewMap)) setPreviewMap("baseColor");
  }, [availablePreviewMaps, previewMap]);

  useEffect(() => {
    if (busy || !contentSignature || autoGeneratedSources.current === contentSignature) return;
    autoGeneratedSources.current = contentSignature;
    void generate();
  }, [busy, contentSignature]);

  useEffect(() => {
    if (!drag) return;
    const activeDrag = drag;
    function cancel(event: KeyboardEvent): void {
      if (event.key !== "Escape") return;
      event.preventDefault();
      const element = dragElement.current;
      if (element?.hasPointerCapture(activeDrag.pointerId)) element.releasePointerCapture(activeDrag.pointerId);
      setDrag(null);
      dragElement.current = null;
    }
    window.addEventListener("keydown", cancel);
    return () => window.removeEventListener("keydown", cancel);
  }, [drag]);

  const invalidIds = useMemo(() => {
    return layoutRegionIssues(regions, output, drag ? { regionId: drag.regionId, bounds: drag.preview } : undefined);
  }, [drag, output, regions]);

  function publishFailure(next: CommandFailure | null): void {
    setFailure(next);
    onFailure(next);
  }

  function acceptState(state: LayoutStateSnapshot): void {
    setStored(state.layout);
    if (state.layout) {
      setItems(state.layout.items);
      setSettings(state.layout.layout.settings);
      setPreset(state.layout.layout.preset);
    }
    onLayoutState(state);
  }

  async function generate(overrides?: { items?: LayoutItem[]; preset?: LayoutPreset; settings?: LayoutSettings; sourceTransform?: typeof sourceTransform }): Promise<void> {
    const participatingSourceSetIds = selectedSourceSetIds.length ? selectedSourceSetIds : project.sourceSets.map((sourceSet) => sourceSet.id);
    const nextPreset = overrides?.preset ?? preset;
    const nextSettings = overrides?.settings ?? settings;
    const nextItems = overrides?.items ?? items;
    const nextSourceTransform = overrides?.sourceTransform ?? sourceTransform;
    const layoutId = stored?.layout.id ?? crypto.randomUUID();
    const sourceSetId = participatingSourceSetIds.find((id) => project.sources.some((source) => source.sourceSetId === id && source.channel === "base_color"));
    if (nextPreset !== "atlas" && !sourceSetId) {
      publishFailure(null);
      return;
    }
    const request = nextPreset === "atlas"
      ? (() => {
        const layoutRequest = buildLayoutRequest(project, {
          layoutId,
          preset: nextPreset,
          settings: nextSettings,
          selectedSourceSetIds: participatingSourceSetIds,
          includePatches: true,
          items: nextItems,
          existingRegions: stored?.layout.regions,
        });
        return layoutRequest.items.some((item) => item.enabled && item.participates)
          ? buildCustomAtlasGenerateLayoutRequest(layoutRequest, commandGroup.current++)
          : null;
      })()
      : buildTemplateGenerateLayoutRequest(sourceSetId!, layoutId, nextSettings, nextSourceTransform, commandGroup.current++, templateIdentity);
    if (!request) {
      publishFailure(null);
      return;
    }
    const generation = solveSequence.current.begin();
    setBusy(true);
    publishFailure(null);
    try {
      const result = await invoke<GenerateLayoutResult>("generate_layout", {
        request,
      });
      if (!solveSequence.current.isCurrent(generation)) return;
      setCompiledPreview(result.preview ?? null);
      acceptState(result.state);
    } catch (reason) {
      if (!solveSequence.current.isCurrent(generation)) return;
      const nextFailure = failureFrom(reason, "Trim-sheet generation failed.");
      if (nextFailure.code !== "operation_cancelled") publishFailure(nextFailure);
    } finally {
      if (solveSequence.current.isCurrent(generation)) setBusy(false);
    }
  }

  async function cancelSolve(): Promise<void> {
    solveSequence.current.cancel();
    setBusy(false);
    await invoke<void>("cancel_layout_solve", { request: baseRequest }).catch((reason) => publishFailure(failureFrom(reason, "Cancel layout generation failed.")));
  }

  async function apply(command: LayoutCommand, coalescingGroup?: number): Promise<void> {
    try {
      const state = await commandQueue.current.run(() => invoke<LayoutStateSnapshot>("apply_layout_command", {
          request: { protocolVersion: IPC_PROTOCOL_VERSION, command, coalescingGroup },
        }));
      acceptState(state);
      publishFailure(null);
    } catch (reason) {
      publishFailure(failureFrom(reason, "Layout edit failed."));
    }
  }

  async function history(redo: boolean): Promise<void> {
    try {
      const state = await commandQueue.current.run(() => invoke<LayoutStateSnapshot>(redo ? "redo_project_command" : "undo_project_command", { request: baseRequest }));
      acceptState(state);
      publishFailure(null);
    } catch (reason) {
      publishFailure(failureFrom(reason, `${redo ? "Redo" : "Undo"} failed.`));
    }
  }

  function choosePreset(next: LayoutPreset): void {
    const nextSettings = settingsForPreset(settings, next);
    setPreset(next);
    setSettings(nextSettings);
    setSelectedRegionId(null);
  }

  function chooseTemplate(identity: TemplateIdentity): void {
    setTemplateIdentity(identity);
    setSelectedRegionId(null);
  }

  function zoomBy(factor: number): void {
    zoomAtCursor(factor, lastSheetCursor.current);
  }

  function fitSheet(): void {
    setSheetView({ x: 0, y: 0, scale: 1 });
  }

  function zoomAtCursor(factor: number, cursor = lastSheetCursor.current): void {
    const rect = sheetRef.current?.getBoundingClientRect();
    if (!rect) return;
    const anchor = cursor ?? { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
    setSheetView((current) => {
      const nextScale = Math.max(0.25, Math.min(3, Number((current.scale * factor).toFixed(2))));
      return zoomViewAtPoint(current, nextScale, anchor, rect);
    });
  }

  function beginSheetPan(event: React.PointerEvent<HTMLDivElement>): void {
    lastSheetCursor.current = { x: event.clientX, y: event.clientY };
    if (event.button !== 1) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    sheetPanDrag.current = { pointerId: event.pointerId, x: event.clientX, y: event.clientY, origin: sheetView };
  }

  function moveSheetPan(event: React.PointerEvent<HTMLDivElement>): void {
    lastSheetCursor.current = { x: event.clientX, y: event.clientY };
    const active = sheetPanDrag.current;
    if (!active || active.pointerId !== event.pointerId) return;
    setSheetView((current) => ({ ...current, x: active.origin.x + event.clientX - active.x, y: active.origin.y + event.clientY - active.y }));
  }

  function endSheetPan(event: React.PointerEvent<HTMLDivElement>): void {
    if (sheetPanDrag.current?.pointerId !== event.pointerId) return;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
    sheetPanDrag.current = null;
  }

  function setOutput(dimension: "width" | "height", value: number): void {
    if (!Number.isFinite(value)) return;
    setSettings((current) => ({ ...current, output: { ...current.output, [dimension]: Math.max(64, Math.min(16384, Math.round(value))) } }));
  }

  function beginRegionPointer(event: React.PointerEvent<HTMLElement>, region: LayoutRegion): void {
    if (event.button !== 0 || busy) return;
    const sheet = sheetRef.current;
    if (!sheet) return;
    event.preventDefault(); event.stopPropagation();
    const kind = ((event.target as HTMLElement).dataset.handle ?? "move") as LayoutDrag["kind"];
    event.currentTarget.setPointerCapture(event.pointerId);
    dragElement.current = event.currentTarget;
    setSelectedRegionId(region.id);
    setDrag(beginLayoutDrag(region, kind, event.pointerId, sheetPointFromClient({ x: event.clientX, y: event.clientY }, sheet.getBoundingClientRect(), output), commandGroup.current++));
  }

  function moveRegionPointer(event: React.PointerEvent<HTMLElement>, region: LayoutRegion): void {
    if (!drag || drag.pointerId !== event.pointerId || drag.regionId !== region.id || !sheetRef.current) return;
    const next = updateLayoutDrag(drag, sheetPointFromClient({ x: event.clientX, y: event.clientY }, sheetRef.current.getBoundingClientRect(), output), output, region.locks, region.paddingPx + region.bleedPx);
    setDrag({ ...next, preview: nearestValidLayoutBounds(regions, region.id, next.original, next.preview, output) });
  }

  function endRegionPointer(event: React.PointerEvent<HTMLElement>, cancelled = false): void {
    if (!drag || drag.pointerId !== event.pointerId) return;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
    const bounds = cancelled ? cancelLayoutDrag(drag) : drag.preview;
    const original = drag.original;
    const group = drag.coalescingGroup;
    setDrag(null); dragElement.current = null;
    if (!cancelled && (bounds.x !== original.x || bounds.y !== original.y || bounds.width !== original.width || bounds.height !== original.height)) {
      void apply({ type: "set_bounds", regionId: drag.regionId, bounds }, group);
    }
  }

  function displayedBounds(region: LayoutRegion): PixelBounds {
    return drag?.regionId === region.id ? drag.preview : region.bounds;
  }

  function regionKeyDown(event: React.KeyboardEvent<HTMLElement>, region: LayoutRegion): void {
    if (busy) return;
    if (event.key === "p" || event.key === "P") {
      event.preventDefault(); void apply({ type: "set_locks", regionId: region.id, locks: { ...region.locks, position: !region.locks.position } }); return;
    }
    if (event.key === "w" || event.key === "W") {
      event.preventDefault(); void apply({ type: "set_locks", regionId: region.id, locks: { ...region.locks, width: !region.locks.width } }); return;
    }
    if (event.key === "h" || event.key === "H") {
      event.preventDefault(); void apply({ type: "set_locks", regionId: region.id, locks: { ...region.locks, height: !region.locks.height } }); return;
    }
    if (event.altKey && (event.key === "ArrowUp" || event.key === "ArrowDown")) {
      event.preventDefault();
      const offset = event.key === "ArrowUp" ? -1 : 1;
      void apply({ type: "reorder", regionId: region.id, toIndex: Math.max(0, Math.min(regions.length - 1, region.orderIndex + offset)) }); return;
    }
    if (!event.key.startsWith("Arrow")) return;
    event.preventDefault();
    if (!event.shiftKey && region.locks.position) return;
    if (event.shiftKey && (event.key === "ArrowLeft" || event.key === "ArrowRight") && region.locks.width) return;
    if (event.shiftKey && (event.key === "ArrowUp" || event.key === "ArrowDown") && region.locks.height) return;
    const requested = keyboardBounds(region.bounds, event.key, { shift: event.shiftKey, alt: event.ctrlKey }, output);
    void apply({ type: "set_bounds", regionId: region.id, bounds: nearestValidLayoutBounds(regions, region.id, region.bounds, requested, output) }, commandGroup.current++);
  }

  function updateSelectedItem(update: Partial<LayoutItem>): void {
    if (!selectedItem) return;
    setItems((current) => withUpdatedItem(current, selectedItem.key, update));
  }

  function setRegionFill(region: LayoutRegion, fill: RegionFill): void {
    void apply({ type: "set_fill", regionId: region.id, fill }, commandGroup.current++);
    setRegionContextMenu(null);
  }

  async function addSimple(fill: "color" | "data"): Promise<void> {
    const item: LayoutItem = {
      key: `simple:${crypto.randomUUID()}`,
      fill: fill === "color" ? { type: "simple_color", rgba: hexColor(simpleColor) } : { type: "simple_data", input: { channel: simpleChannel, value: simpleValue } },
      behavior: "stretch",
      naturalSize: { width: 256, height: 256 },
      enabled: true,
      participates: true,
      constraints: {},
    };
    const nextItems = [...items, item];
    setItems(nextItems);
    await generate({ items: nextItems });
  }

  async function deleteSelectedSimple(): Promise<void> {
    if (!selectedRegion || (selectedRegion.fill.type !== "simple_color" && selectedRegion.fill.type !== "simple_data")) return;
    await apply({ type: "delete_simple", regionId: selectedRegion.id });
  }

  const used = usedAreaRatio(regions, output);

  return <section className="hotspot-workpiece layout-workpiece" aria-label="Authoritative trim sheet">
    <header className="split-pane-title layout-title">
      <div><span>Patches &amp; Layout</span><strong>Trim sheet</strong></div>
      <div className="layout-history" aria-label="Project layout history">
        <button onClick={() => void history(false)} disabled={!project.canUndoProject || busy} title="Undo project layout edit (Ctrl+Z)">Undo</button>
        <button onClick={() => void history(true)} disabled={!project.canRedoProject || busy} title="Redo project layout edit (Ctrl+Shift+Z)">Redo</button>
      </div>
    </header>

    <section className="layout-first-entry" aria-labelledby="layout-first-title">
      <span className="eyebrow">{templateMode ? "Trim sheet workbench" : "Patch atlas"}</span><h2 id="layout-first-title">{templateMode ? "Build a reusable trim-sheet layout" : "Pack captured patches as an atlas"}</h2>
      <p>{templateMode ? "Choose a stable template, frame the source, and keep every registered map on the same slot boundaries." : "Atlas mode contains enabled patches only. Padding, bleed, ordering, and free placement apply here."}</p>
      <div className="layout-mode-chooser" role="group" aria-label="Sheet mode">
        <button className={templateMode && templateIdentity.templateId === genericArchitectureTemplate.templateId ? "active" : ""} aria-pressed={templateMode && templateIdentity.templateId === genericArchitectureTemplate.templateId} onClick={() => { choosePreset(genericArchitecturePreset); chooseTemplate(genericArchitectureTemplate); }}>Hotspot</button>
        <button className={templateMode && templateIdentity.templateId !== genericArchitectureTemplate.templateId ? "active" : ""} aria-pressed={templateMode && templateIdentity.templateId !== genericArchitectureTemplate.templateId} onClick={() => { choosePreset(genericArchitecturePreset); chooseTemplate(templateOptions[1]!.identity); }}>Trim</button>
        <button className={!templateMode ? "active" : ""} aria-pressed={!templateMode} onClick={() => choosePreset("atlas")}>Atlas</button>
      </div>
      {templateMode ? <div className="template-controls">
        <label>Template<select value={templateIdentity.templateId} onChange={(event) => { const option = templateOptions.find((candidate) => candidate.identity.templateId === event.target.value); if (option) chooseTemplate(option.identity); }}>
          {templateOptions.map((option) => <option key={option.identity.templateId} value={option.identity.templateId}>{option.label}</option>)}
        </select><small>{selectedTemplateOption.description}</small></label>
        <label>Source framing<select value={sourceTransform.mode} onChange={(event) => setSourceTransform((current) => ({ ...current, mode: event.target.value as typeof current.mode }))}><option value="cover">Cover / crop</option><option value="stretch">Stretch to sheet</option><option value="repeat">Repeat</option></select></label>
        {sourceTransform.mode === "cover" ? <div className="source-framing-focus"><label>Focus X<input aria-label="Source framing focus X" type="range" min={0} max={1} step={0.01} value={sourceTransform.cropFocus.x} onChange={(event) => setSourceTransform((current) => ({ ...current, cropFocus: { ...current.cropFocus, x: event.target.valueAsNumber } }))} /></label><label>Focus Y<input aria-label="Source framing focus Y" type="range" min={0} max={1} step={0.01} value={sourceTransform.cropFocus.y} onChange={(event) => setSourceTransform((current) => ({ ...current, cropFocus: { ...current, y: event.target.valueAsNumber } }))} /></label></div> : null}
      </div> : null}
    </section>

    <div className="layout-canvas-shell">
      <div className="layout-canvas-toolbar" role="toolbar" aria-label="Sheet canvas controls">
        <span>{output.width} Ã— {output.height}</span><span>{regions.length} region{regions.length === 1 ? "" : "s"}</span>
        <span>{Math.round((1 - used) * 100)}% unused</span>
        {invalidIds.size ? <strong className="invalid-summary">{invalidIds.size} invalid</strong> : null}
        {templateMode ? <label>Map <select aria-label="Template preview map" value={previewMap} disabled={!compiledPreview} onChange={(event) => setPreviewMap(event.target.value as CompiledLayoutPreviewMap)}>{layoutPreviewMapOptions.filter((option) => availablePreviewMaps.includes(option.key)).map((option) => <option key={option.key} value={option.key}>{option.label}</option>)}</select></label> : null}
      </div>
      <div
        ref={scrollportRef}
        className="layout-scrollport"
        onPointerDown={beginSheetPan}
        onPointerMove={moveSheetPan}
        onPointerUp={endSheetPan}
        onPointerCancel={endSheetPan}
        onWheel={(event) => { event.preventDefault(); lastSheetCursor.current = { x: event.clientX, y: event.clientY }; zoomAtPoint(event.deltaY < 0 ? 1.1 : 0.9, lastSheetCursor.current); }}
      >
        <div className="layout-sheet-stage" style={{ transform: `translate(${sheetPan.x}px, ${sheetPan.y}px)` }}>
          <div ref={sheetRef} className="layout-sheet" style={{ aspectRatio: `${output.width} / ${output.height}`, maxWidth: "760px", transform: `scale(${zoom})` }} aria-label={`Complete ${output.width} by ${output.height} trim sheet`}>
            {compiledPreview && compiledPreviewDataUrl ? <img className="layout-compiled-preview" src={compiledPreviewDataUrl} width={compiledPreview.width} height={compiledPreview.height} alt="" /> : null}
            {regions.map((region) => {
              const bounds = displayedBounds(region);
              const image = templateMode ? undefined : imageForRegion(project, region);
              const patchSelected = presentation.highlightedRegionIds.has(region.id);
              const selected = region.id === selectedRegionId;
              const regionIssues = invalidIds.get(region.id);
              const invalid = Boolean(regionIssues);
              const locks = [region.locks.position ? "P" : "", region.locks.width ? "W" : "", region.locks.height ? "H" : ""].filter(Boolean).join("");
              const regionColor = `rgb(${region.idColor.join(" ")})`;
              return <div
                key={region.id}
                className={`layout-region ${templateMode ? "template-slot" : ""} ${selected ? "selected" : ""} ${patchSelected ? "patch-selected" : ""} ${invalid ? "invalid" : ""}`}
                role="button" tabIndex={0} aria-pressed={selected}
                aria-label={`${regionLabel(project, region)} region, x ${bounds.x}, y ${bounds.y}, width ${bounds.width}, height ${bounds.height}${locks ? `, locks ${locks}` : ""}`}
                style={{ ...cssBounds(bounds, output), "--region-color": regionColor, backgroundImage: image ? `linear-gradient(rgb(12 16 18 / 22%), rgb(12 16 18 / 22%)), url("${image}")` : undefined, backgroundColor: templateMode ? "transparent" : region.fill.type === "simple_color" ? colorHex(region.fill.rgba) : regionColor } as React.CSSProperties}
                onFocus={() => setSelectedRegionId(region.id)} onClick={() => setSelectedRegionId(region.id)} onKeyDown={(event) => regionKeyDown(event, region)}
                onContextMenu={(event) => { event.preventDefault(); event.stopPropagation(); setSelectedRegionId(region.id); setRegionContextMenu({ regionId: region.id, x: event.clientX, y: event.clientY }); }}
                onPointerDown={(event) => beginRegionPointer(event, region)} onPointerMove={(event) => moveRegionPointer(event, region)} onPointerUp={(event) => endRegionPointer(event)} onPointerCancel={(event) => endRegionPointer(event, true)}
              >
                <span className="layout-region-label"><strong>{regionLabel(project, region)}</strong><small>{bounds.width} Ã— {bounds.height} Â· {region.behavior.replaceAll("_", " ")}</small></span>
                {region.trimCaps ? <><i className={`trim-cap leading ${region.trimCaps.axis}`} /><i className={`trim-cap trailing ${region.trimCaps.axis}`} /></> : null}
                {locks ? <span className="region-locks" title="Position / width / height locks">ðŸ”’ {locks}</span> : null}
                {regionIssues ? <span className="region-invalid">{layoutRegionIssueLabel(regionIssues)}</span> : null}
                <i className="padding-guide" style={externalGuideStyle(bounds, region.paddingPx)} />
                <i className="bleed-guide" style={externalGuideStyle(bounds, region.paddingPx + region.bleedPx)} />
                {(["north", "east", "south", "west", "north-east", "south-east", "south-west", "north-west"] as const).map((handle) => <span key={handle} className={`region-handle ${handle}`} data-handle={handle} aria-hidden="true" />)}
              </div>;
            })}
            {!regions.length ? <div className="layout-empty-sheet"><strong>{busy ? "Updating sheet..." : templateMode ? "Add a Base Color material" : "Create or enable a patch"}</strong><span>{templateMode ? "The selected hotspot template is generated automatically. Patches are optional slot replacements." : "Patch Atlas packs enabled patches only; it never invents regions from the whole source."}</span></div> : null}
          </div>
        </div>
        <div className="viewport-tools layout-viewport-tools" role="group" aria-label="Sheet viewport controls" onPointerDown={(event) => event.stopPropagation()}><button className="active" title="Pan with middle mouse drag">Pan</button><button title="Zoom out (-)" onClick={() => zoomBy(0.8)}>−</button><output aria-live="polite">{Math.round(zoom * 100)}%</output><button title="Zoom in (+)" onClick={() => zoomBy(1.25)}>+</button><button title="Fit sheet (0)" onClick={fitSheet}>Fit</button></div>
      </div>
      <div className="layout-legend" aria-label="Layout visualization legend"><span><i className="legend-padding" />Padding</span><span><i className="legend-bleed" />Bleed</span><span><i className="legend-cap" />Trim cap</span><span>ðŸ”’ Locked</span><span className="legend-invalid">Overlap / invalid</span></div>
    </div>

    <div className="layout-dock">
      <details open className="layout-panel"><summary>Sheet setup</summary>
        {templateMode
          ? <div className="automatic-input-note"><strong>{selectedTemplateOption.label}</strong><span>{selectedTemplateOption.description}</span></div>
          : <div className="automatic-input-note"><strong>Custom Atlas</strong><button onClick={() => choosePreset("balanced")}>Return to Template Presets</button></div>}
        <p className="automatic-input-note"><strong>{templateMode ? "Compiled UV template" : "Patch-only packing"}</strong><span>{templateMode ? "The returned PNG is the authoritative template preview." : "Every enabled patch from the included materials is packed once."}</span></p>
        {templateMode ? <label>Material<select value={templateSourceSetId ?? ""} onChange={(event) => setSelectedSourceSetIds(event.target.value ? [event.target.value] : [])}><option value="">Choose a Base Color material</option>{project.sourceSets.filter((sourceSet) => project.sources.some((source) => source.sourceSetId === sourceSet.id && source.channel === "base_color")).map((sourceSet) => <option key={sourceSet.id} value={sourceSet.id}>{sourceSet.name}</option>)}</select><small>Changing the material recompiles the selected template. Source framing is available above the sheet.</small></label> : null}
        {!templateMode ? <details className="layout-advanced-inputs"><summary>Choose materials</summary>
        <fieldset className="layout-source-choices"><legend>Included materials</legend>
          <div className="button-row"><button onClick={() => setSelectedSourceSetIds(selectedSourceSetId ? [selectedSourceSetId] : [])} disabled={!selectedSourceSetId}>Selected source</button><button onClick={() => setSelectedSourceSetIds(project.sourceSets.map((sourceSet) => sourceSet.id))}>All sources</button></div>
          {project.sourceSets.map((sourceSet) => <label key={sourceSet.id} className="check-field"><input type="checkbox" checked={selectedSourceSetIds.includes(sourceSet.id)} onChange={(event) => setSelectedSourceSetIds((current) => event.target.checked ? [...current, sourceSet.id] : current.filter((id) => id !== sourceSet.id))} />{sourceSet.name}</label>)}
          <p className="automatic-input-note"><span>Enabled participating patches are included automatically.</span></p>
        </fieldset></details> : null}
      </details>

      {!templateMode ? <details className="layout-panel"><summary>Atlas packing</summary>
        <div className="numeric-pair"><label>Width<input type="number" min={64} max={16384} value={settings.output.width} onChange={(event) => setOutput("width", event.target.valueAsNumber)} /></label><label>Height<input type="number" min={64} max={16384} value={settings.output.height} onChange={(event) => setOutput("height", event.target.valueAsNumber)} /></label></div>
        <div className="numeric-pair"><label>Padding<input type="number" min={0} value={settings.paddingPx} onChange={(event) => setSettings((current) => ({ ...current, paddingPx: Math.max(0, event.target.valueAsNumber || 0) }))} /></label><label>Bleed<input type="number" min={0} value={settings.bleedPx} onChange={(event) => setSettings((current) => ({ ...current, bleedPx: Math.max(0, event.target.valueAsNumber || 0) }))} /></label></div>
        <label>Patch order<select value={settings.order} onChange={(event) => setSettings((current) => ({ ...current, order: event.target.value as LayoutSettings["order"] }))}><option value="input">Input order</option><option value="largest_first">Largest first</option><option value="horizontal_first">Horizontal first</option><option value="vertical_first">Vertical first</option></select></label>
        <label className="check-field"><input type="checkbox" checked={settings.autoPack.enabled} onChange={(event) => setSettings((current) => ({ ...current, autoPack: { ...current.autoPack, enabled: event.target.checked } }))} />Arrange regions automatically</label>
        <label>Pack priority<select value={settings.autoPack.priority} onChange={(event) => setSettings((current) => ({ ...current, autoPack: { ...current.autoPack, priority: event.target.value as LayoutSettings["autoPack"]["priority"] } }))}><option value="balanced">Balanced</option><option value="horizontal_strips">Horizontal trims</option><option value="vertical_strips">Vertical trims</option></select></label>
        {selectedRegion ? <label className="check-field"><input type="checkbox" checked={settings.fixedSelectedSize?.regionId === selectedRegion.id} onChange={(event) => setSettings((current) => ({ ...current, fixedSelectedSize: event.target.checked ? { regionId: selectedRegion.id, size: { width: selectedRegion.bounds.width, height: selectedRegion.bounds.height } } : undefined }))} />Keep selected region at {selectedRegion.bounds.width} Ã— {selectedRegion.bounds.height} on auto-pack</label> : null}
      </details> : null}

      <details open={Boolean(selectedRegion)} className="layout-panel"><summary>Selected region</summary>
        {selectedRegion ? <>
          <strong>{regionLabel(project, selectedRegion)}</strong>
          <label>Source / patch<select aria-label="Region source or patch" value={selectedRegion.fill.type === "rectified_patch" ? `patch:${selectedRegion.fill.patchId}` : selectedRegion.fill.type === "whole_source_set" ? `source:${selectedRegion.fill.sourceSetId}` : "simple"} onChange={(event) => {
            const value = event.target.value;
            if (value.startsWith("source:")) setRegionFill(selectedRegion, { type: "whole_source_set", sourceSetId: value.slice(7) });
            else if (value.startsWith("patch:")) {
              const patch = project.patches.find((candidate) => candidate.id === value.slice(6));
              const source = patch ? project.sources.find((candidate) => candidate.id === patch.sourceId) : undefined;
              if (patch && source) setRegionFill(selectedRegion, { type: "rectified_patch", sourceSetId: source.sourceSetId, patchId: patch.id });
            }
          }}>
            {project.sourceSets.map((sourceSet) => <option key={`source:${sourceSet.id}`} value={`source:${sourceSet.id}`}>{sourceSet.name} · whole source</option>)}
            {project.patches.map((patch) => <option key={`patch:${patch.id}`} value={`patch:${patch.id}`}>{patch.name} · patch</option>)}
            {selectedRegion.fill.type !== "whole_source_set" && selectedRegion.fill.type !== "rectified_patch" ? <option value="simple">Simple region</option> : null}
          </select><small>Double-click a region to edit its bounds. Right-click it to choose a patch directly.</small></label>
          {templateMode ? <small>Template regions are selected against the compiled preview, but their slot bounds and source assignment remain editable.</small> : <><label>Behavior<select value={selectedItem?.behavior ?? selectedRegion.behavior} onChange={(event) => { const behavior = event.target.value as FillBehavior; const span = Math.max(1, selectedItem?.naturalSize.width ?? selectedRegion.bounds.width); const cap = Math.min(16, Math.max(0, Math.floor((span - 1) / 2))); updateSelectedItem({ behavior, trimCaps: behavior === "trim_cap" ? selectedItem?.trimCaps ?? { axis: "horizontal", leadingPx: cap, trailingPx: cap } : undefined }); }}>{behaviorOptions.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}</select></label>
          {(selectedItem?.behavior ?? selectedRegion.behavior) === "trim_cap" ? <div className="trim-cap-fields"><label>Axis<select value={selectedItem?.trimCaps?.axis ?? "horizontal"} onChange={(event) => updateSelectedItem({ trimCaps: { axis: event.target.value as "horizontal" | "vertical", leadingPx: selectedItem?.trimCaps?.leadingPx ?? 16, trailingPx: selectedItem?.trimCaps?.trailingPx ?? 16 } })}><option value="horizontal">Horizontal</option><option value="vertical">Vertical</option></select></label><label>Leading<input type="number" min={0} value={selectedItem?.trimCaps?.leadingPx ?? 16} onChange={(event) => updateSelectedItem({ trimCaps: { axis: selectedItem?.trimCaps?.axis ?? "horizontal", leadingPx: event.target.valueAsNumber, trailingPx: selectedItem?.trimCaps?.trailingPx ?? 16 } })} /></label><label>Trailing<input type="number" min={0} value={selectedItem?.trimCaps?.trailingPx ?? 16} onChange={(event) => updateSelectedItem({ trimCaps: { axis: selectedItem?.trimCaps?.axis ?? "horizontal", leadingPx: selectedItem?.trimCaps?.leadingPx ?? 16, trailingPx: event.target.valueAsNumber } })} /></label></div> : null}</>}
          {!templateMode ? <>
          <fieldset className="region-bounds-editor"><legend>Exact pixel bounds</legend>{(["x", "y", "width", "height"] as const).map((field) => <label key={field}>{field.toUpperCase()}<input key={`${selectedRegion.id}-${field}-${selectedRegion.bounds[field]}`} type="number" min={field === "width" || field === "height" ? 1 : 0} defaultValue={selectedRegion.bounds[field]} disabled={(field === "x" || field === "y") ? selectedRegion.locks.position : field === "width" ? selectedRegion.locks.width : selectedRegion.locks.height} onBlur={(event) => { const value = event.currentTarget.valueAsNumber; if (!Number.isFinite(value)) { event.currentTarget.value = String(selectedRegion.bounds[field]); return; } void apply({ type: "set_bounds", regionId: selectedRegion.id, bounds: keyboardBounds({ ...selectedRegion.bounds, [field]: Math.round(value) }, "", {}, output) }, commandGroup.current++); }} onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); else if (event.key === "Escape") { event.currentTarget.value = String(selectedRegion.bounds[field]); event.currentTarget.blur(); } }} /></label>)}</fieldset>
          <div className="region-lock-controls" role="group" aria-label="Region dimension locks">{(["position", "width", "height"] as const).map((lock) => <button key={lock} aria-pressed={selectedRegion.locks[lock]} className={selectedRegion.locks[lock] ? "active" : ""} onClick={() => void apply({ type: "set_locks", regionId: selectedRegion.id, locks: { ...selectedRegion.locks, [lock]: !selectedRegion.locks[lock] } })}>Lock {lock}</button>)}</div>
          <div className="button-row"><button onClick={() => void apply({ type: "reorder", regionId: selectedRegion.id, toIndex: Math.max(0, selectedRegion.orderIndex - 1) })} disabled={selectedRegion.orderIndex === 0}>Earlier</button><button onClick={() => void apply({ type: "reorder", regionId: selectedRegion.id, toIndex: Math.min(regions.length - 1, selectedRegion.orderIndex + 1) })} disabled={selectedRegion.orderIndex === regions.length - 1}>Later</button><button className="danger" onClick={() => void deleteSelectedSimple()} disabled={selectedRegion.fill.type !== "simple_color" && selectedRegion.fill.type !== "simple_data"}>Delete simple region</button></div>
          <small>Keyboard: arrows move; Shift+arrows resize; Ctrl accelerates; Alt+Up/Down reorders; P/W/H toggle locks.</small>
          </> : null}
        </> : <p>Select a region on the complete sheet. Selecting a patch on the left only highlights its region.</p>}
      </details>

      {!templateMode ? <details className="layout-panel"><summary>Simple color / data region</summary>
        <div className="simple-region-controls"><label>Color<input type="color" value={simpleColor} onChange={(event) => setSimpleColor(event.target.value)} /></label><button onClick={() => void addSimple("color")}>Add color region</button><label>Data<select value={simpleChannel} onChange={(event) => setSimpleChannel(event.target.value as SourceChannel)}>{dataChannels.map((channel) => <option key={channel} value={channel}>{channel.replaceAll("_", " ")}</option>)}</select></label><label>Value<input type="number" min={0} max={1} step={0.01} value={simpleValue} onChange={(event) => setSimpleValue(event.target.valueAsNumber)} /></label><button onClick={() => void addSimple("data")}>Add data region</button></div>
      </details> : null}
    </div>

    {regionContextMenu ? <div className="patch-context-menu region-context-menu" role="menu" style={{ left: regionContextMenu.x, top: regionContextMenu.y }} onPointerDown={(event) => event.stopPropagation()}>
      <strong>Assign region source</strong>
      {(() => {
        const region = regions.find((candidate) => candidate.id === regionContextMenu.regionId);
        return region ? <>
          {project.sourceSets.map((sourceSet) => <button key={`source:${sourceSet.id}`} role="menuitem" onClick={() => setRegionFill(region, { type: "whole_source_set", sourceSetId: sourceSet.id })}>{sourceSet.name} · whole source</button>)}
          {project.patches.map((patch) => {
            const source = project.sources.find((candidate) => candidate.id === patch.sourceId);
            return source ? <button key={`patch:${patch.id}`} role="menuitem" onClick={() => setRegionFill(region, { type: "rectified_patch", sourceSetId: source.sourceSetId, patchId: patch.id })}>{patch.name} · patch</button> : null;
          })}
        </> : null;
      })()}
    </div> : null}

    {project.warnings.map((warning) => <section key={`${warning.code}-${warning.message}`} className="layout-warning" role="status"><strong>{warning.message}</strong><span>{warning.recovery}</span></section>)}
    {failure ? <section className="layout-error" role="alert"><strong>{failure.message}</strong><span>{failure.recovery}</span>{failure.detail ? <details><summary>Technical detail</summary><code>{failure.detail}</code></details> : null}</section> : null}
    <div className="layout-generate-bar"><button className="primary" onClick={() => void generate()} disabled={busy}>Update sheet</button>{busy ? <><span role="status">Updating sheetâ€¦</span><button onClick={() => void cancelSolve()}>Cancel</button></> : <small>{templateMode ? "Automatic on material or template changes; use Update sheet after editing slot behavior." : "Atlas repacks enabled patches with the current packing settings."}</small>}</div>
  </section>;
}
