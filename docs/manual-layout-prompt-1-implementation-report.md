# Manual Layout Product Prompt 1 — Implementation Report

Date: 2026-07-18

## Outcome

Prompt 1 now presents the trim-sheet layout as an authored asset rather than a procedural generator recipe. New source-frame documents instantiate the built-in **Diagonal Cascade** preset, direct topology edits remain command-backed, and the normal Layout panel no longer exposes generator recipe or candidate controls.

The built-in Diagonal Cascade topology is the checked-in 24-region topology from `target/hierarchical-goldens/hierarchical-classic-source-hotspot.golden.svg`. Its ordered `GridRect` records are regression-tested against that SVG.

## Architecture implemented

- Added versioned `AuthoredLayoutPreset` and `AuthoredLayoutPresetRegion` contracts in the Rust domain and TypeScript IPC contracts.
- Preset records contain preset identity, schema version, name, logical grid, canonical aspect, ordered preset-local region keys, `GridRect`, and semantic defaults.
- Reusable presets do not contain project material, `SourceSetId`, `PatchId`, or region-content bindings.
- Applying a preset creates deterministic project `RegionId` values from preset ID, preset-local key, and document instance ID.
- Instantiated regions begin with `InheritPrimaryMaterial` bindings.
- `TrimSheetDocument` persists the exact applied preset snapshot and instance ID, so an open project does not depend on later changes to the external user-preset library.
- Added the authoritative `apply_authored_layout_preset` document command. Preset application advances document/topology revisions and stays on the existing document command/history path.
- New source-frame projects instantiate the authored Diagonal Cascade fixture without calling the partition generator.
- New Blank is one full-grid remainder region and therefore starts as a valid exact cover.
- Stage 14 and `compile_persisted` remain the rendering/orchestration authority; no alternate compositor or preview compiler was introduced.

## Layout product changes

### Presets

The Layout sidebar now provides:

- Diagonal Cascade built-in preset.
- New Blank built-in preset.
- Apply preset.
- Duplicate / Save As.
- Rename.
- Save.
- Revert.
- Delete for user presets.

Built-ins are treated as immutable in the UI. User presets are currently stored in the desktop frontend's versioned local-storage library. The project document separately stores its applied snapshot.

### Generator retirement

- Generator recipe, complexity, shares, radial quota, orientation variation, seed, advanced hierarchy, candidate diagnostics, Update, Accept, and Discard are hidden from the normal product UI.
- Automatic candidate preview/debounce was removed from the active layout path.
- Generator-related code remains present for compatibility/migration work but is not the visible product authority.

### Grid resolution

The grid-resolution dropdown was replaced by a discrete slider. Current stops are:

```text
16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 256
```

The previous 160, 192, and 224 stops were removed. Geometry is rescaled exactly when representable; a non-representable change requires confirmation before applying the quantized preset.

## Direct editing changes

- Preserved the existing command-backed draw, eight-handle resize, split, merge, and legal delete/ownership-transfer operations.
- Direct pointer movement updates local draft topology only and does not start a full material compile.
- Draw mode is now cell-based rather than intersection-based:
  - the complete cell under the pointer is highlighted;
  - the crosshair is displayed at the cell center;
  - pointer-down starts from that exact displayed cell;
  - a click creates a 1×1 cell region;
  - dragging includes both the starting and ending cells in every direction;
  - bottom/right boundaries resolve deterministically to the final legal cell.
- Draw and resize previews show the live rectangle and dimensions.
- Escape cancels the active draw/resize gesture and closes the region context menu.
- Each completed gesture submits one document command and therefore creates one history entry.

## History interaction

Hotspot-sheet Undo/Redo buttons were removed. Document history is available through:

- `Ctrl+Z` — Undo.
- `Ctrl+Y` — Redo.
- `Ctrl+Shift+Z` — Redo.

Shortcuts run only while the application is idle and the requested history direction is available. They do not intercept native undo inside inputs, textareas, selects, or content-editable elements.

Undo/redo requests are serialized so key repeat cannot enqueue operations against stale history state. Every returned document snapshot is reconciled into the existing local artifact, including same-topology history steps; repeated draw undo/redo therefore retains a visible hotspot sheet and selection-safe region/slot metadata instead of clearing the artifact as stale.

## Region context menu

- Right-click uses the application menu and suppresses the browser menu.
- Available operations are Split Horizontal, Split Vertical, Merge / Remove Divider, and Delete / Return Area to Neighbor when legal.
- Illegal delete displays a typed reason rather than silently invalidating coverage.
- The menu now renders through a body-level portal. Its position is based on viewport coordinates, so sheet zoom and pan no longer offset or scale it.
- Menu placement is clamped near the window edges.
- Pointer-down anywhere outside the menu, window focus loss, or Escape closes it. Pointer interaction inside the menu does not dismiss it before the action runs.

## Source preview restoration

Local topology and preset edits preserve the visible source relationship before the next authoritative compile:

- every authored `GridRect` is converted to its exact normalized bounds inside the pinned `SourceFrame`;
- local Stage 14 slot metadata is republished for newly instantiated region IDs;
- selecting a sheet region therefore highlights its corresponding source area immediately;
- explicit source overrides remain distinct from partition-owned bounds.

This restoration changes transient metadata only. The authoritative compiled result still comes from Stage 14.

## Verification evidence

Focused command:

```text
npm.cmd run test --workspace @hot-trimmer/desktop -- manual-layout-presets
```

Latest result: **8 passed, 0 failed**.

The focused suite proves:

- deterministic built-in preset keys and rectangles;
- exact Diagonal Cascade parity with the classic-source-hotspot golden SVG;
- exact-cover New Blank;
- representable grid rescaling and quantization detection;
- cell-centered hover selection and inclusive cell drag behavior at non-integer geometry and outer boundaries;
- selected authored-region SourceFrame bounds;
- transformed-canvas-independent context-menu placement and dismissal wiring;
- keyboard-only hotspot history;
- reduced discrete slider resolutions.

The corrected native backend also compiled successfully with:

```text
cargo build -p hot-trimmer-desktop --target-dir C:\tmp\hot-trimmer-manual-layout-build
```

This confirmed that the Rust domain, project store, IPC command deserialization, and Tauri application compile together with `apply_authored_layout_preset`.

## Remaining acceptance work and known limitations

The following items should not be considered fully accepted yet:

- Full native acceptance has not been completed across create, draw, resize, split, merge, delete, history, save/reopen, and reapply. Windows capture failed with `SetIsBorderRequired failed: No such interface supported (0x80004002)` during the earlier visual-review attempt.
- The running native executable was locked by an open untitled draft, so the normal `target/debug` executable could not be replaced. The corrected backend compiled in an isolated target. Save/close the stale running instance and rebuild/relaunch before native review.
- Existing generated documents do not yet have an explicit load-time migration that snapshots their accepted topology into `authoredLayoutPreset`. They remain readable through the compatibility fields, but the required migration should be implemented and tested before Prompt 1 is declared complete.
- The user-preset catalog is currently versioned local storage rather than a project-store-managed asset catalog. Project snapshots are persisted, but cross-machine/library durability and transactional asset operations remain follow-up work.
- Non-representable grid changes use a confirmation dialog with the quantized result prepared in memory; there is not yet a separate visual before/after quantization overlay.
- The focused Prompt 1 suite does not replace the locked DirectCrop and preview-performance regression suites or the required native acceptance pass.

## Files added or materially changed

- `apps/desktop/src/manual-layout-presets.ts`
- `apps/desktop/src/manual-layout-presets.test.ts`
- `apps/desktop/src/source-first-app.tsx`
- `apps/desktop/src/document-app.css`
- `apps/desktop/test-runner.mjs`
- `apps/desktop/package.json`
- `packages/ipc-contracts/src/document-contracts.ts`
- `crates/domain/src/document.rs`
- `crates/domain/src/lib.rs`
- `crates/project-store/src/lib.rs`

## Recommended review order

1. Save and close any stale running Hot Trimmer instance.
2. Build and relaunch the native app from the current worktree.
3. Create a new Base Color project and confirm the 24-region Diagonal Cascade appears immediately.
4. Verify cell-centered Draw behavior at several zoom and pan states, including the bottom/right cells.
5. Verify context-menu placement and outside-click dismissal at several zoom levels.
6. Verify selected-region SourceFrame highlighting before and after direct edits.
7. Verify `Ctrl+Z`, `Ctrl+Y`, and `Ctrl+Shift+Z` across draw, resize, split, merge, delete, and preset application.
8. Save/reopen the project and reapply a saved user preset to another source.
9. Complete the generated-document migration and durable user-preset asset work before declaring Prompt 1 fully accepted.
