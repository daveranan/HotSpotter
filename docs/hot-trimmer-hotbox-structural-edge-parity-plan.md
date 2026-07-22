# Hot Trimmer HotBox Structural Edge Parity Plan

Status: implementation-ready plan  
Date: 2026-07-21  
Scope: Region-derived structural borders, physical Height/Normal/AO parity, edge-weathering separation, and an optional Blender reference-bake mesh  
Primary verification command: `npm.cmd run test --workspace @hot-trimmer/desktop -- mvp-edge-wear`

## 1. Outcome

Hot Trimmer must produce panel and trim borders with the same readable, rounded edge character as the inspected
`HotBox_1.0_Concrete.blend` reference while preserving the application's fast native GPU workflow.

The production path will derive structural relief directly from Region boundaries. It will not require Blender or a
temporary mesh to generate native previews and maps. An optional Blender export will generate a HotBox-compatible
shallow island mesh for Cycles reference renders, editable handoff, and parity validation.

The central implementation decision is:

```text
final physical Height = permanent structural relief + optional chipped/worn relief
```

Procedural wear may alter Base Color, Roughness, Metallic, and low-amplitude chip detail. It must not erase the
underlying bevel or seam profile.

## 2. Relationship to existing plans

This plan is a focused follow-on to:

- `docs/hot-trimmer-native-edge-detail-implementation-plan.md`;
- `docs/hot-trimmer-v1-full-algorithm-stack-implementation-plan.md`;
- `docs/hot-trimmer-template-blender-companion-plan.md`.

The native Edge Detail pass already exists and produces physically scaled output. This plan does not replace that
work. It separates structural edge formation from Edge Detail weathering, connects authored Regions to structural
profiles, adds the missing cavity response, and defines the optional Blender reference-mesh path.

## 3. Measured baseline

### 3.1 HotBox reference

The live Blender file was inspected through Blender MCP in Blender 5.2.0 LTS.

| Property | Observed reference value |
|---|---:|
| Render engine | Cycles |
| Output | 4096 x 4096 |
| Camera | Orthographic, 2 m scale |
| View transform | AgX, Medium High Contrast |
| Light objects | None |
| World | White background, strength 1 |
| Trim variants | Six, `trim_1` through `trim_6` |
| Trim footprint | Approximately 2.0136 x 2.0136 m |
| Island height | 9.2586 mm |
| Straight-edge mesh chamfer | Approximately 4.0716 mm |
| Diagonal chamfer measurement | Approximately 5.7581 mm |
| Cycles Bevel radius | 7 mm |
| Cycles Bevel samples | 16 |

Each trim mesh is a collection of disconnected shallow panel and strip islands. The active `trim_4` contains 81
connected components. It has no Bevel modifier. Most side faces form the measured one-segment chamfer, and the
material supplies the smooth shading rolloff with a Cycles Bevel node.

The `HotBox_1.0` material group:

1. combines source texture detail with Bump;
2. feeds that normal through Cycles Bevel;
3. compares Bevel Normal with Geometry Normal;
4. remaps the normal deviation into several edge bands;
5. applies procedural breakup to material responses;
6. publishes Diffuse, Roughness, Metallic, Normal, Edge Mask, and per-island ID outputs.

The visible border is therefore neither a UV line nor a lighting-only effect. Geometry establishes independent island
boundaries, the Bevel node supplies continuous normal rolloff, and the material derives wear masks from that rolloff.

### 3.2 Current Hot Trimmer project

The active MCP project was inspected and exported at 2048 x 2048.

| Property | Current value |
|---|---:|
| Authored Regions | 24 |
| Structural profile | `flat` on every Region |
| Source channels | Base Color only |
| Physical sheet scale | 2.048 m, 1 mm per pixel at 2K |
| Edge width | 15 mm |
| Bevel radius | 4 mm |
| Height amplitude | -4 mm |
| Edge intensity | 0.72 |
| Edge wear amount | 0.5 |
| AO edge contribution | None |

The exported maps show:

- an Edge Mask band approximately 32 pixels wide;
- meaningful Height and Normal response only approximately 7 to 8 pixels wide;
- a narrow, noisy seam rather than a continuous rounded shoulder;
- entirely white AO;
- a subtle Roughness response;
- almost invisible Base Color edge response on the bright, low-saturation concrete source.

At the displayed preview scale, the 7-to-8-pixel native response becomes roughly 2-to-3 visible pixels.

## 4. Root cause

The current Edge Detail shader computes a rounded distance profile, but then multiplies the physical profile by the
combined noisy wear mask:

```text
combined = max(core, transition * 0.72, fade * 0.30) * intensity
edge_height = height_amplitude * rounded_profile * combined
```

This causes three failures:

1. `bevelRadius` controls only a narrow Height shoulder while `edgeWidth` mostly controls the material mask.
2. Wear, breakup, coverage, and intensity can weaken or remove the physical edge.
3. Regions with a `flat` structural profile have no permanent profile beneath Edge Detail.

The resulting Normal derivation and RNM composition are architecturally sound, but their physical Height input is too
narrow and inconsistent. Improving preview lighting alone cannot repair that input.

Relevant implementation points:

- `crates/preview/src/gpu_edge_detail.wgsl`;
- `crates/preview/src/gpu_edge_detail_composition.wgsl`;
- `crates/preview/src/gpu_structural_profile.wgsl`;
- `crates/effect-compiler/src/edge_detail.rs`;
- `crates/domain/src/templates/mod.rs`.

## 5. Product and architecture decisions

### 5.1 A border is derived topology

A border is not a separate Region. It is a physical band derived from the boundary of an authored Region.

The compiler must create one authoritative boundary record for each:

- Region-to-empty boundary;
- shared Region-to-Region boundary;
- eligible atlas perimeter edge.

Shared boundaries must be deduplicated. Two adjacent Regions must produce one seam with opposing shoulders, not two
overlapping edge effects. Existing per-edge eligibility remains authoritative for inclusion and exclusion.

### 5.2 Structural profiles are permanent

The structural pass owns:

- seam or cavity width;
- panel plateau height;
- bevel or chamfer radius;
- profile curve;
- boundary ownership and orientation;
- physical Height and derivative;
- structural semantic masks.

It must publish a stable result even when Edge Detail is disabled or procedural coverage is zero.

The first implementation should either make the existing `RoundedBevel` and `PanelFrame` programs valid for authored
Regions or add one explicit `PanelSeam` profile if the existing programs cannot represent opposing shoulders and a
central cavity without ambiguous parameter reuse.

### 5.3 Edge Detail is secondary weathering

Edge Detail continues to own:

- broad core, transition, and fade masks;
- breakup and coverage;
- source Height or high-passed luminance response;
- Base Color HSV response;
- Roughness and explicit Metallic response;
- low-amplitude chips and normal microdetail.

It may modulate an additional wear-height term, but it must not multiply the structural Height term.

### 5.4 Native GPU generation remains the default

The native pipeline is:

```text
Regions
-> unique boundary/SDF representation
-> deterministic structural Height + derivative + semantics
-> Edge Detail masks and chipped relief
-> composed physical Height
-> Normal derivation and RNM
-> AO/cavity + material channel composition
-> preview and export
```

This path provides the same physical concept as the HotBox islands without building geometry for every preview.

### 5.5 Blender mesh export is an optional parity path

The Blender companion may generate one disconnected island per Region using the same authoritative boundary/profile
parameters. The reference preset should initially use:

- 9.2586 mm island height;
- 4.0716 mm straight-edge mesh chamfer;
- 7 mm Cycles Bevel radius;
- 16 Bevel samples;
- shared atlas UVs;
- Region ID as the per-island identifier.

The mesh is intended for Cycles reference baking, editable handoff, and validation. Native map generation must not
depend on Blender availability.

## 6. Authoring contract

The structural edge contract needs explicit physical controls. Reuse existing profile fields where their semantics
already match; do not overload Edge Detail fields.

Required authoring concepts:

| Concept | Unit | Responsibility |
|---|---|---|
| Structural profile | enum | Flat, rounded bevel, panel frame, or panel seam |
| Panel/island height | meters | Plateau relative to the base surface |
| Seam width | meters | Central gap or cavity width |
| Bevel radius | meters | Width of each continuous shoulder |
| Cavity depth | meters | Structural seam depth |
| Edge eligibility | per side | Include/exclude Region and atlas edges |
| Perimeter policy | enum/bool | Whether the outer sheet border is profiled |

Edge Detail retains its independent wear width, breakup, intensity, color response, roughness response, microdetail,
and optional chip amplitude.

All physical fields must preserve apparent size across 512, 1024, 2048, and 4096 output resolutions.

## 7. Implementation work

### Phase 1: Structural boundary semantics

1. Define the structural edge intent and persistence/versioning behavior.
2. Compile authored Region rectangles and per-edge eligibility into unique boundary ownership.
3. Deduplicate shared boundaries deterministically.
4. Preserve stable Region IDs and existing template compatibility rules.
5. Publish the boundary data needed by the existing physical SDF/profile pass.

Acceptance:

- adjacent Regions produce one boundary;
- excluded edges produce no structural profile;
- atlas perimeter behavior is explicit and deterministic;
- save/load round-trips preserve all physical values;
- old projects load with their existing flat appearance until the user selects a structural preset.

### Phase 2: Continuous structural relief

1. Implement or adapt the rounded panel-seam profile in `gpu_structural_profile.wgsl`.
2. Generate a central cavity and smooth opposing shoulders from physical distance.
3. Keep structural Height independent of Edge Detail masks and intensity.
4. Publish analytic or numerically stable derivatives for Normal generation.
5. Validate corner joins, thin strips, adjacent rectangles, and sheet perimeter corners.

Acceptance:

- a clean cross-section is smooth and symmetric;
- the non-flat Normal response spans the configured bevel radius;
- the profile contains no one-pixel cliff at any supported resolution;
- corners do not spike, double in height, or change apparent radius;
- disabling Edge Detail does not remove the structural edge.

### Phase 3: Separate wear from structure

1. Change Edge Detail composition to add chipped/worn Height after structural Height.
2. Keep core, transition, and fade masks for channel weathering.
3. Limit high-frequency chip displacement to an explicit, low-amplitude control.
4. Ensure `normalDetailStrength` reweights only the intended detail contribution.
5. Bump the algorithm/cache identity where rendered semantics change.

Acceptance:

- wear amount zero and one produce the same base structural silhouette;
- breakup can interrupt discoloration and chips but cannot erase the base bevel;
- Normal remains derived from final composed physical Height;
- cache hits cannot return maps produced by the coupled legacy algorithm.

### Phase 4: AO, material response, and preview

1. Derive cavity/AO response from structural SDF, Height, or local horizon approximation.
2. Compose AO independently from Base Color darkening.
3. Add a preview lighting profile that clearly exposes Normal and Roughness response.
4. Keep Material, Base Color, Normal, Height, Roughness, AO, and Edge Mask views authoritative.
5. Ensure the Material preview is evaluated against the same exported maps.

Acceptance:

- AO is no longer uniformly white when a recessed seam exists;
- cavity darkening follows the seam without dirtying panel centers;
- Base Color-only and Material views remain distinguishable;
- changing preview lighting does not change exported maps.

### Phase 5: HotBox Concrete preset and controls

1. Add a structural profile selector to the relevant Region/template authoring surface.
2. Separate structural controls from the existing Edge Detail wear controls.
3. Add a `HotBox Concrete` preset seeded from the measured physical reference.
4. Display millimeters consistently while persisting meters.
5. Explain that `Edge Width` is the weathering band and `Bevel Radius` is structural.

Initial preset targets:

| Setting | Initial target |
|---|---:|
| Structural island height | 9.26 mm |
| Mesh-equivalent chamfer | 4.07 mm |
| Structural/shading bevel radius | 7 mm |
| Edge Detail width | 15 mm |
| Structural coverage | 100% |
| Final bake resolution | 4K |

The preset is a starting point, not a hard-coded dependency. It must remain editable and physically scaled.

### Phase 6: Optional Blender reference mesh and bake

1. Add a companion command that converts Region boundaries into disconnected shallow islands.
2. Project the same atlas UV coordinates used by native map generation.
3. Assign Region IDs consistently with package metadata.
4. Create or reuse a material containing the source maps and a Cycles Bevel node.
5. Offer a non-destructive reference bake into a new collection or scene.
6. Do not alter or save the user's Blender file without an explicit user action.

Acceptance:

- generated island counts match eligible Regions/components;
- world dimensions match package metadata;
- UVs address the same atlas areas as native output;
- Cycles Bevel uses the exported physical radius;
- re-running the command updates or replaces its owned collection without duplicating content;
- the generated scene can bake Base Color, Height, Normal, Roughness, AO, Edge Mask, and ID.

## 8. Validation matrix

The focused fixture should contain:

- one isolated rectangular panel;
- two adjacent panels sharing one edge;
- a four-panel intersection;
- a very thin horizontal strip;
- a very thin vertical strip;
- an eligible atlas perimeter;
- an excluded Region edge;
- at least one non-rectangular case when Region geometry supports it authoritatively.

For each supported output resolution, validate:

1. physical cross-section width in meters;
2. peak and baseline Height;
3. Normal support width and direction;
4. AO response at the cavity and panel plateau;
5. shared-boundary deduplication;
6. deterministic output for a fixed seed;
7. structural invariance across wear seeds and wear amounts;
8. preview/export parity.

The 4K HotBox-compatible fixture should additionally compare:

- native Height and Normal line profiles;
- Blender reference-bake Height and Normal line profiles;
- visual Material renders under a shared camera, World, and view transform.

Exact pixel equality between native rasterization and Cycles is not required. Physical width, slope direction,
continuity, semantic ownership, and perceptual edge readability are required.

## 9. Rollout and compatibility

- Existing projects retain their current `flat` structural profile by default.
- Applying `HotBox Concrete` is an explicit authored change.
- New structural fields require deterministic defaults and migration coverage.
- Any change to rendered meaning must update revision, cache key, and telemetry algorithm identity together.
- The package must record enough physical profile data for Blender to reproduce the authored structure.
- The optional Blender feature must fail clearly when Blender/MCP is unavailable without blocking native preview or
  export.

## 10. Definition of done

This plan is complete when:

- Regions generate permanent, deduplicated structural borders;
- structural Height is independent of procedural wear coverage;
- smooth bevel shoulders survive preview downscaling and 2K/4K export;
- Normal is derived from the complete physical Height stack;
- AO contains a real structural cavity contribution;
- the HotBox Concrete preset produces a materially closer result in one action;
- the optional Blender command produces HotBox-style disconnected island geometry from the same Region contract;
- focused regression tests pass with the primary verification command;
- a documented 4K native-versus-Cycles comparison satisfies the validation matrix.

## 11. Recommended execution order

Implement Phases 1 through 3 first. They correct the physical signal and determine whether the native result can meet
the target. Add AO and preview presentation next, then expose the preset. Build the Blender mesh/bake path last because
it should consume the now-authoritative structural contract rather than define a second one.

Do not begin by tuning concrete color, preview contrast, or procedural noise. Those adjustments can improve
presentation, but they cannot compensate for a missing continuous structural profile.
