# Phase 0 Completion Report

- Phase: Engineering Foundation
- Date: 2026-07-15
- Gate status: Implemented; automated gate passed; one manual UI observation remains

## Delivered

- Tauri 2 native Windows shell with a React/TypeScript presentation layer.
- Rust workspace boundaries for domain, project store, image I/O, geometry, render core, preview, and export.
- Stable UUID domain IDs, normalized coordinates, channel semantics, typed recoverable errors, and IPC protocol
  version 1.
- Minimal Tauri capabilities, a typed `foundation_status` command, native OS paths, structured tracing, and path
  redaction.
- Native folder-dialog wiring used only as a Phase 0 shell smoke action.
- Five accepted architecture decisions covering ownership, persistence, renderer authority, color/channels, and
  source ownership.
- Windows CI, dependency locks, generated platform icons, support foundations, and build/check commands.

## Contracts and Formats

- IPC protocol version: 1.
- Project schema reservation: 1; Phase 0 writes no project records.
- Render operation version: 1.
- Export preset version: 1.
- Cross-language fixture: `fixtures/contracts/foundation-status.json`.

## Verification Evidence

- `npm run check`: passed.
  - TypeScript strict checks passed in all four workspaces.
  - TypeScript IPC fixture test passed.
  - Strict Rust formatting and Clippy with warnings denied passed.
  - Five Rust unit/contract tests passed across the desktop and domain crates.
- `npm run build:native`: passed.
  - Release executable: `target/release/hot-trimmer-desktop.exe`.
  - Observed size: 10,760,704 bytes.
- Native launch smoke: passed.
  - The process remained alive during the four-second startup probe.
  - Windows registered a targetable `Hot Trimmer` window.
  - Roaming app data, local cache, local logs, and recovery directories were created in the expected OS-managed
    locations.
- `npm install` audit: 0 known vulnerabilities at the installed audit level.

## Manual Verification Item

The Windows capture helper could enumerate the native window but failed to capture it with
`SetIsBorderRequired failed: No such interface supported (0x80004002)`. Consequently, the actual visual surface
and native folder-dialog click still require a brief manual smoke check. The plugin, capability, frontend action,
and release executable all compiled successfully; this report does not treat that as equivalent to observing the
dialog.

## Security, Privacy, and Recovery

- Runtime network access is absent from the MVP foundation.
- CSP permits only packaged content, Tauri IPC, local asset URLs, and inline CSS required by the shell.
- Tauri permissions expose core defaults and folder-open only.
- Shareable-path redaction is unit tested.
- Project writes remain disabled until Phase 1 implements transactions, locks, migrations, and recovery.

## Known Limitations Inside the Phase Boundary

- Phase 0 does not create, open, migrate, or save projects.
- Image decoding, rendering, GPU preview, and export dependencies are intentionally not linked before their
  owning phases.
- Installer signing and external distribution are release-qualification work; Phase 0 builds a native executable
  without a bundle.

## Gate Decision

The engineering foundation is suitable for Phase 1 development. Before an external Phase 0 artifact is shared,
manually launch the executable and press **Verify native dialog** once on a supported Windows machine.
