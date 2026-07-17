# Algorithm Stage 00 report

## Outcome

Prompt 00 establishes the deterministic, material-agnostic acceptance corpus and the clean twenty-stage engine boundary. No stage algorithm is implemented. The sole new compiler facade returns `UnsupportedStage` for Stage 1 and has no call edge to the former document renderer.

## Delivered contracts

- `fixtures/algorithm-stack/material-corpus.json` covers all ten measured behavior classes, eight required source conditions, Base-Color-only and registered map combinations, and the seven semantic slot roles from the prompt pack.
- Deterministic integer fixture generators cover structure, orientation, periodicity, saliency, and registered-channel correspondence. Each fixture records generator/version provenance, seed, dimensions, and expected behavioral properties.
- `fixtures/algorithm-stack/stage-traceability.json` assigns Stages 1-20 and every section-30 acceptance item to an owning prompt and future focused test.
- Domain contracts include `StageResult`, algorithm provenance, content/cache keys, request and artifact headers, `PreparedSources`, placement/sampling/effect plan headers, deterministic `CompilationReport`, cache interfaces, cancellation, revision publication guards, bounded resources, seed policy, and stable tie-breaking.
- Workspace boundaries now include `material-analysis`, `material-synthesis`, `placement-solver`, and `effect-compiler` with one-way dependencies matching the implementation plan.
- IPC protocol 2 is a clean algorithm-stack baseline. The workbench retains source preparation, while compile and preview are explicitly unavailable until an owning prompt installs a route.

## Authority and publication safety

`hot_trimmer_sheet_compiler::AlgorithmCompiler` is the only exported authoritative compiler entry point. The former document compiler is crate-private and is not called by the facade or desktop build command. Prompt 00 cannot publish maps: cancellation is checked before execution, and all current requests end with a typed Stage 1 failure. `PublicationGuard` separately requires both an uncancelled token and the exact current revision before any future complete cache/artifact publication.

## Determinism and bounds

Canonical request/report serialization uses ordered structs and `BTreeMap`; cache keys are SHA-256 over canonical request bytes. Fixture raster generation uses integer-only deterministic operations. `ResourceLimits::V1_BOUNDED` caps source/output edges, total pixels, cache writes, candidates, graph nodes, iterations, supersampling, and effect operations.

## Evidence

Focused contract test: `cargo test -p hot-trimmer-domain algorithm_stack_contract`.

The test parses both manifests, verifies complete Stage 1-20 ownership, generates every synthetic fixture twice, compares request bytes/cache keys and report bytes, and rejects cancellation and revision supersession for publication.

## Deferred by design

Stages 1-20 remain unsupported until their owning prompts. No placeholder produces pixels, no material/product/filename dispatch exists, and no legacy migration or compatibility adapter was added.
