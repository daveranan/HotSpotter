# Algorithm Stage 08 — authoritative material-domain construction

## Delivered routes

Stage 8 now has one authoritative entry point: `prepare_stage_08_material_domain`. It validates
the complete Stage 4–7 lineage before comparing or executing any engine. The earlier direct,
multi-channel graph-cut, deterministic quilting, and registered PatchMatch engines remain behind
that interface. The final integration also connects the fitted procedural engine and adds a
bounded deterministic classical statistical route for genuinely stochastic isotropic sources. That
route constructs one smooth periodic multiscale correspondence field, bilinearly samples every
registered channel through it, and publishes only after source/output mean, variance, gradient,
and lag-spectrum measurements pass a normalized quality gate; it never resamples pixels independently.

The router implements the revised section 26.1 source classes. Existing tileable PBR, stochastic
fine material, rust/grunge, directional material, lattice, organic grain, manufactured period,
unique detail, radial detail, and mixed/unknown evidence each receive an explicit preferred and
fallback set. The finer source labels (for example end grain) can only come from typed user policy;
filenames, fixture IDs, and product labels are not inputs. Statistical synthesis is explicitly
inapplicable to lattice, motif, directional, unique, radial, and semantic content.

Every route comparison records applicability, rejection reason, bounded preview dimensions, a
normalized integer cost breakdown, stable route-order tie breaking, and whether execution was
attempted. A failed preferred engine advances only to a documented applicable fallback. If none
succeeds, the result is typed actionable insufficiency rather than stretch or a visually plausible
but semantically invalid route. Override, pin, route reset, and full policy reset change routing
intent only; they do not modify template topology.

## Learned-provider boundary

`LocalLearnedMaterialProvider` is a local-only adapter for de-lighting fields, seamless expansion,
super-resolution, and estimated height/normal maps. Its descriptor records provider/model version,
model digest, interface version, capabilities, device policy, deterministic support, and input,
output, and memory bounds. Its output records model/output digests, device, confidence,
determinism, diagnostics, and registered channels.

No model or pretend inference implementation is bundled. Disabled, absent, rejected, available,
and used provider states are ordinary typed diagnostics. An absent provider is recorded in route
comparison and follows the same deterministic classical/procedural fallback contract. Learned
output is accepted only when its model digest matches the descriptor, dimensions are registered,
the actual execution device satisfies policy, confidence is bounded, and two identical inference
requests produce byte-identical canonical channel digests. Provider-claimed output digests must
match that independently computed digest, which is then included with source registration, model,
device, and seed in cache provenance. Learned channels are labeled Estimated.

## Domain validation and diagnostics

Before returning the authoritative result, the router validates:

- all registered channel, validity, provenance, operation, and correspondence dimensions;
- expected periodic/seam behavior measured independently for every returned channel, with the
  worst channel governing publication so smooth channels cannot hide an ID/Normal/scalar discontinuity, while preserving non-periodic
  unique/radial domains; structured direct routes additionally require axis-aligned period evidence
  that divides the domain and measured phase-aligned boundaries;
- physical or explicit relative-scale lineage from Stage 6;
- deterministic execution and stable cache provenance;
- the exact prepared-source, Stage 5, Stage 6, and Stage 7 registration chain.

The result contains selected source class and authority, chosen route, every comparison and
rejection, learned-provider state, validation booleans, recovery messages, and the complete domain
QA view set (registration, boundary, correspondence, operations, validity, provenance, route
comparison, applicability, scale, determinism, and cache provenance).

## Route summary

| Source evidence | Preferred domain route | Documented fallback |
| --- | --- | --- |
| Already tileable PBR | Exact direct domain | Multi-channel graph-cut closure |
| Fine concrete/plaster | Quilting | Classical statistical or fitted aggregate |
| Rust/grunge | Quilting | Classical statistical synthesis |
| Brushed/directional material | Direction-aware quilting | Fitted directional procedural model |
| Brick/tile lattice | Measured, integral, phase-aligned direct domain | Fitted lattice reconstruction |
| Wood face grain | Direction-aware quilting | Fitted wood reconstruction |
| Explicit wood end grain | Fitted polar reconstruction | Approved learned estimate or insufficiency |
| Manufactured border | Measured period-aligned direct domain | User-confirmed typed period/route |
| Unique vent/panel | Exact non-periodic domain | Constrained PatchMatch expansion |
| Radial drain/washer | Exact non-periodic radial source | Explicit fitted radial route or insufficiency |
| Mixed/unknown | Stable applicable-route comparison | User override/pin or actionable insufficiency |

Verification command:

```text
cargo test -p hot-trimmer-material-synthesis algorithm_stage_08_router
```
