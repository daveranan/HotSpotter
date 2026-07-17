# Hot Trimmer V1 Full Algorithm Stack

**Status:** North-star production design, revision 1.1  
**Scope:** Full V1 destination architecture, not an MVP shortcut  
**Primary promise:** One or more source images become a coherent, reusable, hotspotted PBR trim sheet without non-uniform stretching, manual atlas authoring, or manual Blender metadata setup.

---

# 1. Core decision

Hot Trimmer V1 is a **template-driven material compiler**.

It does not merely paste an image into rectangles. It performs four distinct jobs:

```text
1. Understand and prepare source material images.
2. Decide which source content should supply each semantic template slot.
3. Synthesize slot content without aspect distortion or obvious repetition.
4. Generate structural profiles, PBR maps, IDs, metadata, and Blender assignments.
```

The complete path is:

```text
Source images
-> source analysis
-> material-domain construction
-> fixed template topology
-> slot demand model
-> crop and placement solver
-> per-slot synthesis
-> scale-constrained structural profile synthesis
-> scale-constrained detail synthesis
-> scale-aware effect compilation
-> PBR composition
-> atlas finishing and feature-LOD validation
-> Blender package and synchronization
```

The fixed template gives UV stability. The crop and synthesis solver determines what material content fills each slot. The profile system creates the manufactured trim shapes. The Blender companion applies those semantic slots to geometry.

---

# 2. Direct answer: does the algorithm determine how to crop the source?

**Yes, V1 must explicitly include a Source Placement Solver.**

The earlier template and SDF algorithms do not fully solve crop selection by themselves. They define where slots exist and how edges are generated. A separate algorithm must determine:

- Which part of the source image fills each slot.
- What crop dimensions are required.
- Whether the crop can be used directly.
- Whether it can repeat along one or two axes.
- Whether it must be synthesized into a larger field.
- Whether rotation or mirroring is legal.
- How to avoid repeating the same distinctive stain or crack everywhere.
- When the source is insufficient and the tool must report that instead of stretching it.

A source image can be 8,000 x 2,000 pixels. Its full-image aspect ratio does not constrain every slot. The solver searches for crop windows *inside* that image whose aspect ratio and physical footprint match the destination slot.

The central rule is:

> The source crop is chosen to match the destination aspect ratio before resampling. Non-uniform scaling is forbidden.

Uniform isotropic resampling is allowed because every image must eventually be sampled at the destination resolution. That changes pixel density, not geometry. Horizontal-only or vertical-only stretching is not allowed unless the user explicitly chooses Stretch mode.

---

# 3. Non-distortion contract

Every slot binding must choose one of these legal behaviors:

```text
Direct crop
Periodic tile
Repeat X
Repeat Y
Texture synthesis
Unique contain
Unique cover
Three-slice trim cap
Nine-slice panel
Planar radial fill
Polar radial synthesis
Explicit stretch override
```

Default rules:

1. **No non-uniform scaling.**
2. **Crop aspect must equal slot aspect** for unique rectangular fills.
3. **Uniform scaling only** for resolution and physical-scale matching.
4. **Repeating strips preserve cross-axis thickness.** Only the long axis repeats.
5. **Radial UV semantics do not imply radial warping of the material.**
6. **If a source cannot satisfy a slot, synthesize or reject. Do not silently distort.**
7. **Imported PBR maps share one sampling transform.** Base Color, Height, Normal, Roughness, Metallic, and AO never drift apart.
8. **Distinctive source features are globally rationed.** The same stain is not independently selected for every slot.

---

# 4. Full V1 stack at a glance

Hot Trimmer V1 consists of twenty algorithmic stages.

| # | Stage | Output |
| --- | --- | --- |
| 1 | Input ingestion and channel registration | Immutable registered source set |
| 2 | Color, alpha, and data normalization | Canonical source channels |
| 3 | Geometry and perspective correction | Rectified material exemplars |
| 4 | De-lighting and exposure normalization | Approximate intrinsic Base Color |
| 5 | Source-quality and material analysis | Source class and confidence data |
| 6 | Physical-scale and orientation calibration | Pixels-per-meter and direction fields |
| 7 | Feature-field extraction | Saliency, structure, stationarity, periodicity, seamability |
| 8 | Material-domain construction | Seamless or synthesizable material field |
| 9 | Template topology compilation | Exact semantic slot geometry |
| 10 | Slot demand and effect-capacity construction | Material footprint, physical geometry, raster budget, and legal effect vocabulary |
| 11 | Crop-candidate generation | Legal crop and synthesis candidates |
| 12 | Candidate scoring | Ranked candidates per slot |
| 13 | Global placement optimization | Non-repetitive assignment plan |
| 14 | Per-slot material synthesis | Registered slot-local PBR content |
| 15 | Scale-constrained structural profile synthesis | Legal bevel, groove, panel, radial, and fallback profile operations |
| 16 | Scale-constrained detail and pattern synthesis | Physically valid bolts, vents, motifs, caps, and user patches |
| 17 | PBR estimation and composition from compiled effects | Final Height, Normal, Roughness, Metallic, AO, and channel contributions |
| 18 | Scale-aware effect compilation and material-state synthesis | Role-specific, physically scaled, resolution-aware effects and weathering |
| 19 | Atlas finishing, feature LOD, and exact metadata | Supersampling, downsampling, bleed, mips, IDs, manifest, and survivability reports |
| 20 | Preview, Blender application, and QA | Validated material and effect behavior on hotspotted geometry |

The following sections define each stage.

---

# 5. Stage 1: input ingestion and channel registration

A Material Source is an immutable registered set:

```text
Base Color
Normal
Height
Roughness
Metallic
AO
Specular
Opacity
Edge Mask
Material ID
```

Requirements:

- Base Color anchors dimensions and orientation.
- Companion maps must match oriented Base Color dimensions.
- Original bytes remain immutable.
- Source ownership and original path remain separate.
- Color and data maps use different decoding rules.
- All maps receive the same crop and synthesis correspondence field.

The source set may be:

```text
A single photograph
A flat tileable texture
A scan
A screenshot
An existing PBR texture set
Multiple related exemplars
```

---

# 6. Stage 2: color, alpha, and data normalization

## Base Color

- Decode ICC profile.
- Convert into the working linear color space for computation.
- Preserve an sRGB display representation.
- Remove premultiplied-alpha ambiguity.
- Detect clipped highlights and crushed shadows.

## Scalar data

Treat Height, Roughness, Metallic, AO, masks, and IDs as linear data.

Do not apply display gamma.

## Normal data

- Decode tangent-space vectors.
- Record OpenGL or DirectX orientation.
- Normalize vectors.
- Detect invalid or nearly zero vectors.

## Resolution pyramid

Build a mip or Gaussian pyramid for every registered channel. Crop scoring, structure analysis, and interactive preview operate at lower resolutions before authoritative full-resolution rendering.

---

# 7. Stage 3: geometry and perspective correction

A source photograph may not be front-facing.

Use the existing patch rectification system to produce one or more planar exemplars:

```text
Four-point homography
Outline-assisted best-fit quadrilateral
Optional lens-distortion correction
Optional crop mask
```

The source-placement solver works on rectified exemplars, not arbitrary perspective-skewed photographs.

For a full-frame material photograph, allow the user or automatic analysis to define the usable planar area.

---

# 8. Stage 4: de-lighting and exposure normalization

Single photographs contain material reflectance plus lighting. V1 should provide multiple de-lighting routes.

## Classical low-frequency route

Estimate illumination:

```text
illumination = edge-preserving large-scale filter(Base Color)
```

Then estimate reflectance:

```text
reflectance = Base Color / max(illumination, epsilon)
```

or operate in log luminance:

```text
log_reflectance = log_luminance - low_frequency_log_luminance
```

Controls:

- Illumination radius.
- Strength.
- Shadow recovery.
- Highlight recovery.
- Color preservation.
- Edge preservation.

## Intrinsic-image route

A local learned estimator may predict reflectance and shading, but the result remains labeled Estimated.

## Highlight and shadow masks

Preserve masks for later use. A dark region might be dirt, pigment, or shadow. The application should retain confidence rather than pretending the decomposition is exact.

---

# 9. Stage 5: source-quality and material analysis

The algorithm router needs to know what kind of source it is dealing with.

## Source quality measurements

- Sharpness.
- Noise level.
- Compression artifacts.
- Dynamic range.
- Clipping.
- Perspective confidence.
- Usable-area mask.
- Map registration quality.
- Estimated material resolution.

## Material-behavior classes

V1 routes material preparation through one or more of these classes:

```text
Already tileable
Stochastic isotropic
Stochastic directional
Periodic or lattice structured
Layered or banded
Organic directional
Manufactured pattern
Unique detail
Radial detail
Mixed or unknown
```

Examples:

- Fine concrete: stochastic isotropic.
- Brushed metal: stochastic directional.
- Brick: periodic or lattice structured.
- Wood face grain: organic directional.
- Greek-key band: manufactured pattern.
- Vent: unique detail.
- Wood end grain: radial detail.

Classification may combine heuristics and a local model. The user can override it.

---

# 10. Stage 6: physical-scale and orientation calibration

A correct trim sheet needs material scale, not just pixel dimensions.

## Physical scale

Store:

```text
source_pixels_per_meter_x
source_pixels_per_meter_y
```

For isotropic scans:

```text
source_pixels_per_meter_x == source_pixels_per_meter_y
```

Scale may come from:

- Imported metadata.
- User measurement between two points.
- Known motif dimensions.
- Existing texture-set convention.
- Material-class prior as a low-confidence estimate.

Without physical scale, Hot Trimmer may use relative scale, but it must not claim world-size accuracy.

## Orientation field

Estimate a dominant local direction using gradients or a structure tensor.

The structure tensor for image gradients is:

```text
J = GaussianBlur([
    Ix * Ix, Ix * Iy,
    Ix * Iy, Iy * Iy
])
```

Its eigenvectors estimate dominant orientation. Use this for:

- Wood grain direction.
- Brushed metal direction.
- Layered stone.
- Horizontal weather streaks.
- Pattern alignment.

Store both global dominant orientation and local orientation confidence.

---

# 11. Stage 7: feature-field extraction

V1 builds analysis maps used by crop selection and synthesis.

## 11.1 Saliency map

Marks visually distinctive content:

- Large stains.
- Cracks.
- Bolts.
- Text.
- Strong color marks.
- Unique defects.

For generic surface slots, high saliency is often penalized because repeating a unique stain is obvious.

For a unique-detail slot, high saliency may be rewarded.

## 11.2 Structure map

Detect:

- Strong edges.
- Lines.
- Grid intersections.
- Brick courses.
- Board boundaries.
- Directional fibers.

Avoid crops that cut through important structures unless the slot is intended to contain them.

## 11.3 Stationarity map

Measure whether local statistics remain consistent around a location.

Useful local descriptors:

- Mean and variance.
- Color histogram.
- Gradient histogram.
- Frequency energy.
- Local binary pattern or similar texture descriptor.
- Material-map variance.

Stationary zones are good generic trim sources.

## 11.4 Periodicity map

Use autocorrelation or FFT peaks to estimate repeating periods and lattice vectors.

This is critical for:

- Brick.
- Tile.
- Corrugation.
- Rivet rows.
- Manufactured patterns.

## 11.5 Seamability map

Estimate how cheaply a crop can close or repeat at its boundaries.

Boundary descriptors compare:

- Color.
- Gradient.
- Height.
- Normal direction.
- Roughness.
- Structural edge crossings.

## 11.6 Usability mask

Exclude:

- Transparent/out-of-bounds areas.
- Severe highlights.
- Deep cast shadows.
- Occluders.
- Text or logos unless requested.
- Invalid PBR registration areas.

---

# 12. Stage 8: material-domain construction

The material domain is the source field from which slots are sampled. It can be the original image or a synthesized field.

V1 supports multiple domain builders.

## 12.1 Direct source domain

Use when the source is already clean and tileable.

## 12.2 Graph-cut periodic closure

Create a seamless tile by overlapping translated source regions and solving for a low-cost seam.

Per-pixel seam energy may be:

```text
E =
    wc * color_difference
  + wg * gradient_difference
  + wh * height_difference
  + wn * normal_difference
  + wr * roughness_difference
  + ws * structure_cut_penalty
```

Use one seam path or graph cut for all registered channels.

Normals are blended as vectors and renormalized.

IDs are never blended.

## 12.3 Texture quilting

Synthesize a larger domain from source patches with overlapping boundaries.

Candidate patch cost:

```text
Epatch =
    overlap_error
  + histogram_error
  + structure_error
  + duplicate_use_penalty
  + boundary_periodicity_error
```

Choose deterministic near-best candidates, then cut overlaps along minimum-cost seams.

## 12.4 PatchMatch synthesis

Build a nearest-neighbor field from output patches to source patches and optimize coherence across the output.

Useful for:

- Expanding unique but textured surfaces.
- Constrained completion.
- Avoiding obvious regular tiling.
- Rebuilding a larger exemplar from limited source content.

The correspondence field must be shared across every registered PBR channel.

## 12.5 Statistical or spectral synthesis

For stochastic materials, synthesize a new tile that matches source frequency or wavelet statistics.

Useful for:

- Concrete pores.
- Fine rust.
- Dirt.
- Plaster.
- Noise-like stone.

Do not use it for semantic patterns, text, bricks, or unique cracks.

## 12.6 Procedural material reconstruction

Some materials benefit from fitted procedural models:

- Wood grain and end grain.
- Brick lattice.
- Corrugation.
- Brushed metal.
- Concrete aggregate.
- Painted metal layers.

The source image estimates parameters, colors, frequency, scale, and noise distributions. The procedural model then produces arbitrary size and orientation without stretching.

## 12.7 Learned synthesis route

A local model may produce:

- De-lit material field.
- Seamless expansion.
- Super-resolution.
- Estimated height or normals.

The classical and procedural routes remain authoritative fallbacks. Learned outputs are labeled Estimated and remain deterministic when the model and seed are fixed.

---

# 13. Stage 9: template topology compilation

Standard V1 templates use fixed, versioned topology.

There is no runtime bin-packing for the default path.

## Canonical grid

Recommended:

```text
4096 x 4096 integer template units
```

Every unique boundary is scaled once to output pixels and reused by neighboring slots.

```text
pixel_x = round(canonical_x * output_width / 4096)
pixel_y = round(canonical_y * output_height / 4096)
```

If weighted bands are generated, use largest-remainder allocation so all bands sum exactly to the output dimensions.

## Template grammar

Template families may be authored with recursive horizontal and vertical splits, then compiled into exact fixed rectangles.

Example:

```text
Root
├── small horizontal trims
├── medium horizontal trims
├── large surface panels
└── detail zone
    ├── unique cells
    ├── cap cells
    └── radial cells
```

Released template versions pin the final integer rectangles.

---

# 14. Stage 10: slot demand and effect-capacity construction

Each compiled template slot becomes a **Resolved Slot Demand**. It contains both the material footprint required from the source and the procedural feature vocabulary the slot can physically and raster-wise support.

```text
slot_id
slot_role
hotspot_rect
allocation_rect
destination_pixel_width
destination_pixel_height
world_width_m
world_height_m
major_axis_m
minor_axis_m
aspect_ratio
pixels_per_meter_x
pixels_per_meter_y
meters_per_pixel_x
meters_per_pixel_y
desired_texel_density
mapping_mode
allowed_rotations
mirror_policy
material_group
variation_group
profile_type
weathering_class
visual_importance
minimum_survivable_feature_m
maximum_bevel_width_m
maximum_isotropic_feature_m
required_supersampling
supported_feature_lods
```

## 10.1 Material footprint

If physical source scale is known:

```text
required_source_width_px  = world_width_m  * source_pixels_per_meter_x
required_source_height_px = world_height_m * source_pixels_per_meter_y
```

This footprint preserves material scale.

If only destination texel density is known:

```text
world_width_m  = destination_width_px  / desired_texel_density
world_height_m = destination_height_px / desired_texel_density
```

Then derive source crop dimensions from source physical scale.

If crop target area in source pixels is `A` and slot aspect ratio is `r = width / height`:

```text
crop_width  = sqrt(A * r)
crop_height = sqrt(A / r)
```

This guarantees the crop aspect matches the slot before resampling.

## 10.2 Slot-local physical coordinates

Procedural effects are evaluated in slot-local physical coordinates, not directly in normalized rectangle coordinates.

For normalized slot coordinate `q`:

```text
m_x = q_x * world_width_m
m_y = q_y * world_height_m
```

The raster densities are:

```text
pixels_per_meter_x = destination_pixel_width  / world_width_m
pixels_per_meter_y = destination_pixel_height / world_height_m
```

A physical feature of width `s_m` becomes:

```text
feature_width_px_x = s_m * pixels_per_meter_x
feature_width_px_y = s_m * pixels_per_meter_y
```

This prevents a physically circular chip from becoming an ellipse merely because the destination slot is long and narrow.

## 10.3 Effect coordinate spaces

Every procedural effect declares its scale space:

```rust
pub enum EffectScaleSpace {
    World,
    SlotMinorRelative,
    SlotMajorRelative,
    SlotAreaRelative,
    Pixels,
}
```

Use them as follows:

- **World:** bevel width, chip radius, crack width, scratch width, bolt diameter, rust pits, dirt bands.
- **Slot-minor-relative:** cross-strip gradients, edge-to-center variation, broad treatment across thickness.
- **Slot-major-relative:** long-axis modulation, repeating interruption length, longitudinal fading.
- **Slot-area-relative:** sparse macro stains on broad panels.
- **Pixels:** bleed, dilation, antialiasing, minimum filter radius, ID-map safety.

Using only normalized coordinates is forbidden for physically meaningful features because it stretches effect geometry with slot aspect ratio.

## 10.4 Effect-capacity derivation

For each slot, derive an `EffectCapacity`:

```text
can_have_flat_center
maximum_left_profile_width_m
maximum_right_profile_width_m
maximum_top_profile_width_m
maximum_bottom_profile_width_m
maximum_isotropic_feature_m
maximum_radial_feature_m
minimum_full_height_feature_m
minimum_normal_only_feature_m
minimum_roughness_only_feature_m
recommended_supersample_factor
allowed_effect_variants
```

For a strip with minor physical dimension `W`, opposing profiles of widths `b0` and `b1`, and required flat center `f_min` are legal only when:

```text
W - b0 - b1 >= f_min
```

For symmetric bevels:

```text
b_effective = min(b_requested, (W - f_min) / 2)
```

If `W < f_min`, an ordinary flat-centered strip profile is impossible. The profile router must select a fully rounded, merged, normal-only, disabled, or explicitly failed variant.

## 10.5 Raster survivability

For a physical feature width `s_m`:

```text
s_px = s_m * pixels_per_meter
```

The renderer selects a representation level from a feature-LOD policy. A default ladder may be:

```text
more than 6 px:
    full Height + Normal + Base Color + Roughness + AO

3 to 6 px:
    simplified Height + Normal + Roughness

1.5 to 3 px:
    Normal + Roughness only

less than 1.5 px:
    Roughness/Color indication or omit
```

Thresholds are effect-specific and validated through golden fixtures.

## 10.6 Supersampling requirement

Choose internal supersampling from the narrowest active feature:

```text
1x, 2x, 4x, or 8x
```

A useful policy is to render until the narrowest requested analytic feature has at least four internal samples, bounded by memory and operation limits.

Supersampling improves raster fidelity. It does not make an effect physically valid when the slot is too small.

## 10.7 Worked capacity example

Assume a 2048 x 2048 atlas and a logical 64 x 64 template grid. One logical cell is 32 pixels. A slot measuring `0.5 x 10` cells becomes approximately:

```text
16 x 320 pixels
aspect ratio = 20:1
```

That slot may support:

- Micro-roughness.
- Small scratches.
- Thin edge wear.
- Longitudinal streaks.
- Interrupted chips along the long edges.
- A merged rounded profile if opposing bevels consume the width.

It may reject or transform:

- A 32 x 32 isotropic stain.
- Large circular chips.
- Broad central puddles.
- Square grunge masks.
- Two bevels that overlap and leave negative center width.

Stage 10 therefore answers two different questions:

```text
What material source footprint does this slot require?
What structural and weathering effects can this slot represent credibly?
```


# 15. Stage 11: crop-candidate generation

This is the first half of the Source Placement Solver.

## 15.1 Candidate window sizes

For each slot, generate crop sizes at legal isotropic scales:

```text
scale candidates = [0.5, 0.63, 0.8, 1.0, 1.25, 1.6, 2.0]
```

The actual range depends on source quality and physical-scale confidence.

For every scale `k`:

```text
crop_width  = round(required_width  * k)
crop_height = round(required_height * k)
```

The aspect remains unchanged because both axes use the same `k`.

Reject candidates that exceed the usable source area unless the candidate is explicitly a synthesis route.

## 15.2 Candidate positions

Generate windows using:

- Dense sliding windows at low resolution.
- Coarse-to-fine search.
- Feature-aware candidate centers.
- Saliency extrema.
- Stationary-zone sampling.
- Period-aligned positions for structured materials.
- Farthest-point sampling for diversity.

## 15.3 Candidate transforms

For each legal crop, test only permitted transforms:

```text
0°
90° if material orientation permits
180°
270° if permitted
mirror X if permitted
mirror Y if permitted
```

Directional materials normally forbid arbitrary 90° rotation.

## 15.4 Slot-specific candidate types

### Surface or panel

- Exact-aspect crop.
- Seamless tile crop.
- Quilted expansion.
- PatchMatch expansion.
- Procedural resynthesis.

### Repeat X strip

- Crop whose height matches strip thickness.
- One-dimensional repeat segment.
- Long contiguous crop.
- Graph-cut cyclic strip.
- Quilted strip sequence.

### Repeat Y strip

Equivalent with axes swapped.

### Unique detail

- Exact-aspect contain crop.
- Exact-aspect cover crop.
- Patch plus compatible base fill.
- Synthesis extension only if explicitly allowed.

### Trim cap

- Three-slice patch.
- Nine-slice bordered panel.

### Radial

- Square planar crop.
- Radial detail patch.
- Polar procedural generator.
- Annular structural profile over planar material.

---

# 16. Stage 12: candidate scoring

For slot `s` and candidate `c`, calculate a unary cost:

```text
E(s,c) =
    w_scale       * E_scale
  + w_resolution  * E_resolution
  + w_stationary  * E_stationarity
  + w_saliency    * E_saliency
  + w_structure   * E_structure
  + w_orientation * E_orientation
  + w_seam        * E_seam
  + w_boundary    * E_boundary_cut
  + w_quality     * E_quality
  + w_role        * E_role
  + w_synthesis   * E_synthesis_complexity
```

## 16.1 Scale cost

```text
E_scale = abs(log(candidate_physical_scale / requested_physical_scale))
```

## 16.2 Resolution cost

Penalize upsampling beyond the source-resolution confidence threshold.

## 16.3 Stationarity cost

Generic surface slots prefer statistically stable content.

Unique-detail slots may prefer a controlled amount of distinctive structure.

## 16.4 Saliency cost

For a generic repeating slot:

```text
E_saliency = mean_saliency(candidate)
```

For a unique-detail slot, invert or reshape the preference.

## 16.5 Structure cost

Penalize crops that cut through strong unresolved lines at the crop boundary.

Reward lattice alignment for brick or tile.

## 16.6 Orientation cost

Compare candidate dominant direction to slot direction:

```text
E_orientation = 1 - abs(dot(source_direction, slot_direction))
```

## 16.7 Seam cost

For repeatable candidates, compare opposite boundaries or solve a candidate graph-cut seam.

## 16.8 Role cost

Examples:

- A high-detail vent crop is bad for a generic concrete strip.
- A long manufactured border is good for a repeat strip.
- A square circular motif is good for a radial detail slot.

## 16.9 Synthesis complexity cost

Prefer direct legal crops over expensive synthesis when visual quality is comparable.

This prevents the application from using PatchMatch everywhere simply because it can.

---

# 17. Stage 13: global placement optimization

Independent best-crop selection is not enough. It repeatedly chooses the same attractive stain or texture zone.

The solver must optimize all slots together.

## 17.1 Pairwise repetition cost

For two candidates `c` and `d` assigned to slots `s` and `t`:

```text
P(c,d) =
    overlap_penalty
  + descriptor_similarity_penalty
  + repeated_salient_feature_penalty
  + identical_transform_penalty
  + variation_group_penalty
```

## 17.2 Complete objective

```text
TotalCost =
    sum_s UnaryCost(s, assigned_candidate_s)
  + lambda * sum_(s,t) PairwiseCost(assigned_candidate_s, assigned_candidate_t)
```

This is a quadratic assignment problem in the general case, so V1 uses a deterministic approximate solver.

## 17.3 Recommended practical solver

1. Generate the top `K` candidates for every slot.
2. Order slots by visual importance and constraint tightness.
3. Use beam search to assign candidates while tracking pairwise repetition cost.
4. Keep the best `B` partial assignments at each step.
5. Perform local swap and replacement optimization after the beam completes.
6. Use a fixed seed and stable tie-breaking.

Alternative exact or semi-exact routes may include integer programming for small templates.

## 17.4 Crop-reuse policy

Source overlap is not always bad.

- Stochastic concrete can reuse overlapping source regions with different transforms.
- A unique crack should not appear in five visible panels.
- Manufactured periodic patterns may intentionally repeat the same cycle.

The pairwise penalty depends on material class, saliency, and variation group.

---

# 18. Worked example: 8,000 x 2,000 source image

Assume a source image:

```text
width  = 8000 px
height = 2000 px
```

## Case A: square panel slot

Suppose the requested physical footprint requires a 1,600 x 1,600 source crop.

The solver slides a 1,600 x 1,600 square through the usable 8,000 x 2,000 image.

Possible horizontal crop range:

```text
x = 0 through 6400
```

Possible vertical crop range:

```text
y = 0 through 400
```

It evaluates many square candidates and selects one based on stationarity, saliency, structure, seamability, quality, and global diversity.

No stretching occurs. The selected square is uniformly resampled to the square destination slot.

## Case B: 10:1 horizontal trim

Suppose the slot requires a 4,000 x 400 source footprint.

The solver can choose a direct 4,000 x 400 crop anywhere that fits.

If the slot is longer than the available clean region, it may instead choose a 1,000 x 400 repeat segment and synthesize a seamless cycle along X.

The strip height is preserved. Only the long axis repeats.

## Case C: 1:10 vertical trim

Suppose the slot requires a 400 x 4,000 source footprint.

The source is only 2,000 pixels high.

Legal outcomes:

1. Rotate a 4,000 x 400 crop by 90° only if the material is orientation-neutral or rotation is explicitly allowed.
2. Synthesize a taller vertical field using directional quilting or PatchMatch.
3. Lower physical texel density through uniform scale if allowed.
4. Report insufficient source data.

Illegal outcome:

```text
Take 400 x 2000 and stretch it vertically to 400 x 4000.
```

## Case D: square radial concrete slot

Choose a square planar crop, then generate the circular or annular structural profile over it.

The concrete photograph is not polar-warped.

## Case E: wood end-grain radial slot

A face-grain crop is not valid.

Use:

- An end-grain source patch.
- A fitted procedural end-grain generator.
- Or an explicitly estimated radial synthesis route.

---

# 19. Stage 14: per-slot material synthesis

After global assignment, each candidate compiles into a Sampling Plan.

```text
sampling_plan
  source_id
  crop_rect
  isotropic_scale
  rotation
  mirror
  mapping_mode
  synthesis_mode
  correspondence_field
  repeat_period
  seam_path
  seed
```

## 19.1 Direct physical sampling

For slot-local normalized coordinate `q` and slot physical size `L`:

```text
m = (q.x * L.x, q.y * L.y)
source_position = transform(m) + crop_origin
```

All channels sample the same source position.

## 19.2 Periodic tile

```text
u = fract(source_position.x / tile_world_width)
v = fract(source_position.y / tile_world_height)
```

## 19.3 Repeat X

```text
u = fract(source_position.x / repeat_world_length)
v = clamp(source_position.y / strip_world_height)
```

## 19.4 Repeat Y

Swap axes.

## 19.5 Unique contain

Uniformly scale the source patch to fit inside the slot. Preserve the full patch and fill margins with the base material or transparency according to slot policy.

## 19.6 Unique cover

Uniformly scale until the slot is covered, then crop excess. Never distort aspect.

## 19.7 Three-slice cap

Divide the source patch:

```text
left cap | repeatable center | right cap
```

Preserve cap widths. Repeat or synthesize only the center.

## 19.8 Nine-slice panel

Preserve four corners and four edge strips. Repeat or stretch only the center according to explicit policy.

## 19.9 Planar radial

Use ordinary planar material sampling inside a radial hotspot.

## 19.10 Polar radial

For radial coordinate:

```text
r = length(p - center)
theta = atan2(p.y - center.y, p.x - center.x)
```

Map radial and angular coordinates into a procedural or authored radial source.

---

## Stage 14 integration checkpoint: intermediate atlas preview

Stage 14 is the first point at which the installed stack has both a fixed destination topology and actual
allocation-local material pixels. Before Stage 15 begins, Hot Trimmer composes those slot outputs into a typed,
non-exportable `IntermediateAtlasArtifact` so source selection, crop, scale, orientation, synthesis, and placement
can be evaluated visually.

The checkpoint is delivered in two gates so visibility is not delayed by production hardening:

1. **14P-A — first visible atlas.** Close known Stage 14 seam/slice correctness findings, install the single
   Stage 1-14 orchestration path in `AlgorithmCompiler`, compose one real persisted project, and display Base Color,
   available registered channels, slot boundaries, mapping mode, validity, correspondence, and concise lineage.
2. **14P-B — QA/cache hardening.** Complete invalidation, corpus coverage, heatmaps, rich decision inspection,
   performance bounds, cancellation latency, and cache qualification after truthful pixels are already visible.

Current integration reality is part of 14P-A's scope: the Prompt 00 facade still rejects Stage 1 and its
`CompilerRequestHeader` alone cannot execute the installed algorithms. The integration gate supplies sufficient
typed executable inputs to the sole facade and reuses Stages 1-14; it does not create another authority,
reimplement algorithms, or bypass their artifacts.

This artifact is not a partial `CompiledSheet`. It records:

- exact Stage 9 topology;
- Stage 13 PlacementPlan and per-slot SamplingPlan identity;
- exact Stage 14 slot-result identity and validity/correspondence;
- selected source patch and material-domain lineage;
- installed algorithm versions, settings, output, seed, revision, and diagnostics;
- `incomplete_after_stage = 14`.

Only channels genuinely available from registered Stage 14 material are shown. Profiles, semantic details,
effects, generated PBR content, finishing, mips, metadata, export, and Blender application remain pending and may
not be represented with plausible placeholder pixels.

The desktop exposes this as **Intermediate Stage 14 material-placement preview** with authoritative slot,
source-usage, mapping, correspondence, validity, and failure views. It is cancellable and revision-guarded. A
failed required slot, cancellation, or superseded revision publishes no partial atlas. The compositor may not use
a whole-sheet source cover, the removed legacy renderer, implicit Stretch, or center-cover recovery.

Stage 20 still owns production preview fixtures, complete QA, atomic export, and Blender synchronization. It
extends this feedback path; it does not introduce the first visual access to compiled material.

---

# 20. Stage 15: scale-constrained structural profile synthesis

Structural trim edges are generated mathematically from signed-distance fields, but a profile is evaluated only after a constraint solver resolves a legal representation for the slot.

The pipeline is:

```text
requested profile
-> convert widths to physical and pixel units
-> test semantic compatibility
-> test opposing-profile geometry
-> choose role-specific profile variant
-> choose feature LOD
-> choose supersampling
-> evaluate analytic profile
```

## 15.1 Rectangle signed-distance field

For center `c`, half-size `b`, and point `p`:

```text
q = abs(p - c) - b
sdf_rect = length(max(q, 0)) + min(max(q.x, q.y), 0)
```

Inside distance:

```text
d = -sdf_rect
```

## 15.2 Smooth bevel profile

Let:

```text
t = clamp(d / width, 0, 1)
height = A * (2t - t^2)
A = width * tan(angle) / 2
```

At the hotspot boundary, the slope equals `tan(angle)`. For a 45-degree edge:

```text
A = width / 2
```

## 15.3 Profile legality

For a slot minor dimension `W`, opposing profile widths `b0` and `b1`, and minimum center width `f_min`:

```text
W - b0 - b1 >= f_min
```

When the requested profile is illegal, choose one explicit outcome:

```text
Clamp profile widths
Use a fully rounded cross-section
Merge opposing profiles into one convex profile
Use a normal-only micro-bevel
Disable the profile
Return an incompatibility error
```

The choice is part of the compiled profile plan and appears in diagnostics.

## 15.4 Profile feature LOD

A profile may compile as:

```text
FullHeight
SimplifiedHeight
NormalOnly
RoughnessOnly
Disabled
```

The selected representation depends on physical scale, output resolution, mip-survival target, and slot importance.

## 15.5 Required profile programs

```text
Flat
Convex bevel
Concave groove
Rounded bevel
Double bevel
Raised lip
Recessed seam
Panel frame
Fully rounded strip
Merged opposing bevel
Radial disc
Annulus
Custom profile curve
```

## 15.6 Radial signed-distance fields

Disc:

```text
d = radius - length(p - center)
```

Annulus:

```text
outer_distance = outer_radius - length(p - center)
inner_distance = length(p - center) - inner_radius
```

Evaluate inner and outer profiles independently. Radial normal direction rotates continuously around the center.

## 15.7 Analytic filtering and supersampling

Render narrow profiles analytically where possible. Otherwise render at the compiled supersampling factor and downsample with a channel-correct area filter.

Do not solve sub-pixel geometry by widening it in final pixels, because that changes its physical scale.

## 15.8 Structural occupancy and edge authority

Stage 15 publishes more than a rendered profile preview. Its typed output retains:

```text
signed distance and inside/outside fields
flat-center and profile-exclusion masks
raised, recessed, cap, groove, and border occupancy
physical Height and analytic derivative contributions
profile identity, LOD, fallback, and physical dimensions
```

Later details and effects use those fields to declare `Above`, `Below`, `Conform`, `ClipInside`, `ClipOutside`, or
`Accumulate` relationships. They must not infer structure by reading flattened normal pixels.

An authored profile boundary is semantic material structure. An atlas allocation rectangle is only storage topology:
it is never automatically a bevel, seam, cavity, or wear edge. Real silhouette rounding remains geometry, displacement,
or an explicit Blender bevel policy; a texture profile cannot claim to alter the mesh silhouette.


# Reusable source and authoring library integration milestone (Prompt LIB)

The reusable library is product infrastructure between Stage 15 and Stage 16, not a new image-algorithm stage. It
provides stable source evidence, authored patches, and presets to the compiler without becoming a second compiler or a
pixel-composition path.

## LIB.1 Asset identity and kinds

Every project reference resolves through an immutable asset ID, version, and content digest. The initial asset kinds are:

```text
MaterialSourceSet
SourcePatchPreset
StampMask
RegisteredStampChannels
StampSheet
ProfilePreset
EffectRecipe
```

Material sources retain registered PBR channel identities and import settings. Patch presets retain authored crop,
rectification, registration, and calibration lineage. All assets retain provenance/license, tags/category/author,
source evidence, mask polarity where applicable, channel semantics, pivot, orientation, tileability, allowed physical
size/range, aspect policy, defaults, and preview metadata. User-global and project-local libraries share the same
contract. A project may embed a content-addressed snapshot for portability.

Filenames, absolute paths, thumbnails, and a mutable `latest` pointer are discovery conveniences, never compiler
authority. Editing creates a version; referenced content cannot silently change. Delete/replace is dependency-aware,
and an unresolved reference remains a typed, actionable failure rather than an empty substitute.

## LIB.2 Import and management window

Hot Trimmer imports a registered material set, single asset, folder, or atlas/stencil sheet. Import supports channel
association plus bounded automatic component/alpha segmentation, editable manual rectangles, shared registered-channel
crops, physical calibration and pivot authoring, and explicit color/scalar/vector/exact-ID roles. Scalar masks stay
linear and normal maps require a declared convention.

The Library window provides search/filter, thumbnail grid, type/tag/category views, registered-channel inspection,
physical/pivot defaults, provenance/license, version history, project usage, and dependency-safe import/edit/tag/
duplicate/replace/delete commands. Every mutation is a typed transactional command with cancellation and revision
guards. Thumbnail generation and segmentation are resource-bounded.

Stage 16 consumes only immutable `StampAssetRef`/preset references. Interactive painting, scattering, final PBR
composition, cloud synchronization, and marketplace behavior are outside this milestone.

After Stage 20 completes an atomic export, its exact manifest/checksum-pinned result may be published as a
`CompiledTrimPackage`. A mutable export folder or incomplete preview can never masquerade as a library package.


# 21. Stage 16: scale-constrained detail and pattern synthesis

Details are semantic overlays, not new topology by default. Every detail compiles through the same physical-size, applicability, feature-LOD, and supersampling rules as structural profiles.

## 16.1 Detail types

```text
Repeating strip
Unique detail
Radial detail
Trim cap
Bolt group
Vent
Panel stamp
Groove
Decal
Procedural motif
```

## 16.2 Detail contract

Every detail definition declares:

```text
physical width and height or allowed physical range
scale space
compatible slot roles
orientation policy
maximum supported slot aspect ratio
minimum feature pixels
repeat period
contain/cover policy
channel contributions
fallback policy
seed
```

Library-backed operations add:

```text
immutable asset ID/version/content digest
reusable-atlas or asset-specific-deferred scope
target slot/region
physical transform and pivot
rotation and mirror policy
opacity and channel-specific blend policy
clipping and profile-occupancy relationship
layer/dependency order
deterministic seed, spacing, scatter, and jitter
per-channel contributions and provenance
```

A `StampOperation` or `StampStroke` stores these deterministic parameters or committed physical samples, not pasted
display pixels. Screen coordinates may help author a stroke but never become authoritative placement coordinates.
Reusable-atlas stamps become part of the shared material. Asset-specific-deferred stamps remain manifest operations
for Stage 20/Blender and must not be baked into every object using that material.

Example:

```json
{
  "type": "edge_chips",
  "scaleSpace": "world",
  "chipRadiusMeters": [0.002, 0.008],
  "densityPerMeter": 5.0,
  "minimumMinorAxisMeters": 0.025,
  "minimumFeaturePixels": 2.5,
  "compatibleRoles": ["surface", "panel", "strip"],
  "fallback": "roughness_only"
}
```

## 16.3 Mask-to-SDF conversion

For an alpha or binary mask:

1. Compute distance inside the shape.
2. Compute distance outside the shape.
3. Subtract to obtain signed distance.
4. Evaluate bevel, groove, lip, or stamp profile from that distance.

This produces coherent Height and Normal contributions for:

- Greek keys.
- Bolts.
- Vent slots.
- Circular drains.
- Stamped panels.
- Decorative borders.

The output remains a registered mask/SDF, physical Height contribution, vector-normal input, scalar/color/ID
contribution, and lineage. It is not flattened into final PBR pixels at Stage 16. Material IDs are exact categorical
writes; Metallic changes require explicit legal intent; normal assets retain their declared convention.

## 16.4 Repeating motif

For repeat period `P_m` in physical units:

```text
motif_u = fract(local_major_axis_m / P_m)
```

Preserve motif physical scale unless the user explicitly chooses `Fit Pattern to Slot`.

A motif that is wider than the strip minor dimension must not be non-uniformly squeezed. The compiler may:

- Select a smaller authored variant.
- Use a simplified relief variant.
- Clip under an explicit policy.
- Assign the motif to a larger compatible slot.
- Reject the binding.

## 16.5 Role-specific detail variants

One logical effect family may provide several evaluators:

```text
Detail.Surface
Detail.HorizontalStrip
Detail.VerticalStrip
Detail.Radial
Detail.TrimCap
Detail.Unique
```

A bolt group for a panel is not the same evaluator as a repeated rivet line for a strip.

## 16.6 Detail feature LOD

A small chip or bolt may compile as:

```text
Full geometry contribution
Simplified Height/Normal
Normal-only mark
Roughness/Color mark
Disabled
```

The selected level is deterministic for slot geometry, output resolution, settings, and seed.

## 16.7 Radial stamps and mapping

A planar stamp on a radial cap remains planar by default, preserving circles and physical aspect. An explicitly polar
stamp may use radius/angle coordinates to wrap a ring or annulus. Polar/conformal mapping is an authored operation with
visible provenance; it is never introduced as an automatic fisheye effect to make a rectangular stamp fit.


# 22. Stage 17: PBR estimation and composition from compiled effects

Stage 17 does not decide whether an effect fits. It consumes already resolved `CompiledEffect` operations produced by Stage 18 and composes their channel contributions with imported or estimated material maps.

## 17.1 Compiled contribution model

A compiled effect provides:

```text
resolved evaluator
resolved physical scale
resolved pixel scale
role-specific variant
feature LOD
supersampling factor
channel targets
mask dependencies
fallback decision
```

## 17.2 Height

```text
final_height =
    material_height
  + sum(compiled_profile_height)
  + sum(compiled_detail_height)
  + sum(compiled_weathering_height)
```

Use explicit physical amplitudes, material-class ranges, and clamps. Do not add unrestricted 0-1 maps blindly.
Resolve profile, detail, stamp-relief, and weathering Height in the Stage 18 dependency order before deriving the final
generated normal. Stage 17 never guesses layer order from raster overlap.

## 17.3 Normal from height

Use physical pixel spacing:

```text
gx = dH/dx / meters_per_pixel_x
gy = dH/dy / meters_per_pixel_y
normal = normalize((-gx, -gy, 1))
```

Use Scharr gradients for rotational symmetry.

## 17.4 Imported normal composition

Decode vectors, combine using reoriented normal mapping or another vector-correct method, renormalize, and re-encode.

Never average normal RGB values.

Imported normal details are vector-composed only after Height-derived normals are available. Encoded normal RGB is
never alpha-blended. Color/alpha decals follow their declared straight/premultiplied policy separately from linear
scalar maps, exact IDs, and vector channels.

## 17.5 Roughness

Prefer imported Roughness.

Otherwise estimate using:

- Material-class base value.
- Local luminance.
- Local contrast.
- High-frequency detail.
- Explicit material labels.
- Compiled effect contributions.
- Material-state recipe.

Keep the result labeled Estimated.

## 17.6 Metallic

Default to zero.

Change only through:

- Imported map.
- Explicit metal material label.
- Explicit exposed-metal effect.
- Material ID rule.

## 17.7 AO and cavity

Estimate local cavity from multi-radius physical Height differences:

```text
cavity = clamp(blur_small(height) - blur_large(height) + bias, 0, 1)
```

Filter radii are specified in physical or scale-aware units and converted per slot.

## 17.8 Learned map estimation

A local model may estimate Height, Normal, or Roughness, but imported maps and explicit procedural structure take priority. Learned outputs remain Estimated and still pass through the same slot-local physical coordinate and feature-LOD system.


# 23. Stage 18: scale-aware effect compilation and material-state synthesis

Stage 18 is the authoritative router for structural effects, details, weathering, and material-state operations. Raw effect parameters are never rendered directly.

The compiler resolves each requested effect against:

```text
slot role
physical width and height
major and minor axes
aspect ratio
pixels per meter
output resolution
profile occupancy
material class
orientation
mip-survival target
visual importance
```

## 18.1 Effect contracts

```rust
pub enum EffectScaleSpace {
    World,
    SlotMinorRelative,
    SlotMajorRelative,
    SlotAreaRelative,
    Pixels,
}

pub enum EffectFallback {
    Disable,
    ClampScale,
    UseStripVariant,
    UseRadialVariant,
    UseCapVariant,
    RoughnessOnly,
    NormalOnly,
    Simplify,
    Error,
}

pub struct EffectApplicability {
    pub allowed_roles: Vec<SlotRole>,
    pub minimum_minor_axis_m: Option<f32>,
    pub maximum_aspect_ratio: Option<f32>,
    pub minimum_feature_pixels: f32,
    pub required_flat_center_m: Option<f32>,
}

pub struct EffectScale {
    pub space: EffectScaleSpace,
    pub primary: Range<f32>,
    pub secondary: Option<Range<f32>>,
}

pub struct EffectDefinition {
    pub effect_type: EffectType,
    pub scale: EffectScale,
    pub applicability: EffectApplicability,
    pub fallback: EffectFallback,
    pub seed: u64,
}

pub struct CompiledEffect {
    pub evaluator: EffectEvaluator,
    pub role_variant: EffectRoleVariant,
    pub physical_scale: Vec2,
    pub pixel_scale: Vec2,
    pub lod: EffectLod,
    pub supersample_factor: u8,
    pub channel_targets: ChannelTargets,
    pub fallback_reason: Option<EffectFallbackReason>,
}
```

Library-backed effects also retain their immutable asset version/content digest, declared scope, layer dependencies,
mask polarity, and channel-specific blend semantics. `EffectPlan` is an ordered dependency plan, not painter's-order
pixels.

## 18.2 Compilation order

```text
Resolve scale space
-> convert to physical dimensions
-> test semantic role compatibility
-> test physical fit
-> test interaction with profiles and other effects
-> convert to pixel dimensions
-> choose feature LOD
-> choose role-specific evaluator
-> choose supersampling
-> emit CompiledEffect or explicit failure
```

Compilation resolves `Above`, `Below`, `Conform`, `ClipInside`, `ClipOutside`, and `Accumulate` relationships against
Stage 15 occupancy. It validates physical Height units, bounded scalar operations, vector-normal convention, alpha,
exact IDs, and explicit Metallic legality. Conflicts, suppression, and fallbacks are diagnostic facts.

Reusable-atlas operations may render into the material sheet. Asset-specific-deferred operations are preserved for
Stage 20/Blender and never enter the shared atlas compositor.

Conceptual function:

```rust
fn compile_effect(
    effect: &EffectDefinition,
    slot: &ResolvedSlotDemand,
    occupied: &EffectOccupancy,
    output: &OutputSpec,
) -> Result<CompiledEffect> {
    let physical_scale = resolve_effect_scale(effect, slot)?;

    if !is_semantically_compatible(effect, slot) {
        return apply_fallback(effect, slot, IncompatibleRole);
    }

    if !fits_physical_bounds(effect, slot, occupied, physical_scale) {
        return apply_fallback(effect, slot, TooLargeForSlot);
    }

    let pixel_scale = physical_to_pixel_scale(physical_scale, slot);

    if !survives_resolution(effect, pixel_scale) {
        return apply_fallback(effect, slot, BelowPixelThreshold);
    }

    Ok(choose_role_specific_variant(
        effect,
        slot,
        physical_scale,
        pixel_scale,
    ))
}
```

## 18.3 Generated structural masks

```text
Region mask
Exposed-edge mask
Distance-to-edge field
Cavity mask
Raised-detail mask
Recessed-detail mask
Horizontal-up mask
Vertical mask
Downward mask
Radial inner-edge mask
Radial outer-edge mask
Decoration mask
Material-group mask
```

Distances are stored or evaluated in physical units where practical.

## 18.4 Role-specific weathering variants

Do not use one universal grunge evaluator.

```text
Grunge.Surface
Grunge.HorizontalStrip
Grunge.VerticalStrip
Grunge.Radial
Grunge.UniqueDetail
Grunge.TrimCap

EdgeWear.RectangularPanel
EdgeWear.HorizontalStrip
EdgeWear.VerticalStrip
EdgeWear.RadialOuter
EdgeWear.RadialInner
EdgeWear.TrimCap
```

These variants share recipe parameters and deterministic noise primitives but use different spatial logic.

## 18.5 Surface weathering

Broad panels may use approximately isotropic two-dimensional fields in physical coordinates:

```text
mask = structural_influence * noise_2d(m_x / lambda_x, m_y / lambda_y)
```

Large stains remain bounded by the panel's physical minor dimension and effect applicability.

## 18.6 Strip weathering

For a strip, define:

```text
u = physical coordinate along major axis
v = physical coordinate across minor axis
```

Use anisotropic correlation lengths:

```text
lambda_u >> lambda_v
```

Example edge wear:

```text
edge(v) = exp(-(distance_to_long_edge(v)^2) / (2 * sigma^2))
wear(u, v) = edge(v) * noise(u / lambda_u, v / lambda_v)
```

This creates long streaks, interrupted edge wear, thin dirt lines, and local chips without squeezing a square mask into a strip.

## 18.7 Radial weathering

For radial coordinate:

```text
r = length(p - center)
theta = atan2(p.y - center.y, p.x - center.x)
```

Effects may vary:

- Around circumference.
- From outer rim inward.
- From inner cavity outward.
- Along world-down direction.
- Around angularly localized damage.

## 18.8 Trim-cap weathering

Concentrate wear around:

- Cap boundary.
- Center-repeat transition.
- Exposed end face.
- Fastener or seam locations.

## 18.9 Multi-scale material state

A material-state recipe contains several physical scales:

```text
Micro:
    pores, fine scratches, roughness breakup

Meso:
    chips, pits, localized dirt

Macro:
    stains, fading, broad color variation

Structural:
    edge wear, cavity dirt, seams, directional streaking
```

For slot minor physical dimension `D_min`, an effect with characteristic size `s` is accepted only under its applicability rule, often of the form:

```text
s <= k * D_min
```

The coefficient `k` is effect-specific.

## 18.10 Feature LOD

Each effect family defines a representation ladder. Example for chips:

```text
more than 6 px diameter:
    full Height, Normal, Base Color, Roughness, AO

3 to 6 px:
    simplified Height, Normal, Roughness

1.5 to 3 px:
    Normal and Roughness

less than 1.5 px:
    Roughness/Color speck or omit
```

Increasing output resolution may promote an effect to a richer LOD without changing its physical placement or composition seed.

## 18.11 User-facing recipes

V1 may expose:

```text
Clean
Used
Heavy
Wet
Dusty
Chipped Paint
Rusting
Mossy
```

Internally these are bundles of effect definitions. They are compiled separately for each slot.

Therefore `Used` on a 32 x 32 panel and `Used` on a 0.5 x 10 strip share material intent but do not render the same masks or feature geometry.

## 18.12 Worked narrow-strip route

For a logical `0.5 x 10` strip that resolves to `16 x 320` pixels, a possible compiled route is:

```text
Profile:
    MergedRoundedStrip
    width = full minor axis
    LOD = FullHeight
    supersampling = 4x

Edge wear:
    HorizontalStripEdgeWear
    physical width = 4 mm
    LOD = NormalAndRoughness
    supersampling = 4x

Macro stain:
    rejected as isotropic
    fallback = UseStripVariant

Chips:
    radius = 2-4 mm
    placement along major edges
    LOD = NormalAndRoughness

Micro roughness:
    retained
```

The route and fallback decisions are deterministic and visible in diagnostics.


# 24. Stage 19: atlas finishing, feature LOD, and exact metadata

## 19.1 Padding and bleed

When possible, evaluate material, profiles, details, and weathering directly over the full allocation rectangle.

For finite unique details, use nearest-valid-pixel dilation or jump-flood propagation.

IDs never bleed.

## 19.2 Effect-aware supersampling and downsampling

Structural and weathering operations render at their compiled supersampling factor.

Downsample by channel:

- Base Color: color-managed area filtering.
- Height and scalar data: linear area filtering.
- Normal: decode vectors, area filter, renormalize.
- Roughness and Metallic: linear data filtering with range validation.
- IDs: nearest only, mip 0 exact.

Do not downsample normal-map RGB as ordinary color.

## 19.3 Channel-specific atlas filtering

- Base Color: color-managed filtering.
- Height and scalar data: linear filtering.
- Normal: vector-correct filtering.
- IDs: nearest only.

## 19.4 Exact Region ID

Region ID fills hotspot rectangles only.

No antialiasing, bleed, filtering, dithering, or color transform.

## 19.5 Material ID

Slots sharing one material label receive the same exact color.

## 19.6 Manifest

Export:

```text
Template ID and version
Compatibility key
Topology hash
Material revision
Slot IDs
Hotspot rectangles
Allocation rectangles
Slot roles
Fit rules
World sizes
Radial metadata
Map paths
Color spaces
Checksums
Compiled effect-route summary
Feature-LOD summary
Supersampling summary
```

The detailed development report may contain per-effect diagnostics. The runtime manifest should contain only data needed for validation, reproducibility, and Blender behavior.

## 19.7 Mip-survival validation

For each compiled feature, estimate or test survival at:

```text
mip 0
mip 1
mip 2
configured target viewing range
```

Validate that:

- Bevel highlights remain coherent.
- Thin strips do not collapse into unrelated colors.
- Normal bleed prevents seams.
- ID boundaries remain exact at mip 0.
- A simplified effect does not become stronger than its full-resolution version.

## 19.8 Feature diagnostics

Produce a deterministic compilation summary:

```text
17 effects rendered at full fidelity
4 simplified to Normal/Roughness
2 converted to strip variants
1 converted to radial variant
1 disabled because it could not fit
0 unresolved errors
```

The UI may summarize this without exposing every internal parameter. Developer diagnostics retain the full route table.


# 25. Stage 20: preview, Blender application, and QA

## Hot Trimmer preview

Test on:

```text
Plane
Cube
Cylinder
Beveled block
Wall module
Archway
Radial disc
Mechanical prop
```

At least several fixtures use authored hotspot UVs.

## Library-backed stamp authoring

Stage 20 integrates the Prompt LIB window and browser into profile/detail/effect authoring. The stamp/splat tool creates
typed Stage 16 operations with undo/redo, physical size, pivot, rotate/mirror, opacity, channel targeting, layer order,
and deterministic spacing/scatter/jitter. It can author reusable-atlas operations in 2D or asset-specific operations on
compatible 3D preview geometry.

Screen coordinates are transient. A committed 2D operation uses slot/atlas physical coordinates; a committed 3D
operation uses stable geometry/UV anchors with an explicit reprojection policy. Missing or changed geometry produces
reproject/orphan diagnostics instead of silently moving a stamp.

## Blender companion

The companion:

- Imports the manifest.
- Creates or updates the PBR material.
- Reads rectangular, strip, unique, cap, and radial semantics.
- Describes selected UV islands.
- Finds compatible slots.
- Fits without non-uniform distortion.
- Preserves locked assignments.
- Updates textures when material revision changes.
- Reports topology changes.
- Applies asset-specific deferred stamps as versioned decal/bake operations with stable anchors.
- Keeps real silhouette beveling in geometry/modifier/displacement policy rather than claiming texture allocation
  borders changed the mesh.

## QA views

```text
Base Color
Height
Normal
Roughness
Metallic
AO
Region IDs
Material IDs
Source-crop usage
Crop repetition heatmap
Seam energy
Texel density
Effect route
Effect occupancy
Feature LOD
Supersampling factor
Mip-survival warnings
Blender assignment status
```

---

# 26. Algorithm router

The full V1 does not force one synthesis algorithm or one effect evaluator onto every material and slot.

## 26.1 Material-domain and source-placement routes

| Material/source class | Preferred route | Fallback |
| --- | --- | --- |
| Existing tileable PBR set | Direct periodic sampling | Graph-cut closure |
| Fine concrete/plaster | Graph-cut + quilting | Statistical synthesis |
| Rust/grunge | Quilting + graph-cut | Spectral/statistical synthesis |
| Brushed metal | Direction-aware quilting | Procedural directional model |
| Brick/tile | Lattice detection + period-aligned crop | Procedural lattice reconstruction |
| Wood face grain | Direction-aware quilting | Procedural wood reconstruction |
| Wood end grain | Radial patch/procedural polar model | Estimated radial synthesis |
| Greek-key/manufactured border | Motif period detection + repeat strip | User-guided period |
| Unique vent/panel | Exact crop or contain/cover | PatchMatch expansion over base material |
| Radial drain/washer | Unique radial patch + annulus profile | Procedural radial detail |
| Mixed unknown | Candidate comparison across routes | User override |

The router may render low-resolution previews from several routes and score them before selecting a default.

## 26.2 Effect routes

The effect router compiles one material-state recipe differently for each slot class.

| Slot/effect context | Preferred evaluator | Common fallback |
| --- | --- | --- |
| Broad surface + grunge | Isotropic physical 2D field | Simplified macro variation |
| Horizontal strip + wear | Major/minor anisotropic strip field | Normal/Roughness-only wear |
| Vertical strip + streaks | Vertical directional strip field | Roughness/Color streak |
| Radial outer rim + wear | Radial-distance and angular field | Normal-only rim wear |
| Radial inner cavity + dirt | Inner-radius cavity field | AO/Roughness-only dirt |
| Trim cap + damage | Cap-boundary and transition field | Simplified edge mark |
| Sub-pixel chips | Normal/Roughness representation | Roughness/Color speck or omit |
| Overlapping bevels | Merged or fully rounded profile | Normal-only micro-bevel |

A compiled effect route records:

```rust
pub struct EffectRoute {
    pub evaluator: EffectEvaluator,
    pub scale_space: EffectScaleSpace,
    pub physical_scale: Vec2,
    pub pixel_scale: Vec2,
    pub role_variant: EffectRoleVariant,
    pub lod: EffectLod,
    pub supersample_factor: u8,
    pub fallback_reason: Option<EffectFallbackReason>,
}
```


# 27. Source Placement Solver pseudocode

```rust
fn solve_source_placements(
    template: &TemplateSnapshot,
    source: &PreparedMaterialDomain,
    output: OutputSpec,
    seed: u64,
) -> Result<PlacementPlan> {
    let demands = build_slot_demands(template, source, output)?;

    let mut candidates_by_slot = HashMap::new();

    for demand in &demands {
        let crop_sizes = generate_legal_crop_sizes(
            demand,
            source.physical_scale,
            source.quality,
        );

        let mut candidates = Vec::new();

        for crop_size in crop_sizes {
            for position in generate_candidate_positions(
                source,
                demand,
                crop_size,
            ) {
                for transform in legal_transforms(source, demand) {
                    let candidate = CropCandidate::new(
                        position,
                        crop_size,
                        transform,
                    );

                    if candidate.is_in_bounds(source.usable_mask)
                        && candidate.satisfies_aspect(demand)
                    {
                        let cost = score_crop_candidate(
                            source,
                            demand,
                            &candidate,
                        );

                        candidates.push((candidate, cost));
                    }
                }
            }
        }

        candidates.extend(generate_synthesis_candidates(
            source,
            demand,
        )?);

        stable_sort_and_keep_top_k(&mut candidates, 64);
        candidates_by_slot.insert(demand.slot_id, candidates);
    }

    let initial = deterministic_beam_search(
        &demands,
        &candidates_by_slot,
        seed,
    )?;

    let optimized = local_assignment_optimization(
        initial,
        &demands,
        &candidates_by_slot,
        source,
    )?;

    validate_no_nonuniform_stretch(&optimized)?;
    validate_registered_channel_mapping(&optimized)?;

    Ok(optimized)
}
```

---

# 28. Trim-sheet compilation pseudocode

```rust
fn compile_trim_sheet(
    template: &TemplateSnapshot,
    sources: &PreparedSources,
    placements: &PlacementPlan,
    bindings: &SlotBindings,
    decorations: &[DecorationBinding],
    material_state: &MaterialStateRecipe,
    output: OutputSpec,
) -> Result<CompiledMaps> {
    let geometry = scale_template_boundaries(template, output)?;
    let mut atlas = AtlasChannels::new(output);
    let mut compilation_report = EffectCompilationReport::default();

    for slot in &geometry.slots {
        let placement = placements.for_slot(slot.id)?;
        let binding = bindings.resolve(slot.id)?;

        let demand = resolve_slot_demand_and_effect_capacity(
            slot,
            binding,
            sources,
            output,
        )?;

        let base = synthesize_slot_material(
            slot,
            binding,
            placement,
            sources,
        )?;

        let effect_context = EffectContext::new(
            &demand,
            &base,
            material_state,
            output,
        );

        let compiled_profiles = compile_profile_effects(
            slot.profile_requests(),
            &effect_context,
        )?;

        let compiled_details = compile_detail_effects(
            slot,
            decorations,
            &effect_context,
        )?;

        let preliminary_masks = generate_structural_masks(
            slot,
            &compiled_profiles,
            &compiled_details,
            &effect_context,
        )?;

        let compiled_weathering = compile_weathering_effects(
            material_state,
            slot,
            &preliminary_masks,
            &effect_context,
        )?;

        let supersample_factor = maximum_required_supersampling(
            &compiled_profiles,
            &compiled_details,
            &compiled_weathering,
        );

        let rendered_profiles = render_compiled_effects(
            &compiled_profiles,
            supersample_factor,
        )?;

        let rendered_details = render_compiled_effects(
            &compiled_details,
            supersample_factor,
        )?;

        let rendered_weathering = render_compiled_effects(
            &compiled_weathering,
            supersample_factor,
        )?;

        let final_height = compose_height_fields(
            base.height,
            rendered_profiles.height,
            rendered_details.height,
            rendered_weathering.height,
        )?;

        let generated_normal = normal_from_physical_height(
            &final_height,
            slot.world_size,
        );

        let final_normal = compose_normals_vector_correct(
            base.normal,
            generated_normal,
            rendered_profiles.normal,
            rendered_details.normal,
            rendered_weathering.normal,
        );

        let composed = compose_pbr_channels(
            base,
            final_height,
            final_normal,
            rendered_profiles,
            rendered_details,
            rendered_weathering,
        )?;

        let resolved = downsample_compiled_slot_channels(
            composed,
            supersample_factor,
        )?;

        validate_feature_survival(
            slot,
            &resolved,
            &compiled_profiles,
            &compiled_details,
            &compiled_weathering,
            output,
        )?;

        compilation_report.record_slot(
            slot.id,
            &compiled_profiles,
            &compiled_details,
            &compiled_weathering,
        );

        atlas.write_allocation(slot, resolved)?;
        atlas.write_exact_region_id(slot)?;
        atlas.write_exact_material_id(slot, binding.material_id)?;
    }

    atlas.finish_bleed_and_mips()?;
    atlas.validate_registration()?;
    atlas.validate_exact_ids()?;
    atlas.validate_topology_hash(template)?;
    atlas.attach_effect_compilation_summary(compilation_report)?;

    Ok(atlas.finish())
}
```


# 29. Failure policy

The perfect version must be willing to say that the source is insufficient.

Examples:

```text
Source lacks enough vertical material for this orientation-locked strip.
No clean crop avoids the visible logo.
The brick period cannot close without cutting a brick.
The source does not contain end grain for the requested radial wood slot.
The requested texel density exceeds the source resolution by 3.2x.
```

Offer explicit recovery choices:

```text
Use synthesis
Allow 90° rotation
Lower texel density
Choose another source
Capture a patch
Use procedural reconstruction
Force stretch
```

`Force stretch` is an explicit override, never a silent fallback.

Effect-specific failures include:

```text
Requested bevels overlap and leave no legal center profile.
Requested chip diameter exceeds the slot minor dimension.
The selected isotropic stain is incompatible with a 20:1 strip.
The requested effect resolves below the minimum pixel threshold.
The radial treatment is bound to a rectangular-only slot.
The effect would not survive the configured mip target.
```

Offer explicit recovery choices:

```text
Clamp profile width
Use fully rounded profile
Use strip-specific variant
Use radial-specific variant
Simplify to Normal/Roughness
Increase output resolution
Disable effect for this slot
Choose a larger compatible slot
```

---

# 30. V1 acceptance criteria

## Crop correctness

- Every unique fill uses a crop whose aspect equals the destination aspect before resampling.
- Every default mapping uses isotropic scale only.
- Strip mappings preserve cross-axis thickness.
- Radial mapping preserves circular proportion.

## Source diversity

- Large visible slots do not reuse the same salient source feature unless explicitly requested.
- Variation decisions are deterministic for a fixed seed.
- A source-usage debug view shows which source regions supply which slots.

## Structured materials

- Brick crops align to complete lattice periods.
- Wood grain respects orientation.
- Manufactured motifs preserve period and physical size.

## Synthesis

- Registered PBR channels share one correspondence field.
- Seams are evaluated across relevant channels.
- Normal blends are vector-correct.

## Scale-aware effect compilation

Golden fixtures must cover:

```text
32 x 32 broad panel
0.5 x 10 horizontal strip
10 x 0.5 vertical strip
1 x 1 radial slot
thin trim cap
sub-pixel bevel
opposing bevels with no flat center
1K, 2K, 4K, and 8K atlas outputs
```

Required invariants:

- No isotropic physical effect is non-uniformly stretched.
- Physical feature scale remains stable across output resolutions.
- Extreme strips use anisotropic strip variants.
- Radial slots use radial evaluators when required.
- Opposing bevels cannot overlap accidentally.
- Sub-pixel features simplify deterministically.
- Increasing resolution may promote feature LOD without changing physical placement or seed.
- Effects that cannot fit are explicitly reported or resolved through a declared fallback.
- Same inputs, template, output, renderer version, and seed produce the same effect routes and outputs.
- Supersampling changes raster quality, not physical feature dimensions.
- Mip-survival validation catches disappearing or over-strengthened features.

## Template stability

- Material changes do not change topology.
- Resolution changes do not change normalized hotspots.
- Weathering changes do not change Blender assignments.
- Appearance-only effect-route changes do not change the topology hash.

## Blender

- Rectangular, strip, and radial fixtures map without non-uniform UV distortion.
- Locked assignments survive material updates.
- Topology changes produce explicit diagnostics.
- Effect and resolution updates reload maps without remapping UV islands.


# 31. Recommended build direction

The architecture should target the complete V1 from the beginning, even when implemented in slices.

The destination stack is:

```text
Fixed semantic templates
+ source analysis
+ physical scale
+ Source Placement Solver
+ global crop diversity optimization
+ multiple texture-synthesis routes
+ slot effect-capacity analysis
+ scale-constrained SDF structural profiles
+ scale-constrained semantic details
+ scale-aware effect compiler
+ vector-correct PBR composition
+ role-specific geometry-aware weathering
+ feature LOD and supersampling
+ mip-survival validation
+ exact IDs and manifest
+ first-party Blender companion
```

The first implementation may initially activate only some routes, but the contracts must already support all of them. Do not bake a simplistic crop, tile, normalized-mask, or one-weathering-texture assumption into the project model and plan to replace it later.

The two most important first-class plans are:

```text
SamplingPlan
EffectPlan
```

A complete `SamplingPlan` answers:

```text
Which source?
Which crop?
At what physical scale?
With what legal rotation or mirror?
Direct, repeated, quilted, PatchMatch, statistical, or procedural?
What correspondence field is shared across PBR maps?
What seam or repeat period is used?
How does this choice avoid duplicates elsewhere in the sheet?
```

A complete `EffectPlan` answers:

```text
Which effect evaluator is used for this slot role?
In what coordinate space is it defined?
What is its resolved physical scale?
What is its resolved pixel scale?
Does it fit beside the structural profiles?
Which feature LOD is legal?
What supersampling factor is required?
Which channels does it affect?
Which fallback was selected and why?
Will it survive the configured mip target?
```

These plans are the algorithmic bridge between raw source images, extreme trim geometry, and a finished sheet that remains physically and visually coherent across slot shapes and output resolutions.


# 32. Research foundations

The full V1 algorithm family draws from established texture-synthesis approaches:

- Alexei A. Efros and William T. Freeman, **Image Quilting for Texture Synthesis and Transfer**.
- Vivek Kwatra et al., **Graphcut Textures: Image and Video Synthesis Using Graph Cuts**.
- Connelly Barnes et al., **PatchMatch: A Randomized Correspondence Algorithm for Structural Image Editing**.
- Javier Portilla and Eero P. Simoncelli, **A Parametric Texture Model Based on Joint Statistics of Complex Wavelet Coefficients**.

Hot Trimmer should not implement these as isolated novelty demos. They are alternative engines behind one shared Sampling Plan and registered PBR pipeline.
