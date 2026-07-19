# Hot Trimmer GPU rendering migration execution prompt pack

## Purpose and authority

This is the implementation-grade companion to
[`docs/gpu-rendering-migration-plan.md`](gpu-rendering-migration-plan.md). The migration plan owns the architectural
decision, target performance, CPU/GPU boundary, tiling model, and five-phase sequence. This file turns those decisions
into five exhaustive prompts intended to be pasted into five separate Codex tasks.

If this prompt pack and the migration plan ever disagree, stop and reconcile the documents before changing code. Do
not let an implementation task quietly choose a third architecture.

These prompts begin only after all four prompts in
[`docs/manual-layout-preset-product-prompt-pack.md`](manual-layout-preset-product-prompt-pack.md) are accepted. They
assume that authored layouts, multi-source patch assignment, explicit direct/loop/radial region semantics, and
truthful Base Color already work through `compile_persisted`.

## How to run this pack

1. Run exactly one prompt per Codex task, in numerical order.
2. Give the next task the prior task's final report, benchmark artifact, and unresolved issues.
3. Review the native application and recorded evidence after every prompt.
4. Do not start the next prompt while the current acceptance gate is open.
5. Do not run material/effect/export prompt packs in parallel with this migration.
6. A prompt is incomplete if it implements contracts or tests but does not exercise the real production call path.
7. A prompt is incomplete if it reports only unit tests when its acceptance section requires native or real-resolution
   evidence.
8. Preserve unrelated worktree changes. Never use destructive Git commands to make the diff look clean.

## Locked rules for all five prompts

- `TrimSheetDocument` remains the sole project/document authority.
- `compile_persisted` remains the sole production compile and orchestration spine.
- The GPU service executes compiled pixel commands. It never resolves layout, chooses crops, invents sampling modes,
  reads UI state, or reaches back into raw project state.
- Add no TypeScript renderer, frontend shader implementation, preview compiler, legacy fallback, second native
  compositor, or hardcoded replacement pipeline.
- Preview and export consume the same compiled plan and the same authoritative GPU kernels.
- Preserve stable `RegionId`, `RegionBinding`, `SourceSetId`, `PatchId`, source crop, transform, continuity, radial,
  destination, and effect-plan identities end to end.
- No unsupported route may silently become stretch, centered crop, direct crop, or another sampling mode.
- No GPU failure may silently trigger the old slow CPU production path. Unsupported devices and device loss require a
  deliberate typed product response.
- Do not implement new visual/material algorithms during the migration. Port existing authoritative math only.
- Do not store one UUID per atlas pixel. GPU ownership uses a compact integer plus a stable artifact-level lookup.
- Do not allocate all maps when one map was requested.
- Do not PNG-encode or Base64-encode interactive preview pixels.
- Do not assume 16K/24K output or sources fit one GPU texture. Query the adapter and tile.
- Do not claim 8K evidence using a small fixture with an 8K filename.
- Keep cancellation, document revision, draft ID, and cache identity visible at every asynchronous boundary.
- Do not broaden a prompt into unrelated UX, layout, asset-browser, or export work.
- Follow `AGENTS.md`: work in the root task, use no subagents unless explicitly requested, run the one focused
  verification command, make at most one correction pass, rerun the same command, and stop.

## Required report format for every task

The final implementation report must contain all of the following. Missing sections mean the prompt is not accepted.

```text
Outcome
- What is now visible/working in the real application.

Authoritative runtime route
- UI request -> native command -> compile_persisted -> compiled plan -> executor -> publication -> display/export.
- Identify any temporary CPU boundary that remains and why it is removed by the next prompt.

Contracts added or changed
- Exact types, fields, identity rules, coordinate spaces, formats, and versioning.

Files changed
- Group by compiler, GPU service/shaders, native IPC, frontend consumer, tests/fixtures, and docs.

Correctness evidence
- Focused command and result.
- Golden/parity tolerances and why each tolerance is legal.
- Native visual evidence required by the prompt.

Performance evidence
- Hardware, backend, driver, build profile, real source dimensions/count, output dimensions, map set, region count.
- Cold/warm timings by decode/plan/upload/dispatch/readback/encode/IPC/paint.
- Peak CPU memory and estimated/measured GPU memory where required.
- Cache hit/miss and bytes uploaded/read back.

Acceptance checklist
- One line per acceptance requirement: PASS or FAIL with evidence.

Remaining work
- Only work intentionally assigned to a later numbered prompt.
- No vague “future optimization” for a failed current requirement.
```

## Shared target contracts

The exact Rust names may follow repository conventions, but every implementation must preserve these semantics.

### Compiled atlas plan

The immutable executor input must include:

- Contract/schema version.
- Document revision, draft/request generation, topology hash, appearance hash, and complete plan hash.
- Output dimensions, preview/export profile, requested maps, normal convention, and color-space policy.
- Requested full extent or exact output tile/ROI, mip, halo, and valid interior.
- Ordered source records with stable source ID, content digest, oriented dimensions, decoded format/color space, map role,
  and upload/cache identity.
- Ordered region records with stable `RegionId`, compact region index, binding/source/patch IDs, exact source crop, source
  dimensions, destination atlas rectangle, sampling mode, source-to-region transform, radial parameters, continuity,
  padding, edge eligibility, and upstream plan identities.
- Existing structural/effect plan identities when those become active in Prompt 4.
- Typed validation diagnostics.

Every cache-relevant field must participate in deterministic identity. Two plans with the same identity must be safe to
reuse byte-for-byte within documented GPU numeric tolerance. A change to crop, radial parameter, output profile,
requested map, normal convention, decoder version, or shader/algorithm version must invalidate the affected result.

### GPU service

The application-owned service must eventually own:

- One long-lived `wgpu::Instance`, selected `Adapter`, `Device`, and `Queue`.
- Adapter/backend/driver/limits/features/format capability record.
- Versioned shader and pipeline cache.
- Bounded source texture/tile cache.
- Bounded intermediate and rendered output tile cache.
- Reusable upload and readback staging pools.
- Explicit CPU and VRAM budgets with LRU or equivalent deterministic eviction.
- Request generation, cancellation, stale-publication prevention, error scopes, uncaptured errors, and device-loss
  recovery.
- CPU wall timings and GPU timestamps when supported.

It must not be reconstructed per click, drag, map change, region, or tile.

### Compiled preview artifact

Metadata may travel as JSON. Pixel payloads may not be embedded as Base64 strings. The artifact must eventually expose:

- Document revision, request/draft generation, topology/appearance/plan identities.
- Output size, profile, requested/available maps, normal convention, and color-space information.
- Stable compact-index-to-`RegionId` table and region metadata.
- Tile manifest entries with map, mip, output rectangle, valid rectangle, halo, generation, pixel format, row stride,
  and raw-resource handle/endpoint.
- Typed diagnostics.
- Decode/upload/dispatch/readback/publication/paint timings.
- Source/pipeline/tile cache hits, misses, bytes, and evictions.
- Adapter/backend/driver and memory telemetry.

The frontend may blit bytes into its display surface. It may not perform crop, loop, radial, mask, padding, normal,
effect, or atlas math.

---

## Prompt 1 - Real-scale baseline and immutable execution contract

```text
Implement GPU Rendering Migration Prompt 1 exactly as specified in
docs/gpu-rendering-migration-execution-prompt-pack.md. This prompt establishes truthful real-8K evidence, separates
compile_persisted planning from pixel execution without changing pixels, and creates the long-lived GPU capability
boundary. Do not implement GPU pixel rendering yet.

Read first, completely:
1. AGENTS.md.
2. docs/gpu-rendering-migration-plan.md.
3. docs/gpu-rendering-migration-execution-prompt-pack.md.
4. docs/manual-layout-preset-product-prompt-pack.md and its accepted Prompt 4 report.
5. The current git diff/status, preserving all unrelated changes.
6. crates/sheet-compiler/src/persisted_pipeline.rs, especially compile_persisted and the manual-layout/SourceFrame path.
7. crates/sheet-compiler/src/slot_synthesis.rs.
8. crates/sheet-compiler/src/intermediate_atlas.rs.
9. crates/preview and crates/render-core.
10. apps/desktop/src-tauri/src/document_commands.rs Stage 14 command, spawn_blocking boundary, artifact conversion,
    encoding, and publication.
11. IPC contracts and the frontend preview request/controller/consumer.
12. crates/sheet-compiler/tests/source_frame_e2e.rs and any benchmark/telemetry tests that claim large-source coverage.

Before editing:
- Trace the real native preview call from the UI request through compile_persisted to the current CPU slot synthesis,
  CPU atlas composition, PNG/Base64 publication, and paint callback.
- Identify the exact current types that already contain each required CompiledAtlasPlan field. Reuse or embed those
  types when they have correct semantics; do not create duplicate concepts with different coordinate conventions.
- Record all existing cache keys and which exact inputs they omit.
- Locate every test fixture whose filename/description claims 8K while its actual pixel buffer is small.
- Inspect Cargo.lock/workspace Rust policy and select an explicit wgpu version/toolchain pairing. The repository
  currently declares Rust 1.85 while current wgpu documentation declares a newer MSRV. Either deliberately upgrade the
  repository toolchain with compatibility proof or pin a compatible wgpu version. Record the decision and reason.

Implement A - real-resolution benchmark and trace:
- Add one dedicated ignored release benchmark or diagnostic executable. It must not make ordinary unit tests allocate
  hundreds of megabytes.
- The harness must decode or deterministically generate an actual image at least 7952x4016. Its in-memory dimensions
  must match the reported dimensions. Do not merely name it “8K.”
- Exercise the accepted authored manual-layout path through the real compile_persisted entry point, with at least 63
  regions including direct, explicit loop, and radial behavior if those modes are accepted in the product baseline.
- Use an 8192x8192 Base Color output for the authoritative measurement. If the machine cannot complete it, record the
  actual failure and peak observed resource use; do not substitute 2048 and call it 8K.
- Measure a cold run after explicitly emptying the relevant process-owned caches and at least two warm runs without
  mutating inputs.
- Emit machine-readable JSON plus a concise Markdown summary. Store no giant generated source/output in Git.
- Record: build profile, commit/worktree identity, OS, CPU, logical/physical threads, RAM, GPU(s), driver/backend if
  available, source count/formats/dimensions/decoded bytes, region/patch count, requested maps, output size, profile,
  image decode count, cache hits/misses/evictions, full-frame allocation counts/bytes, peak RSS if available, and
  snapshot/decode/prepare/plan/Stage14/compose/encode/Base64/IPC/paint wall time.
- Separate compiler completion from browser paint. If paint cannot be captured in the harness, instrument the native
  walkthrough and join records by request generation.
- Correct misleading test names/comments/assertions that imply real-resolution performance while using small images.
  Preserve useful small tests as coordinate/correctness fixtures.

Implement B - immutable compiled plan:
- Introduce one versioned CompiledAtlasPlan or repository-consistent equivalent at the sheet-compiler/render boundary.
- Include every field listed under “Shared target contracts / Compiled atlas plan” in this prompt pack that is relevant
  through Stage 14 Base Color.
- Use explicit types for source pixels, normalized coordinates, output pixels, tile rectangles, and transforms. Do not
  use an unlabelled four-float rectangle across coordinate spaces.
- Preserve stable IDs. The compact region index is deterministic for one plan and maps losslessly to RegionId; it is
  not a replacement project identity.
- Make plan ordering deterministic. Do not zip independently ordered arrays. Each compiled region command owns its
  exact IDs and parameters.
- Add plan validation for zero/out-of-bounds crops, unsupported sampling/binding pairs, destination bounds, duplicate
  compact indices, missing source identity, inconsistent oriented dimensions, and integer overflow.
- Hash the schema/algorithm version plus every output-affecting field. Add tests proving crop, radial, requested-map,
  output-size, decoder-version, and normal-convention changes alter the correct identity.
- The compiled plan contains no raw UI references, mutable document handles, callbacks, or global-default lookups.

Implement C - executor boundary with unchanged CPU output:
- Add one executor interface used internally by compile_persisted after planning/validation.
- The executor accepts only the immutable plan, prepared source resources identified by that plan, cancellation/current
  revision guards, and an explicit cache/service context.
- Adapt the existing CPU synthesis/composition code behind this interface for Prompt 1. Do not copy the rasterizer into
  a new implementation.
- Every production Stage 14 request must pass through the new plan and executor. Prove this with a capturing/fake
  executor test that receives the exact compiled IDs/crops/transforms/destinations.
- Preserve the public compile_persisted command and artifact shape unless a versioned additive diagnostic is needed.
- Pixel output, selected modes, IDs, error behavior, and caching must remain unchanged in Prompt 1.
- Keep cancellation checks at least as frequent as before. Do not turn the interface into an uninterruptible monolith.

Implement D - long-lived GPU capability boundary only:
- Add the pinned wgpu dependency in the existing GPU boundary, preferably crates/preview unless direct inspection
  demonstrates an already-established better owner. Do not create a new orchestration crate.
- Create an application-lifetime service/state object capable of initializing Instance/Adapter/Device/Queue once.
- Enumerate adapter name/vendor/device/backend/driver/limits/features, storage/sampled texture support for candidate
  formats, timestamp-query support, row-alignment constraints, and maximum safe 2D dimensions.
- Derive an initial tile-size recommendation from capabilities and memory policy; do not hardcode “24K texture.”
- Publish capability/init diagnostics through the existing native diagnostic channel.
- Initialization must not run once per compile. Add a focused proof that repeated requests reuse the same service
  generation/device state.
- Handle “no supported adapter” as a typed capability result. Do not fall back silently and do not change output route
  in this prompt.
- Do not add WGSL sampling kernels or claim GPU acceleration yet.

Tests and evidence:
- Add deterministic plan serialization/hash/order tests.
- Add a fake/capturing executor test proving compile_persisted submits exact manual-layout IDs, source crops, transforms,
  direct/loop/radial modes, destinations, and requested maps.
- Keep the accepted Stage 14 pixel-exact small fixtures green.
- Produce one native diagnostic screenshot/text capture showing adapter capabilities and that preview pixels remain
  unchanged.
- Run the real release benchmark separately and attach JSON/Markdown output paths and exact command.

Forbidden shortcuts:
- Do not move orchestration into crates/preview.
- Do not implement a second CompiledAtlasPlan for preview and export.
- Do not make a GPU device per request.
- Do not delete CPU code yet.
- Do not claim the benchmark target passed with a synthetic tiny fixture.
- Do not weaken an existing pixel test to accommodate the refactor.

Acceptance gate:
- PASS: compile_persisted is still the only production entry and every request crosses the immutable plan/executor
  boundary.
- PASS: existing pixels and stable identities are unchanged.
- PASS: a real 8192 output benchmark/failed-resource trace exists with actual source dimensions and cold/warm timing.
- PASS: GPU/toolchain version is pinned and justified.
- PASS: the long-lived capability service reports real adapter limits and is reused.
- PASS: no GPU pixel work is falsely claimed.

Focused verification command:
cargo test -p hot-trimmer-sheet-compiler gpu_execution_contract

Run exactly that focused verification command. If it fails, make at most one correction pass and rerun the same
command. Run the ignored real-8K release benchmark as acceptance evidence, not as a substitute test. Stop after the
Prompt 1 report. Do not begin Prompt 2.
```

---

## Prompt 2 - Authoritative GPU Stage 14 Base Color

```text
Implement GPU Rendering Migration Prompt 2 exactly as specified in
docs/gpu-rendering-migration-execution-prompt-pack.md. Route authoritative Stage 14 Base Color sampling through one
long-lived native wgpu executor beneath compile_persisted. Preserve the accepted sampling semantics exactly.

Read first, completely:
1. AGENTS.md.
2. docs/gpu-rendering-migration-plan.md.
3. docs/gpu-rendering-migration-execution-prompt-pack.md.
4. The accepted Prompt 1 report, CompiledAtlasPlan contract, CPU baseline JSON, and adapter capability record.
5. SamplingPlan, RegionBinding, SourceCrop, sampling/radial/continuity contracts, and their persistence versions.
6. The exact CPU formulas and coordinate conventions in slot_synthesis.rs.
7. GPU service lifetime, caches, error/cancellation handling, and render-core/preview boundaries.
8. Current native artifact conversion/publication. PNG/Base64 removal belongs to Prompt 3, but its cost must remain
   separately measurable here.

Before editing:
- Write a mode table from actual accepted code: binding type, sampling mode, coordinate source, crop units, pixel-center
  convention, orientation, filter/address policy, alpha/color policy, radial parameters, seam handling, and legal
  output.
- Identify every CPU fallback/default branch. Classify each as authoritative, diagnostic error, or forbidden fallback.
- Determine the batching strategy from real adapter limits. Do not assume unlimited source texture arrays/bindings.
- Decide the exact GPU output format and color-space behavior for Base Color. Record whether source uploads are sRGB or
  linear and where conversion occurs.
- Define parity tolerances before writing shaders. Direct integer-aligned crop should be exact. Bilinear/radial paths
  may use a narrowly documented numeric tolerance derived from filter/float behavior, not a broad visual threshold.

Implement A - GPU source resources:
- Extend the Prompt 1 long-lived service; do not construct another device/service inside sheet-compiler.
- Add a bounded source texture cache keyed by source digest, map role, oriented dimensions, decoded pixel format,
  color-space/decoder version, and orientation application.
- Upload each exact source once per cache identity. Record upload bytes/time and hit/miss/eviction counts.
- Respect wgpu row alignment using explicit padded staging rows without changing logical source coordinates.
- Keep decoded CPU source lifetime independent from GPU cache lifetime. Do not retain duplicate decoded images without
  a bounded reason.
- For this prompt, support sources that fit qualified adapter limits. If a source does not fit, return a typed
  SourceRequiresTiling diagnostic naming Prompt 5 support; never downscale or silently use CPU.

Implement B - versioned GPU command buffers:
- Convert validated CompiledAtlasPlan region records into tightly packed, versioned GPU command data. Keep a lossless
  CPU-side association with RegionId for diagnostics.
- Include destination bounds, exact crop/source dimensions, transform, mode discriminator, loop periods, radial
  center/radius/angle/warp/seam parameters, alpha/filter flags, and compact region index.
- Validate finite values, legal ranges, multiplication/addition overflow, destination overlap assumptions, and buffer
  alignment before submission.
- Pipeline/shader cache identity includes shader/algorithm version, format, mode family, and relevant feature choices.

Implement C - authoritative WGSL Stage 14 kernels:
- Implement the accepted direct crop mapping using output pixel centers and the exact source pixel/normalized conversion
  already proven by CPU tests.
- Implement explicit LoopX, LoopY, and LoopXY only where the compiled contract authorizes them. Modulo/repeat uses the
  authored crop/period, not the whole source unless explicitly authored.
- Implement planar sampling and planar-to-radial mapping using the compiled center, radius, angular interval, warp,
  fisheye/shape parameters, orientation, and seam policy. Port the current authoritative formula; do not substitute a
  visually similar polar mapping.
- Preserve transparent/out-of-domain behavior, alpha, clamp rules, and bilinear sample footprint.
- Unsupported mode/binding/format pairs produce typed diagnostics before dispatch.
- Every invocation writes only its compiled destination. No region may write another region's rectangle.
- Use one provisional Base Color atlas-sized output for <= adapter-limit output so the prompt does not read back one
  allocation per region. Prompt 3 adds formal tiled atlas/padding/ID behavior.
- Batch commands in deterministic groups that respect source bindings and adapter limits. Do not serialize one GPU
  submission and readback per region.
- Insert cancellation/current-generation checks before upload, before each batch, before readback, and before publish.

Implement D - production routing and comparison mode:
- Add the GPU executor as the production Base Color executor selected by the existing compile_persisted route when a
  supported adapter is available.
- Keep the CPU executor callable only by tests and an explicit developer parity mode. It must not be an automatic
  runtime fallback.
- Developer parity mode runs CPU and GPU from the same immutable plan, records mismatched pixels/regions/max error, and
  publishes the GPU result only if the current request is still valid.
- The public artifact remains authoritative and identifies executor backend, shader version, plan hash, timings, and
  whether parity comparison ran.
- Do not remove PNG/Base64 publication yet. Measure GPU completion, readback, encoding, IPC preparation, and paint
  separately so a slow transport is not blamed on the kernel.

Implement E - caches and invalidation:
- Cache source textures and, where safe, completed Base Color output by exact plan/output/profile identity.
- A crop, destination, radial, continuity, source digest, output size, filter/color policy, or shader version change
  invalidates the affected result.
- Selection-only and inspector-only changes do not invalidate pixels.
- Stale/cancelled output never enters the cache as a publishable current artifact.
- Cache budgets and evictions are visible in telemetry.

Tests and evidence:
- Add GPU/CPU parity fixtures for DirectCrop, LoopX, LoopY, LoopXY, non-square oriented input, alpha edge, crop edge,
  transform scale/offset, radial center/radius, angular seam, warp/fisheye, and unsupported-pair diagnostics.
- Add a multi-region test proving distinct crops/destinations/RegionIds remain distinct and one region never writes
  another destination.
- Add cache tests: warm source upload hit, crop invalidation, radial invalidation, output-size invalidation, shader
  version invalidation, and selection non-invalidation.
- Run the real Prompt 1 workload at 8192 output in release mode: one cold and at least two warm runs. Record decode,
  plan, source upload, command packing, GPU dispatch, readback, existing encode/Base64/IPC, paint, and end-to-end.
- Native visual evidence must show the same project/revision with at least one direct, looped, and radial region at a
  useful zoom. Include parity diagnostics or a difference artifact.

Forbidden shortcuts:
- Do not write crop/radial math in TypeScript.
- Do not pass raw project/document state to WGSL or the GPU service.
- Do not replace radial with centered full-source synthesis.
- Do not use nearest filtering merely to get exact tests if production requires bilinear.
- Do not upload/decode the source per region or per warm request.
- Do not dispatch/read back once per region.
- Do not silently use CPU when GPU fails.
- Do not start atlas tiling, binary IPC, material maps, or export hardening here.

Acceptance gate:
- PASS: production compile_persisted Stage 14 Base Color uses the GPU executor on a supported adapter.
- PASS: direct/loop/radial parity meets the predeclared exact/tolerance policy.
- PASS: source uploads and pipelines are reused across warm requests.
- PASS: distinct region identities/crops/destinations remain truthful.
- PASS: real 8K warm Base Color target is 1-3 seconds and cold target is 2-6 seconds on the documented qualification
  GPU, or the report explicitly fails the gate with pass-level evidence. Do not reduce output to pass.
- PASS: no silent CPU fallback or new orchestration spine exists.

Focused verification command:
cargo test -p hot-trimmer-sheet-compiler gpu_stage_14_base_color

Run exactly that focused verification command. If it fails, make at most one correction pass and rerun the same
command. Run the separate real-8K release qualification and native visual comparison. Stop after the Prompt 2 report.
Do not begin Prompt 3.
```

---

## Prompt 3 - GPU tiled atlas and exact interactive preview

```text
Implement GPU Rendering Migration Prompt 3 exactly as specified in
docs/gpu-rendering-migration-execution-prompt-pack.md. Move Base Color atlas ownership/padding to GPU tiles, add exact
1:1 viewport/selected-region rendering, and remove PNG/Base64 from the interactive preview hot path.

Read first, completely:
1. AGENTS.md.
2. docs/gpu-rendering-migration-plan.md.
3. docs/gpu-rendering-migration-execution-prompt-pack.md.
4. Accepted Prompt 1-2 reports, telemetry, CompiledAtlasPlan, GPU service, WGSL kernels, and cache identities.
5. intermediate_atlas.rs including correspondence, validity, RegionId ownership, overlap, allocation, and dilation.
6. SourceFrame/manual-layout destination and padding contracts.
7. Native Stage 14 command, artifact conversion, PNG/Base64 helpers, IPC contract generation, and frontend preview
   request/controller/display code.
8. Tauri raw byte response capabilities and current frontend binary response handling.

Before editing:
- Trace every current post-GPU copy from output texture to screen: GPU readback, CPU allocation/clone, PNG encode,
  Base64 encode, JSON/data URL, browser decode, image upload, paint.
- Record which atlas buffers are required for product behavior versus tests/diagnostics. In particular classify dense
  correspondence, validity, UUID ownership, and per-region rendered buffers.
- Define the tile coordinate convention, output origin, pixel-center convention, mip convention, halo rectangle, valid
  interior, and edge clipping in one versioned contract.
- Query adapter limits and choose tile dimensions from maximum texture size, storage format support, staging alignment,
  and configured memory budget. Normal default candidates are 1024/2048; do not assume either universally.
- Define how the UI expresses a viewport or selected-region exact-output request without calling stages itself.

Implement A - tile/ROI request contract and scheduler:
- Extend CompiledAtlasPlan with explicit request kinds: full draft sheet, progressive refinement sheet, viewport/ROI at
  exact output resolution, selected region at exact output resolution, and authoritative full extent.
- Every tile has map, mip/scale, output rectangle, halo rectangle, valid interior, generation, and deterministic cache
  identity.
- Map viewport coordinates to output pixel tiles in the native request/controller layer. The UI supplies view intent;
  compile_persisted still validates and creates the pixel plan.
- Prioritize visible/selected tiles, then neighboring tiles, then background refinement. Bound in-flight GPU work and
  readbacks.
- Coalesce pointer-drag requests. Cancel/obsolete older generations before publication while allowing immutable cache
  entries with exact identities to remain reusable.
- A 1:1 region request renders exact final-output texels, not a crop enlarged from the 512/1024 complete sheet.

Implement B - GPU atlas, ownership, padding, and validity:
- Move destination writes and overlap/ownership validation into deterministic GPU passes or prevalidated disjoint
  dispatches from the compiled plan.
- Generate compact Region ID as u32 indices with one artifact-level stable RegionId lookup table.
- Generate only the validity/mask information required by later passes and publication. Remove mandatory per-pixel UUID
  ownership.
- Make dense source correspondence an explicit debug map or tile-scoped diagnostic request. It is not allocated for
  ordinary preview.
- Implement the existing padding/dilation policy on GPU. Padding may write only inside authorized padding/allocation
  bounds and must not bleed across another region.
- Declare sampling/padding halos and publish only the valid interior. Adjacent tiles must agree exactly within the
  Prompt 2 tolerance.
- Cache completed GPU tiles by exact plan/map/mip/tile/halo/profile/shader identity.

Implement C - compiled artifact and binary transport:
- Version the CompiledPreviewArtifact to contain every field under “Shared target contracts / Compiled preview
  artifact.”
- Keep lightweight metadata/diagnostics as JSON.
- Add a raw byte command/endpoint or scoped local resource protocol for tile pixel payloads. The payload specifies
  pixel format, dimensions, row stride, and generation out of band; it is not Base64 and not JSON integer arrays.
- Remove full-atlas PNG encoding, PNG clone, Base64 expansion, data URL creation, and browser image decode from the
  interactive preview path. Leave export encoders untouched.
- Make tile handles scoped and revocable. Release native tile resources and frontend Blob/ImageData resources when
  evicted or superseded.
- Prevent arbitrary filesystem/resource access. A resource handle resolves only tiles owned by the application cache.

Implement D - frontend consumer only:
- Update the preview controller to request/receive artifact metadata and raw tile bytes, then blit/upload them to the
  existing display surface.
- The frontend may use ImageData/canvas/WebGL only as a byte-display mechanism already consistent with the app. It may
  not calculate sampling coordinates, radial transforms, masks, padding, effects, or atlas placement.
- Compose visible tiles at their artifact-provided output rectangles and generations. Do not infer placement by array
  index or response order.
- Retain draft tiles until replacement refinement tiles are painted; no black flash or sheet disappearance.
- Revoke obsolete frontend resources and ignore stale generations.
- Report request-to-first-byte, bytes-to-upload, upload-to-paint, and total interaction timing back into preview
  telemetry.

Implement E - interaction behavior:
- Crop/radial edits request only affected visible/selected tiles during drag and coalesce rapidly changing inputs.
- Topology changes invalidate only tiles whose destination/ownership or dependent padding changed; selection does not
  recompile pixels.
- Pan/zoom requests reuse cached tiles. A map switch to an already cached map does not rerun Stage 14.
- The complete 512 draft may arrive first, but exact 1:1 selected/viewport tiles must be requestable immediately and
  replace the relevant display area progressively.
- Cancellation works before dispatch, between tile batches, before readback, before byte publication, and before
  frontend paint.

Tests and evidence:
- Add tile-contract tests for corners, partial edge tiles, non-square output, halo clipping, mip coordinates, and
  selected-region bounds.
- Add GPU goldens comparing monolithic <=8K output to recomposed tiles at every shared boundary.
- Add padding tests proving no black/transparent seam and no cross-region bleed.
- Add Region ID tests proving every compact index resolves to the exact stable RegionId.
- Add stale-generation tests where an older GPU tile completes after a newer edit and is not displayed.
- Add controller tests for coalescing, tile priority, cached pan, resource release, and draft-to-refinement replacement.
- Assert interactive code does not call the PNG/Base64 helpers and IPC payload size matches raw tile bytes plus bounded
  metadata rather than Base64 expansion.
- Native evidence: display a 512 full-sheet navigation view, select a radial region, request exact 1:1 pixels, edit a
  radial parameter, pan, and zoom without sheet disappearance. Capture telemetry and screenshots.

Performance qualification:
- Warm selected-region 1:1 refresh for 1024-2048 output texels: 50-250 ms.
- Warm request for a cached pan tile: <=100 ms to paint.
- Cached map/tile switch: <=50 ms when that map exists.
- Cold 512 complete-sheet first paint: <=1 second.
- Warm 1024 complete-sheet refinement: <=500 ms.
- Report GPU dispatch separately from readback, raw IPC, frontend upload, and paint.

Forbidden shortcuts:
- Do not keep Base64 for “temporary” preview while declaring Prompt 3 complete.
- Do not send raw bytes as JSON number arrays.
- Do not re-encode raw bytes into PNG in TypeScript.
- Do not render the full 8K atlas for every selected-region edit.
- Do not create a frontend sampling renderer.
- Do not infer tile/region correspondence by zipping response arrays.
- Do not delete draft pixels before refinement paint succeeds.
- Do not begin material-map generation or final export tiling here.

Acceptance gate:
- PASS: authoritative Base Color atlas writes, padding, validity, and compact Region ID operate through GPU tiles.
- PASS: selected-region exact 1:1 inspection uses authoritative Stage 14 without a full-atlas compile.
- PASS: interactive pixels contain no PNG/Base64/JSON-array hot-path conversion.
- PASS: tiled and monolithic common texels meet parity and have no seams/bleed.
- PASS: stale tiles cannot overwrite a newer edit and draft pixels remain visible until replacement.
- PASS: all interaction performance targets are measured on the qualification machine or explicitly fail with
  component timings.

Focused verification command:
npm.cmd run test --workspace @hot-trimmer/desktop -- gpu-tiled-preview

Run exactly that focused verification command. If it fails, make at most one correction pass and rerun the same
command. Complete the native 512 + exact-1:1 walkthrough and attach telemetry. Stop after the Prompt 3 report. Do not
begin Prompt 4.
```

---

## Prompt 4 - Requested GPU material-map graph

```text
Implement GPU Rendering Migration Prompt 4 exactly as specified in
docs/gpu-rendering-migration-execution-prompt-pack.md. Extend the accepted authoritative GPU sampling/tile pipeline to
produce and compose the existing requested material maps. Do not invent material algorithms or revive a second
compositor.

Read first, completely:
1. AGENTS.md.
2. docs/gpu-rendering-migration-plan.md.
3. docs/gpu-rendering-migration-execution-prompt-pack.md.
4. Accepted Prompt 1-3 reports, shader/plan/cache versions, tile/halo policy, binary artifact, and telemetry.
5. Every existing authoritative/dead implementation that claims slot/hotspot mask, distance-to-edge, profile height,
   material height, normal, roughness, AO, Metallic, Region ID, padding, or weathering behavior.
6. Effect-plan compiler, stable effect IDs/seeds/scales, edge-eligibility/continuity contracts, normal convention, and
   source-map registration contracts.
7. Preview map tabs/request logic and export channel contracts, but do not implement final export in this prompt.

Before editing:
- Produce a short code-backed inventory for each requested map: current producer, exact inputs, coordinate space,
  working range/format, downstream consumer, whether it is production-connected, and whether another stage overwrites
  it.
- Identify reusable pure math in old/dead compositors. Extract math only; never call their top-level orchestration.
- Define the authoritative pass dependency graph and cache identity for every node before implementing shaders.
- Define working and publication formats, color spaces, normal convention, quantization, and tolerances per map.
- Define halo requirements per pass. Central-difference normals, profiles, AO, and effects may require different halos.
- If an algorithm does not actually exist or lacks a valid compiled plan, mark it unavailable with a typed diagnostic.
  Do not fabricate it to make a map tab light up.

Implement A - requested-map graph:
- Extend CompiledAtlasPlan with a requested map bitset/set and exact existing structural/effect plan identities.
- Build a deterministic dependency graph. Requesting Normal may require composed Height; requesting Base Color alone
  must not run Height, Normal, Roughness, AO, Metallic, or effects.
- Deduplicate shared prerequisites within one plan and across cache hits.
- Version/hash each pass by algorithm/shader version, exact upstream identities, output format, tile/halo, scale,
  convention, and deterministic seed where applicable.
- Telemetry records requested, executed, skipped, cache-hit, and read-back passes.

Implement B - formats and registered source sampling:
- Base Color remains explicitly sRGB/linear-correct.
- Use single-channel float working textures for Height/profile math where qualified; do not collapse working height into
  RGBA8.
- Use float intermediates for normal derivation/composition and pack only at the artifact/export boundary.
- Use single-channel formats for Roughness, AO, Metallic, and masks unless an established export contract packs them.
- Use u32 compact indices for Region ID.
- All supplied source maps use the exact Base Color crop, transform, loop/radial mapping, orientation, and destination
  identity unless the persisted channel contract explicitly defines a registered resolution transform.
- Missing optional source maps use the existing documented material fallback, not arbitrary constants introduced here.

Implement C - structural profile and height:
- Generate the hotspot-local mask from exact region/hotspot bounds, not allocation bounds or full atlas bounds.
- Compute distance/profile coordinates in hotspot-local output pixels with physical/pixel scale from the compiled plan.
- Respect edge eligibility from continuity: explicit horizontal looping suppresses left/right structural seam edges;
  vertical looping suppresses top/bottom; both suppress both pairs; radial behavior follows its authored seam/edge
  contract.
- Port the existing profile function and parameters. Profiles generated but not composed are a failure.
- Combine material height and structural height in the documented working range/order without unintended clamp or
  quantization.
- Tile halos must make distance/profile output continuous across tile boundaries while preventing writes outside the
  hotspot plus authorized halo.

Implement D - normal, roughness, AO, Metallic, Region ID:
- Derive or compose Normal from the final composed Height according to existing product rules.
- Implement OpenGL (+Y) and DirectX (-Y) convention explicitly at the final convention boundary. Do not flip twice.
- Port existing Roughness and AO composition from authoritative inputs. Do not generate visually plausible noise as a
  replacement.
- Pass through/compose Metallic only when requested and supported by current material contracts.
- Region ID comes from Prompt 3 compact ownership and resolves losslessly through the stable table.
- Later passes may not overwrite an earlier completed channel accidentally. Each output has one authoritative final
  writer.

Implement E - one existing effect route only if ready:
- If the existing authoritative compiler already produces a stable effect plan, port one edge/chip/weathering effect
  end to end as a proof. Otherwise publish a typed NotYetCompiled diagnostic and leave effects out of Prompt 4
  acceptance rather than inventing a plan.
- A ported effect must use stable effect-plan ID, hotspot-local coordinates, physical/pixel scale, deterministic seed,
  edge eligibility, and declared halo.
- It may contribute only to channels defined by the existing effect contract and must never write outside the hotspot.
- Do not broaden this into the entire effect library.

Implement F - preview and cache behavior:
- The map UI requests one map through compile_persisted/artifact metadata; it never runs channel stages itself.
- Cache source samples, masks, structural height, composed height, and final map tiles by exact dependency identity.
- Switching to a cached map changes display only. Changing Base Color crop invalidates all dependent registered maps for
  affected tiles. Changing normal convention invalidates packed Normal, not unrelated Base Color.
- Keep GPU intermediates resident within budget and read back only visible/requested preview tiles.
- Publish map availability and typed failure per map. Do not show a stale prior map when current generation failed.

Tests and evidence:
- Add a compact deterministic fixture containing direct, horizontal loop, vertical loop, loop XY, and radial regions.
- Golden Base Color, Height, Normal OpenGL, Normal DirectX, Roughness, AO, Metallic if supported, and Region ID.
- Prove mask/profile coordinates use hotspot bounds rather than allocation bounds.
- Prove structural profile is visible in Height, final Normal derives from composed Height, and no later pass overwrites
  it.
- Prove looped edge suppression and no writes outside hotspot/authorized halo.
- Prove cross-channel registration at selected source/output landmarks.
- Prove height range survives working-format conversion and normal orientation is correct.
- Prove requesting Base Color executes no other map kernels; requesting Normal executes exactly its dependency graph;
  cached map switch performs no new GPU work.
- Recompose tiles and compare every halo boundary.
- Native walkthrough: inspect each available map at a planar, looped, and radial region, including 1:1 Height/Normal.

Performance qualification:
- Cached map switch <=50 ms.
- Warm requested 8K map set target 2-8 seconds on the qualification GPU, with the exact requested channels recorded.
- Report per-pass GPU time, cache hits, readback bytes, raw IPC bytes, and paint.
- A single-map request must show lower work/bytes than the full requested set.

Forbidden shortcuts:
- Do not reactivate an old compositor as a second orchestrator.
- Do not add a new material algorithm to fill a missing contract.
- Do not generate all maps on every edit.
- Do not derive Normal from pre-structural Height.
- Do not apply profiles/effects to allocation bounds.
- Do not hide missing maps behind flat placeholders while reporting success.
- Do not pack every scalar intermediate into RGBA8.
- Do not begin final 16K/24K export hardening here.

Acceptance gate:
- PASS: Base Color, Height, Normal, Roughness, AO, Region ID, and Metallic when supported are compiled through the same
  plan/GPU tile route and share stable identities.
- PASS: structural profiles are visibly composed and normals come from final height.
- PASS: registered transforms, continuity edges, formats, ranges, and normal conventions pass goldens.
- PASS: only requested/dependent passes execute and cached map switch meets target.
- PASS: no old compositor or CPU material raster loop remains in the production path for migrated maps.
- PASS: real 8K requested-map qualification is reported without downscaling or omitted channels.

Focused verification command:
cargo test -p hot-trimmer-sheet-compiler gpu_material_map_pipeline

Run exactly that focused verification command. If it fails, make at most one correction pass and rerun the same
command. Complete the native all-map walkthrough and real-8K qualification. Stop after the Prompt 4 report. Do not
begin Prompt 5.
```

---

## Prompt 5 - Bounded 16K/24K preview, export, and production hardening

```text
Implement GPU Rendering Migration Prompt 5 exactly as specified in
docs/gpu-rendering-migration-execution-prompt-pack.md. Qualify the authoritative GPU pipeline for bounded-memory real
8K/16K/24K multi-source preview and export, implement source/output tiling and streaming encoding, handle failure and
device loss, and retire the CPU production executor.

Read first, completely:
1. AGENTS.md.
2. docs/gpu-rendering-migration-plan.md.
3. docs/gpu-rendering-migration-execution-prompt-pack.md.
4. Accepted Prompt 1-4 reports, all real-resolution traces, tile/halo/format contracts, GPU caches, and known limits.
5. crates/export, image encoders, output metadata/mip contracts, atomic file-write behavior, and Blender/application
   consumers.
6. Multi-source/patch persistence and source decode/cache ownership.
7. GPU error handling, application lifecycle, cache eviction, cancellation, and native progress/diagnostic UI.

Before editing:
- Inventory which source and output dimensions exceed the qualification adapters' maximum texture size.
- Calculate worst-case bytes for each map/intermediate/source/staging allocation at 8K, 16K, and 24K.
- Define explicit default CPU RAM, GPU residency, decoded-source, output-tile, and staging-pool budgets. Include integrated
  GPU/shared-memory considerations.
- Define operation-specific halos for every Prompt 4 pass and the deterministic valid-interior stitching rule.
- Inspect each export encoder. Determine whether it supports scanline/tile streaming. If not, choose a bounded solution
  such as a streaming encoder or temporary disk-backed spool. Do not quietly rebuild a monolithic CPU image.
- Define the supported adapter floor and explicit unsupported-device user experience before removing CPU production
  fallback.

Implement A - source tiling/virtualization:
- Sources larger than adapter limits or source-cache budget must be decoded/accessed as tiles with filter halos. Do not
  downscale authoritative input.
- Source tile identity includes source digest, oriented dimensions, decoder/color version, map role, mip/level, tile
  rectangle, and halo.
- For each output tile, compute the exact source footprint needed by direct, looped, and radial mapping. Radial/warp
  footprints may be non-axis-trivial; conservatively request all necessary source tiles without substituting the full
  source.
- Bilinear samples across source tile boundaries must match monolithic <=limit reference within Prompt 2 tolerance.
- Bound decoded CPU tiles and GPU source tiles separately with visible hit/miss/eviction/upload telemetry.
- Five to ten sources and at least twenty patch bindings must coexist without changing the primary source or losing
  stable assignment identities.

Implement B - bounded output scheduler:
- Make 8K/16K/24K output use the same tile scheduler. Even when an adapter supports a large monolithic texture, choose
  tiling when the configured memory budget requires it.
- Derive tile size, halo, in-flight tile count, resident source tiles, intermediate textures, final map tiles, and
  staging buffers from real limits and budgets.
- Schedule dependencies so only the maps/intermediates required for the current tile batch are resident.
- Prefer preview-visible tiles; export uses deterministic map/tile order. Avoid starving cancellation/progress.
- Reuse immutable cached source/intermediate/output tiles where exact plan identities match.
- Never hold all ten 24K maps in GPU or CPU memory simultaneously.
- Report estimated and observed peak allocation/residency by category.

Implement C - seamless tiles, mips, and padding:
- Apply declared halos for filtering, padding, profile distance, normal derivatives, AO, effects, and mip generation.
- Encode/publish only each tile's valid interior.
- Compare output tile edges, corners, radial seams, loop seams, padding boundaries, profile/normal gradients, AO/effect
  neighborhoods, and mip transitions against a monolithic <=limit reference.
- Mip generation and export metadata use one documented convention and do not introduce cross-tile color-space or
  normal renormalization errors.

Implement D - streaming export:
- Extend the existing export route; do not create a second exporter.
- compile_persisted produces the authoritative export plan and requested map set. The GPU service renders deterministic
  output tiles, then a bounded staging pool reads them back.
- Stream scanlines/tiles to the existing format encoder or a deliberately selected bounded encoder. If format APIs
  require ordering, schedule/spool tiles without holding the whole frame in RAM.
- Write to a temporary sibling output and atomically finalize only after every requested map succeeds. On cancellation,
  encoding error, disk-full, or device failure, remove/retain temporary artifacts according to an explicit recoverable
  policy and never replace a prior valid export with a partial file.
- Preserve bit depth, color space, alpha, normal convention, channel naming, output metadata, mips, and stable project
  identities.
- Export pixels never pass through Base64 or JSON.
- Progress reports map, mip, completed/total tiles, GPU/render/readback/encode time, bytes written, and estimated time.

Implement E - cancellation, eviction, and device loss:
- Check cancellation/current revision before source decode/upload, each GPU batch, readback, encode block, and final
  publication/file replace.
- A cancelled/stale job cannot publish preview tiles or finalize export.
- Make cache eviction safe while in-flight resources are referenced. Account for pinned versus evictable bytes.
- Install error scopes/uncaptured-error diagnostics and classify validation, out-of-memory, device-lost, unsupported
  format/feature, readback, encoder, and filesystem failures.
- On device loss, cancel in-flight GPU work, recreate service-owned device/pipelines, rebuild GPU caches lazily from
  immutable CPU plan/source identities, and either resume from valid completed export tiles or fail cleanly according
  to the documented policy.
- Never display an old cached texture as the result of a failed current generation.

Implement F - retire CPU production rendering:
- Remove CPU executor selection from production preview/export after GPU parity and qualification pass.
- Retain the smallest useful CPU implementation/fixtures as a clearly test-only reference oracle.
- Unsupported hardware receives a clear typed diagnostic and supported-device requirement/remediation. It does not
  silently start a multi-minute CPU job.
- Remove now-dead production full-frame position/correspondence/UUID/atlas buffers and PNG/Base64 preview helpers only
  when no live call references them. Preserve test helpers if explicitly test-only.
- Prove by tracing/counters that no production migrated-map call reaches CPU slot synthesis or CPU atlas composition.

Implement G - qualification matrix and product evidence:
- Add a dedicated ignored release qualification harness using actual pixels, not filename claims.
- Workloads:
  1. Real source >=7952x4016, 8192 output, Base Color only.
  2. Same project, 8192 output, all supported requested maps.
  3. At least five real/generative full-resolution sources and twenty patches, 16384 output, representative maps.
  4. Same multi-source project, 24576 output, representative maps or the documented full product export set.
  5. Repeated warm crop/radial edit and cached map switch.
- Run on at least one discrete GPU and one integrated GPU when hardware is available. If only one class is available,
  report the missing qualification explicitly rather than inventing results.
- Record OS, CPU/RAM, GPU/VRAM/shared memory, backend/driver, adapter limits, build, source/output dimensions, maps,
  regions/patches, tile/halo/in-flight policy, cold/warm phase timings, first-tile time, total time, peak RSS, measured/
  estimated GPU residency, cache behavior, upload/readback/write bytes, output hashes/tolerances, and failures.
- Compare directly to Prompt 1 CPU baseline and Prompt 2/3/4 intermediate traces.

Tests and evidence:
- Source-tile parity across direct, loop, and radial footprints including boundary bilinear samples.
- Output-tile seam goldens for every supported map and mip.
- Multi-source save/reopen proof: exact source/patch/RegionId/plan identities and equivalent output.
- Budget test proving eviction rather than unbounded growth.
- Cancellation during upload, dispatch, readback, encoding, and pre-finalize.
- Stale revision completion, disk-full/write failure, simulated device loss, and unsupported-adapter diagnostic.
- Atomic export test proving an existing valid file is never replaced by partial output.
- Production-route assertion proving the CPU executor/rasterizer is not called.
- Native walkthrough: 24K project shows a progressive first exact tile, remains interactive, exports with progress, and
  can cancel without stale/partial publication.

Performance and memory gates:
- 16K/24K preview first visible exact tile <=500 ms on the qualification discrete GPU.
- Warm 8K targets from Prompts 2-4 remain green after tiling/hardening.
- Memory remains inside explicit budgets; no full-frame 16K/24K all-map CPU or GPU allocation exists.
- Export performance is reported honestly per hardware. Do not impose a fake universal total-time number before the
  real qualification matrix; first-pixel latency and bounded memory are hard gates.

Forbidden shortcuts:
- Do not use a 24K monolithic texture because one test GPU happens to allow it.
- Do not decode every large source fully for every output tile.
- Do not read back or encode all maps at once.
- Do not hide a monolithic CPU buffer behind a “streaming” API.
- Do not retain a silent CPU production fallback.
- Do not weaken seams/tolerances to make tiling pass.
- Do not delete the prior valid export before the replacement succeeds.
- Do not declare integrated/discrete qualification that was not actually run.

Acceptance gate:
- PASS: actual 8K/16K/24K workloads use bounded source/output tiling and never require monolithic oversized resources.
- PASS: multi-source/multi-patch identity and pixels survive save/reopen.
- PASS: all supported maps/mips have seam-free tile goldens.
- PASS: preview first-tile, warm 8K, memory, cancellation, stale revision, failure, and atomic export gates pass.
- PASS: production preview/export use compile_persisted plus the single GPU executor; CPU rendering is test-only.
- PASS: qualification artifacts compare against Prompt 1 and disclose hardware/limits/failures.

Focused verification command:
cargo test -p hot-trimmer-export gpu_tiled_export

Run exactly that focused verification command. If it fails, make at most one correction pass and rerun the same
command. Run the ignored release qualification matrix and native 24K walkthrough separately. Stop with the complete
Prompt 5 report and explicit final acceptance checklist.
```

## Pack completion gate

The five-prompt pack is complete only when the Prompt 5 report proves all of the following in the real product:

- One document/compiler spine: `TrimSheetDocument -> compile_persisted -> CompiledAtlasPlan`.
- One production native GPU executor, shared by preview and export.
- Truthful direct, explicit loop, planar, and radial sampling from exact persisted bindings.
- Exact 1:1 selected-region/viewport inspection without full-atlas waiting.
- GPU atlas ownership, padding, profiles, material maps, and compact Region ID.
- Requested-map-only execution and reusable GPU-resident caches.
- Raw binary tile publication with no interactive PNG/Base64 pipeline.
- Source and output tiling for real 16K/24K workloads.
- Bounded RAM/VRAM and streaming export.
- Typed cancellation, stale revision, unsupported adapter, device loss, and filesystem failure behavior.
- Real release measurements with actual dimensions on documented hardware.
- No silent CPU production fallback, preview approximation, second renderer, or second orchestration spine.

