# Project Recovery

Hot Trimmer treats the project file as authoritative and recovery data as a separate safety net.

- Every open project holds a one-writer lock. An active owner blocks a second writer; a lock whose process no
  longer exists is reported and replaced.
- Source registration commits through SQLite transactions and is also recorded in the autosave journal.
- Explicit Save publishes a new immutable baseline generation, then retires the previous generation. Discard
  restores the active baseline rather than accepting recovery autosave as an explicit save.
- Up to five integrity-checked recovery snapshots are retained in the operating-system recovery directory.
- After an unclean shutdown, the Recovery dialog lists only snapshots that open read-only and pass SQLite
  integrity and schema checks.
- **Recover As** requires a new destination. It validates and flushes a temporary copy before atomically
  publishing it and refuses to overwrite any existing project.
- If a recovery refresh fails after an authoritative edit commits, the edit remains visibly Save-pending and a
  warning asks the user to save explicitly; the operation is never falsely reported as unchanged.

If Hot Trimmer exits unexpectedly, restart it, choose a snapshot from Recovery, and save the recovered copy to
a new name. Keep the original until the recovered project has been inspected. A normal project can also be
opened directly; its integrity is checked before the session is adopted.

Owned source bytes are immutable inside the project. External sources are revalidated against their stored
SHA-256 identity. Recovery, cache deletion, and uninstall behavior do not delete user-owned projects or source
images.
