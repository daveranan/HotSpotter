# Hot Trimmer - Native MVP Implementation Phases

## 1. Purpose

This document converts `mvp-plan.md` into an ordered implementation program for a production-quality
native desktop MVP. It borrows the evidence-based phase gates from the Trim Sheet Studio plan without
bringing across that application's broader library, integration, or DCC scope.

The product loop remains deliberately narrow:

```text
Open image -> mark patches -> create trim layout -> generate maps -> add treatments -> preview -> export
```

Every phase must leave production-shaped code behind. Early builds may expose fewer workflow steps,
but persistent data, rendering, undo, cancellation, recovery, and native desktop behavior must use the
same contracts intended for the MVP release.

## 2. MVP Product Boundary

The MVP includes:

- One open project at a time.
- One source image, or a small registered set of related PBR maps.
- Four-point and rectangular patch extraction with perspective correction.
- Per-patch repeat, stretch, unique, trim-cap, padding, bleed, material ID, and map-generation settings.
- Automatic and manually adjustable trim layout generation.
- Base Color, Height, Normal, Roughness, Metallic, AO/Cavity, Region ID, and Material ID outputs.
- Nondestructive treatment layers: Grunge, Edge Wear, Dirt, Color Adjust, Roughness Adjust, Height Boost,
  Decal, and Mask.
- A real-time material preview with at least one pre-hotspotted mesh.
- Folder export that produces maps immediately usable in Blender.
- Durable save, close, reopen, autosave, recovery, undo, and redo.

The MVP explicitly excludes global material-library management, folder indexing, marketplaces, online
providers, a node graph, UV-set management, complex docking, multi-document browsing, and a
Blender-style outliner. A Blender material package is optional only after the required folder export is
complete and proven.

## 3. Native Implementation Direction

### 3.1 Application Shape

Use a native-first desktop architecture with these boundaries:

```text
apps/
  desktop/             Tauri 2 shell, TypeScript UI, native menus/dialogs/window behavior
crates/
  domain/              Stable IDs, project model, commands, validation, undo/redo
  project-store/       SQLite persistence, migrations, locking, autosave, recovery
  image-io/            Bounded decode/encode, metadata, color-space handling
  geometry/            Homography, patch rectification, layout and bleed geometry
  render-core/         Deterministic image operations, layers, map generation, cache keys
  preview/             wgpu texture upload and material/mesh preview
  export/              Export snapshots, presets, atomic writes, validation
packages/
  ui/                   Desktop interaction primitives and accessible controls
  editor/               Workflow state, selection, tools, inspectors, viewport coordination
fixtures/
  images/               Redistributable source and PBR fixtures
  projects/             Versioned project and recovery fixtures
  renders/              Golden patch, map, layer, preview, and export results
docs/
  adr/                  Architecture and format decisions
  support/              Recovery, diagnostics, limitations, and privacy behavior
```

Tauri provides a small signed native shell and OS integration. Rust owns persistent data, geometry,
rendering, validation, and file I/O. TypeScript owns presentation and interaction state, but does not
become a second implementation of project or rendering rules. Large image buffers remain outside JSON
IPC; the UI receives handles, metadata, thumbnails, and progress events.

The existing no-dependency desktop prototype is a UX reference. Its screen flow and terminology may be
retained, but it is not the production runtime architecture.

### 3.2 Rendering Policy

- A deterministic, multithreaded CPU renderer is authoritative for save-time regeneration and export.
- wgpu accelerates interactive compositing and 3D preview. Preview/export differences must be measured
  with golden fixtures and kept within an approved tolerance.
- Render operations are immutable, versioned, cancelable, tile-aware, and cacheable.
- All channels use normalized coordinates so patch boundaries and layer masks stay registered.
- Base Color is color-managed; Height, Normal, Roughness, Metallic, AO/Cavity, masks, and IDs are linear
  data.
- Normal filtering and blending must decode, combine, and renormalize vectors. OpenGL and DirectX
  orientation is an explicit export setting.
- Generated maps are labeled `Estimated` everywhere they appear.

### 3.3 Project and Data Policy

- Use stable UUIDs for sources, patches, layouts, regions, layers, maps, presets, and jobs.
- Persist project state in versioned SQLite with transactional migrations and integrity checks.
- Keep imported source bytes immutable. Store an owned copy or a verified external reference according
  to an explicit import choice; never silently switch between them.
- Store derived thumbnails, rectified patches, previews, and render tiles in a disposable content-addressed
  cache outside authoritative project state.
- Implement commands once in the domain layer and use them for UI actions, undo/redo, autosave, tests,
  and future automation.
- Save atomically, hold a project lock, detect stale locks, retain rotating recovery snapshots, and never
  overwrite the last known-good state during recovery.

## 4. Program Rules

The following are required in every phase rather than deferred hardening work:

- Typed failures with a user-facing explanation and recovery action.
- Automated tests proportional to risk, including malformed input and cancellation paths.
- Versioning and migration before persistent data or serialized operations are introduced.
- Native keyboard, pointer, high-DPI, focus, selection, context-menu, and file-dialog behavior.
- Accessible names, roles, focus order, contrast, and keyboard alternatives for direct manipulation.
- Progress and cooperative cancellation for work that can exceed 100 ms.
- Bounded image dimensions, memory, thread count, cache size, and IPC payload size.
- Structured diagnostics that omit image content and redact user paths from shareable reports.
- No network access in the MVP runtime. Crash reporting, if later added, must be opt-in.
- Measured performance on representative low-, mid-, and high-resolution fixtures.
- Signed and reproducible release artifacts before external distribution.

A phase is complete only when its exit criteria are supported by automated results, golden outputs,
performance measurements, or a recorded manual verification checklist.

## 5. Phase 0 - Engineering Foundation

### Objective

Establish the production repository, native shell, durable contracts, and verification pipeline before
feature implementation spreads.

### Implementation

- Create the Tauri 2 desktop application, Rust workspace, TypeScript packages, and ownership boundaries
  described above.
- Add formatting, linting, type checking, Rust and TypeScript unit tests, integration tests, golden-image
  tests, dependency auditing, and clean-machine packaging checks.
- Define domain IDs, units, coordinate conventions, channel names, color policy, normal orientation,
  deterministic seed policy, and typed error taxonomy.
- Add versioned IPC contracts and reject unknown commands, oversized payloads, and invalid handles.
- Implement application data, project data, cache, log, and recovery directory resolution through OS APIs.
- Establish tracing, redacted support bundles, crash-safe startup/shutdown markers, and developer diagnostics.
- Write ADRs for project persistence, renderer authority, GPU/CPU parity, color management, and source-file
  ownership.

### Required Evidence

- CI builds and tests all supported configurations from a clean checkout.
- Contract tests prove Rust and TypeScript agree on every IPC message.
- A packaged smoke build launches, opens a native dialog, writes only to approved application locations,
  and shuts down cleanly.
- Dependency, license, and parser-threat reviews have no unresolved release blocker.

### Exit Criteria

- The native shell and production module boundaries exist.
- Persistent and IPC formats are versioned before user data is written.
- Later phases can add features without bypassing error, test, diagnostics, or release infrastructure.

## 6. Phase 1 - Native Shell, Project Lifecycle, and Image Import

### Objective

Deliver the first workflow step inside a durable native project that can safely survive close, crash, and
upgrade.

### Implementation

- Implement the seven-step workflow bar, minimal left tool strip, central viewport, right inspector, and
  compact bottom tray from `ux-workflow.md`.
- Keep later steps visibly unavailable until their prerequisites exist; do not expose nonfunctional controls.
- Add New, Open, Save, Save As, Close, Recent Projects, native file/folder dialogs, drag-and-drop, reveal in
  folder, dirty-state prompts, persistent window geometry, and single-instance project routing.
- Implement the project database, migrations, transactions, lock ownership, autosave journal, recovery
  snapshots, integrity check, and recovery UI.
- Import PNG, JPEG, and TIFF source images with EXIF orientation, ICC handling, alpha policy, dimension and
  memory limits, and useful errors. Add only the extra formats needed by the initial PBR-map fixtures.
- Support one Base Color source by default and a small, explicitly assigned PBR set with dimension and
  registration validation.
- Build a mipmapped viewport with pan, zoom, fit, pixel inspection, checkerboard transparency, and responsive
  loading that never decodes a large source on the UI thread.

### Required Evidence

- Migration fixtures cover every schema version and failed/interrupted migration recovery.
- Kill-process tests during save and autosave preserve the previous valid project.
- Malformed, truncated, decompression-bomb, oversized, rotated, color-profiled, and alpha-bearing images fail
  safely or import correctly.
- Keyboard-only and 100%-300% DPI checks cover the shell, dialogs, focus order, and viewport commands.

### Exit Criteria

- A user can create a project, open an image, save, close, reopen, and recover after an unclean shutdown.
- The source is never destructively modified and its ownership status is explicit.
- Image loading, project saving, and recovery cannot block the UI indefinitely or silently lose data.

## 7. Phase 2 - Patch Authoring and Perspective Correction

### Objective

Implement fast, precise patch marking and rectification while preserving editability and source fidelity.

### Implementation

- Add the `Add Patch`, Select, Move, Pan, and Zoom tools with direct manipulation and numeric alternatives.
- Support four-point placement, rectangle placement, live corner adjustment, accept/cancel, duplicate, rename,
  reorder, enable/disable, and delete.
- Validate convexity, winding, minimum area, source bounds, degeneracy, and self-intersection before accepting
  a patch.
- Implement homography estimation and inverse-mapped rectification with appropriate color/data sampling,
  transparent out-of-bounds behavior, and selectable output aspect/scale.
- Show a live rectified preview and make repeated patch creation the default post-accept action.
- Add patch properties for Repeat X, Repeat Y, Tile XY, Stretch, Unique, Trim Cap, padding/bleed, material ID,
  and map-generation participation.
- Route all edits through domain commands with coalesced drag undo, redo, dirty-state tracking, autosave, and
  deterministic cache invalidation.

### Required Evidence

- Property tests cover homography round trips, corner ordering, degeneracy rejection, and coordinate transforms.
- Golden fixtures cover frontal, rotated, skewed, near-boundary, high-resolution, alpha, and color-managed
  sources.
- Interaction tests cover rapid creation, selection changes, drag cancellation, undo/redo, reopen, and high DPI.
- A representative 8K image remains interactive while the rectified preview updates in the background.

### Exit Criteria

- A user can mark several patches, adjust them precisely, assign repeat behavior, and reopen them unchanged.
- Invalid geometry produces a local explanation and recovery path rather than corrupt state or renderer failure.
- Rectification is deterministic and visually matches the approved golden fixtures.

## 8. Phase 3 - Trim Layout Authoring

### Objective

Generate a useful registered trim sheet automatically, then allow focused manual refinement without changing
the source patch definitions.

### Implementation

- Implement layout settings for output resolution, padding, bleed, patch order, auto-pack, horizontal-strip
  priority, vertical-strip priority, fixed selected-patch size, repeat behavior, and trim-cap handling.
- Build a deterministic layout solver with stable tie-breaking and explicit failure diagnostics.
- Preserve normalized cross-channel coordinates and integer output bounds for every region.
- Let users drag boundaries, resize and reorder regions, lock dimensions, set exact numeric values, and rerun
  automatic layout while respecting locks and patch definitions.
- Visualize padding, bleed, trim caps, locked dimensions, overlaps, insufficient resolution, and unused space.
- Assign stable region IDs and deterministic ID colors that survive save/reopen and compatible regeneration.
- Make layout changes command-based, undoable, cancelable, and cache-aware.

### Required Evidence

- Property tests prove non-overlap, in-bounds placement, stable ordering, padding/bleed rules, and deterministic
  results for a fixed input and seed.
- Golden layouts cover mixed repeat modes, locked dimensions, caps, extreme aspect ratios, and impossible fits.
- Regeneration tests prove patch definitions and stable region IDs are retained.
- Large representative patch sets meet the documented interaction and solve-time budgets.

### Exit Criteria

- `Create Trim Sheet` produces a credible first layout from marked patches.
- Users can refine and regenerate the layout without losing source work or cross-channel registration.
- Impossible constraints are reported before export and never produce an apparently valid overlapping sheet.

## 9. Phase 4 - Render Core and Estimated Map Generation

### Objective

Produce deterministic, registered Base Color, Height, Normal, Roughness, Metallic, AO/Cavity, and ID maps
using the math and honesty rules in `mvp-plan.md`.

### Implementation

- Compile sources, rectification, layout, and map settings into immutable versioned render operations.
- Implement bounded tile scheduling, operation halos, full-frame fallbacks, memory estimates, cooperative
  cancellation, progress events, and content-addressed cache fingerprints.
- Compose Base Color from rectified patches with repeat, stretch, unique, cap, padding, and bleed semantics.
- Implement optional de-lighting with low-frequency illumination estimation, amount, radius, shadow recovery,
  highlight recovery, and preserve-color controls.
- Generate Height from Rec. 709 luminance using large-shape blur, high-pass detail, midpoint/gain/clamp, invert,
  edge preservation, and per-patch controls.
- Generate tangent-space Normal from Sobel or Scharr height gradients with strength, detail scale, pre-blur,
  normalization, and OpenGL/DirectX orientation.
- Generate Roughness as an explicitly controllable heuristic using base value, luminance, local contrast,
  high-frequency detail, material ID, clamp, invert, imported maps, and per-patch overrides.
- Default Metallic to zero. Change it only through an imported map, an explicit metal label, or an explicit
  material-ID rule.
- Generate AO/Cavity from multi-radius height differences with radius, strength, bias, invert, and map-or-mask
  use.
- Generate stable Region ID and Material ID maps with exact flat colors and no filtering at boundaries.
- Label generated channels `Estimated` in the tray, inspectors, preview, and export review.

### Required Evidence

- Unit and property tests cover every parameter boundary, seed, color/data rule, and normal orientation.
- Golden 8-bit and 16-bit outputs cover photos, scans, flat textures, imported PBR inputs, repeat seams, caps,
  and ID boundaries.
- CPU results are byte-stable on supported architectures where promised; otherwise tolerances are explicit.
- Cancellation, cache corruption, out-of-memory prediction, and partial-work cleanup tests pass.
- Interactive preview and authoritative CPU output remain within the recorded per-channel tolerance.

### Exit Criteria

- All required maps generate from a valid layout, stay registered, and regenerate deterministically.
- Metallic is never silently inferred from image color, and all inferred outputs are visibly identified.
- Long renders remain responsive, bounded, cancelable, and safe to retry.

## 10. Phase 5 - Nondestructive Treatment Layers

### Objective

Add focused material polish without introducing a node graph or destructive editing path.

### Implementation

- Implement a versioned ordered layer model with visibility, opacity, blend mode, channel targets, mask input,
  seed, strength, scale, and invert.
- Add Grunge, Edge Wear, Dirt, Color Adjust, Roughness Adjust, Height Boost, Decal, and Mask operations.
- Allow layers to target the full layout or selected patches/regions while preserving shared coordinates.
- Implement deterministic procedural noise and edge/cavity masks with explicit seeds.
- Add layer create, duplicate, rename, reorder, group selection, enable/disable, delete, and inspector editing.
- Make drag and slider interaction preview quickly, coalesce undo entries, and schedule authoritative refinement
  after interaction settles.
- Invalidate only affected channel tiles and prevent masks from recursively depending on themselves.

### Required Evidence

- Golden tests cover every layer across each legal target channel and supported blend mode.
- Tests cover ordering, masking, seeded determinism, patch targeting, undo/redo, save/reopen, and cache
  invalidation.
- Dependency validation rejects cycles and unsupported channel combinations with actionable feedback.
- Layer-heavy representative projects meet interaction, memory, save, and regeneration budgets.

### Exit Criteria

- A user can add grunge or edge wear, mask it, target channels, reorder it, and reopen the project unchanged.
- Treatments remain nondestructive, deterministic, registered, undoable, and consistent between preview and
  export.
- No treatment requires node-graph concepts to complete the MVP workflow.

## 11. Phase 6 - 3D Preview and Authoritative Export

### Objective

Prove the trim sheet on relevant geometry and export a complete, validated map set that works in Blender.

### Implementation

- Build a wgpu PBR preview using the same generated map handles and channel conventions as export.
- Provide Plane, Cube, Sphere, Cylinder, Beveled Block, Crate, Wall Module, and Archway fixtures as capacity
  permits; at least one MVP mesh must have authored hotspot UVs that demonstrate actual trim usage.
- Add orbit, pan, zoom, reset, mesh selection, light rotation, environment/exposure controls, and channel/debug
  views without turning preview into a second authoring system.
- Document preview approximations and compare preview shading and channel orientation against Blender fixtures.
- Implement export presets for Blender PBR and generic PBR with output folder, naming template, resolution, bit
  depth, image format, OpenGL/DirectX normal orientation, overwrite policy, and selected maps.
- Export Base Color, Normal, Roughness, Metallic, Height, AO, and ID maps by default. Offer Region Guide and
  preview render only as explicit diagnostics.
- Snapshot project state at job start; render to a staging directory; validate dimensions, channels, bit depth,
  filenames, and checksums; flush; then atomically publish the complete set.
- Add progress, cancellation, retry, conflict prompts, open/reveal in folder, and a concise export report.
- Provide a Blender validation file and short import instructions. A generated Blender material package may be
  added only if it does not delay or weaken the folder-export gate.

### Required Evidence

- Automated export tests cover naming, format, bit depth, normal orientation, overwrite decisions, cancellation,
  disk-full behavior, and atomic publication.
- Blender fixture checks verify color-space assignments, normal direction, roughness/metallic interpretation,
  displacement range, ID flatness, and hotspot alignment.
- GPU preview loss/recreation, device fallback, resize, high DPI, and long-session resource cleanup pass.
- Preview/export comparison images and tolerances are recorded for every required channel.

### Exit Criteria

- A user can judge the sheet on at least one pre-hotspotted mesh and export a complete map set to a folder.
- The exported maps can be connected in Blender without channel repair, flipping, renaming, or realignment.
- Cancellation or failure never leaves a partial folder presented as a successful final export.

## 12. Phase 7 - Integrated MVP Release Qualification

### Objective

Qualify the complete workflow as a safe, supportable native application rather than a collection of passing
feature demos.

### Implementation and Qualification

- Run the acceptance journey from a clean installation: open image, mark several patches, set repeat behavior,
  generate and refine a layout, generate all required maps, add treatment layers, inspect a hotspot mesh,
  export, use in Blender, save, close, and reopen.
- Test representative photos, scans, screenshots, flat textures, and small imported PBR sets at low, typical,
  and maximum supported resolutions.
- Run upgrade, downgrade refusal, migration, autosave, crash recovery, stale lock, cache loss, low disk, long
  path, Unicode path, read-only path, and permission-denied scenarios.
- Complete keyboard-only, screen-reader semantics, focus visibility, contrast, reduced-motion, high-DPI, and
  multiple-monitor checks.
- Measure cold start, project open, patch preview, layout solve, map regeneration, treatment interaction, preview
  frame time, peak memory, save, and export against documented budgets.
- Verify offline operation, path redaction, support-bundle contents, parser limits, dependency audit, license
  notices, installer behavior, uninstall data retention, and signing.
- Write onboarding, Blender import, recovery, diagnostics, known-limitations, project-backup, and uninstall docs.

### Release Gates

- Every acceptance criterion in `mvp-plan.md` passes on a signed clean-machine build.
- No unresolved critical or high security issue remains.
- No known workflow can corrupt a project, overwrite a source, or publish a partial export as valid.
- Preview/export parity, cross-channel registration, ID stability, and generated-map labeling pass golden review.
- Required tests are reliable enough that a pass is meaningful; release-blocking flakes are fixed.
- Performance budgets pass on the minimum supported machine and representative 8K project fixture.
- Recovery has been demonstrated from forced termination during import, save, map generation, and export.

### Exit Criteria

- A signed MVP installer, checksums, release notes, support documentation, and reproducible build record exist.
- The complete user loop works without network access or knowledge of UV sets.
- The release is supportable without relying on developer-only tools or manual project repair.

## 13. Parallelization Plan

After Phase 0 fixes the contracts, work may proceed in parallel with these dependencies:

```text
Phase 0 Foundation
  -> Phase 1 Shell, Persistence, Import
      -> Phase 2 Patch Authoring
          -> Phase 3 Layout
              -> Phase 4 Render and Maps
                  -> Phase 5 Treatments
                  -> Phase 6 Preview and Export
                      -> Phase 7 Release Qualification
```

Useful parallel work:

- Project persistence, migrations, and recovery can advance alongside native shell interaction.
- Homography math and golden fixtures can advance alongside patch-tool UI after coordinate contracts are fixed.
- Layout solver development can advance alongside layout viewport work after region contracts are fixed.
- CPU render operations, wgpu preview operations, and golden-fixture production can advance in parallel after
  channel and color contracts are fixed.
- 3D mesh/Blender fixtures can advance alongside treatment layers after export conventions are fixed.
- Accessibility, threat modeling, diagnostics, performance fixtures, and documentation run continuously.

Parallel branches must converge through shared domain commands and versioned contracts. No branch may create
its own project model, coordinate system, channel convention, or renderer truth.

## 14. Phase Completion Report

Each phase closes with a short report containing:

- Delivered functionality and any approved scope change.
- Contract, schema, migration, and fixture changes.
- Automated test results and golden-output review.
- Performance measurements against named fixtures and hardware.
- Accessibility, security, privacy, and recovery checks performed.
- Known limitations that remain inside the declared MVP boundary.
- Evidence links and the explicit phase-gate decision.

## 15. Non-Negotiable Stop Conditions

Do not advance past a phase gate when any of the following is true:

- Project data, source references, stable IDs, or layer settings can be silently lost or corrupted.
- A schema or render-operation change lacks a tested migration path.
- Patch rectification or layout regeneration is nondeterministic for the same inputs.
- Regions overlap, leave bounds, or lose cross-channel registration without a blocking validation error.
- Generated Metallic is inferred without an explicit user or imported-map decision.
- ID colors change across save/reopen or compatible layout regeneration.
- Preview and export diverge beyond a measured, documented tolerance.
- Background work can deadlock the UI, exceed declared memory bounds, or ignore cancellation indefinitely.
- Export can expose partial outputs as a successful map set.
- A file parser or IPC boundary accepts unbounded or unauthorized input.
- Recovery can overwrite the last known-good project.
- The installer or uninstaller can remove user projects or source images.
- Required release tests are too flaky for a pass to constitute evidence.

Fix the underlying contract in the current phase. Do not reclassify these failures as post-MVP polish.
