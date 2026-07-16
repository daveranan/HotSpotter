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
  type CompiledMapView,
  type CompiledSheetProjection,
  type NormalizedBounds,
  type Patch,
  type PatchCommand,
  type PatchGeometry,
  type ProjectProjection,
  type PreviewSheetProjection,
  type RecentProject,
  type RegionMapping,
  type RegionDefinition,
  type ResolvedRegion,
  type SourceChannel,
  type SourceProjection,
  type TrimSheetDocumentCommand,
} from "@hot-trimmer/ipc-contracts";
import { assignSourceFiles } from "./source-assignment";
import { adjustCrop, anchoredZoom, clamp01, fitView, resizePanes, type CanvasView, type CropDragAction, type PaneDragKind, type PaneState } from "./source-workbench-geometry";
import "./document-app.css";

const protocol = { protocolVersion: IPC_PROTOCOL_VERSION };

const templates = [
  ["ht.generic_architecture", "Generic Architecture"],
  ["ht.horizontal_moulding", "Horizontal Moulding"],
  ["ht.vertical_trim", "Vertical Trim"],
  ["ht.wood_board_moulding", "Wood Board & Moulding"],
  ["ht.detail_ribbon_microtrim", "Detail Ribbon & Microtrim"],
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

type Activity = "starting" | "idle" | "importing" | "compiling" | "saving" | "opening";
type CropProjection = Extract<RegionMapping["projection"], { type: "crop" }>;

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
  const [artifact, setArtifact] = useState<CompiledSheetProjection | null>(null);
  const [preview, setPreview] = useState<PreviewSheetProjection | null>(null);
  const [templateId, setTemplateId] = useState<string>(templates[0][0]);
  const [selectedSourceSetId, setSelectedSourceSetId] = useState<string>("");
  const [selectedChannel, setSelectedChannel] = useState<SourceChannel>("base_color");
  const [selectedRegionId, setSelectedRegionId] = useState<string | null>(null);
  const [mapView, setMapView] = useState<CompiledMapView>("baseColor");
  const [activity, setActivity] = useState<Activity>("starting");
  const [problem, setProblem] = useState<CommandFailure | null>(null);
  const [recentProjects, setRecentProjects] = useState<RecentProject[]>([]);
  const [showRecents, setShowRecents] = useState(false);
  const [panes, setPanes] = useState<PaneState>({ library: 220, source: 470, inspector: 278 });
  const [renaming, setRenaming] = useState(false);
  const [draftName, setDraftName] = useState("");
  const [activePatchId, setActivePatchId] = useState<string | null>(null);
  const [patchTool, setPatchTool] = useState<"rectangle" | "four-point" | null>(null);
  const started = useRef(false);
  const previewDraftId = useRef(0);
  const dirtyPreviewRegion = useRef<string | null>(null);
  const paneDrag = useRef<{ kind: PaneDragKind; start: PaneState } | null>(null);
  const workbenchRef = useRef<HTMLElement | null>(null);

  const sourceSets = project?.sourceSets ?? [];
  const activeSourceSetId = selectedSourceSetId || sourceSets[0]?.id || "";
  const activeSources = useMemo(
    () => project?.sources.filter((source) => source.sourceSetId === activeSourceSetId) ?? [],
    [project?.sources, activeSourceSetId],
  );
  const baseSources = project?.sources.filter((source) => source.channel === "base_color") ?? [];
  const selectedSource = activeSources.find((source) => source.channel === selectedChannel)
    ?? activeSources.find((source) => source.channel === "base_color")
    ?? activeSources[0]
    ?? null;
  const primaryMaterial = project?.document?.primaryMaterial ?? activeSourceSetId;
  const selectedRegion = artifact?.regions.find((region) => region.regionId === selectedRegionId) ?? null;
  const selectedBinding = selectedRegionId ? project?.document?.regionBindings[selectedRegionId] ?? null : null;
  const selectedCrop = selectedBinding?.mapping.projection.type === "crop" ? selectedBinding.mapping.projection : null;
  const stale = !!project?.document && !!artifact && artifact.documentRevision !== project.document.documentRevision;
  const buildState = buildStatus(project, artifact, activity, problem, stale);

  useEffect(() => {
    if (!native || !project?.document) return;
    const dirtyRegion = dirtyPreviewRegion.current;
    dirtyPreviewRegion.current = null;
    void requestPreview(dirtyRegion ?? undefined);
  }, [native, mapView, project?.document?.documentRevision]);

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
    setSelectedSourceSetId(next.document?.primaryMaterial ?? next.sourceSets[0]?.id ?? "");
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
    const occupied = project.sources
      .filter((source) => source.sourceSetId === sourceSetId)
      .map((source) => source.channel);
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
        request: { ...protocol, path, ownership: "owned_copy", channel, sourceSetId },
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
        setProject(current);
      }
      const compiled = await invoke<CompiledSheetProjection>("compile_trim_sheet_document", { request: protocol });
      setArtifact(compiled);
      setSelectedRegionId((selected) => compiled.regions.some((region) => region.regionId === selected) ? selected : null);
    } catch (reason) {
      setProblem(failure(reason));
    } finally {
      setActivity("idle");
    }
  }

  async function createDocumentAndCompile(seed: ProjectProjection, materialId: string) {
    setActivity("compiling");
    setProblem(null);
    try {
      let current = seed;
      if (!current.document) {
        current = await invoke<ProjectProjection>("create_trim_sheet_document", {
          request: { ...protocol, templateId, templateVersion: "1.0.0" },
        });
      }
      if (current.document?.primaryMaterial !== materialId) {
        current = await applyCommand({ type: "set_primary_material", materialId });
      }
      const compiled = await invoke<CompiledSheetProjection>("compile_trim_sheet_document", { request: protocol });
      setProject(current);
      setArtifact(compiled);
      setSelectedRegionId((selected) => compiled.regions.some((region) => region.regionId === selected) ? selected : null);
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

  async function requestPreview(regionId?: string, projection?: CropProjection) {
    if (!native || !project?.document) return;
    const draftId = ++previewDraftId.current;
    try {
      const next = await invoke<PreviewSheetProjection>("preview_trim_sheet_document", {
        request: { ...protocol, draftId, mapView, regionId, projection, maxEdge: 1024 },
      });
      if (next.draftId === previewDraftId.current) setPreview(next);
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
  }

  async function setResolution(size: number) {
    if (!project?.document) return;
    try {
      await command({ type: "set_output_resolution", outputSize: { width: size, height: size } });
    } catch (reason) {
      setProblem(failure(reason));
    }
  }

  async function setLayoutGrid(size: number) {
    if (!project?.document) return;
    try {
      await command({ type: "set_layout_grid", settings: { columns: size, rows: size, padding: project.document.layoutGrid.padding } });
    } catch (reason) { setProblem(failure(reason)); }
  }

  async function setRegionDestination(regionId: string, allocationRect: { x: number; y: number; width: number; height: number }, padding: number) {
    try { await command({ type: "set_region_destination", regionId, allocationRect, padding }); }
    catch (reason) { setProblem(failure(reason)); }
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

      <section ref={workbenchRef} className="workbench" style={{ gridTemplateColumns: `${panes.library}px 6px ${panes.source}px 6px minmax(320px, 1fr) 6px ${panes.inspector}px` }}>
        <SourceLibrary
          project={project}
          activeSourceSetId={activeSourceSetId}
          selectedSource={selectedSource}
          onSelect={chooseSource}
          onAddSourceSet={() => void addSourceSet()}
        />
        <PaneSplitter kind="library-source" paneDrag={paneDrag} setPanes={setPanes} workbenchRef={workbenchRef} />
        <section className="source-workspace">
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
          </div>
          <SourceCanvas
            source={selectedSource}
            crop={selectedCrop}
            selectedRegion={selectedRegion}
            importing={activity === "importing"}
            onOpenBase={() => void chooseImages("base_color")}
            onCommitCrop={(bounds) => void setSelectedCrop(bounds)}
            onDraftCrop={previewSelectedCrop}
            patches={project?.patches.filter((patch) => patch.sourceId === selectedSource?.id) ?? []}
            activePatchId={activePatchId}
            onEditPatch={setActivePatchId}
            onCommitPatch={(patchId, geometry) => void replacePatchGeometry(patchId, geometry)}
            onExitPatch={() => setActivePatchId(null)}
            tool={patchTool}
            onCreatePatch={(geometry, fourPoint) => void createPatch(geometry, fourPoint)}
            onCancelTool={() => setPatchTool(null)}
          />
        </section>
        <PaneSplitter kind="source-sheet" paneDrag={paneDrag} setPanes={setPanes} workbenchRef={workbenchRef} />
        <SheetWorkbench
          project={project}
          artifact={artifact}
          preview={preview}
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
          setLayoutGrid={setLayoutGrid}
        />
        <PaneSplitter kind="sheet-inspector" paneDrag={paneDrag} setPanes={setPanes} workbenchRef={workbenchRef} />
        <Inspector
          project={project}
          artifact={artifact}
          selectedRegion={selectedRegion}
          mapView={mapView}
          setMapView={setMapView}
          onUndo={() => void history(false)}
          onRedo={() => void history(true)}
          onSetDestination={(regionId, rect, padding) => void setRegionDestination(regionId, rect, padding)}
        />
      </section>
      <footer className="statusbar">
        <span>{project?.name ?? "Untitled"}</span>
        <span>{buildState}</span>
        <span>{selectedSource ? `${channelLabel(selectedSource.channel)} / ${selectedSource.width} x ${selectedSource.height}` : "No source selected"}</span>
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
}) {
  const sourceSets = props.project?.sourceSets ?? [];
  return <aside className="source-library">
    <header className="panel-title"><span>WORKPLACE</span></header>
    <section className="library-section"><div className="section-head"><span>SOURCES</span><b>{sourceSets.length}</b></div>
      {sourceSets.map((set) => {
        const base = props.project?.sources.find((source) => source.sourceSetId === set.id && source.channel === "base_color");
        const count = props.project?.sources.filter((source) => source.sourceSetId === set.id).length ?? 0;
        return <button key={set.id} className={`source-set ${set.id === props.activeSourceSetId ? "active" : ""}`} onClick={() => props.onSelect(set.id, base?.channel ?? "base_color")}>
          <span className="thumb">{base ? <img src={base.thumbnailDataUrl} alt="" /> : "+"}</span>
          <span><strong>{base?.displayName ?? set.name}</strong><small>{count} map{count === 1 ? "" : "s"}</small></span>
        </button>;
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
        title={blocked ? "Add Base Color to anchor this source set first." : source?.sourcePath ?? `Add ${option.label}`}
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
  crop: CropProjection | null;
  selectedRegion: ResolvedRegion | null;
  importing: boolean;
  onOpenBase: () => void;
  onCommitCrop: (bounds: NormalizedBounds) => void;
  onDraftCrop: (bounds: NormalizedBounds) => void;
  patches: readonly Patch[];
  activePatchId: string | null;
  onEditPatch: (patchId: string) => void;
  onCommitPatch: (patchId: string, geometry: PatchGeometry) => void;
  onExitPatch: () => void;
  tool: "rectangle" | "four-point" | null;
  onCreatePatch: (geometry: PatchGeometry, fourPoint: boolean) => void;
  onCancelTool: () => void;
}) {
  const stageRef = useRef<HTMLDivElement | null>(null);
  const viewport = useViewportController(props.source ? { width: props.source.width, height: props.source.height } : null);
  const cropDrag = useRef<{ pointerId: number; action: CropDragAction; origin: NormalizedBounds; x: number; y: number } | null>(null);
  const [draftCrop, setDraftCrop] = useState<NormalizedBounds | null>(null);
  const draftCropRef = useRef<NormalizedBounds | null>(null);
  const previewFrame = useRef<number | null>(null);
  const patchDrag = useRef<{ pointerId: number; patchId: string; corner: number; corners: PatchGeometry["corners"] } | null>(null);
  const patchCreate = useRef<{ pointerId: number; start: { x: number; y: number } } | null>(null);
  const [draftPatch, setDraftPatch] = useState<{ patchId: string; geometry: PatchGeometry } | null>(null);
  const [draftRectangle, setDraftRectangle] = useState<PatchGeometry | null>(null);
  const [fourPointDraft, setFourPointDraft] = useState<Array<{ x: number; y: number }>>([]);
  const effectiveCrop = draftCrop ?? props.crop?.bounds ?? null;

  useEffect(() => {
    setDraftCrop(null);
    draftCropRef.current = null;
  }, [props.crop?.bounds.x, props.crop?.bounds.y, props.crop?.bounds.width, props.crop?.bounds.height]);

  useEffect(() => {
    setDraftRectangle(null);
    setFourPointDraft([]);
  }, [props.tool]);

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
    const activePoint = patchDrag.current;
    if (activePoint?.pointerId === event.pointerId) {
      const target = point(event);
      const corners = [...activePoint.corners] as unknown as [typeof target, typeof target, typeof target, typeof target];
      corners[activePoint.corner] = target;
      setDraftPatch({ patchId: activePoint.patchId, geometry: { corners } });
      return;
    }
    const activeCrop = cropDrag.current;
    if (activeCrop?.pointerId === event.pointerId) {
      const target = point(event);
      const dx = target.x - activeCrop.x;
      const dy = target.y - activeCrop.y;
      const next = adjustCrop(activeCrop.origin, activeCrop.action, dx, dy);
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
    if (patchDrag.current?.pointerId === event.pointerId) {
      const patchId = patchDrag.current.patchId;
      patchDrag.current = null;
      if (draftPatch?.patchId === patchId) props.onCommitPatch(patchId, draftPatch.geometry);
    }
    const activeCrop = cropDrag.current;
    if (activeCrop?.pointerId === event.pointerId) {
      cropDrag.current = null;
      if (draftCropRef.current) props.onCommitCrop(draftCropRef.current);
    }
    viewport.endPan(event);
  }

  function beginCrop(event: React.PointerEvent<HTMLElement>, action: CropDragAction) {
    if (!effectiveCrop || event.button !== 0) return;
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    const start = point(event);
    cropDrag.current = { pointerId: event.pointerId, action, origin: effectiveCrop, x: start.x, y: start.y };
  }

  function beginPatchPoint(event: React.PointerEvent<Element>, patch: Patch, corner: number) {
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    const geometry = draftPatch?.patchId === patch.id ? draftPatch.geometry : patch.geometry;
    patchDrag.current = { pointerId: event.pointerId, patchId: patch.id, corner, corners: geometry.corners };
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
    onClick={(event) => { if (event.target === event.currentTarget && !props.tool) props.onExitPatch(); }}
  >
    {props.source ? <div
      ref={stageRef}
      className="source-stage"
      style={{ width: props.source.width, height: props.source.height, transform: `translate(${viewport.view.x}px, ${viewport.view.y}px) scale(${viewport.view.scale})` }}
      onPointerDown={beginPatchCreate}
    >
      <img
        src={props.source.thumbnailDataUrl}
        alt={`${channelLabel(props.source.channel)} source ${props.source.displayName}`}
        draggable={false}
        onClick={() => { if (!props.tool) props.onExitPatch(); }}
      />
      {effectiveCrop ? <button
        className="source-crop"
        style={cropStyle(effectiveCrop)}
        onPointerDown={(event) => beginCrop(event, "move")}
        aria-label={`Move source crop for ${props.selectedRegion?.displayName ?? "selected region"}`}
      >
        <span>{props.selectedRegion?.displayName ?? "Region crop"}</span>
        <b className="handle nw" onPointerDown={(event) => beginCrop(event, "nw")} />
        <b className="handle ne" onPointerDown={(event) => beginCrop(event, "ne")} />
        <b className="handle sw" onPointerDown={(event) => beginCrop(event, "sw")} />
        <b className="handle se" onPointerDown={(event) => beginCrop(event, "se")} />
      </button> : null}
    </div> : <div className="empty-source-canvas">
      <strong>Open or drop a Base Color</strong>
      <span>The source canvas is ready before the project has a save location.</span>
      <button className="primary" onClick={props.onOpenBase}>Open Base Color</button>
    </div>}
    {props.source ? <svg
      className="patch-overlay"
      style={{ left: viewport.view.x, top: viewport.view.y, width: props.source.width * viewport.view.scale, height: props.source.height * viewport.view.scale }}
      viewBox={`0 0 ${props.source.width} ${props.source.height}`}
      aria-label="Editable patch outlines"
    >
      {props.patches.map((patch) => {
        const geometry = draftPatch?.patchId === patch.id ? draftPatch.geometry : patch.geometry;
        const active = props.activePatchId === patch.id;
        const points = geometry.corners.map((corner) => `${corner.x * props.source!.width},${corner.y * props.source!.height}`).join(" ");
        const handleRadius = 8 / viewport.view.scale;
        const hitRadius = 15 / viewport.view.scale;
        return <g key={patch.id} className={`patch-outline ${active ? "active" : ""}`}>
          <polygon points={points} onDoubleClick={(event) => { event.stopPropagation(); props.onEditPatch(patch.id); }} />
          {active ? geometry.corners.map((corner, index) => <g key={index} className="patch-point">
            <circle className="patch-point-hit" cx={corner.x * props.source!.width} cy={corner.y * props.source!.height} r={hitRadius} onPointerDown={(event) => beginPatchPoint(event, patch, index)} />
            <circle className="patch-point-visible" cx={corner.x * props.source!.width} cy={corner.y * props.source!.height} r={handleRadius} />
          </g>) : null}
        </g>;
      })}
      {previewGeometry ? <polygon className="patch-outline draft" points={previewGeometry.corners.map((corner) => `${corner.x * props.source!.width},${corner.y * props.source!.height}`).join(" ")} /> : null}
    </svg> : null}
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

function PaneSplitter(props: {
  kind: PaneDragKind;
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
    if (rect) props.setPanes(() => resizePanes(props.kind, active.start, event.clientX, rect.left, rect.width));
  }
  function up() {
    props.paneDrag.current = null;
  }
  return <div className="pane-splitter" onPointerDown={down} onPointerMove={move} onPointerUp={up} onPointerCancel={up} role="separator" aria-orientation="vertical" />;
}

function SheetWorkbench(props: {
  project: ProjectProjection | null;
  artifact: CompiledSheetProjection | null;
  preview: PreviewSheetProjection | null;
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
  setLayoutGrid: (size: number) => void;
}) {
  const artifact = props.artifact;
  const sheet = props.preview ?? artifact;
  const imageUrl = props.preview?.mapView === props.mapView ? props.preview.dataUrl : artifact?.maps[props.mapView];
  const viewport = useViewportController(sheet ? { width: sheet.width, height: sheet.height } : null);
  return <section className="sheet-workbench">
    <header className="sheet-header">
      <div><strong>HOTSPOT SHEET</strong></div>
      <span className={`build-status ${props.problem ? "error" : props.artifact ? "ready" : ""}`}>{props.buildState}</span>
    </header>
    <section className="template-strip">
      <span>REFERENCE-MESH HOTSPOT TEMPLATE</span>
      <strong>Generate the Generic Architecture hotspot sheet</strong>
      <select value={props.templateId} onChange={(event) => props.setTemplateId(event.target.value)} disabled={!!props.project?.document}>
        {templates.map(([id, name]) => <option key={id} value={id}>{name}</option>)}
      </select>
      <select value={props.project?.document?.renderSettings.outputSize.width ?? 2048} onChange={(event) => void props.setResolution(Number(event.target.value))} disabled={!props.project?.document}>
        <option value={1024}>1024</option>
        <option value={2048}>2048</option>
        <option value={4096}>4096</option>
      </select>
      <select aria-label="Layout grid" value={props.project?.document?.layoutGrid.columns ?? 32} onChange={(event) => void props.setLayoutGrid(Number(event.target.value))} disabled={!props.project?.document}>
        {[16, 24, 32, 48, 64].map((size) => <option key={size} value={size}>{size} x {size} grid</option>)}
      </select>
      <button className="primary" onClick={props.build} disabled={!props.primaryMaterial || props.activity !== "idle"}>
        {props.activity === "compiling" ? "Compiling..." : props.project?.document ? "Update sheet" : "Build trim sheet"}
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
      {!sheet || !imageUrl ? <div className="empty-sheet">
        <strong>{props.project?.legacyLayoutDiscarded ? "No trim sheet yet" : "No compiled sheet"}</strong>
        <span>{props.project?.legacyLayoutDiscarded ? "Sources, maps, and patches were preserved. Old layout state is not shown or converted." : "Build from the current Base Color when ready."}</span>
      </div> : <div
        className="sheet"
        style={{ width: sheet.width, height: sheet.height, transform: `translate(${viewport.view.x}px, ${viewport.view.y}px) scale(${viewport.view.scale})` }}
        onPointerDown={(event) => {
          if (event.button === 0 && !(event.target as Element).closest(".region")) props.setSelectedRegionId(null);
        }}
      >
        <img src={imageUrl} alt={`${props.mapView} trim sheet preview`} />
        <div
          className="sheet-grid"
          style={{
            backgroundSize: `${100 / (props.project?.document?.layoutGrid.columns ?? 32)}% ${100 / (props.project?.document?.layoutGrid.rows ?? 32)}%`,
          }}
        />
        <div className="overlays">{sheet.regions.map((region) => <button
          key={region.regionId}
          className={`region ${region.regionId === props.selectedRegionId ? "selected" : ""}`}
          style={overlayStyle(region, sheet)}
          onClick={(event) => { event.stopPropagation(); props.setSelectedRegionId(region.regionId === props.selectedRegionId ? null : region.regionId); }}
        ><span>{region.displayName}</span></button>)}</div>
      </div>}
      {sheet ? <div className="viewport-tools">
        <button onClick={() => viewport.zoom(0.8)}>-</button>
        <output>{Math.round(viewport.view.scale * 100)}%</output>
        <button onClick={() => viewport.zoom(1.25)}>+</button>
        <button onClick={viewport.fit}>Fit</button>
      </div> : null}
    </section>
    {props.artifact ? <footer className="artifact-footer">
      <span>{props.artifact.width} x {props.artifact.height}</span>
      <span>{props.artifact.regions.length} regions</span>
      <span>{props.artifact.rendererVersion}</span>
      <span>topology {props.artifact.topologyHash.slice(0, 10)}</span>
      <span>appearance {props.artifact.appearanceHash.slice(0, 10)}</span>
    </footer> : null}
  </section>;
}

function Inspector(props: {
  project: ProjectProjection | null;
  artifact: CompiledSheetProjection | null;
  selectedRegion: ResolvedRegion | null;
  mapView: CompiledMapView;
  setMapView: (view: CompiledMapView) => void;
  onUndo: () => void;
  onRedo: () => void;
  onSetDestination: (regionId: string, rect: { x: number; y: number; width: number; height: number }, padding: number) => void;
}) {
  const binding = props.selectedRegion && props.project?.document?.regionBindings[props.selectedRegion.regionId];
  return <aside className="context-inspector">
    <header className="inspector-actions"><button onClick={props.onUndo} disabled={!props.project?.canUndoDocument}>Undo</button><button onClick={props.onRedo} disabled={!props.project?.canRedoDocument}>Redo</button></header>
    <section className="inspector-section">
      <span>MAP VIEW</span>
      <div className="map-view-grid">{mapViews.map(([id, label]) => <button key={id} className={props.mapView === id ? "active" : ""} onClick={() => props.setMapView(id)} disabled={!props.artifact}>{label}</button>)}</div>
    </section>
    <section className="inspector-section">
      <span>SELECTED REGION</span>
      {props.selectedRegion ? <>
        <h2>{props.selectedRegion.displayName}</h2>
        <code>{props.selectedRegion.regionId}</code>
        <dl>
          <dt>Content</dt><dd>{contentLabel(binding?.content.type)}</dd>
          <dt>Projection</dt><dd>{binding?.mapping.projection.type ?? "-"}</dd>
          <dt>Source crop</dt><dd>{cropLabel(binding?.mapping.projection)}</dd>
          <dt>Bounds</dt><dd>{boundsLabel(props.selectedRegion.allocationBounds)}</dd>
          <dt>Material</dt><dd>{props.selectedRegion.materialId.slice(0, 8)}</dd>
        </dl>
        <DestinationEditor
          key={props.selectedRegion.regionId}
          region={props.project?.document?.topology.regions.find((region) => region.id === props.selectedRegion!.regionId) ?? null}
          padding={props.project?.document?.layoutGrid.padding ?? 8}
          onApply={props.onSetDestination}
        />
      </> : <p>Select a patch or create one on the source workbench.</p>}
    </section>
    <LockedSection title="Mapping & Warp" reason="No direct-manipulation document transaction is implemented yet." />
    <LockedSection title="Profiles & Weathering" reason="Generated-map recipes are not command-backed in this slice." />
    <LockedSection title="Decorations" reason="Decoration bindings require authored patch commands." />
  </aside>;
}

function DestinationEditor(props: {
  region: RegionDefinition | null;
  padding: number;
  onApply: (regionId: string, rect: { x: number; y: number; width: number; height: number }, padding: number) => void;
}) {
  const [rect, setRect] = useState(props.region?.allocationRect ?? { x: 0, y: 0, width: 512, height: 512 });
  if (!props.region) return null;
  return <div className="destination-editor">
    <strong>DESTINATION GRID BOUNDS</strong>
    {(["x", "y", "width", "height"] as const).map((field) => <label key={field}>{field}<input type="number" min={0} max={4096} value={rect[field]} onChange={(event) => setRect((current) => ({ ...current, [field]: Number(event.target.value) }))} /></label>)}
    <button onClick={() => props.onApply(props.region!.id, rect, props.padding)}>Apply bounds</button>
  </div>;
}

function LockedSection({ title, reason }: { title: string; reason: string }) {
  return <section className="locked"><strong>{title}</strong><span>{reason}</span></section>;
}

function buildStatus(project: ProjectProjection | null, artifact: CompiledSheetProjection | null, activity: Activity, problem: CommandFailure | null, stale: boolean) {
  if (activity === "importing") return "Importing";
  if (activity === "compiling") return `Compiling revision ${project?.document?.documentRevision ?? 1}`;
  if (problem) return "Region error";
  if (!project?.sources.some((source) => source.channel === "base_color")) return "Empty";
  if (!project.document) return "Ready";
  if (stale || !artifact) return "Stale";
  return `Ready rev ${artifact.documentRevision}`;
}

function channelLabel(channel: SourceChannel): string {
  return channelOptions.find((option) => option.value === channel)?.label ?? channel;
}

function contentLabel(type?: string) {
  return type === "inherit_primary_material" ? "Primary material" : type?.replaceAll("_", " ") ?? "-";
}

function boundsLabel(bounds: { x: number; y: number; width: number; height: number }) {
  return `${bounds.x}, ${bounds.y} / ${bounds.width} x ${bounds.height}`;
}

function cropLabel(projection?: { type: string; bounds?: { x: number; y: number; width: number; height: number } }) {
  if (!projection || projection.type !== "crop" || !projection.bounds) return "-";
  const b = projection.bounds;
  return `${b.x.toFixed(2)}, ${b.y.toFixed(2)} / ${b.width.toFixed(2)} x ${b.height.toFixed(2)}`;
}

function overlayStyle(region: ResolvedRegion, artifact: Pick<CompiledSheetProjection, "width" | "height">): React.CSSProperties {
  const bounds = region.allocationBounds;
  return {
    left: `${bounds.x / artifact.width * 100}%`,
    top: `${bounds.y / artifact.height * 100}%`,
    width: `${bounds.width / artifact.width * 100}%`,
    height: `${bounds.height / artifact.height * 100}%`,
    borderColor: `rgb(${region.idColor[0]} ${region.idColor[1]} ${region.idColor[2]})`,
  };
}

function cropStyle(bounds: NormalizedBounds): React.CSSProperties {
  return {
    left: `${bounds.x * 100}%`,
    top: `${bounds.y * 100}%`,
    width: `${bounds.width * 100}%`,
    height: `${bounds.height * 100}%`,
  };
}

createRoot(document.getElementById("root")!).render(<React.StrictMode><App /></React.StrictMode>);
