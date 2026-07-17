# Algorithm Stage 08B: deterministic registered texture quilting

## Delivered route

`DomainRoute::TextureQuilting` extends the Stage 8A `DomainRequest` and
`PreparedMaterialDomain` authority. Patch extent is declared either relative to the registered
source or in physical micrometers resolved exclusively through the exact Stage 6 report consumed
by Stage 7. Physical sizing requires world-accurate imported, user-measured, motif-derived, or
convention evidence; relative and prior-estimated evidence fail explicitly. Output dimensions,
pixels, patches, candidates, pyramid levels, working bytes, operations, iterations, and
cancellation are bounded before or between work units.

Patch candidates are scored over a registered box-filtered pyramid carrying Base Color, gradient,
Height, Roughness, vector Normal, and structure evidence. Distribution matching uses normalized
16-bin luminance and 8-bin gradient histograms at every level rather than a mean-luminance proxy;
the remaining terms cover overlap, multi-level structure, duplicate use, and output-boundary
periodicity. Candidate iteration and
tie-breaking are stable. A seed-indexed stream chooses among the bounded near-best set, so a fixed
seed replays exactly without collapsing every placement to one identical optimum. Placement
advances receive bounded deterministic jitter for stochastic materials, avoiding a fixed visible
patch grid. Period- or band-constrained requests disable jitter and must align their patch advances
to the declared phase.

Every left or top overlap is cut by one dynamic-programming minimum-cost seam computed from Base
Color, gradient, Height, vector Normal, Roughness, and structure evidence when available. That seam
updates one correspondence/operation field used by every channel. Continuous/scalar/color and
vector-normal channels therefore remain registered; normals retain vector semantics; material IDs
and non-opacity masks remain discrete. Candidate patches containing pixels below the requested
usability/coverage threshold are rejected, counted, and fail explicitly if none remain. Declared
maximum seam-energy and registered boundary-periodicity thresholds are publication gates; an
over-threshold result returns typed recovery instead of `Executed`.

## Semantic routing and QA

Directional requests must identify a directional behavior class and stay within their angular
tolerance because this route does not rotate tangent-space data. Lattice and band requests must be
phase-aligned. Unique detail, manufactured motifs, unapproved lattice quilting, and mixed/unknown
semantics return typed insufficiency rather than being duplicated or smeared.

`QuiltingDiagnostics` records placements, selected candidate rank and cost, source-patch usage,
duplicate count, every overlap seam and its energy, mean seam energy, correspondence confidence,
unusable-candidate rejection count, boundary-periodicity error, and failure-reason capacity. QA
views expose quilting layout and source usage alongside seams, correspondence, operations,
validity, registered channels, and boundary difference.

## Focused evidence

The Stage 08B fixture expands the bundled stochastic-registration and directional-orientation
corpus exemplars in addition to controlled penalty fixtures. It replays byte-stable
correspondence for a fixed seed, verifies one shared discrete seam mapping, checks unit normals,
demonstrates increased unique source-patch usage under a duplicate penalty, quantitatively rejects
a fixed grid advance and dominant exact patch repeat, measures output direction against the source
within angular tolerance, proves registered Base Color/Roughness correspondence, validates Stage 6
physical authority, and rejects high-energy seams, poor output boundaries, unique semantics,
unusable masks, cancellation, and memory overflow. The cache identity includes both Stage 8A and
Stage 8B algorithm versions.

Verification command:

```text
cargo test -p hot-trimmer-material-synthesis algorithm_stage_08b_quilting
```

PatchMatch, statistical/spectral, procedural, learned-provider, and final multi-route comparison
remain owned by Stages 8C through 8F.
