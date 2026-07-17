# Algorithm Stage 02 — Color, alpha, data, normal, and pyramid normalization

## Delivered authority

Stage 2 consumes `RegisteredChannelSet` metadata and immutable bytes addressed by each registered
`SourceId`. It rechecks the Stage 1 digest, interpretation, orientation, and dimensions before
decoding. No decoded pixels are persisted in domain records and no generic RGBA8 buffer is exposed
as authoritative computation data.

The image-I/O boundary now publishes tiled typed planes for linear color, sRGB display color,
linear scalar data, canonical tangent normals, categorical IDs, and masks. Tiles are deterministic
bounded work units and cancellation is checked before each tile.

## Decode and filtering routes

- Base Color applies an embedded ICC profile into sRGB, resolves straight versus premultiplied
  alpha under an explicit policy, converts to linear sRGB for computation, retains an sRGB display
  pyramid, and reports missing ICC assumptions, alpha resolution, clipped highlights, and crushed
  shadows.
- Height, Roughness, Metallic, AO, and Specular use normalized source samples directly. They never
  pass through a display transfer function.
- Opacity and Edge Mask use typed coverage planes. Material ID packs exact RGB category values and
  downsamples with deterministic nearest sampling, never interpolation.
- Tangent normals require an OpenGL or DirectX convention (including an explicit policy for an
  unspecified Stage 1 record), convert DirectX Y into canonical OpenGL, reject nearly-zero vectors,
  normalize level zero, filter decoded vectors at lower levels, and renormalize every result. Normal
  alpha handling is explicit.
- All channel pyramids use the same ceil-halving dimensions and full-plane integer bounds at every
  level. Color uses alpha-aware linear filtering; scalar/mask values average in linear space; normal
  vectors have a dedicated vector path; IDs have a dedicated categorical path.

## Bounds and caching

The conservative peak allocation is calculated before decoding begins. It includes every retained
typed pyramid, worst-case tile and vector allocation overhead (including growth), full-resolution
decoder/color-transform working buffers, and copied embedded-profile bytes. The image decoder also
receives the per-channel scratch bound instead of the whole job limit. A request fails with
`MemoryLimit` before decode when this peak exceeds the declared bound. Prepared cache keys include
every registered source digest and interpretation (including normal convention), decode policy,
working-space identity and version, pyramid version, tile edge, and level policy. The bounded
in-memory prepared-set cache never publishes partial work.

## Focused evidence

`algorithm_stage_02_normalization` covers linear scalar preservation, DirectX-to-OpenGL normal
conversion before vector filtering, normal-convention cache separation, normal renormalization,
categorical nearest sampling, identical registered level dimensions, conservative preflight memory
failure, cancellation, and quantization-aware malformed near-zero normal rejection. The existing
image-I/O ICC tests exercise valid embedded profile handling; malformed profiles return the typed
`ColorProfile` failure. The focused test's Base Color fixture exercises alpha resolution and
clipping/crushing reports.

## Remaining later-stage work

Perspective rectification and shared channel homographies remain Stage 3. De-lighting remains Stage
4. These consumers should use the typed prepared pyramids rather than decode source bytes again.
