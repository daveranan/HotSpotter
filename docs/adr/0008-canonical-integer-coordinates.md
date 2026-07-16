# ADR 0008: Canonical integer coordinates

**Status:** Accepted

Template geometry is authored on a canonical integer grid (initially 4096 by 4096) using half-open rectangles: `[left, top, right, bottom)`. Normalized UV bounds are derived only when exporting.

Integer canonical coordinates make overlap validation, raster IDs, scaling, hashes, and golden fixtures deterministic. Floating-point normalized bounds are not authoritative template data.

