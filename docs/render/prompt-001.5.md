# Render Prompt 001.5 - Make the executor own complete Base Color composition

Implement this prompt only. This file is self-contained. Do not read any other documentation, search for historical
reports, retrace the UI pipeline, inspect unrelated stages, or start GPU shaders.

## Why this exists

Prompt 1 successfully added `CompiledAtlasPlanV1` and routed Stage 14 region synthesis through
`CpuAtlasRenderExecutor`. A genuine 7952 x 4016 source -> 8192 x 8192 Base Color benchmark measured:

- Cold: 24.904 s total = 3.370 s decode + 8.802 s region synthesis + 12.728 s atlas composition.
- Warm: 13.401 s total = 0 ms decode + 0 ms region synthesis + 13.398 s atlas composition.
- Warm region cache: 64/64 hits.
- Peak RSS: 5,500,157,952 bytes.
- Artifact allocation: 1,946,157,056 bytes.

The executor currently returns rendered regions. `compile_source_frame` then constructs
`IntermediateAtlasRequest` and directly calls `AlgorithmCompiler::compile_intermediate_atlas`. That leaves the
dominant pixel pass outside the executor. Prompt 2 must replace Stage 14 sampling and Base Color destination
composition together, so this prompt moves the existing CPU composition call behind the executor contract without
changing output.

## Edit boundary

Edit only these files unless the focused test proves one directly required adjacent export/import:

- `crates/sheet-compiler/src/atlas_executor.rs`
- `crates/sheet-compiler/src/persisted_pipeline.rs`
- `crates/sheet-compiler/tests/gpu_execution_contract.rs`
- `docs/gpu-prompt-1-status-report.md`

Do not edit `slot_synthesis.rs`, `intermediate_atlas.rs`, GPU capability code, Tauri IPC, frontend code, Cargo files,
or material algorithms.

## Current authoritative symbols

- `AtlasRenderExecutor`, `AtlasRenderExecutorOutput`, and `CpuAtlasRenderExecutor` are in `atlas_executor.rs`.
- `CompiledAtlasPlanV1` already contains output size, tile request, requested maps, ordered sources, exact ordered
  region source crops/destinations/sampling/radial parameters, stable IDs, and final plan hash.
- `compile_source_frame` is in `persisted_pipeline.rs`.
- The only SourceFrame composition block to move starts where `IntermediateAtlasRequest` is created and currently calls
  `AlgorithmCompiler::new().compile_intermediate_atlas(...)` directly.
- Existing CPU composition behavior is authoritative and must remain byte-for-byte unchanged.

## Required implementation

1. Extend the existing `AtlasRenderExecutor` contract with a second responsibility for final atlas composition.
   Keep the current region-synthesis method intact; do not rewrite or copy it.

2. Add a small composition input/output contract in `atlas_executor.rs`:

   - Input contains `&CompiledAtlasPlanV1` and `&IntermediateAtlasRequest` plus the existing cancellation/current
     guards required to reject cancelled or stale work.
   - Output contains the completed `IntermediateAtlasArtifact` and measured `compose_ms`.
   - Use repository-consistent names such as `AtlasComposeExecutionInput` and `AtlasComposeExecutorOutput`.
   - The contract must not contain `TrimSheetDocument`, UI state, project state, or global defaults.

3. Implement composition on `CpuAtlasRenderExecutor` by calling the existing authoritative
   `AlgorithmCompiler::compile_intermediate_atlas`. Do not reproduce its loops and do not alter
   `compose_intermediate_atlas`.

4. Preserve cancellation and revision semantics:

   - Return `Cancelled` before composition when the token is cancelled.
   - Return `Superseded` before publication when `is_current()` is false.
   - Map the existing atlas compiler error into an explicit composition error variant that retains its message.
   - Use the plan/document revision already supplied; do not invent a revision.

5. In `compile_source_frame`, keep construction of topology, placement, slot metadata, profile metadata, and
   `IntermediateAtlasRequest` exactly where it is. Replace only the direct
   `AlgorithmCompiler::compile_intermediate_atlas` call with the new method on the same `CpuAtlasRenderExecutor` used
   for region synthesis.

6. Consume the executor's returned artifact and `compose_ms`. Preserve all existing artifact fields, channel pixels,
   validity, correspondence, ownership, diagnostics, pending messages, telemetry, and cache behavior.

7. Update telemetry so the production artifact explicitly includes:

   - `executor=cpu`
   - `plan_hash=<existing exact hash>`
   - `compose_executor=cpu`
   - `compose_ms=<measured value>`

8. Add one focused production contract test named
   `gpu_executor_owns_base_color_composition` in `gpu_execution_contract.rs`. It must prove:

   - A real persisted SourceFrame compile still enters through `compile_persisted`.
   - The published artifact contains the immutable plan identity.
   - Telemetry contains `compose_executor=cpu`.
   - Base Color output dimensions and pixels are identical to the existing CPU reference fixture.
   - Cancellation or a stale guard cannot return a publishable composed artifact.

9. Update `docs/gpu-prompt-1-status-report.md` after the focused test passes:

   - Mark Prompt 1 accepted as the compiler/baseline phase.
   - State that native request-to-paint qualification is deferred to Prompt 3, where publication changes.
   - Add a Prompt 1.5 entry explaining that complete composition now belongs to the executor.
   - Do not rerun or replace the existing real-8K baseline; it remains the before measurement.

## Do not do

- Do not add or change `wgpu` code.
- Do not optimize or cache composition in this prompt.
- Do not change pixels, sampling, padding, ownership, correspondence, or artifact layout.
- Do not move document/topology planning into the executor.
- Do not create a second executor, compiler spine, renderer, or preview path.
- Do not perform broad repository discovery.
- Do not read files outside the edit boundary unless a compiler error names a directly required import/export.
- Do not run a full workspace test or production build.

## Work budget

- Use at most two consolidated read/search commands against the allowed files.
- The third tool action must apply an edit or return a precise blocker.
- If context is missing, inspect the named symbol range only; never reread an entire large file.
- Make one cohesive edit, run the focused command, make at most one correction pass, rerun the same command, and stop.

## Focused verification

```powershell
cargo test -p hot-trimmer-sheet-compiler gpu_executor_owns_base_color_composition
```

## Done means

- The focused test passes.
- `compile_source_frame` no longer calls `compile_intermediate_atlas` directly.
- The same CPU executor owns region synthesis and final Base Color composition.
- Existing CPU pixels and artifact semantics are unchanged.
- Prompt 2 can replace both dominant CPU pixel passes without changing `compile_persisted` orchestration.

Report only: changed files, resulting runtime route, focused test result, and any blocker. Do not begin Prompt 2.

