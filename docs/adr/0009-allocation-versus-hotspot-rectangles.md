# ADR 0009: Allocation versus hotspot rectangles

**Status:** Accepted

Each template slot declares an `allocation_rect` and an enclosed `hotspot_rect`. Allocation includes rendered content, padding, dilation, and mip-safe bleed. Hotspot is the exact UV-fitting boundary and structural profile edge.

Only hotspot bounds feed Blender metadata and Region IDs. Changing allocation padding or output resolution must not move normalized hotspot bounds or alter topology identity.

