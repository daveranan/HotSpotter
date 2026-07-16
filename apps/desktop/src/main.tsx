import React, { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { confirm, open, save } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  IPC_PROTOCOL_VERSION,
  type CloseProjectRequest,
  type CommandFailure,
  type FoundationStatusRequest,
  type ImportSourceRequest,
  type LayoutStateSnapshot,
  type AuthoringHistorySnapshot,
  type ProjectNameRequest,
  type ProjectPathRequest,
  type ProjectSnapshot,
  type PatchStateSnapshot,
  type RecentProject,
  type RecoverProjectRequest,
  type RecoveryCandidate,
  type SourceChannel,
  type SourceSlotRequest,
  type SourceSnapshot,
  type StartupStatus,
} from "@hot-trimmer/ipc-contracts";
import { assignSourceFiles } from "./source-assignment";
import { PatchWorkspace } from "./patch-workspace";
import "../styles.css";

const workspaceModes = [
  { id: "patches", label: "Patches & Layout", available: true, detail: "Build material sources, patches, and the final trim sheet" },
  { id: "maps", label: "Layers & Maps", available: false, detail: "Nondestructive material layers and generated maps" },
] as const;

const channelOptions: ReadonlyArray<{ value: SourceChannel; label: string; short: string; tone: string; description: string }> = [
  { value: "base_color", label: "Base Color / Diffuse", short: "BC", tone: "color", description: "Color-managed surface color and the registration anchor." },
  { value: "normal", label: "Normal", short: "N", tone: "normal", description: "Tangent-space direction data; never color-corrected." },
  { value: "height", label: "Height / Bump", short: "H", tone: "height", description: "Linear grayscale surface height or bump input." },
  { value: "roughness", label: "Roughness", short: "R", tone: "roughness", description: "Linear microsurface response." },
  { value: "metallic", label: "Metallic", short: "M", tone: "metallic", description: "Explicit metal mask; never inferred silently." },
  { value: "ambient_occlusion", label: "Ambient Occlusion", short: "AO", tone: "ao", description: "Linear occlusion or cavity input." },
  { value: "specular", label: "Specular", short: "S", tone: "specular", description: "Optional explicit specular-level input." },
  { value: "opacity", label: "Opacity", short: "O", tone: "opacity", description: "Optional cutout or transparency mask." },
  { value: "edge_mask", label: "Edge Mask", short: "E", tone: "edge", description: "Optional authored edge/detail mask." },
  { value: "material_id", label: "Material ID", short: "ID", tone: "id", description: "Optional flat material-region assignment." },
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

function nextEmptyChannel(sources: SourceSnapshot[]): SourceChannel {
  return channelOptions.find((option) => !sources.some((source) => source.channel === option.value))?.value ?? "base_color";
}

function activeSourceSetId(project: ProjectSnapshot): string {
  return project.sourceSets[0]?.id ?? project.id;
}

function App(): React.JSX.Element {
  const [project, setProject] = useState<ProjectSnapshot | null>(null);
  const [recentProjects, setRecentProjects] = useState<RecentProject[]>([]);
  const [recoveries, setRecoveries] = useState<RecoveryCandidate[]>([]);
  const [showRecents, setShowRecents] = useState(false);
  const [showRecovery, setShowRecovery] = useState(false);
  const [crashRecoveryAvailable, setCrashRecoveryAvailable] = useState(false);
  const [showDirtyPrompt, setShowDirtyPrompt] = useState(false);
  const [failure, setFailure] = useState<CommandFailure | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [importProgress, setImportProgress] = useState<ImportProgress | null>(null);
  const [nameDraft, setNameDraft] = useState("");
  const [importChannel, setImportChannel] = useState<SourceChannel>("base_color");
  const [selectedChannel, setSelectedChannel] = useState<SourceChannel>("base_color");
  const [workspaceMode, setWorkspaceMode] = useState<"sources" | "patches">("patches");
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

  useEffect(() => { setNameDraft(project?.name ?? ""); }, [project?.name]);

  useEffect(() => {
    if (!native) return;
    void invoke<StartupStatus>("startup_status", { request }).then(async (status) => {
      const lists = await refreshLists();
      setCrashRecoveryAvailable(!status.previousShutdownClean && lists.recovery.length > 0);
    });
    void invoke<string | null>("take_pending_project_path", { request }).then((path) => {
      if (path) void requestReplacement(() => openProjectAt(path));
    });
  }, [native]);

  useEffect(() => {
    if (!native) return;
    let removeDrop: (() => void) | undefined;
    let removeRoute: (() => void) | undefined;
    let removeProgress: (() => void) | undefined;
    void getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type !== "drop") return;
      const paths = event.payload.paths;
      const path = paths[0];
      if (!path) return;
      if (path.toLowerCase().endsWith(".hottrimmer")) {
        void requestReplacement(() => openProjectAt(path));
      } else if (!project) {
        void requestReplacement(() => createProject(paths));
      } else {
        void importImages(paths);
      }
    }).then((unlisten) => { removeDrop = unlisten; });
    void listen<string>("open-project-requested", (event) => {
      void requestReplacement(() => openProjectAt(event.payload));
    }).then((unlisten) => { removeRoute = unlisten; });
    void listen<ImportProgress>("import-progress", (event) => {
      setImportProgress(event.payload);
    }).then((unlisten) => { removeProgress = unlisten; });
    return () => { removeDrop?.(); removeRoute?.(); removeProgress?.(); };
  }, [native, importChannel, project, showDirtyPrompt]);

  useEffect(() => {
    if (!native) return;
    let removeClose: (() => void) | undefined;
    void getCurrentWindow().onCloseRequested((event) => {
      if (!project?.dirty) return;
      event.preventDefault();
      pendingAfterClose.current = async () => { await getCurrentWindow().destroy(); };
      setShowRecovery(false);
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
        event.preventDefault(); void requestReplacement(() => createProject());
      } else if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "s") {
        event.preventDefault(); void saveProjectAs();
      } else if (event.ctrlKey && event.key.toLowerCase() === "s") {
        event.preventDefault(); void saveProject();
      } else if (event.ctrlKey && event.key.toLowerCase() === "o") {
        event.preventDefault(); void requestReplacement(chooseProject);
      } else if (event.ctrlKey && event.key.toLowerCase() === "i") {
        event.preventDefault(); if (project) void chooseImages(); else void startFromImages();
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

  async function refreshLists(): Promise<{ recent: RecentProject[]; recovery: RecoveryCandidate[] }> {
    if (!native) return { recent: [], recovery: [] };
    const [recent, recovery] = await Promise.all([
      invoke<RecentProject[]>("list_recent_projects", { request }).catch(() => []),
      invoke<RecoveryCandidate[]>("list_recovery_candidates", { request }).catch(() => []),
    ]);
    setRecentProjects(recent); setRecoveries(recovery);
    return { recent, recovery };
  }

  async function requestReplacement(action: () => Promise<void>): Promise<void> {
    if (project?.dirty) {
      pendingAfterClose.current = action;
      setShowRecovery(false);
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
    if (disposition === "save" && project?.isDraft) {
      const saved = await saveProjectAs();
      if (!saved) return;
      const closed = await closeCurrent("discard");
      if (closed && next) await next();
      return;
    }
    const closed = await closeCurrent(disposition);
    if (closed && next) await next();
  }

  async function createProject(initialImagePaths?: string[]): Promise<void> {
    const result = await run("Create draft", () => invoke<ProjectSnapshot>("create_draft_project", { request }));
    if (result.ok && result.value) {
      acceptProject(result.value);
      if (initialImagePaths?.length) await importImages(initialImagePaths, result.value);
    }
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
    setWorkspaceMode("patches");
    setImportChannel(nextEmptyChannel(snapshot.sources));
    fitView(); setShowRecents(false); setShowRecovery(false); void refreshLists();
    if (snapshot.staleLockRecovered) {
      setFailure({ code: "stale_lock_recovered", message: "A stale project lock was recovered.", recovery: "Review the reopened project, then save normally." });
    }
  }

  function acceptPatchState(state: PatchStateSnapshot | AuthoringHistorySnapshot): void {
    setProject((current) => current ? {
      ...current,
      patches: state.patches,
      ...("layout" in state ? { layout: state.layout } : {}),
      dirty: state.dirty,
      authoringRevision: state.authoringRevision,
      canUndoPatch: state.canUndoPatch,
      canRedoPatch: state.canRedoPatch,
      canUndoProject: state.canUndoProject,
      canRedoProject: state.canRedoProject,
      warnings: state.warnings,
    } : current);
  }

  function acceptLayoutState(state: LayoutStateSnapshot | AuthoringHistorySnapshot): void {
    setProject((current) => current ? {
      ...current,
      ...("patches" in state ? { patches: state.patches } : {}),
      layout: state.layout,
      dirty: state.dirty,
      authoringRevision: state.authoringRevision,
      canUndoPatch: state.canUndoPatch,
      canRedoPatch: state.canRedoPatch,
      canUndoProject: state.canUndoProject,
      canRedoProject: state.canRedoProject,
      warnings: state.warnings,
    } : current);
  }

  async function chooseImage(channelOverride?: SourceChannel, sourceSetIdOverride?: string): Promise<void> {
    const channel = channelOverride ?? importChannel;
    const chosen = await run("Image dialog", () => open({
      title: `Import ${channelLabel(channel)} source`, multiple: false, directory: false,
      filters: [{ name: "Source image", extensions: ["png", "jpg", "jpeg", "tif", "tiff"] }],
    }));
    if (chosen.ok && chosen.value) await importImage(chosen.value, undefined, channel, sourceSetIdOverride);
  }

  async function chooseImages(sourceSetIdOverride?: string): Promise<void> {
    if (!project) return;
    const chosen = await run("Open source set", () => open({
      title: "Open and auto-assign source images", multiple: true, directory: false,
      filters: [{ name: "Source images", extensions: ["png", "jpg", "jpeg", "tif", "tiff"] }],
    }));
    if (chosen.ok && chosen.value) await importImages(Array.isArray(chosen.value) ? chosen.value : [chosen.value], undefined, sourceSetIdOverride);
  }

  async function startFromImages(): Promise<void> {
    const chosen = await run("Open image dialog", () => open({
      title: "Open source images", multiple: true, directory: false,
      filters: [{ name: "Source images", extensions: ["png", "jpg", "jpeg", "tif", "tiff"] }],
    }));
    if (chosen.ok && chosen.value) {
      const imagePaths = Array.isArray(chosen.value) ? chosen.value : [chosen.value];
      await requestReplacement(() => createProject(imagePaths));
    }
  }

  async function importImages(paths: string[], projectOverride?: ProjectSnapshot, sourceSetIdOverride?: string): Promise<void> {
    const targetProject = projectOverride ?? project;
    if (!targetProject) return;
    const sourceSetId = sourceSetIdOverride ?? activeSourceSetId(targetProject);
    const setSources = targetProject.sources.filter((source) => source.sourceSetId === sourceSetId);
    const assignments = assignSourceFiles(paths, setSources.map((source) => source.channel));
    if (!assignments.length) {
      setFailure({ code: "source_map_not_identified", message: "No selected file matched an empty map slot in Material 1.", recovery: "Click an empty Base Color, Normal, Height, or other map slot to import that channel explicitly. Independent material sources arrive in Phase 3." });
      return;
    }
    setImportProgress({ stage: `Auto-assigning 1 of ${assignments.length}`, fraction: 0 });
    let snapshot = targetProject;
    let completed = 0;
    for (let index = 0; index < assignments.length; index += 1) {
      const assignment = assignments[index]!;
      setImportProgress({ stage: `Auto-assigning ${index + 1} of ${assignments.length}: ${channelLabel(assignment.channel)}`, fraction: index / assignments.length });
      const importRequest: ImportSourceRequest = {
        protocolVersion: IPC_PROTOCOL_VERSION, path: assignment.path, ownership: "owned_copy", channel: assignment.channel,
        sourceSetId,
      };
      const result = await run("Import image", () => invoke<ProjectSnapshot>("import_source", { request: importRequest }));
      if (!result.ok || !result.value) break;
      snapshot = result.value; completed += 1;
      setProject(snapshot);
    }
    setImportProgress(null);
    if (completed > 0) {
      setSelectedChannel(assignments[completed - 1]?.channel ?? "base_color");
      setImportChannel(nextEmptyChannel(snapshot.sources.filter((source) => source.sourceSetId === sourceSetId)));
      fitView(); void refreshLists();
      if (completed === assignments.length && assignments.length < paths.length) {
        setFailure({ code: "source_maps_skipped", message: `${paths.length - assignments.length} file(s) did not match an empty map slot and were not imported.`, recovery: "Use an empty channel slot for an explicit map. Phase 3 adds multiple independent material sources." });
      }
    }
  }

  async function importImage(path: string, projectOverride?: ProjectSnapshot, channelOverride?: SourceChannel, sourceSetIdOverride?: string): Promise<void> {
    const targetProject = projectOverride ?? project;
    const targetChannel = channelOverride ?? importChannel;
    if (!targetProject) {
      setFailure({ code: "no_open_project", message: "Create or open a project before importing an image.", recovery: "Use New or Open, then drop the image again." });
      return;
    }
    const importRequest: ImportSourceRequest = {
      protocolVersion: IPC_PROTOCOL_VERSION, path, ownership: "owned_copy", channel: targetChannel,
      sourceSetId: sourceSetIdOverride ?? activeSourceSetId(targetProject),
    };
    setImportProgress({ stage: "Preparing", fraction: 0 });
    const result = await run("Import image", () => invoke<ProjectSnapshot>("import_source", { request: importRequest }));
    setImportProgress(null);
    if (result.ok && result.value) {
      const importedSetId = sourceSetIdOverride ?? activeSourceSetId(targetProject);
      setProject(result.value); setSelectedChannel(targetChannel); setImportChannel(nextEmptyChannel(result.value.sources.filter((source) => source.sourceSetId === importedSetId))); fitView(); void refreshLists();
    }
  }

  function chooseSlot(channel: SourceChannel): void {
    setImportChannel(channel);
    if (project?.sources.some((source) => source.channel === channel)) {
      setSelectedChannel(channel);
      fitView();
    }
  }

  function beginRecovery(candidate: RecoveryCandidate): void {
    setShowRecovery(false);
    void requestReplacement(() => recover(candidate));
  }

  async function cancelImport(): Promise<void> {
    await invoke<void>("cancel_import", { request }).catch((reason) => {
      setFailure(failureFrom(reason, "Cancel import failed."));
    });
  }

  async function removeSelectedSource(): Promise<void> {
    if (!slotSource || !project) return;
    const accepted = await confirm(
      `Clear ${channelLabel(slotSource.channel)} from this project? The original image is never deleted.`,
      { title: "Clear material input", kind: "warning" },
    );
    if (!accepted) return;
    const removeRequest: SourceSlotRequest = {
      protocolVersion: IPC_PROTOCOL_VERSION,
      channel: slotSource.channel,
      sourceSetId: activeSourceSetId(project),
    };
    const result = await run("Clear material input", () => invoke<ProjectSnapshot>("remove_source", { request: removeRequest }));
    if (result.ok && result.value) {
      setProject(result.value);
      setSelectedChannel(result.value.sources[0]?.channel ?? "base_color");
      fitView();
      void refreshLists();
    }
  }

  async function renameProject(): Promise<void> {
    const name = nameDraft.trim();
    if (!project || name === project.name) { setNameDraft(project?.name ?? ""); return; }
    if (!name) { setNameDraft(project.name); return; }
    const renameRequest: ProjectNameRequest = { protocolVersion: IPC_PROTOCOL_VERSION, name };
    const result = await run("Rename project", () => invoke<ProjectSnapshot>("rename_project", { request: renameRequest }));
    if (result.ok && result.value) setProject(result.value);
    else setNameDraft(project.name);
  }

  async function saveProject(): Promise<boolean> {
    if (!project) return false;
    if (project.isDraft) return saveProjectAs();
    const result = await run("Save project", () => invoke<ProjectSnapshot>("save_project", { request }));
    if (result.ok && result.value) { setProject(result.value); void refreshLists(); }
    return result.ok;
  }

  async function saveProjectAs(): Promise<boolean> {
    if (!project) return false;
    const chosen = await run("Save As dialog", () => save({
      title: "Save Hot Trimmer project as", defaultPath: `${project.name}${project.isDraft ? "" : " Copy"}.hottrimmer`,
      filters: [{ name: "Hot Trimmer Project", extensions: ["hottrimmer"] }],
    }));
    if (!chosen.ok || !chosen.value) return false;
    const saveRequest: ProjectPathRequest = { protocolVersion: IPC_PROTOCOL_VERSION, path: chosen.value };
    const result = await run("Save project as", () => invoke<ProjectSnapshot>("save_project_as", { request: saveRequest }));
    if (result.ok && result.value) acceptProject(result.value);
    return result.ok;
  }

  async function clearRecovery(): Promise<void> {
    const result = await run("Clear recovery", () => invoke<void>("clear_recovery_candidates", { request }));
    if (!result.ok) return;
    setRecoveries([]);
    setCrashRecoveryAvailable(false);
    setShowRecovery(false);
  }

  async function closeCurrent(disposition: "save" | "discard"): Promise<boolean> {
    const closeRequest: CloseProjectRequest = { protocolVersion: IPC_PROTOCOL_VERSION, disposition };
    const result = await run("Close project", () => invoke<void>("close_project", { request: closeRequest }));
    if (result.ok) { setProject(null); setWorkspaceMode("patches"); setPixel(null); fitView(); }
    return result.ok;
  }

  async function requestCloseProject(): Promise<void> {
    if (!project) return;
    if (project.dirty) {
      pendingAfterClose.current = async () => {};
      setShowRecovery(false);
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

  const slotSource = project?.sources.find((source) => source.channel === importChannel);
  const selectedSlot = channelOptions.find((option) => option.value === importChannel) ?? channelOptions[0]!;
  const warningCount = (failure ? 1 : 0) + (project?.warnings.length ?? 0);
  const mip = selectedSource?.thumbnailMipmaps.find((level) => view.scale <= 0.55 ? level.maxEdge === 320 : view.scale <= 1.5 ? level.maxEdge === 640 : level.maxEdge === 1280);
  const imageUrl = mip?.dataUrl ?? selectedSource?.thumbnailDataUrl;

  return (
    <main className="app-shell" aria-label="Hot Trimmer desktop workspace">
      <header className="topbar" data-tauri-drag-region>
        <strong className="brand" data-tauri-drag-region>Hot Trimmer</strong>
        <div className="project-actions" aria-label="Project actions">
          <button onClick={() => void requestReplacement(() => createProject())} disabled={!native || busy !== null} title="New project (Ctrl+N)">New</button>
          <button onClick={() => void requestReplacement(chooseProject)} disabled={!native || busy !== null} title="Open project (Ctrl+O)">Open</button>
          <div className="menu-anchor">
            <button onClick={() => setShowRecents((shown) => !shown)} disabled={!native || busy !== null} aria-expanded={showRecents}>Recent</button>
            {showRecents ? <div className="popup-menu" role="menu">{recentProjects.some((recent) => recent.available) ? recentProjects.filter((recent) => recent.available).map((recent) => <button key={recent.path} role="menuitem" onClick={() => void requestReplacement(() => openProjectAt(recent.path))}><strong>{recent.name}</strong><small>{recent.path}</small></button>) : <span>No recent projects</span>}</div> : null}
          </div>
          <button onClick={() => void saveProject()} disabled={!project || (!project.dirty && !project.isDraft) || busy !== null} title="Save (Ctrl+S)">Save</button>
          <button onClick={() => void saveProjectAs()} disabled={!project || busy !== null} title="Save As (Ctrl+Shift+S)">Save As</button>
          <button onClick={() => void requestCloseProject()} disabled={!project || busy !== null} title="Close project (Ctrl+W)">Close</button>
          <button onClick={() => void revealItemInDir(project?.path ?? "").catch((reason) => setFailure(failureFrom(reason, "Reveal in folder failed.")))} disabled={!project || project.isDraft || busy !== null}>Reveal</button>
        </div>
        {project ? <div className="project-context" title={project.isDraft ? "Unsaved working document" : `${project.path}\nClick the name to edit`}><input aria-label="Project name" value={nameDraft} onChange={(event) => setNameDraft(event.target.value)} onBlur={() => void renameProject()} onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); else if (event.key === "Escape") { setNameDraft(project.name); event.currentTarget.blur(); } }} /><span className={project.isDraft || project.dirty ? "pending" : "saved"}>{project.isDraft ? "Draft" : project.dirty ? "Unsaved changes" : "Saved"}</span></div> : null}
        {project ? <nav className="workflow" aria-label="Work modes">
          {workspaceModes.map((mode) => {
            const unavailable = !mode.available || !project;
            return <button key={mode.id} className={`mode ${mode.id === workspaceMode ? "active" : ""}`} aria-current={mode.id === workspaceMode ? "page" : undefined} disabled={unavailable} onClick={() => { if (mode.id !== "maps") setWorkspaceMode(mode.id); }} title={mode.detail}>{mode.label}{!mode.available ? <small>Later</small> : null}</button>;
          })}
        </nav> : null}
        <span className="window-drag-space window-drag-space-main" data-tauri-drag-region aria-hidden="true" />
        {project ? <div className="publish-actions" aria-label="Output actions"><button disabled title="Export arrives in a later phase">Export <small>Later</small></button><button disabled title="Send directly to Blender arrives after export integration">Send to Blender <small>Later</small></button></div> : null}
        <span className="window-drag-space window-drag-space-tail" data-tauri-drag-region aria-hidden="true" />
        {native ? <div className="window-controls" aria-label="Window controls">
          <button aria-label="Minimize" title="Minimize" onClick={() => void getCurrentWindow().minimize()}>−</button>
          <button aria-label="Maximize or restore" title="Maximize or restore" onClick={() => void getCurrentWindow().toggleMaximize()}>□</button>
          <button className="window-close" aria-label="Close" title="Close" onClick={() => void getCurrentWindow().close()}>×</button>
        </div> : null}
      </header>

      {!project ? <section className="start-screen" aria-labelledby="start-title">
        <div className="start-content">
          <div className="start-hero">
            <span className="start-mark" aria-hidden="true">HT</span>
            <div><span className="eyebrow">Hot Trimmer</span><h1 id="start-title">Start a material workbench</h1><p>Open source images immediately, or return to a saved project. You choose a filename only when you save.</p></div>
          </div>
          <div className="start-actions">
            <button className="primary" onClick={() => void startFromImages()} disabled={!native || busy !== null}>Open images</button>
            <button onClick={() => void requestReplacement(() => createProject())} disabled={!native || busy !== null}>New blank</button>
            <button onClick={() => void requestReplacement(chooseProject)} disabled={!native || busy !== null}>Open project</button>
          </div>
          {crashRecoveryAvailable ? <section className="crash-recovery-card"><div><strong>Work from an interrupted session is available</strong><span>Recovery stays optional and always creates a separate project.</span></div><button onClick={() => setShowRecovery(true)}>Review</button><button onClick={() => void clearRecovery()}>Dismiss</button></section> : null}
          <section className="recent-section" aria-labelledby="recent-title"><div className="section-heading"><h2 id="recent-title">Recent projects</h2>{recentProjects.some((recent) => recent.available) ? <button onClick={() => void requestReplacement(chooseProject)}>Browse…</button> : null}</div>
            <div className="recent-grid">{recentProjects.some((recent) => recent.available) ? recentProjects.filter((recent) => recent.available).slice(0, 8).map((recent) => <button key={recent.path} className="recent-card" onClick={() => void requestReplacement(() => openProjectAt(recent.path))}><span className="recent-icon" aria-hidden="true">HT</span><span><strong>{recent.name}</strong><small>{recent.path}</small></span></button>) : <div className="recent-empty"><strong>No recent projects yet</strong><span>Saved projects will appear here.</span></div>}</div>
          </section>
        </div>
        {busy ? <div className="busy" role="status"><span /><strong>{importProgress?.stage ?? busy}…</strong></div> : null}
      </section> : null}

      <section className="workspace" aria-labelledby="workspace-title" hidden={!project || workspaceMode !== "sources"}>
        <header className="panel-title"><div><span className="eyebrow">{selectedSource ? channelLabel(selectedSource.channel) : "Source workspace"}</span><strong id="workspace-title">{selectedSource?.displayName ?? "No source image"}</strong></div></header>
        <div className={`viewport ${selectedSource ? "has-source" : ""}`} onPointerDown={pointerDown} onPointerMove={pointerMove} onPointerUp={pointerUp} onPointerCancel={pointerUp} onPointerLeave={() => setPixel(null)} onWheel={(event) => { if (!selectedSource) return; event.preventDefault(); zoomBy(event.deltaY < 0 ? 1.1 : 0.9); }}>
          {selectedSource && imageUrl ? <img ref={imageRef} src={imageUrl} alt={`${channelLabel(selectedSource.channel)} source ${selectedSource.displayName}`} draggable={false} onLoad={prepareSampler} style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.scale})` }} /> : <div className="empty-state"><span className="empty-icon" aria-hidden="true">▧</span><h1>{project ? "Open your material sources" : "Open images and start"}</h1><p>{project ? "Select a texture set and Hot Trimmer will auto-assign its named maps." : "Choose one or more images first; Hot Trimmer will then ask where to save the project."}</p><div className="empty-actions">{project ? <><button className="primary" onClick={() => void chooseImages()} disabled={busy !== null}>Open all</button><button onClick={() => { setImportChannel("base_color"); void chooseImage("base_color"); }} disabled={busy !== null}>Add Base Color only</button></> : <><button className="primary" onClick={() => void startFromImages()} disabled={!native || busy !== null}>Open images</button><button onClick={() => void requestReplacement(() => createProject())} disabled={!native || busy !== null}>New Empty Project</button><button onClick={() => void requestReplacement(chooseProject)} disabled={!native || busy !== null}>Open Project</button></>}</div><small>{native ? "Drop an image set here to auto-assign it, or add maps individually." : "Native actions are available in the desktop build."}</small></div>}
          {busy ? <div className="busy" role="status"><span /> <div><strong>{importProgress?.stage ?? busy}…</strong>{importProgress ? <progress max={1} value={importProgress.fraction} aria-label="Image import progress" /> : null}</div>{busy === "Import image" ? <button onClick={() => void cancelImport()}>Cancel</button> : null}</div> : null}
          {pixel ? <output className="pixel-readout" aria-live="polite">x {pixel.x} y {pixel.y} · RGBA {pixel.r}, {pixel.g}, {pixel.b}, {pixel.a}</output> : null}
          {selectedSource ? <div className="viewport-tools" aria-label="Viewport controls" onPointerDown={(event) => event.stopPropagation()}><button className="active" title="Pan by dragging">Pan</button><button title="Zoom out (-)" onClick={() => zoomBy(0.8)}>−</button><output aria-live="polite">{Math.round(view.scale * 100)}%</output><button title="Zoom in (+)" onClick={() => zoomBy(1.25)}>+</button><button title="Fit source (0)" onClick={fitView}>Fit</button></div> : null}
        </div>
      </section>

      <aside className="inspector" aria-label="Material input manager" hidden={!project || workspaceMode !== "sources"}>
        <header className="panel-title source-panel-title"><div><span className="eyebrow">Sources mode</span><strong>Material inputs</strong></div><div className="source-header-actions"><span className="slot-count">{project?.sources.length ?? 0} / {channelOptions.length}</span><button className="primary" onClick={() => void chooseImages()} disabled={!project || project.sources.length >= channelOptions.length || busy !== null}>+ Open all</button></div></header>
        <section className="source-slots" aria-label="Material input slots">
          {channelOptions.map((option) => { const source = project?.sources.find((candidate) => candidate.channel === option.value); const unavailable = option.value !== "base_color" && !baseColor; return <button key={option.value} className={`source-slot ${importChannel === option.value ? "active" : ""} ${source ? "filled" : ""}`} onClick={() => chooseSlot(option.value)} disabled={!project || unavailable} title={unavailable ? "Add Base Color first" : option.description}><span className={`channel-swatch ${option.tone}`}>{option.short}</span><span><strong>{option.label}</strong><small>{source ? source.displayName : unavailable ? "Needs Base Color" : "Empty slot"}</small></span><b aria-label={source ? "Assigned" : "Not assigned"}>{source ? "●" : "+"}</b></button>; })}
        </section>
        <section className="inspector-section slot-editor"><div className="slot-heading"><span className={`channel-swatch ${selectedSlot.tone}`}>{selectedSlot.short}</span><div><h2>{slotSource?.displayName ?? selectedSlot.label}</h2><p>{selectedSlot.label} · {selectedSlot.description}</p></div></div>
          <div className="slot-actions"><button className="wide primary" onClick={() => void chooseImage()} disabled={!project || (importChannel !== "base_color" && !baseColor) || busy !== null}>{slotSource ? `Replace ${selectedSlot.label}…` : `Add ${selectedSlot.label}…`}</button>{slotSource ? <button className="danger" onClick={() => void removeSelectedSource()} disabled={busy !== null}>Clear</button> : null}</div>
          {slotSource ? <dl className="facts source-facts"><div><dt>File</dt><dd title={slotSource.displayName}>{slotSource.displayName}</dd></div><div className="path-fact"><dt>Path</dt><dd title={slotSource.sourcePath}>{slotSource.sourcePath || "Original path unavailable"}</dd></div><div><dt>Dimensions</dt><dd>{slotSource.width} × {slotSource.height}</dd></div></dl> : <p className="empty-slot-help">Choose one image, or use Open all to assign a named texture set automatically.</p>}
        </section>
        {project?.warnings.map((warning) => <section key={`${warning.code}-${warning.message}`} className="warning-card" role="status"><strong>{warning.message}</strong><span>{warning.recovery}</span></section>)}
        {failure ? <section className="error-card" role="alert"><strong>{failure.message}</strong><span>{failure.recovery}</span>{failure.detail ? <details><summary>Technical detail</summary><code>{failure.detail}</code></details> : null}</section> : null}
      </aside>

      <section className="bottom-tray" aria-label="Imported source library" hidden={!project || workspaceMode !== "sources"}><span className="tray-label">Sources</span>{project?.sources.map((source) => { const option = channelOptions.find((candidate) => candidate.value === source.channel)!; return <button key={source.id} title={source.sourcePath || source.displayName} className={`tray tray-source ${source.channel === selectedSource?.channel ? "active" : ""}`} onClick={() => { setSelectedChannel(source.channel); setImportChannel(source.channel); fitView(); }}><span className={`channel-swatch ${option.tone}`}>{option.short}</span><span><strong>{source.displayName}</strong><small>{source.width} × {source.height}</small></span></button>; })}{project && project.sources.length < channelOptions.length ? <button className="tray add-tray" onClick={() => void chooseImages()}>+ Open all</button> : null}{warningCount ? <span className="tray warning" role="status">Warnings <b>{warningCount}</b></span> : null}</section>
      {project ? <PatchWorkspace hidden={workspaceMode !== "patches"} project={project} onPatchState={acceptPatchState} onLayoutState={acceptLayoutState} onFailure={setFailure} onAddSource={() => void chooseImage("base_color", crypto.randomUUID())} onOpenSources={(sourceSetId) => void chooseImages(sourceSetId)} onOpenSourceChannel={(channel, sourceSetId) => void chooseImage(channel, sourceSetId)} /> : null}
      {project ? <footer className="statusbar"><span>{project.name}</span><span>{project.isDraft ? "Draft · choose Save when ready" : project.dirty ? "Unsaved changes" : "Saved"}</span><span>{selectedSource ? `${channelLabel(selectedSource.channel)} · ${selectedSource.width} × ${selectedSource.height}` : "No source selected"}</span></footer> : null}

      {showDirtyPrompt ? <div className="modal-backdrop" role="presentation"><section ref={modalRef} className="modal" role="alertdialog" aria-modal="true" aria-labelledby="dirty-title" aria-describedby="dirty-description"><span className="modal-kicker">Unsaved changes</span><h2 id="dirty-title">Save changes to {project?.name}?</h2><p id="dirty-description">Autosave recovery exists, but closing with Discard restores the last explicit save.</p><div className="modal-actions"><button className="primary" onClick={() => void resolveDirty("save")}>Save</button><button className="danger" onClick={() => void resolveDirty("discard")}>Discard</button><button onClick={() => { pendingAfterClose.current = null; setShowDirtyPrompt(false); }}>Cancel</button></div></section></div> : null}
      {showRecovery ? <div className="modal-backdrop" role="presentation"><section ref={modalRef} className="modal recovery-modal" role="dialog" aria-modal="true" aria-labelledby="recovery-title"><span className="modal-kicker">Interrupted session</span><h2 id="recovery-title">Recovery</h2><p>Choose this only when you want to restore work after a crash. Recovery never overwrites a project.</p><div className="recovery-list">{recoveries.length ? recoveries.map((candidate) => <div key={candidate.path}><div><strong>{candidate.projectName}</strong><small>{new Date(candidate.modifiedUnix * 1000).toLocaleString()} · {candidate.sourceCount} inputs</small></div><button onClick={() => beginRecovery(candidate)}>Recover As…</button></div>) : <span>No valid recovery snapshots were found.</span>}</div><div className="modal-actions"><button className="danger" onClick={() => void clearRecovery()} disabled={!recoveries.length}>Clear recovery data</button><button onClick={() => setShowRecovery(false)}>Close</button></div></section></div> : null}
    </main>
  );
}

createRoot(document.getElementById("root")!).render(<React.StrictMode><App /></React.StrictMode>);
