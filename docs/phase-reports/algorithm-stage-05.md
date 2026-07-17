# Algorithm Stage 05: source quality and material behavior

## Delivered authority

Stage 5 consumes only the Stage 4 `DelitPreparedExemplar`. It does not inspect source paths,
fixture IDs, product names, or channel-assignment filenames. `analyze_source` produces one
deterministic, bounded, cache-addressed `SourceAnalysisReport` and records the Stage 5 algorithm
ID/version in `StageResult::Executed`.

The authoritative prepared-patch desktop path executes Stage 5 after Stage 4, reuses the bounded
`SourceAnalysisCache`, and projects concise evidence into the context inspector. The inspector's
routing selector invokes the typed native `MaterialClassificationCommand` (`Override` or
`ResetToAnalysis`). `MaterialClassificationIntent` is stored in the project database and projected
back to the UI on reload; applying it changes only routing intent on a cloned report.

## Measurement contract

All report values have explicit stable units:

| Measurement | Unit and range |
| --- | --- |
| sharpness | mean four-neighbor linear-luminance gradient, `[0,1000]` milli-score |
| noise | high-frequency residual, `[0,1000]` milli-score |
| compression | excess discontinuity on 8-pixel boundaries, `[0,1000]` milli-score |
| dynamic range | linear-luminance P95-P05, `[0,1000]` milli-score |
| clipping | RGB samples at either endpoint, `[0,1,000,000]` ppm |
| perspective confidence | Stage 3 evidence carried through Stage 4, `[0,1000]` |
| usable area | alpha-and-coverage-valid samples, `[0,1,000,000]` ppm |
| registration quality | worst companion-map zero-shift correspondence, `[0,1000]`, plus strongest signed pixel offset |
| estimated material resolution | effective detail samples on the shorter source axis, pixels |

Analysis samples at a maximum 256-pixel edge by default and bounds the registration search to
four **source pixels** in every direction. Downsampling never changes the declared search range or
reported offset unit. Thresholds are typed and public. Imperfect sources produce
explicit warnings with recoveries such as lower texel density, synthesis, adjusted registration,
or another source; they remain usable by a lower-fidelity route.

## Behavior evidence and confusion policy

The deterministic heuristic measures opposite-boundary agreement, orientation coherence and
variation, horizontal and vertical autocorrelation, bandedness, regularity, localized saliency,
radial symmetry, and stationarity. Those measurements provide evidence for every revised class:

- Already tileable
- Stochastic isotropic
- Stochastic directional
- Periodic/lattice structured
- Layered/banded
- Organic directional
- Manufactured pattern
- Unique detail
- Radial detail
- Mixed/Unknown

The complete normalized distribution is stored in stable score/class order and sums to 1000.
The default confusion policy requires at least 350 milli support and a 35 milli top-two margin.
Evidence below either tolerance is reported as Mixed/Unknown rather than becoming a hard routing
fact. `ClassificationCommand::Override` changes only `RoutingIntent`; `ResetToAnalysis` restores
evidence routing. Neither operation rewrites quality, measurements, analyzed class, confidence,
or ranked distribution.

`AlreadyTileable` is conjunctive rather than seam-only: its support is boundary agreement multiplied
by stationarity and by low localized saliency. Matching or flat borders therefore cannot overwhelm
unique internal content, while seamless stationary low-saliency evidence remains strong.
The boundary term is cubed so moderate accidental edge agreement cannot masquerade as authored
seamability. Period evidence additionally requires actual variation on the tested axis, radial
symmetry is measured as variance explained by radius, and stationarity compares both quadrant means
and quadrant gradient energy. These calibrated measurements distinguish smooth directional
correlation, one-axis bands, radial structure, and composite domains without changing thresholds.

Directionality is rotation-invariant: Stage 5 accumulates the global 2x2 gradient structure tensor
`[[sum Ix^2, sum IxIy], [sum IxIy, sum Iy^2]]` and computes coherence as
`(lambda_max-lambda_min)/(lambda_max+lambda_min)`. Local orientation variation uses the same tensor
over four regions and compares doubled-angle vectors, so horizontal, vertical, and diagonal evidence
share one metric. Scharr gradient vectors are magnitude-capped before their squared components are
accumulated; this preserves second-moment anisotropic energy while limiting isolated high-contrast
wraps and outliers.

## Provider and inspector boundary

`LocalClassifierProvider` is a versioned local-only deterministic interface. Provider output must
contain each behavior class exactly once, be ranked, bounded, and sum to 1000. Missing, failed,
incompatible, or malformed providers fall back to the authoritative deterministic heuristics.
Provider cancellation instead propagates `SourceAnalysisError::Cancelled`, so cancelled work cannot
publish an executed report. Both probability and support values are range-validated.

The cache key includes the Stage 5 and provider interface versions, provider/model identity, Stage 4
Base Color and every companion-map pixel, channel roles, coverage, perspective confidence, settings,
and exemplar identity. Registration, usable-area, perspective, or provider changes therefore cannot
reuse stale evidence.

`SourceAnalysisReport::inspector_evidence` exposes concise quality, analyzed/routed class,
confidence, principal measurements, and warning count for the source inspector without exposing
large analysis fields.

## Acceptance evidence

The focused test loads the Stage 0 universal corpus, generates every checked-in integer fixture, and
analyzes its pixels. These feature fixtures assert only their declared measurements: localized
structure, oblique tensor coherence, two-axis periods, saliency above the stochastic registration
baseline, and exact registered companion correspondence. They do not invent behavior labels. The test also
proves that the corpus behavior vocabulary matches the typed Stage 5 vocabulary, then supplies
independent image evidence for every class without passing corpus IDs to the classifier. Every class
must dominate or remain in a declared top ambiguity set. A natural
lattice/stochastic composite remains Mixed/Unknown under the default confusion tolerance. The same
test asserts deterministic/cacheable output, complete cache-key sensitivity, override/reset evidence
preservation, downsampled image-correspondence detection of deliberately shifted PBR inputs in source
pixels, provider cancellation and malformed-support handling, and concise inspector evidence.

Verification command:

```text
cargo test -p hot-trimmer-material-analysis algorithm_stage_05_quality_classification
```

Stage 6 remains responsible for physical scale and orientation calibration; Stage 7 remains
responsible for spatial feature fields used by crop and synthesis planning.
