# Hot Trimmer V1 full algorithm stack: Codex prompt pack

## Purpose

This pack implements the complete twenty-stage architecture in
`docs/hot-trimmer-v1-full-algorithm-stack-revised.md` according to
`docs/hot-trimmer-v1-full-algorithm-stack-implementation-plan.md`.

Run one prompt per Codex task, in the order listed here. Do not combine prompts. A prompt is complete only when
its stage is implemented, connected to the authoritative pipeline, and its focused verification passes.

This is a quality-first clean engine replacement:

- Preserve the useful source-first desktop workbench and authoring workflow.
- Do not preserve old project/schema compatibility, the legacy sheet compiler, runtime semantic repacking, or
  old renderer behavior.
- Do not create dual authorities, adapters that become permanent, or silent legacy fallbacks.
- Delete a replaced path in the same prompt that establishes its replacement as authoritative.
- Never hard-code behavior for brick, wood, metal, a fixture filename, or one supplied screenshot.

## What “all materials” means

The compiler routes sources by measured behavior and explicit user overrides:

| Behavior class | Representative materials |
| --- | --- |
| Already tileable | Authored seamless PBR sets |
| Stochastic isotropic | Concrete, plaster, rubber, dirt, ground |
| Stochastic directional | Brushed metal, directional fabric, wood grain |
| Periodic/lattice structured | Brick, tile, grating, patterned rubber |
| Layered/banded | Planks, siding, corrugated sheet, sedimentary stone |
| Organic directional | Wood, fibers, bark |
| Manufactured pattern | Panels, tread plate, vents, repeating borders |
| Unique detail | Container doors, labels, cracks, access panels |
| Radial detail | End grain, washers, drains, circular fixtures |
| Mixed/unknown | Composite or ambiguous sources |

Classification is not permission to invent facts. Base-Color-only PBR results remain Estimated. Missing semantic
content is synthesized through an explicit route or reported as insufficient; it is never hidden by stretching.

## Universal material corpus

Prompt 00 establishes a deterministic corpus and manifest. Every later prompt reuses it. The corpus covers:

- Fine stochastic concrete/plaster.
- Wood face grain and separate end grain.
- Wood planks with seams.
- Brushed, corrugated, and painted metal.
- Brick/tile lattice.
- Manufactured rubber pattern.
- Smooth plastic/coating.
- Dirt/ground.
- Storage-container/manufactured panel.
- Unique vent/panel detail.
- Radial washer/drain.
- Mixed/unknown material.
- Registered PBR and Base-Color-only variants.
- Clean planar, perspective, unevenly lit, clipped, low-resolution, transparent, salient-logo, and deliberately
  misregistered source conditions.

The slot matrix includes broad and square panels, horizontal and vertical strips, unique details, trim caps, and
radial slots. Representative pairwise goldens prevent a combinatorial explosion; property tests enforce universal
invariants across every generated case.

## Universal invariants

Every prompt preserves these invariants:

1. Standard-template topology is fixed and independent of material, output resolution, effects, and seed.
2. Non-uniform scaling is forbidden except through a visible `ExplicitStretch` override.
3. Registered channels share one source transform, seam, and correspondence field.
4. Base Color is color-managed; scalar maps remain linear; normals are vector data; IDs are categorical.
5. Directionality, lattice periods, bands, and unique salient features are preserved according to material class.
6. Strip mappings preserve cross-axis thickness; radial mappings preserve circular proportion.
7. Physical feature scale is stable across slot aspect ratios and output resolutions.
8. Fixed inputs, versions, settings, template, output, and seed produce deterministic plans and output.
9. Unsupported or insufficient input produces a typed diagnostic and recovery choices, not plausible fake output.
10. Preview, final render, QA views, export, and Blender consume one `CompiledSheet` lineage.

## Required result state for every stage

Every stage records one of:

```text
Executed { algorithm_id, version, settings_hash, diagnostics }
PassThrough { reason }
SkippedBecauseUnused { reason }
FailedWithRecovery { reason, recovery_choices }
```

Pass-through and unused are legal only where the revised stack allows them. They do not count as implementations
of missing algorithms. Any route not yet implemented must fail explicitly; it must not call Stretch, the legacy
renderer, or an unrelated synthesis engine.

## Rules for every prompt

- Read `AGENTS.md`, both governing algorithm documents, this pack’s common rules, and current `git status` before
  editing. Preserve unrelated user changes.
- Work in the root task without subagents.
- Keep the prompt’s file/subsystem ownership concrete. Do not broaden into later stages.
- Persist user intent and small plan/report artifacts. Store large analysis fields and intermediate imagery in a
  content-addressed cache keyed by inputs, settings, algorithm versions, output, and seed.
- Use typed contracts rather than generic JSON recipes or stringly typed algorithm names.
- Bound dimensions, allocations, candidates, graph sizes, iterations, supersampling, operation counts, and cache
  writes. Observe cancellation between bounded work units.
- Make deterministic iteration, tie-breaking, reductions, and pseudo-random streams part of the tests.
- Add success, pass-through, insufficiency, cancellation, and malformed-input evidence.
- Add no enabled UI control unless it invokes a typed native command and changes an authoritative artifact.
- Run exactly the one verification command named by the prompt. If it fails, make at most one focused correction
  and rerun the same command. Do not substitute a broad workspace sweep.
- Write or update the named stage report with delivered routes, evidence, measurements, and remaining later-stage
  work. Stop when the prompt’s gate is green.

## Execution order and traceability

| Order | Prompt | Stage |
| --- | --- | --- |
| 00 | Corpus, harness, and engine cutover contracts | Cross-cutting setup |
| 01-07 | One prompt per source-preparation/analysis stage | 1-7 |
| 08A-08F | Domain engines and router | 8 |
| 09-16 | One prompt per topology, placement, synthesis, profile, and detail stage | 9-16 |
| 18 | Effect compilation | 18 |
| 17 | PBR composition from compiled effects | 17 |
| 19 | Finishing and metadata | 19 |
| 20 | Preview, QA, export, and Blender | 20 |
| 21 | Full qualification and old-engine deletion | Final V1 gate |

Stage 18 precedes Stage 17 in implementation because Stage 17 consumes `CompiledEffect` operations produced by
Stage 18. The product-stage numbers remain unchanged.

---

## Prompt 00 — Universal corpus, traceability, and clean engine skeleton

```text
Implement Prompt 00 from docs/hot-trimmer-v1-full-algorithm-stack-prompt-pack.md.

Read AGENTS.md, the revised algorithm specification, the implementation plan, and the prompt-pack common rules.
Inspect the current document/compiler/render/store/IPC/UI paths and current git status. No subagents.

Objective:
Establish the material-agnostic acceptance harness and the new single-authority engine skeleton before implementing
individual algorithms. This prompt must not optimize for the current brick screenshot or any named material.

Scope:
- Add a machine-readable material-corpus manifest covering every behavior class, source condition, registered-map
  combination, and semantic slot role listed in this pack.
- Add deterministic synthetic fixture generators for structure/orientation/periodicity/saliency/registration
  assertions. Keep fixture provenance and expected behavioral properties explicit.
- Add the stage/route traceability matrix mapping every Stage 1-20 requirement and section-30 acceptance invariant
  to its owning prompt and future focused test.
- Introduce typed StageResult, algorithm/version provenance, cache key, PreparedSources placeholder,
  PlacementPlan/SamplingPlan header, EffectPlan header, CompilationReport, and the sole new compiler facade.
- Scaffold and register the new `material-analysis`, `material-synthesis`, `placement-solver`, and
  `effect-compiler` workspace crates with dependency directions matching the implementation plan. Their algorithms
  remain explicitly unsupported until their owning prompts.
- Establish content-addressed cache interfaces, cancellation, resource limits, seed policy, stable tie-breaking, and
  deterministic report serialization. Do not implement stage algorithms yet.
- Reset persistence/IPC contracts as needed for the new engine. Do not write legacy migrations or compatibility
  adapters. Preserve the useful workbench shell but make unsupported engine actions explicitly unavailable.
- Prevent the existing sheet compiler from claiming success through the new facade. Until a later prompt installs a
  route, return a typed UnsupportedStage diagnostic.
- Add docs/phase-reports/algorithm-stage-00.md.

Acceptance:
- The corpus describes all behavior classes and slot roles without branching on filenames or product labels.
- Every one of the twenty stages has a traceability owner and planned evidence.
- Identical request headers produce byte-identical cache keys and report headers.
- Cancellation or revision supersession cannot publish a partial artifact.
- The new facade has one authority and no silent call into the old compiler.

Verification — run exactly:
cargo test -p hot-trimmer-domain algorithm_stack_contract

Stop conditions:
- Stop if the corpus is brick-specific or relies only on visual snapshots.
- Stop if a placeholder returns plausible pixels instead of UnsupportedStage.
- Stop if old and new compiler facades can both publish authoritative output.
```

---

## Prompt 01 — Stage 1: input ingestion and channel registration

```text
Implement Prompt 01 / Stage 1 from the full algorithm-stack prompt pack.

Read the common rules and revised Stage 1. Start from Prompt 00’s contracts. No subagents.

Objective:
Create immutable, registered Material Sources that accept photographs, scans, flat textures, existing PBR sets,
multiple exemplars, and Base-Color-only inputs without embedding assumptions about one material family.

Scope:
- Replace the source document contract with RegisteredChannelSet and MaterialSource intent covering Base Color,
  Normal, Height, Roughness, Metallic, AO, Specular, Opacity, Edge Mask, and Material ID.
- Keep original bytes/digest/path provenance separate from owned storage. Base Color anchors oriented dimensions.
- Require companion maps to match oriented Base Color dimensions and orientation; reject mismatches with typed
  diagnostics and recovery choices.
- Record channel interpretation, normal convention, source ownership, confidence/provenance, exemplar grouping,
  and immutable digests. Do not decode computation buffers in domain types.
- Replace persistence and typed IPC projections for this new source contract; no legacy migration.
- Ensure import, replace, remove, and multi-file assignment update authoritative source revision/cache invalidation.
- Wire existing source-library UI to the new typed records without creating algorithm truth in React.
- Add docs/phase-reports/algorithm-stage-01.md.

Generalization contract:
The same registration path handles stochastic, directional, periodic, manufactured, unique, radial, and unknown
materials. Filename tokens may assist channel assignment only; they never select a material algorithm.

Acceptance:
- Every corpus source shape registers or fails for an explicit dimension/role reason.
- Original bytes remain immutable and checksummed.
- Companion maps cannot silently drift, resize, rotate independently, or receive Base Color interpretation.
- Base-Color-only and full-PBR sources are both valid and carry honest provenance.

Verification — run exactly:
cargo test -p hot-trimmer-project-store algorithm_stage_01_registration

Stop conditions:
- Stop if import resizes a companion map to make it fit.
- Stop if material routing depends on filenames.
- Stop if source replacement can reuse stale derived cache entries.
```

---

## Prompt 02 — Stage 2: color, alpha, data, normal, and pyramid normalization

```text
Implement Prompt 02 / Stage 2 from the full algorithm-stack prompt pack.

Read the common rules and revised Stage 2. Use Stage 1 records as the only input authority. No subagents.

Objective:
Decode every registered channel into canonical typed computation buffers and registered resolution pyramids.

Scope:
- Introduce bounded tiled ImagePlane types for linear color, scalar data, normal vectors, categorical IDs, and masks.
- Decode Base Color ICC profiles into the selected linear working color space and retain an sRGB display
  representation. Resolve premultiplied-alpha ambiguity and report clipped/crushed ranges.
- Decode Height/Roughness/Metallic/AO/Specular/masks as linear data and Material ID as categorical data.
- Decode tangent normals, record/convert OpenGL or DirectX convention, reject nearly-zero/invalid vectors,
  normalize valid vectors, and keep alpha policy explicit.
- Build registered Gaussian/mip pyramids for every channel using color-, scalar-, vector-, and ID-correct filters.
- Keep coordinates/bounds identical across channels and pyramid levels. Bound memory and observe cancellation per tile.
- Cache prepared channel sets by source digest, decode policy, working-space version, and pyramid version.
- Add docs/phase-reports/algorithm-stage-02.md.

Acceptance:
- Scalar inputs never receive display gamma and ID values never interpolate.
- Normal pyramids filter decoded vectors and renormalize; no RGB averaging path exists.
- Every channel remains registered at every level.
- The corpus’s malformed normal, alpha, ICC, clipping, and cancellation cases produce expected reports.

Verification — run exactly:
cargo test -p hot-trimmer-image-io algorithm_stage_02_normalization

Stop conditions:
- Stop if authoritative computation remains generic RGBA8.
- Stop if normal maps or IDs use ordinary color filtering.
- Stop if pyramid generation can exceed declared memory bounds without failure.
```

---

## Prompt 03 — Stage 3: registered geometry and perspective correction

```text
Implement Prompt 03 / Stage 3 from the full algorithm-stack prompt pack.

Read the common rules and revised Stage 3. Reuse the existing geometry primitives only after adapting them to the
new prepared-source contracts. No subagents.

Objective:
Produce one or more planar PreparedExemplars from arbitrary source captures while preserving cross-channel
registration and a valid pass-through route for already-planar textures.

Scope:
- Implement four-point homography, outline-assisted best-fit quadrilateral, optional bounded lens correction,
  optional crop/alpha mask, and full-frame usable-planar-area selection.
- Apply one rectification coordinate field to every registered channel and use channel-correct sampling.
- Produce rectified dimensions from physical/source geometry within declared limits; retain masks and perspective
  confidence. Do not bake display-only pixels into authoritative exemplars.
- Implement explicit PassThrough for already-planar sources and record why rectification was unnecessary.
- Make patch/source editing invalidate only affected prepared exemplars and downstream cache keys.
- Wire the existing four-point/outline UI to typed commands and the shared stage result.
- Add docs/phase-reports/algorithm-stage-03.md.

Acceptance:
- Synthetic grid and multi-channel fixtures remain registered after skew, lens correction, and masking.
- Planar pass-through is byte-stable and does not resample unnecessarily.
- Singular, concave, crossed, tiny, or excessive operations fail with recoveries.
- Preview and authoritative rectification share geometry and differ only by resolution/work limits.

Verification — run exactly:
cargo test -p hot-trimmer-render-core algorithm_stage_03_rectification

Stop conditions:
- Stop if companion maps compute separate homographies.
- Stop if invalid geometry publishes a partially rectified exemplar.
- Stop if planar textures are degraded by mandatory resampling.
```

---

## Prompt 04 — Stage 4: de-lighting and exposure normalization

```text
Implement Prompt 04 / Stage 4 from the full algorithm-stack prompt pack.

Read the common rules and revised Stage 4. Consume only Stage 3 PreparedExemplars. No subagents.

Objective:
Provide de-lighting as an explicit, reversible source-preparation capability for photographs while making exact
PassThrough the default Stage 4 route. Stage 4 must always produce its typed downstream contract, but it must not
modify Base Color unless the user explicitly selects a de-lighting route. Preserve already de-lit sources and
uncertainty.

Scope:
- Define persisted typed route intent: PassThrough, ClassicalLowFrequency, or LocalIntrinsicProvider. New and
  unclassified sources default to PassThrough. Existing authored textures/PBR sets also use documented PassThrough.
  Do not infer permission to de-light from filenames, Base-Color-only registration, or missing companion maps.
- Implement a deterministic classical low-frequency route in linear/log luminance with bounded edge-preserving
  filtering, epsilon protection, color preservation, strength, shadow/highlight recovery, and radius in physical or
  scale-aware units. Execute it only when ClassicalLowFrequency is explicitly selected.
- Produce highlight, shadow, clipping, and confidence masks for later usability/scoring stages when analysis is
  requested. PassThrough forwards existing Stage 2/3 diagnostics and coverage without inventing inferred lighting.
- Implement byte-stable typed PassThrough for the default, authored textures/PBR sets, and user-disabled
  de-lighting. Record the specific reason.
- Define a versioned local intrinsic-image provider interface and deterministic result contract. Until a model is
  installed, keep the route unavailable. If the user explicitly selects it, return explicit unavailable and use only
  the fallback they explicitly chose; do not fake ML or silently activate the classical route.
- Record Estimated provenance for all inferred reflectance and retain original prepared Base Color.
- Add bounded opt-in preview controls and authoritative commands. The control is Off/PassThrough by default, makes
  the selected route and strength visible, and can restore the original without recomputation ambiguity. Never apply
  de-lighting to scalar/normal/ID maps.
- Add docs/phase-reports/algorithm-stage-04.md.

Acceptance:
- A new source with no Stage 4 override takes byte-stable PassThrough and performs no de-lighting work.
- Uneven-light fixtures reduce low-frequency illumination without erasing structural edges beyond tolerance.
- Clean authored textures take a documented pass-through route.
- Disabling an enabled route restores the original prepared Base Color and records user-disabled PassThrough.
- Dark pigment and cast-shadow ambiguity remains represented by confidence/masks.
- Same input/settings/version produce deterministic output and diagnostics.

Verification — run exactly:
cargo test -p hot-trimmer-material-analysis algorithm_stage_04_delighting

Stop conditions:
- Stop if every Base Color is automatically de-lit.
- Stop if de-lighting is activated from filenames, Base-Color-only status, missing maps, or an unconfirmed heuristic.
- Stop if scalar or ID channels are altered.
- Stop if unavailable learned inference is reported as executed.
- Stop if unavailable learned inference silently enables a fallback the user did not select.
```

---

## Prompt 05 — Stage 5: source quality and material-behavior analysis

```text
Implement Prompt 05 / Stage 5 from the full algorithm-stack prompt pack.

Read the common rules and revised Stage 5. Consume Stage 4 results and the universal corpus. No subagents.

Objective:
Measure source fitness and classify material behavior from image evidence plus explicit user overrides, never from
fixture filenames or hard-coded product labels.

Scope:
- Implement sharpness, noise, compression, dynamic-range, clipping, perspective-confidence, usable-area,
  registration-quality, and estimated-material-resolution measurements with documented units/ranges.
- Implement deterministic heuristic evidence for all revised behavior classes. Store a ranked class distribution,
  confidence, supporting measurements, and Mixed/Unknown fallback rather than forcing certainty.
- Provide typed user override and reset-to-analysis commands. An override changes routing intent without rewriting
  measured evidence.
- Add source-quality thresholds and explicit warnings/recoveries; do not reject merely imperfect sources when a
  lower-fidelity route is valid.
- Define a local classifier-provider interface with versioned deterministic output; heuristics remain authoritative
  fallback when no model exists.
- Expose concise quality/classification evidence in the source inspector.
- Add docs/phase-reports/algorithm-stage-05.md.

Acceptance:
- Corpus classes are separated by measurable behavior with declared confusion/tolerance, not filename checks.
- Ambiguous mixed sources remain Mixed/Unknown with usable evidence and override.
- Deliberately misregistered PBR inputs are detected.
- Classification and quality reports are deterministic, bounded, and cacheable.

Verification — run exactly:
cargo test -p hot-trimmer-material-analysis algorithm_stage_05_quality_classification

Stop conditions:
- Stop if tests pass by inspecting fixture IDs or filenames.
- Stop if a low-confidence class silently becomes a hard routing fact.
- Stop if user override deletes measured evidence.
```

---

## Prompt 06 — Stage 6: physical scale and orientation calibration

```text
Implement Prompt 06 / Stage 6 from the full algorithm-stack prompt pack.

Read the common rules and revised Stage 6. Consume Stage 5 reports. No subagents.

Objective:
Establish honest physical/relative material scale and global/local orientation fields for arbitrary material
behavior classes.

Scope:
- Implement source pixels-per-meter X/Y with Imported, UserMeasured, MotifDerived, Convention, PriorEstimated, and
  RelativeOnly provenance/confidence.
- Add two-point user measurement, known motif size, imported metadata, and explicit reset/override commands.
- Never claim world accuracy when only relative scale or a class prior exists.
- Implement Scharr gradients and multi-scale structure tensors, eigenvector orientation, anisotropy, global dominant
  direction, local direction, and confidence. Treat 180-degree equivalent axes correctly.
- Preserve isotropic scans and detect/report inconsistent anisotropic scale.
- Expose measurement and orientation overlays without making display coordinates authoritative.
- Add docs/phase-reports/algorithm-stage-06.md.

Acceptance:
- Directional corpus fixtures recover orientation within declared angular tolerance.
- Isotropic/noisy fixtures report low orientation confidence rather than arbitrary direction.
- Physical measurements round-trip and invalidate downstream footprints.
- Relative-only sources compile but all world-scale claims remain explicitly unavailable.

Verification — run exactly:
cargo test -p hot-trimmer-material-analysis algorithm_stage_06_scale_orientation

Stop conditions:
- Stop if unknown physical scale defaults to a claimed real-world number.
- Stop if low-confidence orientation permits an undocumented destructive rotation.
- Stop if image aspect is confused with physical anisotropy.
```

---

## Prompt 07 — Stage 7: feature-field extraction

```text
Implement Prompt 07 / Stage 7 from the full algorithm-stack prompt pack.

Read the common rules and revised Stage 7. Consume Stage 6 prepared analyses and use registered pyramids. No subagents.

Objective:
Produce the material-agnostic fields required for crop selection, structured alignment, synthesis, and explicit
source insufficiency.

Scope:
- Implement multi-scale saliency for distinctive stains/cracks/text/strong marks without hard-coded material names.
- Implement structure maps for edges, lines, boundaries, grids, fibers, and intersections.
- Implement local stationarity from color/gradient/frequency/texture descriptors and registered-map variance.
- Implement periodicity and lattice candidates using bounded autocorrelation/FFT evidence with confidence.
- Implement multi-channel seamability comparing color, gradient, Height, vector normals, Roughness, and structural
  crossings where those maps exist.
- Combine transparency, clipping, highlight/shadow confidence, occluders/logos, and registration validity into a
  usability mask with inspectable reasons.
- Keep fields registered, pyramid-aware, deterministic, cancellable, bounded, and content-addressed.
- Add QA/debug views and docs/phase-reports/algorithm-stage-07.md.

Acceptance:
- Periodic/lattice, directional, stochastic, salient-unique, and unusable corpus cases produce the expected
  behavioral evidence within documented tolerances.
- Missing PBR maps change available seam terms but never change coordinates.
- Generic slots can later penalize saliency while unique slots can reward it from the same field.
- Fields contain no filename/material-specific branches.

Verification — run exactly:
cargo test -p hot-trimmer-material-analysis algorithm_stage_07_feature_fields

Stop conditions:
- Stop if periodicity is inferred only from material classification.
- Stop if usability destroys uncertainty into an unexplained binary mask.
- Stop if fields for registered maps can drift.
```

---

## Prompt 08A — Stage 8A: material-domain interface, direct domain, and graph-cut closure

```text
Implement Prompt 08A / the first Stage 8 slice from the full algorithm-stack prompt pack.

Read the common rules and revised Stage 8. Consume Stage 7 analysis only through typed contracts. No subagents.

Objective:
Establish one registered material-domain interface and implement the direct and graph-cut periodic-closure routes.

Scope:
- Create PreparedMaterialDomain, DomainRequest, DomainRoute, shared CorrespondenceField/OperationField,
  DomainDiagnostics, and registered channel access without duplicating source-preparation state.
- Implement DirectSourceDomain for already clean/tileable exemplars, including explicit pass-through evidence.
- Implement bounded deterministic multi-channel graph-cut periodic closure. Cost terms include available linear color,
  gradients, Height, vector normals, Roughness, and structure-cut penalties with normalized weights.
- Use one selected seam for every channel. Blend continuous channels correctly, renormalize normals, select IDs
  categorically, and retain validity/provenance masks.
- Support X, Y, and XY closure with explicit overlap/search limits, stable tie-breaking, cancellation, and insufficient
  overlap diagnostics.
- Cache domains by prepared-source/analysis digests, route settings, algorithm version, and seed.
- Add domain/seam QA views and docs/phase-reports/algorithm-stage-08a.md.

Generalization contract:
Direct and graph-cut routes are selected from tileability/seam evidence and user policy, never material names.

Acceptance:
- Existing seamless inputs take Direct without resampling.
- Seamable stochastic/directional fixtures close below declared multi-channel boundary thresholds.
- Structured edges are not cut when a lower-cost legal seam exists.
- All channels use one seam and remain registered; IDs never blend.

Verification — run exactly:
cargo test -p hot-trimmer-material-synthesis algorithm_stage_08a_graphcut

Stop conditions:
- Stop if each channel solves its own seam.
- Stop if graph-cut failure silently tiles a visible boundary.
- Stop if normal or ID composition uses ordinary RGBA blending.
```

---

## Prompt 08B — Stage 8B: deterministic texture quilting

```text
Implement Prompt 08B / the quilting slice of Stage 8.

Read the common rules and revised Stage 8.4. Use Prompt 08A’s domain interface. No subagents.

Objective:
Synthesize larger registered material domains from source patches without obvious grid repetition.

Scope:
- Implement bounded multi-resolution image quilting with physical/relative patch sizes, registered overlap costs,
  histogram/structure error, duplicate-use penalty, and boundary-periodicity error.
- Select deterministic near-best candidates from a seeded stable stream; do not always choose one identical best patch.
- Cut every overlap through one shared minimum-cost seam and apply it to all channels using correct channel semantics.
- Track source-patch usage, duplication, seam energy, correspondence, confidence, and failure reasons.
- Respect orientation fields and lattice/band constraints when requested; reject semantically incompatible quilting.
- Add output-size, patch-count, candidate-count, overlap, memory, iteration, and cancellation limits.
- Add quilting/source-usage QA views and docs/phase-reports/algorithm-stage-08b.md.

Acceptance:
- Stochastic isotropic and directional corpus cases expand without registered-map drift or obvious fixed-grid repeats.
- Directional structure remains within angular tolerance.
- Duplicate-use penalties measurably diversify source patches for a fixed seed.
- Unique text/logos and unsuitable lattice patterns fail or route away rather than smear.

Verification — run exactly:
cargo test -p hot-trimmer-material-synthesis algorithm_stage_08b_quilting

Stop conditions:
- Stop if output is a regular tile grid marketed as quilting.
- Stop if overlap seams differ by channel.
- Stop if the algorithm can select unusable-mask pixels without a diagnostic.
```

---

## Prompt 08C — Stage 8C: registered PatchMatch synthesis

```text
Implement Prompt 08C / the PatchMatch slice of Stage 8.

Read the common rules and revised Stage 8.4. Use the established domain/correspondence contracts. No subagents.

Objective:
Implement deterministic constrained completion and expansion through a registered nearest-neighbor field.

Scope:
- Implement a bounded multi-resolution PatchMatch nearest-neighbor field with deterministic initialization,
  propagation, random search, stable tie-breaking, and fixed seed/version behavior.
- Define multi-channel patch distance over available canonical fields with normalized weights, usability rejection,
  orientation/structure constraints, coherence, and duplicate/saliency penalties.
- Use one NNF/correspondence field to reconstruct every channel; filter normals as vectors and IDs categorically.
- Support constrained completion masks and seamless expansion targets. Record confidence, source usage, convergence,
  operation counts, and incomplete/insufficient failures.
- Enforce image/patch/search/iteration/memory bounds and cancellation between passes/tiles.
- Add NNF/coherence/source-usage QA views and docs/phase-reports/algorithm-stage-08c.md.

Acceptance:
- Registered stochastic and directional fixtures expand coherently without cross-channel drift.
- Masked completion never samples excluded source regions.
- Same request is byte-deterministic, including NNF and diagnostics.
- Manufactured unique content is preserved only under explicit compatible constraints, not unintentionally repeated.

Verification — run exactly:
cargo test -p hot-trimmer-material-synthesis algorithm_stage_08c_patchmatch

Stop conditions:
- Stop if random search uses nondeterministic system randomness.
- Stop if channels reconstruct from independent nearest-neighbor fields.
- Stop if nonconvergence publishes output without a declared degraded/failure state.
```

---

## Prompt 08D — Stage 8D: statistical and spectral synthesis

```text
Implement Prompt 08D / the statistical-spectral slice of Stage 8.

Read the common rules and revised Stage 8.5. Use the established domain interface. No subagents.

Objective:
Generate seamless domains for genuinely stochastic materials by matching multi-scale frequency/statistical behavior.

Scope:
- Implement deterministic bounded spectral and/or steerable-wavelet-statistics synthesis for eligible stationary
  sources, including color covariance, frequency energy, histogram/range, and optional registered scalar coupling.
- Define strict applicability from stationarity, saliency, periodicity, and structure evidence. Reject text, semantic
  patterns, unique cracks, bricks/tiles, and strong manufactured structures.
- Preserve physical/relative frequency scale, orientation anisotropy where supported, and documented provenance.
- Produce or derive one deterministic correspondence/operation record for registered non-Base-Color maps; if a map
  cannot be synthesized consistently, return explicit per-route insufficiency rather than drifting it.
- Bound transform dimensions, iterations, memory, convergence, and cancellation.
- Add spectral/statistics QA views and docs/phase-reports/algorithm-stage-08d.md.

Acceptance:
- Fine concrete/plaster/rust/ground fixtures match declared spectrum/statistics tolerances without visible seams.
- Structured and high-saliency fixtures are rejected by applicability tests.
- Frequency scale remains stable across requested domain sizes.
- Results and diagnostics are deterministic for fixed seed/version.

Verification — run exactly:
cargo test -p hot-trimmer-material-synthesis algorithm_stage_08d_spectral

Stop conditions:
- Stop if spectral synthesis is a universal fallback.
- Stop if registered channels are independently randomized.
- Stop if output frequency changes solely because output pixel dimensions changed.
```

---

## Prompt 08E — Stage 8E: procedural material reconstruction

```text
Implement Prompt 08E / the procedural-reconstruction slice of Stage 8.

Read the common rules and revised Stage 8.6. Use measured fields and explicit user overrides. No subagents.

Objective:
Fit deterministic procedural models for materials whose structure benefits from arbitrary-size, orientation-aware
generation without stretching.

Scope:
- Define a typed ProceduralDomainModel interface with fitted parameters, confidence, physical/relative units,
  seed/version, channel outputs, diagnostics, and explicit unsupported states.
- Implement bounded initial V1 models for wood face grain, wood end grain, brick/tile lattice, corrugation, brushed
  metal, concrete aggregate, and painted-metal layers.
- Fit colors, periods, scales, orientation, distributions, layer masks, and noise from Stage 5-7 evidence; permit
  typed user correction without discarding measured evidence.
- Generate registered Base Color and supported PBR/structural contributions in one model coordinate system.
- Distinguish material reconstruction from semantic details: a vent, label, or container door is not generic material
  noise and must remain a detail/unique-content concern.
- Bound model fitting/generation and provide fallback choices when confidence is insufficient.
- Add model-parameter QA views and docs/phase-reports/algorithm-stage-08e.md.

Acceptance:
- Each model passes its behavior-class fixtures across horizontal, vertical, square, and radial-compatible domains.
- Physical/relative motif scale and orientation remain stable across output dimensions.
- Face-grain input does not silently claim authentic end grain; it requires its fitted estimated model or source.
- Router-facing model outputs are deterministic and registered.

Verification — run exactly:
cargo test -p hot-trimmer-material-synthesis algorithm_stage_08e_procedural

Stop conditions:
- Stop if material selection is driven by filenames instead of measured evidence/override.
- Stop if one generic noise shader is presented as all required procedural models.
- Stop if procedural semantic details bypass later detail contracts.
```

---

## Prompt 08F — Stage 8F: learned-route adapter and complete material-domain router

```text
Implement Prompt 08F / the final Stage 8 integration slice.

Read the common rules and all revised Stage 8 routes. Integrate Prompts 08A-08E. No subagents.

Objective:
Complete the material-domain router, including an honest local learned-provider boundary, deterministic route
comparison, and explicit classical/procedural fallbacks.

Scope:
- Define a local learned provider for de-lit material fields, seamless expansion, super-resolution, and estimated
  maps with model digest/version, device policy, deterministic mode, bounds, provenance, and confidence.
- Do not bundle or fake an unapproved model. Provider absence is a normal typed state and routes to the documented
  classical/procedural alternative.
- Implement the full material/source-class routing table from revised section 26.1 using measured evidence,
  applicability, user policy, and bounded low-resolution route previews.
- Normalize route-quality costs, apply stable tie-breaking, record compared routes/rejections, and expose user
  override/pin/reset without changing topology.
- Require every chosen domain to validate registration, seam/period expectations, physical/relative scale,
  correspondence, determinism, and cache provenance.
- Produce the authoritative Stage 8 result and domain diagnostics views.
- Add docs/phase-reports/algorithm-stage-08.md summarizing all Stage 8 routes.

Acceptance:
- Every corpus behavior class selects an applicable route or returns actionable insufficiency.
- Removing a learned provider yields a deterministic documented fallback, not a different hidden contract.
- Route selection never depends on fixture identity or filename.
- All Stage 8 engines are reachable only through the one domain interface and router.

Verification — run exactly:
cargo test -p hot-trimmer-material-synthesis algorithm_stage_08_router

Stop conditions:
- Stop if learned inference is required for baseline correctness.
- Stop if the router chooses the visually plausible but semantically inapplicable route.
- Stop if a route can bypass registered-channel validation.
```

---

## Prompt 09 — Stage 9: fixed semantic template topology

```text
Implement Prompt 09 / Stage 9 from the full algorithm-stack prompt pack.

Read the common rules and revised Stage 9. No subagents.

Objective:
Make fixed, versioned semantic templates the immutable topology authority while supporting multiple asset-family
vocabularies for geometry authored later in Blender.

Scope:
- Replace generated standard-template packing with authored 4096 x 4096 integer allocation/hotspot rectangles,
  stable order, roles, fit semantics, world sizes, profile intent, material/variation groups, and radial metadata.
- Compile shared canonical boundaries once per output resolution; implement exact largest-remainder allocation only
  for authored weighted grammar compilation, never per-material runtime packing.
- Ship/validate initial fixed families for generic architecture, horizontal/vertical trim banks, hard-surface/panel
  work, detail-heavy props, and radial accents using the same template grammar/registry.
- Standard template material/effect/resolution changes cannot alter rectangles, IDs, normalized hotspots, or topology
  hash. Custom template authoring must pin accepted integer rectangles before material compilation.
- Remove semantic_pack_template, SetLayoutGrid topology mutation for standard templates, and equivalent UI authority.
- Keep topology selection explicit in the workbench and manifest.
- Add docs/phase-reports/algorithm-stage-09.md.

Acceptance:
- Every template has nonoverlapping, in-bounds, unique, stable regions and exact shared scaled boundaries.
- The same template works with multiple corpus materials without moving slots.
- Template-family changes are explicit topology changes with compatibility diagnostics.
- Runtime source analysis and seed cannot affect standard geometry.

Verification — run exactly:
cargo test -p hot-trimmer-domain algorithm_stage_09_fixed_topology

Stop conditions:
- Stop if filling unused space causes slot geometry to change.
- Stop if a Detail/Unique role is arbitrarily reshaped by material input.
- Stop if material appearance enters the topology hash.
```

---

## Prompt 10 — Stage 10: resolved slot demand and effect capacity

```text
Implement Prompt 10 / Stage 10 from the full algorithm-stack prompt pack.

Read the common rules and revised Stage 10. Consume Stage 9 topology plus source/output intent. No subagents.

Objective:
Resolve every slot’s material footprint, physical coordinates, raster density, legal mapping vocabulary, and effect
capacity before crop selection or effect rendering.

Scope:
- Implement every ResolvedSlotDemand field from the revised design, including role/orientation/aspect, output pixels,
  world dimensions, major/minor axes, pixels/meters, mapping permissions, groups, importance, feature survival,
  profile/weathering intent, and supersampling needs.
- Derive required source footprint from physical scale when known and an honest relative footprint otherwise.
- Implement EffectScaleSpace and EffectCapacity, opposing-profile/flat-center constraints, maximum isotropic/radial
  features, LOD thresholds, allowed variants, and 1x/2x/4x/8x recommendation.
- Express procedural coordinates in slot-local meters/relative units; forbid normalized coordinates for physical
  effects. Supersampling cannot legalize a physically impossible effect.
- Make capacity and insufficiency diagnostics inspectable in UI/QA without allowing UI recomputation.
- Add docs/phase-reports/algorithm-stage-10.md.

Acceptance:
- Required broad-panel, extreme horizontal/vertical strip, radial, cap, sub-pixel, and opposing-bevel fixtures match
  the revised capacity equations.
- Isotropic features remain isotropic despite destination aspect.
- Unknown world scale produces relative results with honest provenance.
- Resolution changes pixel capacity/LOD but not physical intent or topology.

Verification — run exactly:
cargo test -p hot-trimmer-effect-compiler algorithm_stage_10_slot_capacity

Stop conditions:
- Stop if normalized rectangle size is treated as physical size.
- Stop if narrow strips accept impossible opposing profiles.
- Stop if supersampling changes declared physical feature dimensions.
```

---

## Prompt 11 — Stage 11: crop and synthesis candidate generation

```text
Implement Prompt 11 / Stage 11 from the full algorithm-stack prompt pack.

Read the common rules and revised Stage 11. Consume PreparedMaterialDomain and ResolvedSlotDemand. No subagents.

Objective:
Generate a complete bounded set of legal direct, repeat, unique, cap, radial, and synthesis candidates without
silently distorting arbitrary source materials.

Scope:
- Generate exact-aspect crop sizes at legal isotropic physical/relative scales, with configurable scale ladder and
  quality/upscale limits. Reject out-of-usable-area direct crops.
- Generate low-resolution dense/coarse-to-fine, feature-aware, saliency, stationary-zone, period-aligned, and
  farthest-point positions with stable ordering.
- Generate only permitted quarter-turn rotations/mirrors according to source orientation/class confidence, template
  permissions, and explicit override. Arbitrary rotations are not legal crop candidates.
- Implement the role-specific candidate families from revised Stage 11 for panel, Repeat X, Repeat Y, unique,
  three/nine-slice cap, planar radial, polar radial, and synthesis routes.
- Carry source/domain ID, exact crop, transform, isotropic scale, route, period/seam/correspondence references,
  descriptors, seed, and eligibility evidence.
- Bound candidate count/work before scoring and report insufficiency/recovery choices.
- Add candidate/source-footprint QA views and docs/phase-reports/algorithm-stage-11.md.

Acceptance:
- Candidate sets cover every legal mapping mode and contain no illegal non-uniform scale.
- Directional/periodic corpus cases obey orientation and lattice alignment.
- Horizontal/vertical strips preserve cross-axis footprint and receive distinct candidate semantics.
- Same request produces stable candidate identities/order.

Verification — run exactly:
cargo test -p hot-trimmer-placement-solver algorithm_stage_11_candidates

Stop conditions:
- Stop if output rectangle bounds are reused as source crop coordinates.
- Stop if 90-degree rotation is universally enabled.
- Stop if synthesis candidates conceal an inapplicable direct crop.
```

---

## Prompt 12 — Stage 12: candidate scoring

```text
Implement Prompt 12 / Stage 12 from the full algorithm-stack prompt pack.

Read the common rules and revised Stage 12. Consume only legal candidates from Stage 11. No subagents.

Objective:
Rank candidates through an inspectable material- and role-aware unary objective rather than arbitrary template crops.

Scope:
- Implement scale, resolution, stationarity, saliency, structure, orientation, seam, boundary-cut, quality, role,
  and synthesis-complexity costs from the revised equation.
- Normalize each term to documented ranges/confidence and define weights by behavior/slot role, not fixture identity.
- Generic repeating slots penalize salient uniqueness; compatible unique slots may reward controlled saliency.
- Structured materials reward lattice/period completion and penalize cut boundaries; directional materials compare
  source/slot direction with 180-degree equivalence.
- Prefer simpler direct routes only when quality is comparable; never make synthesis complexity outweigh legality.
- Produce full per-candidate cost breakdown, applicability rejections, stable top-K (initially 64), and QA views.
- Add docs/phase-reports/algorithm-stage-12.md.

Acceptance:
- Corpus rankings select stationary low-saliency candidates for generic surfaces, aligned candidates for structured
  materials, and controlled salient candidates for unique roles.
- Changing one weight produces explainable term-level ranking changes.
- Top-K ordering/ties are deterministic.
- Illegal candidates cannot re-enter through low cost.

Verification — run exactly:
cargo test -p hot-trimmer-placement-solver algorithm_stage_12_scoring

Stop conditions:
- Stop if one universal weighted score ignores role/material behavior.
- Stop if tests assert only the chosen index rather than the cost explanation.
- Stop if synthesis wins solely because it can reproduce any target.
```

---

## Prompt 13 — Stage 13: global placement optimization

```text
Implement Prompt 13 / Stage 13 from the full algorithm-stack prompt pack.

Read the common rules and revised Stage 13. Consume Stage 12 top-K candidates. No subagents.

Objective:
Select all slot placements jointly so a trim sheet remains diverse, role-correct, and deterministic.

Scope:
- Implement pairwise source-overlap, descriptor-similarity, repeated-salient-feature, identical-transform, and
  variation-group penalties with material-class-dependent reuse policy.
- Order slots by visual importance and constraint tightness; implement bounded deterministic beam search with
  configurable beam width and stable ties.
- Implement deterministic local replacement and pairwise-swap improvement after the beam completes.
- Preserve intentional periodic reuse, permit stochastic overlap under policy, and strongly ration unique salient
  features. Record every accepted/rejected pairwise decision and objective breakdown.
- Validate complete assignment, no non-uniform scale, registered mapping, required slots, and explicit insufficiency.
- Add crop reuse/repetition heatmap and PlacementPlan QA views.
- Add docs/phase-reports/algorithm-stage-13.md.

Acceptance:
- Distinctive marks do not populate multiple large visible slots unless explicitly permitted.
- Stochastic and manufactured-periodic reuse policies differ as documented.
- Fixed inputs/seed/versions yield byte-identical PlacementPlan and objective report.
- Bounded beam/local work observes cancellation and returns no partial authoritative plan.

Verification — run exactly:
cargo test -p hot-trimmer-placement-solver algorithm_stage_13_global_placement

Stop conditions:
- Stop if each slot independently chooses its cheapest candidate.
- Stop if diversity is simulated only with random offsets.
- Stop if a failed complete assignment silently drops required slots.
```

---

## Prompt 14 — Stage 14: registered per-slot material synthesis

```text
Implement Prompt 14 / Stage 14 from the full algorithm-stack prompt pack.

Read the common rules and revised Stage 14. Consume only validated SamplingPlans from Stage 13. No subagents.

Objective:
Execute every legal mapping/synthesis mode in slot-local physical coordinates while keeping all channels registered.

Scope:
- Implement DirectPhysical, PeriodicTile, RepeatX, RepeatY, TextureSynthesis, UniqueContain, UniqueCover,
  ThreeSliceCap, NineSlicePanel, PlanarRadial, PolarRadial, and ExplicitStretch execution.
- Map normalized slot coordinates into physical/relative coordinates before domain correspondence. Preserve strip
  thickness, cap widths, corners, repeat period, and contain/cover semantics.
- Sample every registered channel through one source/correspondence position with channel-correct interpolation,
  vector normal transform/filtering, categorical ID selection, and validity masks.
- Make ExplicitStretch a visible user override in SamplingPlan/diagnostics; never use it as fallback.
- Keep planar material planar within ordinary radial hotspots. Polar mapping requires the explicit PolarRadial plan.
- Evaluate into allocation/hotspot-local intermediate channels with cancellation/resource bounds.
- Add sampling/correspondence QA views and docs/phase-reports/algorithm-stage-14.md.

Acceptance:
- Property tests find no non-uniform scaling outside ExplicitStretch.
- Cross-channel markers remain aligned through every mapping mode, transform, repeat, cap, and radial route.
- Directional/periodic scale and cross-axis thickness remain stable.
- Circular details retain proportion; planar radial and polar radial produce intentionally different results.

Verification — run exactly:
cargo test -p hot-trimmer-sheet-compiler algorithm_stage_14_slot_synthesis

Stop conditions:
- Stop if SamplingPolicy is declared but ignored by the raster path.
- Stop if imported normals are sampled as color.
- Stop if a failed plan falls back to center-cover or Stretch.
```

---

## Prompt 15 — Stage 15: scale-constrained structural profile synthesis

```text
Implement Prompt 15 / Stage 15 from the full algorithm-stack prompt pack.

Read the common rules and revised Stage 15. Consume Stage 10 capacities and Stage 14 slot-local material. No subagents.

Objective:
Compile and render physically legal structural trim profiles instead of normalized rectangle effects.

Scope:
- Replace the current normalized profile API with requested and CompiledProfile plans expressed in physical/relative
  units, pixel scale, occupancy, LOD, supersampling, evaluator, and fallback reason.
- Implement rectangle/disc/annulus signed-distance fields and the complete required programs: Flat, convex bevel,
  concave groove, rounded bevel, double bevel, raised lip, recessed seam, panel frame, fully rounded strip, merged
  opposing bevel, radial disc, annulus, and bounded custom profile curve.
- Enforce opposing-profile/minimum-flat-center legality. Implement explicit clamp, fully rounded, merged, normal-only,
  disabled, and incompatibility outcomes selected by declared policy.
- Implement FullHeight, SimplifiedHeight, NormalOnly, RoughnessOnly, and Disabled profile LODs without changing
  physical profile dimensions or seed.
- Generate physical Height and analytic/compiled normal contributions; use analytic filtering or selected 1x/2x/4x/8x
  supersampling. Do not widen sub-pixel geometry in final pixels.
- Add profile occupancy/LOD/fallback QA views and docs/phase-reports/algorithm-stage-15.md.

Acceptance:
- Required broad panel, extreme strips, radial, cap, sub-pixel, and opposing-bevel fixtures pass at 1K/2K/4K/8K.
- Opposing profiles never overlap accidentally or leave negative center width.
- Physical slope/width remain stable across slot aspect and resolution.
- Every fallback is deterministic and present in EffectPlan diagnostics.

Verification — run exactly:
cargo test -p hot-trimmer-effect-compiler algorithm_stage_15_profiles

Stop conditions:
- Stop if lengths remain fractions of the hotspot minor edge while reported as meters.
- Stop if supersampling is used to make an illegal profile fit.
- Stop if generated normals overwrite imported normals before Stage 17 composition.
```

---

## Prompt 16 — Stage 16: scale-constrained semantic details and patterns

```text
Implement Prompt 16 / Stage 16 from the full algorithm-stack prompt pack.

Read the common rules and revised Stage 16. Consume Stage 10 capacities and Stage 15 occupancy. No subagents.

Objective:
Compile material-independent semantic detail overlays that remain physically valid across panels, strips, caps, and
radial slots.

Scope:
- Define typed DetailDefinition/CompiledDetail contracts with physical range, scale space, compatible roles,
  orientation, aspect limits, minimum pixels, repeat period, contain/cover policy, channel contributions, fallback,
  provenance, and seed.
- Implement repeating strip, unique detail, radial detail, trim cap, bolt group, vent, panel stamp, groove, decal,
  procedural motif, and user-patch detail families.
- Implement registered mask-to-SDF conversion and coherent bevel/groove/lip/stamp Height/Normal contributions.
- Implement physical motif periods and role-specific Surface/HorizontalStrip/VerticalStrip/Radial/TrimCap/Unique
  variants. Never squeeze an oversized motif to fit.
- Implement full, simplified Height/Normal, NormalOnly, Roughness/Color, and Disabled detail LODs with explicit
  variant/fallback selection and occupancy interaction.
- Distinguish source material-domain structure from semantic details supplied by template/user/procedural intent.
- Add detail route/occupancy QA views and docs/phase-reports/algorithm-stage-16.md.

Acceptance:
- Details preserve physical size, orientation, and repeat period across slot shapes and output resolutions.
- Panel bolt groups do not share the strip-rivet evaluator.
- Oversized/incompatible details choose a declared variant/fallback or fail; they never stretch.
- An empty detail list produces a valid SkippedBecauseUnused stage result.

Verification — run exactly:
cargo test -p hot-trimmer-effect-compiler algorithm_stage_16_details

Stop conditions:
- Stop if one square mask is resized into all slot roles.
- Stop if decorations modify topology by default.
- Stop if a user patch loses registered PBR correspondence.
```

---

## Prompt 18 — Stage 18: scale-aware effect compilation and material-state synthesis

```text
Implement Prompt 18 / Stage 18 from the full algorithm-stack prompt pack before Prompt 17.

Read the common rules and revised Stage 18. Consume slot demand, profile/detail plans, masks, and material intent.
No subagents.

Objective:
Make one authoritative compiler resolve raw profiles, details, weathering, and material-state recipes into legal
role-specific CompiledEffect operations before PBR rendering/composition.

Scope:
- Implement EffectDefinition, EffectApplicability, EffectScale/ScaleSpace, EffectFallback, EffectOccupancy,
  EffectRoute, CompiledEffect, EffectPlan, and typed fallback reasons from the revised contracts.
- Enforce compilation order: scale space -> physical dimensions -> role compatibility -> physical fit/occupancy ->
  pixel dimensions -> feature LOD -> role evaluator -> supersampling -> compiled operation/failure.
- Generate registered region, exposed edge, distance, cavity, raised/recessed, direction, radial inner/outer,
  decoration, and material-group masks in physical units where practical.
- Implement Grunge and EdgeWear variants for surface, horizontal strip, vertical strip, radial inner/outer, unique,
  and trim cap; implement strip anisotropy, radial coordinates, cap transitions, and world-direction influence.
- Implement Micro/Meso/Macro/Structural material state and user recipes Clean, Used, Heavy, Wet, Dusty, Chipped Paint,
  Rusting, and Mossy as typed bundles compiled separately per slot.
- Implement effect-family LOD ladders, deterministic placement, occupancy conflict resolution, 1x/2x/4x/8x
  supersampling, complete fallback diagnostics, and Clean/empty valid plans.
- Remove any raw-effect direct render path or universal normalized grunge texture.
- Add effect-route/occupancy/LOD/supersampling QA views and docs/phase-reports/algorithm-stage-18.md.

Generalization contract:
Recipes express material state, not a named material. Their compiled evaluator depends on measured class, slot role,
physical capacity, masks, and user intent.

Acceptance:
- Required panel, extreme strip, radial, cap, sub-pixel, and conflicting-profile/effect fixtures select the expected
  role evaluator/LOD/fallback across 1K/2K/4K/8K.
- No isotropic physical effect becomes non-uniformly stretched.
- Resolution may promote LOD without moving seeded features.
- Clean compiles to a valid empty effect plan; incompatible effects are reported, never hidden.

Verification — run exactly:
cargo test -p hot-trimmer-effect-compiler algorithm_stage_18_effect_compilation

Stop conditions:
- Stop if one universal weathering evaluator handles all roles.
- Stop if raw recipe parameters can reach render-core without CompiledEffect.
- Stop if a fallback changes topology or silently changes physical scale.
```

---

## Prompt 17 — Stage 17: PBR estimation and composition from compiled effects

```text
Implement Prompt 17 / Stage 17 from the full algorithm-stack prompt pack after Prompt 18.

Read the common rules and revised Stage 17. Consume only Stage 14 registered material and Stage 18 compiled
operations; do not decide effect applicability here. No subagents.

Objective:
Render and compose imported, estimated, structural, detail, and weathering contributions into physically coherent
PBR channels with explicit provenance.

Scope:
- Implement physical Height composition with explicit amplitudes, material-class ranges, clamps, and contribution
  provenance. Do not add unrestricted normalized maps.
- Generate normals from physical Height using Scharr derivatives divided by meters-per-pixel X/Y.
- Decode imported normals, combine them with generated/compiled contributions through reoriented or equivalent
  vector-correct normal mapping, renormalize, and re-encode in selected convention.
- Prefer imported Roughness; otherwise estimate from class/base/contrast/high-frequency evidence and compiled effects
  with Estimated provenance and bounded ranges.
- Keep Metallic zero unless imported, explicitly labeled metal/material-ID-driven, or changed by exposed-metal effect.
- Implement multi-radius physical Height AO/cavity and compose compiled contributions.
- Define a versioned local learned-map provider boundary; imported maps and explicit procedural structure take
  precedence. Unavailable provider routes explicitly to classical estimates/pass-through.
- Produce per-channel contribution/provenance QA views and docs/phase-reports/algorithm-stage-17.md.

Acceptance:
- Imported registered maps remain aligned and retain priority according to policy.
- Normal composition passes vector fixtures and contains no RGB-average path.
- Metallic never appears from Base Color alone without explicit allowed intent.
- Base-Color-only outputs label estimated channels and remain deterministic.

Verification — run exactly:
cargo test -p hot-trimmer-sheet-compiler algorithm_stage_17_pbr_composition

Stop conditions:
- Stop if Stage 17 changes an effect route or fit decision.
- Stop if generated structure overwrites imported normals instead of composing.
- Stop if inferred channels lose Estimated provenance.
```

---

## Prompt 19 — Stage 19: atlas finishing, feature LOD validation, exact IDs, and metadata

```text
Implement Prompt 19 / Stage 19 from the full algorithm-stack prompt pack.

Read the common rules and revised Stage 19. Consume fully composed slot channels and plans. No subagents.

Objective:
Finish a registered atlas with correct bleed/filtering/mips, exact categorical maps, survival evidence, and complete
runtime metadata.

Scope:
- Evaluate material/effects over allocation rectangles where possible; implement nearest-valid-pixel/jump-flood
  dilation for finite unique content. IDs never bleed.
- Downsample compiled supersampled slots by channel: color-managed area Base Color, linear scalar filtering,
  decoded-vector normal filtering/renormalization, validated Roughness/Metallic, nearest categorical IDs.
- Implement channel-specific atlas filtering/mips. Region ID fills hotspot rectangles only with exact colors and no
  AA/dither/bleed/color transform; Material ID uses exact shared labels.
- Implement mip-survival validation at mip 0/1/2 and configured target range for bevel coherence, strip collapse,
  seams/bleed, simplified-effect strength, and ID exactness at mip 0.
- Produce complete manifest data: template/version/compatibility/topology hash, material revision, slot/allocation/
  hotspot/role/fit/world/radial data, map paths/colorspaces/checksums, and effect/LOD/supersampling summaries.
- Produce deterministic detailed and concise compilation reports plus all Stage 19 QA views.
- Add docs/phase-reports/algorithm-stage-19.md.

Acceptance:
- All channels have identical authoritative dimensions/registration.
- Region/Material IDs are exact and categorical; Region ID covers hotspot, not allocation bleed.
- Supersampling changes raster quality but never physical dimensions.
- Survival validation catches disappearing and over-strengthened fixtures.
- Same inputs produce byte-identical maps, checksums, manifest payload, and report.

Verification — run exactly:
cargo test -p hot-trimmer-sheet-compiler algorithm_stage_19_atlas_finishing

Stop conditions:
- Stop if normal downsampling treats encoded RGB as color.
- Stop if IDs are antialiased, bled, or mip-filtered at mip 0.
- Stop if a survival warning is computed from UI approximations rather than authoritative channels/plans.
```

---

## Prompt 20 — Stage 20: preview, QA workflow, atomic export, and Blender application

```text
Implement Prompt 20 / Stage 20 from the full algorithm-stack prompt pack.

Read the common rules and revised Stage 20. Consume the Stage 19 CompiledSheet lineage and manifest only.
No subagents.

Objective:
Expose the complete material compiler through the existing source-first UI, prove it on representative geometry,
publish it atomically, and apply semantic slots correctly in Blender.

Scope — desktop workflow:
- Keep the established source library/canvas/sheet/inspector shell. Add typed controls for material-class override,
  physical-scale measurement, orientation, de-lighting, domain-route policy, seed, source sufficiency, placement pin/
  override, legal transforms, ExplicitStretch warning, material-state recipe, and effect fallback.
- Add staged cancellable analysis/domain/placement/compile/export jobs with revision guards, progress, cache reuse,
  bounded preview refinement, and no partial authoritative publication.
- Add all QA views from revised Stage 20: channels, IDs, crop usage, repetition heatmap, seam energy, texel density,
  effect route/occupancy/LOD/supersampling, mip warnings, plan/provenance, and Blender status.
- Every slot inspector shows selected source/domain/crop/transform/mode/cost, confidence, route, and fallback. UI does
  not recompute compiler truth.

Scope — preview/export:
- Preview Plane, Cube, Cylinder, Beveled Block, Wall Module, Archway, Radial Disc, and Mechanical Prop, including
  several authored hotspot-UV fixtures. Preview consumes exported-equivalent map handles and conventions.
- Implement snapshot-based cancellable atomic package export with complete maps, manifest, checksums, colorspaces,
  overwrite policy, staging validation, flush, and publish. Failure/cancellation never exposes partial success.

Scope — Blender companion:
- Load/validate manifest, create/update full Principled material, color spaces, normal, Height/displacement policy,
  Roughness, Metallic, AO, opacity as present, and material revision tracking.
- Describe selected UV islands/geometry, find compatible rectangular/strip/unique/cap/radial slots, fit without
  non-uniform distortion, preserve locked assignments, and report insufficiency/topology mismatch.
- Material/effect/resolution updates reload maps without remapping unchanged topology; topology changes require an
  explicit compatibility decision.
- Add one root `check:algorithm-stage-20` script that runs the focused desktop Stage 20 tests and Blender companion
  fixture tests as one command. Do not hide unrelated workspace tests behind it.
- Add docs/phase-reports/algorithm-stage-20.md and user-facing workflow/diagnostic documentation.

Acceptance:
- The universal corpus can be compiled through the UI without material-specific workflows.
- QA views are authoritative and explain every important crop/effect/fallback decision.
- Preview/export agree within channel tolerances.
- Blender fixtures map rectangular, strip, and radial semantics without non-uniform UV distortion; locks survive
  updates and map-only revisions reload without remapping.
- Failed/cancelled jobs and exports publish no partial result.

Verification — run exactly:
npm run check:algorithm-stage-20

Stop conditions:
- Stop if enabled UI controls are not command-backed.
- Stop if preview has a shortcut renderer different from final/export.
- Stop if Blender only calculates fit values without authoring and validating UV/material state.
```

---

## Prompt 21 — Full V1 qualification and old-engine removal

```text
Implement Prompt 21: qualify the complete twenty-stage Hot Trimmer V1 algorithm stack and remove every replaced
legacy authority.

Read AGENTS.md, both governing algorithm documents, every algorithm stage report, the corpus manifest, and the
traceability matrix. Inspect current git status. No subagents.

Objective:
Prove the full product promise across behavior classes, source conditions, slot roles, output sizes, QA views,
export, and Blender. This prompt implements no new algorithm and creates no new acceptance criteria.

Scope:
- Execute every revised section-30 acceptance criterion against the universal corpus and required geometry/effect
  fixtures at 1K, 2K, 4K, and 8K where specified.
- Close every traceability row with unit/property/plan/image/failure/performance evidence. A pass-through result is
  accepted only where the stage contract allows it and its reason is asserted.
- Measure named 8K analysis/domain/placement/compile memory and time, preview latency, cancellation latency, cache
  reuse, save/reopen, export, and Blender update on documented hardware.
- Exercise malformed/bounded inputs, cache loss, low disk, cancellation, revision supersession, deterministic
  reruns, project crash recovery, atomic package publication, and offline operation.
- Delete the legacy sheet compiler, normalized profile renderer, runtime semantic repacking, obsolete mappings,
  duplicated IPC/React authority, old schema/migrations, unused fixtures, temporary adapters, and dead dependencies.
- Confirm one source document, one staged algorithm pipeline, one CompiledSheet lineage, and one manifest authority.
- Update README, technical spec, architecture decisions, support diagnostics, algorithm version registry, Blender
  guide, known limitations, and docs/phase-reports/algorithm-stack-v1-qualification.md.

Acceptance:
- All twenty stage reports and traceability rows are green with no unimplemented route presented as complete.
- Universal invariants pass across all behavior classes and required slot roles.
- Same complete input tuple produces deterministic plans/routes/maps/reports.
- Source insufficiency and invalid effects are actionable and never silently distorted.
- Preview/export/Blender agree and locked geometry assignments survive material/effect/resolution updates.
- No live code can publish through the old engine or alter standard topology from appearance.

Verification — run exactly:
npm run check

Stop conditions:
- Stop rather than waive a failed revised acceptance criterion.
- Stop if a deleted legacy path still has a runtime caller.
- Stop if broad green tests conceal missing corpus/visual/Blender evidence.
```

---

## Continuation prompt for an incomplete stage

```text
Continue the active Hot Trimmer algorithm-stage prompt. Read its exact scope, current stage report, focused test
output, current diff, and AGENTS.md. Do not widen scope, start a later stage, create new acceptance criteria, or
rerun broad tests. Resolve the reported blocker, make at most one focused correction pass, run only the prompt’s
one verification command, update its stage report, and stop. If the same external/tool blocker has repeated for
three consecutive turns and no meaningful in-scope work remains, report it as blocked with concrete evidence.
```

## Read-only stage audit prompt

```text
Audit the completed Hot Trimmer algorithm stage named by the user against its governing revised-spec section,
prompt acceptance criteria, universal invariants, stage report, and focused verification evidence. Do not edit files,
run the application, create new requirements, or review unrelated stages. Report only concrete gaps that could make
the stage’s claimed result false, with exact file/contract/test evidence. If no such gap exists, say so plainly.
```
