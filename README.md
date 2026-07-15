# Hot Trimmer

Hot Trimmer is a focused image-to-trim-sheet desktop app.

It is not a full DCC, a general material-library manager, or a Blender/Substance clone. The first
product pass is intentionally narrow:

```text
Sources -> Patches & Layout (with embedded preview) -> Maps & Polish -> Export / Send to Blender
```

Current delivery: Phase 0 through the revised Phase 2 implementation and automated gates are complete. Durable
projects, multi-image auto-assignment, provenance display, the integrated source/patch workplace, real-time
perspective correction, and the hotspot workpiece are working. Multiple material-source sets and authoritative
layout remain Phase 3; later output capabilities stay disabled or absent until their phases are implemented.

## Project Shape

- `docs/mvp-plan.md` is the product and implementation plan.
- `docs/phases.md` is the production implementation program.
- `docs/ux-workflow.md` describes the intended user workflow and screen model.
- `docs/technical-spec.md` defines interaction, slot, patch, layout, persistence, and accessibility contracts.
- `apps/desktop` contains the Tauri 2 native shell and React presentation layer.
- `crates` contains the Rust domain, persistence, geometry, image, render, preview, and export boundaries.
- `packages` contains shared TypeScript UI, editor, and versioned IPC contracts.
- `fixtures` contains cross-language contract fixtures and, in later phases, render/project fixtures.

The MVP release target is Windows 10/11 x64. Core Rust contracts remain portable for later platform
qualification.

## Current Capability

Phases 1 and 2 provide the native workspace shell, complete project/image lifecycle, and editable patch
authoring: versioned SQLite
projects, New/Open/Save/Save As/Close, recent projects, dirty-state handling, locks, autosave and recovery,
single-instance routing, and bounded PNG/JPEG/TIFF import. Ten explicit registered material-input slots can be
managed and inspected through a mipmapped pan/zoom viewport without modifying source bytes. Patches can be
captured with rectangle, four-point, or assisted polygon tools, corrected numerically or directly, assigned
repeat/material behavior, and previewed through deterministic background rectification.

See `docs/phase-reports/phase-2.md` for the current gate evidence and `docs/support/recovery.md` for recovery behavior.

## Local Commands

```powershell
npm.cmd install
npm.cmd run check
npm.cmd run dev
npm.cmd run build:native
```

`npm run dev` launches the native Tauri application. `npm run build:native` creates a clean native executable
without producing an installer bundle; signed installer qualification is a release activity.
