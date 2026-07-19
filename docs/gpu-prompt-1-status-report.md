# GPU Migration Prompt 1 status report

Date: 2026-07-18

Status: **implementation and focused verification are green; Prompt 1 is not yet accepted**.

## Implemented

- Added and validated the deterministic `CompiledAtlasPlanV1` contract.
- Routed production SourceFrame Stage 14 pixel synthesis through `CpuAtlasRenderExecutor` beneath `compile_persisted`.
- Kept the existing CPU sampler as the Prompt 1 pixel implementation.
- Added the long-lived native GPU capability-service skeleton and application-owned lifetime.
- Pinned `wgpu` 26.0.1 and the published compatible `wgpu-hal` 26.0.0 dependency graph.
- Added a genuine 7952×4016 PNG/RGBA8 source and 8192×8192 Base Color release harness with 64 regions.
- Relabeled misleading small fixtures that previously implied real 8K coverage.

## Focused verification

```text
cargo test -p hot-trimmer-sheet-compiler gpu_execution_contract
```

Result: 1 matching production contract test passed. The test confirms that a production SourceFrame request executed through the CPU executor and published the immutable plan identity.

## Real-8K baseline

```text
cargo test -p hot-trimmer-sheet-compiler --release --test real_8k_baseline -- --ignored --nocapture
```

The restricted child-process sandbox could not create the trace directory, so the successful run used the harness's documented `HOT_TRIMMER_GPU_BASELINE_DIR` override with an approved unsandboxed execution. Workload semantics and build profile were unchanged.

Qualification machine:

- Windows 10 Home
- AMD Ryzen 9 5900XT, 16 physical / 32 logical cores
- 51,444,936,704 bytes RAM
- NVIDIA GeForce RTX 3090, Vulkan, NVIDIA driver 610.74
- 25,769,803,776 bytes VRAM
- Maximum 2D texture dimension: 32768
- Selected tile recommendation: 2048
- Timestamp queries: supported

Workload and results:

| Run | Total | Decode | Stage 14 | Compose | Decode cache | Render cache |
| --- | ---: | ---: | ---: | ---: | --- | --- |
| Cold | 24.904 s | 3.370 s | 8.802 s | 12.728 s | miss, 1 decode | 0/64 hits |
| Warm 1 | 13.401 s | 0 ms | 0 ms | 13.398 s | hit, 0 decodes | 64/64 hits |
| Warm 2 | 13.351 s | 0 ms | 0 ms | 13.346 s | hit, 0 decodes | 64/64 hits |

- Stable plan hash across all runs: `8509b9180233c06988132cd3d91eb3831b23531be0d764dea0779ad159e77e96`
- Peak observed RSS: 5,500,157,952 bytes
- Reported artifact allocation: 1,946,157,056 bytes per run
- Base Color encode: 110–128 ms
- Current Base64 IPC preparation: 0–1 ms for the highly compressible 1,968,956-byte payload
- GPU upload bytes: 0, as required for the Prompt 1 CPU pixel path

The machine-readable trace is `docs/gpu-prompt-1-real-8k-baseline.json`.

## Remaining acceptance blockers

1. `docs/gpu-rendering-migration-plan.md` requires the four Manual Layout Product prompts to be accepted before this migration pack. Only an explicitly unaccepted Prompt 1 report currently exists.
2. The headless harness cannot capture browser UI paint. Native request-generation and UI-paint evidence still needs to be recorded.

No Prompt 2 work has started.
