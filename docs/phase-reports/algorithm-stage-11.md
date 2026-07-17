# Algorithm Stage 11: crop and synthesis candidates

## Delivered

Stage 11 consumes a prepared material-domain view and the authoritative Stage 10 slot-demand view. It emits stable,
bounded `CropCandidate` records carrying source/domain lineage, exact source-domain crop coordinates, one isotropic
scale, permitted quarter-turn/mirror transform, mapping/family route, period and seam references, correspondence
lineage, descriptors, seed, and inspectable eligibility evidence.

The configurable scale ladder applies one scalar independently to both Stage 10 source-footprint axes; integer
rounding never averages or coerces those axes to destination-pixel aspect. Quarter-turn crops swap source width and
height before position search, so rectangular 90/270-degree candidates retain legal geometry. Candidate positions combine a
low-resolution dense grid, coarse anchors, supplied feature/saliency/stationarity centers, period-aligned anchors,
and a deterministic farthest-point anchor. Whole-window validity is checked with an integral mask; direct windows
touching unusable pixels are rejected. Directional, confidently classified material excludes 90/270 degree turns
unless an explicit destructive-turn override is present and the template also permits the turn.

Role-specific families cover panel direct/tile/quilting/PatchMatch/procedural routes, distinct Repeat X and Repeat Y
segments/contiguous/graph-cut/quilting semantics, unique contain/cover/base-patch/explicit extension, three- and
nine-slice caps, planar square/detail/annular radial material, and polar synthesis. Planar radial material is never
polar-warped. Synthesis records retain explicit direct-crop applicability and the reason a direct crop failed.

## Bounds, insufficiency, and QA

Domain-mask work is rejected before candidate scoring when it exceeds `max_work`. Positions per crop size and final
candidates per slot are bounded before Stage 12; truncation and rejected-window counts are reported. Empty or
direct-insufficient requests expose typed recoveries (another source, lower texel density, or an explicitly permitted
synthesis route). QA views include candidate windows, source footprints, position strategies, eligibility, and source
usage. Family-aware round-robin truncation retains each legal direct, periodic, and synthesis family when the declared
bound can represent them; a smaller bound remains explicitly reported as truncation. Output bounds never participate
in source crop coordinates. The `source_placement_acceptance` gate covers isotropic footprint scaling, rotated crop
geometry, family retention, genuine farthest-point selection, and unknown lattice evidence.

Stage 12 remains responsible for unary scoring; Stage 13 remains responsible for global allocation and salient-feature
rationing.
