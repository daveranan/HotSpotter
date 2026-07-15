# ADR 0002: Project Persistence

- Status: Accepted
- Date: 2026-07-15

## Decision

Phase 1 will store authoritative project state in versioned SQLite. Commands execute transactionally, migrations
run against fixtures before user data, and save/recovery never replace the last known-good project until the new
state passes integrity checks.

Derived thumbnails, rectified previews, and render tiles use a disposable content-addressed cache outside the
project database. Stable UUIDs identify every authoritative object.

## Consequences

SQLite is not introduced as a UI cache or queried directly from TypeScript. Project locking, autosave journals,
rotating recovery snapshots, and stale-lock handling belong to `project-store`.

