# Hot Trimmer Base Color product prompt pack

## One authoritative sequence

This is the authoritative SourceFrame Base Color implementation sequence. It supersedes
docs/runtime-integration-gate-prompt-pack.md; do not run or merge that obsolete fixed-template pack.

There are six prompts total. Prompt 1 is complete and locked. Run Prompts 2–6 in order, one Codex task per prompt, and
visually review the native application after each task.

| Prompt | Visible product result | Status |
| --- | --- | --- |
| 1 | One SourceFrame divided into unique DirectCrop regions | Complete / locked |
| 2 | Truthful selection and patch-quality SourceFrame/region editing | Next |
| 3 | Interactive cached 512 preview on large sources | Pending |
| 4 | Intentional panel/strip/detail layout authoring | Pending |
| 5 | Radial/disc/annulus authoring without reuse | Pending |
| 6 | Edge-dilated padding and 8K/16K/24K qualification | Pending |

Product path:

~~~text
arbitrary source image
→ movable atlas-aspect SourceFrame
→ user-controlled logical partition
→ one unique source rectangle per region
→ optional explicit per-region override
→ direct Base Color sampling through compile_persisted
→ optional radial mapping inside its owning cell
→ owning-edge padding/dilation
→ interactive preview and authoritative refinement
~~~

The primary workflow has no automatic crop search, source reuse, repeat, tiling, or synthesis.

## Rules for Prompts 2–6

- TrimSheetDocument is the sole persisted authoring authority.
- compile_persisted is the sole live compiler/orchestration spine. Add no compiler, renderer, preview orchestrator,
  fallback, legacy bridge, or TypeScript pixel/layout authority.
- Preserve Prompt 1 lineage: SourceFrame → GridRect → resolved source pixels → DirectCrop → destination → Stage 14
  pixels, joined by stable IDs.
- No default region overlaps or reuses another region source rectangle. Only an explicit detached override may overlap.
- RepeatX, RepeatY, PeriodicTile, TextureSynthesis, crop relocation, and full-source fallback are forbidden. Padding
  dilation is not semantic reuse.
- Source, logical-grid, preview, and authoritative output resolutions are independent. Original oriented source
  coordinates remain authoritative.
- Source overlay, atlas overlay, inspector, pixels, IDs, and later export inputs share one compiled artifact lineage.
  Never zip separately ordered arrays.
- Preserve unrelated worktree changes. Read AGENTS.md and the current diff before editing.
- Each prompt requires focused deterministic tests, visual evidence when pixels change, and a native walkthrough.
  Performance prompts also require measured before/after timings and counters.
- Do not declare success from unit tests, hashes, bounds, accessibility, or mocks alone.
- Stop at the assigned boundary and report changes, tests, visuals, timings, and remaining prerequisites.

---

## Prompt 1 — Truthful SourceFrame partition and Base Color

### Complete and locked

Prompt 1 established versioned SourceFrame/LogicalGridSpec/GridRect contracts, deterministic 16/63/103 partitions,
shared pixel boundaries, DirectCrop-only compile_persisted plans, exact non-overlapping coverage, stable ID publication,
and pixel-perfect reconstruction through crates/sheet-compiler/tests/source_frame_e2e.rs.

Regression gate:

~~~text
cargo test -p hot-trimmer-sheet-compiler --test source_frame_e2e
~~~

Do not rerun Prompt 1 or replace its topology, sampling, renderer, or spine. Fix later regressions in the prompt that
caused them.

---

## Prompt 2 — Unify source, patch, and region interaction

~~~text
Implement SourceFrame Product Prompt 2: correct the selection-overlay mismatch and make SourceFrame and region editing
use the existing patch-quality interaction model.

Prerequisite: Prompt 1 pixel/visual evidence is green. Preserve its Stage 14 semantics.

Read AGENTS.md; this pack; Prompt 1 evidence; current diff; patch gesture/draft commands; source overlay in
apps/desktop/src/source-first-app.tsx; Stage 14 slot projections; and relevant Rust/IPC handlers. No subagents.

Fix the observed defects:
- The compiled partition is currently combined with a legacy full-source crop target, causing an opaque/black selection
  block and false 0,0 / 1x1 inspector values.
- Source, atlas, and patch selection/editing feel like separate tools.
- Boundary-limited rotation may jump back to gesture start.

One selection contract:
- One RegionId drives source overlay, atlas overlay, inspector, and preview.
- Clicking either canvas selects the same RegionId and exact source/destination rectangles.
- Partition-owned bounds come from the compiled resolved SourceFrame crop, never legacy projection/default crop.
- Remove/hide/disable the legacy full-source crop target for partition cells. It must not cover/intercept the partition.
- Use transparent selection fill and readable boundaries/handles.
- Show RegionId, GridRect, source pixel/normalized bounds, destination, mapping origin, and artifact revision from one
  compiled record.

Shared patch-quality gizmo:
- Reuse/extract patch interaction primitives; do not create a third drag/resize system.
- SourceFrame supports move, aspect-locked resize, pixel/normalized fields, Center, Fit Width/Height, and Largest Fit.
- Clamp to oriented bounds. Square defaults: 8000x4000 → centered 4000x4000; 750x3600 → centered 750x750.
- Invalid move/resize/rotation holds/clamps at the last valid transform; never reset to gesture start. Escape rolls back.
- Use BeginDraftEdit → bounded preview → CommitDraftEdit. One gesture is one undo entry; stale A/B cannot replace C.

Explicit per-region override:
- Partition cells are selectable but not independently movable. Add Detach Source Cell.
- Detach creates RegionSourceOverride from the exact deterministic crop and uses the shared patch gizmo.
- Only an explicit detached override may overlap; warn with involved RegionIds.
- Reset to Partition removes it and restores the exact SourceFrame + GridRect crop.
- Moving SourceFrame moves every non-overridden crop coherently without crop search.

Carry mapping_origin = partition | explicit_override through document, compile_persisted, IPC, preview, recovery,
undo/redo, and save/reopen. React renders compiled overlays/draft handles but owns no canonical geometry or pixels.

Automated proof:
- Source/atlas selection round-trips one RegionId and exact compiled bounds.
- Partition selection never mounts an opaque full-frame target or false 0,0 / 1x1 crop.
- Moving an 8000x4000 frame by (+500,0) moves all default crops by the shared-boundary delta without changing GridRects.
- Detach changes only one mapping/appearance identity; Reset restores exact pixels.
- Forced A → B → C responses show C only.
- Boundary collision preserves last valid rotation.

Verification:
cargo test -p hot-trimmer-sheet-compiler source_frame_authoring
npm run test --workspace @hot-trimmer/desktop -- source-frame-authoring

Native/visual acceptance:
Select small/medium/large cells from both canvases; show exact matching highlights and no black block. Move/resize the
frame, collide rotation with an edge, detach/move/reset one cell, undo/redo, and save/reopen.

Stop if UI arrays rebuild identity, partition cells move without Detach, or unchanged geometry changes Prompt 1 pixels.
Do not implement layout generation, radial mapping, padding, performance work, or material channels.
~~~

---

## Prompt 3 — Interactive Base Color preview and large-source hot path

~~~text
Implement SourceFrame Product Prompt 3: make the truthful DirectCrop path interactive before richer layout tools.
Extend compile_persisted; add no preview-only compiler or renderer.

Prerequisite: Prompts 1–2 are visually green.

Read AGENTS.md; runtime-pipeline-audit performance findings; compile_persisted/build_domain; decode/preparation; Tauri
publication; caches; Prompt 1–2 evidence; and current diff. Instrument first and report before/after.

One profiled spine:
- Add 512 Draft Base Color, 1024 Refinement, and user-selected Authoritative profiles to the existing request/artifact.
  Profiles change requested resolution/work only, never topology, coordinates, sampling, IDs, or ownership.
- For explicit SourceFrame + DirectCrop Base Color, do not run/wait for Stages 3–8 analysis when they cannot affect
  plans/pixels. Report not-required/bypassed truthfully and preserve those stages for other explicit workflows.
- Keep required registration/orientation/decode/preparation inside compile_persisted.
- Generate Base Color only; allocate no dormant maps/effects.

Caching:
- Decode/orient once per digest + orientation + decoder version.
- Cache bounded proxies/tiles by digest, frame bounds, resolution, filter, and version.
- Cache accepted topology/direct plans by exact upstream identities.
- Cache regions/atlas by source revision, mapping, destination, requested map, profile, and renderer version, including
  later radial/padding identities.
- Frame moves invalidate required pixels, not source decode/topology. Selection/overlay changes do not compile.
- No mutable-global or path-only key.

Cancellation/publication:
- Coalesce drags to a bounded rate; cancel superseded decode/render/encode/publication.
- Publish 512 first, refine to 1024 only while inputs match.
- Remove post-compile decode and independently ordered metadata reconstruction.
- Avoid PNG + base64 JSON when an existing revisioned binary/asset handle is available. Keep PNG only for explicit
  files/goldens or measured fallback. Add no renderer.

Telemetry:
Report source/oriented/proxy/output dimensions, regions, requested maps, decode/full-frame allocation counts, cache
hits/misses, timings, cancellations, encode/IPC bytes/time, browser upload, and first visible paint.

Automated proof:
- Prompt 1 pixels stay byte-identical at matched resolution.
- One cold decode and zero warm decodes per unchanged digest/orientation.
- Thirty drag events yield bounded compiles and final-state publication.
- Selection/overlay changes cause zero compile/decode.
- Draft/refinement retain coordinates/IDs and match at equal profile.
- Base Color preview publishes no dormant maps.

Targets:
- Cold first-visible 512 <= 3 s; target <= 2 s.
- Warm input-to-visible <= 500 ms; target <= 250 ms.
- Cached selection/overlay <= 50 ms with no compile.
- 1024 refinement <= 5 s.
- Report authoritative time separately; never hide proxy substitution.

Verification:
cargo test -p hot-trimmer-sheet-compiler source_frame_preview_profile
npm run test --workspace @hot-trimmer/desktop -- source-frame-preview-performance

Native/visual acceptance:
Capture before/after traces for one 8K-class 63-region document. Show immediate 512 then truthful refinement, warm edits
within target, zero-compile selection, and telemetry proving decode/cache/cancellation counts.

Stop if the fast path changes coordinates/ownership/pixels, adds a compiler/renderer, or makes a proxy authoritative.
Do not implement layout controls, radial mapping, padding, or material maps.
~~~

---

## Prompt 4 — Intentional trim-sheet layout authoring

~~~text
Implement SourceFrame Product Prompt 4: replace proof-quality target-count controls with a user-directed
panel/strip/detail composition while preserving complete coverage and unique source ownership.

Prerequisite: Prompts 1–3 are green and warm 512 editing meets target.

Read AGENTS.md; existing recursive weighted split grammar/shared-boundary machinery; SourceFrame/GridRect contracts;
Prompt 1–3 evidence; and current diff. Extend the existing generator; add no second topology system.

Product behavior:
- Logical grid is a coordinate lattice, not region count. Default 64x64 is editable and independent of pixels.
- Any feasible bounded target count is legal; 53 has no special meaning.
- Layouts combine controllable broad panels, medium blocks, horizontal bands, vertical strips, radial reservations,
  small details, and micro strips.
- Every result fills the frame exactly with shared boundaries, no holes/overlap/reuse.

Versioned composition recipe:
- Persist grid size, target count, seed, split bias, variance, minimum dimensions, aspect bounds, work/depth limit, and
  composition profile.
- Expose count or area share plus bounded sizes/aspects for broad panels; medium blocks; horizontal strips with min/max
  thickness; vertical strips with min/max thickness; small details; micro strips; radial reservations with count and
  allocation/diameter range.
- Infeasible quotas produce deterministic typed diagnostics and suggested corrections. Never silently omit requested
  families, return fewer regions, overlap, or leave space empty.
- Fill remainder deterministically with eligible families while honoring accepted count/constraints.
- Same recipe + seed + version gives byte-identical tree, GridRects, stable IDs/order, and topology hash.

Candidate and editing:
- Layout panel: grid, count, seed, composition controls, Generate/Regenerate, Preview, Discard, Accept.
- Candidate uses compile_persisted 512 and cannot overwrite accepted state. Accept is one undoable command pinning
  recipe/version/tree/rectangles/IDs. Save/reopen never regenerates.
- Category overlays never alter DirectCrop.
- Reuse Prompt 2 gizmos for Split Horizontal/Vertical, Merge Sibling, and shared-boundary drag/numeric edit.
- Move shared boundaries, never isolated edges. Update neighbors atomically with snapping, constraints, cancellation,
  undo/redo, and typed diagnostics. Preserve IDs for boundary-only edits; document split/merge identity rules.

Automated proof:
- Grids 32x32, 64x64, 128x64; targets 16, 32, 63, 64, 100, 103; square/rectangular sources.
- Every feasible result covers exactly, has no overlap, satisfies accepted family constraints, reconstructs SourceFrame,
  and is deterministic.
- Prove strip thickness, broad-panel allocation, radial reservation count, and deterministic remainder fill.
- Prove candidate discard/accept/undo/redo/save/reopen and adjacent-only boundary updates.

Verification:
cargo test -p hot-trimmer-geometry intentional_source_partition
npm run test --workspace @hot-trimmer/desktop -- intentional-source-partition

Native/visual acceptance:
Produce panel-heavy, strip-heavy, and balanced/radial-reserved layouts at 16, 64, and 103. Adjust panel counts, strip
counts/thickness, detail density, variance, and radial reservations; preview/discard/accept; split, drag shared boundary,
merge, undo/redo, save/reopen.

Stop if roles enable repeat/search, ownership changes, holes/overlap appear, or candidates bypass compile_persisted.
Do not implement radial pixel transforms, padding, or material channels.
~~~

---

## Prompt 5 — Radial regions without source reuse

~~~text
Implement SourceFrame Product Prompt 5: turn reserved/selected cells into authorable discs/annuli while retaining one
unique source footprint and compile_persisted.

Prerequisite: Prompts 1–4 are green. A radial region starts as an existing unique GridRect.

Read AGENTS.md; existing RadialParameters/RegionMapping/Stage 14 code; document/IPC/UI contracts; Prompt 1–4 invariants;
and current diff. Reuse correct existing primitives; add no renderer.

Ownership/mapping:
- Appearance modes: Planar, Radial Disc, Annulus. Appearance never changes allocation ownership.
- Retain rectangular GridRect allocation and SourceFrame-derived unique source rectangle.
- Author center, outer/inner radius, seam angle, orientation, and Planar Mask versus One-Shot Polar.
- Planar Mask directly samples the owner rectangle then masks it.
- One-Shot Polar traverses the owner rectangle once around angle. It is not repeat/tile/synthesis and never reads
  outside the crop.
- Allocation resizing uses Prompt 4 shared boundaries; center/radius/seam edits change appearance only.

Interaction:
- Convert selected/reserved cells and reuse Prompt 2 gizmos for center, radii, seam.
- Add numeric controls, keyboard nudge, reset, drafts, last-valid clamping, undo/redo, save/reopen.
- Overlays distinguish allocation, source rectangle, hotspot, center, radii, seam.
- Invalid settings preserve last valid artifact and emit diagnostics; no NaN/silent fallback.

Automated proof:
- Conversion leaves crop and neighboring ownership unchanged.
- Center/radius/seam changes only that region appearance identity/pixels.
- Known-coordinate Planar/Polar fixtures remain inside exact crop with no repeat/tile/synthesis/fallback.
- Undo/redo/save/reopen preserve parameters/pixels.

Verification:
cargo test -p hot-trimmer-sheet-compiler unique_radial_partition
npm run test --workspace @hot-trimmer/desktop -- unique-radial-partition

Native/visual acceptance:
Show one cell as planar, disc, annulus, and one-shot polar. Create requested radial reservations, edit center/radii/seam,
resize allocations through shared boundaries, undo/redo, save/reopen, and prove neighboring ownership never moves.

Stop if radial mode searches, reads outside its crop, repeats, or mutates neighbors.
Do not add padding, material channels, effects, or another renderer.
~~~

---

## Prompt 6 — Padding, arbitrary large sources, and product qualification

~~~text
Implement SourceFrame Product Prompt 6: add owning-edge padding, then qualify the complete Base Color workflow through
24K without changing proven mapping semantics.

Prerequisite: Prompts 1–5 are green and Prompt 3 targets hold.

Read AGENTS.md; padding/bleed validation and pure dilation helpers; Prompt 1–5 evidence; decode/proxy telemetry;
compile_persisted; and current diff. Do not reactivate an old compositor as orchestrator. No subagents.

Terms:
- Allocation: complete GridRect destination.
- Content/hotspot: allocation inset by padding/gutter.
- Padding/bleed: inside owning allocation, outside content, filled from nearest valid owning edge.
- True gutter/background: optional explicit empty space; default zero.

Padding/occupancy:
- Add global/per-region padding, dilation, and optional true gutter with explicit logical/pixel resolution semantics.
- Compute allocation first, then content. Preserve aspect and sample the complete owner crop without nonuniform stretch.
- Fill rectangular padding from nearest owner edge/corner; radial padding from nearest valid owner radial boundary.
  Never scale interiors or borrow neighbor content.
- Classify every atlas pixel as content, owner padding, explicit gutter, or outside. Publish exact counts/debug overlay.
  Default output has no unexplained black/unclassified pixels.
- Add compact global controls and per-region override/reset.

Large-source qualification:
- Preserve original encoded source, oriented dimensions, and full-resolution coordinate identity.
- Preview may use Prompt 3 proxies/tiles; authoritative output samples original detail. Proxy is never silent final truth.
- Qualify 750x3600, 8000x4000, and a source with at least one 24,000-pixel edge.
- Qualify grids 32x32/64x64/128x64; counts 16/64/103; moved frame; explicit override; panel/strip/detail layouts; radial
  cells; and padding.
- Finish bounded tile/pyramid/memory work required to retain Prompt 3 targets without adding another path.

Automated proof:
- Zero padding is byte-identical to Prompt 1 at matched resolution.
- Each padding pixel equals nearest owning edge/corner and never crosses RegionId.
- Rectangular, narrow, tiny, disc, and annulus cases pass at 512/1024/4096.
- All cases have complete classification, no default overlap/duplicates, no repeat/tile/synthesis, stable IDs,
  deterministic save/reopen, and exact original-coordinate proof.
- Authoritative detail is not an upscaled proxy.

Performance acceptance:
- Cold first-visible 512 <= 3 s; target <= 2 s.
- Warm frame/boundary/radial/padding <= 500 ms; target <= 250 ms.
- Cached selection/overlay <= 50 ms with no compile.
- 1024 refinement <= 5 s.
- One cold decode per digest/orientation; zero warm decodes.
- Report authoritative time and peak memory by output resolution.

Verification:
cargo test -p hot-trimmer-sheet-compiler source_frame_product_qualification
npm run test --workspace @hot-trimmer/desktop -- source-frame-product

Native/visual acceptance:
Capture all source aspects, 16/64/103 layouts, panel/strip-heavy compositions, moved frame, detached override, radial
cell, and zero/small/large padding. Include original-detail comparison and occupancy overlay proving no seams/black
pixels. Exercise the complete flow, timings, undo/redo, save/reopen, and telemetry.

Final acceptance:
A user can open arbitrary-aspect 8K/16K/24K input, position an atlas frame, create an intentional trim-sheet hierarchy,
edit shared divisions with patch-quality controls, detach one cell explicitly, author radial cells, add edge-dilated
spacing, and receive a responsive deterministic Base Color trim sheet with zero automatic reuse.

Stop if padding stretches/crosses ownership, large-source work changes pixels/boundaries, a proxy becomes authoritative,
or native visual qualification is unconvincing.

Do not add Height, Normal, Roughness, AO, effects, export, or Blender until Base Color is reviewed green.
~~~

## Review points

~~~text
Prompt 1 — GREEN: one source is divided once into unique DirectCrop regions.
Prompt 2 — selection is truthful and SourceFrame/region editing matches patch UX.
Prompt 3 — editing is interactive on an 8K-class source through one compiler.
Prompt 4 — layout hierarchy, counts, sizes, and shared boundaries are user-controlled.
Prompt 5 — radial regions work inside their unique owning cells.
Prompt 6 — padding has no black seams and Base Color is qualified through 24K.
~~~

Do not begin material channels or effects until all six review points are green.
