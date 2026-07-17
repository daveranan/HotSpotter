# Algorithm Stage 12: candidate scoring

## Delivered objective and legality gate

Stage 12 consumes Stage 11 `CropCandidate` records and ranks only candidates whose recorded mapping, transform,
isotropic scale, aspect, usable-window, strip cross-axis, lattice, and slot ownership evidence remains legal. Every
non-synthesis candidate requires a source crop, positive direct-crop applicability, and explicit whole-crop usability;
missing evidence fails closed. Only a family/route pair recognized as typed synthesis or polar synthesis may use the
documented no-crop/unknown-usability exception; explicit unusability rejects every route. Every direct or synthesized
repeat family also requires positive cross-axis-preservation evidence rather than treating missing evidence as a pass.
A failed predicate produces an `ApplicabilityRejection`; rejected
candidates never receive a numerical cost and cannot re-enter the top K.

Every accepted candidate carries an eleven-term `CandidateCostBreakdown` in the revised equation order. Each entry
records normalized cost, confidence, behavior/role weight, applicability, weighted contribution, and a short
interpretation. Total cost is the sum of those visible contributions. Sorting uses total cost followed by the stable
content-derived candidate ID; the configurable top K defaults to 64.

## Normalization and policy

All term costs and confidences are clamped to `[0, 1]`, with lower cost preferred. Milli-unit measurements map
`0..=1000` to `[0, 1]`; missing evidence has zero confidence rather than an invented measurement. Scale is symmetric
absolute log error, normalized to one at a configurable 4x or 1/4x ratio. Resolution measures the deficit below one
source pixel per output pixel. Generic stationarity is `1 - stationarity`, while unique roles target controlled
stationarity. Generic saliency is a direct uniqueness penalty; compatible unique-detail material targets saliency
`0.70` rather than maximizing it without bound.

Structured behavior uses inverse measured lattice/period completion plus an independent strong-boundary-cut cost;
Stage 11 lattice eligibility never overwrites the measured completion value. Boundary-cut evidence also contributes
for directional and other non-lattice materials whenever that measurement has confidence.
Directional behavior compares the measured source direction after the candidate quarter turn with the authored slot
axis using `1 - abs(cos(delta))`, so directions separated by 180 degrees are equivalent. Seam cost is inverse measured
opposite-boundary or solved-seam quality for both direct repeats and quilted repeat synthesis. Quality is inverse
measured visual quality and uses only its own confidence; Stage 11 usability is legality evidence, not a substitute
quality measurement. Role cost combines typed
candidate families with authored slot role and behavior compatibility. Synthesis complexity is bounded from direct
`0.0` through graph-cut/quilting/PatchMatch to procedural or polar reconstruction `1.0`.

Weights are selected only from `MaterialBehaviorClass` and `TemplateSlotRole`. Generic planar/repeating roles emphasize
stationarity and low saliency; unique roles reshape those preferences; lattice/manufactured/banded behavior emphasizes
structure, boundary cuts, and seams; directional behavior emphasizes 180-degree-equivalent alignment; radial and cap
roles emphasize typed role compatibility. Term-specific overrides support inspectable tuning experiments. The
synthesis weight is deliberately below quality and role weights: it prefers simpler routes at comparable quality but
cannot legalize a route or force a visibly worse direct crop.

## Evidence and QA

The focused corpus-style tests inspect term values and contributions, not only a winning index. They cover calm
generic-surface selection, structured boundary and orientation alignment, controlled unique-role saliency, a ranking
change caused by one saliency-weight override, candidate-ID tie determinism, legality rejection, and the bounded
complexity preference. QA views expose ranked candidates, full cost breakdowns, applicability, per-term contribution,
and the active role/material policy. Stage 13 remains responsible for pairwise reuse, overlap, diversity, and global
assignment.
