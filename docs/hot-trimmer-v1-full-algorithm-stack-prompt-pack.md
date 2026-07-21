# Hot Trimmer V1 full algorithm stack: Codex prompt pack

## Purpose

This pack implements the complete twenty-stage architecture in
`docs/hot-trimmer-v1-full-algorithm-stack-revised.md` according to
`docs/hot-trimmer-v1-full-algorithm-stack-implementation-plan.md`.

Run one prompt per Codex task, in the order listed here. Do not combine prompts. A prompt is complete only when
its stage is implemented, connected to the authoritative pipeline, and its focused verification passes.

This is a quality-first algorithm replacement on the accepted GPU execution architecture:

- Preserve the useful source-first desktop workbench and authoring workflow.
- Preserve `TrimSheetDocument`, `compile_persisted`, the immutable compiled-plan boundary, the application-owned GPU
  service, tiled artifacts, and streaming export as the single production orchestration/execution lineage.
- Treat `AlgorithmCompiler` as internal low-volume plan compilation beneath `compile_persisted`, never as a second live
  facade. `CompiledSheet` is a manifest/lineage over tiled GPU results, not a monolithic CPU map container.
- Do not preserve old project/schema compatibility, runtime semantic repacking, normalized profile/effect behavior,
  or the CPU production pixel executor.
- Do not create dual authorities, adapters that become permanent, or silent legacy fallbacks.
- Delete a replaced algorithm or CPU pixel path only after its compact-plan/GPU replacement is authoritative and its
  focused gate passes. Keep the smallest explicitly test-only CPU parity oracles.
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
10. Preview, final render, QA views, export, and Blender consume one `compile_persisted`/`CompiledSheet` lineage and the
    same tiled GPU executor semantics.

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
- CPU work is limited to document/algorithm planning, validation, branch-heavy bounded solvers, cache identities,
  scheduling policy, diagnostics, small reductions, final encoding, filesystem I/O, and explicit test oracles. The
  production CPU route must not rasterize slots, effects, atlas maps, QA images, padding, or mips.
- Production pixel-parallel work uses the existing application-owned GPU service and compact immutable commands. Keep
  intermediates GPU-resident, generate only requested maps/QA outputs, operate in bounded tiles with declared halos,
  and read back only final tiles or bounded summaries.
- Do not add a second renderer, GPU owner, preview-only shader, exporter, or silent CPU fallback. A GPU failure is a
  typed failure under the supported-hardware policy.
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
| 09 | Fixed semantic template topology | 9 |
| 09V | Grid authoring, deterministic layout variants, and template presets | Stage 9 authoring integration |
| 10-14 | One prompt per demand, placement, and synthesis stage | 10-14 |
| 14P-A | First visible authoritative atlas integration | Through Stage 14 |
| 14P-B | Intermediate-preview QA and cache hardening | Through Stage 14 |
| 15 | Compact structural-profile compilation and tiled GPU occupancy/Height | 15 |
| LIB | Reusable source/patch/stamp/profile library and management window | Cross-cutting before Stage 16 |
| 16 | Compact semantic-detail plans and tiled GPU evaluation | 16 |
| 20A | Early profile/detail authoring, raw contribution inspection, and Stage 15-20 feedback telemetry | Product feedback integration after Stage 16 |
| 18 | Effect compilation and tiled GPU effect evaluation | 18 |
| 17 | Requested-map GPU PBR composition from compiled effects | 17 |
| 19 | Tiled GPU finishing, reduced validation, and metadata | 19 |
| 20 | Product workflow, QA, package publication, and Blender on the accepted GPU path | 20 |
| 21 | Full GPU V1 qualification and remaining obsolete-path removal | Final V1 gate |

Stage 18 precedes Stage 17 in implementation because Stage 17 consumes `CompiledEffect` operations produced by
Stage 18. The product-stage numbers remain unchanged.

Prompt 20A deliberately runs after Prompt 16 without claiming Stage 20 completion. It exposes only installed compiler
truth, gives product feedback a real in-app loop, and establishes UI/telemetry contracts that Prompts 18-20 extend.

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

## Prompt 09V — grid authoring, layout variants, and template presets

```text
Implement Prompt 09V after Prompt 09 and before Prompt 10.

Read the common rules and revised Stage 9 plus the Stage 09 report. This is an authoring integration milestone for
Stage 9, not material-dependent runtime packing. Consume the existing TemplateDefinition/registry/snapshot contracts.
No subagents.

Objective:
Let users author trim-sheet topology on a logical grid, generate useful deterministic subdivision variants, save and
reuse layouts as presets, and explicitly freeze one draft as immutable Stage 9 topology before material placement.

Scope — typed layout authoring:
- Define TemplateDraft, LayoutTree, LayoutNode, GridGuide, LayoutEditCommand, LayoutVariantIntent, LayoutVariant,
  LayoutPreset, validation diagnostics, and an explicit FreezeTemplate result producing a versioned
  TemplateDefinition/TemplateSnapshot.
- Use the Stage 9 canonical 4096 x 4096 integer space as authority. Offer a 64 x 64 logical authoring grid by default,
  with bounded 8/16/32/64/128 grid densities and authored guides that map exactly to canonical integer boundaries.
  Display pixels and zoom coordinates never become topology authority.
- Implement recursive horizontal/vertical split with equal, weighted, count, or guide-aligned children; resize shared
  boundaries; subdivide; merge edge-adjacent compatible cells; duplicate patterns; reorder stable semantic groups;
  reserve/unused zones; and explicit allocation padding/hotspot inset. Preserve exact shared boundaries throughout.
- Use a hybrid rectangular-region draft as authoring authority: recursive split trees/groups retain useful generation
  and hierarchy provenance, but users may freely move valid leaf rectangles on the grid. Direct manipulation must not
  be artificially restricted to guillotine partitions or require editing the hierarchy tree.
- Let each leaf author stable key/name/order, Planar/RepeatingStrip/UniqueDetail/TrimCap/Radial role, fit semantics,
  orientation, structural profile intent, material/variation group, world-size intent, importance, legal transforms,
  ID color, source-mapping default, radial parameters, and lock state. Merge/split must require an explicit policy for
  incompatible semantic metadata; it must not guess or discard it.
- Maintain command-backed undo/redo and transactional persistence. Draft edits create draft revisions only. They do
  not mutate released standard templates, project-pinned snapshots, compiled topology, placement, or Blender UVs.

Scope — layout variant generation:
- Generate a bounded deterministic family from author intent: logical grid, seed, hierarchy-depth/count limits,
  desired panel/strip/detail/cap/radial proportions, minimum/maximum cell spans, preferred aspect families, reserved
  zones, symmetry/repetition preferences, and locked nodes/guides.
- Include useful initial intents: Balanced Architecture, Panel Heavy, Strip Heavy, Dense Modular, Detail Heavy,
  Mechanical/Radial, and Minimal/Mobile. Treat them as general layout constraints, never named-material workflows.
- Rank variants by inspectable topology-only terms: requested-role coverage, aspect-family coverage, size hierarchy,
  reuse-friendly strip banks, radial/cap availability, wasted/reserved area, fragmentation, and constraint violations.
  Material pixels, source classification, crop quality, output seed, and effect appearance cannot influence topology.
- Preview several candidate trees/rectangles without making any one authoritative. Regeneration with the same complete
  intent and seed is byte-identical; accepting a variant copies it into an editable draft rather than silently
  replacing the current template.

Scope — authoring UI and presets:
- Make the Grid/Layout workspace feel like a robust Tetris/inventory editor. The primary canvas supports click-select,
  box/multi-select, click-drag Draw Region, pick-up/drag/drop with a snapped ghost footprint, edge/corner resize handles,
  keyboard nudge, duplicate, delete, copy/paste, 90-degree rotate where semantically legal, and pan/zoom. Green ghost
  means a valid atomic drop; red ghost explains collision, bounds, minimum-size, lock, or semantic failure.
- Support two explicit creation modes: Draw in Empty Space and Carve/Split Selected Region. Drawing never silently
  destroys overlapped regions. Delete leaves intentional unallocated space; Fill Empty, Pack Selection, and compact/
  distribute commands are explicit operations with preview, not automatic reflow after every edit.
- Add grid/guide/neighbor-edge snapping with temporary modifier override, numeric canonical/logical position and size,
  align/distribute, equalize width/height, array/strip creation, repeat subdivision, merge, semantic-role palette,
  region colors/icons/labels, locks, hierarchy/outliner, breadcrumbs, validation list, and right-click command menu.
- Collision policy is visible and selected before the gesture: Block by default, Swap only for compatible equal
  footprints, and explicit Push/Repack preview for a bounded selected group. No hidden cascade movement is permitted.
- Keep selection state and preview gestures ephemeral. A drop/draw/resize becomes one typed atomic command and one
  undo step only after validation. Escape cancels without mutation; stale/cancelled commands restore the prior draft.
- Provide solo/isolate, hide/show labels, allocation versus hotspot/bleed overlays, logical/canonical coordinates,
  semantic filters, minimap, and exact 1K/2K/4K/8K boundary preview. A material image may appear as an optional visual
  reference beneath the grid but cannot affect rectangles, snapping, validation, variant scoring, or frozen topology.
- Every enabled control invokes a typed native command; UI geometry is projection only.
- Support New Draft, Duplicate Template, Generate Variants, Accept Variant, Save Draft, Save Preset, Freeze as New
  Template Version, and Reset Draft. Saving a preset records immutable intent/tree/version identity, not a mutable
  filepath or pointer to the current canvas.
- Store project-local and user-global presets through the template registry/project-store contract. Prompt LIB later
  indexes the same preset identities in its unified browser; do not create a second incompatible library model here.
- On freeze, validate topology and semantics, assign stable IDs/colors/order, snapshot canonical JSON/hash, and show a
  compatibility diff. Changing a project from one frozen template/version to another is an explicit topology change
  with keep/remap/clear decisions for pins, mappings, and later Blender assignments.
- Bound tree depth, leaf count, guides, variant count, generation work, memory, serialized size, and cancellation
  latency. A cancelled/stale generation or freeze publishes no draft/template/preset mutation.
- Add docs/phase-reports/algorithm-stage-09-layout-variants.md.

Acceptance:
- A user can author the worked 64 x 64 progressively subdivided layout, undo/redo it, save it as a preset, reopen it,
  freeze it, and obtain byte-identical canonical rectangles/hash at 1K/2K/4K/8K.
- Direct-manipulation fixtures cover draw, carve, move, resize, duplicate, delete, multi-select, swap, explicit repack,
  snapping override, keyboard nudge, rotation legality, collision rejection, and gesture cancellation as atomic undoable
  commands. Invalid drops change nothing and report the exact conflicting cells/regions.
- Split/resize/merge property fixtures retain in-bounds nonoverlap, exact shared boundaries, stable locked nodes, and
  complete semantic metadata or return a typed incompatibility without mutation.
- Every initial variant intent produces multiple deterministic, genuinely different valid layouts satisfying its
  declared role/aspect/size constraints; the same intent and seed reproduce byte-identical ordering and scores.
- Standard templates and existing project snapshots never change when drafts, presets, variants, materials,
  resolutions, or seeds change. Freeze/version switching is explicit and compatibility-diagnosed.
- The accepted frozen template enters Prompt 10 through the existing Stage 9 topology compiler with no alternate
  runtime packing or UI-side conversion.

Verification — run exactly:
cargo test -p hot-trimmer-desktop algorithm_stage_09_layout_authoring

Stop conditions:
- Stop if material/source analysis influences generated rectangles or topology scores.
- Stop if a draft or preview variant can reach Stage 10 without explicit successful freeze.
- Stop if screen-space rectangles, fractional rounding, or per-resolution editing become topology authority.
- Stop if drawing or moving a region silently deletes, clips, resizes, or cascades other regions.
- Stop if editing a preset/template silently changes an existing project's pinned topology or Blender assignments.
- Stop if merge/split drops role, fit, group, world-size, radial, transform, or lock metadata without an explicit choice.
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

## Prompt 14P-A — First visible authoritative atlas through Stage 14

```text
Implement Prompt 14P-A, the first visible authoritative atlas integration gate, from the full algorithm-stack
prompt pack.

Read the common rules, revised Stages 9-14, and every Stage 9-14 phase report. Consume only authoritative artifacts
from the installed Stage 1-14 pipeline. No subagents.

Prerequisite:
- Finish the remaining Stage 14 review corrections, especially selected-seam execution and the slice-center
  synthesis route. Rerun Stage 14's own focused gate under its prompt before starting this integration prompt.
- Do not make the compositor compensate for, reinterpret, or hide a known-invalid Stage 14 slot result.

Current integration reality:
Stages 1-14 are implemented as authoritative typed algorithms, but the Prompt 00 AlgorithmCompiler facade still
rejects Stage 1 and CompilerRequestHeader does not carry sufficient executable inputs. Installing the single
Stage 1-14 orchestration path inside AlgorithmCompiler is in scope. Reuse the installed stage implementations;
do not reimplement their algorithms, bypass their artifacts, or treat the request header alone as executable input.

Objective:
Get the first truthful atlas on screen quickly: run one real persisted project through the installed Stage 1-14
algorithms, compose actual Stage 14 slot outputs into Stage 9 topology, and display the explicitly incomplete result.

Scope:
- Add a typed `IntermediateAtlasArtifact` distinct from final `CompiledSheet`. It must carry the Stage 9 topology,
  Stage 13 PlacementPlan identity, Stage 14 slot-result identities, concise source/patch lineage, revision,
  algorithm versions, diagnostics, and explicit `incomplete_after_stage: 14` status.
- Connect the sole `AlgorithmCompiler` authority through the installed Stage 1-14 path. Remove the Prompt 00
  unconditional Stage 1 rejection for this intermediate route; do not create a second compiler facade.
- Start with one representative persisted project and its authored patches. Execute the existing Stage 1-14
  implementations required by the selected template without broad corpus or cache-matrix hardening.
- Composite each successful allocation-local Stage 14 channel into its exact Stage 9 allocation/hotspot rectangle.
  Use the Stage 14 validity/correspondence result and channel semantics. Do not center-cover the whole source,
  stretch a failed slot, call the legacy renderer, or substitute placeholder pixels.
- Permit an intermediate Base Color view and only those imported/Stage 14 registered channel views that actually
  exist. Missing generated Height/Normal/Roughness/Metallic/AO remain explicitly unavailable; Stage 15-19 work is
  not simulated.
- Add only the essential authoritative inspection: slot boundaries/names, mapping mode, validity, correspondence,
  and concise selected patch/domain/candidate/SamplingPlan lineage for the clicked slot.
- Replace the desktop's hard-coded preview-unavailable command with a cancellable, revision-guarded native command
  backed by `IntermediateAtlasArtifact`. Enable a clearly labeled `Preview through Stage 14` action when inputs are
  sufficient.
- Label the canvas `Intermediate Stage 14 material-placement preview`. It is non-exportable and must list profiles,
  semantic details, effects, final PBR composition, finishing, mips, metadata, and Blender application as pending.
- Enforce cancellation, revision supersession, and required-slot atomicity. No failure publishes a partial atlas.
- Keep final compile, export, and Blender actions disabled. Do not add the full invalidation matrix, heatmaps, rich
  decision explorer, broad corpus qualification, or cache/performance hardening; Prompt 14P-B owns those.
- Keep Stage 20 responsible for production preview fixtures, complete QA, package publication, and Blender
  synchronization on the accepted GPU preview/streaming-export infrastructure.
- Add docs/phase-reports/algorithm-stage-14-preview-a.md.

Acceptance:
- One representative persisted material produces a visible atlas whose required regions are composed from the exact
  Stage 14 slot outputs and whose topology matches Stage 9 byte-for-byte.
- Every visible slot can identify its selected source patch, domain, candidate, SamplingPlan, mapping mode, and
  Stage 14 result. Correspondence and validity views align with Base Color boundaries.
- Imported registered channels remain aligned. Unavailable later-generated channels are labeled unavailable rather
  than filled with constants or estimates.
- A failed required slot, cancellation, or revision supersession publishes no partial atlas. No preview call edge
  reaches the removed legacy renderer or an implicit Stretch/center-cover fallback.
- The desktop displays the intermediate artifact and its incomplete-through-Stage-14 label while final compile,
  export, and Blender actions remain unavailable.

Verification — run exactly:
cargo test -p hot-trimmer-desktop algorithm_stage_14_preview_a

Stop conditions:
- Stop if the preview is made by covering the atlas with one source image instead of composing Stage 14 slots.
- Stop if an unimplemented Stage 15-19 feature is represented by plausible placeholder pixels.
- Stop if the UI recomputes crop, correspondence, validity, or mapping diagnostics independently of artifacts.
- Stop if intermediate preview can be mistaken for, saved as, or exported as a final compiled sheet.
```

---

## Prompt 14P-B — Intermediate-preview QA and cache hardening

```text
Implement Prompt 14P-B, the Stage 14 intermediate-preview hardening gate, from the full algorithm-stack prompt pack.

Read the common rules, Prompt 14P-A report, and revised Stage 14 integration checkpoint. Consume the working
`IntermediateAtlasArtifact` path from 14P-A. No subagents.

Objective:
Harden the truthful Stage 14 preview after it is already visible, without widening it into final Stage 20 preview,
export, or Blender integration.

Scope:
- Complete the cache-key and invalidation matrix for sources, patches, preparation, calibration, classification,
  material domains, topology, slot demand, candidates, scoring, placement, SamplingPlans, output, versions, and seed.
- Add authoritative source-usage/repetition heatmaps, crop overlays, mapping-mode views, registered-channel alignment,
  required-slot failures, and a rich per-slot Stage 11-14 decision explorer. UI views consume artifact data only.
- Expand representative coverage across the universal corpus and every Stage 14 mapping family without changing
  the algorithms or adding special cases.
- Measure and bound preview memory, execution time, cache reuse, cancellation latency, and revision supersession.
- Preserve 14P-A's non-exportable incomplete label and keep Stage 15-19 output explicitly pending.
- Add docs/phase-reports/algorithm-stage-14-preview-b.md.

Acceptance:
- Patch, scale/orientation, classification, domain, placement, resolution, version, and seed changes invalidate the
  correct key; identical complete inputs yield byte-identical pixels, lineage, QA data, and diagnostics.
- Every corpus behavior class reaches either an honest intermediate atlas or a typed insufficiency recovery.
- QA views align with the exact Stage 9/13/14 artifacts and contain no duplicated UI calculations.
- Bounded cancellation and stale revisions publish no partial artifact; cache hits cannot return stale lineage.
- Final compile, export, and Blender remain unavailable and no Stage 15-19 placeholder content appears.

Verification — run exactly:
cargo test -p hot-trimmer-desktop algorithm_stage_14_preview_b

Stop conditions:
- Stop if QA hardening delays or replaces the working 14P-A Base Color preview.
- Stop if a cache hit survives any authoritative input or version change.
- Stop if a heatmap or decision explanation is reconstructed from UI-side math.
- Stop if this prompt begins implementing profiles, effects, final finishing, export, or Blender.
```

---

## GPU readiness gate before Prompt 15

Prompt 15 may start only after Render Prompt 005 has accepted the single `compile_persisted` GPU executor, bounded
source/output tiling, requested-map pass graph, binary preview, streaming export, and the supported-GPU failure policy.
The accepted GPU route must also lower every Stage 14 `SamplingPlan` used by the full algorithm stack into exact compact
GPU commands or a prepared registered source domain. `TextureSynthesis`, `ThreeSliceCap`, `NineSlicePanel`, contain/
cover, and radial variants may not be rejected, collapsed into ordinary direct sampling, or sent to the CPU production
rasterizer. If that lowering is incomplete, stop and complete it as a Stage 14 GPU integration correction before
starting Stage 15.

For Prompts 15–21, the standing execution split is:

```text
CPU: validate intent -> resolve physical legality/LOD/dependencies -> compile immutable compact commands
GPU: schedule bounded tiles -> evaluate pixels/intermediates -> cache GPU resources -> publish requested final tiles
CPU: consume bounded summaries/readbacks -> encode/write metadata/files -> publish atomically
```

Do not read a GPU intermediate back merely so the CPU can perform the next pixel pass.

---

## Prompt 15 — Stage 15: compiled structural profiles and tiled GPU synthesis

```text
Implement Prompt 15 / Stage 15 from the full algorithm-stack prompt pack.

Read the common rules and revised Stage 15. Consume Stage 10 capacities and Stage 14 slot-local material. No subagents.

Prerequisite:
- The GPU readiness gate above is green, including exact full-Stage-14 sampling-plan lowering.

Objective:
Compile physically legal structural profiles on the CPU and evaluate their occupancy, physical Height, and derivatives
as bounded GPU tile passes instead of normalized CPU or shader shortcuts.

Scope:
- Replace the current normalized profile API with requested and `CompiledProfile` plans expressed in physical/relative
  units, resolved pixel scale, occupancy semantics, LOD, supersampling, evaluator, fallback reason, algorithm version,
  compact resource references, required halo, and deterministic cache identity.
- Keep profile constraint solving, opposing-edge legality, fallback selection, and compact command compilation on the
  CPU. These are bounded branch-heavy decisions and must not become per-pixel GPU policy.
- Implement rectangle/disc/annulus signed-distance fields and the complete required programs: Flat, convex bevel,
  concave groove, rounded bevel, double bevel, raised lip, recessed seam, panel frame, fully rounded strip, merged
  opposing bevel, radial disc, annulus, and bounded custom profile curve.
- Enforce opposing-profile/minimum-flat-center legality. Implement explicit clamp, fully rounded, merged, normal-only,
  disabled, and incompatibility outcomes selected by declared policy.
- Implement FullHeight, SimplifiedHeight, NormalOnly, RoughnessOnly, and Disabled profile LODs without changing
  physical profile dimensions or seed.
- Add compact profile commands to the immutable atlas plan and execute SDF, occupancy, physical Height, analytic
  derivatives, filtering, and compiled 1x/2x/4x/8x supersampling on the application-owned GPU service. Do not widen
  sub-pixel geometry in final pixels.
- Represent structural occupancy authoritatively as compiled semantics plus GPU-resident tile-local fields: signed
  distance, inside/outside, flat-center, raised, recessed, cap, groove, and profile-exclusion, with physical Height and
  derivative contributions. Do not allocate or persist unconditional full-atlas CPU occupancy arrays.
- Cache compiled plans by exact upstream identity and GPU outputs by plan/map/tile/halo/format/shader identity. Keep
  occupancy and Height GPU-resident for Stages 16–19; read back only requested QA tiles or bounded validation summaries.
- Use the existing requested-map graph: Base Color-only work runs no profile pass; a Height/Normal/effect request runs
  only its required profile dependencies. Declare sufficient halos for every SDF/filter/derivative operation.
- Treat authored profile edges as material structure. Atlas allocation borders are never bevels, seams, or wear edges
  merely because two rectangles touch. Preserve enough semantic occupancy for later stamps to conform above, below,
  inside, outside, or across a profile without reverse-engineering pixels.
- Keep physical Height authoritative until Stage 17 derives and composes normals. An imported normal may coexist with
  the profile plan, but Stage 15 must not flatten the two into an opaque replacement map.
- Add profile occupancy/LOD/fallback QA views and docs/phase-reports/algorithm-stage-15.md.
- Remove the normalized profile CPU production path and the current hard-coded normalized GPU profile evaluator after
  GPU parity passes. Retain only minimal CPU formula fixtures as an explicitly test-only oracle.

Acceptance:
- Required broad panel, extreme strips, radial, cap, sub-pixel, and opposing-bevel fixtures pass at 1K/2K/4K/8K.
- Opposing profiles never overlap accidentally or leave negative center width.
- Physical slope/width remain stable across slot aspect and resolution.
- Allocation boundaries with no requested profile remain flat, while requested radial/linear profile occupancy can
  be queried consistently by Stage 16 and Stage 18.
- Every fallback is deterministic and present in the `CompiledProfile` diagnostics that Stage 18 later incorporates
  into `EffectPlan`.
- GPU tile output matches the compact CPU oracle within declared float/packing tolerances, adjacent tiles agree at
  valid interiors, unrequested profile passes dispatch zero work, and production CPU profile raster counters remain
  zero.
- Profile intermediates remain within declared GPU tile/residency budgets at 1K/2K/4K/8K and survive cancellation and
  revision supersession without stale publication.

Verification — run exactly:
cargo test -p hot-trimmer-sheet-compiler algorithm_stage_15_gpu_profiles

Stop conditions:
- Stop if lengths remain fractions of the hotspot minor edge while reported as meters.
- Stop if supersampling is used to make an illegal profile fit.
- Stop if generated normals overwrite imported normals before Stage 17 composition.
- Stop if the production route constructs full-frame CPU SDF/occupancy/Height planes, reads profile intermediates back
  for another pixel pass, or adds another GPU service/shader authority.
```

---

## Prompt LIB — reusable material-source, patch, stamp, and preset library

```text
Implement Prompt LIB from the full algorithm-stack prompt pack after Prompt 15 and before Prompt 16.

Read the common rules and the revised reusable-library contract. This is a cross-cutting product milestone, not a
new numbered image-algorithm stage. Consume no rendered atlas as library authority. No subagents.

Objective:
Give Hot Trimmer a durable, searchable, dependency-safe library for material source sets, authored patches, masks,
registered stamp channels, profile presets, and effect recipes, with a real management window feeding typed references
to source preparation, Stage 15, and Stage 16.

Execution split:
- Keep hashing, versioning, dependency checks, metadata/database transactions, path resolution, import validation, and
  command handling on the CPU; these are low-volume correctness work and gain nothing from GPU dispatch.
- Use the existing GPU service for pixel-parallel registered-channel preview, mask/SDF preview, thumbnail resampling,
  and atlas-sheet preview where it is materially beneficial. These previews consume the same channel interpretation
  and compact asset identity later used by compilation; they do not create renderer authority.
- Do not upload an entire library eagerly. Decode/upload/cache only visible or requested assets under the existing
  CPU/GPU memory budgets, and never read GPU previews back as authoritative library source pixels.

Scope — library contracts and storage:
- Define versioned LibraryAssetId, LibraryAssetVersion, content digest, LibraryAssetKind, StampAssetRef, provenance,
  license/source metadata, tags, category, author, default physical size/range, aspect policy, pivot, orientation,
  tileability, channel semantics, and preview metadata.
- Initially support MaterialSourceSet, SourcePatchPreset, LayoutPreset, StampMask, RegisteredStampChannels,
  StampSheet, ProfilePreset, and EffectRecipe while keeping the asset-kind enum extensible. LayoutPreset indexes the
  immutable Prompt 09V preset identity/tree rather than copying or recompiling topology. Material sources retain registered
  PBR channel identities/import settings; patch presets retain their authored crop/rectification/calibration lineage.
  A library item stores reusable source evidence and defaults, never executable compiler truth, cached analysis as
  primary data, or already-composited atlas pixels.
- Support user-global and project-local libraries. Projects pin immutable asset versions/content digests and may
  embed a portable snapshot; bare filesystem paths, filenames, thumbnails, and mutable latest-version pointers are
  not authoritative references.
- Import registered material sets, individual files, folders, and atlas/stencil sheets. Provide channel association,
  bounded connected-component/alpha segmentation, manual rectangles for sheet extraction, shared registered-channel
  cropping, pivot editing, physical-size/calibration defaults, mask polarity, and explicit linear/scalar/vector/color/
  exact-ID channel roles.
- Content-hash duplicates, preserve source/license provenance, generate bounded thumbnails asynchronously, and make
  replacement/version creation explicit. Never gamma-correct scalar masks or treat a JPEG normal as trustworthy
  without an explicit channel/convention declaration.
- Deleting or replacing an asset referenced by a project must be blocked or produce an explicit migration/embed
  choice. Missing assets remain typed unresolved references with recovery diagnostics; they never become blank or
  substitute stamps silently.

Scope — Hot Trimmer management window:
- Add a Library window with searchable/filterable thumbnail grid, kinds/tags/categories, detail and registered-channel
  preview, provenance/license, physical-size defaults, pivot/orientation, version history, project usage count, and
  dependency-safe import/edit/tag/duplicate/replace/delete commands.
- Provide atlas-sheet slicing with editable rectangles and a preview of each resulting item. All enabled controls
  invoke typed native commands, persist transactionally, observe cancellation/revision guards, and update stable
  references without UI-side compiler logic.
- Let layout selection, source selection, Stage 15 profile selection, and Stage 16 detail authoring browse/select
  items by compatible kind, but do
  not implement brush strokes, scattering, final PBR composition, export packaging, cloud sync, or marketplace work.
- Reuse the application-owned GPU device, source cache, cancellation generations, and raw tile publication for library
  pixel previews. Do not add a library-specific GPU service, shader authority, or Base64 pixel path.

Acceptance:
- Registered material-set, authored-patch, folder, single-mask, registered-stamp-channel, and atlas-sheet fixtures
  round-trip with stable IDs, content digests, metadata, mask polarity, pivots, physical defaults, and pixel
  registration.
- Global and project-local items can be searched and selected; embedded project snapshots reopen without the original
  source path, while unresolved external references report an actionable failure.
- Referenced assets cannot be destructively deleted or silently mutated, and editing creates deterministic versions.
- The Library window can author every initial asset kind through command-backed controls; no thumbnail or filename is
  used as compiler authority.
- Large/registered preview generation is bounded and GPU-backed when selected by policy; CPU work remains limited to
  decode/validation/metadata, and preview cache eviction cannot mutate pinned library identities.

Verification — run exactly:
cargo test -p hot-trimmer-desktop reusable_asset_library

Stop conditions:
- Stop if a project depends on a mutable absolute path or an unversioned filename.
- Stop if library deletion can alter an existing compile without an explicit migration decision.
- Stop if masks, normals, scalar maps, or color maps share one ambiguous decode path.
- Stop if this prompt implements raster brush painting or final channel composition.
```

---

## Prompt 16 — Stage 16: compiled semantic details and tiled GPU evaluation

```text
Implement Prompt 16 / Stage 16 from the full algorithm-stack prompt pack.

Read the common rules and revised Stage 16. Consume Stage 10 capacities, Stage 15 occupancy, and immutable Prompt LIB
asset references. No subagents.

Objective:
Compile material-independent semantic detail operations that remain physically valid across panels, strips, caps, and
radial slots, then evaluate reusable-atlas contributions as bounded GPU tile passes.

Scope:
- Define typed DetailDefinition/CompiledDetail contracts with physical range, scale space, compatible roles,
  orientation, aspect limits, minimum pixels, repeat period, contain/cover policy, channel contributions, fallback,
  provenance, seed, required source resources, required halo, operation dependencies, and deterministic GPU/cache
  identity.
- Define non-destructive StampOperation and StampStroke contracts: immutable asset/version reference, reusable-atlas
  versus asset-specific-deferred scope, target slot/region, physical transform, pivot, rotation, mirror, opacity,
  blend policy, clipping, seed, spacing/scatter/jitter, layer order, occupancy relation, and per-channel contributions.
  Store deterministic operation parameters or stroke samples, never pasted display pixels.
- Implement repeating strip, unique detail, radial detail, trim cap, bolt group, vent, panel stamp, groove, decal,
  procedural motif, and user-patch detail families.
- Keep role/fit/LOD/fallback selection, immutable asset resolution, layer intent, and compact deterministic operation/
  scatter compilation on the CPU. Do not run branch-heavy authoring policy independently in shaders.
- Upload immutable registered stamp/mask/channel assets through the existing digest-keyed GPU source cache. Implement
  registered mask-to-SDF conversion, stamp sampling, coherent bevel/groove/lip relief, procedural motifs, clipping,
  repetition, deterministic scatter evaluation, and Height/normal-input/scalar/color/ID contributions as tiled GPU
  passes. Keep their intermediates GPU-resident for Stages 18–19.
- Support planar and explicitly polar radial stamps without fisheye distortion by default. Preserve the stamp's
  physical aspect under rotation; any conformal/polar warp is an authored mapping mode with visible provenance.
- Implement physical motif periods and role-specific Surface/HorizontalStrip/VerticalStrip/Radial/TrimCap/Unique
  variants. Never squeeze an oversized motif to fit.
- Implement full, simplified Height/Normal, NormalOnly, Roughness/Color, and Disabled detail LODs with explicit
  variant/fallback selection and occupancy interaction.
- Distinguish source material-domain structure from semantic details supplied by template/user/procedural intent.
- Emit registered masks, physical Height, vector-normal inputs, scalar/color/ID contributions, and operation lineage;
  represent pixel fields as GPU-resident tile resources rather than CPU images, and do not flatten them into final PBR
  pixels. Material-ID contributions remain exact categorical writes, Metallic is legal only through explicit intent,
  and imported normal stamps retain their declared convention.
- Generate only dependencies needed by the requested final map or QA view. Declare halos for SDF, filtering, relief,
  scatter footprint, and derivatives; valid interiors must match across tile boundaries.
- Cache compilation separately from GPU evaluation. Changing selection or viewport does not recompile operations;
  changing an asset digest, physical transform, seed, algorithm/shader version, map, tile, or halo invalidates the exact
  affected descendants.
- Add detail route/occupancy QA views and docs/phase-reports/algorithm-stage-16.md.

Acceptance:
- Details preserve physical size, orientation, and repeat period across slot shapes and output resolutions.
- Panel bolt groups do not share the strip-rivet evaluator.
- Oversized/incompatible details choose a declared variant/fallback or fail; they never stretch.
- Stamp operations survive save/reopen and resolution changes with identical physical placement and seed. Library
  version changes are explicit invalidations; missing assets cannot compile as invisible success.
- A material-reusable stamp may compile into the atlas, while an asset-specific deferred stamp remains an operation
  for Stage 20/Blender and is not accidentally baked into every asset using that material.
- An empty detail list produces a valid SkippedBecauseUnused stage result.
- GPU tile goldens cover every detail family, registered channel alignment, deterministic scatter across tile edges,
  physical scale at 1K/2K/4K/8K, and declared float/packed tolerances. Exact IDs and plan identities remain exact.
- Production CPU detail/mask/SDF/stamp raster counters remain zero, no full-atlas CPU detail planes exist, and
  unrequested contribution passes dispatch zero work.

Verification — run exactly:
cargo test -p hot-trimmer-sheet-compiler algorithm_stage_16_gpu_details

Stop conditions:
- Stop if one square mask is resized into all slot roles.
- Stop if decorations modify topology by default.
- Stop if a user patch loses registered PBR correspondence.
- Stop if display-space brush coordinates become placement authority.
- Stop if asset-specific damage is baked into a reusable trim material without explicit scope.
- Stop if an intermediate is read back for CPU composition, a detail shader resolves raw user intent, or a second GPU
  asset/render cache is created.
```

---

## Prompt 20A — early Stage 15-16 product feedback workbench and Stage 15-20 telemetry

```text
Implement Prompt 20A from the full algorithm-stack prompt pack after Prompt 16 and before Prompt 18.

Read the common rules and the Stage 15, Prompt LIB, Stage 16, and Stage 20 contracts. Consume only the accepted Stage
15/16 compiler plans, GPU tile publications, persisted document commands, and existing source-first application shell.
No subagents.

Objective:
Start the product feedback cycle now by exposing typed structural-profile and semantic-detail authoring, raw compiler
contribution/QA previews, and one copyable Stage 15-20 diagnostic payload without inventing unfinished Stage 17-20
material results or creating a second renderer.

Scope — product feedback workbench:
- Unlock a deliberately limited Layers & Maps workbench inside the existing source-first shell. Label it as
  "Profile & Detail Contributions" until final Stage 17-19 composition exists; never present raw Stage 15/16 fields as
  a finished PBR material.
- Provide an in-app deterministic Stage 15-16 feedback project/sample that is created through normal typed project and
  document commands, uses owned registered assets, and can be saved, closed, reopened, and edited. Do not require SQL,
  hand-authored decoration JSON, hidden test hooks, or external fixture preparation to begin review.
- Let the user select a hotspot/region and author its Stage 15 structural profile using legal typed controls for Flat,
  Bevel, RoundedBevel, Groove, PanelFrame, RadialDisc, and Annulus where the slot role permits them. Show physical width/
  depth/radius inputs, selected role evaluator, occupancy semantics, LOD, fallback, and compilation failure from the
  authoritative compiled profile. Do not imply that a texture profile changes mesh silhouette.
- Integrate the existing Prompt LIB browser for immutable registered stamp/channel assets. Let the user create, select,
  edit, duplicate, enable/disable, reorder, and delete typed Stage 16 DetailDefinition, StampOperation, and StampStroke
  intent with undo/redo.
- Expose physical target region, family, size, position, pivot, rotation, mirror, opacity, spacing, deterministic seed,
  scatter/jitter, clipping/fit, mapping mode, blend policy, occupancy relation, layer order, per-channel amounts, exact
  Material ID, and explicit material-reusable versus asset-specific-deferred scope. Commit physical slot/atlas
  coordinates and immutable asset/version references; screen coordinates are transient input only.
- Provide direct-manipulation placement overlays for bounds, pivot, orientation, repeat period, stroke samples, valid
  interior, halo, and occupancy conflicts. Decorations remain non-destructive and do not modify topology by default.
- Persist authoring through typed/versioned IPC and domain commands. The frontend must not manufacture decoration keys,
  serialize Rust contracts by hand, resolve fallback/fit/LOD, or become placement authority.

Scope — previews and QA:
- Reuse the single persisted compiler, application-owned GPU service, digest-keyed source cache, requested-map graph,
  binary tile publication, revision guards, and cancellation path. Do not add preview-only material math, CPU raster
  composition, Base64/PNG atlas transport, a second GPU owner, or a second asset/render cache.
- Add explicitly labeled raw contribution views for Stage 15 occupancy/Height and Stage 16 registered mask, physical
  Height, vector-normal input, scalar contribution by semantic channel, Base Color contribution, exact Material ID, and
  Material-ID validity. Request only the visible map and its real dependencies.
- Add compiler-owned Stage 15 profile route/occupancy/LOD/fallback views and Stage 16 detail route/occupancy/LOD/scope/
  asset-resolution views. QA pixels come from requested GPU tiles; text and outlines come from compiled plan metadata.
  The UI must not reverse-engineer compiler truth from displayed pixels.
- Support 1K/2K/4K/8K review, before/after and selected-operation isolation, cache-hit visibility, and a clear distinction
  among current, compiling, cancelled, superseded, failed, missing-asset, deferred-only, and skipped-because-unused
  states. Never publish a stale generation as current.
- Keep hotspot/region selection independent from the active contribution/QA map and source-inspector channel. Selecting
  a region changes only selection overlays and inspector metadata: it must preserve the current map, must not force Base
  Color/Diffuse, and must schedule zero compile, render, upload, publication, or readback work.
- Retain current published contribution/QA tiles by exact document revision, preview profile, map/view, tile, halo,
  format, and generation identity. Switching back to an already-current view reuses its publication immediately;
  selection overlays do not invalidate pixel artifacts.
- Show Stages 18, 17, 19, and final 20 as explicit NotInstalled/Unavailable stages in this early workbench. Do not add
  guessed weathering, PBR composition, finishing, final-material, export, or Blender behavior to fill those gaps.

Scope — Copy Stage 15-20 telemetry + debug:
- Extend the existing "Copy telemetry + debug" interaction and F2 shortcut rather than creating a competing debug
  button. Rename its visible action to "Copy Stage 15-20 telemetry + debug" in this workbench and preserve concise
  copied/copy-failed feedback.
- Define one versioned, deterministic clipboard schema and human-readable summary. Later Prompts 18-20 must extend this
  schema compatibly rather than replace it. Use explicit InstalledNotRequested, Requested, Executed, CacheHit,
  SkippedBecauseUnused, DeferredOnly, Failed, Cancelled, Superseded, and NotInstalled states; absence is never ambiguous.
- Include app/build/protocol/schema versions; GPU adapter/backend/capabilities; project/document/topology/appearance/
  generation identities; selected region/hotspot and physical scale; requested map/QA view; tile output/valid rectangles,
  halo, format, row stride, opaque-handle identity, and publication/paint gates.
- For Stage 15 include requested and compiled profile identities, physical parameters, role/evaluator, occupancy, LOD,
  fallback/diagnostics, dependencies, requested/executed/cache-hit counts, timings, formats, residency, readback, and CPU
  profile-raster counters.
- For Stage 16 include definition/operation/stroke counts and identities, family, physical placement, scope, asset ID/
  version/digest, layer order, channel intent, mapping/fit/blend/occupancy, LOD/fallback/diagnostics, requested/executed/
  cache-hit counts, command/upload bytes, source/detail residency and pins, tile/halo/format/shader identities, readback,
  and CPU detail/mask/SDF/stamp raster counters.
- For Prompt 20A include its workbench/schema version, active tool and contribution view, selected/draft/committed
  operation identities, dirty/undo/redo state, before/after or isolation mode, last typed command result, and current
  authoring/preview error state.
- For Stages 18, 17, 19, and final 20 include the explicit current availability state and any installed version/identity;
  once those prompts land, add their plan routes, requested/executed/cache data, timings, residency, readbacks, CPU pixel
  counters, validation summaries, and publication status to the same payload.
- Include exact typed compiler/GPU/IPC errors, cancellation/supersession reason, preview-client paint summary, display
  gates, and the last bounded telemetry records needed to reproduce the selected view. Provide a one-click copy from
  both success and error states.
- Do not copy source pixels, encoded asset bytes, clipboard contents, credentials, environment variables, or absolute
  user paths. Prefer stable IDs, immutable content digests, project-relative labels, numeric measurements, and bounded
  diagnostics so users can paste the payload directly into a feedback report.

Scope — contracts and evidence:
- Add versioned IPC projections/commands for profile/detail authoring, compiled Stage 15/16 inspection, QA tile requests,
  and the Stage 15-20 debug payload. Keep Rust/TypeScript fixtures aligned and reject unknown or stale command versions.
- Add `stage15-20-feedback` to the desktop test runner. That focused suite must run the UI/IPC feedback-workbench tests
  and the native `algorithm_stage_20a_feedback_workbench` test under the single verification command below.
- Add docs/phase-reports/algorithm-stage-20a.md with the exact in-app review workflow, feedback sample contents,
  screenshots or deterministic view evidence, delivered limitations, and the handoff contract for Prompts 18-20.

Acceptance:
- From a clean launch, a reviewer can create/open the bundled feedback project, select a region, change a legal bevel or
  groove profile, place and transform a registered detail stamp, switch raw contribution/QA views, and see only the
  current GPU generation update without editing project internals.
- Profile and detail edits survive save/reopen with identical physical placement, seed, asset version/digest, scope,
  operation order, compiled identities, and requested-map pixels on the same supported backend.
- Review at 1K/2K/4K/8K preserves physical profile/detail scale and deterministic placement; selected-operation isolation
  and before/after do not mutate authoritative intent.
- Material ID 0 and its validity remain distinguishable; imported normal convention and registered channel alignment
  remain visible and exact through the raw contribution views.
- Missing assets, illegal role/fit, oversized motifs, occupancy conflicts, deferred-only operations, empty detail lists,
  cancellations, and superseded generations are visible typed states rather than blank previews or silent success.
- Repeating an identical visible request reports cache reuse and zero avoidable stamp upload/dispatch work. Unrequested
  Stage 15/16 contribution and QA passes dispatch zero work.
- A region remains selected while switching among all installed contribution/QA maps. Clicking a region in Normal,
  Height, mask, scalar, color, or ID view never switches to Diffuse and produces no preview request; returning to a
  current cached view performs no compiler/GPU work.
- The copied Stage 15-20 payload is available on success and failure, is deterministic apart from explicitly identified
  timings/runtime adapter data, contains enough identities and route evidence to reproduce the selected view, marks
  unfinished stages NotInstalled, and contains no source pixels, encoded assets, secrets, or absolute user paths.
- Production CPU profile/detail/mask/SDF/stamp raster counters remain zero, GPU intermediates remain bounded/tiled, and
  the early workbench introduces no alternate renderer, compositor, cache, exporter, or frontend material evaluator.

Verification — run exactly:
npm test --workspace @hot-trimmer/desktop -- stage15-20-feedback

Stop conditions:
- Stop if the workbench displays invented final PBR/weathering/finishing output before Stages 18, 17, and 19 exist.
- Stop if a frontend control writes raw decoration JSON, decides compiler fit/LOD/fallback, or commits display-space
  coordinates as physical authority.
- Stop if a QA/preview image is computed in React/TypeScript or on the production CPU instead of requested through the
  accepted GPU tile path.
- Stop if selecting a region changes the active map/source channel, clears a current publication, or triggers compiler/
  GPU work.
- Stop if telemetry requires SQL/log-file inspection, omits exact stage availability/cache/error state, leaks source
  pixels or private paths, or cannot be copied from a failed preview.
- Stop if Prompt 20A replaces the existing source-first shell or creates a renderer, cache, exporter, or debug schema
  that final Prompt 20 would need to discard.
```

---

## Prompt 18 — Stage 18: scale-aware effect compilation and tiled GPU evaluation

```text
Implement Prompt 18 / Stage 18 from the full algorithm-stack prompt pack before Prompt 17.

Read the common rules and revised Stage 18. Consume slot demand, profile/detail plans, masks, and material intent.
No subagents.

Objective:
Make one authoritative compiler resolve raw profiles, details, weathering, and material-state recipes into legal
role-specific `CompiledEffect` operations, then evaluate their pixel contributions through the existing tiled GPU pass
graph before Stage 17 PBR composition.

Scope:
- Implement EffectDefinition, EffectApplicability, EffectScale/ScaleSpace, EffectFallback, EffectOccupancy,
  EffectRoute, CompiledEffect, EffectPlan, and typed fallback reasons from the revised contracts.
- Keep scale conversion, physical fit, role compatibility, occupancy conflict resolution, feature-LOD selection,
  supersampling selection, dependency ordering, fallback choice, and compact command construction on the CPU. The
  compiler produces immutable WGSL-ready commands/resource tables; shaders never interpret raw recipes or decide fit.
- Enforce compilation order: scale space -> physical dimensions -> role compatibility -> physical fit/occupancy ->
  pixel dimensions -> feature LOD -> role evaluator -> supersampling -> compiled operation/failure.
- Generate registered region, exposed edge, distance, cavity, raised/recessed, direction, radial inner/outer,
  decoration, and material-group masks in physical units as bounded GPU tile passes. Reuse Stage 15/16 GPU-resident
  occupancy and contribution tiles; do not reconstruct them from readback pixels.
- Implement Grunge and EdgeWear variants for surface, horizontal strip, vertical strip, radial inner/outer, unique,
  and trim cap; implement strip anisotropy, radial coordinates, cap transitions, and world-direction influence.
- Implement Micro/Meso/Macro/Structural material state and user recipes Clean, Used, Heavy, Wet, Dusty, Chipped Paint,
  Rusting, and Mossy as typed bundles compiled separately per slot.
- Implement effect-family LOD ladders, deterministic placement, occupancy conflict resolution, 1x/2x/4x/8x
  supersampling, complete fallback diagnostics, and Clean/empty valid plans.
- Implement the compiled surface/strip/radial/cap weathering evaluators, masks, seeded noise/features, and channel
  contribution generation in the application-owned GPU service. Deterministic feature coordinates are derived from
  global slot/atlas physical coordinates so tile boundaries and scheduling order cannot move them.
- Compile profile, procedural detail, stamp, and decal operations into one deterministic dependency/layer plan. Resolve
  above/below/conform/clip/accumulate relationships against Stage 15 occupancy before rendering and record every
  conflict, suppressed contribution, library version, mask dependency, and fallback.
- Preserve the Stage 16 scope boundary: reusable-atlas operations may enter the material plan; asset-specific deferred
  operations remain referenced in the manifest for Stage 20/Blender and must not be rendered into the shared atlas.
- Validate channel legality and blend semantics during compilation, including exact IDs, bounded scalar channels,
  physical Height units, imported vector normals, alpha, and exposed-metal-only Metallic changes.
- Add effect operations and their dependencies to the existing requested-map graph. A requested map dispatches only
  effect operations that can contribute to it and their real prerequisites. Keep GPU intermediates resident for Stage
  17, use compact single-channel/vector formats, and read back only requested QA tiles or bounded route/occupancy
  summaries.
- Key CPU plans by exact intent/scale/library/version/seed inputs and GPU resources by plan/operation/map/tile/halo/
  format/shader identity. Declare operation-specific halos and prove seamless valid interiors.
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
- Layer-order fixtures prove that a stamp below a lip, a decal above a flat panel, and dirt conforming to a groove
  resolve differently but deterministically; deferred operations never appear in the reusable atlas.
- GPU/CPU-oracle fixtures prove compact-command parity, deterministic seeded placement across tile boundaries, exact
  operation/ID identity, declared numeric tolerances, zero unrequested dispatches, and zero production CPU effect
  raster calls.
- Effect evaluation stays within declared tile/halo/residency budgets and cancellation/revision checks prevent stale
  contribution tiles from entering Stage 17.

Verification — run exactly:
cargo test -p hot-trimmer-sheet-compiler algorithm_stage_18_gpu_effects

Stop conditions:
- Stop if one universal weathering evaluator handles all roles.
- Stop if raw recipe parameters can reach render-core without CompiledEffect.
- Stop if a fallback changes topology or silently changes physical scale.
- Stop if painter's-order pixels replace the typed operation/dependency plan.
- Stop if production weathering/mask/contribution pixels are generated on the CPU, GPU kernels choose applicability or
  fallback policy, or a contribution is read back for CPU PBR composition.
```

---

## Prompt 17 — Stage 17: requested-map GPU PBR estimation and composition

```text
Implement Prompt 17 / Stage 17 from the full algorithm-stack prompt pack after Prompt 18.

Read the common rules and revised Stage 17. Consume only Stage 14 registered material and Stage 18 compiled
operations; do not decide effect applicability here. No subagents.

Objective:
Render and compose imported, estimated, structural, detail, and weathering contributions into physically coherent
PBR channels with explicit provenance through the existing dependency-driven tiled GPU pass graph.

Scope:
- Compile material-class ranges, clamps, imported/estimated precedence, learned-provider choice, contribution order,
  normal convention, requested-map dependencies, formats, and provenance on the CPU into compact immutable PBR pass
  commands. CPU planning must not allocate or rasterize final/intermediate map planes.
- Implement physical Height composition with explicit amplitudes, material-class ranges, clamps, and contribution
  provenance as a float GPU tile pass. Do not add unrestricted normalized maps.
- Compose in declared semantic layer order. Structural profiles, SDF details, stamp relief, decals, and weathering may
  mask or conform to one another only through the Stage 18 plan; Stage 17 does not infer order from pixels.
- Generate normals on the GPU from final physical Height using Scharr derivatives divided by meters-per-pixel X/Y;
  declare and schedule the derivative halo so valid interiors match across tiles.
- Decode imported normals, combine them with generated/compiled contributions through reoriented or equivalent
  vector-correct normal mapping, renormalize, and re-encode in selected convention.
- Derive normals after all physical Height contributions are resolved, then vector-compose explicitly imported normal
  details. Never alpha-blend encoded normal RGB. Respect premultiplied/unpremultiplied color decal policy separately
  from linear scalar, exact ID, and vector channels.
- Prefer imported Roughness; otherwise estimate from class/base/contrast/high-frequency evidence and compiled effects
  with Estimated provenance and bounded ranges. Execute sampling/estimation/composition on GPU tiles while the CPU
  records the chosen route and provenance.
- Keep Metallic zero unless imported, explicitly labeled metal/material-ID-driven, or changed by exposed-metal effect.
- Implement multi-radius physical Height AO/cavity and compose compiled contributions as GPU passes with declared
  radius/halo and bounded multi-scale intermediates.
- Define a versioned local learned-map provider boundary; imported maps and explicit procedural structure take
  precedence. A provider may use the shared GPU when beneficial but returns a registered GPU resource/tiled source
  whenever possible so later stages do not require a round-trip. Unavailable routes explicitly use the compiled
  classical estimate/pass-through policy.
- Extend, rather than replace, the accepted requested-map graph. Deduplicate prerequisites and keep Stage 14 sampling,
  Stage 15 occupancy/Height, Stage 16 details, Stage 18 effects, final Height, Normal, Roughness, AO, and Metallic
  intermediates GPU-resident. Base Color alone runs none of these passes; Normal runs Height dependencies but not
  unrelated Roughness/AO/Metallic work.
- Use qualified float/single-channel/vector formats internally and pack only requested publication/export tiles.
  Cache every pass by exact dependency identity and record requested/executed/cache-hit/skipped/readback telemetry.
- Retain minimal CPU calculations only as small deterministic parity fixtures. Remove CPU production PBR raster and
  composition paths after GPU parity passes.
- Produce per-channel contribution/provenance QA views and docs/phase-reports/algorithm-stage-17.md.

Acceptance:
- Imported registered maps remain aligned and retain priority according to policy.
- Normal composition passes vector fixtures and contains no RGB-average path.
- Metallic never appears from Base Color alone without explicit allowed intent.
- Base-Color-only outputs label estimated channels and remain deterministic.
- Stamp/decal fixtures preserve registration, physical size, layer order, mask polarity, exact IDs, and normal-vector
  length across output resolutions.
- Requested-map telemetry proves no unrelated pass or readback ran. GPU tile seams pass for Height, Normal, Roughness,
  AO, Metallic, alpha, and imported/generated contribution boundaries at every required halo.
- Exact/categorical values and plan identities are exact; float/filtered cross-backend comparisons use declared fixed
  tolerances. Same hardware/backend and complete input tuple produce stable output hashes.
- Production CPU PBR pixel counters remain zero and 8K requested-map performance remains within the accepted GPU
  budget after the new algorithms are enabled.

Verification — run exactly:
cargo test -p hot-trimmer-sheet-compiler algorithm_stage_17_gpu_pbr_composition

Stop conditions:
- Stop if Stage 17 changes an effect route or fit decision.
- Stop if generated structure overwrites imported normals instead of composing.
- Stop if inferred channels lose Estimated provenance.
- Stop if an intermediate GPU map is read back for CPU composition, a frontend/preview shader computes material math,
  or all maps are generated when only one was requested.
```

---

## Prompt 19 — Stage 19: tiled GPU finishing, reduced validation, exact IDs, and metadata

```text
Implement Prompt 19 / Stage 19 from the full algorithm-stack prompt pack.

Read the common rules and revised Stage 19. Consume fully composed slot channels and plans. No subagents.

Objective:
Finish a registered atlas with correct bleed/filtering/mips, exact categorical maps, survival evidence, and complete
runtime metadata without materializing full-resolution CPU maps.

Scope:
- Extend the accepted GPU tile/halo/compact-ID infrastructure; do not create a second finishing path. Evaluate
  material/effects over allocation rectangles where possible and implement nearest-valid-pixel/jump-flood dilation for
  finite unique content as bounded GPU passes. IDs never bleed.
- Downsample compiled supersampled GPU tiles by channel: color-managed area Base Color, linear scalar filtering,
  decoded-vector normal filtering/renormalization, validated Roughness/Metallic, and nearest categorical IDs. Keep
  intermediates resident and publish/read back only requested final tiles or export batches.
- Implement channel-specific GPU filtering and mip generation. Region ID fills hotspot rectangles only using compact
  `R32Uint` indices plus the stable table, with no AA/dither/bleed/color transform; Material ID uses exact shared labels.
- Implement mip-survival measurement using GPU reductions/compares over requested tiles at mip 0/1/2 and the configured
  target range for bevel coherence, strip collapse, seams/bleed, simplified-effect strength, and ID exactness at mip
  0. Read back only bounded statistics/diagnostics, not the analyzed full maps.
- Compile finishing policy, requested mip ranges, exact ID tables, validation thresholds, manifest lineage, and tile/
  map scheduling on the CPU. Execute all pixel filtering, dilation, downsampling, mip generation, ID writes, and
  pixel-parallel survival analysis on the existing GPU service.
- Reuse Prompt 005 budgets, source/output tiling, staging pools, streaming encoder boundary, progress, cancellation,
  device-loss policy, and atomic finalization. Operation-specific halos and valid interiors must remain seamless at
  8K/16K/24K; no monolithic fallback is permitted even on permissive adapters.
- Produce complete manifest data: template/version/compatibility/topology hash, material revision, slot/allocation/
  hotspot/role/fit/world/radial data, map paths/colorspaces/checksums, and effect/LOD/supersampling summaries.
- Build the manifest/reports on the CPU from immutable plan identities, GPU tile manifests, bounded reduction results,
  and streaming output checksums. `CompiledSheet` references this lineage and tiled results; it does not own duplicate
  monolithic CPU pixel buffers.
- Produce deterministic detailed and concise compilation reports plus all Stage 19 QA views. QA pixels are requested
  GPU tiles through the same artifact route, never UI-side or CPU recomputation.
- Add docs/phase-reports/algorithm-stage-19.md.

Acceptance:
- All channels have identical authoritative dimensions/registration.
- Region/Material IDs are exact and categorical; Region ID covers hotspot, not allocation bleed.
- Supersampling changes raster quality but never physical dimensions.
- Survival validation catches disappearing and over-strengthened fixtures.
- Same inputs produce byte-identical maps, checksums, manifest payload, and report.
- Tile/monolithic-within-limit references agree at valid interiors for all maps/mips, bounded GPU reductions match small
  CPU oracle statistics, and exact IDs remain exact across tiles and mip-0 publication.
- Production CPU finishing/ID/mip/QA pixel counters remain zero; memory telemetry proves bounded CPU/GPU residency and
  unrequested maps/mips/QA passes dispatch no work.

Verification — run exactly:
cargo test -p hot-trimmer-sheet-compiler algorithm_stage_19_gpu_atlas_finishing

Stop conditions:
- Stop if normal downsampling treats encoded RGB as color.
- Stop if IDs are antialiased, bled, or mip-filtered at mip 0.
- Stop if a survival warning is computed from UI approximations rather than authoritative channels/plans.
- Stop if finishing allocates a full 16K/24K CPU map, duplicates the Prompt 005 scheduler/exporter, or reads GPU maps
  back for CPU dilation/filtering/mip/QA work.
```

---

## Prompt 20 — Stage 20: GPU-backed product workflow, QA, package publication, and Blender

```text
Implement Prompt 20 / Stage 20 from the full algorithm-stack prompt pack.

Read the common rules, revised Stage 20, and Prompt 20A report. Extend the accepted Prompt 20A authoring, QA, and
versioned Stage 15-20 telemetry contracts. Consume the Stage 19 `CompiledSheet` lineage/tile manifest through the single
`compile_persisted` GPU artifact and accepted streaming-export routes only.
No subagents.

Objective:
Expose the complete material compiler through the existing source-first UI, prove it on representative geometry,
publish it through the already-atomic GPU streaming exporter, and apply semantic slots correctly in Blender without
creating another renderer, map compositor, QA rasterizer, or exporter.

Scope — desktop workflow:
- Extend the Prompt 20A workbench in place. Preserve its typed Stage 15/16 commands, saved intent, review project,
  contribution/QA views, F2 copy interaction, and versioned Stage 15-20 debug schema; do not fork or replace them.
- Keep the established source library/canvas/sheet/inspector shell. Add typed controls for material-class override,
  physical-scale measurement, orientation, de-lighting, domain-route policy, seed, source sufficiency, placement pin/
  override, legal transforms, ExplicitStretch warning, material-state recipe, and effect fallback.
- Add staged cancellable analysis/domain/placement/compile/export jobs with revision guards, progress, cache reuse,
  bounded GPU tile preview refinement, and no partial authoritative publication. Scheduling commands remain CPU work;
  every pixel result comes from the existing GPU service and current compiled generation.
- Add all QA views from revised Stage 20: channels, IDs, crop usage, repetition heatmap, seam energy, texel density,
  effect route/occupancy/LOD/supersampling, mip warnings, plan/provenance, and Blender status.
- Every slot inspector shows selected source/domain/crop/transform/mode/cost, confidence, route, and fallback. UI does
  not recompute compiler truth. Pixel QA views request only visible GPU tiles; scalar summaries consume bounded GPU
  reductions or plan metadata.
- Integrate the Prompt LIB management window and browser into profile/detail/effect authoring. Provide stamp/splat
  tools that create typed Stage 16 operations with undo/redo, physical size, pivot, rotate/mirror, opacity, spacing,
  deterministic scatter/jitter, channel contribution preview, and explicit reusable-atlas versus asset-specific scope.
- Support 2D atlas placement and compatible 3D surface placement. Screen coordinates are transient input only; commit
  operations in atlas/slot physical coordinates or Blender geometry/UV anchors with a declared reprojection policy.
- Keep real silhouette rounding in geometry or a Blender bevel/displacement policy. Texture profiles may represent
  grooves, lips, seams, relief, and shading detail, but Hot Trimmer must never claim an atlas-allocation border has
  physically beveled the mesh.

Scope — persistent selection and per-region PBR controls:
- Preserve hotspot/region selection across Base Color, Height, Normal, Roughness, Metallic, AO, Specular, Opacity,
  Edge Mask, Region ID, Material ID, every QA view, and compatible 3D previews. Selecting a region must never change the
  active map, force Diffuse/Base Color, change the selected source channel, or schedule compiler/GPU work.
- Keep current map/QA publications cached by exact document revision, preview profile, map/view, tile, halo, format,
  generation, and dependency identity. Switching to an already-current view is immediate; keep the last valid pixels
  pinned while a genuinely stale dependency refines. Selection and inspector navigation are overlay/metadata changes,
  not appearance invalidations.
- Define a typed, versioned per-region PBR tuning intent with material-level defaults plus explicit inherit/override/
  reset behavior. Persist it through normal document commands with undo/redo and show provenance for every effective
  value. The frontend does not edit pixels or derive final map values.
- Expose physical Height amplitude/scale in meaningful units; generated-from-Height normal influence; imported-normal
  influence; Stage 16 detail-normal influence; Roughness estimated base/bias/range; AO/cavity strength and physical
  radii; opacity where legal; and explicit material class/Material-ID-driven metal intent. Use bounded material-class
  ranges and visible validation/fallback diagnostics.
- A friendly "Generated normal strength" control may drive the typed physical Height/derivative contribution, but it
  must not multiply encoded normal RGB. Imported and authored normal contributions use vector-correct composition and
  retain their convention. Show which part of the final normal came from Height, imported normals, details, and effects.
- Expose Stage 17's "Generate missing maps" policy per material and optionally per region. Prefer imported registered
  maps; otherwise allow the installed classical or local learned route for Height, Normal, and Roughness with explicit
  Estimated provenance, provider/version, confidence, range, and fallback. Generate AO/cavity from physical Height.
- Never infer Metallic merely from Base Color. Metallic remains zero unless imported, explicitly classified/labeled,
  selected by an exact Material ID rule, or changed by a legal exposed-metal effect. Make this restriction visible in
  the UI instead of presenting a misleading generic metallic-generation slider.
- Invalidate only the affected region and exact dependent maps: Height tuning may invalidate Height, generated Normal,
  and AO; imported/detail normal influence invalidates Normal only; Roughness tuning invalidates Roughness only; an
  explicit metal rule invalidates Metallic and its true dependents. Unrelated maps, regions, plans, and tiles remain
  reusable.

Scope — packed scalar-map import and channel extraction:
- Define typed, versioned `PackedChannelLayout`, `PackedChannelBinding`, and `ChannelSwizzle` contracts for immutable
  packed source containers. Include named ARM/ORM (R=AO/Occlusion, G=Roughness, B=Metallic), RMA, and MRA presets plus a
  custom R/G/B/A mapping. Provider naming is a hint only; always show the resolved component mapping before commit.
- Make packed-map expansion part of the normal `Add maps...` file-picker and drag/drop workflow, not a separate advanced
  tool and not three manual slot assignments. Recognize common filename/provider labels such as ARM, ORM,
  AO-Rough-Metal, OcclusionRoughnessMetallic, RMA, and MRA; group the packed file with the selected material set; and
  pre-populate every semantic child channel from the matching preset.
- For a confidently recognized ARM/ORM file, one `Add maps...` action must propose R=AO, G=Roughness, and B=Metallic and
  register all three together after one compact confirmation. The reviewer may inspect or correct the mapping, but must
  not click AO, Roughness, and Metallic slots individually. Remember an explicit provider/layout choice for subsequent
  imports without overriding contradictory filenames or metadata.
- Commit packed-source storage plus all accepted semantic bindings as one validated atomic project command and one undo/
  redo step. A failure in any requested binding publishes none of them; cancellation never leaves a partially populated
  material set.
- Let each component map to a legal linear scalar/mask semantic such as AO, Roughness, Metallic, Height, Specular,
  Opacity, or Edge Mask, or be ignored/filled by an explicit constant. Support an explicit invert transform for
  Smoothness/Glossiness-to-Roughness and display the resulting semantic preview. Do not reinterpret packed components
  as Base Color, tangent normals, or categorical IDs through this generic scalar path.
- Store or reference the encoded packed asset once under its immutable digest. Register semantic child views that point
  to the same source asset, component, transform, orientation, dimensions, and registration identity; do not duplicate
  encoded files or make users extract separate images. Decode/upload once through the accepted image/GPU source cache
  and expose independent semantic channels to downstream stages.
- Treat the packed file as an input container, never as internal material truth. After registration, Stage 14-20 plans
  consume separate typed AO/Roughness/Metallic/etc. channel views with shared correspondence. Packed RGB order must not
  leak into shaders, PBR composition, QA, manifests, or Blender logic.
- Treat packed data channels as linear regardless of container metadata. Preserve qualified EXR precision and PNG bit
  depth where supported. Allow JPEG only with a visible lossy-data warning and provenance because compression can alter
  scalar values and contaminate neighboring channels; prefer lossless PNG/EXR when the provider offers them.
- Preview each extracted channel independently before commit, including component name, inversion, value range,
  dimensions, orientation, digest, and registration alignment. Reject unsupported channel counts, inconsistent
  dimensions/orientation, non-finite values, or mappings that assign one component ambiguously.
- If standalone and packed sources both provide the same semantic channel, show both provenances and require an
  explicit precedence/replacement decision. Never silently replace an imported standalone map or create two active
  authorities for one material channel.
- Allow remapping/swizzling the immutable packed source without reimporting its bytes. Changing a binding invalidates
  only that semantic channel and its exact descendants. Save/reopen preserves layout preset/custom mapping, component,
  inversion, constants, digest, precedence, and registration identity.
- Separately support manifest-described ARM/ORM/custom packing as an export view when requested. Export packing combines
  authoritative semantic channels at publication time and never replaces their separate internal identities.
- Extend "Copy Stage 15-20 telemetry + debug" with active map and region-selection state, zero-work selection evidence,
  current/reused publication keys, effective per-region PBR tuning/provenance, packed source digest/layout/component/
  swizzle/inversion/precedence, filename/provider inference rule and confidence, automatic versus user-corrected mapping,
  atomic command result, decode/upload/cache route, precision/lossy warnings, and exact invalidated descendants. Do not
  include encoded pixels or absolute provider/user paths.

Scope — preview/export:
- Preview Plane, Cube, Cylinder, Beveled Block, Wall Module, Archway, Radial Disc, and Mechanical Prop, including
  several authored hotspot-UV fixtures. Preview consumes exported-equivalent GPU tile/map handles and conventions.
  Display geometry may sample compiled maps, but it never resolves crop/effect/material semantics or becomes a second
  production material renderer.
- Reuse the accepted Prompt 005 snapshot-based cancellable streaming export exactly. Stage 20 supplies the completed
  Stage 19 manifest/package metadata, map request set, overwrite intent, and publication destination; it does not
  implement another encoder, staging directory, atomic finalizer, or full-frame buffer.
- Keep requested QA/preview maps GPU-resident and publish through the binary tile manifest. Do not restore PNG/Base64,
  full-atlas preview readback, frontend material math, or CPU geometry-preview baking.

Scope — Blender companion:
- Load/validate manifest, create/update full Principled material, color spaces, normal, Height/displacement policy,
  Roughness, Metallic, AO, opacity as present, and material revision tracking.
- Describe selected UV islands/geometry, find compatible rectangular/strip/unique/cap/radial slots, fit without
  non-uniform distortion, preserve locked assignments, and report insufficiency/topology mismatch.
- Material/effect/resolution updates reload maps without remapping unchanged topology; topology changes require an
  explicit compatibility decision.
- Apply asset-specific deferred stamps as versioned Blender-side operations/decals or bake targets with stable anchors,
  explicit reproject/orphan diagnostics, and no contamination of the reusable material atlas.
- Add one root `check:algorithm-stage-20` script that runs the focused desktop Stage 20 tests and Blender companion
  fixture tests as one command. Do not hide unrelated workspace tests behind it.
- Add docs/phase-reports/algorithm-stage-20.md and user-facing workflow/diagnostic documentation.
- Add `CompiledTrimPackage` publishing to the library only after atomic Stage 20 export succeeds. Published packages
  pin the complete manifest/checksums and source recipe lineage; they never alias a mutable build directory.
- Remove any remaining product call edge to CPU slot/effect/PBR/finishing rasterization. UI, Blender coordination,
  metadata validation, encoding, and filesystem operations remain CPU work; material pixels remain GPU work.

Acceptance:
- The universal corpus can be compiled through the UI without material-specific workflows.
- Region selection works in every map/QA/compatible 3D view without changing the active map or source channel and
  produces zero compiler/GPU work. Returning to a current cached map is immediate and preserves the selected region.
- Per-region Height/generated-normal/imported-normal/detail-normal/Roughness/AO and explicit metal intent survive
  save/reopen, report effective inherited/overridden provenance, use vector/physical semantics, and invalidate only the
  exact affected region and dependent maps.
- Base-Color-only fixtures can publish explicitly Estimated Height, Normal, Roughness, and AO through installed Stage 17
  routes while Metallic remains zero until imported or enabled by explicit legal intent.
- ARM/ORM, RMA, MRA, and custom RGBA fixtures register correct independent semantic channels with byte/float oracle
  agreement, shared correspondence, one immutable encoded source, save/reopen stability, remappable swizzles, and exact
  descendant invalidation. JPEG fixtures show a lossy-data warning; PNG/EXR precision and provenance remain visible.
- Selecting a conventionally named ARM/ORM file through `Add maps...` populates AO, Roughness, and Metallic together in
  one confirmed atomic action and one undo step, with no per-channel slot clicks and no partial state after failure or
  cancellation. Ambiguous names open the same pre-populated mapping review instead of silently guessing.
- Standalone-versus-packed duplicate channels require an explicit precedence decision, and import/export manifests
  retain source digest, packing layout, component, inversion, precision, and semantic channel lineage.
- QA views are authoritative and explain every important crop/effect/fallback decision.
- Preview/export agree within channel tolerances.
- Blender fixtures map rectangular, strip, and radial semantics without non-uniform UV distortion; locks survive
  updates and map-only revisions reload without remapping.
- Failed/cancelled jobs and exports publish no partial result.
- Library-authored stamps round-trip through save, preview, export manifest, and Blender application; reusable and
  asset-specific scopes remain visibly distinct throughout.
- A successfully exported trim package can be published, searched, reopened, and applied from the Library without
  changing its pinned manifest or source asset versions.
- QA/map/geometry-preview requests prove requested-tile execution, revision safety, bounded payloads, zero frontend
  material math, and zero production CPU pixel counters. Cached map/QA switching does not recompile plans or read back
  unrelated maps.
- Atomic package export is demonstrably the Prompt 005 streaming route with Stage 19 data, and 16K/24K package creation
  retains its accepted bounded-memory/failure behavior.

Verification — run exactly:
npm run check:algorithm-stage-20

Stop conditions:
- Stop if enabled UI controls are not command-backed.
- Stop if selecting a region forces Diffuse/Base Color, changes the source channel, clears a current map, or schedules
  compilation/rendering/readback.
- Stop if "Normal Strength" multiplies encoded normal RGB, per-region controls bypass physical/vector composition, or
  Base Color silently creates Metallic without explicit legal intent.
- Stop if ARM/ORM naming is trusted without a visible component layout, packed data receives color management, JPEG
  loss is hidden, semantic channels remain dependent on packed RGB order downstream, encoded sources are duplicated per
  extracted channel, or standalone/packed precedence is chosen silently.
- Stop if a recognized packed map requires separate AO/Roughness/Metallic imports, produces more than one undo step, or
  can leave only some extracted channels registered.
- Stop if preview has a shortcut renderer different from final/export.
- Stop if Blender only calculates fit values without authoring and validating UV/material state.
- Stop if interactive splats are stored as flattened preview pixels or if screen coordinates become authoritative.
- Stop if Stage 20 creates another exporter/GPU service, computes QA or material pixels on the CPU/frontend, or turns
  `CompiledSheet` into a monolithic in-memory map bundle.
```

---

## Prompt 21 — Full GPU V1 qualification and remaining obsolete-path removal

```text
Implement Prompt 21: qualify the complete twenty-stage Hot Trimmer V1 algorithm stack on the accepted tiled GPU
production path and remove every remaining replaced algorithm, duplicate authority, and CPU production pixel route.

Read AGENTS.md, both governing algorithm documents, every algorithm stage report, the corpus manifest, and the
traceability matrix. Inspect current git status. No subagents.

Objective:
Prove the full product promise across behavior classes, source conditions, slot roles, output sizes, QA views,
export, and Blender. This prompt implements no new algorithm and creates no new acceptance criteria.

Preserve explicitly:
- `TrimSheetDocument`, `compile_persisted`, the immutable compiled-plan/compiler-executor boundary, the application-owned
  GPU service, compact command/source caches, requested-map pass graph, tiled artifacts, binary preview transport,
  streaming export, and Stage 20 package/Blender consumers.
- The smallest clearly test-only CPU parity fixtures needed to validate GPU math. They are never selectable at runtime.

Scope:
- Execute every revised section-30 acceptance criterion against the universal corpus and required geometry/effect
  fixtures at 1K, 2K, 4K, and 8K where specified, plus representative/full-product 16K and 24K tiled preview/export
  workloads from Prompt 005 after the Stage 15–19 algorithms are enabled.
- Close every traceability row with unit/property/plan/image/failure/performance evidence. A pass-through result is
  accepted only where the stage contract allows it and its reason is asserted.
- Measure named CPU planning phases separately from GPU upload/dispatch/cache/readback/publication/encode phases.
  Record 8K analysis/domain/placement/compile time, requested-map latency, cancellation latency, cache reuse,
  save/reopen, export, and Blender update, plus 16K/24K first-tile/total time and peak CPU/GPU residency on documented
  discrete and integrated hardware when actually available.
- Exercise malformed/bounded inputs, cache loss, low disk, cancellation, revision supersession, deterministic
  reruns, project crash recovery, atomic package publication, and offline operation.
- Delete only concretely identified obsolete paths: CPU production slot/atlas/profile/detail/effect/PBR/finishing/QA
  rasterizers, normalized-profile and universal-weathering evaluators, runtime semantic repacking, obsolete mappings,
  duplicated IPC/React material authority, superseded schema/migrations, temporary adapters, and dead dependencies.
  Do not delete or rename the accepted `sheet-compiler`/`compile_persisted` GPU orchestration merely because an older
  document called a prior implementation the legacy sheet compiler.
- Confirm one source document, one `compile_persisted` staged planning pipeline, one immutable plan lineage, one
  production GPU executor, one tiled `CompiledSheet`/manifest lineage, one binary preview path, and one streaming
  exporter.
- Add route counters/assertions proving every production pixel class—Stage 14 sampling, Stage 15 profiles, Stage 16
  details, Stage 18 effects, Stage 17 PBR, Stage 19 finishing/IDs/mips/QA—executes on GPU. CPU counters must remain zero
  outside explicit parity fixtures, encoding, and bounded summary/metadata work.
- Re-run tile-boundary, halo, deterministic seed, exact-ID, requested-map, device-loss, cancellation, cache-eviction,
  disk-failure, and atomic-publication evidence after the final algorithms are installed. Prompt 005 evidence produced
  before Stages 15–19 does not alone qualify their new passes.
- Update README, technical spec, architecture decisions, support diagnostics, algorithm version registry, Blender
  guide, known limitations, and docs/phase-reports/algorithm-stack-v1-qualification.md.

Acceptance:
- All twenty stage reports and traceability rows are green with no unimplemented route presented as complete.
- Universal invariants pass across all behavior classes and required slot roles.
- Same complete input tuple produces deterministic plans/routes/maps/reports.
- Source insufficiency and invalid effects are actionable and never silently distorted.
- Preview/export/Blender agree and locked geometry assignments survive material/effect/resolution updates.
- No live code can publish through the old engine or alter standard topology from appearance.
- 8K/16K/24K workloads stay within declared CPU/GPU budgets, generate only requested maps, publish no stale/partial
  artifacts, and contain no monolithic all-map allocation or GPU-to-CPU-to-GPU intermediate round-trip.
- Supported GPU failures are typed; unsupported hardware never invokes a CPU production renderer. Exact identities/
  categorical outputs are exact and float/filtered parity uses the fixed declared tolerances.

Verification — run exactly:
npm run check

Stop conditions:
- Stop rather than waive a failed revised acceptance criterion.
- Stop if a deleted legacy path still has a runtime caller.
- Stop if broad green tests conceal missing corpus/visual/Blender evidence.
- Stop if cleanup targets `compile_persisted`, the production GPU executor, tiled artifact/export infrastructure, or the
  test-only CPU oracle, or if qualification hides CPU pixel work behind a helper with a non-rendering name.
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
