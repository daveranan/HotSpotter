# Hot Trimmer V1 Full Algorithm Stack Implementation Plan

**Status:** Approved planning baseline  
**Source of truth:** `docs/hot-trimmer-v1-full-algorithm-stack-revised.md` revision 1.1  
**Implementation posture:** Quality-first engine replacement. Existing project files, schema versions, compiler internals, and render behavior do not require backward compatibility.  
**Product boundary:** Preserve the useful desktop authoring workflow and replace or reshape its native contracts, algorithm, compiler, and render layers as required.

## 1. Outcome

Replace the current direct crop-and-profile renderer with the complete twenty-stage material compiler described in the revised algorithm-stack document.

The completed system must:

- Analyze and prepare one or more registered material sources.
- Select globally diverse, physically scaled source content for every semantic template slot.
- Never apply silent non-uniform scaling.
- Synthesize missing material domain and slot content through deterministic routed algorithms.
- Compile structural profiles, details, weathering, and material state against physical and raster constraints.
- Compose registered PBR channels correctly.
- Produce exact IDs, metadata, diagnostics, preview output, and Blender assignments.
- Return explicit insufficiency and fallback decisions instead of hiding invalid source or effect choices.

This is not an extension of the present rendering shortcut. The new plans and compiler become authoritative, and obsolete paths are deleted after parity gates pass.

## 2. Scope decisions

### In scope

- All twenty stages in the revised stack.
- Every classical material-domain route: direct, graph-cut closure, quilting, PatchMatch, statistical/spectral, and procedural reconstruction.
- A versioned learned-route interface for de-lighting, expansion, super-resolution, and estimated PBR maps. Learned routes are optional at runtime; deterministic classical/procedural fallbacks are mandatory.
- The complete Source Placement Solver and global diversity optimization.
- All legal sampling modes in the non-distortion contract.
- Scale-constrained profiles, semantic details, effect compilation, PBR composition, supersampling, channel-aware filtering, mip validation, and diagnostics.
- UI controls and debug views required to configure, run, inspect, override, and recover the compiler.
- Manifest/export and Blender-companion behavior required by Stage 20.

### Out of scope

- Opening or migrating old Hot Trimmer project files.
- Preserving the current SQLite schema, document JSON shape, IPC protocol, or renderer API.
- Pixel parity with the current sheet compiler.
- Keeping current template repacking, manual crop, normalized-profile, or universal-weathering assumptions.
- Network-hosted inference. Any learned route must be local, version-pinned, bounded, and reproducible.

### Fixed design decisions

1. Standard templates use fixed, versioned 4096 x 4096 integer topology. They are never runtime-packed.
2. Custom templates may be authored separately, but accepting one pins its integer rectangles before material compilation.
3. Topology is independent of source content, output resolution, weathering, and effect-route selection.
4. `SamplingPlan` and `EffectPlan` are first-class, inspectable compiler artifacts.
5. User intent is persisted; large derived analysis fields and intermediate images are content-addressed cache artifacts.
6. Every derived artifact is keyed by source digests, settings, algorithm versions, template hash, output specification, and seed.
7. Preview and final output use the same algorithms. Preview changes resolution and work limits, not semantics.
8. Explicit Stretch is the only route allowed to use non-uniform scale.

## 3. Current implementation assessment

The existing code supplies useful primitives but not the revised algorithm architecture.

| Area | Reusable foundation | Required replacement or extension |
| --- | --- | --- |
| Source ingestion | Ten registered channel roles, immutable imported bytes, dimension registration, EXIF orientation, ICC detection | Canonical typed channel buffers, alpha handling, normal convention/validation, full pyramids, source-set preparation records |
| Rectification | Four-point homography, polygon-assisted quadrilateral, bounded deterministic CPU rectification | Registered multi-channel rectification, optional lens correction, masks, confidence, prepared exemplars |
| Templates | 4096-unit templates, stable slot identities, roles, world sizes, topology hashes | Remove standard-template semantic repacking; expand slot demand/effect vocabulary and fixed hotspot/allocation semantics |
| Cropping | Template/manual crop and center-cover helper | Replace with candidate generation, scoring, synthesis candidates, global optimization, and explicit sampling plans |
| Sampling | Clamp/repeat/mirror addressing and preliminary radial mapping | Typed legal mapping modes, isotropic-scale validation, shared correspondence fields, filtered/vector-correct channel sampling |
| Profiles | Seven normalized SDF-like profiles and basic normal generation | Physical profile constraint solving, complete profile set, legality/fallback plans, feature LOD, supersampling, physical gradients |
| PBR | Imported maps plus hard-coded structural overrides | Composable float/scalar/vector channel pipeline, imported-normal composition, estimation provenance, physical channel contributions |
| Effects | Persisted treatment metadata | Full effect definitions, capacity analysis, role-specific compilation, structural masks, weathering, LOD, diagnostics |
| Finishing | Nearest valid-pixel allocation dilation and simple ID painting | Channel-specific bleed/downsampling/mips, hotspot-only Region ID, exact Material ID, survival validation |
| Preview | Bounded 2D compiled-map preview | Shared staged compiler, analysis/placement/effect debug views, geometry fixtures, cancellation and progressive refinement |
| Export/Blender | Manifest skeleton and minimal rectangular/radial helpers | Complete manifest, atomic map package, material update, UV-island classification/fitting, locks, revision reload, diagnostics |

Specific current behaviors that must not survive as hidden fallbacks:

- `MappingTransform.scale` permits independent X/Y values.
- The document compiler ultimately samples through integer nearest lookup even when a sampling policy exists.
- Template-authored crops are treated as the placement answer rather than candidate hints or explicit overrides.
- Structural Height and Normal overwrite imported contributions instead of composing them.
- Profile widths and amplitudes are normalized to the slot minor edge rather than expressed physically.
- Region and Material IDs are painted over allocation rectangles; Region ID must cover hotspot rectangles only.
- Stored treatments and decorations are not compiled into output.
- The Blender companion records fit values but does not author UVs, preserve locks, or synchronize a complete package.

## 4. Target engine boundaries

The workspace should converge on these ownership boundaries. Names may be adjusted during implementation, but responsibilities must not be merged back into one monolithic compiler file.

### `crates/domain`

Owns serialized user intent and stable value types:

- Material-source registration and channel interpretation.
- Physical scale, orientation overrides, material class, confidence, and preparation settings.
- Fixed template topology, slot roles, mapping permissions, and effect requests.
- Material-state recipes and detail definitions.
- Output, quality, determinism, memory, and mip-survival settings.
- Explicit user overrides, including Stretch.

It does not own executable image buffers or derived analysis fields.

### `crates/image-io`

Owns safe decode/encode and canonical channel conversion:

- Color-managed Base Color decode into linear working color plus sRGB display output.
- Linear scalar and integer ID decode.
- Tangent-normal decode, convention conversion, validation, and normalization.
- Alpha unpremultiplication and transparency policy.
- Tiled image access and channel-correct pyramid construction.

### New `crates/material-analysis`

Owns Stages 3 through 7 after canonical decode:

- Registered rectification orchestration.
- De-lighting and exposure normalization.
- Quality measurement and material classification.
- Physical-scale and orientation calibration.
- Saliency, structure, stationarity, periodicity, seamability, and usability fields.

### New `crates/material-synthesis`

Owns material-domain and correspondence-field algorithms:

- Direct domain.
- Graph-cut periodic closure.
- Texture quilting.
- PatchMatch.
- Statistical/spectral synthesis.
- Procedural reconstruction engines.
- Versioned learned-route adapter.

Every route works on a registered channel set and returns one shared correspondence/operation field.

### New `crates/placement-solver`

Owns Stages 10 through 13 as they relate to source material:

- Resolved slot material footprint.
- Legal candidate generation.
- Unary scoring.
- Pairwise repetition/diversity scoring.
- Deterministic beam search and local optimization.
- Final `PlacementPlan` containing one `SamplingPlan` per slot.

### New `crates/effect-compiler`

Owns effect capacity and Stages 15, 16, and 18:

- Physical and raster capacity calculation.
- Profile/detail/weathering applicability and occupancy.
- Role-specific evaluators and fallbacks.
- Feature LOD and supersampling selection.
- Final `EffectPlan` and compilation report.

### `crates/render-core`

Becomes a low-level deterministic raster/kernel library:

- Typed float/scalar/vector/ID planes.
- Filtered sampling, correspondence evaluation, graph costs, SDFs, gradients, normal composition, masks, noise, morphology, and channel-correct resampling.
- No UI policy, no candidate selection, and no uncompiled raw effect rendering.

### `crates/sheet-compiler`

Becomes the sole orchestration boundary:

```text
prepare sources
-> build/select material domains
-> compile fixed topology
-> resolve slot demands
-> solve placements
-> synthesize slot material
-> compile profiles/details/weathering
-> compose PBR
-> finish atlas and validate
-> return maps + SamplingPlan + EffectPlan + diagnostics
```

### Persistence, IPC, UI, export, and Blender

- `crates/project-store` persists the new document without legacy migration code and manages content-addressed cache records.
- `packages/ipc-contracts` exposes staged jobs, progress, cancellation, summaries, overrides, plans, and debug views.
- `apps/desktop` keeps the useful source/sheet workbench but replaces controls that assume manual per-slot crop/render truth.
- `crates/export` packages authoritative compiler output and the complete manifest atomically.
- `integrations/blender` consumes manifest semantics and performs actual material synchronization and UV fitting.

## 5. Authoritative contracts

These contracts are implemented before expensive algorithms. They must represent every revised route without generic JSON blobs.

### Prepared material contracts

- `RegisteredChannelSet`: channel identity, digest, dimensions, color/data interpretation, normal convention, provenance.
- `PreparedExemplar`: registered rectified channels, masks, working-space metadata, scale/orientation estimates, confidence.
- `SourceQualityReport`: sharpness, noise, compression, range, clipping, perspective, registration, usable area, estimated resolution.
- `MaterialClassification`: all behavior classes from Stage 5 plus confidence and user override.
- `FeatureFields`: saliency, structure, stationarity, periodicity/lattice, seamability, usability, orientation and confidence.
- `PreparedMaterialDomain`: domain route, dimensions, physical scale, registered channels, correspondence field, seed, route diagnostics.

### Placement contracts

- `ResolvedSlotDemand`: every field listed in Stage 10, including physical dimensions, pixel density, scale permissions, variation group, visual importance, survivability, and effect capacity.
- `SamplingMode`: DirectCrop, PeriodicTile, RepeatX, RepeatY, TextureSynthesis, UniqueContain, UniqueCover, ThreeSliceCap, NineSlicePanel, PlanarRadial, PolarRadial, ExplicitStretch.
- `CropCandidate`: source/domain, exact crop, isotropic scale, transform, route, descriptors, unary costs, seam/repeat data, correspondence reference.
- `SamplingPlan`: selected source/domain, crop, physical transform, mapping/synthesis mode, repeat period, seam/correspondence data, seed, and fallback provenance.
- `PlacementPlan`: ordered per-slot plans, objective breakdown, pairwise diversity decisions, solver version, seed, and validation summary.

### Effect contracts

- `EffectScaleSpace`: World, SlotMinorRelative, SlotMajorRelative, SlotAreaRelative, Pixels.
- `EffectApplicability`, `EffectDefinition`, and explicit channel contribution types.
- `EffectCapacity`: physical fit limits, raster thresholds, supported variants, LODs, and supersampling recommendation.
- `CompiledEffect`: evaluator, role variant, physical/pixel scale, LOD, supersampling, channel targets, dependencies, fallback reason.
- `EffectPlan`: ordered compiled profiles, details, weathering, generated masks, occupancy, survival targets, and deterministic report.

### Provenance and determinism

Every analysis, domain, placement, effect, and render artifact records:

- Algorithm ID and version.
- Input/content hashes.
- Settings hash.
- Template topology hash where applicable.
- Output specification.
- Seed.
- Route and fallback choices.
- Estimated/imported/measured/user-authored provenance.

Iteration order, tie-breaking, candidate truncation, parallel reductions, and random-number streams must be stable.

## 6. Stage traceability

| Stage | Deliverable | Implementation phase | Primary owner |
| --- | --- | --- | --- |
| 1 | Immutable registered input set | Phase 1 | image-io/domain |
| 2 | Canonical color, data, normal, alpha, and pyramids | Phase 1 | image-io/render-core |
| 3 | Registered rectified exemplars | Phase 1 | material-analysis/geometry |
| 4 | De-lit Base Color plus confidence masks | Phase 2 | material-analysis |
| 5 | Quality report and material behavior class | Phase 2 | material-analysis |
| 6 | Physical scale and orientation field | Phase 2 | material-analysis |
| 7 | Six feature fields and usability | Phase 2 | material-analysis |
| 8 | Routed material domains | Phase 3 | material-synthesis |
| 9 | Exact fixed template topology | Phase 1 | domain |
| 10 | Slot demand and effect capacity | Phase 4 | placement/effect compilers |
| 11 | Legal direct and synthesis candidates | Phase 4 | placement-solver |
| 12 | Complete unary candidate costs | Phase 4 | placement-solver |
| 13 | Globally diverse deterministic assignment | Phase 4 | placement-solver |
| 14 | Registered per-slot material synthesis | Phase 5 | material-synthesis/sheet-compiler |
| 15 | Legal scale-constrained structural profiles | Phase 6 | effect-compiler/render-core |
| 16 | Legal scale-constrained details and motifs | Phase 6 | effect-compiler/render-core |
| 17 | Vector-correct, provenance-aware PBR composition | Phase 7 | sheet-compiler/render-core |
| 18 | Role-specific compiled material state/effects | Phase 6 | effect-compiler |
| 19 | Channel-correct atlas, mips, exact IDs, manifest data | Phase 7 | sheet-compiler/export |
| 20 | Preview, Blender synchronization, and QA views | Phase 8 | desktop/Blender |

Stage 18 compiles raw effects before Stage 17 renders and composes their contributions, matching the revised compilation pseudocode even though their document numbers are reversed.

## 7. Implementation phases

Each phase is a gate. A later phase may start only when the preceding contracts and focused fixtures are green. Temporary adapters may exist during a phase, but the old and new engines must not both claim authority.

### Phase 0 - Engine skeleton and destructive cutover

Deliverables:

- Create the new crate boundaries and typed image/plan primitives.
- Replace the document schema with one designed around source preparation settings, fixed templates, placement settings, effect recipes, and output settings.
- Reset the project-store schema to a new baseline; do not add legacy migrations.
- Replace IPC v1 with a staged, cancellable job contract.
- Add cache keys and version constants for every compiler stage.
- Add a single compiler facade returning plans, maps, reports, and debug handles.
- Capture performance baselines and hard memory/operation bounds for representative 8K input.

Acceptance gate:

- The workspace builds with the new contracts.
- A minimal fixed template travels through an empty deterministic compiler skeleton.
- Identical requests produce identical serialized plan headers and cache keys.
- Cancellation and revision supersession cannot publish partial results.

Targeted verification:

```powershell
cargo test -p hot-trimmer-domain -p hot-trimmer-sheet-compiler compiler_contract
```

### Phase 1 - Canonical inputs, rectification, and fixed topology (Stages 1-3 and 9)

Deliverables:

- Decode Base Color into linear working color and retain sRGB display output.
- Decode scalar maps as linear data and IDs as exact integer data.
- Decode, normalize, validate, and convert OpenGL/DirectX normals.
- Resolve premultiplied alpha and build registered channel pyramids.
- Enforce oriented Base Color dimensions across companion maps.
- Rectify every registered channel through the same homography and mask.
- Add optional lens-correction parameters and usable planar-area selection.
- Load standard templates from their authored integer allocation/hotspot rectangles only.
- Remove standard-template semantic packing and topology mutation controls.

Acceptance gate:

- A registered PBR set remains pixel-aligned after orientation, rectification, and pyramid generation.
- Normal vectors remain unit length and conventions are fixture-correct.
- Scalar and ID inputs never receive display gamma.
- Template rectangles share exactly scaled boundaries at 1K, 2K, 4K, and 8K.
- Material and resolution changes do not change normalized hotspots.

Targeted verification:

```powershell
cargo test -p hot-trimmer-image-io -p hot-trimmer-render-core -p hot-trimmer-domain registered_preparation
```

### Phase 2 - Source intelligence (Stages 4-7)

Deliverables:

- Classical low-frequency de-lighting in linear/log luminance with highlight/shadow masks.
- Local learned intrinsic-image adapter with deterministic model/version metadata and classical fallback.
- Quality measures for sharpness, noise, compression, range, clipping, perspective, registration, usable area, and material resolution.
- Heuristic classification for every material behavior class plus user override.
- Physical-scale calibration from metadata, two-point measurement, motif size, convention, or low-confidence prior.
- Global and local orientation fields using Scharr gradients and structure tensors.
- Multi-scale saliency, structure, stationarity, periodicity/lattice, seamability, and usability fields.
- Low-resolution inspection views and quantitative fixture reports.

Acceptance gate:

- Analysis fields are registered, bounded, deterministic, and cacheable.
- Directional wood/brushed-metal fixtures recover orientation within declared tolerance.
- Brick/manufactured fixtures recover dominant periods without promoting arbitrary edges.
- Logos, severe highlights, shadows, transparency, and invalid registration are excluded by usability policy.
- Confidence and provenance survive serialization; estimates are never labeled measured.

Targeted verification:

```powershell
cargo test -p hot-trimmer-material-analysis feature_field_goldens
```

### Phase 3 - Material-domain construction and router (Stage 8)

Implement routes in this order behind one interface:

1. Direct clean/tileable domain.
2. Multi-channel graph-cut periodic closure.
3. Deterministic texture quilting with minimum-cost overlaps.
4. Multi-channel PatchMatch correspondence optimization.
5. Statistical/spectral synthesis for stochastic materials.
6. Procedural reconstruction for wood face/end grain, brick/tile lattice, corrugation, brushed metal, concrete aggregate, and painted metal layers.
7. Learned seamless expansion and super-resolution adapter.

Rules:

- One seam/correspondence field drives every registered channel.
- Seam costs include relevant color, gradient, Height, Normal, Roughness, and structure terms.
- Normals blend as vectors and renormalize; IDs select discretely and never blend.
- The router may compare bounded low-resolution previews but uses stable scoring and tie-breaking.
- Unsupported semantic patterns must not fall into spectral synthesis.

Acceptance gate:

- Every route produces registered output, explicit provenance, and deterministic results.
- Periodic boundaries meet per-channel seam thresholds.
- Structured fixtures retain lattice, grain, or motif semantics.
- Insufficient inputs return explicit route failures and recovery choices.

Targeted verification:

```powershell
cargo test -p hot-trimmer-material-synthesis domain_route_goldens
```

### Phase 4 - Slot demand and Source Placement Solver (Stages 10-13)

Deliverables:

- Resolve physical slot footprint, pixels/meters, aspect, role, allowed transforms, importance, variation/material groups, raster survivability, and effect capacity.
- Generate isotropically scaled candidate sizes and coarse-to-fine positions.
- Add stationary, salient, period-aligned, feature-aware, and farthest-point candidate centers.
- Generate only legal rotations/mirrors for the source class and slot.
- Generate role-specific direct, repeat, contain/cover, cap, radial, and synthesis candidates.
- Score all unary terms from Stage 12 with an inspectable cost breakdown.
- Keep a stable top K, defaulting to 64 after fixture tuning.
- Perform deterministic importance/tightness ordering, beam search, and local swap/replacement optimization.
- Apply class/saliency/variation-aware pairwise overlap and duplicate-feature costs.
- Validate the final plan for non-uniform scaling and shared registered mapping.

Acceptance gate:

- The 8000 x 2000 worked-example fixtures produce legal square, horizontal, vertical, and radial outcomes.
- The vertical orientation-locked failure never stretches and offers the documented recovery choices.
- Large visible slots do not repeat the same salient feature unless policy permits it.
- Fixed inputs and seed produce byte-identical `PlacementPlan` output.
- A source-usage view proves the selected crop, transform, overlap, and diversity decision for every slot.

Targeted verification:

```powershell
cargo test -p hot-trimmer-placement-solver source_placement_acceptance
```

### Phase 5 - Per-slot material synthesis (Stage 14)

Deliverables:

- Execute DirectPhysical, PeriodicTile, RepeatX, RepeatY, UniqueContain, UniqueCover, ThreeSliceCap, NineSlicePanel, PlanarRadial, PolarRadial, and routed synthesis plans.
- Evaluate slot coordinates in meters before mapping to a prepared domain.
- Preserve strip cross-axis thickness and cap/corner widths.
- Share source positions and correspondence across all registered maps.
- Add vector-correct normal sampling and filtering.
- Keep planar material planar inside ordinary radial slots; polar mapping is explicit.

Acceptance gate:

- Property tests find no non-uniform scale outside ExplicitStretch.
- Imported PBR maps never drift under crop, repeat, synthesis, mirror, rotation, or radial mapping.
- Repeat seams and cap transitions meet the declared thresholds.
- Circular source details remain circular after legal mapping.

Targeted verification:

```powershell
cargo test -p hot-trimmer-sheet-compiler slot_synthesis_acceptance
```

### Phase 6 - Effect-capacity compiler, profiles, details, and material state (Stages 15, 16, and 18)

Deliverables:

- Implement the complete profile set: Flat, convex/concave/rounded/double bevel, raised lip, recessed seam, panel frame, fully rounded strip, merged opposing bevel, radial disc, annulus, and custom profile curve.
- Resolve profile widths and amplitudes in physical units; test opposing-profile legality and required flat center.
- Implement declared profile fallbacks and record every decision.
- Implement semantic repeating strips, unique/radial details, trim caps, bolt groups, vents, stamps, grooves, decals, and procedural motifs.
- Convert masks to SDFs for coherent Height/Normal contributions.
- Implement world, minor-relative, major-relative, area-relative, and pixel scale spaces.
- Build structural masks in physical coordinates.
- Compile surface, horizontal-strip, vertical-strip, radial, unique, and trim-cap weathering variants.
- Implement micro/meso/macro/structural material-state recipes.
- Select deterministic feature LOD and 1x/2x/4x/8x supersampling from physical and raster constraints.

Acceptance gate:

- Golden fixtures cover the required panel, extreme strips, radial, cap, sub-pixel, and opposing-bevel cases at all four output sizes.
- Opposing profiles never overlap accidentally.
- Isotropic physical effects never become ellipses in extreme slots.
- Resolution promotes LOD without moving features or changing seeds.
- Unfit effects are rejected or use a declared fallback visible in `EffectPlan` diagnostics.

Targeted verification:

```powershell
cargo test -p hot-trimmer-effect-compiler scale_aware_effect_goldens
```

### Phase 7 - PBR composition and atlas finishing (Stages 17 and 19)

Deliverables:

- Compose Height with explicit physical amplitudes and material-class clamps.
- Generate normals from physical Height using Scharr gradients and per-axis meters per pixel.
- Combine imported and generated normals with vector-correct reoriented normal mapping.
- Prefer imported Roughness; otherwise estimate it with explicit Estimated provenance.
- Keep Metallic at zero unless imported, labeled metal, material-ID-driven, or exposed-metal effect-driven.
- Generate multi-radius physical AO/cavity.
- Render effects at compiled supersampling, then downsample by channel semantics.
- Render/evaluate allocation bleed without contaminating IDs.
- Fill Region ID over hotspot only and Material ID by exact material label.
- Generate channel-correct mips and validate feature survival at mip 0, 1, 2, and configured viewing target.
- Produce the complete manifest payload and deterministic compilation summary.

Acceptance gate:

- All output channels have identical dimensions and registered boundaries.
- Normal filtering/composition passes vector fixtures; no RGB averaging exists.
- Region ID is exact at mip 0 with no antialiasing, bleed, transform, or dithering.
- Supersampling changes raster fidelity, never physical dimensions.
- Mip validation catches disappearing and over-strengthened features.
- Same complete input tuple produces byte-identical maps and reports.

Targeted verification:

```powershell
cargo test -p hot-trimmer-sheet-compiler atlas_finishing_acceptance
```

### Phase 8 - UI, preview, QA, export, and Blender (Stage 20)

UI deliverables:

- Keep source registration, patch capture, split source/sheet workbench, map selection, and inspector patterns where they remain useful.
- Add source class, physical-scale calibration, orientation, preparation, synthesis policy, quality/confidence, and seed controls.
- Add per-slot automatic/manual status, candidate comparison, selected crop, recovery choices, and explicit Stretch override.
- Add material-state recipe and effect fallback controls.
- Add all QA views from Stage 20, including crop usage/repetition, seam energy, texel density, effect route/occupancy/LOD/supersampling, mip warnings, and Blender status.
- Run analysis, placement, compile, and export as cancellable revision-guarded jobs with bounded preview refinement.

Preview/export/Blender deliverables:

- Preview Plane, Cube, Cylinder, Beveled Block, Wall Module, Archway, Radial Disc, and Mechanical Prop, including authored hotspot UV fixtures.
- Export all compiled maps, checksums, colorspaces, topology, world sizes, fit rules, radial data, route summaries, and revision data atomically.
- Build/update the complete Blender Principled material, including AO/Height policy.
- Describe selected UV islands, classify compatible slots, and fit rectangular, strip, unique, cap, and radial semantics without non-uniform distortion.
- Preserve locked assignments across material/effect/resolution updates.
- Reload maps without remapping when topology is unchanged; report topology mismatch explicitly.

Acceptance gate:

- Every QA view is generated from authoritative artifacts, not duplicated UI math.
- Preview and exported maps match within declared channel tolerances.
- Blender fixtures pass rectangular, strip, and radial mapping without non-uniform UV distortion.
- Locked assignments survive material updates and map revisions reload without remapping.
- Failed/cancelled exports never publish a partial package.

Targeted verification:

```powershell
npm test --workspace @hot-trimmer/desktop -- stage-20 && python -m unittest discover integrations/blender/hot_trimmer_companion/tests
```

### Phase 9 - Full V1 qualification and old-engine removal

Deliverables:

- Run every acceptance criterion in section 30 of the revised design.
- Run the complete golden matrix across material classes, source quality failures, mapping roles, effects, atlas sizes, and Blender fixtures.
- Measure 8K memory, analysis/solve/compile time, preview latency, cancellation latency, and cache reuse on named hardware.
- Fuzz/bound all image dimensions, parameters, candidate counts, graph sizes, iteration counts, supersampling, and IPC payloads.
- Verify offline determinism, crash-safe project writes, cache loss recovery, and atomic export.
- Delete the legacy sheet compiler, normalized profile path, legacy IPC/contracts, obsolete layout repacking, and unused schema/migration fixtures.
- Update technical, architecture, diagnostics, algorithm-version, and Blender documentation.

Final gate:

```powershell
npm run check
```

No compatibility waiver, legacy fallback, or disabled acceptance fixture counts as completion.

## 8. Fixture and evidence plan

### Source fixtures

- Clean tileable PBR set.
- Fine stochastic concrete/plaster.
- Rust/grunge.
- Brushed metal.
- Brick/tile lattice.
- Wood face grain and separate end grain.
- Manufactured repeating border.
- Unique vent/panel and radial drain/washer.
- Mixed/unknown material.
- 8000 x 2000 worked-example source.
- Perspective photo, clipped/highlighted photo, logo/occluder case, low-resolution source, and deliberately misregistered PBR set.

### Geometry/effect fixtures

- 32 x 32 broad panel.
- 0.5 x 10 horizontal strip.
- 10 x 0.5 vertical strip.
- 1 x 1 radial slot.
- Thin trim cap.
- Sub-pixel bevel.
- Opposing bevels with no legal center.
- 1K, 2K, 4K, and 8K atlas output.

### Required evidence per phase

- Unit/property tests for mathematical invariants.
- Deterministic JSON plan goldens.
- Per-channel image goldens with explicit tolerances.
- Failure/recovery fixtures, not just successful examples.
- Performance and memory measurement for the named representative fixture.
- A short phase report recording delivered routes, tests, measurements, known limits, and gate result.

## 9. Cross-cutting acceptance invariants

These are checked continuously rather than deferred to Phase 9.

### Geometry and sampling

- Unique crop aspect equals destination aspect before resampling.
- Default scale is isotropic.
- Strip cross-axis thickness is preserved.
- Radial proportion is preserved.
- Registered channels use one mapping/correspondence field.
- Standard-template topology never depends on material or appearance.

### Physical and raster scale

- Physical feature size is stable across slot aspect ratios and output resolutions.
- Feature LOD changes representation, not placement or seed.
- Supersampling improves sampling only.
- Effects and profiles cannot occupy physically impossible space.

### Channel correctness

- Base Color is color managed.
- Scalar data remains linear.
- Normals are decoded, transformed, filtered, composed, normalized, and re-encoded as vectors.
- IDs remain exact categorical data.
- Metallic is never inferred without an explicit allowed source.

### Determinism and failure

- Stable input tuple yields stable routes, plans, diagnostics, and outputs.
- Insufficient sources and invalid effects produce actionable typed failures.
- Explicit Stretch is visible in plans and diagnostics.
- Cancellation, failure, or revision supersession cannot publish partial authoritative state.

## 10. Execution rules for following this plan

1. Implement one phase at a time and keep its targeted gate green.
2. Do not add UI-only calculations that duplicate compiler truth.
3. Do not expose a route before it has determinism, bounds, failure, and registered-channel tests.
4. Do not allow a temporary fallback to violate the non-distortion or channel-registration contracts.
5. Do not optimize full-resolution rendering before the low-resolution algorithm route is quantitatively correct.
6. Treat diagnostics and recovery choices as part of each algorithm, not later polish.
7. Update the stage traceability table and phase report whenever scope or ownership changes.
8. Delete replaced paths at the phase that establishes the new authority; avoid indefinite dual engines.
9. Stop a phase when its focused gate exposes a contract flaw and fix the contract before widening tests.
10. Declare V1 complete only after Phase 9 passes the full revised acceptance matrix.

## 11. Recommended first implementation slice

Begin with Phase 0 and the following narrow vertical proof:

```text
registered Base Color + optional registered Normal/Roughness
-> canonical typed decode
-> fixed two-slot template
-> manually constructed legal SamplingPlan
-> empty EffectPlan
-> direct physical sampling
-> vector-correct channel output
-> exact hotspot Region ID
-> deterministic plan/report serialization
```

This slice proves the new data flow and deletes the assumption that a `RegionMapping` is both user intent and the final algorithmic answer. It deliberately does not implement automatic placement until canonical inputs, fixed topology, plans, channel semantics, and exact output ownership are correct.
