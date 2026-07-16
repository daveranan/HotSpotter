# Generic Architecture v1

This 4096 by 4096 canonical trim sheet is a dense topology layered over one continuous base material, not a collection of independent source crops. Every allocation is an integer, half-open rectangle `[x, x + width) x [y, y + height)`; 32-pixel gutters separate adjacent slots and remain Region ID background.

The stable 52-slot layout is organized as follows:

- Top: three full-span architectural strips, two header strips, and four fine grooves.
- Center: four large panel-frame allocations, eight rectangular detail cells, four radial fixtures, and four trim caps.
- Lower: three long sills, four mid grooves, eight lower detail panels, three tall baseboards, three footer grooves, and three plinth strips.

Roles are manifest-owned: `planar`, `repeatingStrip`, `uniqueDetail`, `trimCap`, and `radial`. Structural profiles describe the intended topology (`flat`, `bevel`, `groove`, `roundedBevel`, `panelFrame`, `radialDisc`, and `annulus`). Radial slots are square and carry normalized radial parameters. Each slot retains its stable key, compatibility key, material and variation groups, hotspot, world placement, allocation, and deterministic Region ID color.

The supplied 4K fixture lists canonical Region ID rectangles. The 2K fixture is the exact lossless half-scale map: every coordinate and extent is even in the canonical grid.
