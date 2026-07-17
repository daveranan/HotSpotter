# Algorithm Stage 04 — de-lighting and exposure normalization

## Delivered contract

Stage 4 consumes only the Stage 3 `PreparedExemplar` contract and always returns a typed
`DelitPreparedExemplar`. The output retains the immutable prepared Base Color, registered
coverage, upstream stage result, route execution, Stage 4 result, and reflectance provenance.
Only Base Color can be replaced; scalar, normal, mask, and categorical ID channels are copied
without filtering or reinterpretation.

Each material source set persists `DelightingIntent`. New and unclassified sources default to
`PassThrough(DefaultNewOrUnclassified)`. Authored textures and PBR sets have the documented
`AuthoredTextureOrPbrSet` reason, and disabling a previously enabled route records
`UserDisabled`. No filename, Base-Color-only state, absent companion map, or heuristic can change
the route.

## Routes

- `PassThrough` retains exact Stage 3 values and coverage, performs no lighting analysis, and
  records the specific reason. Invalid dormant classical settings are deliberately not evaluated.
- `ClassicalLowFrequency` operates on epsilon-protected log luminance with a deterministic,
  bounded separable edge-preserving filter. Radius accepts pixels, image-relative basis points,
  or physical millimeters with an explicit pixels-per-meter measurement. Strength, shadow and
  highlight recovery, color preservation, and edge preservation are bounded integer controls.
  Equal RGB correction preserves chroma by default and alpha remains unchanged. A local
  log-luminance edge detector attenuates the correction field at structural discontinuities,
  recombining the retained reflectance residual instead of dividing it away.
- `LocalIntrinsicProvider` uses interface version 1 with provider/model version metadata and a
  deterministic result contract. No model is bundled. Selecting an unavailable provider returns
  an explicit Stage 4 failure/status and uses only the persisted `None`, `PassThrough`, or
  `ClassicalLowFrequency` fallback. Installed results are rejected unless reflectance and every
  requested mask have registered dimensions, finite values, and normalized values.

All inferred reflectance is marked `Estimated`. Classical analysis can emit registered highlight,
shadow, clipping, and confidence masks. Confidence is intentionally reduced where dark pigment
and cast shadow cannot be separated reliably.

Classical execution preflights a conservative working-allocation estimate and an upper bound for
both separable filter passes. Explicit limits reject excessive radius/dimension combinations
before any Stage 4 work buffers are allocated.

## Authoring and reversibility

The source workbench exposes the selected route and bounded strength. It starts at
Off/PassThrough. The typed native command persists the complete intent and invalidates derived
source cache entries. Turning the control off selects `UserDisabled`; preview then reads the
retained Stage 3 Base Color, avoiding inverse processing or recomputation ambiguity.

## Focused evidence

`algorithm_stage_04_delighting` covers default and authored byte/value-stable PassThrough,
explicit classical correction of a synthetic illumination gradient, structural-edge retention,
deterministic repeated results and diagnostics, ambiguity confidence masks, unchanged scalar/ID
planes, exact disable/restore, unavailable-provider behavior without an implicit fallback,
malformed provider outputs, and operation/allocation budget rejection.
The provider-route evidence also keeps an intentionally invalid classical fallback dormant while
a valid installed provider executes, then proves the same fallback is validated when the provider
is unavailable.

Verification command:

```text
cargo test -p hot-trimmer-material-analysis algorithm_stage_04_delighting
```

Stage 5 remains responsible for source usability and quality scoring; Stage 4 only preserves the
diagnostics and masks required by that later decision.
