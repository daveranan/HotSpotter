# Hot Trimmer

Hot Trimmer is a focused image-to-trim-sheet desktop app.

It is not a full DCC, a general material-library manager, or a Blender/Substance clone. The first
product pass is intentionally narrow:

```text
Open image -> mark patches -> create trim layout -> generate maps -> add treatments -> preview -> export
```

## Project Shape

- `docs/mvp-plan.md` is the product and implementation plan.
- `docs/phases.md` is the production implementation program.
- `docs/ux-workflow.md` describes the intended user workflow and screen model.
- `apps/desktop` contains the Tauri 2 native shell and React presentation layer.
- `crates` contains the Rust domain, persistence, geometry, image, render, preview, and export boundaries.
- `packages` contains shared TypeScript UI, editor, and versioned IPC contracts.
- `fixtures` contains cross-language contract fixtures and, in later phases, render/project fixtures.

The MVP release target is Windows 10/11 x64. Core Rust contracts remain portable for later platform
qualification.

## Current Capability

Phase 1 provides the native seven-step shell and the complete project/image lifecycle: versioned SQLite
projects, New/Open/Save/Save As/Close, recent projects, dirty-state handling, locks, autosave and recovery,
single-instance routing, and bounded PNG/JPEG/TIFF import. Base Color and explicitly assigned registered PBR
channels can be inspected through a mipmapped pan/zoom viewport without modifying source bytes.

See `docs/phase-reports/phase-1.md` for the gate evidence and `docs/support/recovery.md` for recovery behavior.

## Local Commands

```powershell
npm.cmd install
npm.cmd run check
npm.cmd run dev
npm.cmd run build:native
```

`npm run dev` launches the native Tauri application. `npm run build:native` creates a clean native executable
without producing an installer bundle; signed installer qualification is a release activity.
