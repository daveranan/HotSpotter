# Algorithm Stage 13 — global placement optimization

## Delivered

- `optimize_placements` consumes the stable Stage 12 top-K artifacts and assigns every slot jointly. Slots are
  ordered deterministically by descending visual importance and constraint tightness, then by candidate count and
  stable slot ID. A bounded beam keeps a configurable number of partial assignments; fixed-seed semantic tie keys
  make equal-cost results independent of input iteration order.
- The complete objective records unary cost plus source-overlap, descriptor-similarity, repeated-salient-feature,
  identical-transform, and variation-group pairwise terms. Every evaluated candidate pair is retained as accepted,
  rejected by material policy, or rejected by the global objective, with the complete term breakdown and reason.
- Reuse is class-aware. Stochastic inputs may overlap at a reduced cost when policy permits, but this does not
  legalize duplicate salient marks and the stochastic discount requires different transforms. Manufactured/periodic
  candidates with an explicit matching period may reuse a
  cycle intentionally with overlap and transform penalties suppressed. Unique salient source regions are rejected
  across large visible slots unless repeated-salient reuse is explicitly enabled.
- Deterministic single-slot replacement and atomic two-slot replacement (the slot-specific equivalent of a swap)
  improve the completed beam result within configurable pass and evaluation bounds. Cancellation is checked between
  bounded beam and local work units; cancellation returns an error and never returns a partial authoritative plan.
- Final validation requires one placement per input slot, every required slot, finite positive isotropic scale, and
  the prepared-domain identity, authoritative domain dimensions, and registered correspondence reference supplied
  for that slot. Empty or policy-exhausted candidate sets return a
  typed Stage 13 insufficiency diagnostic with recovery choices instead of dropping a slot.
- `PlacementPlan` serializes selected sampling plans in slot-ID order, solver/version/seed, objective totals,
  pairwise decisions, validation, and QA views. Crop-reuse and repetition heatmap cells expose repeated source
  coverage alongside selected-placement, source-usage, objective, pairwise, and validation views.
- Raw crop overlap is evaluated only for candidates in the same source, prepared domain, and correspondence space.
  Heatmap normalization uses the prepared domain's declared dimensions and keys cells by both source and domain, so
  neither cross-domain coordinates nor the other selected crops can change a crop's heatmap location.

## Acceptance and stop-condition evidence

- The focused fixture gives two large unique-detail slots the same independently cheapest salient crop. The global
  solver rejects that pair and selects a distinct crop for one slot, proving the result is not independent cheapest
  selection or random-offset diversity.
- Separate fixtures prove stochastic overlap is accepted under its transform-aware reduced-penalty policy while a
  manufactured candidate with an explicit common period is recorded as intentional periodic reuse. The two policies
  remain distinguishable in the serialized pairwise breakdown.
- Repeating the solver with identical top-K inputs, settings, versions, and seed produces byte-identical complete
  `PlacementPlan` and objective-report JSON, including after caller input-slot permutation. Cancellation produces
  only `PlacementError::Cancelled`; missing Stage 12
  candidates produce explicit `InsufficientAssignment`.

## Focused verification

`cargo test -p hot-trimmer-placement-solver algorithm_stage_13_global_placement`

## Remaining later-stage work

Stage 14 consumes only validated Stage 13 sampling plans to execute registered channel sampling and synthesis. Later
preview/export stages render the QA records already carried by this authoritative plan; they must not recompute
placement or invent a second reuse policy.
