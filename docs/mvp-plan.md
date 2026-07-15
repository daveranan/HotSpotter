# Hot Trimmer MVP Plan

Status: Draft 0.1  
Product name: Hot Trimmer  
Working thesis: one or more images in, game-ready trim-sheet maps out

## 1. Product Thesis

Hot Trimmer is a focused tool for turning images into usable trim sheets. The MVP should not inherit
the broad TrimSheetCreator application model. It should start from the smallest useful loop:

```text
Open image -> mark patches -> create trim layout -> generate maps -> add treatments -> preview -> export
```

Every first-pass feature must support that loop. Anything that mostly serves large library
management, arbitrary DCC authoring, complex workspace docking, or Blender-style outliner behavior is
deferred.

## 2. Core Objects

### Project

A saved workspace containing imported images, patch definitions, generated trim layouts, map settings,
layers, preview settings, export settings, and recovery data.

### Source Image

The original imported image. It may be a photo, scan, screenshot, or already-flat texture. It is never
destructively edited.

### Patch

A selected part of a source image. Users create a patch by placing four points or by drawing a simple
rectangle. Hot Trimmer rectifies the patch into a usable rectangular texture region.

Patch is the user-facing word. Avoid "quad", "ripped asset", and "source reference" in the main UI.

### Trim Layout

The generated arrangement of patches into a sheet. It owns region size, order, padding, bleed, repeat
behavior, trim caps, output resolution, and channel registration.

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

## 3. MVP Workflow

### Step 1 - Open Image

The first screen has one obvious action: Open Image.

Supported inputs:

- One image that is already a texture.
- One image that needs patch extraction and perspective correction.
- A small set of related PBR maps when the user already has them.

If only one image exists, treat it as Base Color until the user changes the channel assignment.

### Step 2 - Mark Patches

Primary action: Add Patch.

Flow:

1. User clicks Add Patch.
2. User places four points on the source image.
3. Hot Trimmer previews the rectified patch.
4. User adjusts corners if needed.
5. User accepts the patch.
6. Patch appears in the right panel's Patches list.
7. Add Patch remains ready so the user can continue quickly.

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

### Step 3 - Create Trim Layout

Primary action: Create Trim Sheet.

The app creates an initial trim layout from marked patches. Users should get a useful first layout
without hand-placing everything.

Layout controls:

- Output resolution.
- Padding and bleed.
- Preserve patch order.
- Auto pack.
- Horizontal strip priority.
- Vertical strip priority.
- Fixed size for selected patch.
- Repeat behavior per patch.
- Trim-cap handling.

Users can drag boundaries, reorder patches, lock dimensions, and rerun layout without losing patch
definitions.

### Step 4 - Generate Maps

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

### Step 5 - Add Treatments

Treatments are layers.

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

### Step 6 - Preview

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

### Step 7 - Export

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

1. Open one image.
2. Mark several patches with four-point correction.
3. Set repeat behavior per patch.
4. Generate a trim layout.
5. Generate Normal, Height, Roughness, AO/Cavity, Metallic, and ID maps.
6. Add grunge or edge wear as layers.
7. Preview on at least one hotspotted mesh.
8. Export maps to a folder and use them in Blender.
9. Save, close, reopen, and keep patches, map settings, layers, and export settings intact.
