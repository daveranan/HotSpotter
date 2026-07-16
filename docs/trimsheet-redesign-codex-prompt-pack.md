# Hot Trimmer document-first experience rebuild: Codex prompt pack

## Purpose and starting point

This pack replaces the trim-sheet engine and authoring authority with the Rust-owned
`TrimSheetDocument` while preserving Hot Trimmer's established source-first desktop experience. The canonical
domain groundwork is already present. Do not rerun the old domain-only foundation slice, and do not treat an
engine cutover as permission to redesign the application shell, project workflow, or visual language.

Run one prompt per Codex task, in order. Each prompt ends in a coherent user-visible state. Do not advance
when its automated checks or required in-app walkthrough fail.

The governing plans remain:

```text
docs/trimsheet-document-generation-redesign-plan.md
docs/hot-trimmer-template-blender-companion-plan.md
```

## Breaking cutover decisions

These decisions override older incremental-transition language in the governing plans:

- `TrimSheetDocument` is the only trim-sheet authoring state, persisted model, command target, compiler
  input, IPC projection, undo/redo unit, recovery state, and publishing identity.
- Do not add or retain a runtime bridge, adapter, dual reader, dual writer, fallback renderer, or parallel
  TypeScript layout model.
- Delete `TrimSheetDocument::from_legacy_template_layout`, `StoredLayout::to_trim_sheet_document`, and every
  equivalent conversion path in the first prompt.
- Remove legacy trim-layout tables, stored solver intent/results, duplicated item/region fills, source-layer
  overrides, old compiler requests, CSS atlas rendering, local React layout authority, and obsolete tests.
- Preserve independent project assets—projects, imported sources/maps, patches, names, and source ownership—
  but do not translate an old trim layout into the new document. A project with only legacy trim state opens
  in the full source workbench with its source library intact and an inline `No trim sheet yet` state.
- Preserve the established application chrome, source library, registered-map workflow, source canvas, project
  commands, interaction density, colors, typography, and overall desktop composition. Rewire those surfaces to
  canonical document commands; do not replace them with a new product concept.
- Pure algorithms and UI primitives may be reused after they accept canonical inputs and return canonical results.
  Legacy layout authority is not a compatibility path, but the established user workflow is the product shell.
- Delete replaced code within the same prompt that installs its replacement. Every task must finish compiling;
  do not leave a half-old/half-new application between prompts.

## Final screen contract

The rebuild retains the established Hot Trimmer workbench shell from Prompt 1 onward:

```text
Top action bar
  New / Open / Recent / Save / Save As / Close / undo / redo / build state / Export / Send to Blender

Left — Source Workspace
  source-set library and registered maps
  source/patch canvas
  Select / Rectangle / Four Point / Outline Fit
  established material-source and patch interaction model

Right work area — Trim Sheet Workpiece
  current CompiledSheet map
  compile-plan overlays and stable region selection
  Beauty and channel views
  2D / Material Preview switch

Context Inspector
  document settings when nothing is selected
  Content / Mapping & Warp / Profile / Decorations / Layout / Diagnostics for a region
  only capabilities valid for the selected target

Status area
  Draft preview / Compiling revision N / Up to date / Needs rebuild / Region error
```

The app opens directly into this workbench as an untitled draft. Opening or dropping a Base Color may begin the
document immediately; creating a project file is not a prerequisite for exploring or building. Ask for a project
path only when the user saves. Advanced mapping, profiles, topology, and DCC metadata appear only in their
appropriate modes. Never duplicate the same authoritative control in multiple panels.

## Rules for every prompt

- Read `AGENTS.md`, the named plan sections, and the current git status before editing. Preserve unrelated user
  changes.
- Work in the root task without subagents unless the prompt explicitly authorizes one bounded worker.
- Start from user intent and visible behavior, then implement the Rust/persistence/compiler/IPC support required
  to make that behavior true. A metadata-only or backend-only delivery does not satisfy a UI-bearing prompt.
- React stores stable IDs, view state, and draft interaction state only. Rust owns canonical product state.
- Every enabled authoring control invokes a typed Rust command and produces a matching compiled-artifact change.
- Preview pixels, overlays, 3D preview, export, and Blender publishing consume the same `CompiledSheet` lineage.
- Unsupported future features remain visibly disabled with a concise prerequisite; inert controls are forbidden.
- Add focused Rust and desktop tests named with the prefix specified by the prompt. Cross-stack slices intentionally
  run both listed automated commands. Make at most one correction pass and rerun only the failed command(s).
- After automated checks, launch or attach to the desktop app and complete the prompt's required walkthrough.
  Record observed behavior. If the app cannot be run, report the slice as unverified rather than green.
- Stop at the assigned boundary. Report deleted legacy paths, authoritative commands/artifacts, automated evidence,
  walkthrough results, and remaining disabled prerequisites.

---

## Prompt 1 — Restore the source-first product around the document-first engine

```text
Implement Prompt 1: restore Hot Trimmer's established source-first desktop UX and correct the compiler so a
trim sheet is derived from intentional subdivisions of one source material. Keep TrimSheetDocument as the sole
authoring authority. This is an engine/workpiece cutover, not a visual redesign of the application.

Read AGENTS.md; trimsheet-document-generation-redesign-plan.md sections 2, 3, 8, 9, 10, 11, 12,
Milestone 1, and stop conditions; companion-plan sections 21 and 25.1. Inspect the completed canonical
domain and every current layout/store/compiler/IPC/React entry point. Preserve unrelated changes. No subagents.

Scope — restore the product experience:
- Use the pre-redesign desktop UI and the supplied reference screenshots as the visual and interaction baseline.
  Restore its top command bar, workbench tabs, source-set list, registered map slots, large source canvas, compact
  panels, typography, colors, spacing, borders, and status bar. Do not reinterpret or modernize the design.
- Remove the centered welcome card and every workflow that blocks the workbench behind New Project/Open Project.
  Launch into an untitled draft. New resets to an untitled draft; Open loads an existing project; importing,
  opening, or dropping a Base Color begins work immediately; Save/Save As asks for a project path when needed.
- Preserve multiple source sets and optional registered Normal, Height, Roughness, Metallic, AO, Specular, Opacity,
  Edge Mask, and Material ID maps. Base Color is the anchor map for a source set, not a one-time setup wizard.
- Keep the document-first Trim Sheet Workpiece, exact RegionId selection, compiled pixels, compile-plan overlays,
  map views, and inspector behavior, but place them inside the established workbench composition.
- Restore UI and workflow only. Do not restore StoredLayout, the legacy layout reducer, the old compiler request,
  CSS atlas previews, or old layout persistence.
- Implement honest empty, importing, ready, compiling, stale, and region-error states inside the restored shell.
- Features without a working document command and compiler effect must remain unavailable and clearly explained;
  do not render controls or visual effects that merely pretend to work.

Scope — non-negotiable trim-sheet meaning:
- A source texture is mapped into a trim-sheet topology. The topology subdivides the output into purposeful strips,
  panels, edges, and details; it is not a request to resize the whole source into every region.
- Every compiled region resolves an explicit source-space UV crop or mapping from canonical template/document data.
  Different template regions generally sample different areas of the source. Reusing a crop is allowed only when
  it is explicitly authored by the template or user.
- Never default every region to the full-source rectangle. Never infer a region mapping from its output bounds when
  that would make every region another scaled copy of the same image.
- Preserve recognizable spatial variation from the source across the compiled sheet. Long strips, broad panels,
  narrow edges, and detail regions use mappings appropriate to their purpose and aspect ratio.
- Region profiles, bevels, normals, displacement, weathering, and decorations are treatments applied to these
  source-derived subdivisions. They must not replace the underlying source-space relationship. Do not fake these
  treatments in this prompt if their canonical command/compiler implementation is not present.

Scope — preserve the completed document cutover:
- Treat the new TrimSheetDocument domain, template topology, direct document persistence, document journal, schema
  migration, revisions, hashes, undo/redo, and typed IPC as foundation. Do not revert or reimplement that work.
- Inspect the current diff and selectively restore the pre-redesign UI/workflow. Do not use a broad git reset and do
  not discard unrelated user changes or the completed document/template work.
- Wire the restored UI to typed Rust document commands. React owns only selection and transient interaction state.
- Keep one compiler entry point: TrimSheetDocument -> resolved compile plan -> CompiledSheet. The resolved plan must
  contain the exact source-space mapping for every region and apply the same mapping to every registered map in that
  source set. Region ID, Material ID, pixels, and overlays come from that one plan.
- Do not bring back StoredLayout product state, old template/atlas preview requests, duplicated fills/bounds, local
  React layout reducers, content-signature heuristics, CSS output substitutes, or old live compiler calls.

Acceptance:
- The application opens directly into the restored workbench; the user can open/drop a Base Color and see it on the
  source canvas before choosing a save location or creating a named project.
- New/Open/Recent/Save/Save As/Close and untitled-draft behavior match the pre-redesign workflow while saving and
  reopening the canonical TrimSheetDocument.
- The source library, map slots, source canvas, application chrome, styling, information density, and panel layout
  visibly match the pre-redesign UI baseline.
- A nonuniform source image compiles into regions with visibly different source crops. The result must not be the
  same whole texture resized and repeated across the sheet.
- A compiler test using a synthetic quadrant/grid image proves that at least three regions sample their declared,
  distinct source-space rectangles and that all registered maps use the identical per-region mapping.
- Preview pixels and overlays share revision, topology hash, appearance hash, and exact region bounds.
- Save/reopen reproduces the document and compiled hashes.
- A pre-cutover project retains sources/maps/patches and opens in the full workbench with an inline “no trim sheet
  yet” state; no old layout is shown or silently converted, and no blocking creation screen appears.
- There is one live document, one compiler entry, one artifact, and no bridge/adaptor/fallback path.

Automated verification:
cargo test --workspace source_first_document
npm run typecheck --workspace @hot-trimmer/desktop
npm run test --workspace @hot-trimmer/desktop

Required in-app walkthrough:
launch -> confirm full workbench appears without a modal/welcome wall -> open or drop a visibly nonuniform Base Color
without first creating a project -> inspect it on the source canvas -> build the trim sheet -> select three regions
and confirm each shows a purposeful, different crop -> switch Base Color and Region ID views -> Save As -> close ->
reopen -> confirm identical pixels, overlays, mappings, selection behavior, styling, and build status.

Stop conditions:
- Stop and fix the compiler if multiple regions silently resolve to the same full-source crop.
- Stop and fix the shell if importing an image requires a project-creation ceremony first.
- Stop and fix the UI if the result introduces a new visual language instead of restoring the established one.
- Do not declare success from hashes, IDs, or region bounds alone; the required source-space mapping test and
  in-app visual walkthrough must both pass.
```

---

## Prompt 2 — Stable source workbench, patch tools, framing, and gestures

```text
Implement Prompt 2: make the rebuilt Source Workspace and Trim Sheet Workpiece reliable under real pointer,
keyboard, undo, and asynchronous compile behavior.

Read AGENTS.md; redesign-plan sections 4, 8, 9 and Milestone 2. Inspect Prompt 1's document-only shell. No subagents.

Scope:
- Implement stable selection references by kind and ID; never retain editable document objects in React state.
- Implement BeginDraftEdit / bounded draft preview / CommitDraftEdit for sheet framing, patch geometry, and region
  content transforms. One completed gesture creates one Rust command and one undo entry.
- Keep the final draft visible until its accepted revision arrives. Reject stale snapshots and preview jobs by
  document revision, draft ID, and input hash.
- Implement explicit Select, Rectangle, Four Point, and Outline Fit tools. Select-mode double-click edits an existing
  patch/footprint and never creates an identity. Empty double-click does nothing.
- Use one accessible gizmo framework for translate, scale, rotate, perspective points, numeric editing, snapping,
  keyboard nudging, cancellation, pointer capture, and minimum handle hit targets.
- Replace detached focus sliders with the actual aspect-locked crop rectangle on the source canvas. Repeat mode shows
  every wrapped footprint honestly.
- Synchronize right-region selection with its exact left source footprint and inspector breadcrumb.
- Delete superseded patch/layout canvases, event fallthrough handlers, optimistic snapshot replay, and old gesture tests.

Acceptance:
- Rapid A/B/C manipulation never visibly returns to A or B after C; reopen equals C.
- Square destinations display and compile square crops; left footprint and right pixels agree.
- Double-click point editing preserves patch/region counts and IDs.
- Escape restores once; command failure restores once with a target-specific error.
- Every operation is usable by pointer, keyboard, and numeric entry.

Automated verification:
cargo test -p hot-trimmer-project-store trim_sheet_interaction
npm run test --workspace @hot-trimmer/desktop

Required in-app walkthrough:
create rectangle patch -> edit four points -> cancel once -> commit move/scale/rotate -> undo/redo -> edit sheet crop ->
rapidly drag through A/B/C -> save/reopen -> verify no duplication, rollback flicker, or mismatched source/sheet view.
```

---

## Prompt 3 — Complete source library and region content workflow

```text
Implement Prompt 3: make material sources, registered maps, patches, solids, and region content assignment a
complete user workflow through RegionBinding.

Read AGENTS.md; redesign-plan sections 3.2, 7, 9 and Milestone 3; companion-plan section 10 and gate 25.7.
Inspect the rebuilt workbench. No subagents.

Scope:
- Finish the Source Library: semantic source-set cards, map registration/status, thumbnails, physical scale,
  orientation/tiling metadata, missing-map state, rename/remove, and primary/secondary material choice.
- Make the selected region inspector's Content control canonical: inherit primary, material source, patch, solid, or
  registered procedural reference. Changes preview immediately and persist through one Rust command.
- Rectify a patch once per revision and sample every available channel through identical geometry/mapping.
- Add explicit missing-map fallback and Estimated labels; never infer Metallic from color.
- Patch/source deletion or disable performs one atomic fallback command with one undo entry and an explicit notice.
- Keep region ID, hotspot, mapping, profile, and compatibility unchanged during content swaps.
- Remove remaining UI terminology that conflates source, patch, region, and template slot.

Acceptance:
- Concrete primary, metal secondary, and a contrasting vent patch coexist in different regions.
- Assigning content changes exactly one region in every available map.
- Missing channels are clearly authored/estimated/fallback, and Metallic remains explicit.
- Undo/redo and save/reopen preserve binding and cross-map alignment.

Automated verification:
cargo test -p hot-trimmer-sheet-compiler region_binding
npm run test --workspace @hot-trimmer/desktop

Required in-app walkthrough:
import concrete set -> register Normal/Roughness -> import metal set -> create vent patch -> assign all three to
separate regions -> inspect maps -> delete/undo patch -> save/reopen -> confirm exact targeted fallback.
```

---

## Prompt 4 — Layout generator and accepted/candidate topology experience

```text
Implement Prompt 4: deliver deterministic procedural layout generation as a clear candidate-review workflow,
not a hidden mutation of the accepted sheet.

Read AGENTS.md; redesign-plan sections 4.1, 5, 9, 10, 11 and Milestone 4; companion procedural-generation sections.
No subagents.

Scope:
- Implement versioned LayoutGeneratorRecipe, TopologyCandidate, deterministic seeded generation, validation,
  candidate compilation, acceptance/discard, pinned provenance, and compatibility report.
- Support broad/medium/small/micro families, orientation mix, strip/unique/cap/radial quotas, margins, padding,
  bleed, occupancy, minimum size, aspect ranges, reserved banks, grouping, and profile policy.
- Ship Balanced Architecture, Horizontal Trim Bank, Vertical Panel Bank, Dense Detail Field, and Radial Accent through
  one generator engine.
- Add a focused Layout Generator drawer with presets, Advanced disclosure, live summary, seed/regenerate, and explicit
  Preview Candidate / Discard / Accept Layout actions.
- Candidate state is visually unmistakable in the workpiece and cannot overwrite accepted topology until acceptance.
- Accept is one undoable topology command. Save/reopen pins the accepted snapshot and never reruns generation.

Acceptance:
- Same recipe/seed/version produces byte-identical regions, IDs, ordering, profiles, and topology hash.
- Different seed previews a different valid candidate without changing the accepted sheet.
- Summary explains population, occupancy, gutters, errors, and Blender compatibility impact.
- Candidate and accepted sheet both preview real current material content.

Automated verification:
cargo test -p hot-trimmer-geometry layout_generator
npm run test --workspace @hot-trimmer/desktop

Required in-app walkthrough:
open generator -> try three presets -> change seed/population -> compare candidate and accepted -> discard -> regenerate ->
accept -> undo/redo -> save/reopen -> verify pinned geometry and provenance.
```

---

## Prompt 5 — Mapping & Warp and radial authoring

```text
Implement Prompt 5: make Mapping & Warp a complete direct-manipulation experience for any selected region.

Read AGENTS.md; redesign-plan section 6, sections 8.3 and 9, and Milestone 5; companion section 11.2. No subagents.

Scope:
- Evaluate Planar/Crop, Perspective, Polar/Radial, Cylindrical/Arc, ordered Spiral/Twirl, and Radial Lens operations,
  followed by transform and address mode, through the canonical compiler.
- Build the inspector's Mapping & Warp stack with add/remove/duplicate/reorder/enable/reset, typed numeric inputs,
  direct center/radius/seam/quad handles, and visible operation order.
- Add Radial Pattern as an appearance preset. It never cuts a circle, changes region bounds, or changes topology.
- Apply identical coordinates to every map and transform tangent-space normals using the complete local Jacobian,
  including reflections.
- Bound operation count, strength, radius, turns, derivatives, sampling, and intermediate work. Singularities produce
  region diagnostics with repair guidance, never NaNs or silent fallback.
- Persist and undo the exact ordered typed recipe; remove any unversioned/generic warp payload or renderer shortcut.

Acceptance:
- Any rectangular region can display radial, spiral, fisheye, or arc-mapped content from any source.
- Mapping edits leave topology hash inputs and Blender UV assignments unchanged.
- Left handles, right preview, final compile, reopen, undo, and export inputs agree.
- Normal direction fixtures pass at mirror, seam, center, and nonlinear cases.

Automated verification:
cargo test -p hot-trimmer-render-core warp_mapping
npm run test --workspace @hot-trimmer/desktop

Required in-app walkthrough:
select region -> edit perspective quad -> add Radial Pattern -> move center/seam -> add twirl/lens -> reorder/disable ->
mirror -> inspect Base Color and Normal -> undo/redo -> save/reopen -> verify unchanged region/hotspot overlay.
```

---

## Prompt 6 — Structural profiles, generated maps, and material inspection

```text
Implement Prompt 6: make structural profile and generated-map authoring understandable and visibly trustworthy.

Read AGENTS.md; redesign-plan sections 7.1 through 7.3, 9 and Milestone 6. No subagents.

Scope:
- Implement Flat, Convex Bevel, Concave Groove, Rounded Bevel, Double Bevel, Raised Lip, Recessed Seam, Panel Frame,
  Radial Disc, and Annulus analytic programs.
- Add simple global Edge Style controls and contextual per-group/per-region Profile overrides with progressive
  disclosure for width/radius, curve, inset/extrusion, edges, hardness, cavity, normal intensity, and reference scale.
- Generate Height, tangent-space Normal, AO/cavity, edge masks, padding, bleed, and dilation from authoritative
  region/hotspot geometry. Compose imported/estimated material maps correctly; never average Normal RGB.
- Add fast map isolation and a canonical Material Preview scene with wall, broad panel, edge trim, strip, and radial
  fixture using the current CompiledSheet. Label estimated channels and replacement status clearly.
- Resolution changes scale appearance work without moving normalized hotspots, IDs, or topology hash.

Acceptance:
- Profile changes are immediately legible in Height/Normal/AO and the material preview.
- Boundaries match workpiece overlays and ID/hotspot geometry at 1K/2K/4K/8K.
- Authored maps replace estimated channels cleanly; Metallic remains explicit.
- 2D channel views and material preview reference the same artifact hashes.

Automated verification:
cargo test -p hot-trimmer-render-core structural_profile
npm run test --workspace @hot-trimmer/desktop

Required in-app walkthrough:
apply three global edge styles -> override one region with rounded bevel and recessed seam -> inspect Height/Normal/AO ->
switch resolutions -> open Material Preview -> isolate channels -> confirm topology/selection remain stable.
```

---

## Prompt 7 — Decorations, condition, and nondestructive treatments

```text
Implement Prompt 7: deliver region decorations and condition/treatment authoring as a coherent inspector workflow.

Read AGENTS.md; redesign-plan section 7.4, section 9, and Milestone 7; companion detail/weathering sections. No subagents.

Scope:
- Implement ordered DecorationBinding for Repeating Strip, Unique Detail, Trim Cap, Radial Detail, Seam, Panel,
  Bolt, Pattern, Vent, and Decal with source, placement, transform, channel blends, masks, targeting, and seed.
- Add a Decorations inspector with add browser, compatible-region suggestions, canvas placement, reorder, enable,
  duplicate, remove, and channel contribution controls.
- Implement Clean/Used/Heavy as approachable document-level Condition presets backed by visible ordered treatment
  layers: Grunge, Edge Wear, Dirt, Color/Roughness Adjust, Height Boost, Decal, and Mask.
- Add a Treatments panel for advanced region/group/global targeting, masks, deterministic seeds, reorder, enable,
  and reset. Compile decorations before treatments through CompiledSheet.
- Persist, undo/redo, cache, preview, and reopen nondestructively. No treatment mutates source pixels.

Acceptance:
- A vent, repeating strip, radial detail, and trim cap coexist over one base material.
- Clean/Used/Heavy are useful immediately while advanced layers remain inspectable and editable.
- Reorder/disable/target changes only intended channels and is deterministic after reopen.
- Every visible layer has a compiled pixel effect or an explicit diagnostic.

Automated verification:
cargo test -p hot-trimmer-sheet-compiler decoration_treatment
npm run test --workspace @hot-trimmer/desktop

Required in-app walkthrough:
add four decoration kinds -> place/edit/reorder -> switch Clean/Used/Heavy -> edit masks/targets -> isolate affected maps ->
disable/undo/redo -> save/reopen -> confirm deterministic layer order and pixels.
```

---

## Prompt 8 — Custom topology and Atlas as first-class document modes

```text
Implement Prompt 8: deliver Custom Template and Custom Atlas without resurrecting the deleted legacy model.

Read AGENTS.md; redesign-plan sections 4.1, 5, 9, 10 and Milestone 8; companion custom-topology sections. No subagents.

Scope:
- Implement Customize Layout as an explicit clone to CustomTemplate with new compatibility key and warning.
- Add Layout mode using the shared transactional gizmo: move, resize, axis-aligned 90-degree rotate, add/delete,
  lock, numeric edit, collision/clearance repair, validation, candidate review, and acceptance.
- Reimplement or reuse only the pure seeded packing algorithm for CustomAtlas; it must emit TopologyCandidate and
  never recreate StoredLayout, LayoutItem/LayoutRegion fill authority, or CSS atlas preview.
- Provide a clear Advanced Custom Atlas entry point for patches/simple entries with packing controls and summary.
- Compile standard, generated, custom template, and custom atlas documents into identical CompiledSheet contracts.
- Standard templates keep topology controls locked behind Customize Layout; Content mode remains available.

Acceptance:
- Custom edits persist and match Base Color/Height/Normal/IDs/hotspots/overlay metadata.
- Atlas packs and rasterizes real patch content in every available map.
- Preview, undo, save/reopen, export input, and compatibility report use the same hashes.
- Repository search finds no reintroduced legacy product model or CSS-render substitute.

Automated verification:
cargo test -p hot-trimmer-geometry custom_topology
npm run test --workspace @hot-trimmer/desktop

Required in-app walkthrough:
attempt locked standard edit -> Customize Layout -> move/resize/rotate/add/delete -> repair collision -> accept -> undo/redo ->
create Custom Atlas from patches -> repack -> inspect real maps -> save/reopen.
```

---

## Prompt 9 — Export, canonical 3D preview, and Blender delivery

```text
Implement Prompt 9: make inspection, export, and Blender consume the exact current CompiledSheet without
recomputing appearance.

Read AGENTS.md; redesign-plan sections 9, 10.4 and Milestone 9; companion export, manifest, sync, and Blender sections.
No subagents unless one bounded Blender-package worker is explicitly approved.

Scope:
- Complete the canonical Material Preview for wall, panel, edge, strip, and radial fixtures, channel isolation,
  normal strength, roughness inspection, and neutral lighting using current artifact maps.
- Implement atomic generic export with map selection, format/bit depth, packed channels, OpenGL/DirectX normals,
  progress, cancellation, disk/memory estimate, staging, Unicode paths, and no partial replacement.
- Manifest includes document revision, topology/appearance hashes, renderer version, map checksums, region/hotspot
  metadata, color-space policy, mapping/profile provenance, and generator snapshot.
- Publish the same artifact atomically for Blender. Complete material nodes, connection/status UI, hotspot browser,
  rectangular/strip/radial fitting, candidate cycling, locking, and appearance-versus-topology sync reports.
- Appearance updates reload maps without touching UV assignments. Topology changes always require a compatibility
  report before any remapping.

Acceptance:
- 2D, Material Preview, export, manifest, and Blender revision share exact artifact hashes.
- Cancellation/disk/read-only failures preserve prior valid output.
- Appearance-only publish preserves UVs/locks; topology change never silently remaps.
- A user can complete generic export without installing Blender.

Automated verification:
cargo test -p hot-trimmer-export compiled_sheet
npm run test --workspace @hot-trimmer/desktop

Required in-app walkthrough:
inspect 2D/3D maps -> export selected maps -> cancel overwrite -> export Unicode path -> publish to Blender -> fit and lock
rectangular/strip/radial islands -> change crop/condition -> republish -> confirm unchanged UVs -> preview topology report.
```

---

## Prompt 10 — Product qualification and proof of legacy removal

```text
Implement Prompt 10: qualify the rebuilt document-first experience, remove every remaining obsolete path, and
produce the release-gate report.

Read AGENTS.md; every acceptance scenario and stop condition in the redesign plan; companion gates 25 and 26.
Inspect git status and the complete implementation. Preserve unrelated intentional changes. No subagents unless the
user explicitly authorizes one bounded worker.

Scope:
- Run the complete product loop: project/source import, template creation, source framing, region content, patches,
  generator candidates, mapping/warp, profiles, maps, decorations/treatments, custom/atlas, material preview, export,
  save/reopen/recovery, and Blender publishing.
- Delete obsolete tables, Rust types, commands, serializers, IPC shapes, React components/hooks/reducers, CSS,
  compiler adapters, tests, fixtures, debug labels, roadmap controls, and feature flags once proven unreferenced.
- Add a repository guard test that fails when banned legacy type/path names or a second live compiler entry return.
- Verify crash recovery, cancellation, stale results, bounded resources, Unicode, permission/read-only/disk-full,
  offline use, high DPI, keyboard-only operation, screen-reader labels, focus order, contrast, and performance budgets.
- Ensure every enabled control has a persisted Rust command plus matching artifact effect, or is explicitly a viewport
  control. Disabled controls state their prerequisite.
- Update architecture/release documentation to describe only the shipped document-first system.

Acceptance:
- All redesign-plan scenarios pass and all stop conditions are false.
- No legacy trim state can be created, read, compiled, rendered, or displayed.
- No old compiler or CSS path can produce user-visible maps.
- The workbench is understandable without internal phase/template-slot/DCC jargon in its default path.
- Clean install through Blender update is coherent, deterministic, accessible, and recoverable.

Automated verification:
npm run check

Required in-app walkthrough:
perform the entire release matrix documented by the prior prompts at normal and high DPI using pointer and keyboard;
capture final screenshots and artifact/hash evidence; report each gate pass/fail and do not call the release green with
an unverified enabled workflow.
```

---

## Continuation prompt for an incomplete slice

```text
Continue the same document-first Hot Trimmer slice; do not start the next prompt.

Read AGENTS.md, the exact slice, its previous report, current diff/status, failed automated output, and failed in-app
step. Preserve all existing changes. Reproduce the first concrete failure. Fix only what prevents the slice's stated
acceptance. Do not restore a deleted legacy path, add a bridge/fallback, defer an enabled control, or widen the slice.
Make at most one correction pass, rerun the failed verification, repeat the failed walkthrough step, and stop with
updated evidence or the exact remaining blocker.
```

---

## Read-only product verification prompt

```text
Verify the just-completed document-first Hot Trimmer slice in the running desktop app without modifying code.

Read AGENTS.md, the slice walkthrough, and current project state. Launch or attach to the app and perform every step.
Record visible document revision/build status, selected IDs/modes, artifact hashes where exposed, and observed pixels.
Capture the first failure with exact actions and error text. Stop after the walkthrough; do not widen into a general
audit. A passing automated test does not override a visible product failure.
```
