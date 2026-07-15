# Hot Trimmer UX Workflow

The UI should feel like a small, purpose-built DCC, not a wizard and not a full DCC.

## Delivery State

- The integrated material/patch workplace and real-time rectification are implemented through Phase 2.
- Multiple material-source sets and the authoritative hotspot layout are Phase 3 contracts.
- Generated maps and multi-source treatment layers are Phase 4 and Phase 5 contracts.
- Export and Send to Blender are separate disabled placeholders until Phase 6.
- Rectification is embedded in the fixed right workpiece; floating preview cards are not part of the contract.

## Workspaces

The dependency loop remains important, but source import is not a separate destination:

```text
Workbench & Hotspot Sheet | Layers & Maps                          Export | Send to Blender
```

Workspaces are destinations, not numbered tasks. They preserve selection, viewport, and inspector context. One is
disabled only while its underlying capability is absent; enabled controls always perform an action.

## Screen Model

- **Top:** native project actions, directly editable project identity/save state, work modes, Export, and Send
  to Blender.
- **Left half:** the workplace. Phase 3 adds a narrow persistent material-source library rail; the selected source
  exposes explicit registered map slots above its patch canvas and hosts capture/direct manipulation. Middle mouse
  pans and the wheel zooms.
- **Right half:** the workpiece. It always represents the evolving hotspot sheet and shows real-time rectification
  until Phase 3 replaces the temporary region strip with authoritative layout editing.
- **Right inspector:** one patch or layer list plus contextual behavior. It never duplicates selection in a bottom tray.
- **Bottom:** status, explicit warnings, project/save state, schema, and offline status only.

## Material Sources

- **Open images** works before project creation; the project save location follows.
- **Open all** accepts multiple images and auto-assigns common texture naming conventions. Base Color imports
  first and only matching empty slots are filled. Ambiguous or occupied-role files remain unimported and direct the
  user to an explicit channel slot instead of being guessed into a data-map role.
- Show ten stable role slots: Base Color/Diffuse, Normal, Height/Bump, Roughness, Metallic, AO, Specular,
  Opacity, Edge Mask, and Material ID.
- Phase 3 adds many ordered material-source sets. Each represents one material idea, owns registered maps, and
  owns zero or many optional patches. The selected set remains in the left workplace while the sheet stays visible.
- Adding a map targets an explicit channel on the selected source set. Adding an independent photograph/material
  creates another source-set entry in the Phase 3 rail; it is never guessed to be a Normal or Height map.
- Base Color anchors registration. Companion maps with mismatched dimensions are blocked locally.
- Ownership, ICC, alpha, and linear/color policy are backend safety details and are not routine Sources controls.
- “Recovery updated · explicit Save pending” replaces the contradictory “Autosaved · unsaved” label.

## Patch Workplace

- `N`, four-point, and rectangle capture are first-class. Four clicks may occur in any order and auto-finish;
  rectangle capture auto-finishes on release. There is no Done step.
- Single selection exposes move, resize, and rotate transforms. Double-click exposes labeled TL/TR/BR/BL point
  handles. A valid new patch remains selected for immediate manipulation.
- Cached GPU rectification updates in the right workpiece throughout pointer motion; native refinement follows
  without blocking interaction.
- One patch list supports double-click rename and drag reorder. Right-click supplies duplicate, enable, and delete.
- Four-to-eight-point polygon assistance derives an editable quadrilateral plus optional mask; arbitrary shapes
  are never presented as exact four-point perspective solutions.

## Layout Presets

Balanced, Horizontal Trims, Vertical Trims, Modular Kit, and Atlas presets explain their best use. Selection is
offered during New Project or first layout entry, stored in the project, and changeable later. Changing a preset
reruns only unlocked layout intent and never deletes patches or locked manual edits without confirmation.

## Preview and Output

The right workpiece shows the selected rectified patch during Phase 2 and becomes the authoritative layout in
Phase 3. Each enabled patch becomes its own placeable layout region rather than replacing the full canvas; no
floating card covers the source.
Export and Send to Blender remain distinct top-level actions and return to the same authoring context.

## UX Principles

- Direct manipulation first, numeric editing second.
- Expert controls are present but collapsed by default.
- Use Layers, not Stack; Patch, not Quad or Rip Asset.
- Generated maps are visibly labeled Estimated.
- Undo covers patch creation, layout edits, map settings, and layer edits.
- The user never needs to understand UV sets or a node graph to complete the MVP.
- Dense UI is acceptable; contradictory state and inert controls are not.
