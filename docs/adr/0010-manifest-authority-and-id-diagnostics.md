# ADR 0010: Manifest authority and ID-map diagnostics

**Status:** Accepted

The exported Hot Trimmer manifest is authoritative for template identity, slot geometry, fit rules, radial semantics, stable region IDs, map metadata, and revision compatibility. The Region ID map is a lossless visual diagnostic and selection aid.

The Blender companion must not infer slot type, fit axis, radial status, or topology from ID-map pixels. ID maps contain one exact color per enabled hotspot plus background and are not a substitute for the manifest.

