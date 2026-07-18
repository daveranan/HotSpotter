# Hot Trimmer manual layout preset product prompt pack

## Product decision

Hot Trimmer is no longer asking the user to design a trim sheet by tuning a procedural partition generator.
The product workflow is now:

```text
add one or more source materials
-> choose an authored layout preset
-> draw, resize, split, merge, and classify regions directly
-> create source patches on any material
-> assign a whole source or exact patch to any region
-> author explicit planar, continuous, looped, or radial behavior per region
-> compile truthful Base Color through compile_persisted
-> save the authored layout as a reusable preset
-> add material processing in the later material prompt pack
```

The layout is an authored asset, not a generator recipe. Automatic source reuse, duplicate crops, crop search,
synthesis, and implicit repetition remain forbidden. Repetition is legal only when the user explicitly marks a region
as looped and authors its source crop.

This pack supersedes the unfinished generator work in Prompt 4 and Prompts 5-6 of
`docs/source-frame-partition-prompt-pack.md`. Prompt 1's pixel-exact DirectCrop lineage and Prompt 3's preview profiles,
caches, cancellation, and artifact publication remain locked regression gates. Preserve the useful direct atlas editor
from Prompt 4, but remove the procedural recipe/candidate workflow from normal product authority.

Do not run the old 9B, 14PB, source-frame radial, source-frame generator, material, effect, export, or asset-browser
prompts in parallel with this pack. Reconcile those packs after Prompt 4 below is accepted. Later material prompts must
consume the authored region semantics established here; they must not recreate layout or source placement.

## Non-negotiable architecture

- `TrimSheetDocument` remains the sole project authoring authority.
- `compile_persisted` remains the sole live compile/orchestration spine.
- Stage 14 remains the sole authoritative Base Color sampling/composition route. Add no renderer, TypeScript pixel
  compositor, legacy fallback, preview compiler, or second orchestration path.
- A preset is a versioned authored topology snapshot plus region semantics. Loading a preset instantiates a document;
  it does not leave a live generator attached to the project.
- Stable `RegionId`, `RegionBinding`, `SourceSetId`, `PatchId`, `GridRect`, source crop, and atlas destination identities
  survive UI, commands, compiler, IPC, preview, undo/redo, and save/reopen.
- Layout geometry and region content are separate. Resizing a destination region never silently changes its source or
  chooses a new crop. Assigning a patch never changes topology.
- The SourceFrame belongs to its explicitly pinned primary source. Adding or selecting another source must not replace
  that ownership or invalidate the sheet.
- Direct layout gestures update local command-backed topology immediately. They do not clear pixels or start a full
  material compile on every pointer move.
- Every visible control must either produce a visible/debuggable artifact change in its prompt or remain hidden until
  the prompt that wires it. Do not ship inert controls.
- Preserve unrelated worktree changes. Read `AGENTS.md`, this file, the current diff, and directly relevant contracts
  before editing. Use no subagents unless explicitly requested.
- Run one focused verification command, make at most one correction pass, rerun the same command, and stop.

## Authoritative sequence

| Prompt | Visible product result | Review gate |
| --- | --- | --- |
| 1 | Authored presets replace generator recipes; direct region editing is clear and stable | Create, edit, save, reload, and reapply a layout without generating it |
| 2 | Multiple sources and patches can be managed and assigned to any region | Two sources and many patches coexist; assignment changes only the selected region |
| 3 | Region type, continuity, explicit looping, and radial mapping are truthful | Each selected mode visibly changes only its region and publishes future edge eligibility |
| 4 | Base Color workflow is shippable on large sources | Padded, persistent, responsive 8K/16K/24K workflow with no stale/black output |

Run one prompt per Codex task and visually review the native application after each prompt. Do not combine them into
one implementation task.

---

## Prompt 1 - Make authored layout presets the product authority

```text
Implement Manual Layout Product Prompt 1: replace the procedural partition/candidate product workflow with versioned
authored layout presets and finish the direct region editor.

Read AGENTS.md; docs/manual-layout-preset-product-prompt-pack.md; the current diff; TrimSheetDocument,
AcceptedTopology, RegionDefinition, RegionBinding, logical-grid commands, and preset/generator code; and the current
layout UI in apps/desktop/src/source-first-app.tsx. Preserve the locked SourceFrame DirectCrop and preview fast paths.
Do not use subagents.

Product outcome:
- Opening a Base Color project immediately instantiates the built-in Diagonal Cascade authored preset.
- The user can choose a built-in or saved preset, create a new preset, duplicate, rename, save, and delete user presets.
- The user edits the loaded topology directly. No complexity/share/quota/seed/candidate controls are needed.
- A saved preset can be applied to a new source and produces the same grid rectangles and stable preset-local identities.

Preset contract:
- Add/reuse one versioned AuthoredLayoutPreset contract containing:
  preset ID, schema version, name, logical grid, canonical aspect, ordered region records, GridRect, stable preset-local
  region key, role/orientation/UV-fit/structural defaults, and provenance.
- Region content bindings and project SourceSetId/PatchId references are not baked into a reusable layout preset.
  Applying a preset creates project RegionIds deterministically from preset ID + preset-local key + instance ID and
  initially binds them to InheritPrimaryMaterial.
- Store a project snapshot of the applied preset so save/reopen does not depend on an external preset changing.
- Built-in presets are immutable. User presets are versioned user assets with Duplicate/Save As, Rename, Delete, and
  Revert. Do not build the broad Asset Browser from the deferred 9B prompt in this task.
- Convert the current Diagonal Cascade result into a checked-in authored topology fixture. Loading it must not call the
  procedural partition generator.
- "New blank" creates one full-sheet remainder region so exact coverage remains valid. The user can subdivide or draw
  ownership rectangles over it; do not create an invalid uncovered atlas.

Retire generator authority:
- Remove from the normal layout inspector: Composition preset recipe controls, Complexity, Large panel share, Strip
  share, Radial slots, Orientation variation, Advanced hierarchy, recipe diagnostics, Update now, candidate Accept,
  and candidate Discard.
- Remove automatic recipe preview/debounce from the live product path.
- Keep generator code only if it is still needed to migrate an existing document or regenerate a checked-in fixture.
  It must not run on project load, source import, direct edit, preset selection, or preview. Delete code only after
  proving it is unreferenced by active runtime and migration.
- Existing documents with accepted generated topology migrate by snapshotting their current exact topology as an
  embedded authored preset. Never silently regenerate them.

Layout inspector:
- Keep Select/resize, Draw region, Texture, Region colors, Grid, grid opacity, Undo/Redo, preset selection/CRUD, and
  selected-region summary.
- Replace the free numeric grid recipe with one discrete grid-resolution control using:
  16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256.
- Changing grid resolution must use an explicit command. Preserve geometry exactly when all boundaries are representable;
  otherwise preview the quantized result and require confirmation. Never truncate or reinterpret rectangles silently.
- Move selected-region editing into a clear Inspector section. Do not show source analysis, Stage 14 lineage dumps,
  weathering placeholders, or recipe internals in the normal layout panel.

Finish direct editing:
- Preserve the current exact-cover draw/resize/split/merge behavior, stable IDs where ownership is unchanged, atomic
  commands, one undo entry per gesture, and no Stage 14 recompile during pointer movement.
- Add a visible snapped cursor/crosshair in Draw mode before pointer-down. Show the exact grid intersection and cell.
- Pointer-down must start at the displayed snap point in every zoom/pan/DPI state. Eliminate the observed next-cell
  offset at the bottom/right edges.
- During draw and resize, show an unmistakable live rectangle, coordinates, width/height, affected ownership, and valid
  versus invalid state. Escape cancels; release commits exactly once.
- Right-click a region opens the application menu, not the browser menu. Include Split horizontal, Split vertical,
  Merge/remove divider where legal, and Delete/return area to neighbor or remainder with a typed explanation when no
  legal ownership transfer exists.
- Keep the source workbench visible/toggleable; this prompt must not delete or permanently hide it.

Automated proof:
- Applying the built-in Diagonal Cascade twice yields the same preset-local keys and GridRects without invoking the
  generator.
- New blank is exact-cover and remains valid after draw, eight-handle resize, split, merge, delete, undo, and redo.
- Draw hover point equals committed start point under non-integer zoom, pan, high DPI, and bottom/right boundaries.
- Save/reopen preserves topology, grid, preset snapshot, RegionIds, and selection-safe identity.
- Changing an algorithm recipe value is impossible from the normal product UI and no automatic generator call occurs.

Focused verification:
npm.cmd run test --workspace @hot-trimmer/desktop -- manual-layout-presets

Native acceptance:
Create a project, observe Diagonal Cascade immediately, create New Blank, draw and resize several regions at multiple
zoom levels, split/merge/delete, undo/redo, save as a user preset, reopen the project, and apply the preset to another
source. Capture the cursor before drag and the exact resulting rectangle after release.

Stop after authored presets and direct layout editing are green. Do not implement source assignment, loop/radial
sampling, padding, material maps, effects, export, or the full Asset Browser.
```

---

## Prompt 2 - Restore the multi-source and patch content workflow

```text
Implement Manual Layout Product Prompt 2: make Sources and Patches a real multi-source content library and let the
user assign any whole source or authored patch to any layout region without invalidating topology.

Prerequisite: Prompt 1 is visually accepted. Read AGENTS.md; docs/manual-layout-preset-product-prompt-pack.md; current
diff; SourceFrame ownership; MaterialSourceSet/SourceSetId/PatchId/RegionBinding/ContentReference; source import/replace
commands; compile_persisted source resolution; and the existing patch authoring gestures. Do not use subagents.

Correct the current multi-source contract bug:
- A SourceFrame is owned by its persisted sourceFrame.sourceSetId. compile_persisted must validate its dimensions and
  digest against that source, not against whichever source is currently selected or happens to be first/primary.
- Adding Source B, selecting Source B, or creating a patch on Source B must not mutate Source A's SourceFrame, change
  primary_material, clear the compiled sheet, or produce "source-frame contract is invalid."
- Primary source is explicit and pinned. Add Set as primary/Rebase layout as an explicit command with a previewed
  migration; never change it as a side effect of selection or import.
- RegionBinding already permits inherited primary, MaterialSource(SourceSetId), and Patch(PatchId). Use that contract
  through compile_persisted; add no parallel fill model.

Sources library UX:
- Make Sources a compact, scrollable list using the available vertical panel height. Each row shows thumbnail, name,
  map count, dimensions, revision/status, and selection.
- Remove Exemplar group, De-lighting, and Strength controls from every source card. Put those existing settings in a
  contextual Source Inspector/Advanced Material Preparation section shown only for the selected source.
- Implement an application-owned right-click menu and prevent the browser context menu. Include Rename, Replace Base
  Color/File, Add or replace channel maps, Set as primary, Reveal source, and Remove.
- Replace preserves SourceSetId and dependent PatchIds when legal, increments the correct content revision/digest,
  invalidates only dependent caches, revalidates oriented geometry, and reports affected bindings before a destructive
  incompatibility. Remove is dependency-aware and never silently strands a region.
- Adding an independent source creates a new SourceSetId. Adding maps to a source registers channels in that existing
  source set. Make the distinction explicit in the UI.

Resizable workspace:
- Sources/Patches, source canvas, sheet canvas, and Inspector use visible draggable splitters with persisted sizes and
  sensible minimums. The source workbench can be hidden and restored without losing its size or selection.
- Fit operates per canvas. Importing or selecting a different-sized source must not force the sheet canvas to refit.

Patches library and source editing:
- Populate the Patches section as an actual list grouped/filterable by owning source. Show thumbnail, name, source,
  shape, dimensions, enabled/assigned status, and selection.
- Preserve the existing Rectangle and Four Point authoring interaction. Every patch has stable PatchId, owning
  SourceSetId, authored geometry in oriented source coordinates, and registered cross-channel correspondence.
- Patch creation/editing happens on the selected source canvas. Selecting a patch restores its editable handles.
- Support many sources and patches; do not assume one source or one patch.

Region content assignment:
- Selecting a region exposes a Content section in the Inspector: Inherit primary, Whole source, Patch, or Solid.
- "Assign patch to region" is enabled when both a region and patch are selected. Drag/drop and the region right-click
  menu may call the same command; they must not create separate authorities.
- Assignment changes only that RegionBinding.content and appearance identity. It preserves RegionId, GridRect,
  destination, neighboring pixels, and topology revision.
- The source canvas shows and edits the exact assigned crop/patch for the selected region. The atlas shows a bounded
  transient preview during editing and then the compile_persisted result. No full-source fallback is allowed.
- Cross-channel maps use the same source/patch transform. Missing optional maps use typed channel fallbacks; missing
  Base Color is an actionable binding error.

Automated proof:
- Add Source A and Source B, keep A as SourceFrame/primary, select B, and compile without SourceFrame invalidation.
- Create at least 20 patches across both sources and save/reopen with stable IDs and ownership.
- Assign an A patch to region 1, a B patch to region 2, whole B to region 3, and inherited A elsewhere. Assert exact
  source lineage and pixels per region through compile_persisted.
- Replace B Base Color while preserving its SourceSetId; only B-dependent caches/bindings change.
- Removing a referenced source or patch is rejected or completed through an explicit atomic fallback chosen by the user.
- Right-click source/patch/region opens the app menu and never the browser menu.

Focused verification:
npm.cmd run test --workspace @hot-trimmer/desktop -- multi-source-patch-assignment

Native acceptance:
Load two differently sized Base Colors, create patches on each, freely resize both workbench panes, assign patches and
whole sources to multiple regions, edit one assigned patch on its source, undo/redo, replace Source B, save/reopen, and
confirm the sheet never disappears and unrelated regions never change.

Stop after truthful multi-source Base Color assignment is green. Do not add loop/radial sampling, padding, material
processing, effects, export, or a broad asset browser.
```

---

## Prompt 3 - Author truthful per-region behavior

```text
Implement Manual Layout Product Prompt 3: add one authoritative per-region behavior inspector for planar, continuous,
explicitly looped, and radial regions, and compile the selected behavior through compile_persisted and Stage 14.

Prerequisite: Prompts 1-2 are visually accepted. Read AGENTS.md; docs/manual-layout-preset-product-prompt-pack.md;
current diff; RegionDefinition role/orientation/UV fit/structural profile; RegionBinding mapping/projection/address mode;
RadialMappingSettings; existing Stage 14 sampling branches; and later structural-mask contracts. Do not use subagents.

Important policy:
- There is still no automatic repeat, source reuse, duplicate crop, tiling, synthesis, or crop search.
- A region repeats only after the user explicitly chooses Loop X, Loop Y, or Loop XY and authors the crop/period.
- "Continuous" describes which destination edges are structural seams. "Loop" describes source address/sampling. Store
  both explicitly; do not infer one by accident from aspect ratio, orientation, or role.
- Do not expose a mode until Stage 14 executes it exactly. Unsupported combinations are disabled with a typed reason.

Versioned region behavior contract:
- Add/reuse a typed per-region contract with:
  role = panel | horizontal_strip | vertical_strip | unique | radial;
  continuity = none | x | y | xy;
  sampling = one_shot | loop_x | loop_y | loop_xy;
  orientation/quarter-turn policy;
  edge eligibility for left/right/top/bottom;
  optional radial parameters and deterministic behavior version.
- Continuity X disables structural left/right edges; Continuity Y disables top/bottom; XY disables all four perimeter
  seams. This publishes edge eligibility for later height/normal/weathering work but does not fake those maps now.
- Presets store default behavior by preset-local region key. Project edits override the instantiated region only.
- Include behavior in appearance/topology hashes at the correct boundary and preserve it through commands, IPC,
  diagnostics, undo/redo, save/reopen, and preset Save As.

Inspector and context menu:
- When one region is selected, show Role, Continuity, Sampling, Orientation, source assignment summary, exact crop,
  and eligible structural edges. Put these in the right-side Inspector, not the global layout settings.
- Provide the same common choices in the region right-click menu.
- Add a debug "Edge eligibility" overlay so continuity changes are visible before material profiles exist.
- One-shot DirectCrop remains the default and pixel-exact.

Explicit looping:
- Loop X/Y/XY uses the assigned whole-source crop or patch and its exact authored transform. It never borrows pixels
  from another region or silently expands to the full source.
- Preserve aspect and texel scale according to the existing mapping contract. Publish period, transform, address mode,
  crop, sampling-plan ID, and executed mode in the compiled artifact.
- Cross-channel maps share the exact looping transform.
- Padding is outside the semantic sample area and is handled in Prompt 4.

Radial authoring:
- A Radial region owns one rectangular atlas destination and one explicitly assigned source/patch crop.
- Expose center, inner radius, outer radius, seam angle/rotation, falloff/warp controls supported by the existing typed
  radial mapping. Add a direct on-source gizmo with center, inner/outer rings, and seam handle.
- The gizmo uses the same draft/commit/stale-result rules as patch editing. Clamp at the last valid value; do not reset
  the gesture or move another region.
- Stage 14 must perform the selected radial mapping inside only that destination. No copying into multiple regions, no
  centered fake synthesis, and no writes outside the region/hotspot.
- If fisheye/annulus behavior is not implemented by the existing renderer, do not invent it in React. Either wire the
  existing typed warp through Stage 14 or keep that specific control disabled with an explicit prerequisite.

Automated proof:
- For one numbered source/patch, one-shot, Loop X, Loop Y, and Loop XY produce known different pixels and publish the
  requested mode as the executed mode.
- No region repeats under defaults. Shuffling region iteration cannot copy a plan or crop across RegionIds.
- Continuity X/Y/XY publishes the exact expected eligible edge set and the debug overlay matches it.
- Radial center/radius/seam gestures alter only the selected RegionBinding and its Stage 14 pixels.
- Save/reopen and preset Save As preserve every behavior field and stable identity.
- Unsupported role/sampling/radial combinations cannot reach Stage 14.

Focused verification:
cargo test -p hot-trimmer-sheet-compiler manual_region_behavior

Native acceptance:
Classify several manually drawn regions as panel, horizontal strip, vertical strip, and radial. Toggle continuity and
inspect the edge overlay. Explicitly loop one strip in each axis and verify defaults do not repeat. Adjust a radial gizmo
on the source and confirm only its region changes. Undo/redo and save/reopen everything.

Stop after truthful Base Color mapping and edge-eligibility metadata are green. Do not generate height, normals,
roughness, AO, weathering, or export assets.
```

---

## Prompt 4 - Close the Base Color product slice

```text
Implement Manual Layout Product Prompt 4: qualify the authored-preset, multi-source, per-region Base Color workflow as
a responsive product slice, including owning-edge padding and large-source persistence.

Prerequisite: Prompts 1-3 are visually accepted. Read AGENTS.md; docs/manual-layout-preset-product-prompt-pack.md;
Prompt 1 pixel-exact SourceFrame tests; Prompt 3 preview/cache telemetry; Stage 14 atlas composition/encoding; padding
helpers; save/recovery; and current native performance traces. Do not use subagents.

Owning-edge padding:
- Add a user-visible atlas padding control in output pixels plus a preview-scaled equivalent.
- Reserve padding in destination topology/composition without changing semantic region bounds or source crops.
- Fill padding by deterministic nearest owning-edge dilation after each region is rendered. Dilation is not repetition,
  tiling, source reuse, or synthesis.
- Never bleed across a neighboring RegionId. At corners use deterministic nearest-owner/tie behavior.
- For continuity/looped edges, respect the explicit seam policy established in Prompt 3; do not manufacture a
  structural edge where continuity disabled one.
- Publish semantic rect, padded rect, atlas destination, and RegionId together. Region ID output must identify padding
  ownership consistently for later mip generation.

Large-source and preview requirements:
- Preserve original oriented coordinates for arbitrary dimensions and aspect ratios. Test at least 7952x4016,
  8000x8000, a 16K-class source, and a synthetic 24K-class source without treating source size as atlas size.
- Keep 512 draft and 1024 refinement profiles. Authoritative output may be 2K/4K/8K independently of source size.
- Importing a large source must show progress and cancellation; it must not block the UI or allocate a full final atlas
  per region.
- Keep bounded decode/domain/render/composition caches keyed by exact source, patch, topology, behavior, padding, and
  profile identities. Direct layout edits must not flush source decodes. A patch edit invalidates only dependent regions.
- Coalesce drag requests, cancel stale drafts through encode/publication, publish draft before refinement, and never
  replace a newer artifact with an older revision.
- Do not encode every channel or perform source analysis that Base Color preview did not request.

Persistence and recovery:
- Save/reopen preserves the embedded applied preset, user preset reference/version, topology, grid, region behavior,
  multi-source registrations, patches, bindings, SourceFrame owner, output/padding settings, and stable IDs.
- Missing or replaced sources produce per-binding diagnostics while all unaffected regions and the layout remain
  visible. Never clear the whole sheet because one optional source is unavailable.
- Migrate existing accepted generator documents by snapshotting their exact current topology. Do not regenerate.

Performance targets measured in the native app:
- Cold 512 preview on representative 8K Base Color: <= 3 seconds.
- Warm unchanged preview: <= 500 ms.
- One-region patch/crop/behavior edit: <= 500 ms to updated 512 draft.
- Map/preset UI-only switch with cached artifact: <= 50 ms paint target.
- 1024 refinement: <= 5 seconds.
- Report decode count, rendered-region hit/miss, composed-artifact hit/miss, allocated bytes if available, encode time,
  IPC payload, upload/paint, cancellation, source/output sizes, region count, and patch count.
- If hardware misses a target, report the measured bottleneck and keep the prompt open; do not weaken the fixture.

End-to-end automated fixture:
- Two large sources with different aspect ratios, at least 64 manually authored regions, at least 20 patches, a mix of
  inherited/whole-source/patch bindings, one-shot and explicitly looped strips, one radial region, and nonzero padding.
- Assert complete non-overlapping semantic coverage, deterministic padding ownership, stable IDs, no default duplicate
  crops, correct per-region pixels, no black/transparent holes, no writes outside padded ownership, and exact save/reopen.
- Exercise rapid A->B->C edits and cancellation; only C may publish.

Focused verification:
cargo test -p hot-trimmer-sheet-compiler manual_base_color_product

Native acceptance:
Run the complete fixture and a real 8K source. Create/load/save presets, add two sources, create/assign patches, edit
layout and behavior, adjust padding, cancel a compile, save/reopen, and capture 512 plus 1024 output and telemetry.
The result must remain a fully filled, truthful trim sheet with no stale/black disappearance.

Stop when the Base Color product slice is visually and measurably accepted. Do not implement height, normals,
roughness, AO, weathering, final export, Blender integration, or the broad Asset Browser. After acceptance, rewrite the
later material prompt sequence so it consumes these authoritative region behaviors and bindings through the same
compile_persisted artifact.
```

## What happens after this pack

Do not return to the old horizontal stage sequence. The next material milestone should take this accepted artifact:

```text
authored preset topology
+ stable per-region source binding and mapping
+ edge eligibility and radial/continuity semantics
+ truthful padded Base Color
-> structural profile height
-> material + structural height
-> normal
-> roughness and AO
-> Region ID
```

The later material sequence consumes one authoritative `compile_persisted` artifact. No material prompt may recreate
topology, resolve a second binding, choose a new crop, infer continuity, or bypass the published semantic/padded owner
rectangles:

1. **Structural profile height** — prove one visible profile on one manually classified RegionId, using its published
   edge eligibility and padded ownership. Base Color sampling and layout remain byte-identical.
2. **Material + structural height** — combine registered height from the same source/patch transform with the structural
   profile inside the same owner rectangle, preserving continuity and radial semantics.
3. **Normal** — derive or combine normals from that accepted height and registered mapping; padding consumes the same
   owning-edge dilation and never crosses RegionId.
4. **Roughness and AO** — sample only the already-resolved binding/transform, apply typed fallbacks per region, and keep
   Base Color plus prior maps unchanged.
5. **Region ID** — encode the `compile_persisted` per-pixel ownership already published for semantic and padding texels;
   categorical IDs use nearest ownership through mip generation.

Each step extends the same artifact and cache identities. The first step remains the gate: one structural profile must
be visibly and measurably correct before expanding across all regions or adding later effects.
