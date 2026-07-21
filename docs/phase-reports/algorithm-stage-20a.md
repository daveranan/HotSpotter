# Algorithm Prompt 20A — Stage 15-16 feedback workbench

## Delivered product loop

The existing source-first desktop shell now unlocks **Layers & Maps** as the deliberately limited **Profile & Detail Contributions** workbench. It does not claim a finished material. Stages 18, 17, 19, and final 20 are always shown as `NotInstalled / Unavailable`.

Exact in-app review workflow:

1. Start Hot Trimmer on its empty draft and open **Layers & Maps**.
2. Choose **Create bundled feedback sample**. This imports the deterministic owned registered source through the normal project store, creates the source-frame document, then commits typed Stage 15 and Stage 16 commands.
3. Select a hotspot/region. Choose a legal Flat, Bevel, Rounded Bevel, Groove, Panel Frame, Radial Disc, or Annulus program. Enter physical width/depth/radius and choose **Commit typed profile**.
4. Inspect the compiler-owned evaluator, occupancy, LOD, fallback/diagnostic, and cache identity. Texture profiles are explicitly labeled as unable to change mesh silhouette.
5. Select the immutable registered asset in the Prompt LIB section, create a `DetailDefinition`, then place a `StampOperation`. Select operations to edit physical position/size/pivot/rotation/mirror/opacity/spacing/seed/scatter/jitter/fit/blend/occupancy/layer/channel/Material-ID/scope intent. Duplicate, enable/disable, delete, reorder through typed commands, and use the existing document undo/redo.
6. Select a raw contribution or metadata QA view and 1K/2K/4K/8K review profile. Pixel views issue one versioned request through `preview_stage_15_16_feedback`; metadata-only QA views dispatch no pixel work. Before/after and selected-operation isolation are display requests and never mutate intent.
7. Press **F2** or the existing footer action, now labeled **Copy Stage 15-20 telemetry + debug** while this workbench is open. Copy remains available when the current preview failed or no tile is paintable.
8. Save, close, and reopen normally. Profile requests, immutable asset/version/digest, physical placement, deterministic seed, scope, order, and command history are document-owned state.

## Bundled deterministic sample

The sample contains a generated 64×64 owned Base Color source with a stable visual grid, a source-frame document, a 4 mm / 2 mm 45-degree bevel request, one registered-channel `DetailDefinition`, and one 30 mm registered stamp. The stamp uses physical position, pivot, 15-degree rotation, deterministic seed `201520`, reusable-atlas scope, Height contribution, and exact Material ID `0`. Material ID validity remains a separate view.

No SQL, decoration JSON, external fixture, or hidden test hook is needed. Rust owns persisted decoration keys and serialization; the frontend sends typed version-1 commands only.

## Deterministic view evidence

- `Stage14SlotProjection.compiledProfile` and `.compiledDetails` are copied from the exact persisted `CompiledAtlasPlanV1` region commands that drive GPU execution.
- Pixel views return the existing binary GPU tile publication with generation, output/valid rectangles, halo, format, row stride, and opaque handle. The existing revision and cancellation gates reject stale publications.
- Route/occupancy/LOD/fallback/scope/asset-resolution text is rendered from compiled metadata, not inferred from pixels.
- `apps/desktop/src/stage15-20-feedback.test.ts` fixes view-to-dependency routing, legal role/profile vocabulary, explicit unfinished-stage availability, F2 integration, and absence of frontend raster operations.
- Native `algorithm_stage_20a_feedback_workbench` evidence fixes command/schema versions, native key ownership, legal-role rejection, explicit state vocabulary, and private-path redaction.

## Stage 15-20 debug schema

Schema `hot-trimmer.stage15-20-feedback`, version 1, is deterministic except for explicitly carried runtime timings and adapter data. It contains app/build/protocol/schema versions; safe project/document/revision/hash identities; selected region and compiled physical scale; requested view/profile/comparison; tile publication and paint gates; authoritative Stage 15/16 compiled inspection; intent counts; CPU raster counters; workbench tool/selection/dirty/undo/redo/command/error state; exact typed failures; and the last 32 bounded telemetry records.

The native sanitizer removes source pixels, encoded bytes, byte payloads, clipboard/credential/environment content, and absolute user paths. Project labels are basename-only. Stages 18, 17, 19, and 20 have explicit `NotInstalled` records instead of absent keys.

## Delivered limitations

- This is contribution inspection through Stage 16, not weathering, PBR composition, finishing, final material preview, export, Blender application, or geometry beveling.
- Prompt 20A uses the existing application-owned GPU service, cache, compiler, tile publication, and cancellation/revision route. It adds no renderer, compositor, CPU rasterizer, source cache, exporter, or frontend material evaluator.
- Metadata QA views intentionally request no pixels. Pixel contribution views request only their real map dependency.
- Asset-specific operations remain `DeferredOnly`; they are preserved for final Prompt 20/Blender and are not baked into the reusable atlas.

## Handoff for Prompts 18-20

Prompts 18, 17, 19, and 20 must extend this workbench and schema in place. They must preserve command version 1 compatibility, saved Stage 15/16 intent, bundled sample, contribution views, F2 interaction, explicit state vocabulary, and sanitizer. Each installed stage replaces only its own `NotInstalled` record with plan routes, request/execution/cache counts, timings, residency/readback/CPU counters, validation, and publication status. Final Prompt 20 must not fork the shell, renderer, cache, exporter, or debug payload.

Focused verification:

```text
npm test --workspace @hot-trimmer/desktop -- stage15-20-feedback
```
