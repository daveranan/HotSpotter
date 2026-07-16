# Hot Trimmer trim-sheet document and generation redesign

## Status and authority

**Status:** Replacement execution plan for the desktop trim-sheet authoring and generation path.

**Product vision:** This plan implements the template compiler, procedural generation, material-map,
and Blender goals in `hot-trimmer-template-blender-companion-plan.md`.

**Execution decision:** The companion plan remains the broad product contract. This document owns
the order and definition of done for rebuilding the broken desktop authoring-to-render pipeline.
Where older implementation slices permit metadata, overlays, previews, and final pixels to advance
separately, this plan requires one end-to-end vertical result before the next capability begins.

**Primary promise:** Every visible authoring control changes one persisted trim-sheet document; the
same document produces the 2D preview, material preview, exported maps, manifest, and Blender package.

---

## 1. Problems this plan must eliminate

The current product can display controls and overlays that do not affect compiled pixels. In
particular:

- Cover/crop exposes a full-source rectangle while a second hidden aspect crop determines the
  rendered result.
- Patch-to-region assignment is persisted but not consumed by the template compiler.
- Edited region bounds can move independently from the bounds used to rasterize the sheet.
- Template and Atlas do not produce the same kind of authoritative rendered artifact.
- Imported companion maps appear in the UI but are not all sampled through the compiled mapping.
- Global source framing and per-region source editing look similar but use separate state paths.
- Direct-manipulation pointer updates race project snapshots and preview jobs, causing rollback,
  replay, and visible position oscillation.
- Double-click editing can fall through to creation, producing a second patch/footprint instead of
  editing the selected identity.
- Transform controls may be visible when the selected object or layout kind cannot execute them.
- Existing tests validate isolated geometry or PNG existence, not the complete workflow.

This is a data-flow and authority failure. It must not be treated as a collection of CSS or event
handler defects.

---

## 2. Product invariants

1. One Rust-owned `TrimSheetDocument` is the authoritative authoring state.
2. TypeScript renders projections of that document and sends typed commands; it does not own a
   second layout, binding, source-transform, or revision model.
3. Every render-relevant edit changes the document revision or an explicit draft transaction.
4. The compiler consumes the whole immutable document revision, never an ad hoc subset assembled
   by a workspace component.
5. The compiler returns one `CompiledSheet` artifact containing maps, region metadata, diagnostics,
   and the exact input revision/hash.
6. Sheet preview, material preview, export, and Blender publish consume the same compiled artifact.
7. A UI overlay is derived from the resolved compile plan. It cannot use different bounds from the
   pixels beneath it.
8. Source-only edits never silently change sheet topology or Blender hotspot compatibility.
9. Topology changes are candidates until explicitly accepted and pinned.
10. No pointer gesture can implicitly change tools or create a new identity.

The required flow is:

```text
sources + maps + patches
        +
accepted/generated topology
        +
region bindings and mapping/warp recipes
        +
profiles, decorations, and treatment layers
        |
        v
TrimSheetDocument revision
        |
        v
resolved compile plan
        |
        v
CompiledSheet
  -> 2D sheet preview
  -> 3D material preview
  -> map export
  -> manifest / Blender revision
```

---

## 3. Authoritative trim-sheet document

The document separates topology, content mapping, material appearance, and publishing without
disconnecting them.

```rust
pub struct TrimSheetDocument {
    pub id: TrimSheetId,
    pub document_revision: u64,
    pub topology_revision: u64,
    pub appearance_revision: u64,
    pub topology: AcceptedTopology,
    pub materials: Vec<MaterialSourceSet>,
    pub patches: Vec<Patch>,
    pub region_bindings: BTreeMap<RegionId, RegionBinding>,
    pub decorations: Vec<DecorationBinding>,
    pub treatments: Vec<TreatmentLayer>,
    pub render_settings: RenderSettings,
    pub generator_provenance: Option<GeneratorProvenance>,
}
```

### 3.1 Accepted topology

```rust
pub struct AcceptedTopology {
    pub kind: TopologyKind,
    pub snapshot: TopologySnapshot,
    pub topology_hash: Hash,
    pub compatibility_key: String,
    pub regions: Vec<RegionDefinition>,
}

pub enum TopologyKind {
    StandardTemplate,
    GeneratedTemplate,
    CustomTemplate,
    CustomAtlas,
}
```

Every `RegionDefinition` owns exactly one authoritative geometry record:

```text
stable region ID
semantic name and role
allocation rectangle
hotspot rectangle
orientation and UV-fit policy
structural-profile recipe
material/weathering group
enabled state
```

There is no second mutable `LayoutItem.fill` versus `LayoutRegion.fill` representation. Template
definitions and generator recipes produce initial region definitions; accepted region definitions
are what the compiler and overlays use.

### 3.2 Region binding

```rust
pub struct RegionBinding {
    pub region_id: RegionId,
    pub content: ContentReference,
    pub mapping: RegionMapping,
    pub variation: VariationSettings,
    pub blend: BlendPolicy,
}

pub enum ContentReference {
    InheritPrimaryMaterial,
    MaterialSource(MaterialSourceSetId),
    Patch(PatchId),
    Solid(SolidChannelValues),
    Procedural(ProceduralMaterialId),
}
```

The binding is the only place that says what content fills a region. Assigning a patch replaces
`ContentReference`; the renderer must resolve and rectify that patch before sampling it.

### 3.3 Mapping recipe

```rust
pub struct RegionMapping {
    pub projection: Projection,
    pub warps: Vec<WarpOperation>,
    pub transform: MappingTransform,
    pub address_mode: AddressMode,
    pub sampling: SamplingPolicy,
}
```

Evaluation order is fixed and versioned:

```text
region-local UV
-> crop or perspective source projection
-> ordered warp stack
-> scale / rotate / mirror / offset
-> clamp / repeat / mirrored repeat
-> channel sampling and normal-vector correction
-> blend into the region content rectangle
```

### 3.4 Revisions and hashes

- `document_revision` changes for every accepted command.
- `appearance_revision` changes for sources, patches, bindings, mapping, profiles that do not move
  hotspots, decorations, treatments, and output channel settings.
- `topology_revision` changes only when accepted region geometry, hotspot geometry, region
  population, or UV-fit metadata changes.
- `topology_hash` excludes appearance-only data.
- `appearance_hash` covers every compiler input, including source content checksums, ordered warp
  operations, renderer version, seeds, and map policies.

Blender UV assignments survive appearance changes. A topology candidate cannot replace the accepted
topology without an explicit compatibility report and acceptance command.

---

## 4. Two transformations that the UI must never confuse

### 4.1 Region layout transform

This changes where the region exists on the trim sheet:

```text
move allocation rectangle
resize allocation/hotspot rectangle
change padding/gutter
rotate an axis-aligned custom region by 90 degrees
add/delete/reorder regions
```

It is a topology edit. It is available while editing a generated candidate, a Custom Template, or
a Custom Atlas. Standard accepted templates do not pretend these controls work: the controls are
hidden or visibly locked with `Customize Layout` as the explicit action.

Arbitrary visual rotation of the rectangular hotspot is not supported. A 90-degree topology
rotation swaps axes and dimensions. Texture rotation inside the region is a content transform.

### 4.2 Region content transform

This changes how texture content is sampled inside a stable region:

```text
move source footprint
scale/crop source footprint
rotate sampled texture freely
mirror
edit perspective points
change repeat and seam
apply polar, spiral, lens, or cylindrical warps
```

It is an appearance edit. It is always available for an editable binding and does not move the
region, change its ID, or invalidate Blender UV assignments.

The inspector and toolbar must label these modes `Layout` and `Content`. The same icons must not
silently switch meanings between them.

---

## 5. Layout generation system

Layout generation is a first-class topology authoring workflow, not a one-shot call to the generic
packer.

### 5.1 Generator lifecycle

```text
choose generator recipe
-> edit recipe parameters
-> Generate Candidate
-> inspect candidate topology and compiled material preview
-> optionally manipulate candidate regions
-> validate and compare compatibility
-> Accept Layout
-> pin immutable topology snapshot
```

`Generate Candidate` never mutates the accepted document. It returns a `TopologyCandidate` with a
candidate ID, generator version/seed, validation report, and preview compile plan. Closing or
cancelling discards it. `Accept Layout` is one undoable topology command.

After acceptance, material swaps, patch assignments, content warps, profiles that do not change
hotspot geometry, treatments, and map settings never repack the sheet.

### 5.2 Generator domain

```rust
pub struct LayoutGeneratorRecipe {
    pub kind: GeneratorKind,
    pub version: SemVer,
    pub seed: u64,
    pub canonical_size: PixelSize,
    pub size_families: Vec<SizeFamilyRecipe>,
    pub orientation_policy: OrientationPolicy,
    pub radial_policy: RadialPopulationPolicy,
    pub packing_policy: PackingPolicy,
    pub profile_policy: ProfileAssignmentPolicy,
    pub reserved_banks: Vec<ReservedBank>,
}
```

Required generator tools:

- Recursive size-family generator: broad, medium, small, micro-trim, and detail populations.
- Horizontal trim-bank generator.
- Vertical panel/trim-bank generator.
- Balanced architecture generator.
- Dense detail-field generator.
- Radial-accent population generator.
- Reserved-bank generator for strips, caps, radial regions, or project-specific groups.
- Seeded general packer for Custom Atlas.
- Clone accepted template as Custom and manipulate its topology.

All generators use the same region/profile contracts. First-party recipes are parameter presets,
not separate hardcoded renderers.

### 5.3 Generator controls

The Layout Generator drawer exposes:

```text
Recipe and seed
Output aspect/canonical grid
Outer margin, padding, bleed, and alignment
Occupancy target
Size-family counts and weights
Minimum/maximum region dimensions
Aspect-ratio ranges
Horizontal/vertical population mix
Strip, unique-detail, cap, and radial quotas
Reserved banks and grouping
Profile family per group
Bevel/seam/inset/radius defaults
```

The candidate summary reports:

```text
region count by family and orientation
radial-region count and radii
occupancy and unused area
minimum padding/bleed
overlap/out-of-bounds/minimum-size errors
profile and ID-map validity
topology/Blender compatibility impact
```

### 5.4 Generator determinism

The same recipe JSON, seed, generator version, and canonical size must produce byte-identical:

```text
region ordering
allocation/hotspot rectangles
semantic roles and groups
profile assignments
stable IDs and ID colors
topology hash
```

Golden fixtures qualify every first-party recipe at supported canonical/output sizes.

---

## 6. Radial, spiral, lens, and projection authoring

The user-facing feature is **Mapping & Warp**, not material estimation. Do not describe polar or
spiral mapping as wood end-grain generation.

Three independent concepts remain separate:

1. **Radial UV fit:** Blender metadata saying a mesh island is fitted radially.
2. **Radial structural profile:** analytic disc, annulus, rim, lip, or radial bevel height/normal.
3. **Radial/spiral content mapping:** a UV warp that changes how any source texture is projected
   inside any region.

A rectangular region may use radial mapping. A radial-fit region may use ordinary planar mapping.

### 6.1 Mapping and warp tools

Initial projections:

- Planar.
- Perspective quad.
- Polar/Radial.
- Cylindrical/Arc.

Initial composable warp operations:

- Spiral/Twirl.
- Radial Lens with barrel, pincushion, and fisheye ranges.
- Planar scale/offset/bias.
- Additional perspective warp.
- Mirror and rotation.

Address modes:

- Clamp.
- Repeat.
- Mirrored repeat.
- Transparent/outside mask when supported by the binding.

### 6.2 Radial Pattern tool

Selecting a region and choosing `Mapping & Warp > Radial Pattern` creates an editable mapping
recipe; it does not cut a circle or alter region topology.

Default recipe:

```text
Polar/Radial projection
+ optional Spiral/Twirl operation
+ optional Radial Lens operation
```

Direct controls appear over the source and output preview:

- center of rotation;
- inner and outer radius;
- angular seam/start angle;
- clockwise/counter-clockwise direction;
- radial and angular scale;
- turns/twist strength and falloff;
- lens strength, radius, bias, and falloff;
- source-axis choice;
- repeat/clamp addressing.

The left canvas shows the source grid/footprint and warp handles. The right canvas shows the
selected compiled region in place. Both are projections of the same draft recipe.

### 6.3 Warp safety and map correctness

- Operations are typed, ordered, versioned, enableable, reorderable, duplicable, and removable.
- Operation count, radius, strength, turns, sampling scale, and intermediate allocations are
  bounded.
- Singular perspective quads, invalid radii, NaN/infinity, and excessive derivatives fail on the
  affected region with a repair action.
- All registered maps use the same coordinate transform.
- Tangent-space normals are reoriented by the local mapping Jacobian, including handedness changes
  from mirror, polar, twirl, lens, and cylindrical mappings.
- CPU final and interactive preview share the same recipe and have golden parity fixtures.

---

## 7. Material and map generation pipeline

The compiler resolves every enabled region in this order:

```text
resolve content reference and registered source maps
-> rectify patch or source projection
-> evaluate ordered mapping/warp recipe
-> sample Base Color and authored companion maps
-> apply region variation
-> evaluate analytic structural profile and masks
-> composite decorations
-> compose imported/estimated material height with structure
-> generate and correctly compose tangent-space normals
-> apply ordered treatment/weathering layers
-> apply padding, dilation, and mip-safe bleed
-> raster exact Region ID and Material ID
-> publish registered map set and diagnostics
```

### 7.1 Authoritative channels

```text
Base Color
Height
Normal
Roughness
Metallic
Ambient Occlusion
Opacity
Region ID
Material ID
named generated masks
```

Packed channels such as ORM are export views, never internal sources of truth.

### 7.2 Source maps and estimated maps

- Imported maps are authoritative for their material channel.
- Missing Height, Roughness, AO, or Normal may be generated by explicit recipes and are labeled
  `Estimated` in previews and metadata.
- Metallic is imported or explicitly authored; it is never guessed from Base Color.
- The same source/patch geometry and mapping recipe applies to every channel.
- Normal maps are vector-composed, never averaged as RGB.

### 7.3 Structural generator tools

Profiles can be assigned globally, by generator group/size family, or per region:

```text
Flat
Convex bevel
Concave groove
Rounded bevel
Double bevel
Raised lip
Recessed seam
Panel frame
Radial disc
Annulus
custom analytic profile
```

Controls include width/radius, curve, inset/extrusion, edge selection, hardness, cavity response,
normal intensity, and reference-resolution scaling. Profiles generate Height, Normal, AO/cavity,
edge masks, and padding-aware dilation from the current authoritative region bounds.

### 7.4 Decoration and treatment generators

Decorations are bound to a region without changing topology:

```text
repeating strip
unique detail
trim cap
radial detail
seam/panel/bolt/pattern/decal
```

Treatments are ordered, nondestructive, maskable, and channel-targeted:

```text
Clean / Used / Heavy recipes
Grunge
Edge Wear
Dirt
Color/Roughness Adjust
Height Boost
Decal
Mask
```

Every generator, decoration, and treatment has a stable version and deterministic seed.

---

## 8. Interaction and transaction model

The workbench must not persist on every pointer move and then reconcile competing snapshots.

### 8.1 Gesture lifecycle

```text
pointer down
-> BeginDraftEdit(target_id, edit_kind, base_revision)
-> local/GPU draft updates during pointer motion
-> affected-region preview compiles from the draft recipe
-> pointer up sends one CommitDraftEdit(edit_id, final_value)
-> Rust validates and persists one command
-> UI keeps displaying the final draft until an acknowledgement with that commit revision arrives
```

Rules:

- Project snapshots older than the active draft or acknowledged commit are ignored for that target.
- A stale preview result cannot overwrite a newer draft or compiled artifact.
- Only one serialized mutation queue owns accepted document commands.
- Auto-generation is suspended for the target during its direct-manipulation transaction.
- Pointer cancellation or Escape discards the draft and restores the exact base value once.
- A failed command displays a region-specific error and restores once; it never falls through to
  resize, rotate, creation, or another transform mode.
- One completed drag, slider scrub, or point-edit gesture is one undo entry.

This eliminates move/rollback/replay oscillation.

### 8.2 Identity and double-click rules

- Creation happens only when an explicit creation tool is active.
- Double-clicking empty workbench space does nothing in Select mode.
- Double-clicking a patch calls `BeginPatchPointEdit(existing_patch_id)`.
- Double-clicking a region source footprint calls
  `BeginRegionSourcePointEdit(existing_region_id, existing_layer_id)`.
- Neither command allocates a new patch, region, or source layer identity.
- Region and patch counts/IDs are asserted unchanged by edit-mode entry tests.
- Canvas event handlers do not infer creation from a missed child hit-test.

### 8.3 Shared gizmo behavior

Patch geometry, global framing, region content mapping, and custom-layout regions use one gizmo
framework with capability flags:

```text
translate
scale
rotate
perspective points
warp center/radius/seam handles
```

The selected target declares supported operations. Unsupported operations are not shown. Handles
have minimum hit areas, pointer capture, cursor feedback, keyboard equivalents, and explicit
active-state highlighting.

### 8.4 Cover/crop behavior

Cover mode exposes the actual effective aspect-locked crop rectangle:

- sheet framing uses sheet aspect;
- region override uses destination-region aspect;
- changing crop size moves the opposite edge or center according to the active anchor;
- moving the crop changes focus directly, so detached Focus X/Y sliders are unnecessary;
- numeric X/Y/width/height values describe the same visible rectangle;
- reset restores one canonical full-coverage crop for the target aspect.

Repeat mode shows every wrapped source footprint rather than one false rectangle.

---

## 9. Workbench screen model

### Left: sources, patches, and content authoring

- Material-source library and registered maps.
- Explicit Patch Create tools and Select mode.
- Selected region's exact source footprint.
- `Sheet Framing`, `Region Content`, and `Patch` modes with visible breadcrumbs.
- Shared transform gizmo.
- Mapping & Warp stack with Polar/Radial, Spiral/Twirl, Radial Lens, and Cylindrical/Arc.

### Center/right: authoritative trim-sheet workpiece

- Always displays the current `CompiledSheet` map plus overlays from the same compile plan.
- Selecting a region synchronizes its source/patch and inspector.
- Map views: Beauty, Base Color, Normal, Height, Roughness, Metallic, AO, Region ID, Material ID,
  masks, and Hotspots.
- Candidate-layout mode visibly separates unaccepted topology from the accepted sheet.
- Material preview applies the same compiled maps to canonical wall, panel, strip, edge, and radial
  fixtures.

### Inspector

One contextual inspector, never duplicate controls in workpiece and sidebar:

```text
Selection identity and mode
Content binding
Mapping projection and warp stack
Transform values
Variation and addressing
Profile and weathering overrides
Decorations
Layout bounds only when topology editing is allowed
Diagnostics and revision/build status
```

Build status is explicit:

```text
Draft preview
Compiling revision N
Up to date at revision N
Needs rebuild
Error in Region X
```

---

## 10. Compiler and preview authority

### 10.1 Resolved compile plan

The compiler first resolves the document into a validated plan containing:

```text
exact output and canonical dimensions
authoritative allocation/hotspot rectangles
resolved content and source-map checksums per region
rectified patch/source buffers
typed mapping and warp programs
profile, decoration, treatment, and blend programs
channel policies
cache keys and memory estimate
```

Overlay data is returned from this plan. UI code does not independently reconstruct template
bounds or source footprints.

### 10.2 Compiled artifact

```rust
pub struct CompiledSheet {
    pub document_revision: u64,
    pub topology_hash: Hash,
    pub appearance_hash: Hash,
    pub renderer_version: SemVer,
    pub dimensions: PixelSize,
    pub maps: CompiledMapSet,
    pub regions: Vec<CompiledRegionMetadata>,
    pub diagnostics: Vec<CompileDiagnostic>,
}
```

Template, Generated Template, Custom Template, and Custom Atlas all return this artifact. Atlas is
not allowed to substitute CSS image backgrounds for raster composition.

### 10.3 Incremental preview

- Pointer motion recompiles the affected region at bounded preview resolution.
- Accepted commands invalidate affected region/channel tiles through deterministic cache keys.
- Background full-sheet compilation is cancellable and publishes atomically.
- The last valid artifact remains visible until the next complete artifact is ready.
- Preview results are accepted only when document revision, draft edit ID, and input hash match.

### 10.4 Export and Blender parity

Generic export and Blender publish use a completed `CompiledSheet`; they never invoke a second
appearance renderer. Export may re-encode or pack channels but cannot recompute region content.
The manifest references the exact topology and appearance hashes of the maps it ships.

---

## 11. Domain commands

Commands are implemented in Rust and used by UI, persistence, undo/redo, tests, recovery, export,
and future automation.

### Document and topology

```text
CreateTrimSheetDocument
GenerateTopologyCandidate
UpdateTopologyCandidateRecipe
TransformCandidateRegion
AcceptTopologyCandidate
DiscardTopologyCandidate
CloneTopologyAsCustom
TransformCustomRegion
SetOutputResolution
```

### Content and mapping

```text
SetRegionContent
ResetRegionContentToMaterial
SetSheetFraming
SetRegionProjection
SetRegionSourceGeometry
SetRegionMappingTransform
AddRegionWarp
UpdateRegionWarp
ReorderRegionWarp
EnableRegionWarp
RemoveRegionWarp
SetRegionAddressMode
SetRegionVariation
```

### Profiles, maps, decorations, and treatments

```text
SetRegionProfile
SetGroupProfile
SetGeneratedMapRecipe
AddDecoration
UpdateDecoration
RemoveDecoration
AddTreatmentLayer
UpdateTreatmentLayer
ReorderTreatmentLayer
RemoveTreatmentLayer
```

### Publishing

```text
CompilePreview
CompileFinal
ExportCompiledSheet
PublishBlenderRevision
```

Every command declares whether it is appearance-only or topology-changing before validation.

---

## 12. Persistence and migration

Use the next schema version after the repository's current version. The persisted model must include:

```text
trim_sheet_documents
accepted_topology_snapshots
topology_regions
topology_generator_recipes
topology_candidates or recoverable candidate drafts
region_bindings
region_mapping_recipes
region_warp_operations
decorations
treatment_layers
compiled_artifact_metadata/cache references
```

Requirements:

- Migrate current template snapshots, source framing, fills, and source layers into the canonical
  document without losing IDs, patches, maps, or geometry.
- Resolve duplicated legacy item/region values with an explicit migration rule and fixture; do not
  continue storing both.
- Legacy generic layouts become `CustomAtlas`.
- Existing built-in template layouts preserve compatibility only when their pinned region/hotspot
  geometry matches the registered template snapshot.
- Migration is transactional. Failure leaves the previous project valid.
- Reopen never reruns a generator. Accepted topology snapshots are immutable.
- Draft transactions are not persisted as accepted state; crash recovery restores the last accepted
  command plus any explicitly supported recoverable candidate.

---

## 13. Vertical implementation program

Each milestone owns domain, persistence, compiler, IPC, desktop behavior, and one end-to-end test.
No milestone is complete when it only adds controls, metadata, or isolated renderer functions.

### Milestone 0: Freeze broken behavior with executable acceptance fixtures

Add a wide numbered/checker material, contrasting patch, authored companion maps, and small layout
fixture. Create failing tests for:

- square cover crop and direct crop movement;
- selected region content transform changing compiled pixels;
- patch assignment changing exactly one region;
- edited region bounds matching raster bounds in custom topology;
- Normal/Roughness/Height following identical geometry;
- Atlas returning a real compiled artifact;
- double-click edit preserving region/patch count and identity;
- rapid drag ending at the last position without rollback/replay;
- save/reopen reproducing the final mapping and compiled hash.

**Exit:** The failures reproduce the reported product defects without manual interpretation.

### Milestone 1: Canonical document and truth renderer

Implement the document, revisions, resolved compile plan, migration, and `CompiledSheet`. Support the
smallest complete path: one material, one accepted topology, all registered source maps, exact
region bounds, 2D preview, save/reopen, and export of the same artifact.

**Exit:** Changing the one authoritative binding/bounds value changes final pixels and survives
reopen; no legacy parallel state is consulted by the compiler.

### Milestone 2: Stable interaction transactions and shared gizmos

Implement draft edit IDs, stale-snapshot rejection, one commit per gesture, explicit creation tools,
shared gizmos, direct cover crop, patch edit, and region content edit.

**Exit:** Move/scale/rotate/perspective editing is responsive, persistent, undoable, and never
duplicates or visually oscillates.

### Milestone 3: Patch and multi-source region bindings

Resolve material or patch content per region; rectify patches once; sample every registered map;
add fallback and atomic deletion behavior.

**Exit:** Assigning a patch or secondary material changes exactly the selected region in every map,
and deleting it restores the documented fallback in one undoable command.

### Milestone 4: Procedural topology generator and candidate/accept workflow

Implement generator recipes, deterministic size families, orientation/radial quotas, seeded packing,
candidate preview, manual candidate manipulation, validation, acceptance, pinned provenance, and
compatibility reporting. Ship at least Balanced Architecture, Horizontal Trim Bank, Vertical Panel
Bank, and Radial Accent recipes.

**Exit:** Users can generate materially different valid layouts, inspect them with real compiled
maps, accept one, reopen it unchanged, and regenerate a candidate without mutating the accepted one.

### Milestone 5: Mapping & Warp including radial pattern creation

Implement Planar, Perspective, Polar/Radial, Cylindrical/Arc, Spiral/Twirl, and Radial Lens with
direct center/radius/seam handles, typed persistence, CPU evaluation, bounded diagnostics, and normal
Jacobian correction.

**Exit:** A selected region can turn any source texture into a radial, fisheye, spiral, or arc-mapped
result without changing the region rectangle; preview, final render, reopen, undo, and export agree.

### Milestone 6: Structural and generated map tools

Implement region/group profile assignment, analytic rectangular/radial profiles, Height and normal
composition, AO/cavity/edge masks, estimated-map labeling, padding, dilation, and resolution scaling.

**Exit:** Generator/profile controls produce correct map boundaries at 1K, 2K, 4K, and 8K, and
imported maps replace estimated channels without changing topology.

### Milestone 7: Decorations and nondestructive treatments

Implement strip, unique, cap, radial, decal, and procedural decorations plus ordered treatment
layers and Clean/Used/Heavy recipes.

**Exit:** Multiple details and treatments coexist, target selected channels/regions, remain
deterministic, and survive reorder/save/reopen.

### Milestone 8: Custom topology and Atlas on the same compiler

Reconnect advanced packing and freeform layout editing through topology candidates/custom topology.
Use the canonical renderer and artifact for both template and Atlas.

**Exit:** Custom region move/resize/90-degree rotate, collision repair, packing, preview, export, and
undo all operate on one document; Atlas never falls back to CSS-only visualization.

### Milestone 9: Material preview, generic export, and Blender package

Complete the canonical material scene, channel inspection, atomic export, manifest hashes, package
revision publishing, Blender material update, hotspot browser, rectangular/strip/radial fitting, and
appearance-versus-topology update handling.

**Exit:** The exact current `CompiledSheet` is visible in 2D/3D, exported, and applied in Blender;
appearance changes update maps without remapping UVs.

### Milestone 10: Release qualification

Run migration, crash recovery, cancellation, bounded-resource, Unicode/path, permissions, high-DPI,
keyboard/accessibility, deterministic-build, and clean-install gates.

**Exit:** No supported workflow can display a successful control state without a persisted command
and matching final artifact.

---

## 14. Required acceptance scenarios

### A. Cover crop

With a 2:1 source and square sheet, the source canvas displays a square effective crop. Moving and
resizing it updates the square sheet, persists, and reproduces after reopen.

### B. Region transform persistence

Select a region, enter Content mode, move/scale/rotate it rapidly, and release at C after passing A
and B. The UI never shows A or B after C. The persisted recipe and reopened project equal C.

### C. Double-click identity

Double-click a selected patch or region source footprint and edit its four points. Patch count,
region count, patch ID, region ID, and source-layer identity remain unchanged.

### D. Patch assignment

Assign a contrasting patch to `Wall Secondary`. Only that region changes in Base Color and every
available companion map. Undo and deletion restore its primary-material fallback.

### E. Custom region layout

Clone a template as Custom, move and resize `Wall Secondary`, and accept it. Overlay, Base Color,
Height, Normal, ID maps, hotspot metadata, export, and reopen use identical bounds.

### F. Procedural generation

Generate a sheet with configured broad fields, horizontal/vertical strips, micro details, and three
radial regions. The same recipe/seed is byte-identical; a different seed produces a valid different
candidate without modifying the accepted topology.

### G. Radial pattern

Select a rectangular region, choose Radial Pattern, move the center, add twirl, and apply fisheye
lens strength. The rectangle and hotspot remain unchanged while texture projection changes in every
map. Normal orientation passes a reference-vector fixture.

### H. Map generation

Apply a rounded bevel plus recessed seam. Height, Normal, AO, cavity, and edge masks follow current
region boundaries at all supported resolutions; padding does not alter hotspot bounds.

### I. Atlas parity

Pack patches into a Custom Atlas and compile it. The workpiece, material preview, exported maps, and
manifest all reference the same artifact hashes.

### J. Blender stability

Publish, assign UV islands, then change crop, warp, source material, and treatment layers. Blender
updates maps without changing UV assignments. Accepting a topology change produces a compatibility
report before any remapping.

---

## 15. Stop conditions

Do not advance when any of the following are true:

- A control changes only TypeScript state or overlay geometry.
- The compiler does not consume patch/material binding, current bounds, and mapping recipe.
- Template and Atlas return different notions of preview/final output.
- Imported maps bypass the selected region mapping.
- An accepted topology is regenerated implicitly.
- Layout and Content transform modes are ambiguous.
- A creation command can be triggered from Select mode.
- Pointer moves publish competing persisted snapshots.
- A stale project or preview revision can overwrite a newer draft/commit.
- Radial fit, radial profile, and radial content mapping are conflated.
- Polar, spiral, or lens mapping is described as a material-specific estimation feature.
- Normal maps are warped without vector-direction correction.
- Preview, export, and Blender can render appearance through different code paths.
- Tests assert only metadata or PNG existence instead of changed target pixels and stable identity.

---

## 16. Definition of the finished product loop

```text
Import one or more registered material sources
-> choose or generate a layout recipe
-> inspect and accept the topology
-> select regions and bind sources or patches
-> directly crop, rectify, repeat, rotate, or warp their content
-> add radial/spiral/lens/arc mappings where useful
-> assign structural profiles and generated map recipes
-> add decorations and nondestructive treatments
-> inspect the same compiled sheet in 2D and 3D
-> export or send that exact artifact to Blender
-> update appearance later without invalidating stable hotspot UV assignments
```

Every step is connected through the same document, revision, compiler, and artifact. That is the
non-negotiable completion criterion for this redesign.
