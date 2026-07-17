# Algorithm Stage 06: physical scale and orientation calibration

## Delivered authority

Stage 6 consumes the Stage 4 prepared pixels and the complete Stage 5 report. Persisted
`MaterialCalibrationIntent` owns only explicit scale/orientation intent and a monotonic revision;
derived gradients, tensors, local fields, overlays, and footprint keys remain cache-owned. The
desktop prepared-patch path executes Stage 6 after Stage 5 and projects its concise evidence and
source-pixel overlay vectors.

Typed commands cover imported X/Y sampling metadata, a source-space two-point measurement with a
known distance, known motif pixel/physical dimensions, explicit scale overrides, orientation-axis
overrides, and independent scale/orientation resets. Project persistence deletes the source's
derived-cache rows after every accepted calibration command. Stage 6's content key and downstream
footprint key also include the calibration revision, evidence, settings, and Stage 5 cache identity.

## Physical-scale contract

Pixels-per-meter values are stored as integer milli-pixels-per-meter independently for X and Y.
Their provenance is one of `Imported`, `UserMeasured`, `MotifDerived`, `Convention`,
`PriorEstimated`, or `RelativeOnly`, with confidence in `[0,1000]`.

- Imported, measured, motif-derived, and explicit convention evidence may expose world scale.
- A class prior retains numeric planning evidence but is always `UnavailablePriorEstimate`; it
  cannot support a world-size claim.
- Unknown scale defaults to `RelativeOnly`, carries no pixels-per-meter number, and is always
  `UnavailableRelativeOnly`.
- Two-point measurements produce exactly equal X/Y sampling, preserving isotropic scans.
- Unequal declared X/Y values are reported when their relative disagreement exceeds the typed
  tolerance. Image width, height, and aspect ratio never create physical anisotropy.

Measurement overlays state their `SourcePixels` coordinate space and whether world scale is
available. Viewport transforms are presentation-only.

## Orientation contract

Stage 6 computes capped Scharr gradients, accumulates symmetric structure tensors, and derives
the material axis from the minor tensor eigenvector (the axis along a directional structure rather
than its gradient normal). A Scharr sample is valid only when its complete 3x3 neighborhood is
covered by the retained Stage 3/4 mask, preventing transparent or invalid rectification boundaries
from creating orientation authority. Local samples combine bounded integral-tensor windows at three
scales. Their energy is normalized by the weighted valid-sample count in those windows rather than
the full image size. Global and local anisotropy use
`(lambda_max-lambda_min)/(lambda_max+lambda_min)`; confidence also requires non-trivial gradient
energy.

All axes are normalized to `[0,180)` millidegrees, making opposite directions equivalent. Measured
axes below the declared confidence threshold are stored as unavailable, not as arbitrary angles,
and cannot authorize destructive rotation. A user override is explicit authority and remains
separate from measured confidence. Orientation overlays contain source-pixel positions and omit
low-confidence local axes.

Stage 6 preflights conservative working-byte and operation counts before allocating luminance,
gradient, and integral-tensor buffers. Luminance, Scharr, and integral construction check
cancellation every 32 rows; local extraction checks it for every grid row. Resource excess and
cancellation return typed errors without publishing an executed report.

The active inspector exposes command-backed two-point measurement, known motif dimensions,
imported X/Y pixels-per-meter, convention/prior overrides, orientation overrides, and independent
resets. Measurement coordinates are labeled and submitted in source pixels; viewport zoom never
enters persisted intent.

## Acceptance evidence

The checked-in Stage 0 directional fixture recovers its declared axis within the five-degree
tolerance. Coordinate-hashed 2D noise is exercised over four deterministic seeds and square,
landscape, and portrait aspect ratios; every case reports no global axis and permits no destructive
rotation. A deliberately strong pattern in uncovered padding likewise produces no orientation
authority. Grid-size comparison checks local confidence normalization, and the gate covers resource
preflight and cancellation in addition to two-point round-trip scale, exact isotropic X/Y
preservation, reset/override commands, changed downstream footprint keys, prior-estimate and
relative-only world-scale unavailability, inconsistent declared anisotropy, explicit orientation
override, and source-coordinate overlay authority.

Verification command:

```text
cargo test -p hot-trimmer-material-analysis algorithm_stage_06_scale_orientation
```

Stage 7 remains responsible for saliency, structure, stationarity, periodicity, seamability, and
usability fields used by crop and synthesis planning.
