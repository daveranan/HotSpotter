# Phase 1 Completion Report

- Phase: Native Shell, Project Lifecycle, and Image Import
- Date: 2026-07-15
- Gate status: Implementation complete; automated gate passed; packaged keyboard/DPI record pending

## Delivered Functionality

- A native three-mode authoring shell—Sources, Patches & Layout, and Maps & Polish—with separate Export and Send
  to Blender actions, a compact viewport HUD, material-input manager, and source tray.
- New, Open, Save, Save As, Close, Recent Projects, drag-and-drop, reveal in folder, native dialogs, persistent
  window state, dirty Save/Discard/Cancel handling, and single-instance project routing.
- A versioned SQLite project with transactions, integrity checks, one-writer locks, stale-lock detection,
  autosave journal, last-explicit-save baseline, five rotating recovery snapshots, and Recover As publication
  that never overwrites an existing project.
- Bounded PNG, JPEG, and TIFF import with EXIF orientation, alpha preservation, ICC detection and Base Color
  conversion to sRGB for display, explicit owned/external source identity, cooperative cancellation, progress,
  and typed recovery guidance.
- Ten explicit material-input slots: Base Color/Diffuse, Normal, Height/Bump, Roughness, Metallic, Ambient
  Occlusion, Specular, Opacity, Edge Mask, and Material ID. Registration requires Base Color and exact dimensions.
- Open images is a direct first-run action; Open all auto-assigns filename-recognized texture sets, imports Base
  Color first, and preserves filled slots. Actual filename, original path, and dimensions replace ownership/ICC/
  alpha detail in the Sources UI. Project name is directly editable in the top bar.
- Three thumbnail mip levels, pan, zoom, fit, checkerboard transparency, channel switching, and pixel-coordinate
  RGBA inspection. Decode and persistence execute outside the UI thread.

No Phase 2 authoring control is exposed as functional.

## Implemented Runtime Logic

1. **Direct start:** Open images selects one or more files before project creation. The user then chooses the
   `.hottrimmer` destination; the new project is created and the selected files are imported.
2. **Open all assignment:** filenames are token-matched against Base Color/Diffuse, Normal, Height/Bump,
   Roughness, Metallic, AO, Specular, Opacity, Edge Mask, and Material ID. Base Color is selected and committed
   first. Recognized companions fill matching empty slots; ambiguous files use visible empty-slot order. Filled
   slots are never replaced by a batch operation.
3. **Partial batch failure:** every image import is its own authoritative transaction and recovery refresh. If a
   later file fails or is cancelled, earlier successful imports remain visible and durable.
4. **Registration:** companion inputs require Base Color and must match its oriented dimensions. Individual
   Add/Replace remains available when filename inference needs correction.
5. **Source provenance:** schema v4 stores `origin_path` separately from owned/external storage policy. The UI
   shows filename, original path, dimensions, and role while keeping ownership, alpha, ICC, and data-policy
   details out of the routine inspector.
6. **Project rename:** editing the top-bar name journals `rename_project`, marks the project dirty, refreshes
   recovery, and updates Recent Projects best-effort. Enter commits; Escape restores the previous name.
7. **Viewport:** source selection chooses an appropriate bounded thumbnail mip. Drag pans, wheel/buttons zoom,
   Fit resets the transform, the lower-left HUD reports scale, and the lower-right readout reports sampled RGBA.
8. **Durability:** source edits and rename commit before recovery refresh. Recovery publication failure returns a
   visible warning while keeping the authoritative edit and dirty state intact.

## Specified but Not Yet Implemented

- Patch capture, sheet layout, generated maps, treatments, embedded 3D preview, Export, and Send to Blender remain
  later-phase contracts. Their controls are disabled or absent; no inert control is presented as functional.

## Contracts, Schema, and Fixtures

- IPC protocol remains version 1. Phase 1 requests, lifecycle dispositions, recovery paths, source channels,
  dirty state, ownership, stale-lock state, and thumbnail mipmaps are typed on both sides.
- Project schema version 4 adds durable original-import provenance while preserving v1 Base Color, v2 PBR, and
  v3 ten-slot projects.
- Migration is transactional from v0 through v4 with schema/data fixtures for each historical transition and
  rollback coverage.
- Image limits are 16,384 pixels per edge, 1 GiB conservative decoded allocation, and 512 MiB encoded input.
  UI raster IPC is limited to bounded 320, 640, and 1,280-pixel thumbnail mipmaps.

## Automated Evidence

- `npm run check`: passed on 2026-07-15, including strict TypeScript, Clippy with warnings denied, all Rust/TS
  tests, schema fixtures, parser limits, and kill-process durability tests.
- `npm run build:native`: passed on 2026-07-15 as an optimized no-bundle Tauri production build. The standard
  `target/release/hot-trimmer-desktop.exe` was rebuilt and relaunched as a targetable `Hot Trimmer` window.
- Persistence tests cover new/current migrations, the v1 fixture, migration rollback, active and stale locks,
  PBR registration, Save As, backup/restore, baseline preservation, and five-snapshot rotation.
- Separate child-process tests force termination after autosave commit and during Save; reopening retains a
  valid previous project and committed autosave journal.
- Image tests cover PNG alpha, JPEG, TIFF, real JPEG EXIF orientation, embedded ICC conversion, linear-data ICC
  preservation, encoded and decoded limits, oversized dimensions, truncation, cancellation, and the 8K fixture.
- Cross-language fixtures cover foundation plus project/import/lifecycle/recovery snapshots, warnings, expanded
  material slots, source provenance, dirty state, and thumbnail-mipmap shapes. Dedicated assignment tests cover
  common PBR suffixes, Base Color-first ordering, and refusal to silently replace occupied slots.
- Static shell checks require dialog roles, accessible naming/live regions, progress labeling, and reduced
  motion support. Strict TypeScript and Clippy run with warnings denied.

## Performance

Fixture: generated 8,192 x 8,192 grayscale PNG, decoded under the production bounds and reduced to all three
thumbnail mip levels. A debug test run completed in 16.81 seconds (17.09 seconds wall clock), under its 30-second
acceptance ceiling, on Windows 10.0.19045 with an AMD64 Family 25 Model 33 processor and 32 logical processors.
The test also asserts that each mip stays within its declared edge and IPC size bounds.

## Accessibility, Security, Privacy, and Recovery

- Keyboard shortcuts cover New, Open, Save, Save As, Close, Fit, Zoom In, and Zoom Out. All workflow, project,
  source, tray, and dialog actions are native buttons/selects with visible focus. Modal focus is contained,
  Escape cancels safely, focus is restored, progress is announced, and reduced motion is honored.
- Packaged keyboard-only and process-local 100%/300% scale checks: pending final verification.
- File parsers reject input before unbounded allocation, IPC paths are length-bounded, external sources are
  SHA-256 revalidated, and authoritative source bytes are never rewritten.
- Baselines use immutable generations; the prior generation is not moved or removed until its successor is
  validated, flushed, and adopted. Recovery publication failure becomes a visible warning with dirty state intact.
- Runtime behavior is offline. Shareable diagnostics redact paths and do not include image content.
- Recovery snapshots are integrity checked, recovery uses a new destination, and forced-termination tests prove
  the previous project remains valid.

## Known Limitations Inside the MVP Boundary

- Phase 1 sends bounded PNG thumbnail data through JSON IPC. Full-resolution image buffers and render tiles stay
  out of IPC and are work for later rendering phases.
- Cancellation is cooperative between bounded parser/decode/profile/mipmap/persistence stages; an individual
  image-codec call cannot be interrupted mid-call, but its dimensions and predicted allocation are bounded first.
- Base Color color management is applied to viewport mipmaps. Immutable authoritative source bytes retain their
  embedded profile for the later authoritative renderer.

## Gate Decision

Phase 1 implementation and automated acceptance are complete. The release-verification gate remains open only
for the packaged keyboard-only and 100%/300% DPI checklist. No Phase 1 code is deferred by that manual record.
