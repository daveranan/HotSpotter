import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const app = readFileSync(new URL("./source-first-app.tsx", import.meta.url), "utf8");
const styles = readFileSync(new URL("./document-app.css", import.meta.url), "utf8");

test("hotspot map preview retries the settled revision and keeps generated map controls visible", () => {
  assert.match(app, /const pendingAutomaticPreviewKey = useRef<string \| null>\(null\)/);
  assert.match(app, /if \(lastAutomaticPreviewKey\.current === key \|\| pendingAutomaticPreviewKey\.current === key\) return/);
  assert.match(app, /pendingAutomaticPreviewKey\.current = key/);
  assert.match(app, /void requestPreview\(undefined, undefined, interactivePreviewProfile, project\.document\.documentRevision, false, true\)/);
  assert.match(app, /lastAutomaticPreviewKey\.current = automaticKey/);
  assert.doesNotMatch(app, /lastAutomaticPreviewKey\.current = key;\s*dirtyPreviewRegion/);

  assert.match(app, /failureReason\.code !== "operation_cancelled"/);
  assert.match(app, /request_outcome=superseded/);
  assert.match(app, /const latestRevision = projectRef\.current\?\.document\?\.documentRevision/);
  assert.match(app, /window\.setTimeout\(\(\) => void requestPreview\(regionId, projection, profile, latestRevision, scheduleRefinement, true\), 0\)/);

  assert.match(app, /<fieldset className="map-view-section hotspot-map-view"><legend>Map view<\/legend>/);
  for (const label of ["Diffuse", "Height", "Normal", "Roughness", "Metallic", "AO", "Region ID"]) {
    assert.match(app, new RegExp(`"${label}"`));
  }
  assert.match(styles, /\.context-inspector\.layout-mode \.inspector-section:not\(\.layout-summary\):not\(\.map-view-section\)/);
});

test("topology edits cannot discard their replacement preview or bless retained pixels as current", () => {
  const topologyEffect = app.slice(
    app.indexOf("useEffect(() => {\n    // A topology command starts its replacement request"),
    app.indexOf("useEffect(() => {\n    if (!native || !project?.document) return;"),
  );
  assert.ok(topologyEffect.length > 0);
  assert.doesNotMatch(topologyEffect, /previewDraftId\.current/);

  const retopology = app.slice(
    app.indexOf("function retopologizeArtifact"),
    app.indexOf("function stableRegionColor"),
  );
  assert.ok(retopology.length > 0);
  assert.doesNotMatch(retopology, /revision: document\.documentRevision/);
  assert.doesNotMatch(retopology, /documentRevision: document\.documentRevision/);
  assert.doesNotMatch(retopology, /topologyHash: hashBytes/);
  assert.match(retopology, /local topology edit: retained compiled map pixels/);

  assert.doesNotMatch(
    app,
    /priorTopologyHash !== nextTopologyHash\) suppressAutomaticPreviewRevision/,
    "topology undo/redo must be allowed to schedule its automatic replacement preview",
  );
});

test("full-resolution and F2 telemetry report requested map, revision, problem, and terminal outcome", () => {
  assert.match(app, /request_outcome=published/);
  assert.match(app, /requested_revision=\$\{requestedRevision\}/);
  assert.match(app, /requested_map=\$\{requestedMapView\}/);
  assert.match(app, /terminalOutcome\?: "published" \| "failed" \| "superseded"/);
  assert.match(app, /Full-resolution preview succeeded/);
  assert.match(app, /Full-resolution preview superseded/);
  assert.match(app, /problem: props\.problem/);
});
