# Prompt 003 review report

Captured: 2026-07-19

> Historical review snapshot: this report records an intermediate Prompt 003 review and was superseded by the later
> accepted Prompt 003-005 implementation. Current queue state is maintained in [`README.md`](README.md).

## Status at capture time (superseded)

Prompt 003 is materially improved, but not fully accepted yet.

The current work proves the raw GPU preview path is usable enough to debug in the app: the desktop can request GPU
Base Color preview artifacts, fetch raw tile bytes, paint them into the sheet surface, copy useful telemetry, and avoid
the most misleading fallback behavior. It does not yet satisfy the full Prompt 003 acceptance bar for exact viewport
authority, capability-sized tile splitting, compact Region ID output, ownership-aware padding/dilation, completed
telemetry, and the required native walkthrough.

Recommendation: review this report before deciding whether to continue Prompt 003 remediation. Prompt 004 should remain
blocked until the remaining Prompt 003 acceptance items are either completed or explicitly descoped.

## Completed checklist

- [x] Raw binary Tauri payload fetch/release commands are registered.
- [x] Frontend accepts Tauri binary payloads and paints through `ImageData`.
- [x] Stale generations are rejected and handles are released.
- [x] Native job ID is the publication-generation authority.
- [x] GPU executor supports bounded partial output rectangles.
- [x] WGSL uses global atlas coordinates while writing tile-local coordinates.
- [x] Selected-region requests use stable `RegionId`.
- [x] Native compilation resolves the authoritative selected-region rectangle.
- [x] Pixel-cache reuse excludes publication generation.
- [x] Cache hits receive a new current-generation manifest.
- [x] GPU readback uses reusable pooled `wgpu::Buffer` allocations.
- [x] Tile pixels use shared `Arc<[u8]>` storage.
- [x] Partial tile bytes are not published as a complete-atlas channel.
- [x] Silent Source Frame fallback was removed.
- [x] User-approved radial and diagnostic-profile changes are permitted.
- [x] `multi-source-patch-assignment` stale expectations were repaired and the focused command passes.

Relevant implementation locations:

- `crates/sheet-compiler/src/atlas_executor.rs`
- `crates/sheet-compiler/src/persisted_pipeline.rs`
- `apps/desktop/src-tauri/src/document_commands.rs`
- `apps/desktop/src/source-frame-preview-controller.ts`
- `apps/desktop/src/source-first-app.tsx`

## Fixes applied

### Raw tiled preview publication

- Added a GPU tile manifest to the preview artifact contract with generation, map, output rectangle, valid rectangle,
  dimensions, row stride, pixel format, and an opaque payload handle.
- Added native commands to fetch and release GPU tile payloads by handle.
- Changed the desktop display path to fetch raw `Uint8Array` payloads and paint via `ImageData`.
- Added payload validation so byte count, row stride, valid rect, and pixel format must match the manifest before paint.

Why: Prompt 003 requires interactive preview pixels to stop travelling as PNG/Base64. Raw bytes make phase timing and
payload size honest, and they expose whether the GPU produced actual pixels rather than a decoded image artifact.

### Tiled interactive Base Color path

- Prompt 002 provided the compact GPU Base Color execution foundation.
- Prompt 003 made that path interactive by adding tile request metadata, raw payload publication, frontend tile painting,
  stale-generation rejection, cache handles, and debug telemetry.
- Added diagnostic complete preview profiles for 1024, 2048, 4096, and 8192.

Why: the desktop needed an inspection path that uses the production GPU renderer without asking the frontend to perform
sampling, crop, radial, padding, or atlas math.

### Frontend paint and stale generation behavior

- Added `GpuTiledPreviewPainter` to own frontend display resources only.
- Rejects stale generations before paint and releases their native handles.
- Keeps the previous valid artifact visible while a replacement preview renders.
- Removed the lower-quality draft flash during radial or assigned patch edits.

Why: the editor should not disappear or briefly show a square/lower-quality intermediate when a radial or patch edit is
in flight. The product needs to feel stable even when a new tile has not arrived yet.

### Removed misleading source fallback

- Removed the silent Source Frame texture fallback behind region overlays.
- Added a Prompt 003 note that any future fallback must be explicit and visibly marked as diagnostic.

Why: the fallback made a failed Stage 14 render look successful. During GPU remediation, a blank or wrong tile should be
obvious so the bug can be fixed instead of hidden.

### Preview sizes and auto-refresh

- Added interactive preview profiles for `1024`, `2048`, `4096`, and `8192`.
- Fixed automatic preview suppression to key by `documentRevision + profile`, not just revision.
- The size dropdown now triggers a real new preview request when changing profile.
- Raised the default single-tile GPU cache from `64 MiB` to `512 MiB` as an 8K stress-test bridge.

Why: the earlier dropdown felt inert because changing from 2K to 4K/8K could be suppressed when the document revision
did not change. The cache bump lets one 8K RGBA tile publish for testing, but it is not the final architecture.

### Copy telemetry and debug

- Added a `Copy telemetry + debug` action that captures preview progress, artifact metadata, tile manifest, native
  telemetry, frontend paint summary, display gates, canvas pixel summary, and current UI preview profile.
- Added `F2` as a one-key shortcut for the same copy operation.

Why: the failure mode is often visual and timing-dependent. One-key capture makes it much easier to attach the exact
state needed for debugging without manually selecting status text.

### Radial editing cleanup

- Removed the visible `Transition width` inspector control.
- Removed the transition-width ring and drag handle from the radial gizmo.
- Kept `Falloff` as the primary radial shaping control.
- Changed new radial defaults so `blendWidth` starts at `0` in both frontend and domain defaults.

Why: the transition-width control made radial edits look like a square or partial intermediate while the real radial
render caught up. Starting at zero and using falloff as the user-facing shaping control makes the authored state less
surprising.

### Prompt notes

- Updated `docs/render/prompt-003.md` with explicit notes for:
  - no silent Source Frame substitution;
  - no partial selected/viewport tile masquerading as a full sheet;
  - 1024/2048/4096/8192 complete preview stress profiles;
  - the 512 MiB cache bump being temporary until split manifests exist.

## Verification run

Passed:

```powershell
npm.cmd run typecheck --workspace @hot-trimmer/desktop
npm.cmd run test --workspace @hot-trimmer/desktop -- gpu-tiled-preview
npm.cmd run test --workspace @hot-trimmer/desktop -- src/document-workbench.test.ts
npm.cmd run test --workspace @hot-trimmer/desktop -- src/source-frame-preview-performance.test.ts
npm.cmd run test --workspace @hot-trimmer/desktop -- multi-source-patch-assignment
cargo check -p hot-trimmer-sheet-compiler
cargo check -p hot-trimmer-desktop
git diff --check
```

Focused test repair:

- Before this report correction, `multi-source-patch-assignment` had 17 passes and 2 failures.
- The failures were obsolete report/test expectations: one expected the old `lastAutomaticPreviewRevision` token, and
  one used a whitespace-sensitive Rust regex for `patch_domain_cache_key(...)`.
- Those expectations were updated to the profile-aware `lastAutomaticPreviewKey` behavior and a formatting-tolerant
  cache-key assertion.
- The focused command now passes: 19 desktop assertions passed, plus the filtered Rust test passed.

## Telemetry interpretation

Observed 4K preview telemetry showed GPU rendering was not the primary bottleneck in that path:

- `output=4096x4096`
- `upload_ms=16`
- `render_ms=90`
- `readback_ms=74`
- frontend/publish/paint path still took seconds with a 64 MiB raw payload

Observed authoritative 2K telemetry was much slower:

- `output=2048x2048`
- upload and render were each around several seconds
- native elapsed was around 8.5 seconds
- total request-to-paint was around 9 seconds

These 2K, 4K, and 8K comparisons are useful debugging evidence, but they are not apples-to-apples benchmarks until
cold/warm state, cache state, profile, tile rectangle, and frontend paint phases are recorded consistently.

Current interpretation: some GPU paths are already fast, but the product still pays heavily for upload/readback,
publication, frontend row copy/ImageData upload, canvas paint, and the current single-tile architecture. The 8K cache
error was a symptom of submitting one 256 MiB tile payload, not proof that 8K is impossible.

## Real Prompt 003 gaps

### 1. Exact viewport authority

Current problem: authoritative preview without a selected region still becomes a complete-output request. The frontend
currently leaves `viewportRect` undefined.

Fix:

- Convert the visible canvas viewport from pan/zoom coordinates into authoritative atlas pixels.
- Send that rectangle with `exactViewport`.
- Validate it natively.
- Add a test proving a pan changes `outputRect` without changing sampling coordinates.

### 2. Capability-sized tile splitting

Current problem: one request produces one manifest. Large rectangles, including diagnostic 8K profiles, remain single
giant tiles.

Fix:

- Split requested rectangles by the GPU-selected tile edge, likely around 2048.
- Publish multiple manifests.
- Schedule visible/selected tiles first, neighbors second, background refinement last.
- Update the painter to apply manifests incrementally while preserving the previous draft.
- Remove the 512 MiB single-tile bridge once splitting works.

### 3. Compact Region ID output

Current problem: `R32Uint` and compact lookup types exist, but the GPU does not yet produce an ownership tile.

Fix:

- Write each matched command's compact region index into a tile-sized `R32Uint` texture.
- Publish the compact-index-to-stable-`RegionId` lookup.
- Only read back or publish the ID tile when selection, diagnostics, or padding needs it.
- Test pixels from every region and atlas edge.

### 4. Ownership-aware padding/dilation

Current problem: halo rectangles exist, but no GPU dilation pass fills padding while respecting region ownership.

Fix:

- Use the compact ownership texture during dilation.
- Restrict expansion to each region's authorized allocation/padding bounds.
- Require sufficient halo for the configured padding.
- Test adjacent regions with contrasting colors to prove no cross-region bleed.
- Compare tile edges against monolithic common texels.

### 5. Telemetry completion

Current problem: `raw_ipc_ms` is not yet a reliable measured IPC phase, and frontend upload versus paint is not separated
with enough precision.

Fix:

- Measure payload-fetch round trip in the frontend.
- Separately time row tightening, `ImageData` construction/upload, `putImageData`, and final canvas paint.
- Report cache hit/miss/eviction and bytes.
- Join every phase using native generation.
- Record cold/warm state, adapter/backend, profile, plan hash, and tile rectangle.

### 6. Required native walkthrough

Still required acceptance evidence:

- 512 navigation preview.
- 1024 refinement while the draft remains visible.
- Select radial region.
- Request exact 1:1.
- Edit while prior work is active.
- Pan across a tile boundary and revisit it.
- Record cache and request-to-paint telemetry.

## False gaps removed

These are no longer listed as runtime blockers:

- Selected region still trusts frontend profile-scaled coordinates.
- Pixel-cache identity still includes generation.
- GPU staging buffers are not pooled.
- Tile bytes are cloned through every layer.
- Partial tile bytes are stored as a complete-atlas channel.
- Native publication generation still comes from frontend draft ID.
- Original edit-boundary objections superseded by user-approved radial and diagnostic-profile changes.

## Decision for Prompt 004 at capture time (superseded)

At the time of this review, Prompt 004 depended on Prompt 003 having an exact, capability-bounded, trustworthy preview
path. That state was good enough to debug visually and gather telemetry, but was not then good enough to accept Prompt
003 as complete.

Prompt 004 therefore remained blocked at capture time until the gaps above passed. This decision is historical and has
been superseded by the completed Prompt 003-005 implementation recorded in the queue ledger.
