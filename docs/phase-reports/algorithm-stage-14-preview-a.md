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

- The intermediate route uses Stage 2's typed maximum-2048-edge level-zero setting. Original bytes, digest,
  orientation, and registered dimensions are verified before reduction; the reduction is diagnostic, versioned,
  and cache-keyed, avoiding full-resolution float pyramids for this non-exportable gate.
- Automatic desktop preview requests are deduplicated by document revision. Superseded analysis stays silent while
  the newest revision continues, including React development double-effects and rapid resolution changes.

- The native `preview_through_stage_14` command invokes
  `AlgorithmCompiler::compile_persisted_stage_14_preview`. That compiler route consumes the persisted source set,
  authored patch geometry, and Stage 9 template and executes the installed Stage 1–14 implementations in order,
  including Stage 10 demands, Stage 11 candidates, Stage 12 scoring, and Stage 13 optimization.
- Content is resolved per region. Inherited/material regions build or reuse a full-frame Stage 3–8 domain with no
  patch claim; patch-bound regions build or reuse the domain rectified from that patch. Reported
  patch/domain/candidate/SamplingPlan lineage therefore describes the visible pixels for each slot.
- Authored crop window, addressing, isotropic scale, filter, tangent-normal correction, quarter-turn rotation,
  mirrors, offset, and radial parameters are carried into Stage 11–14. Unsupported warp, perspective, anisotropic,
  arbitrary-rotation, or mirrored-repeat policies fail explicitly instead of being discarded.
- Stage 11 receives bounded positions extracted from the Stage 7 saliency, stationarity, and edge fields. Stage 12
  receives candidate-local resolution, structure, orientation, seam, boundary, quality, and role measurements.
- Supersession cancels the Stage 2 image token, Stage 3–8 render token, and Stage 13 engine token, and is checked
  inside the Stage 10 region loop, Stage 11 candidate work, Stage 12 scoring, Stage 14 raster rows, and immediately
  before publication.
- The workbench action is labeled **Preview through Stage 14** and the canvas artifact is labeled
  **Intermediate Stage 14 material-placement preview**. Slot overlays expose mapping, validity, correspondence,
  selected patch/domain/candidate/SamplingPlan, and Stage 14 result identities.
- Only artifact-provided map views are enabled. Export and Blender remain disabled, and the artifact visibly lists
  profiles, semantic details, effects, final PBR composition, finishing, mips, metadata, export, and Blender
  application as pending.

## Focused evidence

The follow-up integration corrections select the authored rotation/mirror only from transforms Stage 11 originally
generated as legal, measure lattice completion from the selected crop origin/extent and executable period, supply
projection maps for every persisted material source set, and poll the live persisted document revision into the
active Stage 2–14 cancellation predicate.

The optional real-source acceptance route was exercised with the 7952×4016
`Texturelabs_Brick_120XL.jpg`; the same persisted native builder completed successfully.

`cargo test -p hot-trimmer-desktop algorithm_stage_14_preview_a`

The desktop test creates a real persisted project with aligned Base Color/Roughness sources and an authored patch
owned by Roughness. One region is explicitly patch-bound while the remaining regions inherit the primary material.
It invokes the same native preview builder as the command and verifies exact per-region lineage, non-exportability,
installed-stage provenance, Base Color publication, and distinct Stage 11–13 selected crops.

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
