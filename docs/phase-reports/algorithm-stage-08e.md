# Algorithm Stage 08E — procedural material reconstruction

## Delivered route

Stage 8E adds the typed `ProceduralDomainModel` boundary and a fitted
`FittedProceduralDomainModel`. A fit records its model family, fitted parameters, confidence,
physical-micrometer or relative-millionth units, seed, algorithm version, registered channel
outputs, evidence lineage, diagnostics, QA views, and bounded generation limits. Unsupported
fits return `ProceduralFitOutcome::Unsupported` with a typed reason and recovery choices; they
never fall through to stretch or an unrelated shader.

The bounded V1 families are independent deterministic generators:

- wood face grain: warped longitudinal bands and pores;
- wood end grain: explicitly estimated polar growth rings and rays;
- brick/tile lattice: staggered cells and fitted mortar masks;
- corrugation: oriented manufactured ridges and valleys;
- brushed metal: anisotropic fine streaks and metallic response;
- concrete aggregate: fitted aggregate-cell distribution and matrix;
- painted metal: paint, deterministic chip mask, and exposed metal layer.

All channels are evaluated from one model coordinate transform. Base Color, Height, Normal,
Roughness, optional Metallic/AO, and the layer mask therefore have identical dimensions and
registration. Domain extents are expressed in fitted units, so changing output pixel dimensions
changes sampling density rather than motif scale. Cartesian and polar-compatible mappings use
the same fitted model coordinates.

## Evidence and corrections

Selection uses Stage 5 measured/routed behavior, never filenames. Stage 6 supplies orientation
and honest physical/relative scale authority. Stage 7 supplies registered periodicity, grid/fiber
fields, and source lineage. Palette quantiles and roughness are measured from registered Stage 4
channels. Stage 5 records its prepared-source digest, Stage 6 records that digest plus the exact
Stage 5 cache key, and Stage 7 records the exact Stage 6 key. Stage 8E rejects any break in this
complete chain before fitting.

A typed `ProceduralUserOverride` may correct model family, orientation, motif scale, palette,
roughness, or noise. Fitting always computes and retains `MeasuredProceduralParameters` first,
then derives separately inspectable effective values and per-parameter authorities. Only an
explicit model-family correction may authorize a family below the measured confidence gate;
unrelated partial corrections do not bypass it.

Radial classification alone is deliberately insufficient to claim wood end grain. End grain
requires the explicit typed estimated-model choice or a future source-specific authority and is
labeled estimated in diagnostics. Unique/semantic content—including vents, labels, text, and
container doors—returns `SemanticDetailRequiresDetailContract` and remains owned by later
detail/unique-content stages.

Low-confidence, non-procedural, missing-period, semantic-detail, and unconfirmed-end-grain cases
have explicit unsupported states and recovery choices. Fitting accepts a cancellation token,
preflights palette/roughness/grid/fiber scans and sorting, and checks cancellation between bounded
rows/tiles. Output pixels, fit samples, working bytes, operations, and cancellation checks are
bounded. Generation peak memory includes retained row-major arrays, generated normals, cloned
tiled channel storage, both mask copies, and tile metadata.

## QA and acceptance evidence

Model QA exposes parameter summary, coordinate grid, motif scale, orientation, layer masks, and
channel registration. The focused fixture constructs registered Stage 5–7 evidence and calls the
public fitting contract for all seven model families on horizontal, vertical, square, and
polar-compatible domains. It covers deterministic fit/generation replay, lineage rejection,
measured-versus-effective corrections, partial-override confidence behavior, explicit unsupported
states, fitting cancellation/bounds, conservative generation memory, registered channel sizes,
and the no-silent-end-grain rule. Corresponding model coordinates at two resolutions independently
assert identical height motifs, stable motif counts, stable normal orientation, and stable normal
strength.

Generated normals use fourth-order five-point model-space derivatives in the interior and
fourth-order one-sided five-point formulas at the first two/last two samples. Narrow domains use
bounded lower-order formulas. Derivatives retain physical coordinate spacing and motif-unit
normalization, check cancellation during the normal pass, and are included in generation's
operation preflight.

Verification command:

```text
cargo test -p hot-trimmer-material-synthesis algorithm_stage_08e_procedural
```

Stage 8F still owns final multi-route router policy and comparison.
