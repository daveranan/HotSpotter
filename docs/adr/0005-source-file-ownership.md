# ADR 0005: Source File Ownership

- Status: Accepted
- Date: 2026-07-15

## Decision

Imported sources are immutable. Phase 1 will offer an owned project copy or a verified external reference as an
explicit choice. External references retain identity metadata and fail visibly when changed or unavailable.

## Consequences

Hot Trimmer never edits or deletes source images. It never silently changes ownership mode, relocates an
external file, or substitutes a stale cache entry for authoritative source bytes.

