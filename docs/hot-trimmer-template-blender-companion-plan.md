# Hot Trimmer Template Compiler and Blender Companion
## Consolidated implementation plan

**Status:** Execution plan  
**Purpose:** Replace the current generic Phase 3 layout direction with a template-driven hotspot-sheet compiler and make a first-party Blender companion part of the core product.  
**Primary user promise:** One or more material images in, a usable hotspotted PBR trim-sheet material on selected Blender geometry out.

---

# 1. Executive decision

Hot Trimmer is not primarily a generic trim-sheet packing application.

The default product must be:

```text
Open material image
-> choose a proven hotspot template
-> choose surface condition
-> optionally add details
-> build
-> send to Blender
-> selected geometry is hotspotted and textured
```

The current generic layout solver, freeform boundary editing, and atlas-packing controls are not deleted, but they are demoted to an **Advanced Custom Atlas** workflow.

The normal workflow is **template instantiation and material compilation**.

The Blender workflow is not delegated to Zen UV, DreamUV, or manual UV editing. Hot Trimmer ships with its own focused Blender companion that consumes Hot Trimmer metadata, creates the material, fits UV islands into compatible hotspots, handles radial slots, and updates when the Hot Trimmer project changes.

This plan supersedes the current default Phase 3 direction and narrows the Blender portion of Phase 6 into a first-party product requirement.

---

# 2. Product invariant

The app and Blender companion share one authoritative contract:

```text
Template topology
+ material sources
+ optional patches/details
+ structural profiles
+ weathering recipe
+ stable hotspot metadata
= versioned Hot Trimmer package
```

Hot Trimmer owns the package.

The Blender companion consumes the package and applies it to geometry.

The following must remain stable when the user changes concrete to rusted steel, changes weathering, regenerates normals, or increases output resolution:

- Template identity.
- Template compatibility key.
- Hotspot rectangles.
- Stable slot IDs.
- Region IDs.
- Region ID colors.
- Slot types.
- Radial tags.
- Fit-axis rules.
- World-size metadata.
- Existing Blender UV assignments.

Only content maps and material appearance should change.

If topology changes, the compatibility hash changes and Blender must report which assignments remain valid, which can be remapped, and which locked assignments are broken.

---

# 3. The user experience

## 3.1 Default Hot Trimmer path

The minimum successful flow is:

```text
1. Open a concrete image.
2. Choose "Generic Architecture."
3. Choose Clean, Used, or Heavy.
4. Press Build Trim Sheet.
5. Preview on a pre-hotspotted wall module.
6. Press Send to Blender.
7. The selected Blender objects receive the material and hotspot UV assignments.
```

No patch capture is required for a useful first result.

No user-facing knowledge of these concepts is required:

- Atlas packers.
- Hotspot metadata.
- Region ID encoding.
- Fit axes.
- Insets.
- Bleed.
- Radial tags.
- UV-island scoring.
- Normal-map boundary math.
- Template compatibility hashes.

## 3.2 Optional detail path

The user may add extra source images or captured patches and classify each with one simple intent:

```text
Repeating Strip
Unique Detail
Radial Detail
Trim Cap
```

Examples:

```text
Greek-key photograph
-> Repeating Strip
-> decorative strip slots

Vent photograph
-> Unique Detail
-> bounded panel/detail slots

Drain-cover photograph
-> Radial Detail
-> radial square slots

Decorative end panel
-> Trim Cap
-> cap slots
```

Hot Trimmer suggests compatible slots and previews the result. It never silently changes template topology.

## 3.3 Blender path

The Blender companion provides:

- Import or connect to Hot Trimmer project.
- Create or update Blender material.
- Auto-map selected objects.
- Auto Fit selected UV islands.
- Next Compatible hotspot.
- Previous Compatible hotspot.
- Use Selected Hotspot.
- Rotate 90°.
- Mirror.
- Match Texel Density.
- Lock Assignment.
- Classification: Auto, Rectangular, Radial.
- Re-sync Material.
- Open Project in Hot Trimmer.

The plugin includes a visual hotspot browser. Clicking a hotspot assigns the selected UV island to that hotspot.

The plugin may change which hotspot a mesh island uses. It does not normally edit the sheet topology. Hot Trimmer remains the only authority for adding, removing, moving, or resizing template slots.

---

# 4. System boundaries

## 4.1 Hot Trimmer desktop application owns

- Projects and persistence.
- Material-source sets.
- Registered PBR input maps.
- Patch capture and rectification.
- Versioned template registry.
- Template instantiation.
- Slot bindings.
- Detail and decoration bindings.
- Structural profile generation.
- Weathering recipes and masks.
- Base Color, Height, Normal, Roughness, Metallic, AO, ID generation.
- Exact hotspot metadata.
- Export package creation.
- Revision publishing.
- Topology compatibility decisions.

## 4.2 Blender companion owns

- Blender-side package import.
- Material node creation and map assignment.
- Image reload/update.
- UV-island extraction and descriptors.
- Rectangular, strip, unique, cap, and radial fitting.
- Hotspot matching.
- Texel-density scaling.
- Manual candidate cycling.
- Assignment locking and local overrides.
- Object, mesh, material, and image metadata.
- Sync polling and update reports.
- Blender-facing diagnostics.

## 4.3 Shared contract owns

- Project ID.
- Material ID.
- Template ID and version.
- Template compatibility key.
- Template snapshot hash.
- Material revision.
- Slot IDs and stable region IDs.
- Hotspot rectangles.
- Allocation rectangles.
- Slot roles.
- UV-fit rules.
- Content-mapping rules.
- World sizes.
- Insets.
- Radial metadata.
- Map filenames, color spaces, and checksums.

## 4.4 External tools

Zen UV, DreamUV, and Trim View are design and behavior references only.

Do not create a runtime dependency on them.

Do not copy their source code or bundled assets unless a separate license review explicitly permits it. Reimplement the required behavior as a focused Hot Trimmer companion.

---

# 5. Core concepts that must remain separate

| Concept | Responsibility |
| --- | --- |
| Template topology | Where hotspots are and what each hotspot means |
| Material source | Concrete, metal, brick, wood, plaster, and registered PBR inputs |
| Structural profile | Bevels, seams, grooves, frames, lips, circular rims |
| Decoration | Bolts, vents, patterns, stamps, panels, user patches |
| Weathering recipe | Edge wear, dirt, streaks, polish, chipping, roughness variation |
| DCC metadata | Fit kind, fit axis, radial classification, world size, inset, rotation rules |
| UV assignment | Which Blender UV island uses which hotspot |

Never compress all of these into a generic `LayoutRegion`.

---

# 5.1 Terminology and user-facing language

Use these terms consistently:

- **Source**: an immutable imported material input and its registered PBR maps.
- **Patch**: an optional authored source asset or isolated detail captured from a source. A patch
  is reusable content; it is not a location on the final sheet.
- **Template slot**: an internal, stable topology definition emitted by a template or layout
  generator. `TemplateSlot` may remain in Rust, persistence, manifests, and compatibility logic.
- **Region**: the user-facing instantiated area on a compiled trim sheet. A region has stable
  sheet bounds and a source-layer recipe. All desktop and Blender UI calls it a region.
- **Source layer**: the executable mapping that samples a source or patch into a region, including
  framing, projection, UV warps, blend policy, and channel-consistent transforms.
- **Decoration**: optional content layered into or over a region without redefining its stable
  sheet bounds.

Do not use `slot`, `patch`, and `region` as synonyms. In particular, selecting a region on the
right edits its source layer on the left; it does not convert the region into a patch.

---

# 6. Layout kinds

Introduce two explicit layout kinds:

```rust
pub enum LayoutKind {
    Template,
    CustomAtlas,
}
```

## Template

The normal product path.

Properties:

- Uses a pinned, versioned topology.
- Slot rectangles are not freely moved in basic mode.
- Hotspot identity is stable.
- Supports material swapping without UV changes.
- Supports deterministic DCC metadata.
- Supports one-click Blender application.
- May originate from a deterministic procedural layout generator, but is pinned as an immutable
  template snapshot before it is used for Blender assignments.

## CustomAtlas

The advanced path.

Properties:

- Uses the existing generic packer and freeform region editing.
- May auto-pack arbitrary patches.
- May permit boundary dragging and manual region creation.
- Has a separate compatibility key.
- Cannot claim compatibility with a standard template after topology changes.

Rename existing layout infrastructure accordingly:

```text
Current generic layout engine
-> CustomAtlasLayoutEngine
```

Do not continue improving it as the default Create Trim Sheet path.

---

# 7. First-class template model

## 7.1 Canonical coordinates

The authoritative template representation uses an integer canonical grid, not floating-point normalized bounds.

Recommended canonical grid:

```text
4096 x 4096 template units
```

Use half-open rectangles:

```text
[left, top, right, bottom)
```

Benefits:

- Deterministic boundaries.
- Stable scaling to 1K, 2K, 4K, and 8K.
- Exact ID maps.
- No float drift.
- Straightforward overlap validation.
- Reproducible golden fixtures.

Normalized UV coordinates are derived at export time.

## 7.2 Template identity

Every template contains:

```text
template_id
template_version
compatibility_key
schema_version
canonical_width
canonical_height
default_output_resolution
reference_resolution
reference_texel_density
reference_bevel_width
display_name
intended_asset_family
slots
```

Example:

```json
{
  "schemaVersion": 1,
  "templateId": "ht.generic_architecture",
  "templateVersion": "1.0.0",
  "compatibilityKey": "ht.generic_architecture.topology.v1",
  "displayName": "Generic Architecture",
  "intendedAssetFamily": "architecture_and_modular_props",
  "canonicalGrid": [4096, 4096],
  "referenceResolution": [2048, 2048],
  "defaultOutputResolution": [2048, 2048],
  "referenceTexelDensityPxPerMeter": 512,
  "slots": []
}
```

## 7.3 Template snapshot

Projects pin a complete snapshot of the instantiated template.

Persist:

```text
template_id
template_version
compatibility_key
template_snapshot_json
template_snapshot_hash
```

Never silently reinterpret an old project through a newly shipped template definition.

A user may explicitly upgrade a template. The app must preview the consequences and produce a new snapshot and hash.

## 7.4 Template slot definition

Each slot requires these groups of data:

```text
Identity
Geometry
Semantic role
Material binding
UV fitting
Content mapping
Structural profile
Weathering class
Variation behavior
DCC metadata
Stable IDs
```

Conceptual JSON:

```json
{
  "slotId": "horizontal_trim_03",
  "displayName": "Horizontal Trim 03",

  "allocationRect": [0, 520, 4096, 760],
  "hotspotRect": [8, 528, 4088, 752],

  "role": "strip",
  "materialGroup": "primary",
  "variationGroup": "horizontal_trims",

  "uvFit": {
    "kind": "rectangular",
    "fitAxis": "vertical",
    "keepProportion": true,
    "allowedRotations": [0, 180],
    "mirrorAllowed": true,
    "worldSizeMeters": [2.0, 0.12],
    "classificationTags": ["HOTSPOT"]
  },

  "contentMapping": {
    "kind": "repeat_x",
    "orientation": "horizontal",
    "anchor": "center"
  },

  "profile": {
    "kind": "bevel_band",
    "edges": ["top", "bottom"],
    "widthPxAtReference": 10,
    "angleDegrees": 45
  },

  "weatheringClass": "exposed_strip",
  "defaultSeed": 17422,

  "regionId": "stable-uuid",
  "regionIdColor": [61, 147, 233]
}
```

Radial example:

```json
{
  "slotId": "radial_large_01",
  "displayName": "Radial Large 01",

  "allocationRect": [0, 3584, 512, 4096],
  "hotspotRect": [8, 3592, 504, 4088],

  "role": "radial",
  "materialGroup": "primary",
  "variationGroup": "radial_large",

  "uvFit": {
    "kind": "radial",
    "fitAxis": "none",
    "keepProportion": true,
    "allowedRotations": [0],
    "mirrorAllowed": false,
    "worldSizeMeters": [0.4, 0.4],
    "classificationTags": ["HOTSPOT", "Radial"]
  },

  "contentMapping": {
    "kind": "planar"
  },

  "profile": {
    "kind": "annulus",
    "innerRadiusNormalized": 0.25,
    "outerRadiusNormalized": 0.47,
    "edgeWidthPxAtReference": 8
  },

  "weatheringClass": "radial_exposed",
  "defaultSeed": 55201,

  "regionId": "stable-uuid",
  "regionIdColor": [212, 82, 119]
}
```

## 7.5 Required slot enums

Suggested Rust contracts:

```rust
pub enum SlotRole {
    Surface,
    Strip,
    Panel,
    UniqueDetail,
    Radial,
    TrimCap,
    Utility,
}

pub enum UvFitKind {
    Rectangular,
    Strip,
    Radial,
    Unique,
    Cap,
}

pub enum FitAxis {
    None,
    Horizontal,
    Vertical,
    Automatic,
}

pub enum ContentMappingKind {
    Planar,
    RepeatX,
    RepeatY,
    TileXY,
    Stretch,
    Polar,
    Unique,
}

pub enum ProfileKind {
    Flat,
    ConvexBevel45,
    ConcaveGroove45,
    RoundedBevel,
    DoubleBevel,
    RaisedLip,
    RecessedSeam,
    PanelFrame,
    RadialDisc,
    Annulus,
    CustomProfile,
}

pub enum WeatheringClass {
    Neutral,
    ExposedEdge,
    Recessed,
    HorizontalExposed,
    VerticalExposed,
    GroundFacing,
    RadialExposed,
    Decorative,
}
```

---

# 8. Allocation rectangles and hotspot rectangles

Every slot has two rectangles:

```text
allocation_rect
hotspot_rect
```

## Allocation rectangle

Contains:

- Rendered content.
- Padding.
- Dilation.
- Bleed.
- Mipmap safety gutter.

## Hotspot rectangle

Contains:

- The exact UV fitting bounds.
- The structural profile boundary where UV edges are expected to land.
- The rectangle exported to Blender.

Rules:

- `hotspot_rect` must be fully inside `allocation_rect`.
- Beauty and data maps may bleed through the allocation gutter.
- Region ID uses the hotspot rectangle only.
- DCC metadata uses the hotspot rectangle only.
- Changing padding or output resolution must not change normalized hotspot bounds.
- The normal/profile compiler must preserve the intended bevel direction exactly at the hotspot boundary.

This separation prevents ID-mask reconstruction from inflating a hotspot by the bleed width.

---

# 9. Stable region identity and ID colors

## 9.1 Stable region IDs

For template layouts:

```text
region_uuid = UUIDv5(layout_instance_uuid, slot_id)
```

Persist the result.

## 9.2 Region ID colors

Allocate exact 24-bit colors once and persist them.

Do not rely only on truncating UUID hashes because collisions are possible.

Validation must reject duplicate colors.

## 9.3 Region ID output rules

The Region ID map contains:

```text
one background color
+ one exact flat color per enabled hotspot
```

Export rules:

- PNG or another lossless integer format.
- No antialiasing.
- No filtering.
- No dithering.
- No color management.
- No lossy compression.
- No interpolation at region edges.
- No bleed into neighboring hotspot IDs.
- Background remains exact.

## 9.4 Material ID

Material ID is separate from Region ID.

Multiple slots may share one material label and therefore one Material ID color.

Metallic generation may use explicit material labels. It may not infer metal from the appearance of Base Color.

---

# 10. Material sources, patches, slots, and decorations

## 10.1 Material source responsibility

A material source describes material content and registered maps:

- Base Color.
- Normal.
- Height.
- Roughness.
- Metallic.
- AO.
- Specular.
- Opacity.
- Edge Mask.
- Material ID.

It may provide:

- Physical scale.
- Orientation policy.
- Tiling capability.
- Material label.
- Imported map provenance.

## 10.2 Patch responsibility

A patch describes extracted content:

```text
source reference
rectification geometry
rectified dimensions
can tile X
can tile Y
preferred physical size
orientation policy
material label
alpha or mask
map-generation participation
```

A patch does not own:

- Hotspot rectangle.
- Padding.
- Bleed.
- Fit axis.
- Radial tag.
- Template world size.
- Slot role.

Those belong to the destination slot.

## 10.3 Slot responsibility

A slot describes:

```text
where the content goes
how UV islands fit
how content repeats
how the structural profile behaves
how weathering treats the region
what Blender metadata is exported
```

## 10.4 Slot binding

A slot binding connects a slot to content:

```rust
pub enum SlotFillKind {
    MaterialSource,
    Patch,
    SolidFill,
    ProceduralMaterial,
}

pub struct SlotBinding {
    pub slot_id: SlotId,
    pub fill_kind: SlotFillKind,
    pub source_id: Option<MaterialSourceId>,
    pub patch_id: Option<PatchId>,
    pub transform: SlotContentTransform,
    pub seed: u64,
}
```

## 10.5 Decoration binding

Decorations layer on top of a slot without replacing the base material:

```text
bolt group
vent
Greek-key pattern
panel stamp
user patch
radial cap
seam
label
```

A decoration can contribute to:

- Base Color.
- Height.
- Normal.
- Roughness.
- Metallic.
- AO.
- Opacity.
- Masks.

Conceptual contract:

```rust
pub struct DecorationBinding {
    pub id: DecorationId,
    pub slot_id: SlotId,
    pub source: DecorationSource,
    pub placement: DecorationPlacement,
    pub transform: DecorationTransform,
    pub channel_blends: ChannelBlendSet,
    pub seed: u64,
}
```

## 10.6 Detail intents

The default UI exposes four intents only:

```text
Repeating Strip
Unique Detail
Radial Detail
Trim Cap
```

The app maps those intents to compatible slot roles.

The app may suggest classification using aspect ratio, alpha shape, or source metadata, but it must not silently decide radial semantics.

---

# 11. Radial semantics

Radial UV fitting and radial material generation are independent.

Store them independently:

```text
uv_fit.kind
content_mapping.kind
```

Examples:

| Use case | UV fit | Content mapping |
| --- | --- | --- |
| Concrete circular cap | Radial | Planar concrete |
| Wood end grain | Radial | Polar procedural wood |
| Circular vent | Radial | Unique detail |
| Washer or rim | Radial | Procedural annulus |
| Pipe side | Strip/rectangular | Repeat X |

A circular concrete cap does not require polar-warping a concrete photograph.

Content projection is an independent authoring choice. A radial UV-fit region may use an
unwarped source, and a rectangular region may use a polar, spiral, lens, or other UV warp.

## 11.1 Radial profile math

For radial slots:

```text
p = normalized pixel coordinate relative to slot center
r = length(p)
theta = atan2(p.y, p.x)
```

Annulus masks:

```text
inner_distance = r - inner_radius
outer_distance = outer_radius - r
```

Generate:

- Inner bevel.
- Outer bevel.
- Flat ring.
- Concave inner rim.
- Convex outer rim.
- Radial edge and cavity masks.

Normal directions rotate continuously around the center. Do not wrap a horizontal normal strip around a circle.

## 11.2 Region source projection and UV warp stack

A selected region owns a versioned, deterministic **UV warp stack** that maps region-local
coordinates to source UVs before sampling. Projection is independent of region shape, UV-fit
classification, structural profile, and material type. It changes how source content is mapped
into the final trim-sheet region; it does not cut or reshape the region.

Process coordinates in this order:

```text
region-local UV
  -> source crop or perspective-quad rectification
  -> ordered UV warp operations
  -> scale / rotate / mirror / offset
  -> clamp / repeat / mirrored-repeat addressing
  -> sample source maps
  -> clip and composite into the final trim-sheet region
```

The first implementation supports these warp operations:

- **Planar**: direct rectangular mapping.
- **Perspective**: homography from an editable four-point source quad.
- **Polar**: map one source axis around an editable center and the other along radius.
- **Spiral / Twirl**: rotate UV angle as a function of radius.
- **Radial Lens**: barrel, pincushion, or fisheye-style radial displacement.
- **Cylindrical / Arc**: bend one source axis around a configurable arc.

Warp operations are composable rather than mutually exclusive presets. For example, a user may
rectify a perspective quad, apply radial-lens correction, then add a twirl. Each operation has
an enabled flag and typed parameters so later versions can add bend, spline, or flow-map warps
without changing the region contract.

Common controls include:

- Warp center and optional center gizmo.
- Strength, falloff, radius, bias, and clamp range.
- Angle, seam position, turns/twist, and arc span.
- Source-axis choice, scale, offset, rotation, mirror, and address mode.
- Operation order, reset, duplicate, enable/disable, and remove.
- A live left-source and right-region preview using the exact compiled transform.

Every registered map follows the same coordinate mapping. Tangent-space Normal vectors must be
transformed using the local Jacobian of the complete warp stack before composition with
structural normals; sampling Normal RGB through warped UVs without reorientation is invalid.
Warp singularities, excessive derivatives, and out-of-range samples must produce visible
diagnostics rather than NaNs or silent corruption.

---

# 12. Template registry and template packs

## 12.1 Ownership

The Rust domain layer owns:

- Template loading.
- Template validation.
- Template snapshot creation.
- Template IDs and versions.
- Slot geometry.
- Stable IDs.
- Compatibility keys.
- Hashing.
- Coordinate conversion.

TypeScript displays template data and sends commands. It must not maintain an independent template truth.

## 12.2 Package structure

Recommended:

```text
assets/templates/
  generic_architecture/
    1.0.0/
      manifest.json
      preview.webp
      guide.svg
      fixture.glb
      expected-region-id.png
      expected-profile-height.png
      expected-profile-normal-opengl.png
      expected-profile-normal-directx.png
```

## 12.3 Validator

Reject:

- Duplicate template IDs and versions.
- Duplicate slot IDs.
- Duplicate Region ID colors.
- Overlapping allocation rectangles.
- Hotspot rectangles outside allocation rectangles.
- Invalid half-open bounds.
- Zero-sized slots at supported output resolutions.
- Insufficient minimum gutter.
- Unsupported slot/profile combinations.
- Radial slots with invalid center or radius.
- Invalid world dimensions.
- Invalid rotation lists.
- Missing compatibility key.
- Missing stable slot order.
- Unknown enum values.
- Non-deterministic canonicalization.

Validate built-in templates:

- At build/test time.
- At application startup.
- Before project snapshot creation.

---

# 13. The first production template

Build exactly one complete template before creating a library:

```text
ht.generic_architecture
version 1.0.0
compatibility key ht.generic_architecture.topology.v1
```

The topology should faithfully reproduce the supplied generic hotspot layout rather than approximate it through the existing packer.

It should contain:

- Several full-width horizontal strips.
- Progressively larger horizontal trims.
- Large panel/surface areas.
- Mixed-detail lower region.
- Narrow trim cells.
- Unique bounded detail cells.
- Several square radial cells.
- Optional cap cells.
- Stable material groups and variation groups.

The supplied visual reference suggests recurring major divisions around:

```text
Horizontal:
approximately 4%, 9%, 15-16%, 26%, 41-42%, and 64-65%

Vertical in the detail region:
approximately 33-34%, 56-58%, 72-73%, 82-83%, followed by narrower cells
```

These percentages are tracing guidance only.

The final template must use committed exact integer coordinates and golden fixtures. Runtime code must never infer the topology from the reference image.

After Generic Architecture is proven, add template families in this order:

1. Ultimate Horizontal Trims.
2. Modular Facade.
3. Prop and Panel Atlas.
4. Wood Planks.
5. Mechanical and Radial.

Concrete, rusted metal, painted metal, wood, and stone are material choices, not topology identities.

---

# 14. Template compiler pipeline

The renderer compiles slots. It does not merely paste images into rectangles.

For every slot:

```text
Base material sampling
+ structural profile
+ decorations
+ imported or estimated material maps
+ weathering
+ padding and dilation
= final registered channels
```

## 14.1 Base material fill

The primary material source automatically fills all compatible primary slots.

Per-slot deterministic variation may include:

- Crop offset.
- Rotation where allowed.
- Mirroring where allowed.
- Scale.
- Seed.
- Source selection from a material-source group.

Variation groups prevent obvious identical crops across adjacent or equivalent slots.

## 14.2 Structural profile

Generate structural height through a controlled profile library.

Do not paint fixed normal RGB values directly.

Use signed-distance or analytic profile math for:

- Distance to rectangle edges.
- Distance to selected strip boundaries.
- Distance to inner and outer radial boundaries.
- Distance to panel frame.
- Distance to grooves and seams.

Required initial profile programs:

```text
Flat
Convex 45° bevel
Concave 45° groove
Rounded bevel
Double bevel
Raised lip
Recessed seam
Panel frame
Radial disc
Annulus
```

At the UV-fitting boundary, the profile must produce the configured tangent-space bevel orientation.

## 14.3 Height and normal composition

Prefer height as the common structural representation:

```text
final_height =
    imported_or_estimated_material_height
  + structural_profile_height
  + decoration_height
  + treatment_height
```

Generate a normal from final structural height.

When an imported material normal exists, combine it with the generated profile normal using a proper tangent-space normal-composition method such as reoriented normal mapping.

Never average normal-map RGB values.

## 14.4 Weathering masks

The compiler generates reusable masks:

```text
region mask
edge-distance mask
exposed-edge mask
cavity mask
raised-detail mask
recessed-detail mask
horizontal-up mask
vertical mask
downward-direction mask
cap mask
radial-inner-edge mask
radial-outer-edge mask
decoration mask
material-group mask
```

## 14.5 Weathering recipes

Initial product presets:

```text
Clean
Used
Heavy
```

Internally they parameterize:

```text
edge_wear = exposed_edge_mask * breakup_noise
dirt = cavity_mask * broad_noise
streaks = directional_noise * downward_mask
polish = exposed_edge_mask * fine_noise
chipping = edge_mask * thresholded_noise
```

Channels respond differently:

- Base Color: stains, fading, chipping, discoloration.
- Roughness: dust, polish, wetness, grime.
- Height: buildup or material loss.
- Normal: regenerated or recomposed.
- AO: stronger cavity response.
- Metallic: only imported, explicitly labeled, or exposed by an explicit material rule.

## 14.6 Output channels

Authoritative internal channels remain separate:

```text
Base Color
Height
Normal
Roughness
Metallic
AO
Region ID
Material ID
Masks
```

ORM is an export packing preset:

```text
R = AO
G = Roughness
B = Metallic
```

ORM is not internal source of truth.

---

# 15. Phase responsibility changes

This plan supersedes the old Phase 3 through Phase 6 feature breakdown where the two disagree.
The cross-cutting engineering rules and release-qualification requirements in `docs/phases.md`
remain mandatory: deterministic authoritative CPU output, preview parity, cancellation and bounded
resources, transactional migrations and publishing, recovery, accessibility, offline operation,
diagnostics, performance budgets, and signed release artifacts.

## Phase 3 owns

- Schema migration to ordered material-source sets.
- `LayoutKind::Template`.
- `LayoutKind::CustomAtlas`.
- Template registry.
- Template snapshot.
- First production template.
- Slot bindings.
- Stable slot and region IDs.
- Allocation and hotspot rectangles.
- Exact Region ID.
- Exact Material ID.
- Structural height masks.
- Edge and cavity masks.
- Radial profile generation.
- Basic profile normal preview.
- Package manifest export.
- Blender import vertical slice.
- One rectangular and one radial Blender fixture.

## Phase 4 owns

- Full Base Color composition.
- De-lighting.
- Estimated material Height.
- Material micro-normal generation.
- Roughness estimation.
- AO generation.
- Imported-map composition.
- Full-resolution deterministic CPU bake.
- ORM packing.
- Full OpenGL and DirectX normal output.

## Phase 5 owns

- General nondestructive treatment layers.
- Arbitrary masks.
- Grunge sources.
- Cross-source masks.
- Dirt, edge wear, decals, and adjustments.
- Layer ordering and channel targeting.

## Phase 6 owns

- Finished Blender companion UX.
- Install Companion.
- Connection/session status.
- Send to Blender.
- Auto-map selected geometry.
- Hotspot browser.
- Manual hotspot cycling.
- Assignment overrides and locking.
- Automatic revision updates.
- Topology-change diagnostics.
- Blender package validation.
- Generic folder export remains supported.

Phase 3 must still prove a narrow vertical slice:

```text
one material
+ one template
+ basic structural profiles
+ manifest
+ Region ID
+ rectangular and radial Blender fitting
```

Do not postpone this proof until after the entire renderer is complete.

---

# 16. Persistence schema migration

The repository is currently at schema v8. Implement these additions in the next migration (v9
unless another migration lands first); never reuse the historical v6 number. The migration
introduces or completes:

```text
MaterialSourceSet
TemplateSnapshot
LayoutInstance
SlotBinding
SlotOverride
RegionSourceLayer
RegionWarpOperation
LayoutGeneratorRecipe
DecorationBinding
StyleRecipe
```

## 16.1 Suggested tables

```text
material_sources
material_source_maps
patches

layout_instances
  id
  kind
  template_id
  template_version
  compatibility_key
  template_snapshot_json
  template_snapshot_hash
  output_width
  output_height
  seed
  style_recipe_id

slot_bindings
  layout_id
  slot_id
  fill_kind
  source_id
  patch_id
  material_group
  transform_json
  seed

slot_overrides
  layout_id
  slot_id
  enabled
  profile_override_json
  weathering_override_json

region_source_layers
  id
  layout_id
  slot_id
  source_id
  patch_id
  projection_kind
  source_bounds
  perspective_quad
  sampling_mode
  blend_mode
  opacity
  variation_seed

region_warp_operations
  source_layer_id
  operation_index
  operation_kind
  typed_parameters
  operation_version

layout_generator_recipes
  id
  generator_kind
  generator_version
  seed
  canonical_size
  padding_policy
  size_family_policy
  orientation_policy
  radial_policy
  profile_policy
  parameters

decoration_bindings
  id
  layout_id
  slot_id
  content_source_json
  placement_mode
  channel_blends_json
  transform_json
  seed

style_recipes
  id
  name
  version
  parameters_json
```

Projection and warp operations must use typed, versioned records or a versioned tagged-union
payload validated at the domain boundary. Do not hide the executable warp stack in an unversioned
generic `mapping_override_json` blob.

## 16.2 Migration from the current schema

Migration requirements:

1. Preserve the existing ordered material sources and convert any legacy single input only when it
   is actually encountered.
2. Preserve every imported map and original path.
3. Preserve patch IDs, names, ordering, geometry, and properties.
4. Do not create a template layout automatically unless the old project already has layout state requiring migration.
5. Existing legacy layout state becomes `LayoutKind::CustomAtlas`.
6. New projects default to `LayoutKind::Template`.
7. Stable IDs remain stable.
8. Migration is transactional and covered by fixtures.
9. Failed migration preserves the previous valid project.
10. Explicit Save advances the baseline only after the migrated project validates.
11. Preserve existing schema-v8 layout and source-framing state while creating explicit source
    layers and an empty typed warp stack where required.
12. Preserve generator recipes and their emitted pinned snapshots independently: reopening a
    project must not silently regenerate topology with newer generator code.

---

# 17. Domain commands

Implement commands once in Rust and use them for UI, undo, redo, autosave, tests, and future automation.

Required new commands:

```text
InstantiateTemplate
ChangeTemplate
UpgradeTemplate
CloneTemplateAsCustom
SetLayoutKind
SetOutputResolution
SetPrimaryMaterialSource
SetSlotBaseBinding
ClearSlotBaseBinding
SetSlotContentTransform
SetSlotSeed
SetSlotProfileOverride
SetSlotWeatheringOverride
EnableSlot
DisableSlot
AddSlotDecoration
UpdateSlotDecoration
RemoveSlotDecoration
SetStyleRecipe
RegenerateTemplateOutputs
PublishBlenderRevision
```

Command rules:

- All commands validate before commit.
- All commands are undoable where appropriate.
- Drag and slider interactions coalesce.
- Cache invalidation is deterministic and scoped.
- Topology-changing commands update the compatibility hash.
- Appearance-only commands do not update the topology hash.
- Template snapshot mutation requires an explicit template upgrade or clone-to-custom operation.

---

# 18. Blender package contract

## 18.1 Exported package

A published revision contains:

```text
manifest.hottrim.json
BaseColor.png
Normal.png
Height.png
Roughness.png
Metallic.png
AO.png
RegionID.png
MaterialID.png
optional ORM.png
optional preview.png
optional masks/
```

## 18.2 Manifest

The manifest is authoritative.

The ID map is a diagnostic and interoperability bridge.

Required top-level fields:

```json
{
  "schemaVersion": 1,
  "projectId": "uuid",
  "materialId": "uuid",
  "materialName": "Concrete Used",
  "materialRevision": 42,

  "templateId": "ht.generic_architecture",
  "templateVersion": "1.0.0",
  "compatibilityKey": "ht.generic_architecture.topology.v1",
  "templateSnapshotHash": "sha256",

  "outputSize": [2048, 2048],
  "normalOrientation": "OpenGL",

  "maps": {},
  "slots": []
}
```

Each map record includes:

```text
role
relative path
dimensions
bit depth
color space
checksum
```

Each slot record includes:

```text
slot ID
region ID
name
normalized hotspot rectangle
pixel hotspot rectangle
slot role
UV-fit kind
fit axis
keep proportion
allowed rotations
mirror policy
world size
inset
classification tags
content mapping
variation group
enabled state
Region ID color
```

## 18.3 Topology and material revisions

Use separate concepts:

```text
template_snapshot_hash
material_revision
```

Rules:

- Appearance change only: same topology hash, increment material revision.
- Output resolution change: same topology hash, increment material revision.
- Slot content or weathering change: same topology hash, increment material revision.
- Slot rectangle, role, fit rule, radial tag, or world size change: new topology hash.
- Template change: new topology hash and possibly new compatibility key.

---

# 19. Blender companion architecture

## 19.1 Add-on structure

Suggested package:

```text
blender_addon/
  hot_trimmer_companion/
    __init__.py

    model/
      manifest.py
      properties.py
      validation.py

    sync/
      revision_client.py
      session_registry.py
      update_report.py

    materials/
      builder.py
      image_loader.py
      color_spaces.py

    uv/
      islands.py
      descriptors.py
      matching.py
      fit_rect.py
      fit_strip.py
      fit_radial.py
      classification.py
      texel_density.py

    operators/
      connect_project.py
      import_package.py
      sync_now.py
      auto_map_selected.py
      fit_selected.py
      next_hotspot.py
      previous_hotspot.py
      assign_hotspot.py
      lock_assignment.py
      classify_island.py
      open_project.py

    ui/
      panels.py
      hotspot_browser.py
      status.py

    tests/
      manifest_fixtures/
      blender_fixtures/
```

## 19.2 Blender metadata

Store project/material metadata on the material or image datablock:

```text
ht_project_id
ht_material_id
ht_template_id
ht_template_version
ht_compatibility_key
ht_template_snapshot_hash
ht_material_revision
ht_manifest_path
```

Store assignment state on object/mesh or face-domain attributes:

```text
ht_slot_id
ht_assignment_group
ht_locked
ht_rotation
ht_mirror
ht_scale
ht_classification_override
```

The exact Blender storage mechanism may use custom properties, attributes, or a compact serialized mapping, but it must survive save/reopen and object duplication.

## 19.3 Material creation

The plugin creates or updates a Principled BSDF material.

Color-space rules:

```text
Base Color -> sRGB
Normal -> Non-Color
Height -> Non-Color
Roughness -> Non-Color
Metallic -> Non-Color
AO -> Non-Color
Region ID -> Non-Color / nearest
Material ID -> Non-Color / nearest
```

Normal setup:

- Use Blender Normal Map node.
- Respect manifest OpenGL or DirectX orientation.
- If DirectX input is not converted before export, flip green through the node graph or a generated channel operation.
- Prefer Hot Trimmer exporting the requested orientation directly.

Height:

- Connect through Bump by default.
- Optional displacement mode may be exposed later.
- Do not silently enable costly true displacement.

AO:

- Keep separate or multiply into Base Color according to explicit import preset.
- Do not silently bake AO into Base Color if the package exposes it separately.

## 19.4 UV-island descriptors

For each selected UV island compute:

```text
UV bounding box
aspect ratio
UV area
mesh world area
long-axis direction
boundary vertex count
closed/open boundary
circularity estimate
radial symmetry estimate
existing Hot Trimmer assignment
classification override
```

## 19.5 Candidate filtering

Filter slots before scoring:

- Enabled.
- Compatible role.
- Compatible UV-fit kind.
- Compatible radial classification.
- Compatible rotation.
- Compatible mirror policy.
- Satisfies lock and existing assignment rules.
- Satisfies minimum world-size/texel-density constraints where required.

## 19.6 Matching score

Use a deterministic score such as:

```text
aspect_cost =
    abs(log(island_aspect / slot_aspect))

uv_area_cost =
    abs(log(max(island_uv_area, eps) / max(slot_uv_area, eps)))

world_area_cost =
    abs(log(max(island_world_area, eps) / max(slot_world_area, eps)))

texel_density_cost =
    abs(log(max(island_density, eps) / max(slot_density, eps)))

score =
    aspect_weight * aspect_cost
  + uv_area_weight * uv_area_cost
  + world_area_weight * world_area_cost
  + texel_density_weight * texel_density_cost
  + orientation_penalty
  + classification_penalty
  + role_penalty
```

Use role-specific scoring:

- Unique slots emphasize aspect and full fit.
- Strip slots emphasize thickness, fit axis, and world scale.
- Radial slots emphasize circularity and radial dimensions.
- Repeating strips do not penalize long world length the same way as unique rectangles.

Tie-breaking must be stable.

Equivalent slots may be cycled manually or selected deterministically through variation group and seed.

## 19.7 Rectangular fitting

For unique rectangular slots:

1. Test allowed rotations.
2. Preserve proportion when required.
3. Fit bounding box inside hotspot rectangle.
4. Apply inset.
5. Center or use configured anchor.
6. Apply world-size/texel-density scaling when enabled.
7. Record assignment metadata.

## 19.8 Strip fitting

For `repeat_x` slots:

1. Align island cross-section to slot vertical bounds.
2. Preserve strip thickness.
3. Scale the long UV axis from world length and reference texel density.
4. Allow U to repeat according to the material wrap policy.
5. Use slot anchor and orientation rules.
6. Do not squeeze a long beam entirely into one unique rectangle.

For `repeat_y`, apply the equivalent vertical logic.

## 19.9 Radial classification

Automatic radial classification is heuristic and may be wrong.

Initial heuristic inputs:

- Closed boundary loop.
- UV bounding-box aspect near 1.
- Low variance of boundary distance from centroid.
- Similar PCA eigenvalues.
- Circular or annular mesh topology.
- Existing polar-style UV layout.
- Face or island metadata from prior assignment.

Expose:

```text
Auto
Rectangular
Radial
```

Manual override always wins.

## 19.10 Radial fitting

For radial slots:

1. Confirm or override radial classification.
2. Normalize island around its UV centroid.
3. Preserve circular proportion.
4. Fit into the radial hotspot square with inset.
5. Respect configured inner/outer radius behavior for annular assignments where supported.
6. Record slot assignment.
7. Never select a rectangular-only slot.

MVP does not need to invent a perfect polar unwrap from arbitrary topology.

MVP may:

- Fit an already radial/polar island.
- Offer a focused radial unwrap helper for common discs and annuli.
- Report unsupported topology instead of forcing a bad result.

## 19.11 Assignment locking

Locked assignments survive:

- Material updates.
- Weathering updates.
- Resolution changes.
- Slot-content changes.
- Compatible template renderer updates.

If topology changes:

```text
valid locked assignment
-> preserve

slot removed or incompatible
-> report broken locked assignment
-> never silently remap
```

Unlocked invalid assignments may be remapped with explicit user confirmation or through an Auto Remap action.

---

# 20. Synchronization architecture

## 20.1 Principle

Do not begin with a socket server.

Use an atomic local revision protocol first.

This remains offline and works even if Hot Trimmer or Blender crashes.

## 20.2 Revision directory

Recommended structure:

```text
HotTrimmerSync/
  projects/
    <project_id>/
      current.json
      revisions/
        <revision_id>/
          manifest.hottrim.json
          BaseColor.png
          Normal.png
          Height.png
          Roughness.png
          Metallic.png
          AO.png
          RegionID.png
          MaterialID.png

  sessions/
    <blender_session_id>.json

  requests/
    pending/
    claimed/
    complete/
    failed/
```

## 20.3 Atomic publish

Hot Trimmer:

1. Creates a staging revision directory.
2. Writes all maps and manifest.
3. Validates dimensions, checksums, IDs, and slot metadata.
4. Flushes files.
5. Renames staging to the immutable revision directory.
6. Atomically replaces `current.json`.
7. Optionally creates a Send to Blender request.

Blender never reads staging directories.

## 20.4 Blender polling

Use `bpy.app.timers`.

The plugin checks:

- Connected project.
- `current.json` modification time.
- Material revision.
- Template snapshot hash.
- Pending targeted requests.

When appearance changes:

- Update Blender image file paths.
- Reload images.
- Update material revision.
- Leave UV assignments untouched.

When topology changes:

- Validate every stored assignment against the new slot set.
- Produce a report.
- Preserve valid assignments.
- Offer remap for unlocked invalid assignments.
- Never silently replace locked invalid assignments.

## 20.5 Blender session registry

To support one-click Send to Blender without network services:

Each Blender instance writes a small heartbeat file:

```json
{
  "sessionId": "uuid",
  "pid": 12345,
  "blendFile": "redacted-or-display-name",
  "lastSeenUtc": "...",
  "addonVersion": "1.0.0",
  "capabilities": ["import", "auto-map", "live-update"]
}
```

Hot Trimmer may display:

```text
Blender: Connected
Session: Warehouse.blend
```

For MVP:

- Use the most recently active compatible session by default.
- Allow session selection when several are active.
- Expire stale heartbeats.
- Do not expose raw user paths in shareable diagnostics.

## 20.6 Later local IPC

A named pipe may later provide faster commands:

```text
Ping Blender
Get selected object count
Send and auto-map selection
Open project
Report warnings
```

The atomic revision package remains the authoritative fallback.

---

# 21. Hot Trimmer UI changes

## 21.1 Basic template UI

Default right-side controls:

```text
Template
  Generic Architecture

Primary Material
  Concrete_01

Edge Style
  Soft Concrete

Condition
  Clean | Used | Heavy

Resolution
  1024 | 2048 | 4096

Build Trim Sheet
Send to Blender
```

Optional detail section:

```text
Vent Patch          Unique Detail
Greek Key Patch     Repeating Strip
Drain Patch         Radial Detail
```

Output views:

```text
Beauty
Normal
Height
Roughness
Metallic
AO
Region IDs
Material IDs
Hotspots
```

## 21.2 Region inspector

Clicking a region exposes compact overrides. The UI never calls it a slot:

```text
Content
Variation
Scale
Rotation
Mirror
Profile
Weathering amount
Decoration list
Enabled
```

Do not expose these in the default global UI:

- Fit-axis internals.
- Exact Region ID color.
- DCC tags.
- Integer template coordinates.
- Hotspot scoring tolerances.
- Bleed geometry.
- Inset internals.

## 21.3 Template topology editing

Standard templates are read-only in basic mode.

When the user chooses `Customize Topology`:

1. Clone the template snapshot.
2. Change layout kind to `CustomAtlas` or `CustomTemplate`.
3. Generate a new compatibility key.
4. Warn:

```text
This layout will no longer share hotspot UV assignments with Generic Architecture.
```

5. Allow boundary editing only after confirmation.

---

# 22. Blender companion UI

Recommended sidebar panel:

```text
Hot Trimmer
  Connection
    Project: Concrete Used
    Revision: 42
    Status: Up to date
    [Sync Now] [Open in Hot Trimmer]

  Material
    [Create/Update Material]
    [Apply to Selected]

  Mapping
    Classification: Auto | Rectangular | Radial
    Current Hotspot: Horizontal Trim 03
    [Auto Map Selected]
    [Fit Selected]
    [Previous] [Next]
    [Lock Assignment]

  Hotspot Browser
    visual slot thumbnails
    role filters
    material-group filters
    radial-only filter

  Diagnostics
    18 valid assignments
    3 remappable
    0 broken locks
```

The panel is deliberately narrow.

Do not recreate a complete UV suite.

---

# 23. Repository work plan

Suggested locations based on the current architecture.

## Rust domain

```text
crates/domain/src/layout_kind.rs
crates/domain/src/templates/
  mod.rs
  ids.rs
  definition.rs
  slot.rs
  snapshot.rs
  validation.rs
  commands.rs

crates/domain/src/layout/
  instance.rs
  slot_binding.rs
  decoration.rs
  style_recipe.rs
```

## Geometry and compiler

```text
crates/geometry/src/template_compile.rs
crates/geometry/src/profile/
  mod.rs
  rect_sdf.rs
  radial_sdf.rs
  bevel.rs
  groove.rs
  panel.rs

crates/geometry/src/hotspot_rect.rs
crates/geometry/src/id_raster.rs
```

## Rendering

```text
crates/render-core/src/template/
  base_fill.rs
  variation.rs
  profile_height.rs
  normal_compose.rs
  weathering_masks.rs
  ids.rs
```

## Project store

```text
crates/project-store/src/migrations/v6.rs
crates/project-store/src/template_snapshot.rs
crates/project-store/src/layout_instance.rs
crates/project-store/src/slot_binding.rs
```

## Export and sync

```text
crates/export/src/hottrim_manifest.rs
crates/export/src/hottrim_package.rs
crates/export/src/blender_revision.rs
crates/export/src/atomic_publish.rs
```

## Desktop

```text
apps/desktop/src/features/templates/
apps/desktop/src/features/layout/template-workpiece/
apps/desktop/src/features/layout/slot-inspector/
apps/desktop/src/features/blender-sync/
```

Move hardcoded template rectangles out of TypeScript.

## Blender add-on

```text
blender_addon/hot_trimmer_companion/
```

The exact paths may be adapted to the repository, but ownership boundaries must remain.

---

# 24. Execution slices

Do not implement this entire document in one Codex run.

Each slice must leave production-shaped code, targeted tests, and a recorded gate result.

## Slice 0: Freeze and rename the current layout engine

Tasks:

- Freeze feature work on the current default Phase 3 implementation.
- Rename it internally to `CustomAtlasLayoutEngine`.
- Remove it from the default Create Trim Sheet action.
- Keep tests passing.
- Record what existing code remains reusable.
- Do not delete functioning packing code.

Exit:

- Existing behavior is preserved under an advanced/internal path.
- Default template work can proceed without fighting legacy preset semantics.

## Slice 1: Architecture decision records

Write ADRs for:

- Template-first product model.
- Template versus Custom Atlas.
- Canonical integer coordinates.
- Allocation rectangle versus hotspot rectangle.
- Manifest authority versus ID-map diagnostics.
- Blender companion ownership.
- Atomic local revision sync.
- Clean-room reimplementation of reference-tool behavior.

Exit:

- Contracts are agreed before schema and renderer work spreads.

## Slice 2: Schema v6 and domain types

Implement:

- `LayoutKind`.
- Template IDs and versions.
- Compatibility keys.
- Template snapshot.
- Template slot contracts.
- Slot bindings.
- Decoration bindings.
- Style recipe.
- Stable region IDs and colors.
- Migration fixtures.

Exit:

- Schema v5 projects migrate without loss.
- Legacy layouts become Custom Atlas.
- New projects can persist a pinned template snapshot.

## Slice 3: Template registry and validator

Implement:

- Rust-owned registry.
- JSON/resource loading.
- Canonicalization.
- Validation.
- Snapshot hashing.
- Build/startup validation tests.
- Typed errors.

Exit:

- Invalid templates cannot load.
- TypeScript no longer owns template rectangles.

## Slice 4: Generic Architecture v1

Implement:

- Exact traced topology.
- Stable slot IDs.
- Exact integer rectangles.
- Material groups.
- Variation groups.
- Rectangular, strip, unique, cap, and radial slot metadata.
- Preview guide.
- Golden Region ID at 2K and 4K.

Exit:

- The template is deterministic.
- ID output is exact.
- No runtime packing is involved.

## Slice 5: Manifest and Blender vertical slice

Implement:

- `.hottrim.json` manifest.
- Package export.
- Minimal Blender add-on import.
- Material creation.
- One rectangular fixture.
- One radial fixture.
- One Fit Selected operator.
- Manual Auto/Rectangular/Radial override.

Exit:

- Blender imports slot semantics without Zen UV.
- Rectangular and radial fixtures fit correctly.
- ID map is not used to infer radial status.

## Slice 6: Structural profile compiler

Implement and golden-test:

- Flat.
- Convex 45° bevel.
- Concave groove.
- Rounded bevel.
- Panel frame.
- Radial disc.
- Annulus.
- OpenGL normal.
- DirectX normal.
- Hotspot-boundary normal validation.

Exit:

- The generic template produces convincing structural edge previews.

## Slice 7: Primary material filling

Implement:

- Auto-bind primary source.
- Physical scale.
- Deterministic crop offsets.
- Rotation and mirror policies.
- Variation groups.
- Repeat X.
- Repeat Y.
- Unique fit.
- Planar radial fill.
- Padding and dilation.

Exit:

- One concrete source produces a complete template preview with no patches.

## Slice 7A: Region-to-source authoring

This is the primary usability slice. In user-facing language, call template areas
**regions**, not slots. A region has two deliberately separate coordinate spaces:

- **Sheet UV bounds**: the stable template rectangle on the trim sheet, shown on the right.
- **Source UV mapping**: the editable normalized 2D footprint sampled from the source image,
  shown on the left.

Implement:

- Selecting a region on the right enters **Region Source** mode on the left source workspace.
- Draw the exact inferred source footprint for that region over the source image, including its
  cover crop, repeat wrapping, rotation, and mirror state. Do not show a misleading single
  rectangle when repeat mapping wraps; show the repeated/wrapped footprint honestly.
- Show normalized source-UV coordinates and pixel coordinates for the selected footprint.
- Let the user move, resize, and numerically edit the source footprint in the left workspace.
  Support grid/texture-feature snapping so a user can align a boundary to a brick course or
  mortar seam.
- Persist a per-region **source layer** recipe: source bounds or perspective quad, rectification
  transform, sampling mode, rotation, mirror, opacity/blend policy, and deterministic variation
  offset. A region may still opt into the whole-source default.
- Expose the composable region UV-warp stack in §11.2. Planar, Perspective, Polar, Spiral/Twirl,
  Radial Lens, and Cylindrical/Arc mappings all operate on source content without changing the
  region's sheet shape or stable identity.
- Treat an edited left-side rectangle or perspective quad as executable material authoring. The
  compiler must rectify that source layer and composite it into the selected right-side region;
  it must not leave the right sheet as the original whole-source fill while only changing
  metadata or a source-workspace preview.
- Use the same source-layer transform for every registered channel. Base Color, Height,
  Roughness, Metallic, AO, Opacity, and masks are sampled through the same mapping; transformed
  Normal maps must have their tangent-space X/Y direction corrected for rotation or mirroring
  before final normal composition.
- Compile the selected region as a real layer stack:

  ```text
  global framed base material
    -> selected-region rectified source layer (optional override)
    -> region structural profile / bevel / seam
    -> later decoration and weathering layers
    = final right-side PBR and ID outputs
  ```

  The source layer is clipped to that region's content rectangle. It can replace the base
  sample or blend over it, but its preview and final output must use the identical transform.
- Rebuild the affected right-side region immediately at preview resolution while editing; commit
  the same recipe to the full-resolution sheet on Update. The user must be able to see the
  rectified brick, wood, or metal result on the right while manipulating its four left-side
  points.
- Keep right-sheet selection and left-source selection synchronized. Selecting a left footprint
  selects its region; selecting a right region reveals the matching left footprint.
- Add **Edit Source Framing**. It opens the left workspace in framing mode, overlays the global
  cover/crop rectangle there, and makes crop focus draggable and numerically editable. Framing
  must never be a pair of unexplained sliders detached from the source image.
- Recompile every registered PBR and ID output from the same persisted mapping recipe.
- Preserve template region identity and sheet UV bounds during this slice. Editing source
  mapping is not the same operation as changing topology.

Exit:

- A user can click a region on the trim sheet, see exactly where it comes from in source UVs,
  and align that sampling area to visible source features.
- Editing or rectifying that source area visibly changes the corresponding right-side region and
  every output channel; it is never a disconnected preview or UV-only edit.
- A user can click Edit Source Framing and directly manipulate the crop on the source image.
- The left and right canvases always describe the same mapping; no transform is hidden in a
  side-panel control.

## Slice 7B: Trim-sheet workbench and material preview

Implement:

- Replace debug labels such as `Material 1` with semantic region names from the template
  manifest, for example `Crown Wide`, `Brick Field`, `Stud Ring`, or `Recessed Rail`.
- Apply label level-of-detail: no labels at overview zoom, a compact name on hover/selection,
  and name/role/dimensions only when zoomed close enough. Keep diagnostics behind a Debug
  overlay rather than on the authoring canvas.
- Consolidate sheet type, template recipe, source choice, framing, and layout-group controls
  into a collapsible **Trim Sheet Settings** drawer. The normal canvas should show the sheet,
  not permanent implementation settings. Keep undo/redo available via shortcuts or an overflow
  menu rather than consuming primary workspace space.
- Expose optional template groups such as rails, large fields, radial details, and bottom
  micro-detail banks. Turning a group off reclaims and regenerates its space; it is an explicit
  topology/compatibility change, not visual hiding.
- Add a material-preview mode beside the sheet preview. Render the compiled Base Color,
  Normal, Height, Roughness, Metallic, and AO on a canonical preview scene containing a wall,
  broad panel, edge trim, strip, and radial detail under neutral lighting.
- Provide fast 2D map inspection and 3D material inspection from the same workbench, including
  normal-strength, roughness, and channel-isolation views.

Exit:

- The workbench reads as a trim-sheet authoring tool rather than a debug atlas.
- A user can judge the actual material result, not only the rectangle arrangement.
- Region labels remain readable without obscuring the trim sheet at any zoom level.

## Slice 7C: Authoritative rendering, warp limits, and preview parity

Implement:

- Make the deterministic CPU compiler authoritative for final output; GPU rendering is a
  cancellable interactive preview of the same recipe.
- Define typed evaluation and operation order for every source projection and UV warp.
- Bound operation count, sampling scale, iteration count, and intermediate allocation size.
- Reject NaN, infinity, singular perspective quads, invalid radii, and non-invertible mappings
  with region-specific diagnostics rather than emitting corrupt pixels.
- Include source revisions, ordered warp operations, generator snapshot hash, profile recipe,
  output resolution, and renderer version in cache keys.
- Correct tangent-space normals using the local mapping Jacobian, including handedness changes
  caused by reflection and polar or nonlinear transforms.
- Add cancellation, progress, tile scheduling, memory estimates, and bounded caches for preview
  and full-resolution compilation.
- Mark inferred Height, Roughness, AO, and Normal channels as **Estimated** until replaced by
  authored or imported maps.
- Add CPU/GPU parity fixtures for planar, perspective, polar, spiral, lens, cylindrical, mirrored,
  and singular-input cases.

Exit:

- Preview and final compilation agree within documented per-channel tolerances.
- Invalid mappings fail locally and explain how to repair the affected region.
- A large or adversarial warp recipe cannot create unbounded work or memory use.

## Slice 7D: Procedural layout generation and structural profile families

Build deterministic layout generators that emit valid template snapshots. A generator is allowed
to pack regions freely; its output becomes stable only when the generated snapshot is accepted and
pinned.

Implement:

- Generate region size families recursively from a canonical area: broad half-size fields, two or
  three quarter-size variants, then progressively halved medium, small, micro-trim, and detail
  families until configured minimum dimensions are reached.
- Let each size family request a configurable number of variants so repeated use does not force
  identical dimensions or sampling.
- Generate configurable horizontal and vertical populations, plus a deliberately limited radial
  population. Orientation is a semantic constraint supplied to packing, not an after-the-fact
  rotation of labels.
- Support deterministic seeded packing with adjustable outer margin, inter-region padding, bleed,
  alignment, grouping, occupancy target, minimum region size, aspect-ratio ranges, and reserved
  banks. The generator may repack the entire sheet when topology settings change.
- Allow recipe-level weights and quotas rather than hardcoded coordinates: size-family weights,
  orientation mix, radial count/radii, strip count, unique-field count, and detail-bank density.
- Assign a structural profile recipe per generator, group, family, or individual region. Profiles
  define seam shape, bevel radius, bevel curve, inset/extrusion, edge hardness, cavity response,
  trim-cap behavior, and normal intensity.
- Compile those profile assignments into structural Height, Normal, AO/cavity, edge masks, and
  padding-aware dilation. Generated normals must follow generated region boundaries; they are not
  a static overlay copied from one preset.
- Provide several first-party generator recipes (for example balanced architecture, horizontal
  trim bank, vertical panel bank, dense detail field, and radial-accent field) using the same
  generator domain model rather than separate hardcoded rectangle lists.
- Show a generation summary before acceptance: region count by family/orientation, radial count,
  occupancy, unused area, minimum padding/bleed, validation errors, and compatibility impact.
- `Regenerate` creates a new candidate snapshot. `Accept Layout` pins its rectangles, IDs, profile
  assignments, generator version/seed, and compatibility key. Material, weathering, or source-layer
  changes after acceptance must not repack it.
- Changing padding, size-family policy, population, packing seed, orientation quotas, radial
  policy, or structural profile geometry is an explicit topology regeneration. It creates a new
  snapshot/hash and runs Blender assignment compatibility diagnostics before replacement.

Exit:

- The same recipe, seed, and generator version produce byte-identical topology and profile data.
- Users can generate materially different useful layouts without editing hardcoded coordinates.
- Padding and profile controls visibly regenerate correct structural height and normal boundaries.
- An accepted layout remains stable across material swaps, save/reopen, export, and Blender sync.

## Slice 8: Detail bindings

Implement:

- Repeating Strip.
- Unique Detail.
- Radial Detail.
- Trim Cap.
- Compatible-slot suggestions.
- Undoable bindings.
- Per-decoration channel contribution.
- Persistence.

Exit:

- A vent, decorative strip, and radial detail can coexist over one base material.

## Slice 9: Basic weathering recipes

Implement:

- Structural edge and cavity masks.
- Clean, Used, Heavy recipes.
- Base Color and Roughness response.
- Explicit metallic behavior.
- Deterministic seeds.

Exit:

- Non-artist user can produce visibly distinct credible material conditions with one control.

## Slice 9B: General nondestructive treatment layers

Implement ordered, maskable, channel-targeted Grunge, Edge Wear, Dirt, Color/Roughness Adjust,
Height Boost, Decal, and Mask layers. Support cross-source masks, per-region/group targeting,
deterministic seeds, undo/redo, persistence, and CPU/GPU preview parity.

Exit:

- Reordering, masking, disabling, saving, and reopening treatment layers is nondestructive and
  deterministic across every targeted channel.

## Slice 10: Blender matching and browser

Implement:

- UV-island descriptors.
- Candidate filters.
- Role-specific scoring.
- Rectangular fitting.
- Strip fitting.
- Radial fitting.
- Candidate cycling.
- Hotspot browser.
- Assignment persistence.
- Locking.

Exit:

- Selected modular assets can be auto-mapped and manually corrected without generic UV tooling.

## Slice 11: Atomic sync and Send to Blender

Implement:

- Revision folder.
- Atomic current pointer.
- Blender timer polling.
- Session heartbeat.
- Send request.
- Appearance update path.
- Topology-change report.
- Valid/invalid/locked assignment handling.

Exit:

- Changing weathering in Hot Trimmer updates Blender without changing UVs.
- Topology changes produce explicit diagnostics.

## Slice 11B: Generic export and canonical 3D validation

Implement generic folder export independently of Blender sync, with presets for map selection,
format, bit depth, packed channels, and OpenGL/DirectX normals. Stage exports atomically, estimate
disk and memory requirements, and support cancellation without partial output. Validate the same
compiled package in the canonical 3D preview scene and a Blender fixture.

Exit:

- A user can inspect the authored sheet as a material and export a complete portable package
  without installing Blender.
- Disk-full, cancellation, Unicode paths, and read-only destination failures preserve previous
  valid output and produce actionable diagnostics.

## Slice 12: Advanced Custom Atlas reconnection

Implement:

- Advanced entry point.
- Existing packer.
- Custom topology compatibility key.
- Clone Template as Custom.
- Warning and topology invalidation.
- Freeform layout controls.

Exit:

- Existing flexible functionality remains available without polluting the default product.

## Slice 13: Release qualification

Run clean-install, migration, crash-recovery, autosave, Unicode/path/permission, read-only,
keyboard-only, high-DPI, screen-reader labeling, offline, performance-budget, bounded-resource,
package-signing, and documentation gates. Remove user-facing roadmap labels such as `Phase 3` and
`Later`; unavailable controls explain the missing capability or prerequisite instead.

Exit:

- All supported platforms pass the release matrix and recovery fixtures.
- No production UI exposes internal phase numbering.
- Installers and companion packages are signed and reproducible from the release revision.

---

# 25. Acceptance gates

## 25.1 One-image gate

Input:

```text
one concrete Base Color image
```

Action:

```text
Generic Architecture
-> Used
-> Build
```

Required output:

- Credible Base Color.
- Structural Height.
- Structural Normal.
- Concrete-appropriate default Roughness.
- Metallic exactly zero.
- AO/Cavity.
- Region ID.
- Material ID.
- Manifest.

No patch creation.

## 25.2 Material swap gate

Replace concrete with rusted steel.

Must remain identical:

- Template ID/version.
- Compatibility key.
- Template snapshot hash.
- Slot rectangles.
- Hotspot rectangles.
- Region IDs.
- Region colors.
- Blender trim definitions.
- Existing UV assignments.

Only appearance channels change.

## 25.3 Radial gate

In Blender:

- Select a radial disc or annular island.
- Auto or manual Radial classification.
- Fit to a radial slot.
- Preserve circular proportion.
- Store assignment.
- Reopen `.blend` and retain assignment.

## 25.4 Bleed gate

Changing output from 2048 to 4096:

- Scales padding and bleed appropriately.
- Does not move normalized hotspot rectangles.
- Does not change topology hash.
- Does not change Region IDs.
- Does not require UV remapping.

## 25.5 ID gate

Region ID contains exactly:

```text
background
+ one exact flat color per enabled hotspot
```

No extra colors.

## 25.6 Normal-boundary gate

At every configured bevel edge:

- The intended hotspot boundary remains exact.
- The generated normal reaches the configured bevel orientation.
- Padding and dilation do not shift the fitting line.
- OpenGL and DirectX fixtures match expected direction.

## 25.7 Multi-source gate

The project supports:

```text
Concrete source -> primary surfaces
Metal source -> accent slots
Vent patch -> unique detail
Greek key -> repeating strip
Drain cover -> radial detail
```

without destructively merging source data.

## 25.8 Persistence gate

After save, close, reopen, and regenerate, preserve:

- Template snapshot.
- Compatibility key.
- Slot bindings.
- Decorations.
- Seeds.
- ID colors.
- World sizes.
- Radial tags.
- Assignment metadata.
- Deterministic outputs for the same renderer version.

## 25.9 Blender update gate

Appearance-only change:

- Blender updates maps.
- UVs remain byte-equivalent.
- Locks remain.
- No remap prompt.

Topology change:

- Valid assignments preserved.
- Invalid unlocked assignments listed and optionally remapped.
- Broken locked assignments reported.
- No silent destructive remapping.

## 25.10 Crash and partial-output gate

Kill Hot Trimmer during publish:

- `current.json` still points to the previous complete revision.
- Blender never sees a partial package.
- Staging data can be cleaned safely.

Kill Blender during update:

- `.blend` remains valid.
- Next sync retries.
- Existing material and assignments remain recoverable.

## 25.11 Region source-authoring gate

- Selecting a compiled region reveals the exact executable source footprint on the left.
- Editing a rectangle, perspective quad, crop, or ordered warp stack updates that region on the
  right at preview resolution and identically at final resolution.
- Every registered PBR map uses the same mapping; transformed normals retain correct direction.
- Save, close, reopen, undo, redo, and crash recovery preserve the authored mapping.
- Region sheet bounds, stable ID, topology hash, and existing Blender UV assignments do not change
  during source-only edits.

## 25.12 Procedural layout-generation gate

- Identical generator recipe, seed, and version produce byte-identical rectangles, IDs, groups,
  structural profiles, and topology hash.
- Changing padding or population regenerates a new candidate snapshot and never mutates an accepted
  snapshot in place.
- The generated layout passes overlap, bounds, minimum-size, padding, bleed, occupancy, ID, and
  profile validation before acceptance.
- At least three recipes demonstrate recursive size families, horizontal and vertical variants,
  and a bounded radial population without hardcoded final rectangle lists.
- Bevel, seam, inset, and normal settings regenerate Height/Normal/AO masks at the generated region
  boundaries and scale correctly at 1K, 2K, 4K, and 8K.

## 25.13 Render authority and bounded-work gate

- CPU final output is deterministic and GPU preview remains within documented tolerances.
- Cancellation leaves the last valid preview/export intact.
- Invalid or singular warp inputs produce typed, region-specific failures.
- Compilation remains within declared memory, tile, thread, cache, and IPC bounds.

## 25.14 Treatment-layer gate

- Layer ordering, masks, targeting, seeds, undo/redo, and persistence survive reopen.
- Disabling a treatment layer restores the prior compiled channels without destructive source edits.
- Estimated channels are visibly labeled and replaced cleanly by imported authored maps.

## 25.15 Export, accessibility, and release gate

- Generic folder export is atomic and passes cancellation, disk-full, Unicode, permission, and
  read-only fixtures.
- Region selection, source framing, perspective handles, warp controls, generation, and acceptance
  are operable by keyboard at supported DPI scales with accessible labels.
- Clean install, offline operation, recovery, performance budgets, signing, and documentation pass
  the release matrix.
- User-facing UI contains no stale phase-number or `Later` roadmap labels.

---

# 26. Stop conditions

Do not advance when any of these are true:

- Template rectangles are still authored independently in TypeScript and Rust.
- A standard template can mutate without changing its topology hash.
- An ID map is being used as the only source of radial or fit metadata.
- Region colors change across save/reopen.
- Padding changes hotspot rectangles.
- A schema change lacks migration tests.
- Blender silently remaps locked assignments.
- Normal RGB values are averaged instead of vector-composed.
- Metallic is inferred from Base Color without an explicit rule.
- Hot Trimmer can publish a partial revision as current.
- Appearance-only updates trigger UV remapping.
- Template updates silently alter old project snapshots.
- Generic packing remains the default user path.
- Plugin work grows into a full unrelated UV suite.
- A Codex run refactors unrelated architecture instead of completing its assigned slice.

---

# 27. Explicit non-goals

Not required for the first release:

- Full Zen UV feature parity.
- Full Trim View feature parity.
- Arbitrary Blender-side trim topology editing.
- General node-based material authoring.
- AI material understanding.
- Automatic perfect radial unwrap for arbitrary topology.
- Online synchronization.
- Marketplace or cloud library.
- Multi-user project collaboration.
- Generic UV editor replacement.
- Runtime dependency on third-party UV add-ons.
- Large template library before Generic Architecture is proven.

---

# 28. Codex execution protocol

This file is an implementation program, not a request to implement everything in one pass.

For each Codex run:

1. Work on one named slice only.
2. Inspect existing code before proposing replacement.
3. Preserve production-shaped infrastructure already implemented.
4. Do not create a second domain model in TypeScript.
5. Do not refactor unrelated modules.
6. Add only tests required for the current slice.
7. Run targeted tests first.
8. Run the broader gate only after targeted tests pass.
9. Record changed contracts, schema, and fixtures.
10. Stop when the slice exit criteria are satisfied.
11. Do not preemptively implement future slices.
12. Do not repeatedly reimplement working code because a cleaner abstraction is imaginable.
13. Report unresolved blockers directly.
14. Update this plan’s checklist or a phase report with evidence.

Every slice report should include:

```text
Delivered
Files changed
Contracts changed
Migrations changed
Tests run
Golden fixtures changed
Performance observations
Known limitations
Gate decision
Next slice
```

---

# 29. Historical starting task (completed)

This section records the original bootstrap instruction and is no longer the current execution
entry point. Slice status must be tracked separately from the product UI.

Required result:

1. Identify the existing Phase 3 layout entry points.
2. Rename the current generic layout path to Custom Atlas terminology without changing behavior.
3. Remove it from the default template path.
4. Write the ADRs listed in Slice 1.
5. Produce a short repository-specific map of reusable code:
   - packer
   - region persistence
   - ID generation
   - layout UI
   - renderer hooks
   - export hooks
6. Do not implement the then-proposed schema v6 during the bootstrap task. Future persistence work
   uses the next migration after the repository's current schema, as specified in section 16.
7. Do not implement the Blender add-on yet.
8. Do not redesign unrelated Phase 1 or Phase 2 code.
9. Stop after tests and the Slice 1 gate pass.

The next run should begin Slice 2 using the ADRs as constraints.

---

# 30. Final product definition

The finished product loop is:

```text
Open concrete
-> choose Generic Architecture
-> choose Used
-> optionally add vent, decorative strip, and radial detail
-> build registered PBR trim sheet
-> Send to Blender
-> selected objects receive material and hotspot UV assignments
-> correct two unusual islands through the hotspot browser
-> change weathering in Hot Trimmer
-> Blender updates automatically
```

Hot Trimmer authors the material and hotspot vocabulary.

The Blender companion applies that vocabulary to geometry.

That is the product.
