# Render Prompt 003 - GPU tiles, compact Region ID, and exact binary preview

Implement this prompt only after Prompt 002 passes. This file is self-contained. Do not read other docs or retrace
layout/sampling behavior.

## Outcome

Replace the monolithic GPU Base Color publication path with capability-bounded output tiles. The same GPU kernels must
serve full draft, progressive refinement, viewport/selected-region 1:1 inspection, and later export. Interactive pixel
payloads must be raw binary, not PNG/Base64.

## Current facts

- `compile_persisted -> CompiledAtlasPlanV1 -> GpuAtlasRenderExecutor` already produces correct final Base Color atlas
  pixels for direct, loop, planar, and radial regions.
- Source textures, compact region commands, shaders, and pipelines are GPU-cached.
- Current frontend publication still uses `dataUrl: string`, PNG encoding, and Base64.
- Existing plan contains `CompiledTileRequest { output_rect, mip_level, halo_px, valid_rect }`.
- Qualification adapter recommends 2048 tiles, but tile size must remain capability/memory selected.

## Edit boundary

- GPU service/executor and WGSL files introduced by Prompt 002
- `crates/sheet-compiler/src/compiled_atlas_plan.rs`
- `crates/sheet-compiler/src/atlas_executor.rs`
- `crates/sheet-compiler/src/persisted_pipeline.rs`
- `apps/desktop/src-tauri/src/document_commands.rs`
- `packages/ipc-contracts/src/document-contracts.ts`
- `apps/desktop/src/source-frame-preview-controller.ts`
- the Stage 14 preview display portion of `apps/desktop/src/source-first-app.tsx`
- focused native/frontend tests for tiled preview

Do not edit layout authoring, sampling math, material maps, or export.

## GPU memory rule

Keep compact region/source command buffers from Prompt 002. Do not add full-frame coordinate/seam/correspondence
buffers. Allocate only bounded output/intermediate tiles and staging buffers.

Region ownership uses:

```text
GPU pixel: u32 compact region index
artifact metadata: compact index -> stable RegionId
```

Never store a `RegionId` UUID per pixel. Generate Region ID only when required for selection/diagnostics or downstream
passes.

## Required implementation

1. Add deterministic tile identities containing plan hash, map, mip, output rectangle, halo, valid rectangle, profile,
   shader version, and generation. Selection/inspector state is not part of pixel identity.

2. Add request kinds without creating another compiler:

   - Complete 512 draft.
   - Complete 1024 progressive refinement.
   - Exact output-resolution viewport rectangle.
   - Exact output-resolution selected-region rectangle.

   UI sends view intent. `compile_persisted` validates and produces `CompiledTileRequest`; UI never calls GPU stages.

3. Schedule visible/selected tiles first, neighbors second, background refinement last. Bound in-flight tiles,
   readbacks, and frontend resources. Coalesce drag requests and stop scheduling obsolete generations.

4. Render each tile using the exact Prompt 002 commands and sampling shaders. Output invocation coordinates remain
   global atlas coordinates. Clip writes to the tile halo and publish only `valid_rect` so tiles match monolithic
   common texels.

5. Add GPU padding/dilation using the existing authorized padding/allocation bounds. Padding may not cross into another
   region. Declare its required halo explicitly.

6. Add compact GPU Region ID (`R32Uint` where qualified) and stable lookup metadata. Ordinary Base Color preview must
   not allocate a full-frame ID map unless requested/needed.

7. Remove mandatory dense correspondence from ordinary production preview. If debugging still needs correspondence,
   expose it only as an explicitly requested diagnostic tile.

8. Add a bounded GPU tile cache and reusable readback staging pool. Panning or selection reuses exact cached tiles;
   crop/radial edits invalidate only affected plan/tile identities.

9. Version the preview artifact with a tile manifest containing map, mip, output rectangle, valid rectangle, halo,
   generation, pixel format, dimensions, row stride, and opaque tile handle.

10. Replace Stage 14 interactive PNG/Base64 publication:

    - Metadata may remain JSON.
    - Pixel bytes return through a raw Tauri byte response or scoped local resource endpoint.
    - Do not send JSON number arrays.
    - Do not encode/decode PNG in Rust or TypeScript for interactive tiles.
    - Handles resolve only cache-owned tiles and are released on eviction/supersession.

11. Update the frontend to blit raw tile bytes into the existing preview display surface at manifest-provided
    rectangles. It may use canvas/ImageData or an existing display upload mechanism solely for byte display. It may
    not implement crop, loop, radial, padding, Region ID, or atlas math.

12. Keep the old draft visible until replacement tiles have painted. Reject stale generations before native
    publication and again before frontend paint. Revoke superseded frontend resources.

13. Telemetry must separate plan, dispatch, readback, raw IPC, frontend upload, paint, cache hits/misses/evictions, and
    bytes. Join native and frontend timing by request generation.

## Tests

- Edge/corner/partial tiles and non-square output.
- Halo clipping and valid-interior placement.
- Tiled versus monolithic common-texel parity.
- Padding has no black seam or cross-region bleed.
- Compact ID resolves every tested pixel to the correct stable RegionId.
- Correspondence is absent unless explicitly requested.
- Old generation completing after a new edit is never displayed.
- Draft remains until refinement paint.
- Cached pan/selection avoids GPU work.
- Interactive path does not call PNG/Base64 helpers.
- Raw payload size equals declared stride/dimensions.

## Performance gates

- Selected-region exact 1:1 refresh for 1024-2048 output texels: 50-250 ms warm.
- Cached/neighbor pan tile: <=100 ms to paint.
- Cold 512 complete-sheet first paint: <=1 second.
- Warm 1024 refinement: <=500 ms.
- Cached displayed-map tile switch: <=50 ms.

Report component timings if missed; do not fake success with an enlarged 512 image.

## Work discipline

- Read only named symbol ranges in allowed files.
- First edit after at most two consolidated discovery calls.
- No broad audit, documentation reread, full workspace test, material work, or export work.
- One focused test and at most one correction pass.

## Focused verification

```powershell
npm.cmd run test --workspace @hot-trimmer/desktop -- gpu-tiled-preview
```

After the test passes, perform one native check: 512 navigation -> select radial region -> exact 1:1 tile -> edit ->
pan. Record request-to-paint telemetry. Stop and report; do not begin Prompt 004.

