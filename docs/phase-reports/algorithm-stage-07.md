# Algorithm Stage 07: registered feature-field extraction

## Delivered authority

Stage 7 consumes a `DelitPreparedExemplar` and its Stage 6 `ScaleOrientationReport`. It does not
consume the ranked material class, a filename, or a product/material label. Before analysis it
requires every rectified channel and every Stage 4 mask to have the Base Color dimensions. A
dimension mismatch returns `RegistrationDrift` and publishes no fields.

The output is a content-addressed `FeatureFieldReport` with a common registered-source coordinate
digest and one shared pyramid dimension schedule. Scalar pyramids use deterministic 2x2 averaging;
usability reasons use conservative bit union. The report exposes QA views for saliency, six
structure responses, stationarity, periodicity confidence, seamability, and both usability
confidence and reasons.

## Fields and evidence

- Saliency converts linear Base Color to Oklab, builds a registered bounded pyramid, compares every
  level with its parent surround, and fuses those responses back into source coordinates. Confidence
  clears both a fixed visible-contrast floor and a robust median adjacent-color noise floor; it is
  never forced to one by global maximum normalization. This exposes locally distinctive marks
  without naming their material or deciding how a later semantic slot values them. The same field
  can therefore be penalized by a generic slot and rewarded by a unique-detail slot.
- Structure includes edge magnitude, tensor-coherent lines, boundary evidence (including an
  optional registered edge mask), line/intersection grid evidence, Stage 6-aligned fiber evidence,
  and tensor intersection response.
- Stationarity compares local color moments, gradient/frequency variance, neighboring-window
  drift, and variance from every registered scalar material map.
- Periodicity searches bounded X/Y normalized autocorrelation lags, measures peak prominence,
  applies deterministic ordering and candidate truncation, and emits one- or two-vector lattice
  candidates with sample counts and confidence. No behavior-class result participates.
- Seamability always compares Base Color, gradient, and structural crossings. Registered Height,
  vector normals, and Roughness add typed terms when present. Their absence changes the term set,
  never coordinates or pyramid dimensions.
- Usability multiplies continuous coverage/alpha/opacity, clipping, highlight, shadow, and
  de-lighting-confidence evidence. It retains inspectable reason bits instead of reducing uncertain
  pixels to an unexplained Boolean. Compact salient/structured content is flagged as a suspected
  occluder/logo and softly discounted unless explicit settings retain it.

## Bounds and failure behavior

Dimensions, pyramid levels, tile size, stationarity radius, autocorrelation lag, lattice candidate
count, working bytes, and operations are bounded in `FeatureFieldSettings`. Work checks cancellation
between row and lag units. Complete reports alone enter the 16-entry deterministic cache.

Explicit insufficiency diagnostics report a source with less than 25% confident usable area, no
periodicity peak over the declared threshold, or Base-Color-only seam evidence. These are honest
downstream capability limits, not forced material classifications.

## Acceptance evidence

The focused Stage 7 test exercises a two-axis periodic lattice, directional structure, deterministic
stochastic input, one salient unique mark, and a half-unusable source. It also proves optional full
PBR maps add exactly Height, vector-normal, and Roughness seam terms without changing coordinates;
registered-map drift fails explicitly; cancellation and resource preflight publish no report; and
identical inputs/settings produce the same cache key.

Documented tolerances are: periodic peaks must meet the configured 180/1000 confidence floor;
directional line response averages above 0.08; stochastic evidence must not exceed 500/1000 for its
best candidate; the salient mark exceeds the quiet corner by 0.4; and uncovered pixels have zero
usability confidence with a transparency/outside reason.

Verification command:

```text
cargo test -p hot-trimmer-material-analysis algorithm_stage_07_feature_fields
```

Stage 8 remains responsible for interpreting these typed fields when constructing direct,
graph-cut, periodic, directional, and synthesis domains.
