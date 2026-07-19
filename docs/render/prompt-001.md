# Render Prompt 001 - Completed baseline and executor seam

**State: completed. Do not run this prompt again.**

This record exists so later tasks do not rediscover Prompt 1.

## Implemented state

- `CompiledAtlasPlanV1` is in `crates/sheet-compiler/src/compiled_atlas_plan.rs`.
- `AtlasRenderExecutor` and `CpuAtlasRenderExecutor` are in
  `crates/sheet-compiler/src/atlas_executor.rs`.
- Production SourceFrame compilation enters through `compile_persisted` and submits exact ordered source/region
  commands to the CPU synthesis executor.
- The application owns a long-lived GPU capability-service skeleton in `crates/preview`.
- `wgpu` 26.0.1 and its compatible `wgpu-hal` 26.0.0 graph are pinned.
- Focused production tests lock direct, LoopX, LoopY, LoopXY, and radial identities/parameters.
- A real release baseline uses an actual 7952 x 4016 source, 8192 x 8192 output, and 64 regions.

## Baseline to preserve

| Run | Total | Decode | Stage 14 synthesis | CPU composition |
| --- | ---: | ---: | ---: | ---: |
| Cold | 24.904 s | 3.370 s | 8.802 s | 12.728 s |
| Warm | 13.401 s | 0 ms | 0 ms | 13.398 s |

- Warm decode cache: hit.
- Warm rendered-region cache: 64/64 hits.
- Peak RSS: 5,500,157,952 bytes.
- Artifact allocation: 1,946,157,056 bytes.
- Stable plan hash: `8509b9180233c06988132cd3d91eb3831b23531be0d764dea0779ad159e77e96`.
- Qualification adapter: RTX 3090/Vulkan, maximum 2D texture 32768, recommended tile 2048.

## Known boundary defect

The executor owns Stage 14 region synthesis but not `IntermediateAtlasArtifact` composition. `compile_source_frame`
still calls `AlgorithmCompiler::compile_intermediate_atlas` after executor completion. That 13.4-second warm pass is
the reason [`prompt-001.5.md`](prompt-001.5.md) is next.

