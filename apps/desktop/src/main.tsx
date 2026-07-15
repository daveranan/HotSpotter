import React, { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  IPC_PROTOCOL_VERSION,
  type CloseProjectRequest,
  type CommandFailure,
  type CreateProjectRequest,
  type FoundationStatusRequest,
  type ImportSourceRequest,
  type ProjectPathRequest,
  type ProjectSnapshot,
  type RecentProject,
  type RecoverProjectRequest,
  type RecoveryCandidate,
  type SourceChannel,
  type SourceOwnership,
  type SourceSnapshot,
  type StartupStatus,
} from "@hot-trimmer/ipc-contracts";
import "../styles.css";

const workflow = [
  "Open Image",
  "Mark Patches",
  "Layout",
  "Generate Maps",
  "Polish",
  "Preview",
  "Export",
] as const;

const channelOptions: ReadonlyArray<{ value: SourceChannel; label: string }> = [
  { value: "base_color", label: "Base Color" },
  { value: "normal", label: "Normal" },
  { value: "height", label: "Height" },
  { value: "roughness", label: "Roughness" },
  { value: "metallic", label: "Metallic" },
  { value: "ambient_occlusion", label: "Ambient Occlusion" },
];

interface ViewTransform { x: number; y: number; scale: number }
interface PixelInspection { x: number; y: number; r: number; g: number; b: number; a: number }
interface RunResult<T> { ok: boolean; value?: T }
interface ImportProgress { stage: string; fraction: number }

function isNativeRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

function failureFrom(reason: unknown, fallback: string): CommandFailure {
  if (typeof reason === "object" && reason !== null) {
    const candidate = reason as Partial<CommandFailure>;
    if (typeof candidate.message === "string" && typeof candidate.recovery === "string") {
      return {
        code: typeof candidate.code === "string" ? candidate.code : "native_command_failed",
        message: candidate.message,
        recovery: candidate.recovery,
        detail: candidate.detail,
      };
    }
  }
  return {
    code: "native_command_failed",
    message: fallback,
    recovery: "Retry the operation. Restart Hot Trimmer if the problem continues.",
    detail: reason instanceof Error ? reason.message : String(reason),
  };
}

function channelLabel(channel: SourceChannel): string {
  return channelOptions.find((option) => option.value === channel)?.label ?? channel;
}

function App(): React.JSX.Element {
  const [project, setProject] = useState<ProjectSnapshot | null>(null);
  const [recentProjects, setRecentProjects] = useState<RecentProject[]>([]);
  const [recoveries, setRecoveries] = useState<RecoveryCandidate[]>([]);
  const [showRecents, setShowRecents] = useState(false);
  const [showRecovery, setShowRecovery] = useState(false);
  const [showDirtyPrompt, setShowDirtyPrompt] = useState(false);
  const [failure, setFailure] = useState<CommandFailure | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [importProgress, setImportProgress] = useState<ImportProgress | null>(null);
  const [ownership, setOwnership] = useState<SourceOwnership>("owned_copy");
  const [importChannel, setImportChannel] = useState<SourceChannel>("base_color");
  const [selectedChannel, setSelectedChannel] = useState<SourceChannel>("base_color");
  const [view, setView] = useState<ViewTransform>({ x: 0, y: 0, scale: 1 });
  const [pixel, setPixel] = useState<PixelInspection | null>(null);
  const drag = useRef<{ pointerId: number; x: number; y: number; originX: number; originY: number } | null>(null);
  const imageRef = useRef<HTMLImageElement | null>(null);
  const sampleCanvas = useRef<HTMLCanvasElement | null>(null);
  const modalRef = useRef<HTMLElement | null>(null);
  const pendingAfterClose = useRef<(() => Promise<void>) | null>(null);
  const native = isNativeRuntime();
  const request = { protocolVersion: IPC_PROTOCOL_VERSION } satisfies FoundationStatusRequest;
  const selectedSource = project?.sources.find((source) => source.channel === selectedChannel)
    ?? project?.sources.find((source) => source.channel === "base_color")
    ?? project?.sources[0];
  const baseColor = project?.sources.find((source) => source.channel === "base_color");

  useEffect(() => {
    if (!native) return;
    void invoke<StartupStatus>("startup_status", { request }).then(async (status) => {
      await refreshLists();
      if (!status.previousShutdownClean) setShowRecovery(true);
    });
    void invoke<string | null>("take_pending_project_path", { request }).then((path) => {
      if (path) void requestReplacement(() => openProjectAt(path));
    });
  }, [native]);

  useEffect(() => {
    if (!native) return;
    let removeDrop: (() => void) | undefined;
    let removeRoute: (() => void) | undefined;
    let removeMenu: (() => void) | undefined;
    let removeProgress: (() => void) | undefined;
    void getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type !== "drop") return;
      const path = event.payload.paths[0];
      if (!path) return;
      if (path.toLowerCase().endsWith(".hottrimmer")) {
        void requestReplacement(() => openProjectAt(path));
      } else {
        void importImage(path);
      }
    }).then((unlisten) => { removeDrop = unlisten; });
    void listen<string>("open-project-requested", (event) => {
      void requestReplacement(() => openProjectAt(event.payload));
    }).then((unlisten) => { removeRoute = unlisten; });
    void listen<string>("menu-action", (event) => {
      switch (event.payload) {
        case "new_project": void requestReplacement(createProject); break;
        case "open_project": void requestReplacement(chooseProject); break;
        case "save_project": void saveProject(); break;
        case "save_project_as": void saveProjectAs(); break;
        case "close_project": void requestCloseProject(); break;
        case "reveal_project": if (project) void revealItemInDir(project.path); break;
        case "show_recovery": setShowRecovery(true); break;
      }
    }).then((unlisten) => { removeMenu = unlisten; });
    void listen<ImportProgress>("import-progress", (event) => {
      setImportProgress(event.payload);
    }).then((unlisten) => { removeProgress = unlisten; });
    return () => { removeDrop?.(); removeRoute?.(); removeMenu?.(); removeProgress?.(); };
  }, [native, ownership, importChannel, project]);

  useEffect(() => {
    if (!native) return;
    let removeClose: (() => void) | undefined;
    void getCurrentWindow().onCloseRequested((event) => {
      if (!project?.dirty) return;
      event.preventDefault();
      pendingAfterClose.current = async () => { await getCurrentWindow().destroy(); };
      setShowDirtyPrompt(true);
    }).then((unlisten) => { removeClose = unlisten; });
    return () => removeClose?.();
  }, [native, project?.dirty]);

  useEffect(() => {
    function keyboard(event: KeyboardEvent): void {
      if (event.defaultPrevented) return;
      const target = event.target as HTMLElement | null;
      if (target?.matches("input, select, textarea")) return;
      if (event.ctrlKey && event.key.toLowerCase() === "n") {
        event.preventDefault(); void requestReplacement(createProject);
      } else if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "s") {
        event.preventDefault(); void saveProjectAs();
      } else if (event.ctrlKey && event.key.toLowerCase() === "s") {
        event.preventDefault(); void saveProject();
      } else if (event.ctrlKey && event.key.toLowerCase() === "o") {
        event.preventDefault(); void requestReplacement(chooseProject);
      } else if (event.ctrlKey && event.key.toLowerCase() === "w") {
        event.preventDefault(); void requestCloseProject();
      } else if (event.key === "0" && selectedSource) {
        event.preventDefault(); fitView();
      } else if ((event.key === "+" || event.key === "=") && selectedSource) {
        event.preventDefault(); zoomBy(1.25);
      } else if (event.key === "-" && selectedSource) {
        event.preventDefault(); zoomBy(0.8);
      }
    }
    window.addEventListener("keydown", keyboard);
    return () => window.removeEventListener("keydown", keyboard);
  }, [project, selectedSource]);

  useEffect(() => {
    if (!showDirtyPrompt && !showRecovery) return;
    const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const focusable = (): HTMLButtonElement[] => Array.from(
      modalRef.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [],
    );
    focusable()[0]?.focus();
    function containFocus(event: KeyboardEvent): void {
      if (event.key === "Escape") {
        event.preventDefault();
        if (showDirtyPrompt) {
          pendingAfterClose.current = null;
          setShowDirtyPrompt(false);
        } else {
          setShowRecovery(false);
        }
        return;
      }
      if (event.key !== "Tab") return;
      const controls = focusable();
      if (!controls.length) return;
      const first = controls[0];
      const last = controls[controls.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last?.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first?.focus();
      }
    }
    document.addEventListener("keydown", containFocus);
    return () => {
      document.removeEventListener("keydown", containFocus);
      previousFocus?.focus();
    };
  }, [showDirtyPrompt, showRecovery]);

  async function run<T>(label: string, operation: () => Promise<T>): Promise<RunResult<T>> {
    setBusy(label); setFailure(null);
    try { return { ok: true, value: await operation() }; }
    catch (reason) { setFailure(failureFrom(reason, `${label} failed.`)); return { ok: false }; }
    finally { setBusy(null); }
  }

  async function refreshLists(): Promise<void> {
    if (!native) return;
    const [recent, recovery] = await Promise.all([
      invoke<RecentProject[]>("list_recent_projects", { request }).catch(() => []),
      invoke<RecoveryCandidate[]>("list_recovery_candidates", { request }).catch(() => []),
    ]);
    setRecentProjects(recent); setRecoveries(recovery);
  }

  async function requestReplacement(action: () => Promise<void>): Promise<void> {
    if (project?.dirty) {
      pendingAfterClose.current = action;
      setShowDirtyPrompt(true);
      return;
    }
    if (project) {
      const closed = await closeCurrent("discard");
      if (!closed) return;
    }
    await action();
  }

  async function resolveDirty(disposition: "save" | "discard"): Promise<void> {
    setShowDirtyPrompt(false);
    const next = pendingAfterClose.current;
    pendingAfterClose.current = null;
    const closed = await closeCurrent(disposition);
    if (closed && next) await next();
  }

  async function createProject(): Promise<void> {
    const chosen = await run("New project dialog", () => save({
      title: "New Hot Trimmer project", defaultPath: "Untitled.hottrimmer",
      filters: [{ name: "Hot Trimmer Project", extensions: ["hottrimmer"] }],
    }));
    if (!chosen.ok || !chosen.value) return;
    const path = chosen.value;
    const fileName = path.split(/[\\/]/).at(-1) ?? "Untitled";
    const createRequest: CreateProjectRequest = {
      protocolVersion: IPC_PROTOCOL_VERSION, path, name: fileName.replace(/\.hottrimmer$/i, ""),
    };
    const result = await run("Create project", () => invoke<ProjectSnapshot>("create_project", { request: createRequest }));
    if (result.ok && result.value) acceptProject(result.value);
  }

  async function chooseProject(): Promise<void> {
    const chosen = await run("Open project dialog", () => open({
      title: "Open Hot Trimmer project", multiple: false, directory: false,
      filters: [{ name: "Hot Trimmer Project", extensions: ["hottrimmer"] }],
    }));
    if (chosen.ok && chosen.value) await openProjectAt(chosen.value);
  }

  async function openProjectAt(path: string): Promise<void> {
    const openRequest: ProjectPathRequest = { protocolVersion: IPC_PROTOCOL_VERSION, path };
    const result = await run("Open project", () => invoke<ProjectSnapshot>("open_project", { request: openRequest }));
    if (result.ok && result.value) acceptProject(result.value);
  }

  function acceptProject(snapshot: ProjectSnapshot): void {
    setProject(snapshot); setSelectedChannel(snapshot.sources[0]?.channel ?? "base_color");
    setImportChannel(snapshot.sources.some((source) => source.channel === "base_color") ? "normal" : "base_color");
    fitView(); setShowRecents(false); setShowRecovery(false); void refreshLists();
    if (snapshot.staleLockRecovered) {
      setFailure({ code: "stale_lock_recovered", message: "A stale project lock was recovered.", recovery: "Review the reopened project, then save normally." });
    }
  }

  async function chooseImage(): Promise<void> {
    const chosen = await run("Image dialog", () => open({
      title: `Import ${channelLabel(importChannel)} source`, multiple: false, directory: false,
      filters: [{ name: "Source image", extensions: ["png", "jpg", "jpeg", "tif", "tiff"] }],
    }));
    if (chosen.ok && chosen.value) await importImage(chosen.value);
  }

  async function importImage(path: string): Promise<void> {
    if (!project) {
      setFailure({ code: "no_open_project", message: "Create or open a project before importing an image.", recovery: "Use New or Open, then drop the image again." });
      return;
    }
    const importRequest: ImportSourceRequest = {
      protocolVersion: IPC_PROTOCOL_VERSION, path, ownership, channel: importChannel,
    };
    setImportProgress({ stage: "Preparing", fraction: 0 });
    const result = await run("Import image", () => invoke<ProjectSnapshot>("import_source", { request: importRequest }));
    setImportProgress(null);
    if (result.ok && result.value) {
      setProject(result.value); setSelectedChannel(importChannel); fitView(); void refreshLists();
    }
  }

  async function cancelImport(): Promise<void> {
    await invoke<void>("cancel_import", { request }).catch((reason) => {
      setFailure(failureFrom(reason, "Cancel import failed."));
    });
  }

  async function saveProject(): Promise<void> {
    if (!project) return;
    const result = await run("Save project", () => invoke<ProjectSnapshot>("save_project", { request }));
    if (result.ok && result.value) { setProject(result.value); void refreshLists(); }
  }

  async function saveProjectAs(): Promise<void> {
    if (!project) return;
    const chosen = await run("Save As dialog", () => save({
      title: "Save Hot Trimmer project as", defaultPath: `${project.name} Copy.hottrimmer`,
      filters: [{ name: "Hot Trimmer Project", extensions: ["hottrimmer"] }],
    }));
    if (!chosen.ok || !chosen.value) return;
    const saveRequest: ProjectPathRequest = { protocolVersion: IPC_PROTOCOL_VERSION, path: chosen.value };
    const result = await run("Save project as", () => invoke<ProjectSnapshot>("save_project_as", { request: saveRequest }));
    if (result.ok && result.value) acceptProject(result.value);
  }

  async function closeCurrent(disposition: "save" | "discard"): Promise<boolean> {
    const closeRequest: CloseProjectRequest = { protocolVersion: IPC_PROTOCOL_VERSION, disposition };
    const result = await run("Close project", () => invoke<void>("close_project", { request: closeRequest }));
    if (result.ok) { setProject(null); setPixel(null); fitView(); }
    return result.ok;
  }

  async function requestCloseProject(): Promise<void> {
    if (!project) return;
    if (project.dirty) {
      pendingAfterClose.current = async () => {};
      setShowDirtyPrompt(true);
    } else {
      await closeCurrent("discard");
    }
  }

  async function recover(candidate: RecoveryCandidate): Promise<void> {
    const chosen = await run("Recovery destination dialog", () => save({
      title: `Recover ${candidate.projectName} as`, defaultPath: `${candidate.projectName} Recovered.hottrimmer`,
      filters: [{ name: "Hot Trimmer Project", extensions: ["hottrimmer"] }],
    }));
    if (!chosen.ok || !chosen.value) return;
    const recoverRequest: RecoverProjectRequest = {
      protocolVersion: IPC_PROTOCOL_VERSION, recoveryPath: candidate.path, destinationPath: chosen.value,
    };
    const result = await run("Recover project", () => invoke<ProjectSnapshot>("recover_project", { request: recoverRequest }));
    if (result.ok && result.value) acceptProject(result.value);
  }

  function fitView(): void { setView({ x: 0, y: 0, scale: 1 }); setPixel(null); }
  function zoomBy(multiplier: number): void {
    setView((current) => ({ ...current, scale: Math.min(8, Math.max(0.1, current.scale * multiplier)) }));
  }
  function pointerDown(event: React.PointerEvent<HTMLDivElement>): void {
    if (!selectedSource || event.button !== 0) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { pointerId: event.pointerId, x: event.clientX, y: event.clientY, originX: view.x, originY: view.y };
  }
  function pointerMove(event: React.PointerEvent<HTMLDivElement>): void {
    const active = drag.current;
    if (active?.pointerId === event.pointerId) {
      setView((current) => ({ ...current, x: active.originX + event.clientX - active.x, y: active.originY + event.clientY - active.y }));
      return;
    }
    inspectPixel(event.clientX, event.clientY);
  }
  function pointerUp(event: React.PointerEvent<HTMLDivElement>): void {
    if (drag.current?.pointerId === event.pointerId) drag.current = null;
  }
  function prepareSampler(): void {
    const image = imageRef.current;
    if (!image) return;
    const canvas = sampleCanvas.current ?? document.createElement("canvas");
    canvas.width = image.naturalWidth; canvas.height = image.naturalHeight;
    canvas.getContext("2d", { willReadFrequently: true })?.drawImage(image, 0, 0);
    sampleCanvas.current = canvas;
  }
  function inspectPixel(clientX: number, clientY: number): void {
    const image = imageRef.current; const canvas = sampleCanvas.current;
    if (!image || !canvas || !selectedSource) return;
    const rect = image.getBoundingClientRect();
    const nx = (clientX - rect.left) / rect.width; const ny = (clientY - rect.top) / rect.height;
    if (nx < 0 || nx >= 1 || ny < 0 || ny >= 1) { setPixel(null); return; }
    const sampleX = Math.min(canvas.width - 1, Math.floor(nx * canvas.width));
    const sampleY = Math.min(canvas.height - 1, Math.floor(ny * canvas.height));
    const data = canvas.getContext("2d", { willReadFrequently: true })?.getImageData(sampleX, sampleY, 1, 1).data;
    if (!data) return;
    setPixel({ x: Math.floor(nx * selectedSource.width), y: Math.floor(ny * selectedSource.height), r: data[0] ?? 0, g: data[1] ?? 0, b: data[2] ?? 0, a: data[3] ?? 0 });
  }

  const activeStep = baseColor ? 1 : 0;
  const mip = selectedSource?.thumbnailMipmaps.find((level) => view.scale <= 0.55 ? level.maxEdge === 320 : view.scale <= 1.5 ? level.maxEdge === 640 : level.maxEdge === 1280);
  const imageUrl = mip?.dataUrl ?? selectedSource?.thumbnailDataUrl;

  return (
    <main className="app-shell" aria-label="Hot Trimmer desktop workspace">
      <header className="topbar">
        <strong className="brand">Hot Trimmer</strong>
        <div className="project-actions" aria-label="Project actions">
          <button onClick={() => void requestReplacement(createProject)} disabled={!native || busy !== null} title="New project (Ctrl+N)">New</button>
          <button onClick={() => void requestReplacement(chooseProject)} disabled={!native || busy !== null} title="Open project (Ctrl+O)">Open</button>
          <div className="menu-anchor">
            <button onClick={() => setShowRecents((shown) => !shown)} disabled={!native || busy !== null} aria-expanded={showRecents}>Recent</button>
            {showRecents ? <div className="popup-menu" role="menu">{recentProjects.length ? recentProjects.map((recent) => <button key={recent.path} role="menuitem" disabled={!recent.available} onClick={() => void requestReplacement(() => openProjectAt(recent.path))}><strong>{recent.name}</strong><small>{recent.available ? recent.path : "Unavailable"}</small></button>) : <span>No recent projects</span>}</div> : null}
          </div>
          <button onClick={() => void saveProject()} disabled={!project || !project.dirty || busy !== null} title="Save (Ctrl+S)">Save</button>
          <button onClick={() => void saveProjectAs()} disabled={!project || busy !== null} title="Save As (Ctrl+Shift+S)">Save As</button>
          <button onClick={() => void requestCloseProject()} disabled={!project || busy !== null} title="Close project (Ctrl+W)">Close</button>
          <button onClick={() => void revealItemInDir(project?.path ?? "").catch((reason) => setFailure(failureFrom(reason, "Reveal in folder failed.")))} disabled={!project || busy !== null}>Reveal</button>
        </div>
        <nav className="workflow" aria-label="MVP workflow">
          {workflow.map((step, index) => <button key={step} className={`step ${index === activeStep ? "active" : ""} ${index < activeStep ? "complete" : ""}`} aria-current={index === activeStep ? "step" : undefined} disabled={index !== activeStep} title={index > activeStep ? "Available after the preceding workflow steps" : undefined}><span>{index + 1}</span>{step}</button>)}
        </nav>
      </header>

      <aside className="tools" aria-label="Viewport tools">
        <button title="Pan by dragging" className="tool active" disabled={!selectedSource}>Pan</button>
        <button title="Zoom in (+)" className="tool" onClick={() => zoomBy(1.25)} disabled={!selectedSource}>+</button>
        <button title="Zoom out (-)" className="tool" onClick={() => zoomBy(0.8)} disabled={!selectedSource}>−</button>
        <button title="Fit source (0)" className="tool" onClick={fitView} disabled={!selectedSource}>Fit</button>
      </aside>

      <section className="workspace" aria-labelledby="workspace-title">
        <header className="panel-title"><div><span className="eyebrow">{selectedSource ? channelLabel(selectedSource.channel) : "Source workspace"}</span><strong id="workspace-title">{selectedSource?.displayName ?? "No source image"}</strong></div><div className="zoom-readout" aria-live="polite">{Math.round(view.scale * 100)}%</div></header>
        <div className={`viewport ${selectedSource ? "has-source" : ""}`} onPointerDown={pointerDown} onPointerMove={pointerMove} onPointerUp={pointerUp} onPointerCancel={pointerUp} onPointerLeave={() => setPixel(null)} onWheel={(event) => { if (!selectedSource) return; event.preventDefault(); zoomBy(event.deltaY < 0 ? 1.1 : 0.9); }}>
          {selectedSource && imageUrl ? <img ref={imageRef} src={imageUrl} alt={`${channelLabel(selectedSource.channel)} source ${selectedSource.displayName}`} draggable={false} onLoad={prepareSampler} style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.scale})` }} /> : <div className="empty-state"><span className="empty-icon" aria-hidden="true">▧</span><h1>{project ? "Import Base Color" : "Open a source image"}</h1><p>{project ? "Import a PNG, JPEG, or TIFF to begin the registered material source set." : "Create a durable project first, then import a Base Color source."}</p><div className="empty-actions">{project ? <button className="primary" onClick={() => void chooseImage()} disabled={busy !== null}>Import Base Color</button> : <><button className="primary" onClick={() => void requestReplacement(createProject)} disabled={!native || busy !== null}>New Project</button><button onClick={() => void requestReplacement(chooseProject)} disabled={!native || busy !== null}>Open Project</button></>}</div><small>{native ? "Project files and source images can also be dropped here." : "Native actions are available in the desktop build."}</small></div>}
          {busy ? <div className="busy" role="status"><span /> <div><strong>{importProgress?.stage ?? busy}…</strong>{importProgress ? <progress max={1} value={importProgress.fraction} aria-label="Image import progress" /> : null}</div>{busy === "Import image" ? <button onClick={() => void cancelImport()}>Cancel</button> : null}</div> : null}
          {pixel ? <output className="pixel-readout" aria-live="polite">x {pixel.x} y {pixel.y} · RGBA {pixel.r}, {pixel.g}, {pixel.b}, {pixel.a}</output> : null}
        </div>
      </section>

      <aside className="inspector" aria-label="Project and source inspector">
        <header className="panel-title"><strong>Inspector</strong>{recoveries.length ? <button className="recovery-button" onClick={() => setShowRecovery(true)}>{recoveries.length} recovery</button> : null}</header>
        <section className="inspector-section"><h2>Project</h2>{project ? <dl className="facts"><div><dt>Name</dt><dd>{project.name}{project.dirty ? " *" : ""}</dd></div><div><dt>Schema</dt><dd>v{project.schemaVersion}</dd></div><div><dt>Status</dt><dd className={project.dirty ? "pending" : "good"}>{project.dirty ? "Autosaved · unsaved" : "Saved"}</dd></div><div><dt>Sources</dt><dd>{project.sources.length}</dd></div></dl> : <p className="muted">No project is open.</p>}</section>
        <section className="inspector-section"><h2>Registered source</h2>
          <label className="field"><span>Channel</span><select value={importChannel} onChange={(event) => setImportChannel(event.target.value as SourceChannel)}>{channelOptions.map((option) => <option key={option.value} value={option.value} disabled={option.value !== "base_color" && !baseColor}>{option.label}</option>)}</select></label>
          <label className="field"><span>Ownership</span><select value={ownership} onChange={(event) => setOwnership(event.target.value as SourceOwnership)}><option value="owned_copy">Owned project copy</option><option value="verified_external_reference">Verified external reference</option></select></label>
          <p className="hint">{ownership === "owned_copy" ? "Immutable bytes are stored inside the project." : "The external file is SHA-256 verified whenever the project opens."}</p>
          <button className="wide primary" onClick={() => void chooseImage()} disabled={!project || busy !== null}>{project?.sources.some((source) => source.channel === importChannel) ? `Replace ${channelLabel(importChannel)}…` : `Import ${channelLabel(importChannel)}…`}</button>
          {selectedSource ? <dl className="facts source-facts"><div><dt>Dimensions</dt><dd>{selectedSource.width} × {selectedSource.height}</dd></div><div><dt>Format</dt><dd>{selectedSource.format}</dd></div><div><dt>Color</dt><dd>{selectedSource.channel === "base_color" ? "sRGB color" : "Linear data"}</dd></div><div><dt>Alpha</dt><dd>{selectedSource.hasAlpha ? "Preserved" : "None"}</dd></div><div><dt>Orientation</dt><dd>EXIF {selectedSource.exifOrientation}</dd></div><div><dt>ICC</dt><dd>{selectedSource.iccConvertedToSrgb ? "Converted to sRGB" : selectedSource.hasEmbeddedIccProfile ? "Ignored for data" : "Not embedded"}</dd></div><div><dt>Ownership</dt><dd>{selectedSource.ownership === "owned_copy" ? "Owned copy" : "External"}</dd></div></dl> : null}
        </section>
        {failure ? <section className="error-card" role="alert"><strong>{failure.message}</strong><span>{failure.recovery}</span>{failure.detail ? <details><summary>Technical detail</summary><code>{failure.detail}</code></details> : null}</section> : null}
      </aside>

      <section className="bottom-tray" aria-label="Project asset tray">{project?.sources.map((source) => <button key={source.id} className={`tray ${source.channel === selectedSource?.channel ? "active" : ""}`} onClick={() => { setSelectedChannel(source.channel); fitView(); }}><span>{channelLabel(source.channel)}</span><small>{source.width}×{source.height}</small></button>)}<button className="tray" disabled>Patches <span>0</span></button><button className="tray" disabled>Maps <span>0</span></button><button className={failure ? "tray warning" : "tray"} disabled>Warnings <span>{failure ? 1 : 0}</span></button></section>
      <footer className="statusbar"><span>{project ? `${project.name}${project.dirty ? " *" : ""}` : "No project"}</span><span>{selectedSource ? `${channelLabel(selectedSource.channel)} · ${selectedSource.width} × ${selectedSource.height} ${selectedSource.format}` : "Awaiting source image"}</span><span>Offline</span></footer>

      {showDirtyPrompt ? <div className="modal-backdrop" role="presentation"><section ref={modalRef} className="modal" role="alertdialog" aria-modal="true" aria-labelledby="dirty-title" aria-describedby="dirty-description"><span className="modal-kicker">Unsaved changes</span><h2 id="dirty-title">Save changes to {project?.name}?</h2><p id="dirty-description">Autosave recovery exists, but closing with Discard restores the last explicit save.</p><div className="modal-actions"><button className="primary" onClick={() => void resolveDirty("save")}>Save</button><button className="danger" onClick={() => void resolveDirty("discard")}>Discard</button><button onClick={() => { pendingAfterClose.current = null; setShowDirtyPrompt(false); }}>Cancel</button></div></section></div> : null}
      {showRecovery ? <div className="modal-backdrop" role="presentation"><section ref={modalRef} className="modal recovery-modal" role="dialog" aria-modal="true" aria-labelledby="recovery-title"><span className="modal-kicker">Crash-safe snapshots</span><h2 id="recovery-title">Recovery</h2><p>Recovery always creates a new project and never overwrites the original.</p><div className="recovery-list">{recoveries.length ? recoveries.map((candidate) => <div key={candidate.path}><div><strong>{candidate.projectName}</strong><small>{new Date(candidate.modifiedUnix * 1000).toLocaleString()} · {candidate.sourceCount} sources</small></div><button onClick={() => void requestReplacement(() => recover(candidate))}>Recover As…</button></div>) : <span>No valid recovery snapshots were found.</span>}</div><div className="modal-actions"><button onClick={() => setShowRecovery(false)}>Close</button></div></section></div> : null}
    </main>
  );
}

createRoot(document.getElementById("root")!).render(<React.StrictMode><App /></React.StrictMode>);
