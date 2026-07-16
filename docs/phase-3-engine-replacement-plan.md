# Phase 3 Engine Replacement Plan

Date: 2026-07-15
Status: Approved for implementation

## Product contract

Phase 3 has three explicit layout kinds:

1. Hotspot Template: deterministic, versioned reusable rectangle topology.
2. Trim Template: deterministic, versioned reusable band topology.
3. Patch Atlas: generic packing of explicitly included authored patches.

Hotspot and Trim never use the generic packing engine. Atlas never invents whole-source regions.

## Hotspot Texturing 101 guide constraints

The supplied Daveface guide is authoritative for the initial hotspot workflow. The implementation must preserve these principles:

- A hotspot layout is a reusable reference-mesh UV topology, not a generic best-fit image pack.
- There is no single perfect layout. Template choice depends on the intended asset family and the surface shapes it needs.
- Wall, brick, concrete, and other broad environmental surfaces need a small number of large rectangles and simple strips.
- Metal, wood, props, and intricate assemblies need more narrow strips and small rectangles for bolts, beams, edge treatments, and localized detail.
- The same stable layout should be reusable across many compatible materials so geometry can switch materials without changing its UV topology.
- Template regions must be rectangular and designed to match the texel size and shape of the model surfaces that will be mapped to them.
- Beveled edges are a first-class part of the workflow. Template boundaries and generated supporting maps must allow consistent edge highlights and wear.
- The reference workflow assumes approximately 45-degree bevel transitions where the baked hotspot edge profile requires them; mesh bevel width must remain consistent with the authored reference.
- Normal-map generation must be driven by a matching beveled reference mesh or equivalent deterministic edge profile, rather than arbitrary packed-region borders.
- Every region needs at least four output pixels of bleed at its final resolution. Higher-resolution and mip-sensitive outputs may require more.
- Detail that must not stretch, such as bolts or unique markings, belongs in dedicated slots or explicit patch overrides.
- Hotspot assignment in Blender is a downstream UV-matching operation: match rectangular UV islands to compatible template slots while preserving texel-density policy.

The broad-lower-field/progressive-upper-width layout is the default architectural template, not a universal layout. Additional reference templates are required for walls, modular architecture, beams and metalwork, and detail-heavy props.

## Data model

The authoritative layout stores intent separately from solved pixel output.

- Layout kind and output settings.
- Template ID and version for Hotspot or Trim.
- Stable template slots with normalized bounds, orientation, default behavior, and bevel metadata.
- Optional patch-to-slot assignments with framing and behavior overrides.
- Explicit atlas patch entries with packing constraints and locks.
- Derived solved regions used for preview and rendering.

Deleting an assigned patch is one undoable compound mutation: clear its assignments, restore base-material fallback, delete the patch, and regenerate.

## Execution order

### 1. Contracts and persistence

- Add explicit layout-kind and template contracts.
- Add stable slot IDs and patch assignments.
- Version persistence and preserve existing source sets and patches.
- Treat legacy packed Phase 3 layouts as Atlas-compatible data, not valid templates.

### 2. Template engine

- Define a default architectural hotspot template with a broad lower field and progressive upper widths.
- Define initial horizontal and vertical trim templates.
- Add template metadata for intended asset family, reference texel density, supported surface aspect ranges, and reference bevel width.
- Convert normalized slot boundaries directly to shared snapped pixel edges.
- Keep slot topology stable across material changes and output resolutions.
- Apply template-aware gutters with a hard minimum of four final-output bleed pixels per region.
- Keep large general surfaces, repeating trims, and non-stretchable unique details as distinct slot roles.
- Reserve deterministic edge-profile metadata for Height and Normal generation from a matching beveled reference.
- Treat the selected template as the saved reference layout used by downstream UV matching.

### 3. Automatic one-image workflow

- Importing the first valid Base Color immediately creates the default hotspot sheet.
- Regeneration keys off authoritative content and layout revisions rather than source-set IDs.
- Empty projects show neutral onboarding and never invoke a failing solve.
- Companion maps reuse identical slot IDs and pixel boundaries.

### 4. Patch overrides

- Creating a patch leaves it in the source library.
- Selecting a template slot allows assigning or clearing one patch.
- An assigned patch changes only that slot.
- Unassigned slots use the base material.
- Deleting an assigned patch restores fallback content automatically.
- Patches containing bolts, markings, or other exact details target compatible unique-detail slots unless the user explicitly overrides the warning.

### 5. Atlas isolation

- Atlas input is the explicitly included enabled patch set.
- Preserve rectified aspect ratio, padding, bleed, optional rotation, and locks.
- Repack unlocked entries without affecting template layouts.
- Provide Scale to fit, Increase output, Add page, or Unlock and repack recovery where required.

### 6. UI replacement

- Expose Hotspot, Trim, and Atlas as explicit modes.
- Expose template choice and output resolution in the output header.
- Make output slots selectable and assignable.
- Place contextual properties in the right inspector.
- Remove the permanent bottom packing spreadsheet from template modes.
- Add visible patch-row actions and keyboard deletion.
- Use readable typography and a dependable window-caption drag surface.

## Acceptance criteria

- One Base Color immediately produces a recognizable default hotspot sheet.
- No patch is required and no red empty-state error appears.
- Replacing Base Color regenerates even when the source-set ID is unchanged.
- Creating a patch does not modify a template layout until assignment.
- Assigning a patch changes exactly one slot.
- Deleting an assigned patch restores that slot and is undoable.
- Template slot IDs and normalized boundaries remain stable through regeneration.
- The same template can be reused across multiple compatible materials without moving UV boundaries.
- Template metadata identifies its target asset family and reference texel density.
- Every generated region has at least four final-output bleed pixels.
- The architectural default contains broad wall-scale regions; a detail-oriented template contains narrow strips and small exact-detail regions.
- Normal and Height edge treatment follows template bevel metadata and does not infer bevels from Atlas packing borders.
- Atlas contains only explicitly included patches.
- Hotspot and Trim cannot return generic impossible-pack errors.
- Preview movement never paints a region at old and new positions simultaneously.
- Functional text is at least 11 px, body text is at least 13 px, and primary targets are at least 30 px.
- The focused project gate passes and an optimized native release executable is produced.
