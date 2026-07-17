# OBSOLETE — do not run this pack

This pack assumed a fixed-template, independently selected crop workflow that the product owner explicitly rejected. Use `docs/source-frame-partition-prompt-pack.md` instead. It defines one movable source frame, a dynamic recursive logical partition, unique non-overlapping direct source mappings, explicit-only overrides, radial authoring, edge-dilated padding, and large-source qualification.

# Hot Trimmer runtime integration gate prompt pack

## Purpose

This pack converts the runtime audit and integration milestones into six vertical implementation prompts. Run one prompt per Codex task, in order. Review the visible result and evidence before starting the next prompt.

The governing runtime documents are:

- `docs/runtime-pipeline-audit.md`
- `docs/runtime-pipeline-graph.md`
- `docs/runtime-integration-plan.md`
- `docs/runtime-stage-matrix.json`
- `docs/trimsheet-redesign-codex-prompt-pack.md`

These prompts override the old prompt-number sequence only where the runtime audit proved an earlier acceptance gate is still failing. They do not replace the product design.

## Rules for all six prompts

- Keep `compile_persisted` as the sole live orchestration spine. Extend or correct it; do not add a parallel compiler, fallback renderer, preview renderer, or hardcoded replacement pipeline.
- `TrimSheetDocument` remains the only authoring authority. React owns selection and bounded transient interaction state, not compiled layout or pixel authority.
- Preview pixels, overlays, IDs, later channels, export inputs, and diagnostics must descend from the same compiled artifact lineage.
- Work vertically from document/command through Rust compilation, Stage 14/final composition, IPC, and visible UI. A backend-only result does not satisfy a UI-bearing gate.
- Preserve unrelated worktree changes. Read `AGENTS.md` and inspect the current diff before editing.
- Do not add profiles, channels, effects, synthesis, export, Blender work, or broad UI redesign before the prompt that explicitly authorizes them.
- Add focused deterministic tests. Complete the required running-app walkthrough. Do not call a gate green from typechecks, hashes, IDs, or bounds alone.
- Record before/after timings for prompts that touch performance. Do not hide a regression by weakening quality, reducing the acceptance fixture, or bypassing an authoritative stage.
- Stop at the assigned boundary and report exact files changed, deleted/replaced paths, tests, walkthrough observations, timings, and remaining disabled prerequisites.

---

## Prompt 1 — Gate 1: truthful Base Color through the fixed template

```text
Implement Runtime Integration Prompt 1: make the existing fixed-template Base Color path truthful from source crop
to visible preview. This completes the failed Prompt 1 compiler acceptance gate. It may remain slow; correctness is
the only goal of this task.

Read AGENTS.md; docs/runtime-pipeline-audit.md sections 1-4 and 7; docs/runtime-pipeline-graph.md;
docs/runtime-integration-plan.md items A1-A4, A7, C1, D1, D4 and Milestones 1-2; and the current git diff.
Preserve unrelated changes. Work in the root task without subagents.

Non-negotiable architecture:
- Keep compile_persisted as the only live orchestration spine.
- Do not reactivate document_compiler as an orchestrator, add another renderer, restore a legacy layout path, or
  fabricate preview pixels in TypeScript/native glue.
- Keep TrimSheetDocument authoritative. The preview must display the artifact compiled from that document plus an
  explicitly bounded transient crop revision.

Scope — correct source and destination mapping:
- Separate template destination/allocation mapping from authored source-image crop. A template atlas rectangle must
  never become a raw source crop merely because both are normalized rectangles.
- Represent an unplaced/inherited source crop explicitly. Preserve and migrate genuinely user-authored crops without
  reinterpreting existing destination coordinates as source coordinates.
- Declare and enforce normalized source, prepared-source pixel, slot-local, hotspot, allocation, and atlas-pixel
  coordinate spaces at their boundaries.
- Preserve source aspect ratio. Fill regions using an intentional supported crop/cover/repeat/slice/radial policy;
  never nonuniformly stretch unless a future explicit stretch command authorizes it.

Scope — make the selected plan executable and truthful:
- Define the legal MaterialDomain route × SamplingMode table used by Stage 10/11 and Stage 14.
- Do not offer or select a route/mode pair that Stage 14 cannot execute exactly.
- Disable TextureSynthesis candidates for this gate. Do not report synthesis while performing centered direct-source
  sampling. DirectCrop, UniqueCover/Contain, RepeatX/Y, PeriodicTile, slice, and radial modes may remain enabled only
  when their existing Stage 14 branch passes coordinate fixtures.
- Make Stage 14 consume the selected candidate crop, transform, orientation, scale, address mode, and sampling plan
  exactly. Requested mode and executed mode must be identical or compilation must fail with a diagnostic.
- Apply one source transform to every registered channel internally, even though this prompt requests/displays only
  Base Color.

Scope — identity and transient crop publication:
- Carry stable region ID, slot key, source ID/digest, prepared-domain ID, candidate ID, sampling-plan ID, rendered-slot
  ID, and atlas destination through the compiled artifact.
- Join native/UI projections by IDs. Remove order-coupled zipping of separately resolved region and artifact arrays.
- Pass region ID, transient crop projection, draft ID, document revision, and input hash into native compilation.
- Compile the transient crop without first persisting it; commit remains one canonical document command on gesture
  completion. Reject stale results and never redisplay an older crop after a newer draft.

Required automated evidence:
- Add a deterministic quadrant/numbered-grid source fixture large enough to expose source coordinates.
- Prove one slot end-to-end first, then prove the complete Generic Architecture template.
- For all 53 slots, assert stable unique identity, legal non-overlapping atlas destinations, the exact selected source
  transform, aspect-preserving scale, and deterministic output.
- Assert at least representative strip, unique-detail, cap, and radial slots sample known expected source pixels.
- Assert intentional repeats wrap at their declared seam and no unsupported synthesis candidate reaches Stage 13/14.
- Assert shuffled region iteration cannot change artifact/region identity projection.
- Add a desktop interaction test showing crop draft A→B→C publishes C pixels and never returns to A or B.

Automated verification:
cargo test -p hot-trimmer-sheet-compiler truthful_base_color
npm run test --workspace @hot-trimmer/desktop -- truthful-base-color

Required visual fixture:
- Save a Base Color golden for the complete 53-slot fixed template using the numbered-grid source. The golden must make
  crop origin, orientation, repeat, padding, and accidental duplication visually obvious.

Required in-app walkthrough:
launch the existing workbench -> import/open a visibly nonuniform real Base Color -> build the fixed template ->
inspect representative strip, panel/detail, cap, and radial slots -> confirm each uses an intentional crop/repeat and
none is nonuniformly stretched -> drag the selected slot crop through three visibly different areas -> confirm the
preview follows the drag and the last draft wins -> commit -> undo/redo -> save/reopen -> confirm identical pixels,
IDs, overlays, and crop.

Acceptance:
- The fixed template shows 53 correctly identified, intentionally sampled slots.
- Different-purpose slots no longer collapse to centered fake-synthesis bands.
- Repetition is intentional and inspectable; duplicate-looking content always has distinct identity and an explicit
  reuse/repeat explanation.
- Crop dragging visibly changes only the selected region and survives commit/save/reopen.
- Base Color pixels and overlays share one revision/topology/appearance lineage.

Stop conditions:
- Stop if any template destination rectangle is still interpreted as a raw source crop.
- Stop if an unsupported route/mode reaches Stage 14, or requested and executed modes differ.
- Stop if the UI result is assembled by array order instead of stable IDs.
- Stop if the numbered-grid test passes but the running-app real-texture walkthrough is not convincing.
- Do not optimize broad performance, add new channels, implement synthesis, or add layout authoring in this prompt.
```

---

## Prompt 2 — Gate 2: interactive preview and full-resolution source fidelity

```text
Implement Runtime Integration Prompt 2: make the truthful Base Color compiler interactive while preserving real
8K/16K source fidelity. Use compile profiles and caches inside compile_persisted; do not create a preview pipeline.

Prerequisite: Runtime Integration Prompt 1 is reviewed green in automated tests and the required running-app
walkthrough. If it is not green, stop and report the failing Gate 1 evidence.

Read AGENTS.md; docs/runtime-pipeline-audit.md section 6; docs/runtime-pipeline-graph.md;
docs/runtime-integration-plan.md items B2-B5, D2, D4-D5, E1-E6 and Milestone 8; Prompt 1's implementation evidence;
and the current git diff. Preserve unrelated changes. Work in the root task without subagents.

Non-negotiable architecture:
- Keep the exact Gate 1 sampling semantics and IDs. A faster wrong result is a regression.
- Keep one compile_persisted entry and one artifact contract. Draft/preview/refinement are request profiles on that
  compiler, not separate algorithms or renderers.
- Preserve the original encoded source, oriented full-resolution dimensions, and full-resolution source coordinate
  system. Do not silently make a 2048-pixel proxy authoritative.

Scope — source resolution and cached preparation:
- Support representative 8192-pixel and 16384-pixel-edge source images within declared memory limits.
- Decode each source once per digest/orientation and cache Stage 2 prepared channels by digest, orientation,
  normalization settings, algorithm version, and compile profile.
- Use a bounded proxy/pyramid for Stage 3-8 analysis and 512 preview work, while retaining an exact mapping back to
  original source pixels.
- Authoritative 1024 refinement and future export sampling must use the required original-source detail rather than
  upscaling a 2048 proxy. Use existing tiled/pyramid primitives where available; do not lift the 2048 cap and run all
  full-frame feature extraction at 16K.
- Reuse full-source preparation/analysis for patch-derived domains when inputs and coordinate transforms make reuse
  legal. Cache patch-dependent work separately and invalidate only the affected patch revision.
- Replace the per-domain empty MaterialDomainCache with a compile-session/session-owned cache keyed by exact inputs.

Scope — placement/render/publication caching:
- Cache Stage 11 candidate generation/scoring and Stage 13 placement by exact upstream identities, settings, template
  topology, source mapping, and algorithm versions.
- Reuse feature integrals/measurements across slots sharing a domain/window when mathematically equivalent.
- Do not rerun Stage 13 when only the displayed map changes or an unchanged artifact is republished.
- Add a compile request profile containing output size, requested maps, draft/final quality, and refinement intent.
- The 512 preview profile requests Base Color only. Do not allocate, generate, encode, or transfer unused maps.
- Coalesce animation-frame crop drafts, cancel superseded native work through every hot stage, and publish only the
  newest matching revision/input hash.
- Remove post-compile source decoding and region re-resolution. Project UI metadata directly from the compiled
  artifact by ID.
- Remove PNG+base64 JSON from the normal hot path. Use an existing safe Tauri binary/asset-handle mechanism with
  revisioned lifetime; retain PNG only for explicit save/export or a measured compatibility fallback. Do not add a
  renderer.

Scope — telemetry and budgets:
- Add per-stage timing, decode count, full-frame allocation count, retained bytes/peak memory when available,
  cache hit/miss, cancellation, IPC bytes/time, browser decode/upload, and first-visible-paint marks.
- Keep telemetry diagnostic-only and bounded. It must not change artifact identity or ship source pixels in logs.
- Add a repeatable performance fixture for the same 53-slot template with 8K and 16K synthetic/nonuniform sources.

Automated verification:
cargo test -p hot-trimmer-sheet-compiler interactive_preview_profile
npm run test --workspace @hot-trimmer/desktop -- interactive-preview

Required performance evidence on the audit machine:
- Cold 512 Base Color first visible preview: <= 3.0 seconds, target <= 2.0 seconds.
- Warm crop/input-to-visible preview: <= 500 ms, target <= 250 ms.
- Cached map/artifact view switch: <= 50 ms and no compiler invocation.
- 1024 authoritative refinement: <= 5.0 seconds.
- One cold source decode per digest/orientation; zero warm decodes.
- A crop edit invalidates only the affected mapping/patch-dependent work.
- The 8K/16K fixture proves final sample coordinates address original-resolution pixels.

Required in-app walkthrough:
open an 8K or 16K visibly detailed source -> observe cold 512 preview -> drag one crop continuously for at least five
seconds -> confirm responsive newest-draft updates with no rollback -> release and observe 1024 refinement -> switch
views already present in the artifact -> repeat the same edit to demonstrate warm cache behavior -> inspect telemetry
and confirm no post-compile decode or full Stage 13 rerun for a display-only action.

Stop conditions:
- Stop if speed comes from reduced source truth, changed sampling, skipped required stages, or a second preview path.
- Stop if a 16K source is treated as an upscaled 2048 source for authoritative sampling.
- Stop if cache keys omit source mapping, orientation, algorithm version, topology, or compile profile.
- Stop if canceled/stale work can publish after the newest draft.
- Do not add layout generation, structural maps, effects, or synthesis in this prompt.
```

---

## Prompt 3 — Base Color product: layout, padding, occupancy, and radial authoring

```text
Implement Runtime Integration Prompt 3: make the truthful, interactive Base Color trim sheet authorable as a layout.
Deliver only the minimum layout/radial product slice needed to control atlas use; do not add material channels,
profiles, decorations, effects, export, or a new renderer.

Prerequisite: Runtime Integration Prompts 1 and 2 are reviewed green, including the real-texture visual walkthrough
and performance budgets. If either is not green, stop and report the failing gate.

Read AGENTS.md; docs/trimsheet-redesign-codex-prompt-pack.md Prompt 4, Prompt 5, and Prompt 8 only for the relevant
layout/radial/custom-topology requirements; docs/runtime-integration-plan.md Milestones 1-2 and 8; the compiled
artifact contract from Prompts 1-2; and the current git diff. Preserve unrelated changes. No subagents.

Non-negotiable architecture:
- Keep TrimSheetDocument authoritative and compile every accepted/candidate topology through compile_persisted.
- Do not restore StoredLayout, CSS atlas rendering, a TypeScript layout reducer, or a second candidate renderer.
- Candidate and accepted layouts use the same truthful Base Color sampling/rendering contract.
- Preserve Gate 1's no-unintentional-stretch invariant.

Scope — atlas usage controls:
- Add versioned document controls for atlas margin, inter-slot gutter/padding, Base Color bleed/dilation, target
  occupancy, orientation mix, slot-family population, minimum slot size, and aspect ranges.
- Distinguish allocation, hotspot, gutter, bleed, and genuinely unused atlas pixels. Report occupancy numerically and
  visualize each category; never disguise empty/unassigned space as padding.
- Define the default fill policy: cover/crop or repeat may fill a slot without stretching; contain may leave an
  explicit letterbox only when the user selects it. Padding/bleed must copy/dilate valid edge texels rather than
  stretching the source image.
- Validate non-overlap, minimum clearances, bounds, padding, and renderer work limits before candidate publication.

Scope — generated and editable topology:
- Reuse the planned deterministic LayoutGeneratorRecipe/TopologyCandidate model for a focused Base Color layout
  drawer with seed, population, occupancy, padding, margin, Preview Candidate, Discard, and Accept Layout.
- Ship one balanced preset first. Do not implement the entire preset catalog unless already trivial through the same
  recipe engine.
- Candidate preview must show real current Base Color content and remain visually distinct from the accepted layout.
- Accept is one undoable document topology command; save/reopen pins the accepted snapshot and never reruns generation.
- Add focused Custom Template editing for move, resize, axis-aligned 90-degree rotate, lock, numeric bounds, collision
  repair, and accept/cancel. Standard template topology remains locked until the explicit Customize Layout action.

Scope — radial control:
- Keep radial destination geometry separate from radial source mapping.
- Allow the user to move/resize a radial destination region in Custom Template mode.
- For a selected radial region, expose Base Color mapping center, radius/scale, seam angle, orientation/mirror, and
  repeat/address behavior through the canonical mapping recipe and compile_persisted.
- Do not implement spiral/twirl/lens or tangent-normal Jacobians in this prompt. They are outside the Base Color MVP.
- Changing radial mapping must not change topology; changing radial destination bounds must change topology and issue
  the existing compatibility warning/report.

Required automated evidence:
- Same generator recipe/seed/version produces byte-identical topology, IDs, ordering, bounds, and hash.
- Occupancy math exactly partitions allocated, gutter/margin, and unused pixels.
- Padding and bleed never read/write outside declared bounds and never nonuniformly stretch source content.
- Candidate/accepted artifacts use identical sampling semantics and stable ID lineage.
- Move/resize one radial region; assert topology hash changes while mapping-only edits leave it unchanged.
- Save/reopen and undo/redo preserve generator snapshot, custom bounds, padding, occupancy, and radial mapping.

Automated verification:
cargo test -p hot-trimmer-geometry base_color_layout_authoring
npm run test --workspace @hot-trimmer/desktop -- base-color-layout

Required visual goldens:
- Balanced layout at two padding values with occupancy categories visible.
- Same source with radial destination resized while source mapping remains fixed.
- Same radial destination with center/radius/seam mapping changed while topology remains fixed.

Required in-app walkthrough:
open the layout drawer -> preview three seeds -> change padding/margin/occupancy/population -> compare candidate against
accepted sheet -> discard -> regenerate -> accept -> undo/redo -> Customize Layout -> move/resize one strip and one
radial region -> repair a deliberate collision -> edit radial center/radius/seam -> verify no stretching or unexplained
empty space -> save/reopen -> confirm identical topology, mappings, pixels, occupancy, and selection.

Acceptance:
- A user can produce a convincing Base Color trim sheet from one real 8K/16K texture, control how atlas space is
  distributed, control padding/bleed, and resize/map radial content.
- Every visible atlas pixel is classified as sampled content, declared bleed/padding, declared gutter/margin, or
  reported unused space.
- No operation silently stretches source content or bypasses compile_persisted.
- Gate 2 interactive budgets remain within 25% for the same fixed output profile; larger accepted topology work is
  reported separately rather than hidden.

Stop conditions:
- Stop if layout controls mutate local React geometry instead of a canonical document candidate/command.
- Stop if candidate pixels use different rendering from accepted pixels.
- Stop if “no empty space” is implemented by stretching or by mislabeling unused pixels as padding.
- Stop if radial mapping and radial destination geometry are conflated.
- Do not add Height, Normal, Roughness, AO, Region ID, effects, export, or advanced synthesis here.
```

---

## Prompt 4 — Gate 3: material-complete atlas through the same compiler

```text
Implement Runtime Integration Prompt 4: extend the authoritative compile_persisted artifact after truthful Stage 14
sampling to produce Height, Normal, Roughness, AO, and Region ID alongside unchanged Base Color. Extract reusable pure
functions from the dead compositor; do not reactivate it as a second orchestrator.

Prerequisite: Runtime Integration Prompts 1-3 are reviewed green. Preserve their Base Color pixels, identity,
coordinate spaces, layout behavior, and performance profiles.

Read AGENTS.md; docs/runtime-pipeline-audit.md sections 5 and 7; docs/runtime-integration-plan.md items A5-A6, B1,
C2, C5, D1, D3 and Milestones 3-5; crates/sheet-compiler/src/document_compiler.rs only as reusable implementation
material; the current git diff. Preserve unrelated changes. No subagents.

Non-negotiable architecture:
- compile_persisted remains the sole orchestration spine and produces one CompiledPreview/CompiledSheet lineage.
- Do not enable the old document_compiler command/caller, add a second renderer, or let the preview independently
  generate/modify channels.
- Base Color sampling and layout from Prompts 1-3 must remain byte-identical when inputs are unchanged.

Required authoritative order:
sampled slot material
-> structural profile height in hotspot-local coordinates
-> material + structural height composition
-> generated/composed tangent-space normal
-> roughness
-> AO
-> Region ID
-> final atlas channel composition/publication

Scope — structural and channel composition:
- Reuse/extract the existing structural profile height/normal primitives and applicable pure channel-composition,
  cavity, padding, bleed, and dilation functions from dead code.
- Compile structural masks using hotspot-local coordinates and physical/pixel scale. Source material may fill allocation;
  profiles and generated appearance must obey hotspot masks and declared padding policy.
- Define height range/units, imported material-height contribution, structural-height combination, conversion precision,
  and diagnostics. Prevent range collapse at 1K/2K/4K/8K.
- Generate structural normal from the composed height or compose normals using a documented correct method. Never
  average Normal RGB and never let a later imported-normal copy overwrite structural normal.
- Declare OpenGL/DirectX orientation and transform imported tangent normals consistently through mapping/reflection.
- Use imported Roughness/AO when available through the exact Gate 1 transform; otherwise apply explicit deterministic
  defaults/estimated labels. Metallic remains explicit and is not inferred from Base Color.
- Generate a lossless Region ID atlas plane plus lookup table from compiled topology/identity. Do not encode it as a
  guessed material role or reconstruct it in React.
- Publish requested channels, diagnostics, correspondence/validity as needed, and unavailable-channel reasons. One
  missing slot channel must not silently remove the complete atlas channel.

Scope — preview consumption:
- Extend the same artifact contract and existing 2D workpiece to switch among Base Color, Height, Normal, Roughness,
  AO, and Region ID without recompiling when those maps are already present.
- Do not add a new 3D renderer in this prompt. Existing map inspection is sufficient for acceptance.
- Requested-map compile profiles remain active so Base Color-only interaction is not forced to generate every map.

Required automated evidence:
- Rectangular, strip, cap, and radial fixtures have expected hotspot masks, edge-distance height, composed height,
  normal direction, roughness/AO values, Region IDs, allocation padding, and background behavior.
- Cross-channel coordinate fixtures prove imported Base Color/Height/Normal/Roughness/AO share the exact sampling
  transform.
- Numeric tests cover OpenGL/DirectX Y orientation, reflection, height range, no Normal RGB averaging, missing-channel
  fallback, and no writes outside declared bounds.
- Every hotspot center decodes to exactly one stable region/slot ID; uncovered/padding pixels follow the declared ID
  policy.
- Base Color regression golden from Prompt 3 remains byte-identical.

Automated verification:
cargo test -p hot-trimmer-sheet-compiler material_complete_atlas
npm run test --workspace @hot-trimmer/desktop -- material-complete-atlas

Required visual goldens:
- Base Color, Height, Normal, Roughness, AO, and false-color Region ID for the same accepted layout.
- Zoomed rectangular and radial hotspots showing profile/padding boundaries aligned with overlays.

Required in-app walkthrough:
open the reviewed Base Color layout -> request all six channels -> switch maps without recompilation -> select strip,
panel/detail, cap, and radial regions -> confirm profile/channel boundaries and Region ID selection align -> change one
profile parameter -> confirm only appearance hash/channels change, not topology or Base Color source mapping -> switch
resolution -> save/reopen -> confirm identical identities and deterministic pixels.

Stop conditions:
- Stop if any channel is generated by a preview-only path or the old compositor becomes a live orchestrator.
- Stop if structural maps use allocation bounds where hotspot bounds are required.
- Stop if generated normals are overwritten, height collapses, or Region ID is reconstructed by array order.
- Stop if adding channels regresses Prompt 1-3 Base Color truth or interactive Base Color profile.
- Do not implement weathering, chips, decorations, export, Blender, or advanced synthesis in this prompt.
```

---

## Prompt 5 — Gate 4: one deterministic chipped-edge effect

```text
Implement Runtime Integration Prompt 5: add exactly one real scale-aware chipped-edge treatment through the
authoritative compiler. Prove the effect route end-to-end before adding any other decoration/weathering family.

Prerequisite: Runtime Integration Prompt 4 is reviewed green with correct Base Color, Height, Normal, Roughness, AO,
and Region ID channels.

Read AGENTS.md; docs/runtime-pipeline-audit.md section 5; docs/runtime-integration-plan.md items A3, C3-C5 and
Milestone 6; existing EffectPlanHeader and structural edge/profile implementations; the current git diff. Preserve
unrelated changes. No subagents.

Non-negotiable architecture:
- Implement the effect as an immutable, versioned plan compiled and executed inside compile_persisted after structural
  composition at the declared stage. Do not mutate source pixels or add a preview-only effect.
- Implement one effect only: deterministic chipped edge. Do not add a generic layer framework whose visible effect is
  deferred, and do not implement grunge, dirt, decals, scratches, multiple presets, or all weathering families.

Effect contract:
- Stable effect-plan ID/hash derived from effect algorithm/version, region/group target, parameters, deterministic
  seed, hotspot identity/geometry, scale, and required upstream artifact identities.
- Hotspot-local mask derived from structural distance-to-edge and restricted to selected edges/targets.
- Width/depth expressed in physical units with a documented pixel clamp so the effect is visible and stable across
  512/1024/4096/8192 resolutions.
- Deterministic seed and bounded work/memory. Same inputs produce byte-identical plan and pixels.
- Base Color contribution: bounded exposed/lighter/darker chip color modulation.
- Height contribution: bounded negative chip depth composed with material + structural height.
- Normal contribution: regenerated/composed from affected height using Prompt 4's convention.
- Roughness contribution: bounded deterministic change inside chipped pixels.
- AO may respond through existing composed-height/cavity policy, but no independent AO effect family is required.
- No writes outside the hotspot. Declared padding/bleed may be regenerated from the final hotspot result only after
  effect composition.

Scope — minimal authoring and diagnostics:
- Add one approachable chipped-edge enable/amount/size/seed control at the existing appropriate document/group/region
  scope. Every enabled control must invoke a typed Rust command and visibly change compiled pixels.
- Publish effect-plan ID, affected region IDs, scale, seed, bounds, channel contributions, timing, and unavailable/error
  diagnostics in the compiled artifact.
- Cache the effect by exact plan/upstream identities. Changing the effect must not rerun source decode, Stage 2-8,
  candidate generation, or placement.

Required automated evidence:
- Same plan inputs/seed produce identical effect-plan ID, mask, and pixels after save/reopen.
- Different seed changes chip distribution while preserving bounds and scale.
- Physical width is proportionate at 512/1024/4096/8192 within declared tolerances.
- Base Color, Height, Normal, and Roughness change only where the effect mask permits; all pixels outside the hotspot
  remain unchanged.
- Effect edits preserve topology, source mapping, sampling-plan IDs, placement-plan ID, and Region IDs.
- Warm effect edit meets the declared incremental timing and cache-hit expectations.

Automated verification:
cargo test -p hot-trimmer-sheet-compiler deterministic_chipped_edge
npm run test --workspace @hot-trimmer/desktop -- deterministic-chipped-edge

Required visual goldens:
- Clean versus chipped Base Color/Height/Normal/Roughness crops for one rectangular edge and one radial boundary at
  two resolutions.
- A mask/debug golden proving zero writes outside the hotspot.

Required in-app walkthrough:
select a rectangular region -> enable chipped edge -> vary amount/size/seed -> isolate Base Color, Height, Normal, and
Roughness -> verify aligned contribution -> select radial region -> verify scale/edge behavior -> disable/undo/redo ->
save/reopen -> confirm identical plan ID and pixels -> inspect telemetry to confirm upstream caches were retained.

Stop conditions:
- Stop if the implementation grows into multiple effects or a speculative general framework without the required
  visible chipped-edge result.
- Stop if effect identity is unstable, coordinates are allocation/global rather than hotspot-local, or scale changes
  unpredictably with resolution.
- Stop if any channel is overwritten rather than composed or any pixel outside the hotspot changes.
- Do not implement advanced placement/synthesis, export, Blender, or additional effects here.
```

---

## Prompt 6 — Optional advanced placement and real synthesis

```text
Implement Runtime Integration Prompt 6: enable existing advanced placement/synthesis algorithms only for slot/source
cases where reviewed direct crop, cover, repeat, slice, or radial sampling cannot meet the declared demand. Every
enabled mode must execute for real through compile_persisted; no fallback may masquerade as synthesis.

Prerequisite: Runtime Integration Prompts 1-5 are reviewed green. Before editing, run the reviewed real-texture
fixtures and identify at least one concrete slot/source case with an objective failure that direct modes cannot solve
(for example insufficient source extent, unacceptable repeat seam, or required expansion). If no such case exists,
stop and report that advanced synthesis is not currently product-critical rather than enabling it speculatively.

Read AGENTS.md; docs/runtime-pipeline-audit.md sections 2-4 and 6; docs/runtime-integration-plan.md items A2, B2-B3,
C1 and Milestone 7; Stage 8 domain routes, Stage 11 candidate families, Stage 12 scoring, Stage 13 plans, and the actual
existing synthesis implementations; the current git diff. Preserve unrelated changes. No subagents.

Non-negotiable architecture:
- Keep compile_persisted, the artifact contract, and the existing renderer/compositor. Do not add a synthesis preview
  renderer, hardcoded image generator, external service, or parallel orchestration path.
- Preserve all direct-mode outputs byte-for-byte when synthesis is not selected.
- Stage 10/11 may offer a synthesis mode only when the prepared MaterialDomain route contains the exact executable
  synthesis artifact/algorithm required by Stage 14.

Scope — legal route and execution:
- Define exact route/mode/candidate-family/renderer compatibility for quilting, PatchMatch, procedural synthesis, or
  other implementations that actually exist in the repository. Enable only combinations with an end-to-end tested
  executor; leave all others disabled with diagnostics.
- Carry the concrete synthesis artifact identity, parameters, seed, crop/constraint region, scale, and correspondence
  through Candidate -> ScoredCandidate -> SamplingPlan -> Stage 14 result -> atlas diagnostics.
- Replace the old TextureSynthesis generic centered sampler fallback with the actual selected implementation. If the
  required artifact is missing or incompatible, fail before optimization rather than substitute DirectSource.
- Preserve cross-channel correspondence. Synthesis must apply one authoritative coordinate/patch decision to all
  registered channels and transform normals correctly under any actual spatial transform.
- Bound candidate count, search work, memory, output work, and cancellation. Cache synthesis domain/artifact/results
  by exact upstream identities and quality profile.
- Preview may use a bounded quality/refinement setting in the same compiler; authoritative refinement replaces it only
  when document/topology/mapping/effect inputs are unchanged.

Scope — selection quality:
- Add objective eligibility and scoring evidence for why synthesis is preferable to direct/repeat in the forced case:
  required extent, seam cost, invalid coverage, structure preservation, or demand constraint.
- Do not reward synthesis merely because it exists. Direct crop/repeat remains preferred when it satisfies demand at
  equal or better score/work.
- Publish requested mode, executed mode, route, candidate score components, synthesis algorithm/version, seed, cache
  state, and refinement status.

Required automated evidence:
- A forced-synthesis fixture proves output is not the generic centered DirectSource sample and addresses the declared
  failure better than all legal direct candidates under objective metrics.
- Requested mode, executed mode, domain route, candidate family, and algorithm provenance agree exactly.
- Cross-channel alignment, deterministic seed, save/reopen, cache hit, cancellation, memory/work bounds, and stale
  refinement rejection pass.
- Direct-mode regression fixtures from Prompts 1-5 remain byte-identical and retain their performance budgets.

Automated verification:
cargo test -p hot-trimmer-sheet-compiler authoritative_synthesis
npm run test --workspace @hot-trimmer/desktop -- authoritative-synthesis

Required visual goldens:
- Forced case showing best legal direct/repeat result beside authoritative synthesis, with seam/coverage diagnostics.
- Same synthesis result for Base Color and aligned material channels at preview and authoritative resolution.

Required in-app walkthrough:
open the forced-case source/layout -> inspect direct candidate diagnostics -> enable/allow advanced placement -> build
bounded preview -> observe authoritative refinement -> verify synthesis fixes the declared seam/coverage failure rather
than merely changing texture -> inspect requested/executed provenance -> repeat to demonstrate cache -> change crop to
make direct sampling sufficient -> confirm optimizer returns to direct mode -> save/reopen -> confirm deterministic
selection and pixels.

Acceptance:
- There is at least one proven product case where authoritative synthesis improves a declared metric and visible result.
- No slot reports synthesis unless the actual synthesis algorithm produced its pixels.
- Advanced placement does not regress direct Base Color truth, layout authoring, channels, effects, or interactive
  preview outside its bounded refinement work.

Stop conditions:
- Stop if no objective product case requires synthesis.
- Stop if the implementation would require a new renderer/orchestrator or cannot maintain cross-channel identity.
- Stop if a mode is enabled without an actual Stage 14 executor and deterministic fixture.
- Stop if synthesis becomes the default workaround for incorrect crop, repeat, padding, or layout semantics.
```

## Execution summary

```text
Prompt 1  Truthful fixed-template Base Color
    ↓ review visual correctness
Prompt 2  Interactive preview + 8K/16K source fidelity
    ↓ review performance and coordinate truth
Prompt 3  Base Color layout/padding/occupancy/radial product
    ↓ Base Color MVP review
Prompt 4  Material-complete atlas channels
    ↓ channel/identity review
Prompt 5  One deterministic chipped-edge effect
    ↓ effect-route review
Prompt 6  Advanced placement/synthesis only if a proven case requires it
```

The Base Color product requested in the runtime review should be visibly achieved by Prompt 3. Prompts 4-6 increase material completeness and difficult-source coverage; they are not allowed to postpone or redefine the Prompt 1-3 Base Color acceptance.
