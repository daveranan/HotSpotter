# Render Prompt 005 - Bounded 16K/24K source tiling and streaming export

Implement this prompt only after Prompt 004 passes. This file is self-contained. Do not read old export prompt packs
or redesign the product.

## Outcome

Qualify the single `compile_persisted` GPU pipeline for real 8K/16K/24K projects with five to ten sources and at least
twenty patch bindings. Large sources and outputs must be tiled under explicit RAM/VRAM budgets. Export reads back and
encodes bounded final tile batches; it never materializes every full-resolution map at once.

## Current facts

- Compact compiled commands, source caching, authoritative sampling/composition, material passes, output tiles,
  cancellation, compact Region ID, and binary preview are already GPU-backed.
- CPU remains responsible for document planning, file decode scheduling, cache identities, final encoding, and disk I/O.
- Qualification RTX 3090 reports maximum 2D texture 32768, but portability and memory still require tiling.
- A 24K RGBA8 map is approximately 2.2 GiB; ten such maps cannot be treated as one resident product workload.

## Edit boundary

- GPU source/output tile scheduler and caches from prior prompts
- `crates/export`
- existing image encoding/output metadata code
- `crates/sheet-compiler` export-plan/executor boundary
- native export/progress/cancellation commands
- focused tiled-export tests and ignored release qualification harness

Do not edit layout authoring, sampling semantics, frontend material math, or create another exporter.

## Non-negotiable memory model

The GPU holds only:

- Compact source/region/pass commands.
- A bounded set of source tiles with filter halos.
- A bounded set of output/intermediate tiles with operation halos.
- A bounded upload/readback staging pool.

The CPU holds only:

- Bounded decoded source tiles.
- Immutable plan/metadata.
- Bounded final readback/encoder buffers.

Never allocate:

- A monolithic 16K/24K all-map CPU artifact.
- Every 24K output map simultaneously on GPU.
- Full decoded copies of all large sources per output tile.
- Full-frame correspondence or per-pixel UUID ownership.
- Base64/JSON export pixels.

## Required implementation

1. Define explicit configurable budgets for decoded CPU source tiles, GPU source residency, GPU output/intermediate
   residency, staging buffers, and total in-flight tiles. Report pinned versus evictable bytes.

2. Select source/output tile size and concurrency from adapter limits, supported formats, row alignment, halos, and
   budgets. Do not use a monolithic 24K texture merely because one adapter allows it.

3. Add source tiling for sources exceeding adapter/budget limits. Source tile identity contains source digest,
   oriented dimensions, decoder/color version, map role, level/mip, rectangle, and halo.

4. For each output tile, determine the exact source footprint required by direct, loop, and radial mapping. Include
   bilinear/filter halo. Radial/warp footprints may touch several source tiles; request all required tiles without
   downscaling or switching modes.

5. Bound CPU source decode and GPU source upload caches independently with deterministic LRU/equivalent eviction.
   Five to ten sources and twenty patch bindings must retain exact source/patch/RegionId identity.

6. Schedule output map/tile dependencies so only required intermediates are resident. Preview-visible tiles remain
   prioritized; export uses deterministic map/mip/tile order. Cancellation cannot be starved by a giant batch.

7. Preserve seamless valid interiors using the already-declared halos for sampling, padding, profiles, normal
   derivatives, AO/effects, and mips. Compare edges/corners/radial seams/loop seams against a monolithic <=limit
   reference.

8. Extend the existing export route only:

   ```text
   compile_persisted export plan
   -> GPU output tile
   -> bounded staging readback
   -> streaming encoder/spool
   -> temporary output
   -> atomic finalization
   ```

   Inspect the current encoder's actual streaming capability. If it cannot accept bounded scanlines/tiles, add a
   bounded streaming encoder or disk-backed spool. Do not hide a full-frame allocation behind an API named stream.

9. Preserve bit depth, color space, alpha, normal convention, naming, map metadata, and mips. Export pixel bytes never
   pass through Base64 or JSON.

10. Write new output to a temporary sibling and atomically finalize only after all requested maps succeed. A cancelled,
    stale, disk-full, encoder-failed, or device-lost export cannot replace an existing valid export.

11. Add progress containing map, mip, completed/total tiles, render/readback/encode times, bytes written, and estimated
    remaining work. Check cancellation before decode/upload, every GPU batch, readback, encoder block, and finalization.

12. Handle GPU validation, unsupported feature/format, out-of-memory, device loss, readback, encoder, and filesystem
    failures as distinct diagnostics. On device loss, cancel in-flight work, recreate service-owned GPU state, and
    rebuild resources lazily from immutable plan/source identities. Resume only where completed data is provably valid.

13. Define supported GPU minimums. Unsupported hardware gets a clear product diagnostic; it does not silently start
    the old CPU renderer.

14. Remove CPU executor selection from production preview/export after parity and qualification pass. Keep the
    smallest CPU reference fixtures as test-only code. Add a production-route counter/assertion proving no migrated
    map calls CPU slot synthesis or CPU atlas composition.

## Tests

- Source-tile parity at direct, loop, and radial tile boundaries.
- Output tile seams for every supported map and mip.
- Multi-source/multi-patch save/reopen identity and pixel equivalence.
- Budget pressure causes safe eviction, not unbounded growth.
- Cancellation during decode, upload, dispatch, readback, encoding, and pre-finalization.
- Stale revision completion cannot publish/finalize.
- Simulated disk-full/write failure preserves an existing output.
- Simulated device loss returns/rebuilds according to policy.
- Unsupported adapter diagnostic.
- Production CPU renderer counters remain zero.

## Release qualification

Use actual pixels and release builds:

1. >=7952 x 4016 source -> 8192 output Base Color.
2. Same project -> 8192 all supported requested maps.
3. >=5 sources and >=20 patches -> 16384 representative/full product map set.
4. Same multi-source project -> 24576 representative/full product map set.
5. Warm crop/radial edit, cached map switch, and cancellation.

Record hardware/backend/driver/limits, sources, patches/regions, maps, tiles/halos/concurrency, cold/warm phase timings,
first-tile time, total time, peak RSS, GPU residency, cache/upload/readback/write bytes, and output hashes/tolerances.
Run on discrete and integrated GPUs when actually available; report missing hardware honestly.

Hard gates:

- 16K/24K first exact preview tile <=500 ms on the qualification discrete GPU.
- Memory remains within declared budgets.
- Existing warm 8K interaction/map targets remain green.
- No monolithic oversized/all-map resource exists.

## Work discipline

- Read only current scheduler/export/encoder symbol ranges.
- First edit after at most two consolidated discovery calls.
- No broad audit, docs reread, UI redesign, or unrelated cleanup.
- One focused test and at most one correction pass; qualification runs are separate evidence.

## Focused verification

```powershell
cargo test -p hot-trimmer-export gpu_tiled_export
```

Stop after reporting changed files, memory policy, focused test, qualification results, failure behavior, and proof that
production CPU rendering is retired.

