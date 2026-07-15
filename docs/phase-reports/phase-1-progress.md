# Phase 1 Progress Report

- Phase: Native Shell, Project Lifecycle, and Image Import
- Date: 2026-07-15
- Gate status: In progress; first vertical slice implemented

## Delivered in This Slice

- Replaced the engineering-foundation splash with the production seven-step workflow shell, minimal viewport
  tools, source workspace, inspector, bottom asset tray, explicit prerequisite states, and responsive layout.
- Added native New, Open, Save, Close, image-open, and project/image drag-and-drop routing. Project and image
  work runs on the native background pool rather than the UI thread.
- Added schema version 1 as a transactional `SQLite` database with integrity checks, WAL/FULL durability,
  checkpoints, UUID project/source records, and exclusive project lock ownership.
- Added one Base Color source with an explicit owned-copy or verified-external-reference decision. Owned bytes
  are immutable in the project; external bytes are revalidated by SHA-256 when reopened.
- Added bounded PNG, JPEG, and TIFF parsing, conservative decoded-memory prediction, EXIF orientation, alpha
  preservation, ICC-profile detection, SHA-256 identity, and a bounded viewport thumbnail.
- Added viewport pan, zoom, fit, checkerboard transparency, progress state, typed failures, and accessible
  names/focus treatment.

## Contracts and Schema

- IPC protocol remains version 1; all Phase 1 requests carry that version.
- Project schema version 1 introduces `project` and single-channel `sources` records.
- The schema constrains source format, dimensions, ownership/bytes consistency, orientation, digest length, and
  the one-Base-Color Phase 1 boundary.
- Image limits: 16,384 pixels per edge, 1 GiB conservative decoded allocation, and 512 MiB encoded source.
- Viewport thumbnails are bounded to 1,280 pixels on their longest edge and are the only raster payload sent
  through JSON IPC in this slice.

## Automated Evidence

- `npm run check`: passed, including strict TypeScript, contract tests, Rust formatting, Clippy with warnings
  denied, and all workspace tests.
- `npm run build:native`: passed; release executable produced at
  `target/release/hot-trimmer-desktop.exe`.
- Image tests cover alpha import, bounded thumbnail generation, oversized dimensions, truncated input, and
  oriented display dimensions.
- Persistence tests cover v0-to-v1 initialization, close/reopen, concurrent-lock rejection/release, and owned
  source round-trip.
- Cross-language TypeScript tests cover protocol-versioned project and import requests.

## Remaining Before the Phase 1 Gate

- Save As, dirty-close prompts, recent projects, reveal-in-folder, persistent window geometry, single-instance
  project routing, stale-lock decisions, autosave journal, rotating recovery snapshots, and recovery UI.
- Migration fixtures for every schema version plus injected failed/interrupted migration and kill-process save
  tests.
- Full ICC conversion policy (this slice detects and records profiles), TIFF ICC-tag parsing, registered PBR
  source sets, mip pyramids, pixel inspection, and cancelable progressive image loading.
- Malformed-image corpus coverage for decompression bombs, rotated/profiled real-world fixtures, and the full
  keyboard-only and 100%-300% DPI manual matrix.

## Gate Decision

Phase 1 remains open. The delivered slice establishes the authoritative database, ownership, parser bounds,
background-command path, and user-facing shell that the remaining lifecycle/recovery work can extend without
introducing a second project model.
