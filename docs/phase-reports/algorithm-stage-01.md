# Algorithm Stage 01 — Input ingestion and channel registration

## Delivered authority

Stage 1 replaces loose source-document projections with immutable `MaterialSource` and
`RegisteredChannelSet` intent. The contract covers Base Color, Normal, Height, Roughness,
Metallic, Ambient Occlusion, Specular, Opacity, Edge Mask, and Material ID without recording a
material-family route.

Domain records contain role-specific interpretation, normal convention, oriented dimensions,
the applied orientation transform, ownership intent, original-path provenance, immutable SHA-256,
assignment provenance/confidence, exemplar grouping, source revision, and registration digest.
They deliberately contain no decoded pixel or computation buffers.

## Registration and persistence behavior

- Base Color anchors the oriented dimensions and EXIF orientation transform.
- Companion imports fail with typed diagnostics and recovery choices when Base Color is absent,
  oriented dimensions differ, orientation differs, or interpretation does not match the assigned
  role. Registration never resizes or rotates a companion to make it fit.
- Owned bytes are checked against both their encoded byte count and immutable SHA-256 before the
  transaction begins. Original-path provenance is independent of owned or verified-external
  storage.
- Verified external references are re-read and boundedly re-inspected by the store immediately
  before registration. Digest, oriented dimensions, orientation, byte count, format, color type,
  alpha, and ICC metadata must still match the inspected request, closing the desktop-to-store
  file-change window.
- Import/replacement, removal, and exemplar grouping increment `source_revision`, recompute a
  deterministic registration digest, and delete that material source's derived-cache records in
  the same transaction. A replacement uses a new source identity in the digest, so it cannot reuse
  a stale derived entry even when other metadata matches.
- The schema is a clean Stage 1 cutover. Older project schemas are rejected; there is no legacy
  source-contract migration or inferred provenance.

## UI and IPC

Typed IPC now exposes nested material sources and registered channel sets rather than parallel
`sources`/`sourceSets` arrays. The existing source library reads those authoritative records,
including oriented dimensions and provenance, and authors exemplar groups through a typed native
command. Filename tokens remain confined to optional channel
assignment suggestions; they record `filename_suggested` provenance and never select a material
algorithm.

## Focused evidence

The `algorithm_stage_01_registration` test covers Base-Color-only registration, every full-PBR and
auxiliary channel role, multiple grouped exemplars, immutable-byte checks, dimension/orientation
failures, role interpretation, normal convention, transactional revision changes, replacement and
removal, verified-external reinspection/race rejection, and independently seeded cache-invalidation
assertions for replacement, removal, and exemplar grouping.

Verification command:

```text
cargo test -p hot-trimmer-project-store algorithm_stage_01_registration
```

## Deferred to later stages

Color-space conversion, scalar/ID decode, alpha policy, normal normalization/conversion, and image
pyramids remain Stage 2 work. Perspective rectification and shared homographies remain Stage 3.
Material behavior classification and routing remain Stages 5 and 8; Stage 1 stores no such guess.
