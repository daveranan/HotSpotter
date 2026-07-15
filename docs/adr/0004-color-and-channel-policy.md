# ADR 0004: Color and Channel Policy

- Status: Accepted
- Date: 2026-07-15

## Decision

Base Color is color-managed. Height, Normal, Roughness, Metallic, AO/Cavity, masks, and IDs are linear data.
Normal operations decode, filter or blend, and renormalize vectors; orientation is explicit. ID maps use stable
flat colors with no filtering at region boundaries.

Generated physical channels are labeled `Estimated`. Metallic defaults to zero unless imported or explicitly
assigned.

## Consequences

Every decoder, renderer, preview, and export preset must declare channel semantics. Accidental color transforms
or per-channel coordinate drift are release blockers.

