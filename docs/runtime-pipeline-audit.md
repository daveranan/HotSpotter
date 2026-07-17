# Hot Trimmer runtime pipeline audit

Audit date: 2026-07-17  
Scope: the actual desktop **Preview through Stage 14** path, with no runtime fixes or new algorithms.  
Companion files: `docs/runtime-pipeline-graph.md`, `docs/runtime-stage-matrix.json`, and `docs/runtime-integration-plan.md`.

## Executive finding

The repository has a real, continuous persisted-project path through algorithm Stages 1-14, but it stops at an explicitly incomplete material-placement atlas. It is not yet the requested product path. The current preview is an HTML image of imported channels sampled into Stage 9 allocation rectangles. Generated structural profiles, height, normal, roughness/AO, weathering, Region ID, final composition, PBR preview, export, and Blender application are absent from this route.

The two strongest causes of the observed result are:

1. **New documents treat template atlas coordinates as source-image crop constraints.** `TrimSheetDocument::from_pinned_template` copies every template `sourceMapping` into the region binding (`crates/domain/src/document.rs:927-961`, `1130-1145`). Stage 11 converts those normalized rectangles to source pixels and hard-filters candidates to them (`crates/sheet-compiler/src/persisted_pipeline.rs:169-181`, `356-414`). A raw photograph is therefore initially treated as if it were already arranged like the destination atlas.
2. **A crop-less `TextureSynthesis` plan can be selected for a `DirectSource` domain, but Stage 14 does not implement a synthesis branch.** Synthesis candidates carry `crop: None` (`crates/placement-solver/src/candidates.rs:410-424`). Stage 14 reports the requested mode as executed, yet `map_position` sends that mode through the generic centered-sampling fallback (`crates/sheet-compiler/src/slot_synthesis.rs:149-166`, `296-373`). In the measured run, the first five repeated strips all followed this path. This makes different slots sample similar centered bands while retaining different candidate IDs.

The 1024² measured run took **84.23 seconds** for one visible Base Color map. Of that, repeated source preparation/analysis consumed 59.47 seconds and Stage 13 consumed 8.49 seconds. This is a pipeline/cache/orchestration problem more than a raw slot-rendering problem: Stage 14 itself took 0.59 seconds.

## 1. Runtime entry and publication contract

The actual call graph and per-node runtime properties are in `docs/runtime-pipeline-graph.md`. The decisive points are:

- The button calls `build`, which invokes only `preview_through_stage_14` (`apps/desktop/src/source-first-app.tsx:621-652`, `1651-1704`).
- Tauri moves the work to `spawn_blocking` (`apps/desktop/src-tauri/src/document_commands.rs:1001-1013`).
- `build_stage_14_preview` calls `AlgorithmCompiler::compile_persisted_stage_14_preview`, not the old document renderer (`apps/desktop/src-tauri/src/document_commands.rs:1193-1227`).
- `compile_persisted` runs Stages 1-14 and then `compile_intermediate_atlas` (`crates/sheet-compiler/src/persisted_pipeline.rs:113-247`).
- Native code PNG-encodes every available common imported channel and returns base64 strings (`apps/desktop/src-tauri/src/document_commands.rs:1238-1260`, `1751-1761`).
- React selects one string and assigns it to an `<img>` (`apps/desktop/src/source-first-app.tsx:1668-1679`, `1737-1756`).

The older final map compositor is not silently participating. `compile_trim_sheet_document_impl` calls a façade method that always rejects Stage 1 (`apps/desktop/src-tauri/src/document_commands.rs:1274-1315`; `crates/sheet-compiler/src/algorithm_compiler.rs:47-73`). The incremental old preview is compiled out with `#[cfg(any())]` (`apps/desktop/src-tauri/src/document_commands.rs:1317-1400`).

## 2. Stage connectivity

The machine-readable matrix is `docs/runtime-stage-matrix.json`. In compact form:

| Stage/artifact | Exact object passed forward? | Actual 14P disposition | Status |
|---|---|---|---|
| Project/registered source | Reconstructed per domain | Source bytes reread/cloned | Recomputed downstream |
| PreparedChannelSet | Yes, to Stage 3 | Used | Fully connected |
| PreparedExemplar | Yes, to Stage 4 | Used | Fully connected |
| Stage 4 exemplar | Yes, to Stages 5-8 | Used | Fully connected |
| SourceAnalysisReport | Yes | Used for routing/scoring | Fully connected |
| ScaleOrientationReport | Yes | Used for demand/domain/scoring | Fully connected |
| FeatureFieldReport | Yes | Used, then repeatedly rescanned per crop | Recomputed downstream |
| PreparedMaterialDomain | Yes | Used, but route can disagree with selected mode | Blocked by contract mismatch |
| CompiledTemplateTopology | Yes | Used for atlas; UI regions separately resolved | Recomputed downstream |
| ResolvedSlotDemandSet | Yes to Stage 11; fields copied into Stage 13 input | Profile/effect fields ignored | Partially connected |
| CropCandidate/CandidateSet | Yes after mutation | Authored template crop hard-filtered; crop-less synthesis bypasses it | Connected but transformed incorrectly |
| ScoredCandidateSet | Yes | Exact top-K to Stage 13 | Fully connected |
| PlacementPlan/SamplingPlan | Yes | Exact plan to Stage 14 and 14P validation | Connected but transformed incorrectly |
| SynthesizedSlotMaterial | Yes | Exact result copied to allocation | Blocked by contract mismatch |
| StructuralProfileMaps | No | Not called | Connected only in tests/dead renderer |
| EffectPlan | No concrete compiler | No effects | Dead/unreferenced |
| Atlas channels | Imported common roles only | Base Color mandatory; Region ID absent | Preview-only approximation |
| Preview | Receives PNG strings, not native artifact | HTML `<img>` | Preview-only approximation |

### Reaches back to upstream state

- Region content and mapping are reread directly from `TrimSheetDocument.region_bindings` while building every slot (`crates/sheet-compiler/src/persisted_pipeline.rs:128-181`).
- The source is reloaded from `ProjectSummary.sources` for each unique patch/full-source domain (`crates/sheet-compiler/src/persisted_pipeline.rs:273-297`, `534-550`).
- UI overlay metadata is recomputed after atlas compilation by `registered_map_cached` plus `resolve_compile_plan`, rather than projected from `artifact.topology` and `artifact.slots` (`apps/desktop/src-tauri/src/document_commands.rs:1233-1250`).
- Template defaults become authoritative source mappings when a document is created (`crates/domain/src/document.rs:937-961`).
- There is no preview-specific mock pixel generator in the live path. The removed fabrication function is disabled at `apps/desktop/src-tauri/src/document_commands.rs:1015-1186`.

## 3. Data-lineage audit: one visible slot

The acceptance fixture was instrumented temporarily and then restored. The following is one actual 128² control-run lineage for `cornice_long`:

| Identity | Recorded value |
|---|---|
| Template slot key | `cornice_long` |
| Region ID | `59ca687e-5df2-970d-8ce5-2be7b22d0686` |
| Source/prepared-source digest | `07584d57250b51b28c9a866862992b62f3fd962e774f3f58f1ddf9f5e0ae7d88` |
| Patch ID | `eb7d6496-0a1c-4e49-a64b-7dfd27db6e3f` (ephemeral fixture instance) |
| Prepared domain ID | `3c72cbca05b78e930ae5222305c9383b1ce96beb2478b56db09d4a1447769b18` |
| Crop candidate ID | `3248321977a987d26fb159f1d1bb52d07147be7567acbbfa1cb7f70d2cebd7e3` |
| Selected placement ID | There is no separate placement ID; identity is the Stage 13 placement plan hash plus slot/candidate pair |
| Sampling plan hash | `adbaf59a5f2073f0626c0d15757fac4bbf62ec97eb2e7aef9f0670c08cc4988a` |
| Effect plan hash | None; no EffectPlan exists in 14P |
| Stage 14 rendered-slot hash | `10bb4759c38942d8ab807b525da39643de7ad404a43ddccda557994ecb9b67e2` |
| Selected source crop | `None` |
| Executed/reported mode | `TextureSynthesis` |
| Atlas destination rectangle | `(x=1, y=1, width=126, height=4)` |
| Hotspot rectangle | `(x=1, y=1, width=126, height=4)` after low-resolution boundary collapse |
| Preview mesh/island assignment | None; the preview is a 2D image |

The stable region ID is derived deterministically from template and compatibility keys (`crates/domain/src/document.rs:1199-1208`). Candidate IDs include slot ID, domain, crop, transform, family, position strategy, scale, and seed (`crates/placement-solver/src/candidates.rs:397-408`). Sampling plan and rendered-slot hashes are computed in 14P (`crates/sheet-compiler/src/intermediate_atlas.rs:159-186`, `209-235`). Identity is therefore not lost inside Stages 11-14.

Identity **is** weakened at publication:

- There is no standalone placement ID or demand ID in `Stage14SlotProjection` (`packages/ipc-contracts/src/document-contracts.ts:279-295`).
- `IntermediateAtlasArtifact.diagnostics`, `algorithm_versions`, `correspondence`, and `validity` are not projected through IPC (`crates/sheet-compiler/src/intermediate_atlas.rs:63-80`; `apps/desktop/src-tauri/src/document_commands.rs:1251-1260`).
- Native code zips `artifact.slots` with separately resolved regions by list order instead of joining on slot/region identity (`apps/desktop/src-tauri/src/document_commands.rs:1243-1250`). Current template validation makes stable order unique, but the contract is order-coupled.

### Duplicated-island investigation

There are no runtime mesh islands or UV buffers to duplicate. The GPU preview crate is empty apart from a constant (`crates/preview/src/lib.rs:1-3`), and the UI draws one atlas image plus region buttons (`apps/desktop/src/source-first-app.tsx:1737-1756`). The observed duplication is pixel-content reuse, not mesh duplication.

The checks resolve as follows:

- **Same slot assigned to multiple islands:** not applicable; no mesh.
- **Stable IDs regenerated:** no. Region IDs are deterministic and optimizer input rejects duplicate slot IDs (`crates/placement-solver/src/optimizer.rs:353-377`).
- **Index used instead of UUID:** domain selection and result construction use an index vector parallel to persisted region order (`crates/sheet-compiler/src/persisted_pipeline.rs:125-142`, `166-228`); publication also zips by order. This is fragile, but measured slot IDs stayed distinct.
- **Atlas rectangles duplicated:** template validation rejects overlapping allocations (`crates/domain/src/templates/mod.rs:395-404`). 14P writes each result to its Stage 9 allocation (`crates/sheet-compiler/src/intermediate_atlas.rs:147-179`).
- **Candidate/sampling plan copied across slots:** candidate IDs and plan hashes were distinct in the measured first five slots. However, all five chose `TextureSynthesis`, `crop=None`, so they can sample similar centered bands.
- **One rendered slot written to multiple destinations:** no such loop exists; each `IntermediateSlotInput` is written once to its matching topology slot (`crates/sheet-compiler/src/intermediate_atlas.rs:144-187`).
- **Stale texture/UV:** no UV buffer exists. React rejects preview objects whose revision/topology/map view do not match (`apps/desktop/src/source-first-app.tsx:1668-1679`), but HTML image decoding/upload is opaque and not explicitly keyed.

The direct duplication mechanism is Stage 13's legal selection of crop-less synthesis candidates combined with Stage 14's generic fallback. Different identities can therefore produce almost the same source footprint.

## 4. Crop and sampling audit

### Where authored crop becomes pixels

The persisted `Projection::Crop` is normalized. `mapping_window` converts it to prepared-domain pixels using floor for origin and ceil for extent (`crates/sheet-compiler/src/persisted_pipeline.rs:417-425`). `apply_authored_mapping` then shifts/filter candidate pixel crops to that window (`356-414`). Thus:

- authored UI/template crop coordinates: normalized `[0,1]`;
- `SourceCrop` inside candidates/plans: integer prepared-domain pixels;
- Stage 14 correspondence: prepared-domain texel-boundary coordinates, with pixel centers at `N + 0.5` (`crates/sheet-compiler/src/slot_synthesis.rs:460-476`).

Stage 2 enforces common oriented dimensions and Stage 3 uses one coordinate field for every registered channel (`crates/image-io/src/normalization.rs:481-489`; `crates/render-core/src/registered_rectification.rs:313-374`). Stage 14 computes one `positions` vector and samples every channel through it (`crates/sheet-compiler/src/slot_synthesis.rs:129-147`, `432-447`). Cross-channel transforms are therefore consistent when channels survive to 14P.

### Actual mode support by slot type

Stage 10 offers the following modes (`crates/effect-compiler/src/stage10.rs:517-528`):

| Slot role | Modes offered | Stage 14 behavior |
|---|---|---|
| Planar | DirectCrop, PeriodicTile, TextureSynthesis, NineSlicePanel | DirectCrop uses generic centered physical sampling; PeriodicTile wraps; NineSlice has a branch; TextureSynthesis falls through |
| RepeatingStrip | RepeatX/RepeatY, DirectCrop, TextureSynthesis | Repeat axis wraps and clamps cross-axis; DirectCrop and TextureSynthesis fall through |
| UniqueDetail | UniqueContain, UniqueCover | Explicit aspect-preserving branches |
| TrimCap | ThreeSliceCap, DirectCrop | Three-slice branch or generic centered sampling |
| Radial | PlanarRadial, PolarRadial | Explicit radial branches |

There is no non-uniform stretch unless a separately authorized `ExplicitStretch` plan exists. `UniqueContain` preserves aspect and invalidates letterbox pixels; `UniqueCover` preserves aspect and crops; repeating modes preserve one physical scale and wrap one/both axes (`crates/sheet-compiler/src/slot_synthesis.rs:296-373`).

### Worked measured example

Representative run facts:

- raw source: `1929×1033`;
- Stage 3 patch domain used by `cornice_long`: `1736×930`;
- output atlas: `1024×1024`;
- allocation: `(8, 8, 1008, 32)`;
- hotspot: `(10, 10, 1004, 28)`;
- authored template crop: `(0.007813, 0.007813, 0.984375, 0.03125)` from `assets/templates/generic_architecture/1.0.0/template.json`;
- corresponding authored pixel window: approximately `(13, 7, 1709, 30)` in the prepared patch domain;
- selected candidate: `TextureSynthesis`, `crop=None`, isotropic scale `1.0` by construction (`crates/placement-solver/src/candidates.rs:410-424`);
- `crop(None)` substitutes the full domain `(0, 0, 1736, 930)` (`crates/sheet-compiler/src/slot_synthesis.rs:292-294`);
- Stage 10 world size: `12.6×0.4 m`; relative base scale is `min(1736/12.6, 930/0.4) = 137.777… px/m` (`crates/sheet-compiler/src/persisted_pipeline.rs:581-585`);
- generic Stage 14 transform: `p = domain_center + source_local × 137.777…` (`crates/sheet-compiler/src/slot_synthesis.rs:296-308`, `348-350`);
- destination pixel centers therefore sample almost the full X range and approximately source Y `438.3…491.7`, a centered band about 54 pixels high;
- atlas destination is the full allocation `(8,8)-(1015,39)`, not the hotspot.

This preserves the requested world/destination aspect (about `31.5:1`) as a centered cover-like crop. It does **not** consume the authored 30-pixel-tall source window and it does **not** perform synthesis despite the reported mode. The apparent crop is therefore real but not the crop the UI/template contract says was selected.

### Draft crop interaction defect

During a drag, `SourceCanvas` calls `onDraftCrop` once per animation frame (`apps/desktop/src/source-first-app.tsx:1309-1321`). `previewSelectedCrop` constructs a transient projection and passes it to `requestPreview` (`720-727`), but `requestPreview` ignores both `regionId` and `projection` and invokes native code with only the persisted revision (`702-709`). Every drag frame can launch/cancel a full compile while rendering the old crop. Only pointer release persists the crop through `setSelectedCrop` (`683-699`, `1343-1347`). This directly explains “does not visibly crop” during interaction and causes cancellation churn.

## 5. Trim/profile and channel audit

### Implemented profile path

`compile_structural_profile` generates hotspot-local height and tangent-space normals. It calculates distance to the nearest rectangular edge for bevel/groove/rounded/frame profiles, or radius for radial profiles (`crates/render-core/src/structural_profile.rs:189-250`, `333-373`). Normal derivatives are one-sided at hotspot boundaries and OpenGL/DirectX changes only the Y sign (`217-242`).

The old document compositor would:

1. compile the profile for `resolved.hotspot_bounds`;
2. replace Height and Normal in the hotspot;
3. modulate Base Color;
4. synthesize roughness, metallic, and AO from cavity;
5. dilate hotspot pixels into allocation padding.

That code is at `crates/sheet-compiler/src/document_compiler.rs:670-705`, but its final caller is unused and its preview command is compiled out. Stage 14P never calls it.

### Visibility in Stage 14P

| Output | Exists in code | Generated in 14P | Bound/displayed |
|---|---:|---:|---:|
| Hotspot/profile mask | Implicit in `profile_height` | No | No |
| Distance-to-edge | Yes | No | No |
| Structural height | Yes | No | No; only imported Height could survive |
| Derived structural normal | Yes, OpenGL/DirectX | No | No; only imported Normal could survive |
| Material height composition | No authoritative composition in 14P | No | No |
| Roughness/AO from profile | Legacy approximation exists | No | No |
| Edge wear/chips/weathering | No EffectPlan implementation | No | No |
| Region ID | Old compositor can paint it | No | Map button remains disabled |

Profiles are not overwritten; they are never created. Effects are not below pixel resolution; no effect commands exist. The preview does not bind a PBR material; map buttons merely swap the `<img>` source (`apps/desktop/src/source-first-app.tsx:1794-1812`). Imported normals preserve canonical convention through Stage 2/3 and are rotated/mirrored as vectors by Stage 14 (`crates/sheet-compiler/src/slot_synthesis.rs:474-487`), but there is no lighting surface on which to judge them.

The current Stage 14 result is rendered to **allocation** dimensions (`crates/sheet-compiler/src/persisted_pipeline.rs:218-220`) and 14P copies it to the allocation rectangle (`crates/sheet-compiler/src/intermediate_atlas.rs:149-179`). Hotspot bounds are inspection metadata only. Any later profile/effect integration must explicitly establish whether source material fills allocation while structural/effect masks use hotspot; today that distinction is lost during rendering.

## 6. Performance profile

### Method

A temporary timing probe was added around existing calls, the existing persisted Stage 14 acceptance fixture was run, and the probe was removed. No algorithm or production behavior was changed. The representative input was an existing `1929×1033` PNG, one Base Color channel, the 53-slot Generic Architecture template, one patch-backed slot plus inherited full-source slots, and a `1024×1024` output. Debug/test build overhead was already warm; the measured test body finished in 84.94 s and native projection finished in 84.23 s.

### Timings

| Operation | Time |
|---|---:|
| Project snapshot/loading | 60 ms |
| Source registration/byte map | <1 ms per domain, excluding blob clone cost below timer resolution |
| Stage 2 source decode/normalization | 1,878 ms patch domain + 1,874 ms full-source domain = 3,752 ms |
| Stage 3 source preparation/rectification | 3,725 + 4,586 = 8,311 ms |
| Stage 4 de-lighting | 15 + 19 = 34 ms |
| Stage 5 analysis | 2,026 + 2,469 = 4,495 ms |
| Stage 6 scale/orientation | 645 + 798 = 1,443 ms |
| Stage 7 feature fields | 18,543 + 22,566 = 41,109 ms |
| Stage 8 domain route/preparation | 86 + 110 = 196 ms |
| All domain work, including overhead | 59,471 ms |
| Stage 9 template resolution | <1 ms |
| Stage 10 demand generation | 1 ms |
| Stage 11 candidate generation/filter | 388 ms |
| Stage 12 scoring | 30 ms |
| Stage 13 global placement | 8,485 ms |
| Sampling-plan compilation | Included in Stage 13 plan construction; no distinct compiler/timer |
| Effect-plan compilation | 0 ms; no EffectPlan is compiled |
| Stage 14 slot rendering | 586 ms |
| 14P atlas composition + result hashes | 2,405 ms |
| Compiler total | 83,381 ms |
| Post-compile source decode/metadata resolution | 151 ms |
| PNG encoding + base64 | 462 ms |
| Native end-to-end | 84,231 ms |
| JSON IPC, browser decode/GPU upload, first paint | Not measurable in the headless acceptance fixture; no application telemetry exists |

### Counts and resources

| Metric | Observed |
|---|---:|
| Raw source resolution | 1929×1033 |
| Prepared domains | 1736×930 patch; 1929×1033 full source |
| Output resolution | 1024×1024 |
| Slot count | 53: 25 repeating strips, 20 unique details, 4 radial, 4 trim caps |
| Candidates after authored filtering | 2,022 |
| Stage 12 retained upper bound | 53 × 16 = 848 |
| Placement count | 53 |
| Effect count | 0 |
| Emitted maps | 1 Base Color |
| Encoded IPC image payload | 2,116,522 characters, before JSON object overhead |
| Image decodes | At least 3 on a cold preview: patch domain, full-source domain, post-compile metadata decode |
| Cross-preview cache hits | 0 for Stages 2-14 and encoded atlases; post-compile `decoded_sources` can hit after first run |
| Thread usage | One Tauri blocking worker doing serial compute; one 20 ms revision monitor; one 2 ms cancellation monitor; WebView thread later decodes/displays |
| Peak memory | Not captured; Stage 2 exposes `peak_declared_bytes` but the pipeline discards it from diagnostics |
| Minimum atlas-scale live buffers | Per emitted map RGBA plus full correspondence (`f32×2`) plus validity; PNG call clones RGBA and allocates PNG/base64. Per-slot correspondence/validity/channel planes are also retained until composition completes |

### Repeated work

- The same source was decoded, rectified, analyzed, and feature-extracted twice because `source_set|patch_id` creates separate domains. That distinction is valid for rectification but does not justify repeating all full-source decode and upstream pyramids.
- A new `MaterialDomainCache::default()` is created inside every `build_domain` (`crates/sheet-compiler/src/persisted_pipeline.rs:337-338`).
- `ProjectSession` already owns prepared-source, exemplar, source-analysis and scale/orientation caches, but this route never borrows them (`apps/desktop/src-tauri/src/document_commands.rs:59-89`).
- Candidate scoring scans feature fields across each candidate rectangle (`crates/sheet-compiler/src/persisted_pipeline.rs:446-512`) instead of using integral summaries.
- Stage 11 builds an unusable-pixel integral image per slot, although slots sharing a domain/window could share it (`crates/placement-solver/src/candidates.rs:326-337`).
- Stage 14 renders slots serially (`crates/sheet-compiler/src/persisted_pipeline.rs:212-222`).
- `slot_result_id` hashes every correspondence, validity value and channel pixel, then atlas composition rereads those pixels (`crates/sheet-compiler/src/intermediate_atlas.rs:159-215`).
- After the compiler already has topology and slot inspection, native code decodes source data again solely to call `resolve_compile_plan` for UI regions (`apps/desktop/src-tauri/src/document_commands.rs:1233-1250`).
- `png_data_url` receives `channel.rgba8.clone()`, PNG-encodes, then base64-expands before JSON IPC (`1238-1242`, `1751-1761`).
- Every animation-frame crop draft can supersede and restart this full operation even though the draft projection is ignored.

### Explicit performance questions

- **Final export resolution?** Yes. Stage 9 uses `document.render_settings.output_size` directly; there is no preview-resolution mode (`crates/sheet-compiler/src/persisted_pipeline.rs:146-151`).
- **Every map when only Base Color displayed?** It processes every imported channel required/common to all slots, independent of selected map view. It does not generate missing maps.
- **PNG before preview?** Yes, for each available channel.
- **Full buffers through JSON IPC?** Yes, as base64 data URLs.
- **Source analysis per slot?** Domain analysis is shared within the invocation by `source_set|patch`, not per slot; it is repeated per patch/full-source domain and per preview.
- **Candidates per channel?** No; candidates are material-domain based once per slot.
- **Serial independent slots?** Yes.
- **Synchronous disk writes?** No writes in the preview hot path; external source reads and SQLite reads are synchronous on the worker.
- **UI thread blocked?** Not synchronously, but no result is displayed until the entire authoritative run, encoding, IPC, browser decode, and React update finish.
- **Waits for refinement before first preview?** Yes; there is no coarse artifact or progressive update.

## 7. Preview architecture audit

Stage 14P is best classified as **a preview-specific publication path that consumes Stage 14 material pixels but diverges from final composition**, not as a final compiled-atlas consumer.

1. It does not consume final maps; final compile is explicitly unavailable (`packages/ipc-contracts/src/document-contracts.ts:297-315`).
2. It does not independently re-run algorithm stages in React, but native code re-resolves UI regions after compilation.
3. It does not display legacy layout state; it displays Stage 9/14P pixels. The old preview command is disabled.
4. It is a preview-only render/publication path because profiles/effects/final PBR composition are marked pending (`crates/sheet-compiler/src/intermediate_atlas.rs:196-205`).
5. Revision/topology checks reduce stale display risk, but there is no explicit browser upload telemetry or texture cache key.
6. It has no preview mesh or UVs; therefore “incorrect UVs” cannot be the present defect.

### Actual preview contract

```text
IntermediateAtlasProjection {
  label, nonExportable, incompleteAfterStage,
  revision/documentRevision,
  topologyHash/appearanceHash,
  width/height,
  topology: unknown,
  placementPlanId,
  maps: Partial<Record<CompiledMapView, base64-PNG-data-URL>>,
  regions: ResolvedRegion[],
  unavailableChannels: string[],
  slots: Stage14SlotProjection[],
  pending,
  finalCompileAvailable/exportAvailable/blenderAvailable: false
}
```

Compared with the desired `CompiledPreviewArtifact`, it is missing generated Height/Normal/Roughness/Metallic/AO, Region ID, native handles, diagnostics, correspondence/validity, a demand/effect identity chain, and any mesh/material binding contract. It carries the complete topology value but React treats it as `unknown` and uses separately resolved `regions`. The gap is substantial: the shape is roughly halfway to a useful artifact envelope, but only Base Color pixels and slot inspection are convincingly populated in the common one-source case.

## Audit conclusion

The repository is not fourteen disconnected prototypes: Stages 1-14 really do execute in order. The failure is that this vertical slice terminates before the visible product-defining stages, and two contracts inside the existing slice disagree. The next work should connect/correct existing artifacts and establish a preview-resolution compiled artifact; it should not add another renderer or parallel pipeline.
