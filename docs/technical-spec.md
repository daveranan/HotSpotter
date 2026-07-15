# Hot Trimmer Technical Specification

## Implementation Status

| Contract area | Status | Current authority |
| --- | --- | --- |
| Native shell and integrated material workbench | Implemented and tested | Phase 1–2 runtime |
| Schema v5, autosave, baseline, recovery, migrations | Implemented and tested | Rust project store |
| Image parsing, inspection, mipmaps, registration | Implemented and tested | Rust image I/O + project store |
| Multi-file assignment and project rename | Implemented and tested | Desktop UI + typed native commands |
| Patch capture and rectification | Implemented and tested | Phase 2 domain, geometry, renderer, and UI |
| Split workplace/workpiece and live rectification | Implemented and tested | Phase 2 desktop UI |
| Authoritative trim layout | Specified; Phase 3 | Layout intent below |
| Maps, polish, Export, Send to Blender | Specified; Phases 4–6 | Renderer/export roadmap |

“Specified” does not mean a working control exists. Later controls stay disabled or absent until their owning
phase implements and verifies them.

## 1. Interaction Architecture

The application is workspace-based, not wizard-based. The primary surface is permanently split between the
material-source/patch workplace on the left and the hotspot workpiece on the right. Source import is an action
inside that workplace, not a separate primary mode. Layers & Maps is a later contextual workspace; Export and
Send to Blender are top-level commands. Workspace state includes selected material source, patch, channel,
viewport transform, inspector selection, and the last useful subview.

Enabled controls must perform an action. Capabilities that are not compiled into the current phase remain
visibly labelled as later work and disabled. Opening an image is a project action. When no project exists, Open
images asks for one or more images first, then the durable project destination, then runs the same Base
Color-first assignment logic used by Open all.

## 2. Material Input Model

Schema version 5 currently stores one registered material-input set with ten unique slots, durable import
provenance, and ordered editable patches. Phase 3 must migrate this to ordered material-source sets before layout
regions are introduced. Each set uses the same input-slot contract:

| Slot | Data policy | Purpose |
| --- | --- | --- |
| Base Color / Diffuse | color-managed sRGB display | registration anchor and patch color |
| Normal | linear vector data | imported tangent-space normal |
| Height / Bump | linear scalar data | imported height or bump source |
| Roughness | linear scalar data | imported microsurface response |
| Metallic | linear scalar data | explicit metal assignment |
| Ambient Occlusion | linear scalar data | imported AO/cavity |
| Specular | linear scalar data | optional explicit specular level |
| Opacity | linear scalar data | optional cutout/transparency |
| Edge Mask | linear mask data | optional authored trim-detail mask |
| Material ID | flat ID data | optional material-region assignment |

Base Color must exist before a companion slot is filled. Every companion source must match oriented Base Color
dimensions exactly. Slots have stable channel identity, explicit owned-copy or verified-external ownership, and
at most one imported image per role. Imported bytes remain immutable. Empty slots are product state, not rows in
SQLite.

Every newly imported source stores its original path separately from its storage/ownership policy. The workplace
exposes actual filename, original path, dimensions, and slot role. Ownership, ICC, alpha, and data-policy
metadata remain available to persistence/rendering but are intentionally absent from routine inspector UI.

Multi-file Open all applies deterministic filename-token matching for common PBR roles, imports Base Color first,
fills only empty slots, and falls back to visible slot order for ambiguous files. Individual Add/Replace remains
available for correction. Project rename is an autosaved journal command with recovery refresh.

### Implemented Open All Algorithm

1. Filter the selected paths to supported image formats through the native dialog.
2. Compute open channels from the ten visible slots; occupied channels are not candidates.
3. If Base Color is empty, choose a filename recognized as Base Color/Diffuse/Albedo; otherwise use the first
   selected file. Queue it first.
4. Match remaining filenames by delimited long or common short tokens, including BaseColor/Albedo/D, Normal/NRM/N,
   Height/Bump/H, Roughness/R, Metallic/M, AO, Specular/S, Opacity/Alpha, Edge Mask, and Material ID.
5. Assign unmatched files to the next unclaimed empty slot in visible channel order and stop at ten slots.
6. Import sequentially. Each successful native command returns a fresh project snapshot and immediately updates
   the UI, so a later error cannot hide earlier committed files.

The algorithm is deliberately assignment assistance, not semantic image analysis. Users correct an inference by
using an individual slot’s Add/Replace action.

### Implemented Rename Logic

Top-bar editing sends a typed `ProjectNameRequest`. The native command validates 1–255 characters, updates project
metadata and the autosave journal in one transaction, marks the session dirty, refreshes recovery, returns any
recovery warning with the successful snapshot, and refreshes Recent Projects best-effort. Explicit Save advances
the last-saved baseline. Escape in the editor discards only the uncommitted text edit.

The MVP deliberately uses slot cards rather than a node graph. Later channel controls—range, invert, normal
orientation/strength, bump scale, and interpretation—attach to the relevant slot and compile into authoritative
render operations; UI wiring never becomes renderer truth.

## 3. Patch Capture Contract

`N` begins a new patch. Rectangle capture produces four editable corners and commits on release. Four-point
capture accepts any click order, canonicalizes it internally, and commits automatically after the fourth valid
point. The user never needs to follow homography winding rules. Capture validates convexity, minimum area,
bounds, and self-intersection. The active interaction simultaneously drives:

1. the source overlay,
2. the rectified patch preview, and
3. the fixed hotspot workpiece on the right.

There is no Done step for four-point or rectangle capture. Escape cancels incomplete capture or an active
manipulation. A committed patch can be moved immediately, resized/rotated through selection handles, or
double-clicked for labeled point editing. Undo removes the automatically committed patch as one command.

Polygon assistance accepts four to eight boundary samples, derives a best-fit quadrilateral for perspective
rectification, and optionally retains the polygon as a mask. The quadrilateral stays editable and the UI labels
the fit when it is approximate.

## 4. Layout Intent

Layout presets are versioned project intent, not destructive templates. Layout regions exist independently of
patches and may reference an entire material source, a captured patch, or a simple fill. Projects may contain
many ordered material-source sets, each with registered maps and zero or more patches:

- Balanced: general mixed patches.
- Horizontal Trims: long horizontal strips and caps.
- Vertical Trims: columns and vertical architectural pieces.
- Modular Kit: repeatable wall/edge modules with stable dimensions.
- Atlas: mixed unique regions where packing density is preferred.

New Project or first Patches & Layout entry offers the presets with descriptions. The choice can change later.
Regeneration respects locked regions, stable region IDs, patch definitions, and manual constraints. A preset
change previews consequences before replacing the current unlocked layout.

The left workplace owns sources and patches; the right workpiece owns the assembled sheet. Rectification updates
in the right workpiece from cached GPU data during manipulation and receives authoritative native refinement
after interaction settles. Export and Send to Blender consume authoritative output state through separate commands.

## 5. Persistence and Recovery Invariants

- SQLite commits are authoritative autosave state; explicit Save advances an immutable last-saved baseline.
- Baselines use unique generation filenames. A new baseline is fully validated and flushed before the session
  pointer advances; the previous generation is removed only after success.
- Recovery failure after an authoritative command becomes a project warning. It does not report the committed
  command as failed or leave dirty state false.
- Recovery, dirty-close, and other modal states are mutually exclusive.
- Recent Projects is non-authoritative. Failure to update it is logged and never changes the result of project
  creation, opening, Save As, or recovery.

## 6. Accessibility and Input

All direct manipulation has keyboard and numeric alternatives. Modal focus is contained and restored. Work
modes, slots, source assets, patch rows, and warnings expose names and selected/disabled state. UI remains usable
at 100%–300% scale, honors reduced motion, and never communicates channel identity through color alone.
