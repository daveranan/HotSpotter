# Hot Trimmer UX Workflow

The UI should feel like a small, purpose-built DCC, not a full DCC.

## Screen Model

Top workflow bar:

```text
Open Image | Mark Patches | Layout | Generate Maps | Polish | Preview | Export
```

Left side:

- Minimal tool strip.
- Select.
- Add Patch.
- Move.
- Pan.
- Zoom.
- Mask and Paint only after maps exist.

Center:

- Source image during Mark Patches.
- Trim layout during Layout, Generate Maps, and Polish.
- Trim layout plus 3D preview during Preview.

Right:

- Patches list while marking.
- Layers list while polishing.
- Properties for the selected patch, layer, map, or export preset.

Bottom:

- Compact tray.
- Imported image.
- Extracted patches.
- Generated maps.
- Warnings.

## UX Principles

- One obvious next action per step.
- Direct manipulation first, numeric editing second.
- Expert controls are present but collapsed by default.
- Use Layers, not Stack.
- Use Patch, not Quad or Rip Asset.
- Generated maps are visibly labeled Estimated.
- Undo covers patch creation, layout edits, map generation settings, and layer edits.
- The user should never need to understand UV sets to complete the MVP workflow.
- Dense UI is okay; dense concepts are not.

## First Prototype Screen

The first prototype should show:

- A single source image workspace.
- A visible Add Patch button.
- Four-point patch handles.
- A right-side Patches panel.
- A Generate Maps button disabled until at least one patch exists.
- A Create Trim Sheet button enabled after patch selection.
- A compact bottom tray with source, patches, maps, and warnings.
