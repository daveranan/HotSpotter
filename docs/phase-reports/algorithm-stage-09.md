# Algorithm Stage 09 — fixed semantic template topology

## Delivered

- Standard documents now copy their allocation and hotspot rectangles directly from versioned 4096 × 4096 template manifests. Runtime semantic packing and grid/destination topology commands were removed.
- Slots carry stable order and IDs, explicit fit semantics, roles, physical sizes, structural-profile intent, material/variation groups, and optional radial metadata.
- Distinct canonical boundaries are rounded once per requested output edge and reused by every allocation and hotspot edge.
- Recursive horizontal/vertical authored grammar compilation uses deterministic exact largest-remainder allocation. It is an authoring API only; released and custom templates must persist the accepted integer rectangles before a document is created.
- The built-in registry covers generic architecture, horizontal and vertical trim banks, hard-surface panels, detail-heavy props, and radial accents. The workbench and persisted template snapshot keep selection explicit.
- Compatibility comparison reports an explicit topology change when template identity, compatibility key, stable slot order, pinned geometry, or semantic version changes.

## Authority and stop-condition evidence

- Standard and custom template documents validate their stored snapshot hash and reconstruct the expected regions from the pinned manifest. Standard documents additionally require an exact identity-and-definition match with the shipped built-in registry; non-registry definitions are accepted only through custom-template authoring. Mutated or forged standard geometry is rejected.
- Material sources, mappings, render settings, effects, source-analysis provenance, and seed remain outside topology inputs. Material/variation group names and profile/radial intent are semantic slot metadata, not material appearance.
- Unallocated canonical space remains unallocated; no fill pass is allowed to reshape slots.
- Detail/Unique and radial allocations are manifest-owned and cannot be reshaped by source input.

## Focused verification

`cargo test -p hot-trimmer-domain algorithm_stage_09_fixed_topology`

The focused test validates every required family, multiple material bindings without slot movement, 1K/2K/4K/8K shared-boundary compilation, explicit family incompatibility, resolution/seed independence, rejection of standard-template mutation/forgery, and the exact 4096-unit `[1, 2] -> [1365, 2731]` largest-remainder boundary.

## Remaining later-stage work

Stage 10 consumes the fixed slot vocabulary to calculate resolved material footprint, raster density, effect capacity, and supersampling. It must not acquire topology authority.
