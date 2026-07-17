# Algorithm Stage 14 — registered per-slot material synthesis

## Delivered authority

Stage 14 is implemented by `synthesize_slot_material` in the sheet compiler. It accepts only a Stage 13
`SamplingPlan` whose domain/source/correspondence identities and dimensions match the supplied validated material
domain. A failed, malformed, cancelled, or resource-exceeding plan returns a typed Stage 14 failure; it never
executes center-cover, Stretch, or another mapping mode.

The executor covers DirectPhysical (the existing typed `SamplingMode::DirectCrop` plan), PeriodicTile, RepeatX,
RepeatY, TextureSynthesis, UniqueContain, UniqueCover, ThreeSliceCap, NineSlicePanel, PlanarRadial, PolarRadial,
and ExplicitStretch. Normalized output coordinates are first converted to slot-local physical coordinates.
Ordinary routes use one isotropic physical scale; repeats retain the declared physical period and cross-axis
thickness; contain/cover use a single fit scale; slice caps/corners retain their source widths. PlanarRadial uses
ordinary planar correspondence, while only PolarRadial converts radius and angle to source coordinates.

`SamplingPlan` now persists its authoritative `SamplingPolicy`, typed user-override provenance, slot physical size,
complete source-pixels-per-physical-unit coefficient, and explicit three-/nine-slice geometry. The physical
coefficient is derived at the Stage 13 handoff from Stage 10/6 scale and the selected Stage 11 dimensionless ladder
multiplier; those values are no longer treated as interchangeable. Lattice period remains separate from authored
cap widths and corner insets. ExplicitStretch is rejected without user provenance and is absent from failure
recovery choices. The policy's scale, filter, and tangent-normal correction flag are consumed by the raster path.

Selected Stage 8 seam references are executable inputs. Stage 14 checks their axis, full cross-axis path length,
crop containment, and normalized cost against the plan's declared threshold, then applies the selected path as a
row- or column-varying repeat phase. RepeatX consumes X seams, RepeatY consumes Y seams, and PeriodicTile may consume
both; unrelated routes reject seam references. Cross-axis path lookup uses the same texel-center convention as
channel and validity sampling: correspondence center `N + 0.5` selects path element `N`.

Slice centers have three typed outcomes. `Repeat` wraps only the authored center. `Synthesize` is accepted only for
a routed synthesized material domain whose center field already covers the requested physical center, and otherwise
fails with `InsufficientSynthesizedCenter` instead of publishing invalid pixels. Nine-slice
`ExplicitStretch` stretches only the center and requires visible typed user authorization; three-slice caps do not
permit center stretch. Every sliced axis must retain a strictly positive physical center after converting authored
cap/corner pixels through the plan scale; overlapping caps and zero/negative stretch denominators are rejected.

## Registration and channel semantics

One intermediate correspondence position is calculated per output pixel and shared by every registered channel.
Rotation and mirror are applied in physical coordinates before every mapping route. Imported normals are filtered as
vectors, renormalized, and transformed back through the inverse correspondence frame.
Base Color is filtered in its normalized linear representation, scalar maps and masks use scalar interpolation,
material IDs use categorical nearest selection, and normals use vector interpolation, renormalization, and the
selected mirror/quarter-turn transform. Domain validity is sampled into a separate slot-local validity plane.

Intermediate correspondence, validity, and channel planes are allocation-local and are published only after all
channels complete. Dimensions, pixels, operations, crop/period/slice/seam geometry, and tile size are validated;
cancellation is observed per row during correspondence and every channel raster, plus immediately before success. QA exposure
includes sampling coordinates, correspondence, registered markers, validity, and executed mapping mode.

## Focused evidence

The `algorithm_stage_14_slot_synthesis_acceptance` property test is discoverable through both documented filters. It
executes all twelve modes and every transform, compares registered color/scalar/ID markers at every valid pixel,
uses deliberately different physical and pixel units, checks isotropic Jacobians and circular detail, verifies cap
transitions and imported-normal rotation, asserts exact correspondence for all twelve rotation/mirror combinations
on a non-square slot, distinguishes planar and polar correspondence, proves SamplingPolicy changes execution,
executes a varying seam phase and rejects wrong-axis/short/over-threshold seams, covers Repeat, Synthesize, and
authorized nine-slice center stretch, observes mid-raster cancellation, enforces resource limits, rejects malformed
crop/period/slice/seam geometry, and rejects implicit Stretch.

## Deferred work

Stage 15 consumes these allocation/hotspot-local material channels when evaluating structural profiles. Sheet-wide
composition, compiled effects, finishing, export, and Blender integration remain owned by their later stages.
