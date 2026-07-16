# Phase 3 Hotspot and Trim-Sheet UX Review

Date: 2026-07-15

## Executive verdict

The current Phase 3 is not a usable hotspot- or trim-sheet workflow. It combines three different products under one layout surface:

1. a standardized trim sheet,
2. a hotspot atlas driven by a reference rectangle catalog, and
3. a generic image atlas that bin-packs independent crops.

These workflows share an output texture, but they do not share the same layout rules. The implementation currently generates arbitrary whole-source rectangles, appends every enabled patch as another rectangle, and asks a generic packing solver to place them. That is why the output looks like repeated brick thumbnails instead of a designed trim or hotspot sheet, why normal interaction reaches impossible-fit errors, and why patches feel welded into the result.

This needs a Phase 3 product reset. The existing generic packer can remain for an explicit Atlas mode, but it must not define Hotspot or Trim behavior.

## What the terms mean

### Trim sheet

A trim sheet is a deliberately authored, reusable texture layout whose horizontal or vertical bands correspond to common architectural edges and surface spans. The geometry UVs are arranged against that stable layout. The key property is not merely that rectangles fit into one image; it is that the same normalized boundaries remain stable across a material family.

Insomniac's Ultimate Trim workflow describes a standardized UV layout shared by a trim material and a plain tileable material. That consistency lets materials be swapped while the modeled edges continue to land on the same texture boundaries. See the official GDC talk and slides:

- https://www.gdcvault.com/play/1022323/The-Ultimate-Trim-Texturing-Techniques
- https://media.gdcvault.com/gdc2015/presentations/Olsen_Morten_TheUltimateTrim.pdf

### Hotspot atlas

A hotspot atlas is a predefined catalog of useful rectangles. A DCC tool compares a rectangular UV island with that catalog and maps it to the nearest suitable region by dimensions, aspect, position, and texel-density policy. The atlas is authored first; UV assignment consumes it later.

DreamUV describes hotspotting as assigning UV islands by reference to a predefined atlas made from a mesh of varied rectangles. Hammer++ likewise loads predefined rectangle mappings and chooses the nearest matching rectangle. A Maya implementation follows the same order: prepare the hotspot reference, save its UV coordinates, then match selected rectangular UVs to the closest hotspot.

- https://github.com/leukbaars/DreamUV
- https://ficool2.github.io/HammerPlusPlus-Website/updates.html
- https://www.artstation.com/wnswift/blog/3ZEVN/hotspot-texturing-plugin-for-maya

### Generic atlas

A generic atlas packs independent images or crops while minimizing unused space. Its region topology is allowed to change whenever inputs or sizes change. This is the one mode for which the current bin-packing solver is conceptually appropriate.

### Shared visual rules

Hotspot materials commonly use clean rectangular regions, intentionally beveled boundaries, and surface-dependent scale. There is no universal arrangement for every asset: broad walls need fewer large bands, while intricate metal assets benefit from more narrow strips. The important requirement is a deliberate reusable template, not arbitrary packing. See:

- https://www.defaultinteractive.co.uk/post/hotspot-texturing
- https://silver593.artstation.com/projects/D566kE

## Why the current result is wrong

### 1. Adding Base Color does not reliably generate a sheet

The automatic-generation guard in apps/desktop/src/layout-workspace.tsx is keyed only by the list of source-set IDs. A blank source set already has an ID. The empty state consumes that signature and attempts a solve with no participating content. Adding Base Color to the same source set does not change the signature, so the effect declines to run again.

This precisely explains the observed state: Base Color is visible on the left while the output still says “Add a material to begin” and retains the earlier no-participating error.

The trigger must instead use a content revision that changes when Base Color, companion maps, patches, slot assignments, template choice, or output settings change. Empty source sets must not consume the generated revision.

### 2. The generated regions are arbitrary recipes, not a hotspot layout

apps/desktop/src/layout-authoring.ts creates four or five generic whole-source items for most presets. It then appends all enabled authored patches as additional layout items. With one material and three patches, the system therefore produces seven or more independent regions. Those regions repeat the whole brick source or rectified patch; they do not correspond to a reference mesh or a standardized slot topology.

Patches should be optional content overrides for named template slots. Creating a patch must not silently add another region to a trim or hotspot sheet.

### 3. Hotspot and trim modes use a generic bin packer

crates/geometry/src/layout.rs computes a grid from item count, derives target cell sizes, and searches candidate positions around already occupied bounds. That is generic atlas behavior. It has no concept of progressive trim widths, stable normalized boundaries, a reference rectangle catalog, or material-family compatibility.

The reported 450 x 1000 failure is therefore a predictable result of the wrong abstraction. The system has already filled the page with unrelated generated rectangles and then cannot find another non-overlapping position after padding and bleed. The software presents that internal dead end as a task for the user to solve.

Hotspot and Trim modes should instantiate a valid deterministic template. Atlas mode should adaptively scale, reflow, add a page, or offer an output resize action before it reports failure.

### 4. Empty state is presented as an error

“There is nothing enabled for this trim sheet” is a solver precondition leaking into the main workflow. With no Base Color, nothing is broken. The correct state is neutral onboarding: “Add a Base Color to create a hotspot sheet.” There should be no red panel, technical detail, or enabled Create button that can only fail.

### 5. Patch deletion is undiscoverable and then blocked

The patch list exposes Delete only through a right-click context menu. The visible control at the right of each row is a reorder grip, not a menu. Once a layout refers to a patch, project-store validation rejects disabling or deleting it to prevent a dangling reference.

That referential-integrity rule is technically safe but wrong for this authoring workflow. Delete must be visible through a row menu and the Delete key. If a slot uses that patch, deletion should be one undoable compound command: remove the patch assignment, restore the slot's base-material fallback, remove the patch, and regenerate. The layout topology should remain valid.

### 6. The UI is physically too small

The layout surface is not merely dense. apps/desktop/styles.css explicitly uses 7 px region labels, 8 px metadata and advanced controls, and 9 px labels and error text. These values are below a practical desktop reading size, especially at high-DPI scaling. They also make 25 px controls difficult to target.

The baseline should be 13 px body text, 12 px labels, 11 px secondary metadata, and at least 30–32 px interactive targets. Important status and errors must never use the smallest text tier.

### 7. The top bar is not a dependable window caption

The project menus, project identity, workspace tabs, publishing controls, a 52 px flexible drag spacer, and native window controls all compete in one 40 px row. An invisible gap is not a clear, dependable drag affordance, and at narrower widths it is squeezed by the controls around it.

Use the native Windows caption or a dedicated caption strip. If custom chrome is required, separate window dragging and window controls from application navigation. No drag region should sit over or between primary buttons.

### 8. The map scrollbar competes with the map buttons

The visible scrollbar was inserted into the same fixed-height strip as 39 px map buttons. The scrollbar and controls therefore occupy overlapping vertical space. The strip needs reserved scrollbar space, wheel/trackpad horizontal scrolling, and fade or arrow affordances; the bar must not be painted on top of the controls.

### 9. The bottom editor exposes implementation structure

The current layout editor is a horizontally scrolling spreadsheet of Layout recipe, Pack settings, Selected region, and data-region controls beneath the output. It creates nested horizontal and vertical scrolling and makes the output preview compete with internal solver controls.

The primary task is choosing a mode/template and assigning content to slots. Packing constraints and exact pixel bounds are advanced inspector details, not the permanent main navigation.

## Replacement workflow: one imported image

The default successful path should be:

1. The user imports one Base Color image.
2. Hot Trimmer immediately creates a Hotspot sheet using the default architectural template. No patches are required and there is no Create button.
3. The output preview displays named reusable slots and their dimensions.
4. The user may switch between Hotspot, Trim, and Atlas. Switching chooses a different model, not merely a different packing heuristic.
5. Importing Normal, Height, Roughness, or other registered maps fills the same template with identical UV boundaries.
6. Changing template or output resolution regenerates immediately and preserves compatible slot assignments.

For a default square architectural hotspot template, use stable normalized regions such as:

- lower half: one 1.0 x 0.5 broad surface region;
- upper half: progressive vertical widths of 1/2, 1/4, 1/8, 1/16, 1/32, and the remaining narrow cap region;
- shared boundaries: optional generated 45-degree bevel profile represented consistently in Height and Normal;
- every boundary snapped to output pixels after output resolution is known;
- padding and bleed rendered inside a template-aware gutter policy rather than changing slot topology.

This directly matches the supplied reference layout's useful property: a broad field plus progressively narrower reusable spans. It should be one curated template, not claimed as the only correct hotspot topology.

## Replacement workflow: multiple patches

Patches are source crops and perspective corrections. They should remain optional until assigned.

1. The user creates a patch. It appears in the source library only.
2. The user selects an output slot and chooses Use patch, or drags the patch onto that slot.
3. The chosen slot uses the rectified patch while every unassigned slot continues to use the base material.
4. A slot can expose per-assignment behavior such as tile, stretch, trim cap, rotation, and crop framing.
5. Reassigning or deleting the patch restores the base-material fallback without moving other slots.
6. The assignment is stable across companion maps and undo/redo.

Only Atlas mode should interpret each enabled patch as an independent packable region. In that mode, the UI must call them atlas entries and make their inclusion explicit.

## Proposed screen model

### Window chrome

- Row 1: native/dedicated caption with project name, drag surface, and window controls.
- Row 2: project actions, mode switch, undo/redo, and export actions.

### Main work area

- Left rail: material sources and patches. Each patch row has selection, enabled state where meaningful, and a visible menu containing Rename, Duplicate, Assign, and Delete.
- Center: source image and patch authoring.
- Right: authoritative output sheet preview with selectable named slots.
- Right inspector: properties for the current source patch or output slot.

### Output header

- Mode: Hotspot | Trim | Atlas.
- Template thumbnail/name.
- Output resolution.
- Status: Up to date, Generating, or Needs attention.

Advanced packing controls belong only to Atlas mode and should be collapsed by default.

## Failure and repair policy

Normal direct manipulation should settle to a valid result:

- moving or resizing a region clamps to output bounds;
- collision in Atlas mode searches for the nearest valid position or returns to the previous valid position;
- a failed drag never briefly duplicates the texture at old and new positions;
- template slots never overlap because their topology is valid by construction;
- padding and bleed are template-aware and cannot push a slot outside the sheet;
- if an atlas truly cannot fit, offer Scale to fit, Increase to 4096, or Add page as immediate actions;
- technical details remain copyable behind a disclosure, but are not the primary message.

## Architecture changes

1. Add an explicit layout kind: hotspot_template, trim_template, or atlas_pack.
2. Define versioned template assets with stable slot IDs, normalized bounds, allowed behaviors, orientation, and optional bevel-edge metadata.
3. Store patch-to-slot assignments separately from patch geometry and separately from solved pixel bounds.
4. Instantiate Hotspot and Trim pixel bounds directly from a template; do not send those slots through the generic packer.
5. Keep the generic solver only for Atlas and make it adaptive.
6. Key automatic generation to authoritative content and layout revisions, with a short debounce and stale-result rejection.
7. Make patch deletion and slot fallback one transactional undoable operation.
8. Render the preview from one committed/draft state so movement cannot show both old and new texture positions.

## Acceptance criteria for the Phase 3 reset

### Automatic one-image path

- Importing the first valid Base Color produces a visible default hotspot sheet without another click.
- No red error is shown before a material exists.
- Replacing Base Color regenerates even when the source-set ID is unchanged.
- Adding a companion map preserves identical slot IDs and boundaries.

### Layout correctness

- Hotspot and Trim use deterministic versioned templates, not item-count grids.
- The default template visually contains the broad lower field and progressive upper widths.
- Regeneration never changes slot IDs or normalized boundaries for the same template version.
- Atlas remains clearly named and is the only mode that generically packs arbitrary regions.

### Patch behavior

- Creating a patch does not add a region until the user assigns it, except in explicit Atlas mode.
- Assigning a patch changes exactly one selected slot.
- A visible row action and the Delete key can delete a patch.
- Deleting an assigned patch restores the slot fallback, regenerates, and can be undone in one step.

### Interaction

- Every drag ends in a valid clamped/collision-free state or returns to its previous state.
- The preview never paints the same region at both old and new coordinates.
- An ordinary 2048 x 2048 template cannot produce an impossible-fit error.

### Readability and window chrome

- No functional layout text is below 11 px; normal body text is at least 13 px.
- Primary interactive targets are at least 30 px high.
- Map scrolling never overlaps map controls.
- A clearly available caption surface moves the window at all supported widths.

## Phase decision

Phase 3 should not be declared complete and should not be repaired by adding more packing presets. Preserve the working Phase 2 patch geometry and rectification engine, replace the Phase 3 layout model and layout UI, and then qualify the new one-image path before reintroducing patch overrides and Atlas packing.

The implementation sequence should be:

1. correct automatic generation and empty states;
2. introduce versioned hotspot/trim templates and render the one-image path;
3. add patch-to-slot assignment and transactional deletion fallback;
4. replace the bottom spreadsheet with the mode/template/slot UI;
5. isolate and harden generic Atlas packing;
6. run readability, window-drag, scrolling, and direct-manipulation acceptance checks.
