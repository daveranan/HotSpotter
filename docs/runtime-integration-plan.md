# Hot Trimmer runtime integration plan

Plan date: 2026-07-17  
Constraint: connect and correct the existing product path; do not add a replacement renderer or a parallel pipeline.  
Evidence: `docs/runtime-pipeline-audit.md`, `docs/runtime-pipeline-graph.md`, and `docs/runtime-stage-matrix.json`.

## Planning premise

The shortest path is to keep `compile_persisted` as the authoritative orchestration spine, make its artifacts truthful and reusable, extend its existing atlas artifact through structural/material composition, and make the UI consume that artifact. The old `document_compiler` compositor is useful implementation material but must not become a second live orchestrator: it is currently private/dead while Stage 14P calls `compile_persisted` (`crates/sheet-compiler/src/document_compiler.rs:200-315`; `crates/sheet-compiler/src/persisted_pipeline.rs:113-247`).

Effort estimates below are engineering days including focused automated tests. They are not elapsed calendar days. Severity is Critical, High, Medium, or Low.

## A. Contract fixes

| ID | Required work | Severity | Effort | Dependencies | Affected files | Verification |
|---|---|---:|---:|---|---|---|
| A1 | Separate a template's destination/legacy atlas mapping from an authored crop on a raw source. Do not initialize every new raw-image region with the template `sourceMapping`; represent “unplaced source” explicitly. | Critical | 2-4 d | Migration/default decision for existing documents | `crates/domain/src/document.rs:927-961,1130-1145`; `crates/sheet-compiler/src/persisted_pipeline.rs:356-425`; document serialization tests | Create a document from one 1929x1033 raw image; assert `cornice_long` is not constrained to the template's 0.781%-wide atlas-origin window, then round-trip the document without changing an explicit user crop. |
| A2 | Make `MaterialDomain.route`, selected `SamplingMode`, and the Stage 14 implementation agree. A plan may not claim `TextureSynthesis` was executed by `DirectSource` when the renderer used its generic centered sampler. Reject unsupported pairs or route them through the already-selected authoritative implementation. | Critical | 2-3 d | A1; supported-mode table | `crates/placement-solver/src/candidates.rs:410-424`; `crates/sheet-compiler/src/slot_synthesis.rs:149-166,296-373`; `crates/effect-compiler/src/stage10.rs:517-528` | Contract test enumerates every offered route/mode pair and proves it either has a Stage 14 branch or fails before optimization; no diagnostic may have requested=executed for a fallback branch. |
| A3 | Carry exact stable identity through the public artifact: region, slot, source, domain, candidate, placement/sampling plan, rendered-slot result, and later effect-plan IDs. Stop reconstructing projection rows by zipping independently ordered vectors. | High | 1-2 d | None | `crates/sheet-compiler/src/intermediate_atlas.rs:19-80,180-187`; `apps/desktop/src-tauri/src/document_commands.rs:1228-1260`; `packages/ipc-contracts/src/document-contracts.ts:279-315` | Shuffle source-region iteration in a test and assert every published slot projection retains the same IDs and destination rectangle. |
| A4 | Declare and enforce allocation, hotspot, source-pixel, normalized-source, and atlas-pixel coordinate spaces at boundaries. Rendering masks/effects must use hotspot geometry while atlas packing still uses allocation bounds. | High | 2-3 d | A1 | `crates/domain/src/document.rs:1130-1145`; `crates/sheet-compiler/src/persisted_pipeline.rs:356-425`; `crates/sheet-compiler/src/slot_synthesis.rs:113-147`; `crates/sheet-compiler/src/intermediate_atlas.rs:163-179` | Nonzero-padding fixture asserts source crop, hotspot mask edge, and allocation copy coordinates independently at 128, 1024, and 4096 output sizes. |
| A5 | Add Region ID as an explicit compiled atlas channel/plane rather than trying to squeeze it into `MaterialChannelRole`. Define lossless ID encoding and a lookup table in the artifact. | High | 1-2 d | A3, A4 | `crates/sheet-compiler/src/intermediate_atlas.rs:19-80,132-142`; `packages/ipc-contracts/src/document-contracts.ts:279-315` | Read pixels at every hotspot center and assert decoded region/slot identity; assert padding and uncovered pixels decode as background. |
| A6 | Extend the artifact contract to distinguish imported material channels, generated structural channels, composed final channels, correspondence/validity, and unavailable channels with reasons. Avoid the current “intersection of all slot channel roles” silently dropping an atlas channel. | High | 2-3 d | A3 | `crates/sheet-compiler/src/intermediate_atlas.rs:100-206`; `packages/ipc-contracts/src/document-contracts.ts:279-315` | Fixture where one slot lacks roughness still publishes a deterministic roughness atlas using the declared fallback and includes a diagnostic for that slot. |
| A7 | Define the transient crop request as an input revision to the compile contract. `regionId` and the draft projection must not be accepted by TypeScript and then discarded before native compilation. | Critical | 1-2 d | A1, A3 | `apps/desktop/src/source-first-app.tsx:702-727,1309-1347`; `apps/desktop/src-tauri/src/document_commands.rs:1001-1013` | Drag one crop edge without committing; the returned artifact's source crop and candidate/plan IDs change for only that region, and a stale response cannot replace it. |

## B. Orchestration fixes

| ID | Required work | Severity | Effort | Dependencies | Affected files | Verification |
|---|---|---:|---:|---|---|---|
| B1 | Keep one top-level persisted compile and extend it after Stage 14: structural maps, material/effect composition, final atlas channels, diagnostics. Extract reusable functions from the dead compositor as needed; do not activate a second command path. | Critical | 3-5 d | A2, A4, A6 | `crates/sheet-compiler/src/persisted_pipeline.rs:113-247`; `crates/sheet-compiler/src/document_compiler.rs:200-315,431-460,670-705`; `crates/sheet-compiler/src/algorithm_compiler.rs:47-109` | One integration test calls the same compiler API used by Tauri and receives all requested maps; no preview command calls stage functions independently. |
| B2 | Replace per-call, per-domain ephemeral caches with compile-session caches keyed by source digest, oriented dimensions, preparation parameters, patch, stage version, and preview resolution. Reuse full-source Stage 2-7 work when deriving a patch. | Critical | 4-6 d | Stable cache-key definition | `crates/sheet-compiler/src/persisted_pipeline.rs:273-340,534-550`; `apps/desktop/src-tauri/src/document_commands.rs:147-166,1599-1622` | Two identical warm previews report cache hits and zero source decodes/Stage 2-7 recomputations; changing one patch invalidates only patch-dependent entries. |
| B3 | Cache Stage 11/12 and the Stage 13 solution by exact upstream identities. Do not rerun the 8.5-second global placement when a display channel changes or the same artifact is re-encoded. | High | 2-4 d | A3, B2 | `crates/sheet-compiler/src/persisted_pipeline.rs:166-222`; `crates/placement-solver/src/optimizer.rs:284-350,805-900` | Toggle Base Color/Normal in the UI and assert identical sampling-plan ID with a cache hit and no optimizer invocation. |
| B4 | Add a compile request profile to the existing orchestrator: output size, requested maps, draft/final quality, and refinement policy. It is an orchestration parameter, not a new algorithm pipeline. | Critical | 2-3 d | A6 | `crates/sheet-compiler/src/intermediate_atlas.rs:60-80`; `crates/sheet-compiler/src/persisted_pipeline.rs:223-246`; `apps/desktop/src-tauri/src/document_commands.rs:1001-1013` | Preview request at 512 produces 512 output and only requested maps; export profile still selects authoritative full resolution and all export maps. |
| B5 | Propagate cancellation and stage diagnostics through preparation, placement, slot rendering, atlas composition, encoding, and publication. Remove redundant polling threads once one revision token reaches every hot loop. | High | 2-3 d | B1 | `apps/desktop/src-tauri/src/document_commands.rs:1193-1261`; `crates/sheet-compiler/src/persisted_pipeline.rs:82-109`; Stage 2-14 cancellation checks | Cancel during Stage 7, Stage 13, Stage 14, and PNG encoding; each terminates within a stated budget and no canceled artifact is published. |
| B6 | Make partial/unavailable stages explicit in the compiled diagnostics instead of reporting Stage 14P success as a finished material. | Medium | 1 d | A6 | `crates/sheet-compiler/src/intermediate_atlas.rs:196-205`; `apps/desktop/src-tauri/src/document_commands.rs:1228-1260` | A Stage 14-only request reports structural/effect/final maps as unavailable with reasons; the UI renders those diagnostics. |

## C. Rendering fixes

| ID | Required work | Severity | Effort | Dependencies | Affected files | Verification |
|---|---|---:|---:|---|---|---|
| C1 | Consume the selected placement exactly. Implemented modes must use their selected crop, orientation, repeat, and coordinate transform; unsupported synthesis must not become centered full-source sampling. | Critical | 3-5 d | A1, A2, A4 | `crates/sheet-compiler/src/slot_synthesis.rs:113-179,296-373`; `crates/placement-solver/src/candidates.rs:397-424` | Golden grid image proves DirectCrop, Cover, Contain, RepeatX/Y, TileXY, slice, and radial modes map known source coordinates to known atlas pixels. |
| C2 | Compile the existing structural profile for each slot/hotspot, combine its height with sampled material height under an explicit range contract, and derive/compose normals once. Do not let a later imported-normal copy overwrite generated normals. | Critical | 4-6 d | A4, A6, B1 | `crates/render-core/src/structural_profile.rs:189-250,333-373`; `crates/sheet-compiler/src/document_compiler.rs:670-705`; `crates/sheet-compiler/src/intermediate_atlas.rs:163-179` | Rectangular-slot golden has measurable center/edge height difference and expected normal direction; imported flat normal plus bevel remains beveled. |
| C3 | Generate masks and effects in hotspot-local pixel space, scale widths from physical/pixel scale, then composite inside allocation bounds. | High | 3-5 d | A4, C2; actual effect-plan compiler | `crates/render-core/src/structural_profile.rs:333-373`; `crates/domain/src/algorithm_stack.rs:361-367`; `crates/effect-compiler/src/lib.rs:1-14` | Same physical edge effect remains visible and proportionate at 512, 1024, and 4096; padding stays untouched. |
| C4 | Implement the missing effect-plan body behind the existing `EffectPlanHeader` for one existing scale-aware edge/chip/weathering operation, then publish its stable ID. This is completion of the intended stage, not a new renderer. | High | 3-5 d | A3, C3 | `crates/domain/src/algorithm_stack.rs:361-367`; `crates/effect-compiler/src/lib.rs:1-14`; `crates/effect-compiler/src/stage10.rs` | Deterministic fixture asserts effect-plan ID, mask extent, affected channel pixels, and zero changes outside the hotspot. |
| C5 | Define channel composition and conversion rules: linear/sRGB, height range, roughness/AO defaults, normal convention, and alpha/validity. Apply them before atlas encoding. | High | 2-4 d | A6, C2 | `crates/image-io/src/normalization.rs`; `crates/render-core/src/structural_profile.rs:217-242`; `crates/sheet-compiler/src/intermediate_atlas.rs:132-179` | Numeric fixtures verify no height collapse, expected tangent-space Y sign, and round-trip values within tolerances. |
| C6 | Avoid retaining and hashing unnecessary full rendered-slot pixel planes when identity can be derived from immutable inputs plus renderer version; retain inspectable data only for requested diagnostics. | Medium | 1-2 d | A3, B4 | `crates/sheet-compiler/src/intermediate_atlas.rs:209-235`; `crates/sheet-compiler/src/slot_synthesis.rs` | Result IDs remain stable; profiling shows reduced peak retained bytes and no full-plane rehash on a cache hit. |

## D. Preview fixes

| ID | Required work | Severity | Effort | Dependencies | Affected files | Verification |
|---|---|---:|---:|---|---|---|
| D1 | Replace the current projection-shaped response with a `CompiledPreviewArtifact` matching the compiled maps, topology, stable IDs, diagnostics, revision, and cache metadata. The preview remains a consumer; it must not call algorithms. | Critical | 2-4 d | A3, A5, A6, B1 | `packages/ipc-contracts/src/document-contracts.ts:279-315`; `apps/desktop/src-tauri/src/document_commands.rs:1228-1260`; `apps/desktop/src/source-first-app.tsx:621-727` | Contract/serde test round-trips all map descriptors and IDs; UI test proves no stage/native call occurs when switching among already supplied maps. |
| D2 | Stop PNG+base64 JSON transfer in the hot path. Publish a binary/asset handle supported by the existing desktop boundary, with immutable revisioned lifetime. Keep PNG only for explicit save/export or a measured fallback. | Critical | 2-4 d | D1 | `apps/desktop/src-tauri/src/document_commands.rs:1238-1260,1751-1761`; `packages/ipc-contracts/src/document-contracts.ts:279-315` | A 1024 preview response does not contain a multi-megabyte base64 string; transfer/upload timings and bytes are reported. |
| D3 | Bind and display the selected generated map from the artifact, including Height, Normal, Roughness, AO, and Region ID inspection. Do not claim a mesh/UV defect in the current HTML-image view; add mesh bindings only if/when the existing product preview requires them. | High | 2-3 d | D1 | `apps/desktop/src/source-first-app.tsx:1668-1679,1737-1756`; `crates/preview/src/lib.rs:1-3` | UI golden switches maps without recompilation and shows the expected per-channel fixture. |
| D4 | Make draft crop publication revision-safe: debounce/coalesce drag updates, cancel old revisions, and never display a stale texture or stale overlays. | Critical | 1-2 d | A7, B5, D1 | `apps/desktop/src/source-first-app.tsx:702-727,1309-1347`; `apps/desktop/src-tauri/src/document_commands.rs:147-151,1001-1013` | Automated drag emits bounded compiles; last pointer position wins under forced out-of-order native responses. |
| D5 | Show a preview-resolution artifact first and allow the same orchestrator to refine to authoritative quality asynchronously. | High | 1-2 d | B4, D1 | `apps/desktop/src/source-first-app.tsx:621-727`; `apps/desktop/src-tauri/src/document_commands.rs:1001-1013` | Cold preview appears within target while a later revision replaces it only if inputs are unchanged. |

## E. Performance fixes

| ID | Required work | Severity | Effort | Dependencies | Affected files | Verification |
|---|---|---:|---:|---|---|---|
| E1 | Decode each source once per digest/orientation and share Stage 2-7 analysis across slots/domains where inputs are identical. The measured cold path decoded/prepared the source for patch, full source, and post-compile UI metadata. | Critical | Included in B2 | B2 | `crates/sheet-compiler/src/persisted_pipeline.rs:273-340,534-550`; `apps/desktop/src-tauri/src/document_commands.rs:1228-1237,1599-1622` | Representative trace reports one cold decode and zero warm decodes. |
| E2 | Reuse Stage 11 measurement data and integral features across 53 slots instead of rescanning fields per candidate/slot; bound candidate expansion before Stage 13. The measured run had 2,022 authored candidates and 8.49 seconds in Stage 13. | High | 3-5 d | A4, B3 | `crates/sheet-compiler/src/persisted_pipeline.rs:446-512`; `crates/placement-solver/src/optimizer.rs:284-350` | Same chosen plan/score on the fixture with measured Stage 11-13 budget below 300 ms warm and a recorded candidate cap. |
| E3 | Render independent selected slots concurrently after Stage 13, with deterministic atlas composition and bounded worker memory. The current slot loop is serial. | Medium | 2-3 d | C1 | `crates/sheet-compiler/src/persisted_pipeline.rs:212-222`; `crates/sheet-compiler/src/intermediate_atlas.rs:147-179` | 1-thread and N-thread outputs/IDs match byte-for-byte; N-thread trace improves Stage 14 without exceeding memory bound. |
| E4 | Allocate only requested preview maps and reuse atlas/storage buffers. Avoid concurrent full-size RGBA, correspondence, validity, PNG clone, and base64 copies unless requested. | High | 2-3 d | A6, B4, D2 | `crates/sheet-compiler/src/intermediate_atlas.rs:132-142`; `apps/desktop/src-tauri/src/document_commands.rs:1238-1260,1751-1761` | Allocation counters/peak memory for Base Color-only 1024 preview meet an explicit byte budget. |
| E5 | Add per-stage timing, decode/allocation/cache counters, and publication/upload/display marks to the compiled diagnostics in debug/performance builds. | High | 1-2 d | D1 | compiler orchestration, Tauri command, `apps/desktop/src/source-first-app.tsx` | One automated representative run emits every timing requested by the audit, including IPC/upload/display, with cache hit/miss counts. |
| E6 | Avoid filesystem rereads and synchronous PNG work after the compile. If an external source must be read, do it once in the background compile session and attach metadata to the artifact. | High | 1-2 d | B2, D2 | `crates/sheet-compiler/src/persisted_pipeline.rs:534-550`; `apps/desktop/src-tauri/src/document_commands.rs:1228-1260` | Trace contains no post-compile source decode/read and no PNG encode for normal preview. |

## Vertical integration milestones

These milestones deliberately slice vertically through the existing compiler. Each produces pixels through the same Tauri command/artifact boundary that later milestones retain.

### Milestone 1 - one source, one slot, Base Color crop, preview

- **Reuse:** project snapshot/registered input, Stage 2 preparation, Stage 9 geometry for one selected slot, DirectCrop/Cover/Contain Stage 14 branches, intermediate atlas, existing HTML image display (`persisted_pipeline.rs:113-247`; `slot_synthesis.rs:296-373`).
- **Temporarily bypass:** Stage 3-8 analysis unless needed by the selected direct crop; global multi-slot optimization; structural/effect maps.
- **Exact changes:** A1, A3, A4, A7, C1, D1, D4; add a one-slot/requested-map compile profile to B4 without creating a separate compiler.
- **Automated integration test:** 4-color coordinate-grid source, explicit crop, one slot; assert source corner pixels, stable IDs, destination rectangle, and draft-revision behavior through the Tauri-facing compiler API.
- **Visual golden:** one rectangular slot showing a visibly off-center crop plus transparent/checker outside its allocation.
- **Runtime target:** cold <=1.0 s, warm crop update <=150 ms at 512 square.

### Milestone 2 - one source, complete fixed template, Base Color crop/repeat, preview

- **Reuse:** Stage 9's 53 stable Generic Architecture slots, Stage 10 demands, Stage 11/12 candidates/scores, Stage 13 plan, Stage 14 render, atlas topology (`templates/mod.rs:190-320`; `persisted_pipeline.rs:146-246`).
- **Temporarily bypass:** synthesis candidates that fail A2; structural/effects and non-Base Color maps.
- **Exact changes:** A2, B2-B4, C1, E1-E4. Constrain offered modes to verified implementations until synthesis is enabled in Milestone 7.
- **Automated integration test:** fixed template with coordinate/stripe source asserts 53 unique slot IDs, non-overlapping allocation rectangles, expected crop/repeat transform class, and deterministic sampling-plan ID.
- **Visual golden:** complete Base Color sheet where repeated strips visibly repeat and unique details visibly crop rather than share the same centered band.
- **Runtime target:** cold <=3 s, warm <=500 ms at 512; no optimizer run for display-only changes.

### Milestone 3 - structural height and normal for every slot

- **Reuse:** Stage 9 hotspot geometry, `compile_structural_maps`, existing atlas composition mechanics (`render-core/src/structural_profile.rs:189-250,333-373`; `intermediate_atlas.rs:147-179`).
- **Temporarily bypass:** chips/weathering and generated material roughness/AO.
- **Exact changes:** A4, A6, B1, C2, C5. Integrate structural output after selected sampling and before final atlas publication in `compile_persisted`.
- **Automated integration test:** every rectangular/radial/cap slot has valid height/normal pixels only in its hotspot; numeric normals match the declared convention.
- **Visual golden:** Base Color, grayscale Height, and lit-normal inspection for the same template.
- **Runtime target:** warm <=800 ms at 512 for all three maps.

### Milestone 4 - Region ID and verified slot identity

- **Reuse:** Stage 9 topology, Stage 13 placements, Stage 14 slot result IDs, atlas destination rectangles (`intermediate_atlas.rs:147-187`).
- **Temporarily bypass:** material effects.
- **Exact changes:** A3, A5, D1, D3. Publish a lossless Region ID plane and lookup table in the same artifact.
- **Automated integration test:** sample every hotspot center and border; resolve exactly one expected region/slot ID, with no duplicate destination assignment.
- **Visual golden:** false-color Region ID atlas with 53 distinguishable slot regions and allocation padding visible.
- **Runtime target:** <=50 ms incremental cost at 512; zero recompilation when merely switching to Region ID view.

### Milestone 5 - roughness and AO

- **Reuse:** imported channel registration/preparation, the shared Stage 14 sampling transform, final channel composer material from the old compositor (`slot_synthesis.rs:129-147,432-447`; `document_compiler.rs:200-315`).
- **Temporarily bypass:** weathering effects and synthesis.
- **Exact changes:** A6, B1, C5, D1-D3, E4. Apply declared fallback values per missing slot instead of intersecting channel roles.
- **Automated integration test:** checker/gradient cross-channel fixture proves Base Color, Roughness, and AO use the identical source transform; one missing channel exercises the fallback diagnostic.
- **Visual golden:** aligned Base Color/Roughness/AO atlas triplet with no shifted edges.
- **Runtime target:** warm <=1.1 s at 512 for Base Color+Height+Normal+Roughness+AO.

### Milestone 6 - one scale-aware edge/chip/weathering effect

- **Reuse:** physical scale already carried into sampling plans, hotspot geometry, structural edge distance/profile, `EffectPlanHeader` (`optimizer.rs:805-900`; `structural_profile.rs:333-373`; `algorithm_stack.rs:361-367`).
- **Temporarily bypass:** all other unimplemented effect families.
- **Exact changes:** C3-C5 and effect-plan identity from A3; implement one deterministic existing intended effect and compose it in the one authoritative compiler.
- **Automated integration test:** effect width in physical units is stable across 512/1024/4096 and affects only declared channels/hotspot pixels.
- **Visual golden:** before/after Base Color, Height, Normal, and Roughness crops at a trim edge.
- **Runtime target:** <=250 ms incremental at 512.

### Milestone 7 - existing placement and synthesis algorithms where needed

- **Reuse:** Stage 3-8 descriptors/fields/routes, Stage 11 candidates, Stage 12 scoring, Stage 13 optimizer, existing synthesis implementation selected by route (`persisted_pipeline.rs:273-340`; `candidates.rs:397-424`).
- **Temporarily bypass:** no stage; unsupported route/mode combinations remain explicit errors rather than fallbacks.
- **Exact changes:** finish A2/C1 for synthesis, reuse B2/B3 caches, and route Stage 14 through the existing synthesis artifact rather than the generic sampler. Make candidate diagnostics truthful.
- **Automated integration test:** fixture forces synthesis, proves synthesized pixels differ from centered DirectSource sampling, and preserves candidate/plan/result identity across a warm cache hit.
- **Visual golden:** side-by-side DirectCrop, Repeat, and Synthesis slots from the same source.
- **Runtime target:** direct modes retain Milestone 6 target; cached synthesis preview <=2 s, with slower authoritative refinement asynchronous.

### Milestone 8 - interactive preview performance

- **Reuse:** same compile request/artifact and all prior caches; no preview-only algorithms.
- **Temporarily bypass:** nothing semantically; draft requests use preview resolution/quality and authoritative refinement follows.
- **Exact changes:** B4/B5, D2/D4/D5, E2-E6; add timing/cache/allocation diagnostics and enforce budgets in CI performance fixtures.
- **Automated integration test:** scripted crop drag with 30 updates checks coalescing, cancellation, last-revision publication, cache hits, map identity, and runtime budgets.
- **Visual golden:** first draft and final refined artifact must be compositionally identical at matched resolution; allowed differences are only documented quality/refinement details.
- **Runtime target:** cold first visible 512 preview <=2 s; warm input-to-visible <=250 ms; cached map switch <=50 ms; authoritative 1024 refinement <=5 s on the audit machine.

## Distance estimate

### Functional percentages

These percentages weight product capabilities, not lines of code. They are estimates grounded in the matrix and the measured live route.

| Measure | Estimate | Evidence basis |
|---|---:|---|
| Required algorithm capabilities substantially exist | **72%** | Source preparation, template resolution, demands, candidates, scoring, placement, sampling plans, direct/repeat/slice/radial sampling, atlas copy, and structural profile primitives exist. Effect-plan execution, weathering/chips, Region ID generation, final multi-channel composition, and a real preview material contract do not (`docs/runtime-stage-matrix.json`). |
| Required product path connected to the live runtime | **56%** | Stages 1-14 are called, but the route ends at intermediate imported channels; structural maps, effects, final maps, export, and preview-map binding are outside it (`persisted_pipeline.rs:113-247`; `intermediate_atlas.rs:196-205`). |
| Required product path correctly consumed by Stage 14P | **34%** | Stage 14P displays one PNG/base64 channel. It ignores transient crop input, has a synthesis fallback mismatch, exposes no structural/effect/Region ID maps, and has no mesh/PBR consumer (`source-first-app.tsx:702-727,1668-1679,1737-1756`; `slot_synthesis.rs:149-166,296-373`). |

Of the remaining engineering work, the realistic allocation is:

| Work class | Share |
|---|---:|
| Contract and orchestration | **36%** |
| Rendering correctness | **27%** |
| Preview correctness | **14%** |
| Performance | **23%** |

The shares sum to 100%. Performance work overlaps implementation sequencing but is counted here by its primary purpose.

### Engineering-day scenarios

| Scenario | Engineering days | Assumptions |
|---|---:|---|
| Optimistic | **22-28 d** | Source mapping migration is simple, existing synthesis accepts the compiled domain without adaptation, old structural composer extracts cleanly, binary preview handles already fit the Tauri stack, and cache keys expose no hidden invalidation issue. |
| Realistic | **38-52 d** | Includes the eight vertical milestones, contract migrations, a small effect-plan implementation, focused goldens, per-stage caches, cancellation, and one performance correction pass on the audit hardware. |
| Pessimistic | **65-85 d** | Existing saved projects need dual-semantics migration, synthesis/domain contracts require rework, multi-channel composition reveals color/normal/height convention defects, and memory/IPC changes need platform-specific stabilization. |

These totals are not the sum of every table row because several rows are intentionally delivered together and some performance work is included in orchestration estimates.

## Critical path

The critical path is:

1. Fix source-mapping and transient-crop truth (A1, A7).
2. Enforce route/mode/render agreement and exact placement consumption (A2, C1).
3. Stabilize identity, coordinate spaces, and the compiled artifact (A3-A6, D1).
4. Introduce reuse plus preview-resolution requests before adding more maps (B2-B4).
5. Compose structural Height/Normal in the authoritative compiler (B1, C2, C5).
6. Add Region ID, roughness/AO, and one effect through that same artifact (A5, C3-C5).
7. Remove hot-path PNG/base64 transfer and close cancellation/performance budgets (D2, B5, E2-E6).

The first review gate should be Milestone 1. Until its coordinate and identity assertions pass through the actual Tauri-facing contract, enabling more stages will make the current visual and performance failures harder to localize.
