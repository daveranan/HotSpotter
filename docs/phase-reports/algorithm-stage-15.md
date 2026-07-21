# Algorithm Stage 15 — compiled structural profiles and tiled GPU synthesis

Stage 15 replaces normalized structural-profile evaluation with immutable CPU-compiled plans and an application-owned GPU tile pass.

## CPU compilation boundary

`RequestedProfile` accepts meters or explicitly relative minor-edge lengths. `CompiledProfile` resolves those requests to physical meters and records pixel scale, occupancy semantics, LOD, supersampling, evaluator, fallback and reason, algorithm version, compact curve references, halo, diagnostics, seed, and an exact upstream-derived cache identity.

The CPU owns branch-heavy legality and command compilation. It validates custom curves and opposing-edge/minimum-flat-center constraints, then deterministically selects clamp, fully rounded, merged opposing, normal-only, disabled, or incompatible outcomes. Supersampling is selected only after legality and never changes the physical width or makes an illegal profile fit.

The complete compact program set is Flat, convex bevel, concave groove, rounded bevel, double bevel, raised lip, recessed seam, panel frame, fully rounded strip, merged opposing bevel, radial disc, annulus, and bounded custom curve. FullHeight, SimplifiedHeight, NormalOnly, RoughnessOnly, and Disabled LODs retain physical dimensions and seed.

## GPU execution and residency

The existing GPU capability service compiles one bounded Stage 15 compute pipeline. Each requested tile evaluates rectangle, disc, or annulus SDF, physical Height, analytic derivatives, supersampling, and authoritative semantic fields for inside/outside, flat center, raised, recessed, cap, groove, and profile exclusion.

Five R32Float tile-local resources—Height, signed distance, packed semantic occupancy, dHeight/dx, and dHeight/dy—are cached by exact plan/map/tile/halo/format/shader identity. They remain GPU resident for Stages 16–19. Cancellation and revision checks run before dispatch and again before cache publication. The pass rejects a tile whose five fields exceed the declared intermediate residency budget; export scheduling must split such work instead of allocating full-atlas CPU planes.

The requested-map graph dispatches no profile work for Base Color. Height, Normal, and scalar effect dependencies request the profile fields. QA reads back only an explicitly bounded tile. Allocation borders are not inferred as authored structure: compact commands carry authored edge eligibility and profile occupancy instead.

Imported normal sampling remains separate from profile Height. Stage 15 does not bake the two into an opaque replacement; Stage 17 remains the composition authority.

## Removed shortcuts and verification

The material WGSL no longer contains the hard-coded normalized structural evaluator. The production Stage 15 route does not construct CPU SDF, occupancy, Height, or normal planes. CPU formulas used by Stage 15 tests are bounded scalar oracles only.

Focused verification:

```text
cargo test -p hot-trimmer-sheet-compiler algorithm_stage_15_gpu_profiles
```

Coverage includes all programs, 1K/2K/4K/8K physical-scale fixtures, extreme strips, sub-pixel LOD without widening, deterministic opposing-profile fallback, GPU/CPU formula parity, GPU cache reuse, bounded QA readback, and cancellation without stale publication.
