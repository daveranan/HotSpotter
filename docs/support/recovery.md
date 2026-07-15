# Recovery Foundations

Phase 0 reserves an operating-system-managed recovery directory and defines the persistence boundary. It does
not yet write project data.

Phase 1 must implement the following before projects are considered durable:

- Transactional SQLite migrations tested against versioned fixtures.
- One-writer project locks and stale-lock recovery.
- Autosave journals and rotating recovery snapshots.
- Integrity checks before replacing the last known-good project.
- An explicit recovery choice that never silently overwrites the user's project.

Deleting the render cache must never delete or invalidate authoritative project data. Uninstall behavior must
leave projects and user-owned source images intact.

