# Render Prompt 004 - Requested GPU material-map pass graph

Implement this prompt only after Prompt 003 passes. This file is self-contained. Do not read old material prompt packs
or design new material algorithms.

## Outcome

Extend the accepted GPU tile executor from Base Color into a dependency-driven material-map graph. Generate only the
map requested by preview/export and its real prerequisites. Keep intermediate textures GPU-resident and publish only
final requested tiles.

Required output maps:

- Base Color
- Height
- Normal
- Roughness
- AO
- Metallic when supported by the existing material contract
- Region ID

## Current facts

- Exact source binding, crop, transform, loop/radial behavior, destination, continuity, and edge eligibility already
  arrive in compact compiled region commands.
- Base Color sampling, final destination writes, tiles, padding, compact Region ID, cancellation, caching, and binary
  preview publication are already GPU-backed.
- CPU layout/project planning remains authoritative.
- This task ports existing profile/channel math. Missing authoritative math must remain unavailable with a typed
  diagnostic; do not invent a plausible shader.

## Edit boundary

- Existing profile/height/normal/channel source files directly named by current imports/callers
- GPU service, executor, pass-graph, and WGSL files from Prompts 002-003
- `crates/sheet-compiler/src/compiled_atlas_plan.rs` for additive structural/effect identities
- `crates/sheet-compiler/src/atlas_executor.rs`
- `crates/sheet-compiler/src/persisted_pipeline.rs`
- map fields in the compiled preview IPC contract
- Stage 14 map selection/display consumer
- one focused GPU material-pipeline test

Do not edit layout, source assignment, asset browser, final export, or add an effect library.

## Pass graph

Implement this exact dependency order using existing algorithms:

```text
registered source sampling
-> hotspot-local mask/distance/profile
-> structural height
-> material height + structural height
-> final Height
-> Normal derived/composed from final Height
-> Roughness
-> AO
-> Metallic when requested/supported
-> Region ID
-> final requested atlas tiles
```

The graph must deduplicate prerequisites. Base Color alone runs no Height/Normal/Roughness/AO/Metallic pass. Normal may
run Height prerequisites but does not force unrelated maps.

## GPU resource rule

Use compact commands plus tile-local textures. Do not create CPU per-pixel map buffers or full-frame GPU intermediates
for interactive preview. Do not represent every scalar as RGBA8.

Qualified working formats:

- Base Color: existing Prompt 002 color path.
- Height/profile: `R16Float` or `R32Float` selected by real range/precision needs.
- Normal intermediate: float; pack only for publication/export.
- Roughness/AO/Metallic/masks: single-channel formats.
- Region ID: compact `R32Uint` plus stable lookup.

## Required implementation

1. Add requested-map and pass identities to the compiled plan/cache graph. Identity includes upstream plan hash,
   structural/effect plan ID, map, format, tile/halo, algorithm/shader version, scale, seed, and normal convention where
   applicable.

2. Registered source channels use the exact Base Color source/crop/transform/loop/radial mapping. If channel
   resolution differs, use the existing registered-resolution transform. Never select a new crop per channel.

3. Generate hotspot masks and distance/profile coordinates from hotspot-local output bounds, not allocation/full-atlas
   bounds. Use compiled physical/pixel scale. Declare tile halos required by each operation.

4. Respect continuity/edge eligibility:

   - LoopX suppresses left/right structural seam edges.
   - LoopY suppresses top/bottom edges.
   - LoopXY suppresses both pairs.
   - Radial uses its explicit seam/edge contract.

5. Port the existing structural profile function and parameters. Compose structural height with material height in the
   existing order/range. Do not generate a profile and then omit it from final Height.

6. Derive/compose Normal from final composed Height. Apply OpenGL (+Y) versus DirectX (-Y) exactly once at the final
   convention boundary. Tile halo must cover derivatives.

7. Port existing Roughness and AO composition. Missing optional source channels use only existing documented material
   fallbacks. Do not add generic noise or flat placeholders and call them complete.

8. Pass through/compose Metallic only where the current material contract supports it. Publish a typed unavailable
   state otherwise.

9. Reuse Prompt 003 compact Region ID. Stable UUID identity remains in the lookup table, not per pixel.

10. If one already-compiled deterministic effect plan is production-ready, port exactly one effect as proof. It must
    use stable effect-plan ID, deterministic seed, hotspot-local coordinates, physical/pixel scale, edge eligibility,
    declared halo, and channel contributions defined by that plan. Otherwise leave effects out with a typed status.

11. Cache each pass output by exact dependency identity. Crop/source changes invalidate affected descendants. Normal
    convention invalidates packed Normal only. Selection and map display changes do not invalidate pixels.

12. Preview requests maps through `compile_persisted` and consumes artifact tiles. Frontend performs no map math. A
    failed current map must not display a stale prior-generation map.

13. Telemetry records requested, executed, cache-hit, skipped, and read-back passes plus per-pass GPU timings and bytes.

## Tests

Use one compact fixture containing direct, LoopX, LoopY, LoopXY, and radial regions. Prove:

- Structural height is visible in final Height.
- Normal derives from final composed Height.
- OpenGL/DirectX orientation is correct.
- Profile/mask coordinates use hotspot bounds.
- Looped edges are suppressed correctly.
- No pass writes outside hotspot plus authorized halo.
- Cross-channel landmarks remain registered.
- Height working range survives publication conversion.
- Roughness/AO/Metallic behavior matches existing reference math.
- Region ID resolves to stable IDs.
- Tile boundaries are seamless for every map.
- Base Color request runs no material passes.
- Normal request runs exactly its dependency graph.
- Cached map switch dispatches no new work.

Declare exact/tolerant comparison rules per format before running goldens. Do not loosen them afterward.

## Performance gates

- Cached map switch: <=50 ms.
- Warm requested 8K map set: 2-8 seconds on the qualification GPU, with exact maps listed.
- Single-map requests execute/read back materially less work than the full set.

## Work discipline

- At most two consolidated targeted reads: current authoritative math and current GPU pass boundary.
- Then edit or return a precise missing-contract blocker.
- No broad dead-code excavation. Follow only current call/import evidence for the named algorithms.
- One focused test, at most one correction pass, then one native all-map inspection.

## Focused verification

```powershell
cargo test -p hot-trimmer-sheet-compiler gpu_material_map_pipeline
```

Stop after reporting changed files, pass graph, requested/executed telemetry, focused test, native map evidence, and 8K
timings. Do not begin Prompt 005.

