import React, { useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  IPC_PROTOCOL_VERSION,
  type CommandFailure,
  type CompiledMapView,
  type CompiledSheetProjection,
  type ProjectProjection,
  type ResolvedRegion,
  type TrimSheetDocumentCommand,
} from "@hot-trimmer/ipc-contracts";
import "./document-app.css";

const protocol = { protocolVersion: IPC_PROTOCOL_VERSION };
const templates = [
  ["ht.generic_architecture", "Generic Architecture"],
  ["ht.horizontal_moulding", "Horizontal Moulding"],
  ["ht.vertical_trim", "Vertical Trim"],
  ["ht.wood_board_moulding", "Wood Board & Moulding"],
  ["ht.detail_ribbon_microtrim", "Detail Ribbon & Microtrim"],
] as const;
const mapViews: readonly [CompiledMapView, string][] = [
  ["baseColor", "Base Color"], ["normal", "Normal"], ["height", "Height"],
  ["roughness", "Roughness"], ["metallic", "Metallic"],
  ["ambientOcclusion", "AO"], ["regionId", "Region ID"], ["materialId", "Material ID"],
];

type Activity = "idle" | "importing" | "compiling";

function failure(reason: unknown): CommandFailure {
  if (typeof reason === "object" && reason && "message" in reason) {
    const value = reason as Partial<CommandFailure>;
    return { code: value.code ?? "operation_failed", message: String(value.message), recovery: value.recovery ?? "Correct the issue and retry." };
  }
  return { code: "operation_failed", message: String(reason), recovery: "Correct the issue and retry." };
}

function App() {
  const [project, setProject] = useState<ProjectProjection | null>(null);
  const [artifact, setArtifact] = useState<CompiledSheetProjection | null>(null);
  const [templateId, setTemplateId] = useState<string>(templates[0][0]);
  const [primaryMaterial, setPrimaryMaterial] = useState<string>("");
  const [selectedRegionId, setSelectedRegionId] = useState<string | null>(null);
  const [mapView, setMapView] = useState<CompiledMapView>("baseColor");
  const [activity, setActivity] = useState<Activity>("idle");
  const [problem, setProblem] = useState<CommandFailure | null>(null);

  const baseSources = project?.materialSources.flatMap((material) =>
    material.registeredChannels?.channels.filter((source) => source.channel === "base_color") ?? []) ?? [];
  const effectivePrimary = primaryMaterial || project?.document?.primaryMaterial
    || project?.materialSources.find((material) => material.registeredChannels?.channels.some((source) => source.channel === "base_color"))?.id || "";
  const selectedRegion = artifact?.regions.find((region) => region.regionId === selectedRegionId) ?? null;
  const stale = !!project?.document && artifact?.documentRevision !== project.document.documentRevision;
  const buildLabel = activity === "compiling"
    ? `Compiling revision ${project?.document?.documentRevision ?? 1}`
    : problem && selectedRegionId ? `Error in Region ${selectedRegion?.displayName ?? selectedRegionId}`
    : problem ? "Build error"
    : !project?.document ? (baseSources.length ? "Ready to create" : "Import Base Color")
    : stale ? "Needs rebuild"
    : `Up to date at revision ${artifact?.documentRevision}`;

  async function newProject() {
    const path = await save({ title: "New Hot Trimmer Project", defaultPath: "Untitled.hottrimmer", filters: [{ name: "Hot Trimmer", extensions: ["hottrimmer"] }] });
    if (!path) return;
    try {
      const next = await invoke<ProjectProjection>("create_project", { request: { ...protocol, path, name: "Untitled" } });
      reset(next);
    } catch (reason) { setProblem(failure(reason)); }
  }

  async function openProject() {
    const path = await open({ multiple: false, title: "Open Hot Trimmer Project", filters: [{ name: "Hot Trimmer", extensions: ["hottrimmer"] }] });
    if (typeof path !== "string") return;
    try {
      const next = await invoke<ProjectProjection>("open_project", { request: { ...protocol, path } });
      reset(next);
    } catch (reason) { setProblem(failure(reason)); }
  }

  function reset(next: ProjectProjection) {
    setProject(next); setArtifact(null); setSelectedRegionId(null); setProblem(null);
    setPrimaryMaterial(next.document?.primaryMaterial ?? next.materialSources.find((material) => material.registeredChannels?.channels.some((source) => source.channel === "base_color"))?.id ?? "");
  }

  async function importBaseColor() {
    if (!project) return;
    const path = await open({ multiple: false, title: "Import Base Color", filters: [{ name: "Texture Image", extensions: ["png", "jpg", "jpeg", "tif", "tiff"] }] });
    if (typeof path !== "string") return;
    setActivity("importing"); setProblem(null);
    try {
      const sourceSetId = project.materialSources[0]?.id;
      if (!sourceSetId) throw new Error("This project has no material source set.");
      const next = await invoke<ProjectProjection>("import_source", { request: {
        ...protocol, path, ownership: "owned_copy", channel: "base_color", sourceSetId,
        assignmentProvenance: "user_assigned", confidenceMilli: 1000, normalConvention: "not_applicable",
      } });
      setProject(next); setPrimaryMaterial(sourceSetId); setArtifact(null);
    } catch (reason) { setProblem(failure(reason)); }
    finally { setActivity("idle"); }
  }

  async function command(command: TrimSheetDocumentCommand): Promise<ProjectProjection> {
    const next = await invoke<ProjectProjection>("apply_document_command", { request: { ...protocol, command } });
    setProject(next); setProblem(null); return next;
  }

  async function choosePrimary(materialId: string) {
    setPrimaryMaterial(materialId);
    if (!project?.document) return;
    try { await command({ type: "set_primary_material", materialId }); }
    catch (reason) { setProblem(failure(reason)); }
  }

  async function build() {
    if (!project || !effectivePrimary || activity !== "idle") return;
    setActivity("compiling"); setProblem(null);
    try {
      let current = project;
      if (!current.document) {
        current = await invoke<ProjectProjection>("create_trim_sheet_document", { request: { ...protocol, templateId, templateVersion: "1.0.0" } });
        setProject(current);
      }
      if (current.document?.primaryMaterial !== effectivePrimary) {
        current = await command({ type: "set_primary_material", materialId: effectivePrimary });
      }
      const compiled = await invoke<CompiledSheetProjection>("compile_trim_sheet_document", { request: protocol });
      setArtifact(compiled);
      setSelectedRegionId((selected) => compiled.regions.some((region) => region.regionId === selected) ? selected : null);
    } catch (reason) { setProblem(failure(reason)); }
    finally { setActivity("idle"); }
  }

  async function setResolution(size: number) {
    try { await command({ type: "set_output_resolution", outputSize: { width: size, height: size } }); }
    catch (reason) { setProblem(failure(reason)); }
  }

  async function history(redo: boolean) {
    try {
      const next = await invoke<ProjectProjection>(redo ? "redo_document_command" : "undo_document_command", { request: protocol });
      setProject(next); setProblem(null);
    } catch (reason) { setProblem(failure(reason)); }
  }

  async function saveProject() {
    try { setProject(await invoke<ProjectProjection>("save_project", { request: protocol })); setProblem(null); }
    catch (reason) { setProblem(failure(reason)); }
  }

  async function closeProject() {
    try { await invoke("close_project", { request: { ...protocol, save: true } }); setProject(null); setArtifact(null); setSelectedRegionId(null); setProblem(null); }
    catch (reason) { setProblem(failure(reason)); }
  }

  if (!project) return <Landing onNew={newProject} onOpen={openProject} problem={problem} />;

  return <div className="app-shell">
    <header className="topbar">
      <div><span className="brand">HOT TRIMMER</span><span className="project-name">{project.name}</span></div>
      <div className="top-actions">
        <button onClick={() => history(false)} disabled={!project.canUndoDocument}>Undo</button>
        <button onClick={() => history(true)} disabled={!project.canRedoDocument}>Redo</button>
        <button onClick={saveProject} disabled={!project.dirty}>Save</button>
        <button onClick={closeProject}>Close</button>
      </div>
    </header>
    {project.legacyLayoutDiscarded && !project.document && <div className="cutover-notice">Your sources, maps, and patches were preserved. The previous trim layout belonged to an older product model and was deliberately removed. Choose a template and create a new trim sheet.</div>}
    <main className="workbench">
      <SourceWorkspace sources={baseSources} importing={activity === "importing"} onImport={importBaseColor} />
      <Workpiece artifact={artifact} mapView={mapView} selectedRegionId={selectedRegionId} onSelect={setSelectedRegionId} buildLabel={buildLabel} problem={problem} />
      <Inspector
        project={project} templateId={templateId} setTemplateId={setTemplateId}
        primaryMaterial={effectivePrimary} setPrimaryMaterial={choosePrimary}
        selectedRegion={selectedRegion} artifact={artifact} mapView={mapView} setMapView={setMapView}
        build={build} activity={activity} setResolution={setResolution}
      />
    </main>
  </div>;
}

function Landing({ onNew, onOpen, problem }: { onNew: () => void; onOpen: () => void; problem: CommandFailure | null }) {
  return <main className="landing"><div className="landing-card">
    <span className="eyebrow">DOCUMENT-FIRST MATERIAL WORKBENCH</span>
    <h1>Build a trim sheet from material truth.</h1>
    <p>Import one Base Color, choose a template, and compile a sheet whose pixels and selectable regions come from one authoritative plan.</p>
    <div className="landing-actions"><button className="primary" onClick={onNew}>New Project</button><button onClick={onOpen}>Open Project</button></div>
    {problem && <ErrorCard problem={problem} />}
  </div></main>;
}

function SourceWorkspace({ sources, importing, onImport }: { sources: ProjectProjection["sources"]; importing: boolean; onImport: () => void }) {
  return <aside className="source-pane"><PaneHeading index="01" title="Source Workspace" />
    <div className="source-body">
      <span className="eyebrow">PRIMARY INPUT</span>
      {sources.length === 0 ? <div className="empty-source"><div className="source-icon">＋</div><strong>No Base Color</strong><p>The first image establishes a registered material and unlocks trim-sheet creation.</p></div>
        : sources.map((source) => <div className="source-card" key={source.id}><div className="source-swatch"/><div><strong>{source.displayName}</strong><small>Base Color · Registered</small></div></div>)}
      <button className="wide" onClick={onImport} disabled={importing}>{importing ? "Importing…" : sources.length ? "Replace Base Color" : "Import Base Color"}</button>
      <div className="source-note"><strong>Registered maps</strong><p>Normal, Height, Roughness, Metallic, and AO join this material through the same source set. Base Color is the only requirement for this slice.</p></div>
    </div>
  </aside>;
}

function Workpiece({ artifact, mapView, selectedRegionId, onSelect, buildLabel, problem }: {
  artifact: CompiledSheetProjection | null; mapView: CompiledMapView; selectedRegionId: string | null;
  onSelect: (id: string) => void; buildLabel: string; problem: CommandFailure | null;
}) {
  return <section className="workpiece"><PaneHeading index="02" title="Trim Sheet Workpiece" trailing={<span className={`status ${problem ? "error" : artifact ? "ok" : ""}`}>{buildLabel}</span>} />
    <div className="canvas-wrap">
      {!artifact ? <div className="canvas-empty"><div className="grid-mark"/><strong>No compiled sheet</strong><p>Import a Base Color, choose the document settings, then Build.</p></div>
        : <div className="sheet" style={{ aspectRatio: `${artifact.width}/${artifact.height}` }}>
          <img src={artifact.maps[mapView]} alt={`${mapView} compiled trim sheet`} />
          <div className="overlays">{artifact.regions.map((region) => <button
            key={region.regionId} aria-label={`Select ${region.displayName}`}
            className={region.regionId === selectedRegionId ? "region selected" : "region"}
            style={overlayStyle(region, artifact)} onClick={() => onSelect(region.regionId)}
          ><span>{region.displayName}</span></button>)}</div>
        </div>}
    </div>
    {artifact && <footer className="artifact-footer"><span>REV {artifact.documentRevision}</span><span>TOPOLOGY {artifact.topologyHash.slice(0, 10)}</span><span>APPEARANCE {artifact.appearanceHash.slice(0, 10)}</span><span>{artifact.width} × {artifact.height}</span></footer>}
  </section>;
}

function Inspector(props: {
  project: ProjectProjection; templateId: string; setTemplateId: (id: string) => void;
  primaryMaterial: string; setPrimaryMaterial: (id: string) => void; selectedRegion: ResolvedRegion | null;
  artifact: CompiledSheetProjection | null; mapView: CompiledMapView; setMapView: (view: CompiledMapView) => void;
  build: () => void; activity: Activity; setResolution: (size: number) => void;
}) {
  const document = props.project.document;
  const binding = props.selectedRegion && document?.regionBindings[props.selectedRegion.regionId];
  const materialOptions = useMemo(() => props.project.materialSources.filter((material) =>
    material.registeredChannels?.channels.some((source) => source.channel === "base_color")), [props.project]);
  return <aside className="inspector"><PaneHeading index="03" title="Context Inspector" />
    <div className="inspector-body">
      <label>Template<select value={props.templateId} onChange={(event) => props.setTemplateId(event.target.value)} disabled={!!document}>{templates.map(([id, name]) => <option key={id} value={id}>{name}</option>)}</select></label>
      <label>Primary Material<select value={props.primaryMaterial} onChange={(event) => props.setPrimaryMaterial(event.target.value)} disabled={!materialOptions.length}>{materialOptions.map((set) => <option key={set.id} value={set.id}>{set.name}</option>)}</select></label>
      <label>Resolution<select value={document?.renderSettings.outputSize.width ?? 2048} onChange={(event) => props.setResolution(Number(event.target.value))} disabled={!document}><option value={1024}>1024</option><option value={2048}>2048</option><option value={4096}>4096</option></select></label>
      <button className="primary build" onClick={props.build} disabled title="Unavailable until Stage 1 installs the first engine route">Engine unavailable</button>
      <div className="divider" />
      <span className="eyebrow">OUTPUT VIEW</span><div className="map-grid">{mapViews.map(([id, name]) => <button key={id} className={props.mapView === id ? "active" : ""} onClick={() => props.setMapView(id)} disabled={!props.artifact}>{name}</button>)}</div>
      <div className="divider" />
      {props.selectedRegion ? <section className="region-inspector"><span className="eyebrow">SELECTED REGION</span><h2>{props.selectedRegion.displayName}</h2><code>{props.selectedRegion.regionId}</code>
        <dl><dt>Content</dt><dd>{contentLabel(binding?.content.type)}</dd><dt>Projection</dt><dd>{binding?.mapping.projection.type ?? "—"}</dd><dt>Bounds</dt><dd>{boundsLabel(props.selectedRegion.allocationBounds)}</dd><dt>Material</dt><dd>{props.selectedRegion.materialId.slice(0, 8)}</dd></dl>
      </section> : <div className="selection-empty"><strong>No region selected</strong><p>Select a compiled region to inspect its exact identity, content binding, mapping, and bounds.</p></div>}
      <LockedSection title="Mapping & Warp" reason="Available after a region is selected and the direct-manipulation transaction milestone is enabled." />
      <LockedSection title="Profiles & Weathering" reason="Requires generated-map recipes and nondestructive treatment layers." />
      <LockedSection title="Decorations" reason="Requires an authored patch or procedural detail source." />
      <LockedSection title="Send to Blender" reason="Requires a completed compiled artifact and an installed Blender companion." />
    </div>
  </aside>;
}

function LockedSection({ title, reason }: { title: string; reason: string }) { return <section className="locked"><div><strong>{title}</strong><span>Later milestone</span></div><p>{reason}</p></section>; }
function PaneHeading({ index, title, trailing }: { index: string; title: string; trailing?: React.ReactNode }) { return <div className="pane-heading"><div><span>{index}</span><h2>{title}</h2></div>{trailing}</div>; }
function ErrorCard({ problem }: { problem: CommandFailure }) { return <div className="error-card"><strong>{problem.message}</strong><span>{problem.recovery}</span></div>; }
function boundsLabel(bounds: { x: number; y: number; width: number; height: number }) { return `${bounds.x}, ${bounds.y} · ${bounds.width}×${bounds.height}`; }
function contentLabel(type?: string) { return type === "inherit_primary_material" ? "Primary Material" : type?.replaceAll("_", " ") ?? "—"; }
function overlayStyle(region: ResolvedRegion, artifact: CompiledSheetProjection): React.CSSProperties { const b = region.allocationBounds; return { left: `${b.x / artifact.width * 100}%`, top: `${b.y / artifact.height * 100}%`, width: `${b.width / artifact.width * 100}%`, height: `${b.height / artifact.height * 100}%` }; }

createRoot(document.getElementById("root")!).render(<React.StrictMode><App /></React.StrictMode>);
