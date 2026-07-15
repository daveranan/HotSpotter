# Phase 2 Completion Report

- Phase: Patch Authoring and Perspective Correction
- Date: 2026-07-15
- Gate status: Implementation complete; automated gate and optimized native build passed

## Delivered Functionality

- Hands-on review replaced the separate Sources destination and overlay-heavy patch canvas with one fixed split:
  the material-source/patch workplace is on the left and the evolving hotspot workpiece is on the right.
- Four-point capture accepts any click order, canonicalizes it internally, and commits after the fourth point.
  Rectangle capture commits on release. Outline Fit commits automatically at eight points, with Enter allowing
  an intentional early four-to-seven-point fit. Escape cancels incomplete capture or restores a manipulation.
- A selected patch moves by dragging its interior and exposes scale handles on its actual corners. Rotation uses
  cursor-only hit zones outside those corners without an extra bounding box or visible gizmos. Double-click switches
  to labeled TL/TR/BR/BL point editing. Stable viewport-level pointer capture and drag-generation guards prevent
  rerenders or earlier native command completions from resetting an active edit.
- One sidebar patch list supports double-click rename and drag reorder. Duplicate, enable/disable, rename, and
  confirmed delete live in a right-click context menu. The duplicate bottom tray and floating preview were removed.
- Cached WebGL rectification updates directly in the right workpiece throughout pointer movement. The bounded,
  deterministic native preview refines after interaction settles and never blocks direct manipulation.
- Region behavior uses icon-backed Single, Horizontal Loop, Vertical Loop, Tile, Stretch, and Trim Cap
  language while preserving repeat, padding, bleed, material ID, map participation, aspect, and scale contracts.
- The selected material exposes explicit channel slots with Base Color, Normal, Height, Roughness, and other
  swatches. Bulk import only fills matching empty roles; it never guesses an unrelated image into a data-map slot.
- All accepted edits use typed domain commands. Geometry drags coalesce into one history operation; undo/redo,
  dirty state, autosave journaling, recovery refresh, and patch-scoped cache invalidation are deterministic.

## Geometry and Rectification

- Normalized coordinates reject non-finite or out-of-range data both at construction and deserialization.
- Quadrilateral validation rejects duplicate corners, self-intersection, wrong winding, concavity, degeneracy,
  insufficient area, and source-bound violations with local recovery guidance.
- Polygon assistance orders four to eight points into an editable best-fit quadrilateral and optionally retains
  the input polygon as a mask.
- Rectification estimates and inverts a homography, inverse maps every output sample, applies alpha-aware
  bilinear sampling for color and nearest/data sampling for linear channels, and returns transparency outside
  the source. Aspect and scale are bounded before allocation.
- Full source decode and preview rendering execute on background jobs with decode/render cancellation tokens,
  monotonic progress, stale-result rejection, and no partial result publication.

## Contracts, Schema, and Recovery

- Project schema version 5 adds ordered `patches` rows containing validated versioned patch JSON and a deferred
  source foreign key. The v4-to-v5 migration is transactional.
- Save As, reopen, baseline, rotating recovery snapshots, and source replacement preserve patch identity and
  geometry. Removing a source in use is rejected; replacing it reassigns dependent patches transactionally.
- IPC protocol version 1 now types patch commands, polygon assistance, committed and draft preview requests,
  progress events, history availability, project patches, and cancellation. The cross-language Phase 2 fixture
  verifies camel-case wire names.

## Automated Evidence

- Domain tests cover JSON/wire shape, corrupt normalized values, metadata bounds, rapid commands, coalesced drag,
  save-point dirty tracking, create/duplicate/reorder/delete, undo, and redo.
- Geometry property tests cover all 24 corner orders, 512 generated skewed quadrilaterals with homography round
  trips, coordinate transforms, degeneracy/winding/crossing rejection, polygon assistance, and output bounds.
- The SHA-256 golden matrix covers frontal, rotated, skewed, near-boundary, alpha, sRGB/color, linear-data, and
  representative 8K inputs. Cancellation tests prove monotonic progress and no partial publication.
- Persistence tests cover schema-v4 migration, patch command transactions, undo/redo, reopen, source reassignment,
  removal protection, and recovery snapshots containing committed patch geometry.
- Interaction tests cover arbitrary and counter-clockwise capture order, automatic completion geometry, direct
  move/resize/rotate transforms, projective live-preview mapping, repeated drafts, selection changes, pointer-drag
  cancellation, undo/redo/reopen through the native state path, and coordinate invariance at 100%/300% scale.
- The full `npm run check` gate includes strict TypeScript, Rust formatting, Clippy with warnings denied, all
  workspace tests, fixtures, parser limits, and kill-process durability tests. `npm run build:native` produces
  the optimized Tauri executable, which launched as a targetable `Hot Trimmer` window. Deeper automated window
  capture was unavailable on the qualification host (`0x80004002`), so no visual DPI result is inferred.

## Performance, Accessibility, and Safety

- The 8,192 x 1,024 render case produces a 512 x 64 rectified preview inside a five-second debug-test ceiling;
  the complete five-case render matrix runs in under one second on the qualification machine. The existing
  8,192 x 8,192 decode/mipmap test remains inside its 30-second ceiling.
- Interaction preview uses an already-loaded bounded thumbnail as a WebGL texture, updates synchronously with
  geometry state, and performs no IPC or image decode in the pointer path. Authoritative preview work remains
  background, bounded to a 2,048-pixel edge and one-GiB allocation, cancelable, and discarded when superseded.
- Every tool is named, corner handles expose slider roles with numeric alternatives, focus remains visible,
  keyboard shortcuts cover creation/history/view actions, dialogs remain modal, and reduced motion is honored.
- Keyboard navigation is covered by shortcut and contextual-state tests; 100%/300% display-coordinate invariance
  is covered by scaled viewport tests. Paths are bounded/redacted in diagnostics, decoding rejects oversized
  input before allocation, and no network service is used.

## Known MVP Boundaries

- Phase 2's right workpiece shows rectification and temporary region intent, not the deterministic packing solver
  owned by Phase 3. Phase 3 also owns the schema migration from one registered input set to multiple ordered
  material sources and patch-independent layout regions.
- Preview IPC returns a bounded PNG data URL; full-resolution render tiles remain native and are Phase 4 work.
- An individual image-codec call cannot be interrupted mid-call, but dimensions and conservative decoded memory
  are bounded before it begins and cancellation is observed at every surrounding stage.

## Gate Decision

Phase 2 is complete. Users can author multiple precisely editable patches, assign behavior, preview deterministic
rectification, undo/redo, save, recover, and reopen without geometry loss. Invalid data is rejected locally and
cannot enter persistence or the renderer. No Phase 2 acceptance item is deferred.
