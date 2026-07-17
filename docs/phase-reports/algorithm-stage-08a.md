# Algorithm Stage 08A: registered direct and graph-cut material domains

## Delivered authority

Stage 8A now exposes one `PreparedMaterialDomain` boundary with typed `DomainRequest`,
`DomainRoute`, `CorrespondenceField`, `OperationField`, diagnostics, QA views, and typed registered
channel access. Requests carry the immutable prepared-source digest and the typed Stage 7
`FeatureFieldReport`; no filename, material name, generic analysis JSON, or duplicated preparation
state participates. The Stage 3 prepared-source digest is carried through Stage 4 and Stage 7 and
must match the request exactly; every consumed level-zero analysis plane is also dimension-checked.
Direct domains share the Stage 4 exemplar buffers through `Arc` and use identity
correspondence, so accepted seamless inputs are not copied or resampled.

Auto routing selects Direct only when both measured Stage 7 boundary costs clear the declared policy
threshold. Otherwise it selects graph-cut closure. Explicit policy can pin either implemented route.
The cache key includes prepared-source and Stage 7 analysis digests, selected route, all bounded
settings, algorithm version, dimensions/channel inventory, and seed. Only complete domains enter the
bounded deterministic cache.

## Graph-cut closure and composition

X, Y, and XY closure use bounded boundary overlaps and candidate counts. A dynamic-programming seam
uses normalized available terms for linear Base Color, luminance gradient, Height, vector-normal
difference, Roughness, and maximum registered structure evidence. Missing optional maps receive no
weight and the remaining terms are renormalized. Stable lower-coordinate tie-breaking and bounded
one-pixel seam motion make the result deterministic. Each selected seam must also clear the declared
maximum normalized multi-channel cost. An unacceptable seam returns typed insufficiency with
synthesis, alternate-source, and settings recovery choices; it never publishes a visible periodic
boundary. Cancellation is checked between row/column and channel work units; overlapping or
undersized search bands fail with typed insufficiency.

One correspondence and operation field drives every registered channel. Linear color/scalars blend
from its weights, normals blend as vectors and are renormalized, Opacity blends continuously, and
Material IDs/genuinely binary masks choose the highest-weight source categorically with stable ties.
The domain retains continuous validity,
source correspondence, and an explicit original-versus-seam-composed provenance mask.

## QA and acceptance evidence

QA exposes registered channels, seam costs and paths, boundary difference, correspondence,
operations, validity, and provenance. Diagnostics retain before/after boundary costs, normalized
weights, available evidence terms, seam summaries, the selected route, and explicit Direct
pass-through evidence.

Post-closure boundary diagnostics are recomputed with the same normalized available Base Color,
gradient, Height, vector-normal, Roughness, and structure-cut terms used by seam selection. Direct
preflights its validity/provenance allocations and computes identity validity row-wise with bounded
cancellation; it does not allocate a redundant full correspondence vector.

The focused fixture proves two-axis closure reaches zero Base Color boundary difference, a lower-cost
legal seam avoids a structured boundary, all channels share one mapping, vector normals remain unit
length, Opacity uses weighted continuous composition, IDs remain members of the original categorical
set, Direct uses identity correspondence, cache keys are stable, foreign same-sized analysis is
rejected, excessive seam cost fails with recovery, and cancellation/resource/overlap limits fail
explicitly.

Verification command:

```text
cargo test -p hot-trimmer-material-synthesis algorithm_stage_08a_graphcut
```

Quilting, PatchMatch, statistical/spectral, procedural, learned-provider, and final multi-route
comparison remain owned by Stages 8B through 8F.
