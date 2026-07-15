# Phase 1 Completion Report

- Phase: Native Shell, Project Lifecycle, and Image Import
- Date: 2026-07-15
- Gate status: Candidate pending packaged-app verification

## Delivered Functionality

- A native seven-step workflow shell with unavailable downstream steps, viewport tools, source inspector, and
  compact asset tray.
- New, Open, Save, Save As, Close, Recent Projects, drag-and-drop, reveal in folder, native dialogs, persistent
  window state, dirty Save/Discard/Cancel handling, and single-instance project routing.
- A versioned SQLite project with transactions, integrity checks, one-writer locks, stale-lock detection,
  autosave journal, last-explicit-save baseline, five rotating recovery snapshots, and Recover As publication
  that never overwrites an existing project.
- Bounded PNG, JPEG, and TIFF import with EXIF orientation, alpha preservation, ICC detection and Base Color
  conversion to sRGB for display, explicit owned/external source identity, cooperative cancellation, progress,
  and typed recovery guidance.
- A Base Color source plus explicitly assigned Normal, Height, Roughness, Metallic, and Ambient Occlusion
  sources. PBR registration requires an existing Base Color and exact dimensions.
- Three thumbnail mip levels, pan, zoom, fit, checkerboard transparency, channel switching, and pixel-coordinate
  RGBA inspection. Decode and persistence execute outside the UI thread.

No Phase 2 authoring control is exposed as functional.

## Contracts, Schema, and Fixtures

- IPC protocol remains version 1. Phase 1 requests, lifecycle dispositions, recovery paths, source channels,
  dirty state, ownership, stale-lock state, and thumbnail mipmaps are typed on both sides.
- Project schema version 2 adds explicit PBR channels while preserving version 1 Base Color records.
- Migration is transactional from v0 to v1 and v1 to v2. The `schema-v1.sql`, `data-v1.sql`, and
  `migrate-v1-to-v2.sql` fixtures cover the only historical schema transition.
- Image limits are 16,384 pixels per edge, 1 GiB conservative decoded allocation, and 512 MiB encoded input.
  UI raster IPC is limited to bounded 320, 640, and 1,280-pixel thumbnail mipmaps.

## Automated Evidence

- `npm run check`: pending final recorded run.
- `npm run build:native`: pending final recorded run.
- Persistence tests cover new/current migrations, the v1 fixture, migration rollback, active and stale locks,
  PBR registration, Save As, backup/restore, baseline preservation, and five-snapshot rotation.
- Separate child-process tests force termination after autosave commit and during Save; reopening retains a
  valid previous project and committed autosave journal.
- Image tests cover PNG alpha, JPEG, TIFF, real JPEG EXIF orientation, embedded ICC conversion, linear-data ICC
  preservation, encoded and decoded limits, oversized dimensions, truncation, cancellation, and the 8K fixture.
- TypeScript contract tests cover foundation, project/import, lifecycle/recovery, dirty state, PBR channel, and
  thumbnail-mipmap shapes.
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
- File parsers reject input before unbounded allocation, external sources are SHA-256 revalidated, authoritative
  source bytes are never rewritten, and project/database writes are transactionally or atomically published.
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

The Phase 1 gate remains pending only until the final clean verification and packaged-app keyboard/DPI checklist
are recorded. No scope change or deferred Phase 1 contract is approved.
