# GPU render migration queue

Status: Completed through Prompt 005 on 2026-07-20. The render migration queue is closed; subsequent work resumes at
Prompt 15 in [`../hot-trimmer-v1-full-algorithm-stack-prompt-pack.md`](../hot-trimmer-v1-full-algorithm-stack-prompt-pack.md).

Run these prompts directly in normal Codex tasks, one at a time. Do not use an orchestrator prompt, Spark worker,
subagent, or parallel implementation. Each prompt is self-contained and is the only task document the implementation
task should read.

After each prompt, use [`manual-review.md`](manual-review.md) for the product-facing acceptance check. The implementation
task should not read that review guide; it is for the human reviewer after the prompt reports completion.

## Queue

| Prompt | State | Result |
| --- | --- | --- |
| [`prompt-001.md`](prompt-001.md) | Completed | Real 8K baseline, `CompiledAtlasPlanV1`, CPU synthesis executor, GPU capability skeleton |
| [`prompt-001.5.md`](prompt-001.5.md) | Completed | Executor-owned complete CPU Base Color composition |
| [`prompt-002.md`](prompt-002.md) | Completed | GPU compact region commands, sampling, and direct Base Color atlas writes |
| [`prompt-003.md`](prompt-003.md) | Completed | GPU tiles, padding, compact Region ID, exact preview, binary IPC |
| [`prompt-004.md`](prompt-004.md) | Completed | Requested GPU material-map pass graph |
| [`prompt-005.md`](prompt-005.md) | Completed | 16K/24K source/output tiling, streaming export, hardening |

## Execution rules

- Start only the first unblocked prompt.
- Read only that prompt plus automatically supplied repository instructions.
- Do not read the migration plan, old prompt packs, historical reports, or future prompt files.
- The prompt embeds the current runtime facts and exact edit boundary.
- Use targeted symbol reads, not full-file dumps or repository-wide searches.
- Implement the requested code before optional investigation.
- Run only the prompt's focused verification command, with at most one correction pass.
- Run a separately listed benchmark/native check only when the prompt explicitly requires it.
- Stop at the prompt boundary. Do not begin the next file in the same task.
- Update this table only after the current prompt's acceptance gate passes.

## Locked architecture

```text
TrimSheetDocument
-> compile_persisted
-> CompiledAtlasPlanV1
-> one production GPU executor
-> GPU output tiles/maps
-> compiled artifact
-> preview or streaming export
```

The CPU compiles low-volume instructions. The GPU calculates pixels. Do not reproduce the current giant CPU
per-pixel architecture as giant GPU buffers.

Upload and retain:

- Compact source records.
- Compact per-region commands.
- Source textures or source tiles.
- Requested output/intermediate tiles.

Do not create by default:

- Full-frame source-coordinate textures.
- Full-frame seam-coordinate textures.
- Per-pixel UUID textures.
- CPU rendered-region buffers for GPU output.
- Mandatory full-frame correspondence.
- All material maps when one was requested.

Read back only the requested final preview/export tile or bounded export batch.
