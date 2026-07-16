# ADR 0011: Blender companion ownership

**Status:** Accepted

Hot Trimmer owns projects, templates, slot semantics, rendering, manifests, publishing, and topology compatibility decisions. The first-party Blender companion owns Blender material creation, image reloads, UV-island fitting, assignment persistence, locks, and Blender-facing diagnostics.

The companion may select compatible hotspots but does not normally edit sheet topology. Third-party UV tools are behavior references, not runtime dependencies.

