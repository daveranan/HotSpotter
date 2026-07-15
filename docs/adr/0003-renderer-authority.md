# ADR 0003: Renderer Authority

- Status: Accepted
- Date: 2026-07-15

## Decision

A deterministic, multithreaded CPU renderer is authoritative for regeneration and export. wgpu accelerates
interactive compositing and 3D preview. Immutable versioned render operations, normalized coordinates,
deterministic seeds, tile halos, cancellation, and cache fingerprints are shared contracts.

## Consequences

GPU output is never silently treated as export truth. Golden fixtures measure GPU/CPU differences per channel,
and a release gate blocks divergence outside documented tolerances.

