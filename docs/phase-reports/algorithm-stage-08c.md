# Algorithm Stage 08C: registered PatchMatch synthesis

## Delivered route

Stage 08C adds `PatchMatch` behind the Stage 8 material-domain interface. The route builds one
versioned, seeded nearest-neighbor field and uses that field to reconstruct every registered
channel. Color and scalar data use the shared correspondence weights, tangent normals are filtered
as vectors and renormalized, and Material IDs and non-opacity masks remain categorical.

The optimizer constructs a registered descriptor pyramid from Base Color, gradient, Height, vector
Normal, Roughness, Stage 7 structure, saliency, usability, coverage, and source-exclusion evidence.
It initializes a reduced NNF at the coarsest level, upsamples the correspondence into each finer
output grid, and refines against the matching descriptor level with alternating propagation and
shrinking random search. Initialization is deterministically hashed, costs are quantized, and ties
use coordinate order. It never reads system randomness. Coherence, duplicate-use, and
saliency-repeat penalties supplement the normalized patch terms. Directional requests require
compatible Stage 6 orientation evidence.

## Completion, expansion, and safety

Expansion can constrain either output axis to be seamless. Wrapped neighbors participate in the
coherence term, the final NNF is made exactly periodic on requested axes, and reconstructed
multi-channel boundary error is measured and publication-gated rather than inferred. Completion
masks use one to mark pixels to synthesize and zero to preserve registered
source pixels exactly. A separate source-exclusion mask is checked over every tested patch, together
with Stage 7 usability and prepared-source coverage. A rejected or incomplete source field returns a
typed insufficiency result.

Unique detail and manufactured patterns are rejected by default. Unique material is accepted only as
protected masked completion; manufactured content requires an explicit period compatible with the
requested extent. This prevents the route from silently multiplying salient or manufactured content.

Dimensions, pixels, patch radius, pyramid levels, iterations, search radius, random candidates,
working memory, and operations are bounded. Memory preflight sums every retained descriptor-pyramid
level using the actual `Descriptor` allocation size and includes level-vector capacity plus the
output working set; an over-budget pyramid fails before allocation. Cancellation is observed between passes and row tiles.
Failure to reach the declared convergence threshold returns `PatchMatchNonConverged`; an unconverged
image is never published or cached.

## Diagnostics and focused evidence

`PatchMatchDiagnostics` records the fixed algorithm version and seed, every pass and direction,
propagation/random-search acceptances, source usage, synthesized/preserved counts, unusable and
excluded rejections, operation count, convergence, confidence, and incomplete pixels. QA exposes the
NNF, coherence, source usage, correspondence, operations, validity, provenance, and registered
channels.

The focused test expands the registered stochastic and directional corpus fixtures, proves exact NNF
and diagnostic replay, checks cross-channel registration and vector normals, verifies excluded source
pixels are never selected for completion, rejects implicit unique/manufactured repetition, and covers
nonconvergence, cancellation, and resource exhaustion.

Verification command:

```text
cargo test -p hot-trimmer-material-synthesis algorithm_stage_08c_patchmatch
```

Statistical/spectral, procedural, learned-provider, and final multi-route comparison remain owned by
Stages 08D through 08F.
