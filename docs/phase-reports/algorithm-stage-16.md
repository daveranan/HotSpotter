# Algorithm Stage 16 - compiled semantic details and tiled GPU evaluation

Stage 16 now compiles material-independent semantic detail definitions into deterministic, typed commands before any pixel work runs.

`DetailDefinition`, `CompiledDetail`, `StampOperation`, and `StampStroke` record physical size/range, scale space, compatible roles, orientation, aspect limits, minimum pixels, repeat periods, contain/cover/fail policy, channel contributions, fallback, provenance, seed, immutable asset references, required halo, operation dependencies, scope, clipping, layer order, occupancy relation, and exact cache identity. Stamp operations keep immutable asset/version references and deterministic physical parameters or stroke samples; they do not store pasted display pixels.

The compiler supports repeating strip, unique detail, radial detail, trim cap, bolt group, vent, panel stamp, groove, decal, procedural motif, and user-patch families. Oversized or incompatible details must select an explicit fallback or fail; they are not squeezed to fit. Material-reusable stamp operations are separated from asset-specific deferred operations so reusable atlas details are not accidentally baked into every asset using a material.

Atlas region commands now carry a `CompiledDetailSet` next to the Stage 15 structural profile. Detail identities participate in the plan hash and structural/detail plan identity, while the empty detail list is represented as `SkippedBecauseUnused`.

The GPU executor adds a bounded Stage 16 tile pass using the existing digest-keyed GPU cache. It publishes GPU-resident R32Float detail mask, Height, normal-input, scalar, and color/ID contribution fields keyed by exact map/tile/halo/shader identity. Empty detail lists dispatch zero work, and repeated identical detail plans hit the existing rendered-texture cache.

The QA route reports detail route and occupancy lines from compiled semantics. Tests cover all detail families, panel bolt groups as distinct from strip rivets, physical scale stability across 1K and 8K, save/reopen deterministic identity, reusable versus deferred stamp scope, empty-list skipping, GPU field residency, and cache reuse.
