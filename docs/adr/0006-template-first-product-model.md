# ADR 0006: Template-first product model

**Status:** Accepted

The default Hot Trimmer workflow instantiates a pinned, versioned template and compiles material content into its slots. A standard template owns its topology, slot semantics, stable identifiers, and compatibility key.

The generic packer remains available only as the advanced Custom Atlas workflow. Material swaps, weathering changes, and resolution changes must preserve standard-template topology and Blender UV assignments.

