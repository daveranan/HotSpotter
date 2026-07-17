# Algorithm Stage 14 Preview A — first visible authoritative atlas

## Delivered authority

- The sole `AlgorithmCompiler` now has an executable, explicitly intermediate publication route. Its typed
  `IntermediateAtlasRequest` carries Stage 9 topology, the complete Stage 13 `PlacementPlan`, successful Stage 14
  slot materials, source/domain/patch lineage, revision, installed versions, and diagnostics. The Prompt 00
  header-only method remains unable to claim pixels because a header is not executable input.
- `IntermediateAtlasArtifact` is separate from `CompiledSheet`, is always non-exportable, records
  `incomplete_after_stage = 14`, retains the exact Stage 9 topology value, hashes the exact PlacementPlan and each
  SamplingPlan/Stage 14 result, and lists all Stage 15–20 work as pending.
- Composition copies each allocation-local Stage 14 channel into its Stage 9 allocation rectangle. The compositor
  does not crop, resample, stretch, cover, repair validity, reinterpret correspondence, call the removed legacy
  renderer, or fabricate missing maps. A registered channel is visible only when every required slot actually has
  that channel; later-generated Height/Normal/Roughness/Metallic/AO remain unavailable when not imported.
- Required-slot, placement, topology, mode, domain, dimension, cancellation, and revision mismatches fail before an
  artifact is returned. The final revision check occurs after all slot work, so no failed, cancelled, or superseded
  request publishes a partial atlas.

## Persisted pipeline and desktop integration

- The native `preview_through_stage_14` command invokes
  `AlgorithmCompiler::compile_persisted_stage_14_preview`. That compiler route consumes the persisted source set,
  authored patch geometry, and Stage 9 template and executes the installed Stage 1–14 implementations in order,
  including Stage 10 demands, Stage 11 candidates, Stage 12 scoring, and Stage 13 optimization.
- The selected patch exemplar is the domain sampled by Stage 14, so reported patch/domain/candidate/SamplingPlan
  lineage describes the visible pixels. Supersession is observed during correspondence and channel raster rows,
  between slots, and immediately before publication.
- The workbench action is labeled **Preview through Stage 14** and the canvas artifact is labeled
  **Intermediate Stage 14 material-placement preview**. Slot overlays expose mapping, validity, correspondence,
  selected patch/domain/candidate/SamplingPlan, and Stage 14 result identities.
- Only artifact-provided map views are enabled. Export and Blender remain disabled, and the artifact visibly lists
  profiles, semantic details, effects, final PBR composition, finishing, mips, metadata, export, and Blender
  application as pending.

## Focused evidence

`cargo test -p hot-trimmer-desktop algorithm_stage_14_preview_a`

The desktop test creates a real persisted project with an encoded registered source and an authored patch, then
invokes the same native preview builder as the command. It verifies non-exportability, exact patch lineage,
installed-stage provenance, Base Color publication, and distinct Stage 11–13 selected crops across visible slots.

Invalid Stage 14 correspondence pixels remain transparent. Stage 14 result identities include every synthesized
channel's typed role, dimensions, metadata, and canonical pixel payload, in addition to correspondence, validity,
and diagnostics.

The Stage 14 prerequisite gate was rerun first:

`cargo test -p hot-trimmer-sheet-compiler algorithm_stage_14_slot_synthesis`

It passes with selected-seam execution and typed slice-center synthesis coverage.

## Deferred to 14P-B and later stages

The full invalidation matrix, broad corpus/mapping-family qualification, cache/performance hardening, heatmaps, and
rich decision exploration remain 14P-B work. Profiles, details, effects, generated PBR composition, finishing,
production preview QA, atomic export, metadata, mips, and Blender synchronization remain assigned to Stages 15–20.
