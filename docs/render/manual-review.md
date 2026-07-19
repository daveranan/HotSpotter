# GPU render migration manual review

Use this guide after each render prompt. It separates what can be judged in the app from implementation evidence that
must come from the prompt report and focused test. Do not reject a prompt for features assigned to a later prompt, and
do not accept an architectural prompt merely because the existing preview still opens.

## Keep one review project

Use the same saved project after every prompt so comparisons remain meaningful. The preferred fixture contains:

- One 7952 x 4016 or comparable 8K source.
- Several ordinary direct-crop regions.
- One LoopX, one LoopY, and one LoopXY region.
- One radial region with an obvious center and seam.
- Transparent pixels or a hard crop boundary if available.
- A region near every atlas edge.

Save screenshots and telemetry from the same views:

1. Complete sheet fitted in the viewport.
2. An ordinary crop at 100% or 1:1.
3. Every loop seam at 1:1.
4. Radial center and radial seam at 1:1.
5. Atlas edge/corner at 1:1.

Do not use a 512 preview to judge pixel quality. It is only a navigation image. Judge exact sampling using a 1:1 tile
after Prompt 003, or the full-resolution Prompt 002 result before tiled inspection exists.

## Evidence rule

There are two kinds of acceptance evidence:

- **Manual evidence:** pixels, responsiveness, stale-frame behavior, progress, errors, and output files you can observe.
- **Implementation evidence:** focused test result, runtime route, counters, cache telemetry, memory telemetry, and
  benchmark phase timings.

Require both kinds when both are listed. A screenshot cannot prove that CPU composition stopped. A test cannot prove
that the preview is comfortable to inspect.

## Prompt 001.5 — executor owns CPU composition

### What to do

1. Open the standard project.
2. Compile Base Color at the same resolution used before Prompt 001.5.
3. Compare the complete sheet and the saved direct/loop/radial inspection points with the previous build.
4. Cancel one compile if the UI exposes cancellation, then compile normally again.

### What you should see

- The same Base Color pixels, crops, loops, radial mapping, dimensions, and region identity as before.
- The preview continues to publish normally after a successful compile.
- Cancellation or a superseded request does not replace the current valid result.

### What you should **not** expect yet

- Any meaningful speedup.
- GPU utilization from atlas rendering.
- Better 512 quality.
- Progressive tiles, faster panning, binary preview transport, or material maps.
- Any visible UI redesign.

This is a contract move. A visibly different result is a regression, not progress.

### Required report evidence

- `gpu_executor_owns_base_color_composition` passes.
- Runtime route is `compile_persisted -> CompiledAtlasPlanV1 -> CpuAtlasRenderExecutor -> final composition`.
- Telemetry contains `executor=cpu`, the exact plan hash, `compose_executor=cpu`, and `compose_ms`.
- `compile_source_frame` no longer calls `compile_intermediate_atlas` directly.

### Reject Prompt 001.5 if

- Any pixel, crop, loop, radial, alpha, dimension, or identity changes.
- The report claims a performance win as its acceptance result.
- Composition remains a direct call outside the executor.
- Cancellation can publish an obsolete result.

## Prompt 002 — GPU Base Color sampling and composition

### What to do

1. Start the app on a supported GPU and compile the standard 8K project cold.
2. Compile the unchanged project twice more.
3. At full resolution, inspect ordinary crops, every loop seam, the radial center/seam, transparency, and atlas edges.
4. Change one crop and compile again.
5. Change the radial center or radius and compile again.
6. If practical, compare the output against the saved Prompt 001.5 CPU reference or its hashes/tolerance report.

### What you should see

- The same authoritative Base Color result as Prompt 001.5.
- Crop edits affect only the intended source mapping.
- LoopX, LoopY, and LoopXY remain continuous on their authored axes.
- Radial center, radius, falloff, blend, and seam remain correct.
- Distinct sources/regions still write to distinct destinations.
- A substantial 8K improvement: target cold 2–6 seconds and warm 1–3 seconds on the qualification GPU.
- Warm unchanged compiles upload zero source bytes.

### What you should **not** expect yet

- Fast exact viewport tiles or selected-region 1:1 requests.
- Smooth tile-prioritized panning.
- Removal of PNG/Base64 preview publication.
- Height, Normal, Roughness, AO, or Metallic output.
- Bounded 16K/24K export.

Prompt 002 may still read back one complete Base Color atlas through the existing publication boundary. It proves GPU
pixel generation; Prompt 003 fixes interactive publication.

### Required report evidence

- `gpu_stage_14_base_color` passes.
- Runtime route uses the long-lived production GPU executor; CPU synthesis and CPU composition counters are zero.
- Telemetry separates upload, dispatch, readback, composition, and total compiler time.
- Report includes command count/bytes, source and pipeline cache hits, upload bytes, and real 8K cold/warm timings.
- The production route does not silently fall back to CPU when GPU execution fails.

### Reject Prompt 002 if

- It is merely faster but changes sampling, crop, loop, radial, alpha, or color behavior.
- The GPU returns one rendered buffer per region for CPU recomposition.
- It uploads full-frame coordinate, seam, correspondence, or UUID buffers.
- Every warm compile reuploads unchanged sources.
- The reported benchmark uses 512/1024, reduced quality, synthetic pixels, or debug builds instead of the real 8K
  workload.
- Unsupported GPU work silently falls back to the old CPU renderer.

## Prompt 003 — tiled exact preview and binary publication

### What to do

1. Open the standard project and time the first complete 512 navigation image.
2. Wait for the complete 1024 refinement; confirm the draft stays visible until refinement is painted.
3. Select the radial region and request/view it at exact 1:1.
4. Pan across a tile boundary and revisit the same area.
5. Edit the radial region or an ordinary crop while a prior request is still running.
6. Rapidly drag several times, stop, and verify only the newest state becomes visible.
7. Inspect padding and atlas edges against a dark/checker background.

### What you should see

- Complete 512 first paint in no more than about 1 second.
- Warm complete 1024 refinement in no more than about 500 ms.
- Warm selected-region exact 1:1 refresh in roughly 50–250 ms.
- Cached/neighbor pan tiles paint within about 100 ms.
- Exact 1:1 pixels—not an enlarged 512 image—when inspecting the selected region.
- No black quadrants, transparent gaps, tile seams, cross-region padding bleed, or whole-sheet disappearance.
- Old generations never flash over newer edits.
- Revisiting cached tiles avoids new GPU work.

### What you should **not** expect yet

- New Height/Normal/Roughness/AO material quality.
- New profile, weathering, or effect algorithms.
- Final 16K/24K export behavior.
- Layout or source-authoring UI changes unrelated to preview intent.

### Required report evidence

- `gpu-tiled-preview` passes plus one native 512 -> radial 1:1 -> edit -> pan walkthrough.
- Interactive pixels use raw binary transport, not PNG, Base64, or JSON pixel arrays.
- Region ownership is compact `u32 -> stable RegionId`; correspondence is absent unless explicitly requested.
- Telemetry joins native and frontend work by generation and separates dispatch, readback, IPC, upload, and paint.
- Tile cache and staging/readback pools are bounded.

### Reject Prompt 003 if

- “1:1” is a scaled low-resolution preview.
- The draft disappears while refinement runs.
- A stale crop/radial result can flash after a newer edit.
- Tile boundaries show seams, padding crosses region ownership, or edge tiles become black/transparent.
- Preview pixels still require PNG/Base64 encoding.
- The frontend independently runs sampling or material algorithms.

## Prompt 004 — requested GPU material maps

### What to do

1. Inspect Base Color first and confirm it still matches Prompt 003.
2. Switch individually to Height, Normal, Roughness, AO, Metallic when supported, and Region ID.
3. Inspect one ordinary region, every loop type, and the radial region at 1:1 for every relevant map.
4. Toggle OpenGL and DirectX Normal conventions if the product exposes both.
5. Return to a previously viewed cached map.

### What you should see

- Structural profile is actually visible in final Height.
- Normal is derived from the composed Height and flips the Y convention exactly once.
- LoopX has no artificial left/right structural edge; LoopY has no top/bottom edge; LoopXY suppresses both pairs.
- Roughness and AO follow the same crop/radial transform as Base Color with aligned landmarks.
- Region ID selects/resolves the correct stable region everywhere.
- No map writes outside the hotspot and its authorized halo.
- Cached map switching is about 50 ms or faster and performs no new pixel work.

### What you should **not** expect yet

- A newly invented effects library.
- Placeholder noise presented as weathering.
- Unsupported Metallic fabricated as a complete map.
- 16K/24K export qualification.
- Every map to run when only Base Color is requested.

### Required report evidence

- `gpu_material_map_pipeline` passes and native inspection covers all available maps.
- Telemetry lists requested, executed, cache-hit, and skipped passes.
- Base Color requests execute no material passes; Normal executes only its real Height prerequisites.
- Warm requested 8K map set is targeted at 2–8 seconds, with the exact requested maps listed.
- Missing authoritative material/effect math is reported as typed unavailable, not guessed.

### Reject Prompt 004 if

- Height exists internally but is flat or absent from the published Height.
- Normal was generated from pre-composition Height or is overwritten later.
- Cross-channel landmarks shift because channels choose different crops.
- Loop seams receive bevel edges they explicitly suppress.
- Switching to one map renders every map.
- The implementation invents approximations where no authoritative contract exists.

## Prompt 005 — bounded 16K/24K projects and streaming export

### What to do

Run the qualification matrix with real pixels and release builds:

1. One >=7952 x 4016 source -> 8192 Base Color.
2. Same project -> 8192 all supported requested maps.
3. >=5 sources and >=20 patch bindings -> 16384 representative/full product map set.
4. Same multi-source project -> 24576 representative/full product map set.
5. Warm crop edit, radial edit, map switch, and cancellation.
6. Cancel one export midway and verify an existing valid output remains intact.
7. Reopen the multi-source project and compare source/patch/region identities and output.

### What you should see

- First exact 16K/24K preview tile within about 500 ms on the qualification discrete GPU.
- Progress advances by map/mip/tile and cancellation remains responsive.
- Memory rises to declared bounded budgets and then stabilizes/evicts instead of growing with total project size.
- Export writes complete maps with seamless tile, loop, radial, derivative, AO/effect, and mip boundaries.
- A failed/cancelled export never replaces the last valid export.
- Multi-source patches retain the correct source and region identity after save/reopen.

### What you should **not** expect

- All ten 24K maps resident simultaneously.
- One monolithic CPU artifact containing every output map.
- Export pixels travelling through Base64/JSON.
- Silent CPU fallback on unsupported hardware.
- New layout, source-assignment, or material algorithms.

### Required report evidence

- `gpu_tiled_export` passes.
- Report declares CPU decode, GPU source, GPU output/intermediate, staging, and in-flight tile budgets.
- Qualification records hardware/backend/driver, source/patch/region counts, maps, tile/halo/concurrency, timings, peak
  RSS, GPU residency, cache/upload/readback/write bytes, and output hashes/tolerances.
- Production CPU synthesis/composition counters remain zero.
- Disk-full, stale revision, cancellation, device loss, and unsupported-adapter behaviors are distinct and tested.

### Reject Prompt 005 if

- “Streaming” hides a full-frame/all-map allocation.
- 16K/24K succeeds only by downscaling, changing sampling modes, or reducing map quality.
- Memory grows without respecting the declared budgets.
- Cancellation waits for a giant batch or corrupts/replaces a valid output.
- Multi-source or patch identity changes after reopening.
- Qualification evidence omits the actual pixels, release build, hardware, or measured memory.

## What each accepted prompt means for the product

| Accepted through | Honest product statement |
| --- | --- |
| 001.5 | The renderer boundary is ready for replacement; product behavior and speed are intentionally unchanged. |
| 002 | Base Color pixels are generated correctly on the GPU at useful 8K compile speed. |
| 003 | Base Color can be navigated and inspected interactively at exact resolution. |
| 004 | Existing structural and material-map algorithms are visible through the same GPU pipeline. |
| 005 | The pipeline is bounded and qualified for large multi-source preview and export workloads. |

Prompt 002 is the first major runtime win. Prompt 003 is the first major day-to-day UX win. Prompt 004 is the first
material-output win. Prompt 005 is the large-project/export readiness gate.
