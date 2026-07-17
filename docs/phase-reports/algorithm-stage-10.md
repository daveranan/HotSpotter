# Algorithm Stage 10 — resolved slot demand and effect capacity

## Delivered

- `ResolvedSlotDemand` now materializes the complete Stage 10 contract: semantic role, orientation, allocation and hotspot authority, output pixels, authored world dimensions, major/minor axes, aspect, pixel/meter conversions, desired density, legal mapping modes, rotations and mirroring, material/variation groups, profile and weathering intent, importance, survivability, bevel/isotropic limits, feature LODs, supersampling, capacity, and diagnostics.
- Physical source calibration produces the specified `world meters × source pixels per meter` footprint. When authored world dimensions are not the selected authority, the explicit destination-density route derives each world edge as `destination pixels / desired texel density` before calculating the calibrated source footprint. Unavailable or prior-only source scale produces an aspect-preserving relative-texel footprint with its original provenance and an explicit diagnostic; it is never labeled as source pixels or world-accurate.
- `EffectScaleSpace` exposes only World, slot-minor-relative, slot-major-relative, slot-area-relative, and Pixels. There is no normalized-rectangle physical scale. Conversions are expressed in slot-local meters, and world-space isotropic dimensions convert independently through the X/Y raster densities.
- `EffectCapacity` records opposing-edge profile limits, flat-center feasibility, maximum isotropic diameter and radial radius, raster LOD thresholds, legal role variants, and the bounded 1×/2×/4×/8× recommendation.
- Opposing profiles use `minor - first - second >= required_flat`. Symmetric bevel capacity is `(minor - required_flat) / 2`, clamped at zero. Insufficient strips retain explicit merged/rounded, normal-only, and disabled recovery vocabulary rather than accepting an overlapping flat profile.
- Capacity and insufficiency measurements are serialized in the authoritative result. UI and QA consumers inspect this artifact and do not own a second implementation of the equations.

## Authority and stop-condition evidence

- Canonical allocation/hotspot rectangles remain topology metadata. With `Stage9Authored`, only Stage 9 authored world dimensions enter meter-space equations. With `DestinationTexelDensity`, Stage 10 instead derives world dimensions from the compiled destination pixels and desired texel density; the selected source is recorded explicitly.
- Physical feature widths are never modified by supersampling. Physical fit is diagnosed first; supersampling only improves internal raster sampling and is capped at 8×.
- Every requested scale space is resolved before survivability decisions: relative values become slot-local meters, World values remain meters, and Pixels values remain pixel-space raster operations. Pixel-space values are never multiplied by pixels-per-meter as though they were meters.
- Informational relative-scale evidence and ordinary raster warnings remain inspectable per-slot diagnostics. Only actual `Insufficient` findings enter the stage-level compilation diagnostic list, and those use `InsufficientInput` rather than `MalformedInput`.
- With `Stage9Authored`, resolution changes destination pixels, pixels/meters, meters/pixel, LOD thresholds, and supersampling demand without changing topology, world dimensions, feature-meter intent, or semantic groups. With `DestinationTexelDensity`, changing resolution intentionally changes the derived world dimensions while preserving the requested texel density; topology and semantic groups remain unchanged.
- Isotropic world features remain equal in meters on both axes even when their pixel extents differ because of destination density.

## Focused verification

`cargo test -p hot-trimmer-effect-compiler algorithm_stage_10_slot_capacity`

The fixture covers a broad panel, extreme horizontal and vertical strips, radial and cap roles, a sub-pixel feature, impossible opposing bevels, known and unknown source scale, isotropic conversion, and a resolution-only change.

## Remaining later-stage work

Stages 11–13 consume this artifact for legal source placement. Stages 15, 16, and 18 consume its capacity and coordinate vocabulary for profile, detail, and effect compilation. Product UI wiring and final QA views remain owned by the later integration stages; those clients must display this serialized result rather than recomputing it.
