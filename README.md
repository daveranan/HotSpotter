# Hot Trimmer

Hot Trimmer is a focused image-to-trim-sheet desktop app.

It is not a full DCC, a general material-library manager, or a Blender/Substance clone. The first
product pass is intentionally narrow:

```text
Open image -> mark patches -> create trim layout -> generate maps -> add treatments -> preview -> export
```

## Project Shape

- `docs/mvp-plan.md` is the product and implementation plan.
- `docs/ux-workflow.md` describes the intended user workflow and screen model.
- `apps/desktop` contains a no-dependency desktop prototype for the focused workflow.
- `scripts/build.mjs` copies the prototype into `apps/desktop/dist`.

## Local Commands

```powershell
npm.cmd run build
npm.cmd run check
```

Open `apps/desktop/dist/index.html` to view the prototype after building.
