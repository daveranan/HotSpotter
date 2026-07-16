# ADR 0012: Atomic local revision sync

**Status:** Accepted

Hot Trimmer publishes each material revision into a complete revision directory, validates it, then atomically replaces a small local current-pointer file. The Blender companion reads only the pointer-selected completed revision.

Partial staging data is never current. Appearance-only revisions refresh maps without remapping UVs; topology changes are compared through manifest compatibility metadata and reported explicitly.

