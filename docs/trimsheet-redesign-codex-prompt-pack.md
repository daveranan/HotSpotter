# Hot Trimmer trim-sheet redesign: Codex prompt pack

## How to use this file

Run one prompt per Codex task, in order. Do not combine prompts. Do not ask a task to “continue as
far as possible”; each task has a concrete boundary and one focused verification command.

Before running a prompt:

1. Preserve or commit the current worktree intentionally. The repository may already contain user
   changes; no prompt authorizes resetting or reverting them.
2. Start with Prompt 1A. Prompts 1A through 1D are one logical **truth-spine milestone** and must be
   completed in order.
3. Do not start Prompt 2 until Prompt 1D is green in the running desktop product.
4. After every prompt, read the task report and inspect the visible result before starting the next.
5. If a task is incomplete, use the continuation prompt at the end of this file. Do not jump ahead.

The governing plan is:

```text
docs/trimsheet-document-generation-redesign-plan.md
```

The broader product contract is:

```text
docs/hot-trimmer-template-blender-companion-plan.md
```

## Rules included in every prompt

Each prompt below deliberately repeats the critical constraints:

- Read `AGENTS.md` and the relevant plan sections before editing.
- Inspect existing code and current uncommitted changes; preserve them.
- Work locally in the root task. Do not add agents unless explicitly requested.
- Keep one authoritative Rust domain model. Do not create a parallel TypeScript product model.
- Implement the complete assigned slice: domain, persistence/IPC/UI/compiler only where the slice
  explicitly requires them.
- Do not implement later prompts early.
- Use `apply_patch` for edits.
- Run exactly the specified focused verification command. Make at most one correction pass and run
  the same command again.
- Stop when the acceptance criteria are met and report files, contracts, migration, test evidence,
  visible behavior, and known limitations.

---

## Prompt 1A — Canonical trim-sheet domain

Use this prompt first.

```text
Implement Prompt 1A: the canonical trim-sheet domain foundation for Hot Trimmer.

Read AGENTS.md, then read these sections completely before editing:
- docs/trimsheet-document-generation-redesign-plan.md sections 2, 3, 4, 10, 11, and 12
- docs/hot-trimmer-template-blender-companion-plan.md sections 5 through 11

Inspect the current domain/layout/template types and git status. Preserve every existing user change.
Work in the root task without subagents.

Scope:
- Evolve the existing Rust domain instead of creating a competing layout model.
- Introduce the authoritative TrimSheetDocument contracts needed by the plan: document/topology/
  appearance revisions, accepted topology snapshot, authoritative region definition, exactly one
  region binding, typed content reference, typed projection/mapping transform/addressing/sampling,
  ordered versioned warp operations, generator provenance, and render settings.
- Reuse current IDs, integer coordinate types, template identities, source sets, patches, profiles,
  and validation wherever they already express the required contract.
- Define topology-versus-appearance change classification and deterministic topology/appearance
  hashing inputs. Do not put source content, crop, warp, treatments, or map appearance into the
  topology hash.
- Add validation for duplicate region IDs/colors, invalid rectangles, missing bindings/content,
  invalid normalized source geometry, unbounded warp parameters, and non-finite values.
- Provide a narrow conversion constructor from the current stored template layout into the new
  document contract for the next persistence slice. Do not remove legacy storage yet.
- Update IPC contracts only if compilation requires shared serialized types; Rust remains authoritative.

Do not:
- modify the desktop interaction yet;
- change the compiler entry point yet;
- add schema migration yet;
- add generator algorithms yet;
- hide executable warp recipes in unversioned generic JSON.

Acceptance:
- A one-region document can bind primary material or a patch through the same RegionBinding type.
- Source mapping and topology geometry are separate types.
- Appearance-only mapping changes leave topology hash inputs unchanged.
- Invalid or duplicate identities/geometry/warp values fail validation with typed errors.
- Serialization is deterministic and round-trips.

Focused verification command:
cargo test -p hot-trimmer-domain trim_sheet_document

If the named tests do not exist, add focused tests using that name prefix so the command executes them.
Make at most one correction pass, rerun the same command, and stop. Report the exact new authoritative
types and any temporary legacy bridge that Prompt 1B must remove later.
```

---

## Prompt 1B — Persistence, migration, and authoritative commands

```text
Implement Prompt 1B: persist and command the canonical TrimSheetDocument.

Read AGENTS.md and docs/trimsheet-document-generation-redesign-plan.md sections 3, 8, 11, 12,
and Milestone 1. Inspect the completed Prompt 1A types and current project-store schema. Preserve
all existing worktree changes. Work without subagents.

Scope:
- Add the next schema migration after the repository's actual current schema.
- Persist one authoritative trim-sheet document: accepted topology snapshot/regions, region bindings,
  source mapping recipes and ordered typed warps, generator provenance, revisions/hashes, and render
  settings.
- Migrate current template layouts, fills, source framing, and per-region source layers without losing
  source maps, patch IDs, patch geometry, region IDs, template identity, or bounds.
- Resolve duplicated legacy item/region fill and bounds through one explicit documented migration rule.
  Do not keep writing both representations after migration.
- Convert legacy generic layouts to CustomAtlas.
- Add Rust commands for the minimum truth spine: SetSheetFraming, SetRegionContent,
  SetRegionSourceGeometry, SetRegionMappingTransform, SetOutputResolution, and undo/redo.
- Each command validates, journals one before/after document state, increments the correct revisions,
  updates hashes deterministically, refreshes recovery, and is atomic on persistence failure.
- Extend project snapshots/IPC just enough for the desktop and compiler cutovers in Prompts 1C/1D.

Do not:
- change React interaction handlers;
- render through both old and new stored layout models;
- implement generator candidates or warp evaluation;
- silently instantiate or regenerate topology during migration.

Acceptance:
- Create/save/reopen reproduces an identical canonical document and hashes.
- Migrated template and legacy Atlas fixtures preserve IDs, bounds, fills, maps, and patches.
- Mapping commands change appearance revision only.
- Output resolution changes do not change normalized topology/hotspot identity.
- A failed migration or command preserves the previous valid project.
- Undo/redo restores the complete coherent document.

Focused verification command:
cargo test -p hot-trimmer-project-store trim_sheet_document

Add focused tests with that prefix if needed. Make at most one correction pass, rerun the same command,
and stop. Report the schema version, migration rule for conflicting legacy values, and commands added.
```

---

## Prompt 1C — Truth compiler and CompiledSheet artifact

```text
Implement Prompt 1C: compile the canonical TrimSheetDocument into one authoritative CompiledSheet.

Read AGENTS.md and docs/trimsheet-document-generation-redesign-plan.md sections 3, 7, 10 and
Milestone 1. Inspect Prompt 1A/1B contracts, the existing sheet compiler, render core, template
compiler, patch rectifier, and source-map storage. Preserve current worktree changes. No subagents.

Scope:
- Replace the template compiler's ad hoc single-Base-Color request with a document compile entry point.
- Resolve accepted authoritative region allocation/hotspot bounds, content reference, registered maps,
  patch rectification, source mapping, variation, structural profile, and exact ID metadata into a
  validated compile plan.
- Compile Base Color plus every authored registered map available to the selected material/patch.
- Apply the same source geometry/mapping transform to every channel.
- Support primary-material and patch bindings end to end. A patch binding must visibly replace exactly
  its target region.
- For this slice, evaluate Planar and Perspective projection plus translate/scale/rotate/mirror and
  clamp/repeat/mirrored-repeat. Typed nonlinear warps may remain validated but unevaluated until Prompt 5.
- Use current authoritative region bounds for both rasterization and returned overlay metadata.
- Return CompiledSheet with document revision, topology hash, appearance hash, renderer version,
  maps, compiled region metadata, and diagnostics.
- Keep the last legacy entry point only as a thin adapter for tests or migration; it must resolve into
  the canonical document path and not implement separate appearance behavior.
- Add deterministic fixtures with a numbered/checker source and a contrasting patch.

Do not:
- change desktop UI yet;
- synthesize final output by CSS;
- ignore companion maps;
- copy region fills after compilation;
- implement radial/twirl/lens evaluation yet;
- average Normal RGB values.

Acceptance:
- Assigning a contrasting patch changes pixels only inside the selected region.
- Changing authoritative region bounds changes both raster pixels and returned overlay bounds.
- Base Color and companion map geometry are registered.
- Repeating a compile produces identical hashes and bytes.
- Region/Material IDs are exact integer maps with no bleed.
- Invalid binding/mapping produces a region-specific diagnostic and no partial artifact.

Focused verification command:
cargo test -p hot-trimmer-sheet-compiler trim_sheet_document

Add focused tests with that prefix if necessary. Make at most one correction pass, rerun the same
command, and stop. Report any legacy compiler adapter that Prompt 1D must delete from live usage.
```

---

## Prompt 1D — Desktop cutover to the truth spine

Do not run this until Prompts 1A–1C are green.

```text
Implement Prompt 1D: cut the live desktop trim-sheet preview over to TrimSheetDocument and CompiledSheet.

Read AGENTS.md and docs/trimsheet-document-generation-redesign-plan.md sections 2, 8, 9, 10 and
Milestone 1. Inspect the canonical project snapshot/commands and compiler delivered by Prompts 1A–1C.
Preserve existing worktree changes. Work without subagents.

Scope:
- Change Tauri commands and IPC so the live trim-sheet workspace requests compilation of the current
  canonical document revision and receives CompiledSheet metadata/maps.
- Make the 2D sheet preview image and region overlays consume the same returned compiled artifact and
  compiled region bounds. Remove live use of independently reconstructed template bounds.
- Make template and Atlas preview state use the same compiled-artifact contract. Atlas may be disabled
  with an explicit prerequisite until Prompt 8, but it may not pretend CSS backgrounds are final output.
- Make region selection use stable region ID only; resolve the current region from the latest document
  snapshot instead of storing a stale region object in React selection state.
- Wire SetRegionContent and the existing source-framing/content controls to canonical Rust commands.
- Replace content-signature heuristics with document revision/hash scheduling and stale-result rejection.
- Display explicit build status: compiling revision, up to date revision, needs rebuild, or region error.
- Remove or disable the old live generation path once the cutover works.

Do not:
- redesign gestures yet beyond what is necessary for the cutover;
- add layout generators, nonlinear warps, treatments, or export;
- keep two live preview authorities as a fallback.

Acceptance:
- Importing Base Color produces a compiled sheet through the canonical document path.
- Assigning a patch through the inspector visibly changes exactly the selected region.
- Preview pixels and overlays have identical bounds/revision.
- A stale compile cannot replace a newer document revision.
- Save/reopen displays the same canonical result.
- No enabled UI control writes only local metadata that the compiler ignores.

Focused verification command:
npm run typecheck --workspace @hot-trimmer/desktop

Make at most one correction pass, rerun the same command, and stop. In the report, include one concise
manual run checklist for the user: import, select region, assign patch, save/reopen.
```

---

## Prompt 2 — Stable gestures, cover crop, and no duplication

```text
Implement Prompt 2: make patch, sheet-framing, and region-content manipulation transactional and stable.

Read AGENTS.md and docs/trimsheet-document-generation-redesign-plan.md sections 4, 8, 9 and
Milestone 2. Inspect the cut-over desktop state from Prompt 1D. Preserve all user changes. No subagents.

Scope:
- Add stable selection references by kind and ID; never store a stale editable region snapshot.
- Implement BeginDraftEdit/CommitDraftEdit behavior for patch geometry, global sheet framing, and
  region content mapping. Pointer motion updates a bounded local/draft preview; pointer-up sends one
  serialized Rust command and creates one undo entry.
- Keep displaying the final draft until acknowledgement at or beyond its commit revision. Ignore older
  project snapshots and preview jobs for that target.
- Suspend automatic recompilation for the active target during the gesture; compile the affected region
  preview by draft edit ID and reject stale results.
- Make creation possible only through explicit Four Point, Rectangle, or Outline Fit tools.
- In Select mode, double-click empty canvas does nothing. Double-click patch or region footprint edits
  the existing identity and cannot allocate another patch/region/source layer.
- Use one shared gizmo framework with target capabilities for translate, scale, rotate, perspective points,
  and keyboard/numeric editing. Unsupported controls are hidden or explicitly locked.
- Replace hidden cover/focus behavior with the actual aspect-locked crop rectangle on the source canvas.
  Square destination means square crop. Repeat mode shows wrapped footprints honestly.
- Surface command failures on the affected target and restore once; never fall through to another tool.

Acceptance:
- Rapid move through A/B/C never visually returns to A or B after C.
- Final persisted/reopened values equal C.
- Transform handles consistently capture pointer and execute their advertised command.
- Double-click point edit preserves patch/region counts and IDs.
- Cover crop geometry shown on the left exactly matches compiled sampling on the right.
- One gesture is one undo/redo step.

Focused verification command:
npm run test --workspace @hot-trimmer/desktop

Add focused interaction/state-machine tests to the existing desktop test command. Make at most one
correction pass, rerun the same command, and stop. Provide a manual pointer-interaction checklist.
```

---

## Prompt 3 — Multi-source and patch binding completion

```text
Implement Prompt 3: complete multi-source, registered-map, and patch-to-region binding behavior.

Read AGENTS.md and docs/trimsheet-document-generation-redesign-plan.md sections 3.2, 7 and
Milestone 3. Inspect the canonical compiler and transactional desktop. Preserve current changes.
No subagents.

Scope:
- Resolve primary material, secondary material source set, patch, solid, and fallback content through
  RegionBinding.
- Rectify a patch once per revision and sample all registered channels through its geometry/mapping.
- Make the region inspector's Content choice canonical and immediately previewed.
- Implement atomic patch deletion/disable fallback to primary material with one undo entry.
- Preserve region ID, hotspot bounds, mapping recipe, profile, and Blender compatibility when content changes.
- Add per-channel missing-map fallback and Estimated metadata without inventing Metallic.

Acceptance:
- Concrete primary + metal secondary + contrasting vent patch can coexist in distinct regions.
- Assigning content changes exactly the selected region in all available maps.
- Removing content restores the defined fallback atomically.
- Save/reopen and undo/redo preserve binding and map alignment.

Focused verification command:
cargo test -p hot-trimmer-sheet-compiler region_binding

Add focused tests with that prefix if needed. Make at most one correction pass, rerun, and stop.
```

---

## Prompt 4 — Procedural layout candidate generator

```text
Implement Prompt 4: deterministic procedural topology generation with candidate preview and acceptance.

Read AGENTS.md and docs/trimsheet-document-generation-redesign-plan.md sections 5, 10, 11 and
Milestone 4. Also read the procedural layout generation section in the companion plan. Preserve current
changes. Work without subagents.

Scope:
- Implement versioned LayoutGeneratorRecipe, TopologyCandidate, deterministic seeded generation,
  validation, candidate compile preview, Accept/Discard commands, and pinned generator provenance.
- Generate recursive broad/medium/small/micro/detail size families with counts/weights/aspect ranges.
- Support horizontal/vertical population mix, strip/unique/cap/radial quotas, margins, padding, bleed,
  alignment, occupancy target, minimum size, reserved banks, and profile policy.
- Ship parameter presets using one generator engine: Balanced Architecture, Horizontal Trim Bank,
  Vertical Panel Bank, and Radial Accent.
- Candidate generation must not mutate accepted topology. Accept is one undoable topology command and
  reports compatibility impact.
- Add the Layout Generator drawer and candidate-versus-accepted visual state.

Acceptance:
- Same recipe/seed/version produces byte-identical regions, IDs, profiles, ordering, and topology hash.
- Different seed produces a valid different candidate without touching accepted topology.
- Candidate summary reports counts, occupancy, padding/bleed, radial population, errors, and compatibility.
- Accept pins the candidate; save/reopen never reruns the generator.

Focused verification command:
cargo test -p hot-trimmer-geometry layout_generator

Add focused tests with that prefix. Make at most one correction pass, rerun, and stop. Include a manual
desktop checklist for generating, previewing, discarding, accepting, and reopening.
```

---

## Prompt 5 — Mapping & Warp and Radial Pattern

```text
Implement Prompt 5: general Mapping & Warp tools including radial, spiral, fisheye/lens, and arc mapping.

Read AGENTS.md and docs/trimsheet-document-generation-redesign-plan.md section 6 and Milestone 5.
Read the companion plan's Region source projection and UV warp stack section. Preserve current changes.
No subagents.

Scope:
- Implement CPU evaluation for Planar, Perspective, Polar/Radial, and Cylindrical/Arc projections.
- Implement ordered typed Spiral/Twirl and Radial Lens operations plus transform and address modes.
- Add Mapping & Warp UI with operation add/remove/reorder/enable/reset and typed numeric controls.
- Add Radial Pattern as a mapping preset: Polar/Radial projection plus optional Twirl and Lens. It must
  not cut a circular region, change topology, or use material-specific/end-grain wording.
- Add direct center, inner/outer radius, seam angle, direction, radial/angular scale, turns, falloff,
  lens strength/radius/bias, and addressing handles.
- Apply identical coordinates to every map and reorient tangent-space normals through the complete
  mapping Jacobian, including reflection handedness.
- Bound operation count/strength/radius/turns/sampling/allocation and return region-specific diagnostics
  for singular or invalid mappings.
- Persist, undo/redo, reopen, preview, and final compile through the canonical document.

Acceptance:
- Any rectangular region can display a radial, spiral, fisheye, or arc-mapped version of any source.
- Region/hotspot bounds and topology hash do not change.
- CPU golden fixtures cover normal mapping, mirror, seam, singular perspective, polar center, twirl,
  lens, and cylinder cases.
- Preview and final output agree within documented tolerance.

Focused verification command:
cargo test -p hot-trimmer-render-core warp_mapping

Add focused tests with that prefix. Make at most one correction pass, rerun, and stop. Include a manual
Radial Pattern checklist using center move, twirl, and fisheye strength.
```

---

## Prompt 6 — Structural profiles and generated maps

```text
Implement Prompt 6: structural profile and generated-map authoring through the canonical compiler.

Read AGENTS.md and docs/trimsheet-document-generation-redesign-plan.md sections 7.1 through 7.3 and
Milestone 6. Preserve all current changes. No subagents.

Scope:
- Implement/complete Flat, Convex Bevel, Concave Groove, Rounded Bevel, Double Bevel, Raised Lip,
  Recessed Seam, Panel Frame, Radial Disc, and Annulus analytic profile programs.
- Assign profiles globally, by generator group/size family, and per region with validated overrides.
- Generate structural Height, tangent-space Normal, AO/cavity, edge masks, and padding-aware dilation
  from authoritative current region/hotspot bounds.
- Compose imported/estimated material height with structural/decorative height and compose normals
  with a proper vector method.
- Add profile controls for width/radius, curve, inset/extrusion, selected edges, hardness, cavity response,
  normal intensity, and reference-resolution scaling.
- Mark generated missing channels Estimated and replace them cleanly when authored maps are imported.
- Ensure resolution changes scale profile/padding without moving normalized hotspots or changing IDs.

Acceptance:
- Profile changes visibly affect the selected region's Height/Normal/AO and material preview.
- Boundaries match overlays and ID/hotspot geometry exactly.
- 1K/2K/4K/8K fixtures preserve normalized topology and intended profile scale.
- Normal RGB is never averaged and Metallic is never guessed.

Focused verification command:
cargo test -p hot-trimmer-render-core structural_profile

Make at most one correction pass, rerun the same command, and stop. Include a manual profile checklist.
```

---

## Prompt 7 — Decorations and treatment layers

```text
Implement Prompt 7: region decorations and nondestructive treatment/map generators.

Read AGENTS.md and docs/trimsheet-document-generation-redesign-plan.md sections 7.4 and Milestone 7.
Preserve current worktree changes. No subagents.

Scope:
- Implement persisted, ordered DecorationBinding for Repeating Strip, Unique Detail, Trim Cap,
  Radial Detail, Seam/Panel/Bolt/Pattern, and Decal without changing region topology.
- Support per-channel blends, masks, placement, transform, deterministic seed, and compatible-region suggestions.
- Implement ordered treatment layers: Clean/Used/Heavy recipes, Grunge, Edge Wear, Dirt,
  Color/Roughness Adjust, Height Boost, Decal, and Mask.
- Support region/group/global targeting, cross-source masks, enable/disable/reorder, deterministic caches,
  undo/redo, save/reopen, and CPU/final preview parity.
- Compile decorations before treatments through the same CompiledSheet path.

Acceptance:
- A vent, repeating strip, radial detail, and trim cap coexist over one base material.
- Reordering or disabling treatments changes only targeted channels and is nondestructive.
- Same inputs/seeds reproduce identical output after reopen.

Focused verification command:
cargo test -p hot-trimmer-sheet-compiler decoration_treatment

Add focused tests with that prefix. Make at most one correction pass, rerun, and stop.
```

---

## Prompt 8 — Custom topology and Atlas parity

```text
Implement Prompt 8: Custom Template and Custom Atlas through the canonical topology/compiler path.

Read AGENTS.md and docs/trimsheet-document-generation-redesign-plan.md sections 4.1, 5, 10 and
Milestone 8. Preserve all current changes. No subagents.

Scope:
- Implement Clone Layout as Custom with explicit compatibility-key change and warning.
- Enable authoritative custom region move, resize, axis-aligned 90-degree rotate, add/delete, lock,
  collision/clearance repair, and numeric editing using the shared transactional gizmo.
- Reconnect seeded Atlas packing as a TopologyCandidate generator for patches/simple entries.
- Validate overlaps, bounds, minimum size, padding, bleed, occupancy, IDs, and profiles before acceptance.
- Compile Custom Template and Custom Atlas into the same CompiledSheet artifact as standard/generated templates.
- Remove CSS-background Atlas visualization as a substitute for raster output.

Acceptance:
- Standard template shows locked topology controls with Customize Layout action.
- Custom edits persist and match Base Color/Height/Normal/ID/hotspot pixels and metadata.
- Atlas packs and rasterizes real patch content in every map.
- Preview, save/reopen, export input, and undo use the same topology and artifact hashes.

Focused verification command:
cargo test -p hot-trimmer-geometry custom_topology

Add focused tests with that prefix. Make at most one correction pass, rerun, and stop. Include a manual
Custom/Atlas checklist.
```

---

## Prompt 9 — Material preview, export, and Blender artifact parity

```text
Implement Prompt 9: make 2D/3D preview, generic export, and Blender publish consume the exact same CompiledSheet.

Read AGENTS.md and docs/trimsheet-document-generation-redesign-plan.md sections 9, 10.4 and Milestone 9.
Read the companion plan's export, manifest, sync, and Blender matching contracts. Preserve all current changes.
No subagents.

Scope:
- Complete canonical 3D material preview for wall, broad panel, edge trim, strip, and radial fixture using
  the current CompiledSheet maps.
- Add channel/mask isolation and normal-strength/roughness inspection without recomputing source appearance.
- Implement atomic generic export with map selection, format/bit depth, packed views, OpenGL/DirectX normal
  option, cancellation, disk/memory estimate, staging, and no partial replacement.
- Manifest references exact document revision, topology/appearance hashes, map checksums, region/hotspot
  metadata, mapping/profile provenance, and color-space policy.
- Publish the same artifact atomically for Blender. Complete/update material nodes, hotspot browser,
  rectangular/strip/radial fit, locking, and appearance-versus-topology sync reports as defined by the companion plan.
- Appearance changes reload maps without changing UV assignments. Topology changes require compatibility reporting.

Acceptance:
- 2D preview, 3D preview, exported maps, manifest, and Blender revision share identical artifact hashes.
- Export cancellation or failure preserves previous valid output.
- Appearance-only publish preserves Blender UVs/locks; topology change never silently remaps.

Focused verification command:
cargo test -p hot-trimmer-export compiled_sheet

Add focused tests with that prefix. Make at most one correction pass, rerun, and stop. Provide manual export
and Blender verification checklists.
```

---

## Prompt 10 — Final qualification and deletion of obsolete paths

```text
Implement Prompt 10: qualify the completed trim-sheet redesign and remove obsolete live paths.

Read AGENTS.md, all acceptance scenarios and stop conditions in
docs/trimsheet-document-generation-redesign-plan.md, and the release gates in the companion plan.
Inspect the complete implementation and current git status. Preserve intentional user changes.
No subagents unless the user explicitly authorizes one bounded worker.

Scope:
- Run every redesign acceptance scenario through automated fixtures where practical and document the
  remaining manual GUI/Blender checks.
- Remove obsolete live layout/fill/source-transform/compiler paths, adapters, duplicated state, CSS-only
  render substitutes, stale phase labels, and unused tests/helpers only when proven unreferenced.
- Verify migration, crash recovery, cancellation, bounded resources, stale revision rejection, Unicode paths,
  read-only/disk-full behavior, high DPI, keyboard access, and deterministic save/reopen/export.
- Ensure unavailable features are disabled with an actionable prerequisite, never inert.
- Update phase/report documentation with delivered contracts, migrations, fixtures, performance, and limitations.

Acceptance:
- Every enabled authoring control results in a persisted command and matching CompiledSheet change or is an
  explicitly non-rendering viewport control.
- No old compiler can produce user-visible final maps.
- All plan stop conditions are false.
- The product loop from import through generator, mapping/warp, maps, preview, export, and Blender is coherent.

Focused verification command:
npm run check

This is the one intentionally broad final gate. Make at most one correction pass, rerun the same command,
and stop with a release-gate report rather than unrelated cleanup.
```

---

## Continuation prompt for an incomplete slice

Use this only when the immediately previous prompt did not reach its acceptance criteria.

```text
Continue the same Hot Trimmer slice; do not start the next prompt.

Read AGENTS.md, the previous task report, current git diff/status, and the exact prompt that defined this
slice. Preserve all existing changes. Work in the root task without adding agents.

Reproduce the reported blocker with the same focused verification command. Fix only the blocker that prevents
the current slice's acceptance criteria. Do not broaden the definition of done, redesign unrelated modules,
or implement future prompts. Make at most one correction pass, rerun the same verification command, and stop
with updated evidence and any remaining blocker.
```

---

## Prompt for a manual product verification task

Use this after a slice reports green when you want Codex to run the app and verify the visible behavior.

```text
Verify the just-completed Hot Trimmer slice in the running desktop app. Do not modify code unless I explicitly
ask for a fix after the report.

Read AGENTS.md, the slice's manual checklist, and the current project state. Launch or attach to the desktop app,
perform only that checklist, and record each observed result against the slice acceptance criteria. Capture the
first concrete failure with exact actions, selected IDs/modes, visible revision/build status, and relevant error
text. Stop after the checklist; do not widen into a general UX audit.
```
