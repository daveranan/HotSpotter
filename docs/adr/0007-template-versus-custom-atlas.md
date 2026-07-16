# ADR 0007: Template versus Custom Atlas

**Status:** Accepted

`Template` and `CustomAtlas` are explicit, incompatible layout kinds. Template layouts use a pinned topology and stable hotspot vocabulary; basic mode does not permit freeform slot edits.

Custom Atlas retains generic packing, manual boundaries, and arbitrary regions. Editing template topology requires cloning it into a custom layout and issuing a new compatibility key; it must never claim compatibility with the source standard template.

