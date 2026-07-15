# Hot Trimmer MVP Plan

Status: Draft 0.1  
Product name: Hot Trimmer  
Working thesis: one or more images in, game-ready trim-sheet maps out

## 1. Product Thesis

Hot Trimmer is a focused tool for turning images into usable trim sheets. The MVP should not inherit
the broad TrimSheetCreator application model. It should start from the smallest useful loop:

```text
sources -> patches and layout -> maps and polish -> preview and export
```

These are product capabilities and dependency order, not a seven-page wizard. Users can move between persistent
work modes whenever the required data exists. Anything that mostly serves large library
management, arbitrary DCC authoring, complex workspace docking, or Blender-style outliner behavior is
deferred.

## 2. Core Objects

### Project

A saved workspace containing imported images, patch definitions, generated trim layouts, map settings,
layers, preview settings, export settings, and recovery data.

### Material Source

One material idea backed by one or more original imported images. A source may be a photo, scan, screenshot,
already-flat texture, or registered PBR set. It is never destructively edited. A project may contain many
ordered material sources. Each owns up to ten explicit map roles: Base Color/Diffuse, Normal, Height/Bump,
Roughness, Metallic, AO/Cavity, Specular, Opacity, Edge Mask, and Material ID. Companion maps register to that
source's Base Color dimensions, and each material source may own zero or many patches.

### Patch

A selected part of a material source. Users create a patch by placing four points or by drawing a simple
rectangle. Hot Trimmer rectifies the patch into a usable rectangular texture region.

Patch is the user-facing word. Avoid "quad", "ripped asset", and "source reference" in the main UI.

### Trim Layout

The generated or manually authored arrangement of independent regions into a sheet. A region may use an entire
material source, a rectified patch, or a simple fill. The layout owns region size, order, padding, bleed, repeat
behavior, trim caps, output resolution, and channel registration. A useful layout does not require patches.

### Layer

A non-destructive operation applied to the trim layout or to one patch. Layers include patch layers,
generated-map layers, masks, grunge, edge wear, dirt, decals, roughness edits, height boosts, and ID
regions.

Use the word Layers, not Stack.

### Maps

The output channels:

- Base Color.
- Normal.
- Height.
- Roughness.
- Metallic.
- Ambient Occlusion / Cavity.
- ID Map.

## 3. MVP Work Modes

The top-level UI uses one integrated authoring workbench plus contextual later-stage tools:

```text
Workbench & Hotspot Sheet | Layers & Maps                          Export | Send to Blender
```

Opening images, Export, and Send to Blender are actions, not modes. New users can choose **Open images** first;
Hot Trimmer then asks where to save the durable project. Modes preserve selection and camera state.

### Current Implementation Status

- **Implemented:** direct/multi-image start, Open all assignment, the initial registered material source,
  integrated patch workplace, automatic arbitrary-order capture, direct transforms/point editing, live
  rectification, project persistence, autosave/baselines/recovery, migrations, recent projects, and native
  lifecycle actions.
- **Specified for later phases:** multiple ordered material-source sets and independent sheet regions (Phase 3),
  map generation (Phase 4), nondestructive multi-source treatment layers (Phase 5), Export, and Send to Blender.

### Material Sources

The first screen has one obvious action: Open images. Once a project exists, source management lives in the left
workplace and manages registered material inputs as recognizable sets rather than a separate tab or node graph.

Each material source supports:

- One image that is already a texture.
- One image that needs patch extraction and perspective correction.
- A registered set of related PBR maps when the user already has them.
- Zero or many optional captured patches.

**Open all** accepts a texture set, assigns common filename suffixes such as Albedo, Normal, Roughness, Metallic,
AO, Height, and Material ID, and imports Base Color first. It fills empty slots and never silently replaces a
filled one. If only one image exists, treat it as Base Color. Slot cards and the source tray show a
channel-appropriate swatch, actual filename, dimensions, assignment state, and registration errors. The selected
slot shows the original file path. Ownership and color-management policy remain internal safety behavior rather
than everyday inspector controls.

The project name is directly editable in the top bar. Pan, zoom percentage, zoom buttons, and Fit form a compact
viewport HUD at lower left; pixel coordinates remain at lower right.

### Workbench - Material Sources and Patch Authoring

Primary action: Add Patch.

Flow:

1. The left workplace selects one of the project's material sources and shows its registered maps and patches.
2. User presses `N` or chooses four-point/rectangle capture.
3. Four clicks may occur in any order and auto-complete after canonicalization; rectangle capture auto-completes
   on pointer release. There is no separate commit ceremony.
4. The new patch remains selected. Drag inside to move, use transform handles to resize/rotate, or double-click
   to edit labeled TL/TR/BR/BL points.
5. The right workpiece updates rectification immediately from cached source pixels while authoritative native
   refinement follows in the background.
6. One persistent patch list supports double-click rename, drag reorder, and context-menu duplicate/delete.

Polygon-assisted capture may accept four to eight boundary points. It derives a best-fit editable quadrilateral
and keeps the original polygon as an optional mask; it does not pretend an arbitrary octagon has an exact
four-point perspective solution.

Patch properties:

- Name.
- Repeat X.
- Repeat Y.
- Tile XY.
- Stretch.
- Unique.
- Trim Cap.
- Padding and bleed.
- Material ID label.
- Use for map generation.

### Hotspot Workpiece - Sheet Layout

Primary action: Create Trim Sheet.

The app creates an initial trim layout from whole material sources, optional captured patches, or both. Project
creation or first layout entry offers
explained presets such as Balanced, Horizontal Trims, Vertical Trims, Modular Kit, and Atlas. A preset is only
editable starting intent: users may change it later without losing patch definitions or manual locks.

Layout controls:

- Output resolution.
- Padding and bleed.
- Preserve patch order.
- Auto pack.
- Horizontal strip priority.
- Vertical strip priority.
- Fixed size for selected patch.
- Region fill: whole source, captured patch, or simple fill.
- Horizontal Loop, Vertical Loop, Tile, Stretch, Unique Detail, and Trim Cap behavior per region.
- Trim-cap handling.

Users can drag boundaries, reorder regions, lock dimensions, and rerun layout without losing source or patch
definitions. The source/patch workplace stays on the left and the authoritative hotspot sheet stays on the right.

### Real-Time Rectification

Rectification is part of the fixed right workpiece, not a floating card over the source. Cached GPU feedback
updates throughout pointer movement; deterministic CPU refinement follows without replacing the interaction
with a loading state.

### Maps & Polish - Generate Maps

Primary action: Generate Maps.

Generated maps are clearly labeled as estimated. They are deterministic authoring aids, not measured
physical truth.

Required map generation:

- Height from luminance/detail.
- Normal from height.
- Roughness from heuristic controls.
- AO/Cavity from height.
- Metallic from user labels or imported maps, not silent guessing.
- ID Map from regions and material labels.

Imported slots and generated outputs share the same channel vocabulary. Map range, bump/normal strength,
invert, and interpretation controls are contextual; the MVP does not expose a general node graph.

### Maps & Polish - Treatments

Treatments are layers. They may target a whole layout, material source, region, or patch. Imported sources may
also serve as transformed fill or mask inputs, allowing a grunge image to mask rust/weathering over a separate
metal material without flattening either source.

MVP treatment layers:

- Grunge.
- Edge Wear.
- Dirt.
- Color Adjust.
- Roughness Adjust.
- Height Boost.
- Decal.
- Mask.

Layer controls:

- Visibility.
- Opacity.
- Blend mode.
- Channel targets.
- Mask input.
- Seed.
- Strength.
- Scale.
- Invert.

No node graph in the MVP.

### Embedded Preview Content

The preview answers whether the trim sheet works on actual geometry.

Preview meshes:

- Plane.
- Cube.
- Sphere.
- Cylinder.
- Beveled block.
- Crate.
- Wall module.
- Archway.

At least one MVP mesh should be pre-hotspotted so the user can judge trim usage, not just material
surface quality.

### Export Action

Export is a persistent top-level action that opens output settings without changing authoring mode. **Send to
Blender** is a separate adjacent action for the Blender integration path; it is not hidden inside generic Export.

Default export maps:

- Base Color.
- Normal.
- Roughness.
- Metallic.
- Height.
- Ambient Occlusion.
- ID Map.

Optional advanced exports:

- Blender material package.
- Preview render.
- Region guide.

Region guide is diagnostic and should not be a default export.

## 4. Map Generation Math

Map generation should be deterministic and inspectable before any AI-assisted ideas enter the
product.

### Height

Start from luminance:

```text
luma = 0.2126 R + 0.7152 G + 0.0722 B
```

Build height from:

- Large-scale shape from blurred luminance.
- Fine detail from high-pass luminance.
- Contrast remap with midpoint, gain, and clamp.
- Optional de-lighting before extraction.

Controls:

- Detail radius.
- Large-shape radius.
- Strength.
- Midpoint.
- Invert.
- Clamp low/high.
- Preserve edges.

### Normal

Generate tangent-space normals from height gradients:

```text
dx = d(height) / dx
dy = d(height) / dy
normal = normalize((-dx * strength, -dy * strength, 1))
```

Use Sobel or Scharr gradients. Support OpenGL and DirectX orientation by flipping the green channel.

Controls:

- Strength.
- Detail scale.
- Blur before gradient.
- OpenGL / DirectX.
- Normalize output.

### Roughness

Roughness cannot be reliably inferred from color alone. Treat it as a controllable heuristic.

Inputs:

- Luminance.
- Local contrast.
- High-frequency detail.
- Material ID label.
- Optional imported gloss/roughness map.

Controls:

- Base roughness.
- Detail influence.
- Contrast influence.
- Invert.
- Min/max clamp.
- Per-patch override.

### Metallic

Default metallic to 0 unless:

- The user labels a patch as metal.
- A metallic map is imported.
- A material ID rule assigns metallic.

Do not silently guess metallic from photo color.

### AO / Cavity

Approximate cavity from multi-radius height differences:

```text
cavity = clamp(blur(height, small_radius) - blur(height, large_radius))
```

Controls:

- Radius.
- Strength.
- Bias.
- Invert.
- Use as AO map or mask only.

### De-lighting

For photos, estimate and reduce low-frequency lighting:

- Estimate illumination with a large blur or Retinex-style low-frequency layer.
- Divide or subtract illumination from base color.
- Clamp highlights and shadows.

Controls:

- Amount.
- Illumination radius.
- Shadow recovery.
- Highlight recovery.
- Preserve color.

### ID Maps

ID maps are required.

Types:

- Region ID Map: each layout region gets a stable flat color.
- Material ID Map: regions sharing a material label get a shared color.
- Layer ID Map: optional later.

ID colors must be stable across save/reopen and layout regeneration.

## 5. MVP Non-Goals

Defer:

- Global material library management.
- Large folder indexing.
- Smart material marketplace.
- Blender-style outliner.
- Separate UV-set management.
- Complex docking/window management.
- Node graph authoring.
- Online providers.
- Multi-document asset browser.

## 6. Acceptance Criteria

The MVP is credible when a user can:

1. Open one image directly, create its durable project, and optionally fill related material-input slots.
2. Mark several patches with four-point correction.
3. Set repeat behavior per patch.
4. Generate a trim layout.
5. Generate Normal, Height, Roughness, AO/Cavity, Metallic, and ID maps.
6. Add grunge or edge wear as layers.
7. Preview on at least one hotspotted mesh.
8. Export maps to a folder and use them in Blender.
9. Save, close, reopen, and keep patches, map settings, layers, and export settings intact.
