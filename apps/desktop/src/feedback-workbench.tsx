import React, { useEffect, useMemo, useRef, useState } from "react";
import type {
  CompiledMapView,
  CommandFailure,
  EdgeDetailIntentV1,
  FeedbackComparisonMode,
  FeedbackContributionView,
  FeedbackDetailIntent,
  FeedbackPreviewProfile,
  FeedbackProfileIntent,
  FeedbackWorkbenchCommand,
  IntermediateAtlasProjection,
  ProjectProjection,
} from "@hot-trimmer/ipc-contracts";
import { EDGE_DETAIL_PRESETS, type EdgeDetailPresetName, type EdgeDetailPresetSelection, edgeDetailIntentFromPreset, edgeDetailPresetForIntent, occupancyRelationFromValue, occupancyRelations, sanitizeEdgeDetailIntent, updateFeedbackOperationIntent } from "./feedback-workbench-contract";

export const FEEDBACK_WORKBENCH_VERSION = "20A.1" as const;

export function defaultEdgeWearIntent(): EdgeDetailIntentV1 {
  return {
    schemaVersion: 1, enabled: true, ...EDGE_DETAIL_PRESETS["Soft Worn Edge"],
  };
}

export const contributionViews: readonly { value: FeedbackContributionView; label: string; map: CompiledMapView | null }[] = [
  { value: "edgeDetailCoreMask", label: "Edge Detail · Core mask", map: "edgeMask" },
  { value: "edgeDetailTransitionMask", label: "Edge Detail · Transition mask", map: "edgeMask" },
  { value: "edgeDetailFadeMask", label: "Edge Detail · Fade mask", map: "edgeMask" },
  { value: "edgeDetailCombinedMask", label: "Edge Detail · Combined Edge Mask", map: "edgeMask" },
  { value: "edgeDetailHeightContribution", label: "Edge Detail · Height contribution", map: "height" },
  { value: "edgeDetailFinalHeight", label: "Edge Detail · Final Height", map: "height" },
  { value: "edgeDetailFinalNormal", label: "Edge Detail · Final Normal", map: "normal" },
  { value: "edgeDetailBaseColorContribution", label: "Edge Detail · Base Color contribution", map: "baseColor" },
  { value: "edgeDetailRoughnessContribution", label: "Edge Detail · Roughness contribution", map: "roughness" },
  { value: "edgeDetailMetallicContribution", label: "Edge Detail · Metallic contribution", map: "metallic" },
  { value: "stage15Occupancy", label: "Stage 15 · raw occupancy", map: "ambientOcclusion" },
  { value: "stage15Height", label: "Stage 15 · raw physical Height", map: "height" },
  { value: "stage15ProfileRoute", label: "Stage 15 QA · profile route / occupancy", map: null },
  { value: "stage15Lod", label: "Stage 15 QA · LOD", map: null },
  { value: "stage15Fallback", label: "Stage 15 QA · fallback", map: null },
  { value: "stage16RegisteredMask", label: "Stage 16 · raw registered mask", map: "edgeMask" },
  { value: "stage16Height", label: "Stage 16 · raw physical Height contribution", map: "height" },
  { value: "stage16VectorNormal", label: "Stage 16 · raw vector-normal input", map: "normal" },
  { value: "stage16ScalarRoughness", label: "Stage 16 · raw Roughness contribution", map: "roughness" },
  { value: "stage16ScalarMetallic", label: "Stage 16 · raw Metallic contribution", map: "metallic" },
  { value: "stage16ScalarAmbientOcclusion", label: "Stage 16 · raw AO contribution", map: "ambientOcclusion" },
  { value: "stage16BaseColor", label: "Stage 16 · raw Base Color contribution", map: "baseColor" },
  { value: "stage16MaterialId", label: "Stage 16 · exact Material ID", map: "materialId" },
  { value: "stage16MaterialIdValidity", label: "Stage 16 · Material-ID validity", map: "materialId" },
  { value: "stage16Route", label: "Stage 16 QA · route", map: null },
  { value: "stage16Occupancy", label: "Stage 16 QA · occupancy relation", map: null },
  { value: "stage16Lod", label: "Stage 16 QA · LOD / fallback", map: null },
  { value: "stage16Scope", label: "Stage 16 QA · scope", map: null },
  { value: "stage16AssetResolution", label: "Stage 16 QA · immutable asset resolution", map: null },
];

export function visibleMapDependency(view: FeedbackContributionView): CompiledMapView | null {
  if (view === "stage15Occupancy") return "ambientOcclusion";
  if (view === "stage16RegisteredMask") return "edgeMask";
  return contributionViews.find((candidate) => candidate.value === view)?.map ?? null;
}

export function defaultFeedbackProfile(program: FeedbackProfileIntent["program"] = "convex_bevel"): FeedbackProfileIntent {
  const radial = program === "radial_disc" || program === "annulus";
  return {
    program,
    firstWidth: { unit: "meters", value: radial ? 0.006 : 0.004 },
    secondWidth: { unit: "meters", value: radial ? 0.006 : 0.004 },
    minimumFlatCenter: { unit: "meters", value: 0.001 },
    amplitude: { unit: "meters", value: 0.002 },
    angleDegrees: 45,
    innerRadius: { unit: "meters", value: program === "annulus" ? 0.018 : 0 },
    outerRadius: { unit: "meters", value: radial ? 0.04 : 0 },
    legalityPolicy: "clamp",
    lodPolicy: "auto",
    maximumSupersampling: 8,
    seed: 201520,
    customCurve: [],
  };
}

export function legalProfilePrograms(role: string): readonly FeedbackProfileIntent["program"][] {
  return role === "radial"
    ? ["flat", "radial_disc", "annulus"]
    : ["flat", "convex_bevel", "rounded_bevel", "concave_groove", "panel_frame"];
}

function targetOf(intent: FeedbackDetailIntent): string {
  return intent.kind === "definition" ? intent.value.name
    : intent.kind === "operation" ? intent.value.targetRegion
      : intent.value.operation.targetRegion;
}

export interface FeedbackWorkbenchProps {
  project: ProjectProjection | null;
  artifact: IntermediateAtlasProjection | null;
  selectedRegionId: string | null;
  selectedOperationId: string | null;
  view: FeedbackContributionView;
  profile: FeedbackPreviewProfile;
  comparisonMode: FeedbackComparisonMode;
  activeTool: "select" | "profile" | "stamp" | "stroke";
  commandBusy: boolean;
  edgeDetailError: CommandFailure | null;
  onSelectRegion: (id: string) => void;
  onSelectOperation: (id: string | null) => void;
  onView: (view: FeedbackContributionView) => void;
  onInspectView: (view: FeedbackContributionView) => void;
  onProfile: (profile: FeedbackPreviewProfile) => void;
  onComparisonMode: (mode: FeedbackComparisonMode) => void;
  onActiveTool: (tool: FeedbackWorkbenchProps["activeTool"]) => void;
  onCommand: (command: FeedbackWorkbenchCommand) => Promise<void>;
  onRequestVisibleView: () => void;
  onCreateSample: () => void;
  onUndo: () => void;
  onRedo: () => void;
}

export interface ProcessingSidebarProps {
  project: ProjectProjection | null;
  artifact: IntermediateAtlasProjection | null;
  selectedRegionId: string | null;
  commandBusy: boolean;
  edgeDetailError: CommandFailure | null;
  activeTab: "layers" | "outputs";
  onActiveTab: (tab: "layers" | "outputs") => void;
  onCommand: (command: FeedbackWorkbenchCommand) => Promise<void>;
  onRender: () => void;
  onUndo: () => void;
  onRedo: () => void;
}

type EdgeNumberKey = Exclude<{
  [K in keyof EdgeDetailIntentV1]: EdgeDetailIntentV1[K] extends number ? K : never
}[keyof EdgeDetailIntentV1], undefined>;

const primaryEdgeFields: readonly { key: EdgeNumberKey; label: string; minimum?: number; maximum?: number; step: number; millimeters?: boolean }[] = [
  { key: "wearAmount", label: "Wear Amount", minimum: 0, maximum: 1, step: 0.01 },
  { key: "intensity", label: "Intensity", minimum: 0, maximum: 1, step: 0.01 },
  { key: "edgeWidthM", label: "Edge Width", minimum: 0.01, step: 0.1, millimeters: true },
  { key: "bevelRadiusM", label: "Bevel Radius", minimum: 0, step: 0.1, millimeters: true },
  { key: "breakupAmount", label: "Breakup", minimum: 0, maximum: 1, step: 0.01 },
  { key: "heightAmplitudeM", label: "Height", step: 0.05, millimeters: true },
];

function ProcessingNumberField(props: {
  field: typeof primaryEdgeFields[number];
  value: number;
  onDraft: (value: number) => void;
  onCommit: () => void;
}) {
  const displayValue = props.field.millimeters ? props.value * 1000 : props.value;
  const [text, setText] = useState(String(displayValue));
  useEffect(() => setText(String(displayValue)), [displayValue]);
  const accept = (raw: string) => {
    setText(raw);
    const parsed = Number(raw);
    if (!raw.trim() || !Number.isFinite(parsed)) return;
    if (props.field.minimum !== undefined && parsed < props.field.minimum) return;
    if (props.field.maximum !== undefined && parsed > props.field.maximum) return;
    props.onDraft(props.field.millimeters ? parsed / 1000 : parsed);
  };
  const finish = () => props.onCommit();
  return <label className="processing-number-field">
    <span>{props.field.label}{props.field.millimeters ? <small>mm</small> : null}</span>
    <input
      aria-label={`${props.field.label}${props.field.millimeters ? " in millimeters" : ""}`}
      type="number"
      min={props.field.minimum}
      max={props.field.maximum}
      step={props.field.step}
      value={text}
      onChange={(event) => accept(event.currentTarget.value)}
      onBlur={finish}
      onPointerUp={finish}
      onKeyUp={(event) => { if (event.key === "Enter" || event.key.startsWith("Arrow")) finish(); }}
    />
  </label>;
}

export function ProcessingSidebar(props: ProcessingSidebarProps) {
  const persisted = props.project?.document?.edgeDetail;
  // Preset defaults are editor starting values, not evidence of an authored layer.
  const committed = persisted ?? { ...defaultEdgeWearIntent(), enabled: false };
  const [draft, setDraft] = useState<EdgeDetailIntentV1>(committed);
  const [preset, setPreset] = useState<EdgeDetailPresetSelection>(() => edgeDetailPresetForIntent(committed));
  useEffect(() => {
    setDraft(committed);
    setPreset(edgeDetailPresetForIntent(committed));
  }, [props.project?.id, props.project?.document?.documentRevision]);
  const updateDraft = (next: EdgeDetailIntentV1) => {
    setDraft(next);
    setPreset(edgeDetailPresetForIntent(next));
  };
  const applyDraft = () => {
    const next = sanitizeEdgeDetailIntent({ ...draft, enabled: true });
    if (props.commandBusy || !props.project?.document) return;
    updateDraft(next);
    void props.onCommand({ type: "set_edge_detail", intent: next });
  };
  const disableAppliedLayer = () => {
    if (!persisted?.enabled || props.commandBusy || !props.project?.document) return;
    const next = sanitizeEdgeDetailIntent({ ...persisted, enabled: false });
    void props.onCommand({ type: "set_edge_detail", intent: next });
  };
  const stageDraft = (intent: EdgeDetailIntentV1) => {
    const next = sanitizeEdgeDetailIntent(intent);
    updateDraft(next);
  };
  const targetRegion = committed.targetRegion
    ? props.project?.document?.topology.regions.find((region) => region.id === committed.targetRegion)
    : null;
  const targetLabel = targetRegion ? `${targetRegion.displayName} · ${targetRegion.id}` : "All regions";
  const revision = props.project?.document?.documentRevision;
  const artifactCurrent = revision !== undefined && props.artifact?.documentRevision === revision;
  const dirty = JSON.stringify(sanitizeEdgeDetailIntent(draft)) !== JSON.stringify(committed);
  const applied = !!persisted?.enabled;
  const publishDraft = () => {
    if (applied && !dirty) {
      if (!artifactCurrent) props.onRender();
      return;
    }
    applyDraft();
  };
  const outputs: readonly [string, CompiledMapView][] = [
    ["Base Color", "baseColor"], ["Edge Mask", "edgeMask"], ["Height", "height"], ["Normal", "normal"],
    ["Roughness", "roughness"], ["Metallic", "metallic"], ["AO", "ambientOcclusion"],
  ];
  return <aside className="processing-sidebar" aria-label="Processing controls">
    <div className="processing-sidebar-tabs" role="tablist">
      <button role="tab" aria-selected={props.activeTab === "layers"} className={props.activeTab === "layers" ? "active" : ""} onClick={() => props.onActiveTab("layers")}>Layers</button>
      <button role="tab" aria-selected={props.activeTab === "outputs"} className={props.activeTab === "outputs" ? "active" : ""} onClick={() => props.onActiveTab("outputs")}>Outputs</button>
    </div>
    {props.activeTab === "layers" ? <div className="processing-layers">
      <div className="layer-target"><span>Active target</span><strong>{targetLabel}</strong></div>
      <ol className="processing-layer-list">
        <li className="selected expanded">
          <header><button><span>1</span><strong>Edge Detail</strong></button><strong className={`layer-enabled-state ${applied ? "enabled" : "disabled"}`}>{applied ? "ON" : "OFF"}</strong></header>
          <div className="edge-detail-editor">
            <section className={`edge-detail-activation ${applied ? "enabled" : "disabled"}`}><div><strong>{!applied ? "Edge Detail is not applied" : dirty ? "Edge Detail has unapplied changes" : artifactCurrent ? "Edge Detail is applied" : "Rendering applied Edge Detail"}</strong><p>{applied ? `The applied layer targets ${targetLabel}. ${dirty ? "Apply Changes to publish the editor values." : artifactCurrent ? "Edge Mask, Height, and Normal contributions match this document revision." : "The last valid atlas remains visible while this revision renders."}` : "Choose the target and settings below, then apply the layer to generate its map contributions."}</p></div>{applied ? <button onClick={disableAppliedLayer} disabled={props.commandBusy}>Disable</button> : null}</section>
            <label>Target<select value={draft.targetRegion ?? "all"} onChange={(event) => stageDraft({ ...draft, enabled: true, targetRegion: event.currentTarget.value === "all" ? undefined : event.currentTarget.value })}><option value="all">All regions</option>{props.selectedRegionId ? <option value={props.selectedRegionId}>Selected region · {props.project?.document?.topology.regions.find((region) => region.id === props.selectedRegionId)?.displayName ?? props.selectedRegionId}</option> : null}{props.project?.document?.topology.regions.filter((region) => region.id !== props.selectedRegionId).map((region) => <option key={region.id} value={region.id}>{region.displayName} · {region.id}</option>)}</select></label>
            <label>Preset<select value={preset} onChange={(event) => { if (event.currentTarget.value === "Custom") return; stageDraft(edgeDetailIntentFromPreset(event.currentTarget.value as EdgeDetailPresetName, { ...draft, enabled: true })); }}><option>Custom</option>{Object.keys(EDGE_DETAIL_PRESETS).map((name) => <option key={name}>{name}</option>)}</select><small>Preset and control changes remain staged until you apply them.</small></label>
            <div className="processing-primary-controls">{primaryEdgeFields.map((field) => <ProcessingNumberField key={field.key} field={field} value={draft[field.key] as number} onDraft={(value) => updateDraft({ ...draft, enabled: true, [field.key]: value })} onCommit={() => undefined} />)}</div>
            <details><summary>Color response</summary><div className="processing-primary-controls">
              {([{ key: "hueShiftDegrees", label: "Hue", step: 1, minimum: -180, maximum: 180 }, { key: "saturationMultiplier", label: "Saturation (1 = unchanged)", step: 0.01, minimum: 0, maximum: 2 }, { key: "valueMultiplier", label: "Value (1 = unchanged)", step: 0.01, minimum: 0, maximum: 3 }] as const).map((field) => <ProcessingNumberField key={field.key} field={field} value={draft[field.key]} onDraft={(value) => updateDraft({ ...draft, enabled: true, [field.key]: value })} onCommit={() => undefined} />)}
            </div></details>
            <details><summary>Surface response</summary><div className="processing-primary-controls">
              {([{ key: "roughnessOffset", label: "Roughness offset", step: 0.01, minimum: -1, maximum: 1 }, { key: "normalDetailStrength", label: "Normal detail", step: 0.01, minimum: 0, maximum: 2 }] as const).map((field) => <ProcessingNumberField key={field.key} field={field} value={draft[field.key]} onDraft={(value) => updateDraft({ ...draft, enabled: true, [field.key]: value })} onCommit={() => undefined} />)}
              <label className="metal-intent"><input type="checkbox" checked={draft.exposedMetalEnabled} onChange={(event) => stageDraft({ ...draft, enabled: true, exposedMetalEnabled: event.currentTarget.checked, metallicOffset: event.currentTarget.checked ? draft.metallicOffset : 0 })} /> Exposed metal</label>
            </div></details>
            <details><summary>Advanced</summary><div className="processing-primary-controls">
              {([{ key: "edgeSoftness", label: "Edge Softness", step: 0.01, minimum: 0, maximum: 1 }, { key: "breakupScaleM", label: "Breakup Scale", step: 0.1, minimum: 0.01, millimeters: true }, { key: "microDetailAmount", label: "Microdetail Amount", step: 0.01, minimum: 0, maximum: 1 }, { key: "microDetailScaleM", label: "Microdetail Scale", step: 0.1, minimum: 0.01, millimeters: true }, { key: "sourceHeightInfluence", label: "Source Height", step: 0.01, minimum: 0, maximum: 1 }, { key: "sourceLuminanceInfluence", label: "Base Color bump", step: 0.01, minimum: 0, maximum: 1 }, { key: "seed", label: "Seed", step: 1, minimum: 0, maximum: 4294967295 }] as const).map((field) => <ProcessingNumberField key={field.key} field={field} value={draft[field.key]} onDraft={(value) => updateDraft({ ...draft, enabled: true, [field.key]: value })} onCommit={() => undefined} />)}
            </div></details>
            <div className="edge-detail-apply-row"><button className="primary" onClick={publishDraft} disabled={props.commandBusy || !props.project?.document || (applied && !dirty && artifactCurrent)}>{props.commandBusy ? "Applying…" : !applied ? "Apply Edge Detail" : dirty ? "Apply Changes" : artifactCurrent ? "Applied" : "Render Edge Detail"}</button><span>{dirty ? "Unapplied changes" : applied ? artifactCurrent ? `Applied · revision ${revision}` : "Applied · preview is stale" : "Not applied"}</span></div>
            {props.edgeDetailError ? <p className="typed-state failed" role="alert"><strong>{props.edgeDetailError.code}</strong> · {props.edgeDetailError.message}</p> : null}
          </div>
        </li>
      </ol>
      <div className="undo-row"><button disabled={!props.project?.canUndoDocument} onClick={props.onUndo}>Undo</button><button disabled={!props.project?.canRedoDocument} onClick={props.onRedo}>Redo</button></div>
    </div> : <div className="processing-outputs">
      <p>Generated map set · document revision {revision ?? "—"}</p>
      <ul>{outputs.map(([label, map]) => { const ready = artifactCurrent && !!props.artifact && (!!props.artifact.maps[map] || !!props.artifact.tileManifests?.[map] || (props.artifact.tileManifest?.manifest.map === map)); return <li key={map}><span>{label}</span><strong className={ready ? "ready" : "pending"}>{ready ? "Ready" : "Not ready"}</strong></li>; })}</ul>
      <fieldset><legend>Export all maps</legend><label>Resolution <output>{props.project?.document?.renderSettings.outputSize.width ?? 2048}px</output></label><label>Format <output>.hottrim package</output></label><small>Export renders every enabled output for this revision. Edge Mask is included whenever Edge Detail is enabled.</small></fieldset>
    </div>}
  </aside>;
}

export function FeedbackWorkbench(props: FeedbackWorkbenchProps) {
  const selectedRegion = props.project?.document?.topology.regions.find((region) => region.id === props.selectedRegionId);
  const compiled = props.artifact?.slots.find((slot) => slot.regionId === props.selectedRegionId);
  const [profileIntent, setProfileIntent] = useState<FeedbackProfileIntent>(() => defaultFeedbackProfile());
  const records = props.project?.feedbackAuthoring.records ?? [];
  const selectedRecord = records.find((record) => record.operationId === props.selectedOperationId);
  const assets = props.project?.materialSources.flatMap((sourceSet) =>
    (sourceSet.registeredChannels?.channels ?? []).filter((channel) => channel.channel === "base_color").map((channel) => ({
      assetId: sourceSet.id,
      version: String(sourceSet.sourceRevision),
      digest: channel.original.immutableDigest,
      kind: "registered_stamp_channels",
      label: sourceSet.name,
    })),
  ) ?? [];
  const [selectedAssetId, setSelectedAssetId] = useState<string | null>(null);
  // Prompt LIB is a chooser, not an implicit Base Color fallback. The selected
  // immutable asset is included in every typed detail command.
  const asset = assets.find((candidate) => candidate.assetId === selectedAssetId) ?? null;
  const operation = selectedRecord?.intent.kind === "operation" ? selectedRecord.intent.value
    : selectedRecord?.intent.kind === "stroke" ? selectedRecord.intent.value.operation : null;
  const definition = selectedRecord?.intent.kind === "definition" ? selectedRecord.intent.value : null;
  const definitions = records.filter((record) => record.intent.kind === "definition" && targetOf(record.intent) === props.selectedRegionId);
  const availablePrograms = legalProfilePrograms(selectedRegion?.role ?? "planar");
  const viewDependency = visibleMapDependency(props.view);
  const installedSummary = useMemo(() => [
    ["15", compiled?.compiledProfile ? "Executed" : "InstalledNotRequested"],
    ["16", compiled?.compiledDetails ? (records.length ? "Executed" : "SkippedBecauseUnused") : "InstalledNotRequested"],
    ["18", "NotInstalled"], ["17", "NotInstalled"], ["19", "NotInstalled"], ["20", "NotInstalled"],
  ] as const, [compiled, records.length]);

  function changeProgram(program: FeedbackProfileIntent["program"]) {
    setProfileIntent((current) => ({ ...defaultFeedbackProfile(program), seed: current.seed }));
  }

  async function addDefinition() {
    if (!props.selectedRegionId || !asset) return;
    const role = selectedRegion?.role ?? "planar";
    await props.onCommand({
      type: "upsert_detail", enabled: true, intent: { kind: "definition", value: {
        name: props.selectedRegionId, family: role === "radial" ? "radial_detail" : "panel_stamp",
        physicalSize: [0.03, 0.03], scaleSpace: "world", compatibleRoles: [role], orientation: role === "radial" ? "radial" : "slot",
        explicitRotationDegrees: 0, aspectLimits: [0.25, 4], minimumPixels: [2, 2], fitPolicy: "contain", mappingMode: role === "radial" ? "polar_authored" : "planar",
        channels: [{ channel: "height", amount: 0.0015, blend: "add", metallicExplicit: false }], fallback: "normal_only",
        provenance: `Prompt 20A registered asset ${asset.assetId}@${asset.version}`, seed: 201516, requiredSources: [asset], requiredHaloPx: 2, dependencies: [],
      } },
    });
  }

  async function placeStamp() {
    if (!props.selectedRegionId || !asset || definitions.length === 0) return;
    await props.onCommand({
      type: "upsert_detail", enabled: true, intent: { kind: "operation", value: {
        asset, scope: "material_reusable_atlas", targetRegion: props.selectedRegionId,
        physicalPositionM: [0.05, 0.05], physicalSizeM: [0.03, 0.03], pivot: [0.5, 0.5], rotationDegrees: 0, mirror: [false, false],
        opacity: 1, blend: "add", clipping: "contain", seed: 201520, spacingM: [0.04, 0.04], scatter: 0, jitterM: [0, 0], layerOrder: records.length,
        occupancy: "only_flat_center", channels: [
          { channel: "height", amount: 0.0015, blend: "add", metallicExplicit: false },
          { channel: "material_id", amount: 1, blend: "replace", materialId: 0, metallicExplicit: false },
        ],
      } },
    });
  }

  async function placeStroke() {
    if (!props.selectedRegionId || !asset || definitions.length === 0) return;
    const base = operation ?? {
      asset, scope: "material_reusable_atlas" as const, targetRegion: props.selectedRegionId,
      physicalPositionM: [0.02, 0.02] as const, physicalSizeM: [0.02, 0.02] as const, pivot: [0.5, 0.5] as const,
      rotationDegrees: 0, mirror: [false, false] as const, opacity: 1, blend: "add" as const, clipping: "contain" as const,
      seed: 201520, spacingM: [0.015, 0.015] as const, scatter: 0, jitterM: [0, 0] as const, layerOrder: records.length,
      occupancy: "only_flat_center" as const, channels: [{ channel: "height", amount: 0.0015, blend: "add" as const, metallicExplicit: false }],
    };
    await props.onCommand({ type: "upsert_detail", enabled: true, intent: { kind: "stroke", value: {
      operation: { ...base, asset, targetRegion: props.selectedRegionId, layerOrder: records.length },
      physicalSamplesM: [[0.02, 0.02], [0.04, 0.03], [0.06, 0.02]],
    } } });
  }

  async function moveSelected(direction: -1 | 1) {
    if (!selectedRecord) return;
    const ids = records.map((record) => record.operationId);
    const from = ids.indexOf(selectedRecord.operationId);
    const to = from + direction;
    if (from < 0 || to < 0 || to >= ids.length) return;
    const selectedId = ids[from];
    const adjacentId = ids[to];
    if (selectedId === undefined || adjacentId === undefined) return;
    [ids[from], ids[to]] = [adjacentId, selectedId];
    await props.onCommand({ type: "reorder_details", operationIds: ids });
  }

  async function updateOperation(patch: Partial<NonNullable<typeof operation>>) {
    if (!selectedRecord || !operation) return;
    if (selectedRecord.intent.kind !== "operation" && selectedRecord.intent.kind !== "stroke") return;
    const intent = updateFeedbackOperationIntent(selectedRecord.intent, patch);
    await props.onCommand({ type: "upsert_detail", operationId: selectedRecord.operationId, enabled: selectedRecord.enabled, intent });
  }

  async function updateDefinition(patch: Partial<NonNullable<typeof definition>>) {
    if (!selectedRecord || !definition) return;
    await props.onCommand({ type: "upsert_detail", operationId: selectedRecord.operationId, enabled: selectedRecord.enabled, intent: { kind: "definition", value: { ...definition, ...patch } } });
  }

  return <aside className="feedback-workbench" aria-label="Debug diagnostics and fixtures">
    <header><div><strong>Debug · Profile &amp; Detail Contributions</strong><small>Raw contribution, publication, cache, residency, dispatch, and stage evidence</small></div><button disabled={!!props.project?.document || !!props.project?.materialSources.length} onClick={props.onCreateSample}>Debug / Fixtures · Create bundled sample</button></header>
    <div className="feedback-grid">
      <section><h3>Target &amp; Stage 15 profile</h3>
        <label>Hotspot / region<select value={props.selectedRegionId ?? ""} onChange={(event) => props.onSelectRegion(event.currentTarget.value)}><option value="" disabled>Select a region</option>{props.project?.document?.topology.regions.map((region) => <option key={region.id} value={region.id}>{region.displayName} · {region.role}</option>)}</select></label>
        <label>Structural profile<select value={profileIntent.program} onChange={(event) => changeProgram(event.currentTarget.value as FeedbackProfileIntent["program"])}>{availablePrograms.map((program) => <option key={program} value={program}>{program.replaceAll("_", " ")}</option>)}</select></label>
        <div className="physical-controls"><label>Width m<input type="number" min="0" step="0.0005" value={profileIntent.firstWidth.value} onChange={(event) => setProfileIntent({ ...profileIntent, firstWidth: { unit: "meters", value: Number(event.currentTarget.value) } })} /></label><label>Depth m<input type="number" step="0.0005" value={profileIntent.amplitude.value} onChange={(event) => setProfileIntent({ ...profileIntent, amplitude: { unit: "meters", value: Number(event.currentTarget.value) } })} /></label><label>Radius m<input type="number" min="0" step="0.001" value={profileIntent.outerRadius.value} onChange={(event) => setProfileIntent({ ...profileIntent, outerRadius: { unit: "meters", value: Number(event.currentTarget.value) } })} /></label></div>
        <button disabled={!selectedRegion || props.commandBusy} onClick={() => props.selectedRegionId && void props.onCommand({ type: "set_profile", regionId: props.selectedRegionId, requested: profileIntent })}>Commit typed profile</button>
        <dl className="compiler-truth"><dt>Evaluator</dt><dd>{compiled?.compiledProfile?.evaluator ?? "not compiled"}</dd><dt>Occupancy</dt><dd>{compiled?.compiledProfile ? Object.entries(compiled.compiledProfile.occupancy).filter(([, enabled]) => enabled).map(([name]) => name).join(", ") || "none" : "not compiled"}</dd><dt>LOD</dt><dd>{compiled?.compiledProfile?.lod ?? "not compiled"}</dd><dt>Fallback</dt><dd>{compiled?.compiledProfile?.fallbackReason ?? compiled?.compiledProfile?.fallback ?? "not compiled"}</dd><dt>Identity</dt><dd className="identity">{compiled?.compiledProfile?.cacheIdentity ?? "not compiled"}</dd></dl>
      </section>
      <section><h3>Prompt LIB · registered immutable assets</h3>
        {asset ? <div className="asset-card"><strong>{asset.label}</strong><span>{asset.kind}</span><code>{asset.assetId}@{asset.version}</code><small>{asset.digest}</small></div> : <p className="typed-state failed">MissingAsset · import an owned registered Base Color/channel set.</p>}
        <label>Registered asset<select value={asset?.assetId ?? ""} onChange={(event) => setSelectedAssetId(event.currentTarget.value)}><option value="" disabled>Select an immutable asset</option>{assets.map((candidate) => <option key={candidate.assetId} value={candidate.assetId}>{candidate.label} · {candidate.assetId}@{candidate.version}</option>)}</select></label>
        <div className="detail-actions"><button disabled={!props.selectedRegionId || !asset || props.commandBusy} onClick={() => void addDefinition()}>Create DetailDefinition</button><button disabled={!props.selectedRegionId || !asset || definitions.length === 0 || props.commandBusy} onClick={() => void placeStamp()}>Place StampOperation</button><button disabled={!props.selectedRegionId || !asset || definitions.length === 0 || props.commandBusy} onClick={() => void placeStroke()}>Draw StampStroke</button><button disabled={!selectedRecord || records.indexOf(selectedRecord) === 0} onClick={() => void moveSelected(-1)}>Move up</button><button disabled={!selectedRecord || records.indexOf(selectedRecord) === records.length - 1} onClick={() => void moveSelected(1)}>Move down</button></div>
        <ol className="detail-list">{records.map((record) => <li key={record.operationId} className={record.operationId === props.selectedOperationId ? "selected" : ""}><button onClick={() => props.onSelectOperation(record.operationId)}><strong>{record.intent.kind}</strong><small>{record.operationId}</small></button><input aria-label={`Enabled ${record.operationId}`} type="checkbox" checked={record.enabled} onChange={(event) => void props.onCommand({ type: "set_detail_enabled", operationId: record.operationId, enabled: event.currentTarget.checked })} /><button aria-label="Duplicate detail" onClick={() => void props.onCommand({ type: "duplicate_detail", operationId: record.operationId })}>⧉</button><button aria-label="Delete detail" onClick={() => void props.onCommand({ type: "delete_detail", operationId: record.operationId })}>×</button></li>)}</ol>
        <div className="undo-row"><button disabled={!props.project?.canUndoDocument} onClick={props.onUndo}>Undo</button><button disabled={!props.project?.canRedoDocument} onClick={props.onRedo}>Redo</button></div>
      </section>
      <section><h3>Typed placement</h3>
        <div className="tool-row">{(["select", "profile", "stamp", "stroke"] as const).map((tool) => <button key={tool} className={props.activeTool === tool ? "active" : ""} onClick={() => props.onActiveTool(tool)}>{tool}</button>)}</div>
        {operation ? <><div className="physical-controls"><label>X m<input type="number" step="0.001" value={operation.physicalPositionM[0]} onChange={(event) => void updateOperation({ physicalPositionM: [Number(event.currentTarget.value), operation.physicalPositionM[1]] })} /></label><label>Y m<input type="number" step="0.001" value={operation.physicalPositionM[1]} onChange={(event) => void updateOperation({ physicalPositionM: [operation.physicalPositionM[0], Number(event.currentTarget.value)] })} /></label><label>Rotation°<input type="number" value={operation.rotationDegrees} onChange={(event) => void updateOperation({ rotationDegrees: Number(event.currentTarget.value) })} /></label><label>Opacity<input type="number" min="0" max="1" step="0.05" value={operation.opacity} onChange={(event) => void updateOperation({ opacity: Number(event.currentTarget.value) })} /></label><label>Spacing m<input type="number" min="0" step="0.001" value={operation.spacingM[0]} onChange={(event) => void updateOperation({ spacingM: [Number(event.currentTarget.value), operation.spacingM[1]] })} /></label><label>Seed<input type="number" value={operation.seed} onChange={(event) => void updateOperation({ seed: Number(event.currentTarget.value) })} /></label><label>Scatter<input type="number" min="0" max="1" step="0.05" value={operation.scatter} onChange={(event) => void updateOperation({ scatter: Number(event.currentTarget.value) })} /></label><label>Layer<input type="number" value={operation.layerOrder} onChange={(event) => void updateOperation({ layerOrder: Number(event.currentTarget.value) })} /></label></div>
          <div className="placement-overlay" aria-label="Physical placement overlays"><i className="valid-interior" /><i className="halo" /><i className="stamp-bounds" style={{ transform: `translate(-50%,-50%) rotate(${operation.rotationDegrees}deg)` }}><b className="pivot" /><b className="orientation" /></i><i className="repeat-period" /><span>bounds · pivot · orientation · repeat period · valid interior · halo</span></div>
          <dl className="compiler-truth"><dt>Scope</dt><dd>{operation.scope}</dd><dt>Fit / mapping</dt><dd>{operation.clipping} / {selectedRecord?.intent.kind === "operation" ? "planar" : "compiled"}</dd><dt>Blend / occupancy</dt><dd>{operation.blend} / {operation.occupancy}</dd><dt>Mirror / pivot</dt><dd>{String(operation.mirror)} / {operation.pivot.join(", ")}</dd><dt>Jitter</dt><dd>{operation.jitterM.join(", ")} m</dd><dt>Material ID</dt><dd>{operation.channels.find((channel) => channel.channel === "material_id")?.materialId ?? "not authored"} · validity is a separate raw view</dd></dl></> : <p>Select a StampOperation or StampStroke. Screen coordinates are transient; commits use physical slot coordinates.</p>}
        {operation ? <div className="physical-controls"><label>Size X m<input type="number" min="0.0001" step="0.001" value={operation.physicalSizeM[0]} onChange={(event) => void updateOperation({ physicalSizeM: [Number(event.currentTarget.value), operation.physicalSizeM[1]] })} /></label><label>Size Y m<input type="number" min="0.0001" step="0.001" value={operation.physicalSizeM[1]} onChange={(event) => void updateOperation({ physicalSizeM: [operation.physicalSizeM[0], Number(event.currentTarget.value)] })} /></label><label>Pivot X<input type="number" min="0" max="1" step="0.05" value={operation.pivot[0]} onChange={(event) => void updateOperation({ pivot: [Number(event.currentTarget.value), operation.pivot[1]] })} /></label><label>Pivot Y<input type="number" min="0" max="1" step="0.05" value={operation.pivot[1]} onChange={(event) => void updateOperation({ pivot: [operation.pivot[0], Number(event.currentTarget.value)] })} /></label><label>Mirror X<input type="checkbox" checked={operation.mirror[0]} onChange={(event) => void updateOperation({ mirror: [event.currentTarget.checked, operation.mirror[1]] })} /></label><label>Mirror Y<input type="checkbox" checked={operation.mirror[1]} onChange={(event) => void updateOperation({ mirror: [operation.mirror[0], event.currentTarget.checked] })} /></label><label>Scope<select value={operation.scope} onChange={(event) => void updateOperation({ scope: event.currentTarget.value as typeof operation.scope })}><option value="material_reusable_atlas">Reusable atlas</option><option value="asset_specific_deferred">Deferred only</option></select></label><label>Clipping<select value={operation.clipping} onChange={(event) => void updateOperation({ clipping: event.currentTarget.value as typeof operation.clipping })}><option value="contain">Contain</option><option value="cover">Cover</option><option value="repeat">Repeat</option><option value="fail_if_oversized">Fail if oversized</option></select></label><label>Blend<select value={operation.blend} onChange={(event) => void updateOperation({ blend: event.currentTarget.value as typeof operation.blend })}><option value="replace">Replace</option><option value="add">Add</option><option value="multiply">Multiply</option><option value="max">Max</option></select></label><label>Occupancy<select value={operation.occupancy} onChange={(event) => { const occupancy = occupancyRelationFromValue(event.currentTarget.value); if (occupancy) void updateOperation({ occupancy }); }}>{occupancyRelations.map((occupancy) => <option key={occupancy} value={occupancy}>{occupancy.replaceAll("_", " ")}</option>)}</select></label><label>Spacing Y m<input type="number" min="0" step="0.001" value={operation.spacingM[1]} onChange={(event) => void updateOperation({ spacingM: [operation.spacingM[0], Number(event.currentTarget.value)] })} /></label><label>Jitter X m<input type="number" step="0.001" value={operation.jitterM[0]} onChange={(event) => void updateOperation({ jitterM: [Number(event.currentTarget.value), operation.jitterM[1]] })} /></label><label>Jitter Y m<input type="number" step="0.001" value={operation.jitterM[1]} onChange={(event) => void updateOperation({ jitterM: [operation.jitterM[0], Number(event.currentTarget.value)] })} /></label></div> : null}
        {definition ? <div className="physical-controls"><label>Definition size X m<input type="number" min="0.0001" step="0.001" value={definition.physicalSize[0]} onChange={(event) => void updateDefinition({ physicalSize: [Number(event.currentTarget.value), definition.physicalSize[1]] })} /></label><label>Definition size Y m<input type="number" min="0.0001" step="0.001" value={definition.physicalSize[1]} onChange={(event) => void updateDefinition({ physicalSize: [definition.physicalSize[0], Number(event.currentTarget.value)] })} /></label><label>Definition fit<select value={definition.fitPolicy} onChange={(event) => void updateDefinition({ fitPolicy: event.currentTarget.value as typeof definition.fitPolicy })}><option value="contain">Contain</option><option value="cover">Cover</option><option value="repeat">Repeat</option><option value="fail_if_oversized">Fail if oversized</option></select></label></div> : null}
      </section>
      <section><h3>Raw contribution / compiler QA</h3>
        <label>Visible view<select value={props.view} onChange={(event) => props.onView(event.currentTarget.value as FeedbackContributionView)}>{contributionViews.map((view) => <option key={view.value} value={view.value}>{view.label}</option>)}</select></label>
        <div className="physical-controls"><label>Review<select value={props.profile} onChange={(event) => props.onProfile(event.currentTarget.value as FeedbackPreviewProfile)}><option value="preview1024">1K</option><option value="preview2048">2K</option><option value="preview4096">4K</option><option value="preview8192">8K</option></select></label><label>Comparison<select value={props.comparisonMode} onChange={(event) => props.onComparisonMode(event.currentTarget.value as FeedbackComparisonMode)}><option value="after">After</option><option value="before">Before (display-only)</option><option value="selectedOperationIsolation">Selected operation isolation</option></select></label></div>
        <button disabled={!viewDependency || !props.project?.document} onClick={() => props.onRequestVisibleView()}>{viewDependency ? `Request current GPU ${viewDependency} tile` : "Metadata QA · zero pixel dispatch"}</button>
        <p className="qa-note">Pixels come only from the persisted compiler’s requested GPU tile. Route, occupancy, LOD, fallback, scope, and asset-resolution text comes from compiled metadata.</p>
        <dl className="stage-availability">{installedSummary.map(([stage, state]) => <React.Fragment key={stage}><dt>Stage {stage}</dt><dd className={`typed-state ${state === "NotInstalled" ? "unavailable" : ""}`}>{state}{state === "NotInstalled" ? " / Unavailable" : ""}</dd></React.Fragment>)}</dl>
      </section>
    </div>
  </aside>;
}
