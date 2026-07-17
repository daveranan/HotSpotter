# Algorithm Stage 03 — Registered geometry and perspective correction

## Delivered authority

Stage 3 consumes Stage 2 `PreparedChannelSet` values and produces typed planar
`PreparedExemplar` values. A rectification job resolves and validates its planar area before
allocating output, builds one immutable inverse-mapped coordinate field, then applies that same
field to every registered channel. A batch is returned only after every requested exemplar is
complete, so invalid geometry cannot publish a partial batch.

The supported geometry routes are editable four-point homography, outline-assisted best-fit
quadrilateral with an optional retained outline mask, and full-frame usable-planar-area selection.
Bounded radial lens correction may be combined with any rectified route. Singular, crossed,
concave, tiny, invalid-mask, invalid-lens, and excessive-output requests return typed failures with
recovery choices.

## Registered sampling and masks

- Base Color is sampled as alpha-aware linear color; the Stage 2 display pyramid is never copied
  into an authoritative exemplar.
- Scalar and coverage channels use bilinear linear-data sampling.
- Tangent normals filter decoded vectors, apply the shared field Jacobian, and renormalize.
- Material IDs use categorical nearest sampling.
- Crop/outline coverage and Base Color alpha thresholds produce a retained typed usable mask.

Rectified dimensions come from the measured source-space quadrilateral, optional physical aspect,
and declared scale, then are bounded by preview or authoritative work limits. Preview and final
requests retain a resolution-independent geometry digest; only their sampled field resolution and
work limits differ. Perspective confidence is retained as deterministic integer metadata.

## Pass-through and invalidation

Already-planar authored textures, front-facing scans, and user-confirmed planar sources have an
explicit `PassThrough` result. This route clones the exact level-zero typed planes without creating
a homography or resampling; it rejects settings that would silently turn pass-through into a
resample.

Prepared cache keys include the Stage 2 key, Stage 3 version, geometry/settings, source-set revision,
and optional patch revision. Cache invalidation targets only the edited patch or replaced source
set. The active source-first workspace requests `prepare_patch_preview` for the selected edited
patch. That typed Tauri command prepares the registered source set through Stage 2, invokes the
shared Stage 3 result, caches the complete exemplar, and derives display PNG pixels only after the
authoritative linear exemplar exists. Patch apply/undo/redo handlers and source import/removal
handlers call the scoped cache invalidators directly.

The active source toolbar exposes an OpenGL (+Y) / DirectX (-Y) Normal convention selector. Both
single-map and automatic multi-map import persist that explicit selection in Stage 1 registration,
so production Stage 2 preparation never needs to guess a convention. Previously stored
`Unspecified` records continue to fail with `NormalConventionRequired` until the user reimports or
confirms them; preview does not install a silent fallback.

## Interactive authoring and large-source preview

Interactive rectification no longer asks Stage 2 to retain authoritative full-resolution pyramids.
Import/open inspection already creates a registered 1280-pixel source mip; the selected-patch
preview reuses those immutable mip bytes for every channel, retains only level zero, and uses a
256 MiB preparation bound before Stage 3 produces the 512-pixel workpiece. The Stage 3 geometry,
channel roles, normal convention, mask, and source/patch revision lineage remain identical. A later
authoritative job continues to use original source bytes and its separately declared limits. This
keeps 8K and larger source interaction proportional to preview resolution instead of original pixel
count and prevents the former multi-gigabyte preview preflight.

The source overlay now follows the specified direct-manipulation model: hover affordance, click to
select, body drag to move, bounded corner resize, pixel-aspect-correct rotation, and double-click to
enter point editing. Point mode uses transparent crosshair handles with generous hit targets and a
stable bottom-left loupe magnified relative to the current viewport. Geometry is committed through
the same typed patch command on pointer release. The selected rectified Stage 3 Base Color is shown
inside the right workpiece rather than a floating card.

## Focused evidence

`algorithm_stage_03_rectification` exercises a skewed registered grid with lens correction and a
combined crop/alpha mask; Base Color and Roughness remain aligned and normals remain unit length.
It also proves pass-through plane equality, preview/final geometry identity, crossed-geometry and
excessive-lens recovery, and patch-local cache invalidation. Identity rectification checks every
normal including the last row and column, where central/one-sided derivatives prevent boundary
distortion. A bounded off-center lens fixture pushes samples outside the source and proves those
pixels receive zero typed channel values and zero retained coverage rather than edge-clamped data.

## Remaining later-stage work

De-lighting and exposure normalization remain Stage 4. Stage 4 must consume only authoritative
linear Base Color from `PreparedExemplar`, retain the original, and leave scalar, normal,
categorical, and mask channels untouched.
