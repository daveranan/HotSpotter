# Render Prompt 002 - GPU Base Color sampling and direct atlas composition

Implement this prompt only after Prompt 001.5 passes. This file is self-contained. Do not read other documentation,
historical reports, or future prompts. Do not perform a general pipeline audit.

## Outcome

On a supported GPU, `compile_persisted` must submit `CompiledAtlasPlanV1` to one application-owned GPU executor that:

1. Uploads/caches source textures.
2. Uploads a compact source table and compact region-command buffer.
3. Computes direct, explicit loop, and radial source coordinates in WGSL.
4. Samples source pixels.
5. Writes pixels directly into final Base Color atlas destinations.
6. Reads back one completed Base Color atlas result for the existing artifact/publication boundary.

No GPU result may return to CPU as 64 rendered-region buffers for CPU recomposition.

## Current facts

- `compile_persisted` is the only production spine.
- `CompiledAtlasPlanV1` already owns exact ordered source and region commands.
- Prompt 001.5 makes `AtlasRenderExecutor` own region synthesis and final composition.
- `CpuAtlasRenderExecutor` remains the reference implementation.
- The application already owns one long-lived `wgpu` capability service in `crates/preview`.
- `wgpu` 26.0.1 is pinned. Do not research or change the GPU library version.
- The current frontend still accepts the existing PNG/data-URL artifact. Binary tiled publication belongs to Prompt 003.
- Qualification GPU facts: `Rgba8Unorm` supports sampling/storage; `Rgba8UnormSrgb` supports sampling but not storage;
  `R16Float`, `R32Float`, and `R32Uint` support sampling/storage; row alignment is 256 bytes.

## Edit boundary

Expected files:

- `crates/preview/src/lib.rs` plus small WGSL/module files under `crates/preview/src/`
- `crates/preview/Cargo.toml` only for directly required helper dependencies
- `crates/sheet-compiler/src/atlas_executor.rs`
- `crates/sheet-compiler/src/persisted_pipeline.rs`
- `crates/sheet-compiler/src/compiled_atlas_plan.rs` only if GPU packing requires an additive versioned field
- `apps/desktop/src-tauri/src/lib.rs`
- `apps/desktop/src-tauri/src/document_commands.rs`
- one focused GPU Base Color test file

Do not edit layout/document commands, source assignment, material algorithms, export, or frontend UI.

## Required GPU data model

Create a tightly packed, versioned GPU representation derived only from `CompiledAtlasPlanV1`.

Per-source data contains only what shaders need:

- GPU texture/cache index.
- Oriented width/height.
- channel/color policy.

Per-region command contains only fixed-size scalar data:

- Compact region index.
- Source texture index.
- Source crop in source pixels.
- Destination rectangle in output pixels.
- Sampling-mode discriminator.
- Mapping scale, offset, rotation, mirrors.
- Explicit repeat period/address data.
- Radial center, inner/outer radius, falloff, blend/seam values.
- Continuity/edge flags needed by Base Color.

Use integer/float packing with explicit alignment assertions. Stable UUIDs remain in the CPU plan/artifact lookup; do
not upload UUID strings or 16-byte UUID ownership per pixel.

## Critical prohibition

Do **not** create GPU equivalents of the old CPU per-pixel intermediates.

Do not allocate:

- Full-frame source-position texture/buffer.
- Full-frame seam-position texture/buffer.
- Full-frame seam-blend texture/buffer.
- Full-frame correspondence texture/buffer.
- Per-pixel UUID texture.
- One output texture/readback per region.
- CPU rendered-region buffers for GPU output.

The shader calculates source coordinates from the output invocation coordinate plus one compact region command. It
writes the final atlas pixel directly.

## Required implementation

1. Extend the existing application-owned GPU service. Keep one `Instance`, `Adapter`, `Device`, and `Queue` for the
   application lifetime. Cache shader modules, pipelines, bind-group layouts, samplers, and source textures.

2. Add a bounded source texture cache keyed by the exact plan source identity: digest, oriented dimensions, decoded
   format, decoder/color version, and channel role. A warm unchanged compile uploads zero source bytes.

3. Implement a production `GpuAtlasRenderExecutor` satisfying the complete executor contract established by 001.5.
   The desktop-owned service must be passed/injected into the existing compile route; do not create global project
   state or construct a device per request.

4. Port exact CPU semantics from the named authoritative function
   `slot_synthesis.rs::synthesize_slot_material_with_guard`:

   - Direct crop.
   - Explicit LoopX, LoopY, and LoopXY using the authored crop/period.
   - Planar mapping transform.
   - Polar/radial mapping with exact center/radii/falloff/blend/seam behavior.
   - Existing pixel-center, orientation, bilinear filtering, alpha, validity, clamp, and out-of-domain behavior.

   Read only that function and directly called coordinate helpers. Do not inspect unrelated stages.

5. Unsupported binding/mode pairs fail before dispatch with the existing typed plan/executor diagnostic. Never fall
   back to stretch, centered crop, whole-source sampling, or CPU rendering.

6. Write sampled pixels directly to exact compiled destination rectangles in one Base Color atlas output. Batch by
   source/pipeline as adapter binding limits require; do not submit/read back once per region.

7. Because sRGB storage is unavailable on the qualification adapter, use explicit correct color handling with a
   storage-capable output format. Document and test the exact encode/view boundary; do not double-encode sRGB.

8. Preserve cancellation/current generation checks before uploads, batches, readback, and artifact publication. A
   stale GPU result cannot enter the current artifact/cache.

9. Keep CPU execution only as a test/developer parity oracle. Supported-GPU production must not silently fall back.
   An unavailable/unsupported GPU returns a typed user-facing failure.

10. Publish telemetry: executor/backend, plan hash, source/pipeline cache hits, upload bytes/ms, command count/bytes,
    dispatch ms, readback bytes/ms, composition ms, and total compiler ms. GPU composition time is part of the GPU
    executor, not a later CPU pass.

## Correctness tests

Add one focused test target covering:

- Direct crop exact pixels.
- LoopX, LoopY, LoopXY.
- Non-square oriented source.
- Mapping transform.
- Radial center/radius/falloff/seam.
- Alpha and crop boundaries.
- Multiple distinct regions/sources writing distinct destinations.
- Unsupported-pair error.
- Stable plan/RegionId lookup.
- Warm source/pipeline cache hit and invalidation after crop/radial/source changes.
- Production supported-GPU route never calling CPU synthesis or CPU composition counters.

Direct integer-aligned sampling must be exact. Bilinear/radial tolerance must be declared narrowly before comparison;
do not weaken goldens after seeing results.

## Performance evidence

After the focused test passes, run the existing real 7952 x 4016 -> 8192 x 8192 release harness once cold and twice
warm. Preserve its workload. Target:

- Cold 8K Base Color: 2-6 seconds.
- Warm 8K Base Color: 1-3 seconds.
- Warm unchanged source upload: zero bytes.
- CPU Stage 14 and CPU atlas composition: zero production calls.

If a target fails, report decode/upload/dispatch/readback/encode separately. Do not reduce resolution or quality.

## Work discipline

- At most two consolidated discovery reads against allowed files.
- The next action must edit or return a precise blocker.
- No repository-wide search, full docs read, UI walkthrough, or unrelated cleanup.
- One cohesive implementation, one focused test, at most one correction pass.

## Focused verification

```powershell
cargo test -p hot-trimmer-sheet-compiler gpu_stage_14_base_color
```

## Done means

- GPU compact commands and source textures generate the final Base Color atlas.
- No old CPU per-pixel intermediates were recreated on GPU.
- No GPU output is recomposed on CPU.
- Parity and cache tests pass.
- Real 8K measurements are reported.

Stop after reporting changed files, exact runtime route, focused test, and benchmark results. Do not begin Prompt 003.

