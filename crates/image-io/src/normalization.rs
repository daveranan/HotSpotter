//! Stage 2 canonical channel decoding and registered, bounded pyramids.

use std::{collections::BTreeMap, io::Cursor, sync::Arc};

use hot_trimmer_domain::{
    ContentDigest, MaterialChannelRole, NormalConvention, RegisteredChannel, RegisteredChannelSet,
    SourceId,
};
use image::{DynamicImage, ImageFormat, ImageReader, Limits};
use moxcms::{ColorProfile, Layout, TransformOptions};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{CancellationToken, ImageIoError, apply_orientation, extract_icc_profile, read_orientation};

pub const STAGE_02_ALGORITHM_ID: &str = "hot_trimmer.channel_normalization";
pub const STAGE_02_ALGORITHM_VERSION: &str = "2.0.0";
pub const DEFAULT_TILE_EDGE: u32 = 128;
pub const DEFAULT_MAX_PYRAMID_BYTES: u64 = 1_073_741_824;
/// An 8-bit neutral normal encodes as 127 or 128 in each component. Its decoded
/// squared length is `3 / 255^2`, so the validity floor must sit above that
/// quantization artifact rather than merely above floating-point zero.
pub const MIN_NORMAL_LENGTH_SQUARED: f32 = 1.0e-4;

// Conservative accounting constants. The tile allowance covers the tile value,
// inner Vec, outer Vec growth, and alignment even when tile_edge is one pixel.
const TILE_ALLOCATION_OVERHEAD_BYTES: u64 = 128;
// Covers a decoded float RGBA image, a converted working copy, ICC output, and
// one additional decoder/color-transform work buffer at full resolution.
const DECODE_SCRATCH_BYTES_PER_PIXEL: u64 = 64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearColor {
    pub rgb: [f32; 3],
    pub alpha: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SrgbDisplayColor(pub [u8; 4]);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearScalar(pub f32);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TangentNormal {
    /// Canonical OpenGL tangent-space vector.
    pub xyz: [f32; 3],
    pub alpha: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CategoryId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaskValue(pub f32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlaneBounds {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImageTile<T> {
    pub bounds: PlaneBounds,
    pub pixels: Vec<T>,
}

/// A dense plane split into deterministic bounded allocation/work units.
#[derive(Clone, Debug, PartialEq)]
pub struct ImagePlane<T> {
    width: u32,
    height: u32,
    tile_edge: u32,
    tiles: Vec<ImageTile<T>>,
}

impl<T> ImagePlane<T> {
    #[must_use]
    pub const fn width(&self) -> u32 { self.width }

    #[must_use]
    pub const fn height(&self) -> u32 { self.height }

    #[must_use]
    pub const fn tile_edge(&self) -> u32 { self.tile_edge }

    #[must_use]
    pub fn tiles(&self) -> &[ImageTile<T>] { &self.tiles }

    #[must_use]
    pub const fn bounds(&self) -> PlaneBounds {
        PlaneBounds { x: 0, y: 0, width: self.width, height: self.height }
    }

    /// Reads a registered pixel. Coordinates must be inside [`Self::bounds`].
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> &T {
        let tiles_x = self.width.div_ceil(self.tile_edge);
        let tile_index = (y / self.tile_edge) * tiles_x + x / self.tile_edge;
        let tile = &self.tiles[usize::try_from(tile_index).expect("bounded tile index")];
        let local = (y - tile.bounds.y) * tile.bounds.width + (x - tile.bounds.x);
        &tile.pixels[usize::try_from(local).expect("bounded pixel index")]
    }
}

impl<T: Clone> ImagePlane<T> {
    /// Builds a tiled plane from row-major pixels while retaining deterministic tile bounds.
    ///
    /// # Errors
    ///
    /// Returns a typed failure for zero dimensions/tile size or a mismatched pixel count.
    pub fn from_row_major(
        width: u32,
        height: u32,
        tile_edge: u32,
        pixels: &[T],
    ) -> Result<Self, NormalizationError> {
        if width == 0 || height == 0 || tile_edge == 0 {
            return Err(NormalizationError::InvalidPlaneDimensions);
        }
        let expected = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(NormalizationError::InvalidPlaneDimensions)?;
        if pixels.len() != expected {
            return Err(NormalizationError::InvalidPlaneBuffer {
                expected,
                found: pixels.len(),
            });
        }
        let mut tiles = Vec::new();
        for tile_y in (0..height).step_by(usize::try_from(tile_edge).map_err(|_| NormalizationError::InvalidTileEdge)?) {
            for tile_x in (0..width).step_by(usize::try_from(tile_edge).map_err(|_| NormalizationError::InvalidTileEdge)?) {
                let tile_width = tile_edge.min(width - tile_x);
                let tile_height = tile_edge.min(height - tile_y);
                let mut tile_pixels = Vec::with_capacity(
                    usize::try_from(u64::from(tile_width) * u64::from(tile_height))
                        .map_err(|_| NormalizationError::InvalidPlaneDimensions)?,
                );
                for y in tile_y..tile_y + tile_height {
                    let start = usize::try_from(u64::from(y) * u64::from(width) + u64::from(tile_x))
                        .map_err(|_| NormalizationError::InvalidPlaneDimensions)?;
                    let end = start + usize::try_from(tile_width)
                        .map_err(|_| NormalizationError::InvalidPlaneDimensions)?;
                    tile_pixels.extend_from_slice(&pixels[start..end]);
                }
                tiles.push(ImageTile {
                    bounds: PlaneBounds { x: tile_x, y: tile_y, width: tile_width, height: tile_height },
                    pixels: tile_pixels,
                });
            }
        }
        Ok(Self { width, height, tile_edge, tiles })
    }

    /// Returns an exact row-major copy. This is primarily useful for stable cache serialization
    /// and proving pass-through byte/value stability.
    #[must_use]
    pub fn to_row_major(&self) -> Vec<T> {
        let mut pixels = Vec::with_capacity(
            usize::try_from(u64::from(self.width) * u64::from(self.height))
                .expect("validated plane dimensions fit usize"),
        );
        for y in 0..self.height {
            for x in 0..self.width {
                pixels.push(self.pixel(x, y).clone());
            }
        }
        pixels
    }
}

pub type LinearColorPlane = ImagePlane<LinearColor>;
pub type SrgbDisplayPlane = ImagePlane<SrgbDisplayColor>;
pub type ScalarPlane = ImagePlane<LinearScalar>;
pub type NormalPlane = ImagePlane<TangentNormal>;
pub type IdPlane = ImagePlane<CategoryId>;
pub type MaskPlane = ImagePlane<MaskValue>;

#[derive(Clone, Debug, PartialEq)]
pub struct ResolutionPyramid<T> {
    levels: Vec<ImagePlane<T>>,
}

impl<T> ResolutionPyramid<T> {
    /// Creates a typed pyramid from validated levels (used by deterministic fixtures and caches).
    pub fn from_levels(levels: Vec<ImagePlane<T>>) -> Result<Self, NormalizationError> {
        if levels.is_empty() {
            return Err(NormalizationError::InvalidPlaneDimensions);
        }
        Ok(Self { levels })
    }

    #[must_use]
    pub fn levels(&self) -> &[ImagePlane<T>] { &self.levels }

    #[must_use]
    pub fn level(&self, level: usize) -> Option<&ImagePlane<T>> { self.levels.get(level) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlphaDecodePolicy {
    ResolveAutomatically,
    Straight,
    Premultiplied,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolvedAlphaMode {
    Opaque,
    Straight,
    Premultiplied,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NormalAlphaPolicy {
    Preserve,
    Ignore,
    ValidityMask,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkingColorSpace {
    LinearSrgb,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodePolicy {
    pub base_color_alpha: AlphaDecodePolicy,
    pub normal_alpha: NormalAlphaPolicy,
    pub unspecified_normal_convention: Option<NormalConvention>,
}

impl Default for DecodePolicy {
    fn default() -> Self {
        Self {
            base_color_alpha: AlphaDecodePolicy::ResolveAutomatically,
            normal_alpha: NormalAlphaPolicy::Preserve,
            unspecified_normal_convention: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NormalizationSettings {
    pub tile_edge: u32,
    /// Includes level zero. Zero means continue to 1x1.
    pub max_levels: u8,
    pub max_memory_bytes: u64,
    pub decode_policy: DecodePolicy,
    pub working_space: WorkingColorSpace,
    pub working_space_version: String,
    pub pyramid_version: String,
}

impl Default for NormalizationSettings {
    fn default() -> Self {
        Self {
            tile_edge: DEFAULT_TILE_EDGE,
            max_levels: 0,
            max_memory_bytes: DEFAULT_MAX_PYRAMID_BYTES,
            decode_policy: DecodePolicy::default(),
            working_space: WorkingColorSpace::LinearSrgb,
            working_space_version: "linear-srgb-v1".into(),
            pyramid_version: "typed-mip-v1".into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NormalizationDiagnostic {
    EmbeddedIccConverted,
    MissingIccAssumedSrgb,
    AlphaModeResolved(ResolvedAlphaMode),
    ClippedHighlights { samples: u64 },
    CrushedShadows { samples: u64 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NormalizationReport {
    pub diagnostics: Vec<NormalizationDiagnostic>,
    pub peak_declared_bytes: u64,
    pub level_dimensions: Vec<(u32, u32)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PreparedChannel {
    BaseColor {
        linear: ResolutionPyramid<LinearColor>,
        srgb_display: ResolutionPyramid<SrgbDisplayColor>,
        alpha_mode: ResolvedAlphaMode,
    },
    Scalar { role: MaterialChannelRole, pyramid: ResolutionPyramid<LinearScalar> },
    Normal {
        pyramid: ResolutionPyramid<TangentNormal>,
        source_convention: NormalConvention,
        canonical_convention: NormalConvention,
        alpha_policy: NormalAlphaPolicy,
    },
    MaterialId { pyramid: ResolutionPyramid<CategoryId> },
    Mask { role: MaterialChannelRole, pyramid: ResolutionPyramid<MaskValue> },
}

impl PreparedChannel {
    #[must_use]
    pub const fn role(&self) -> MaterialChannelRole {
        match self {
            Self::BaseColor { .. } => MaterialChannelRole::BaseColor,
            Self::Scalar { role, .. } | Self::Mask { role, .. } => *role,
            Self::Normal { .. } => MaterialChannelRole::Normal,
            Self::MaterialId { .. } => MaterialChannelRole::MaterialId,
        }
    }

    #[must_use]
    pub fn dimensions(&self) -> Vec<(u32, u32)> {
        let levels: usize = match self {
            Self::BaseColor { linear, .. } => linear.levels.len(),
            Self::Scalar { pyramid, .. } => pyramid.levels.len(),
            Self::Normal { pyramid, .. } => pyramid.levels.len(),
            Self::MaterialId { pyramid } => pyramid.levels.len(),
            Self::Mask { pyramid, .. } => pyramid.levels.len(),
        };
        (0..levels).map(|i| match self {
            Self::BaseColor { linear, .. } => (linear.levels[i].width, linear.levels[i].height),
            Self::Scalar { pyramid, .. } => (pyramid.levels[i].width, pyramid.levels[i].height),
            Self::Normal { pyramid, .. } => (pyramid.levels[i].width, pyramid.levels[i].height),
            Self::MaterialId { pyramid } => (pyramid.levels[i].width, pyramid.levels[i].height),
            Self::Mask { pyramid, .. } => (pyramid.levels[i].width, pyramid.levels[i].height),
        }).collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreparedChannelSet {
    pub cache_key: PreparedChannelCacheKey,
    pub channels: Vec<PreparedChannel>,
    pub report: NormalizationReport,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PreparedChannelCacheKey(pub ContentDigest);

#[derive(Debug)]
pub struct PreparedChannelCache {
    max_entries: usize,
    entries: BTreeMap<PreparedChannelCacheKey, Arc<PreparedChannelSet>>,
}

impl PreparedChannelCache {
    #[must_use]
    pub fn new(max_entries: usize) -> Self { Self { max_entries, entries: BTreeMap::new() } }

    #[must_use]
    pub fn get(&self, key: &PreparedChannelCacheKey) -> Option<Arc<PreparedChannelSet>> {
        self.entries.get(key).cloned()
    }

    pub fn insert(&mut self, prepared: Arc<PreparedChannelSet>) -> Result<(), NormalizationError> {
        if self.max_entries == 0 { return Err(NormalizationError::CacheCapacityZero); }
        if self.entries.len() == self.max_entries && !self.entries.contains_key(&prepared.cache_key) {
            if let Some(oldest) = self.entries.keys().next().cloned() { self.entries.remove(&oldest); }
        }
        self.entries.insert(prepared.cache_key.clone(), prepared);
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum NormalizationError {
    #[error("image plane dimensions must be nonzero and bounded")]
    InvalidPlaneDimensions,
    #[error("image plane buffer has {found} pixels; expected {expected}")]
    InvalidPlaneBuffer { expected: usize, found: usize },
    #[error("registered bytes are missing for channel {role:?}")]
    MissingSourceBytes { role: MaterialChannelRole },
    #[error("registered source bytes changed for channel {role:?}")]
    SourceDigestMismatch { role: MaterialChannelRole },
    #[error("Stage 1 registration is invalid for channel {role:?}")]
    InvalidRegistration { role: MaterialChannelRole },
    #[error("decoded dimensions or orientation no longer match the registered channel {role:?}")]
    RegistrationDrift { role: MaterialChannelRole },
    #[error("normal convention must be explicitly selected before decoding")]
    NormalConventionRequired,
    #[error("normal map contains {count} invalid or nearly-zero vectors")]
    InvalidNormalVectors { count: u64 },
    #[error("normal pyramid produced an invalid vector at level {level}")]
    InvalidFilteredNormal { level: usize },
    #[error("normalization requires {required} bytes, exceeding the declared {limit} byte bound")]
    MemoryLimit { required: u64, limit: u64 },
    #[error("tile edge must be greater than zero")]
    InvalidTileEdge,
    #[error("the prepared-channel cache capacity must be greater than zero")]
    CacheCapacityZero,
    #[error("the image normalization job was cancelled")]
    Cancelled,
    #[error(transparent)]
    Image(#[from] ImageIoError),
}

/// Stage 1 records are the sole metadata authority; this map only supplies their immutable bytes.
pub fn prepare_registered_channel_set(
    registered: &RegisteredChannelSet,
    encoded_sources: &BTreeMap<SourceId, Vec<u8>>,
    settings: &NormalizationSettings,
    cancellation: &CancellationToken,
) -> Result<PreparedChannelSet, NormalizationError> {
    validate_registration(registered)?;
    if settings.tile_edge == 0 { return Err(NormalizationError::InvalidTileEdge); }
    check_cancel(cancellation)?;
    let dimensions = level_dimensions(registered.oriented_size.width, registered.oriented_size.height, settings.max_levels);
    let required = estimate_peak_bytes(&registered.channels, &dimensions, settings.tile_edge);
    if required > settings.max_memory_bytes {
        return Err(NormalizationError::MemoryLimit { required, limit: settings.max_memory_bytes });
    }
    let key = prepared_cache_key(registered, settings);
    let mut diagnostics = Vec::new();
    let mut channels = Vec::with_capacity(registered.channels.len());
    for channel in &registered.channels {
        check_cancel(cancellation)?;
        let bytes = encoded_sources.get(&channel.source_id).ok_or(
            NormalizationError::MissingSourceBytes { role: channel.registration.role },
        )?;
        if ContentDigest::sha256(bytes) != channel.original.immutable_digest {
            return Err(NormalizationError::SourceDigestMismatch { role: channel.registration.role });
        }
        let decoder_limit = decoder_allocation_limit(channel);
        let decoded = decode_registered(bytes, channel, decoder_limit)?;
        let prepared = decode_channel(channel, decoded, settings, cancellation, &mut diagnostics)?;
        if prepared.dimensions() != dimensions {
            return Err(NormalizationError::RegistrationDrift { role: channel.registration.role });
        }
        channels.push(prepared);
    }
    Ok(PreparedChannelSet {
        cache_key: key,
        channels,
        report: NormalizationReport { diagnostics, peak_declared_bytes: required, level_dimensions: dimensions },
    })
}

#[must_use]
pub fn prepared_cache_key(
    registered: &RegisteredChannelSet,
    settings: &NormalizationSettings,
) -> PreparedChannelCacheKey {
    let mut hash = Sha256::new();
    hash.update(STAGE_02_ALGORITHM_ID.as_bytes());
    hash.update(STAGE_02_ALGORITHM_VERSION.as_bytes());
    hash.update(registered.oriented_size.width.to_le_bytes());
    hash.update(registered.oriented_size.height.to_le_bytes());
    for channel in &registered.channels {
        hash.update([channel.registration.role as u8]);
        hash.update([channel.registration.interpretation as u8]);
        hash.update([channel.registration.normal_convention as u8]);
        hash.update(channel.original.immutable_digest.0.as_bytes());
    }
    hash.update([settings.decode_policy.base_color_alpha as u8]);
    hash.update([settings.decode_policy.normal_alpha as u8]);
    hash.update([settings.decode_policy.unspecified_normal_convention.unwrap_or(NormalConvention::Unspecified) as u8]);
    hash.update([settings.working_space as u8]);
    hash.update(settings.working_space_version.as_bytes());
    hash.update(settings.pyramid_version.as_bytes());
    hash.update(settings.tile_edge.to_le_bytes());
    hash.update([settings.max_levels]);
    PreparedChannelCacheKey(ContentDigest(format!("{:x}", hash.finalize())))
}

fn validate_registration(registered: &RegisteredChannelSet) -> Result<(), NormalizationError> {
    for (index, channel) in registered.channels.iter().enumerate() {
        let role = channel.registration.role;
        if channel.registration.interpretation != role.required_interpretation()
            || channel.oriented_size != registered.oriented_size
            || channel.orientation != registered.orientation
            || (index == 0 && role != MaterialChannelRole::BaseColor)
        {
            return Err(NormalizationError::InvalidRegistration { role });
        }
    }
    if registered.channels.is_empty() {
        return Err(NormalizationError::InvalidRegistration { role: MaterialChannelRole::BaseColor });
    }
    Ok(())
}

struct DecodedSource {
    image: DynamicImage,
    icc: Option<Vec<u8>>,
}

fn decode_registered(bytes: &[u8], channel: &RegisteredChannel, max_bytes: u64) -> Result<DecodedSource, NormalizationError> {
    let reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(ImageIoError::Read)?;
    let format = reader.format().ok_or(ImageIoError::UnsupportedFormat)?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Tiff) {
        return Err(ImageIoError::UnsupportedFormat.into());
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(channel.oriented_size.width.max(channel.oriented_size.height));
    limits.max_image_height = Some(channel.oriented_size.width.max(channel.oriented_size.height));
    limits.max_alloc = Some(max_bytes);
    reader.limits(limits);
    let image = reader.decode().map_err(ImageIoError::Decode)?;
    let orientation = read_orientation(Cursor::new(bytes));
    let image = apply_orientation(image, orientation);
    if orientation != channel.orientation || image.width() != channel.oriented_size.width || image.height() != channel.oriented_size.height {
        return Err(NormalizationError::RegistrationDrift { role: channel.registration.role });
    }
    let icc = extract_icc_profile(bytes, format)?;
    Ok(DecodedSource { image, icc })
}

fn decode_channel(
    channel: &RegisteredChannel,
    decoded: DecodedSource,
    settings: &NormalizationSettings,
    cancellation: &CancellationToken,
    diagnostics: &mut Vec<NormalizationDiagnostic>,
) -> Result<PreparedChannel, NormalizationError> {
    match channel.registration.role {
        MaterialChannelRole::BaseColor => decode_base_color(decoded, settings, cancellation, diagnostics),
        MaterialChannelRole::Normal => decode_normal(channel, decoded.image, settings, cancellation),
        MaterialChannelRole::MaterialId => decode_ids(decoded.image, settings, cancellation),
        MaterialChannelRole::Opacity | MaterialChannelRole::EdgeMask => {
            decode_mask(channel.registration.role, decoded.image, settings, cancellation)
        }
        role => decode_scalar(role, decoded.image, settings, cancellation),
    }
}

fn decode_base_color(
    decoded: DecodedSource,
    settings: &NormalizationSettings,
    cancellation: &CancellationToken,
    diagnostics: &mut Vec<NormalizationDiagnostic>,
) -> Result<PreparedChannel, NormalizationError> {
    let mut rgba = decoded.image.to_rgba8();
    if let Some(profile) = decoded.icc {
        let source = ColorProfile::new_from_slice(&profile).map_err(|e| ImageIoError::ColorProfile(e.to_string()))?;
        let destination = ColorProfile::new_srgb();
        let transform = source.create_transform_8bit(Layout::Rgba, &destination, Layout::Rgba, TransformOptions::default())
            .map_err(|e| ImageIoError::ColorProfile(e.to_string()))?;
        let mut converted = vec![0_u8; rgba.as_raw().len()];
        transform.transform(rgba.as_raw(), &mut converted).map_err(|e| ImageIoError::ColorProfile(e.to_string()))?;
        rgba = image::RgbaImage::from_raw(rgba.width(), rgba.height(), converted)
            .ok_or_else(|| ImageIoError::ColorProfile("converted dimensions are invalid".into()))?;
        diagnostics.push(NormalizationDiagnostic::EmbeddedIccConverted);
    } else {
        diagnostics.push(NormalizationDiagnostic::MissingIccAssumedSrgb);
    }
    let raw = rgba.as_raw();
    let has_translucency = raw.chunks_exact(4).any(|p| p[3] != 255);
    let exceeds_alpha = raw.chunks_exact(4).any(|p| p[3] != 255 && p[..3].iter().any(|c| *c > p[3]));
    let alpha_mode = if !has_translucency {
        ResolvedAlphaMode::Opaque
    } else { match settings.decode_policy.base_color_alpha {
        AlphaDecodePolicy::Straight => ResolvedAlphaMode::Straight,
        AlphaDecodePolicy::Premultiplied => ResolvedAlphaMode::Premultiplied,
        AlphaDecodePolicy::ResolveAutomatically if exceeds_alpha => ResolvedAlphaMode::Straight,
        AlphaDecodePolicy::ResolveAutomatically => ResolvedAlphaMode::Premultiplied,
    }};
    diagnostics.push(NormalizationDiagnostic::AlphaModeResolved(alpha_mode));
    let mut clipped = 0_u64;
    let mut crushed = 0_u64;
    let level0 = plane_from_fn(rgba.width(), rgba.height(), settings.tile_edge, cancellation, |x, y| {
        let p = rgba.get_pixel(x, y).0;
        let alpha = f32::from(p[3]) / 255.0;
        let mut encoded = [f32::from(p[0]) / 255.0, f32::from(p[1]) / 255.0, f32::from(p[2]) / 255.0];
        if alpha_mode == ResolvedAlphaMode::Premultiplied {
            if alpha > 0.0 { for c in &mut encoded { *c = (*c / alpha).clamp(0.0, 1.0); } }
            else { encoded = [0.0; 3]; }
        }
        for c in encoded {
            if c >= 254.0 / 255.0 { clipped += 1; }
            if c <= 1.0 / 255.0 { crushed += 1; }
        }
        LinearColor { rgb: encoded.map(srgb_to_linear), alpha }
    })?;
    if clipped > 0 { diagnostics.push(NormalizationDiagnostic::ClippedHighlights { samples: clipped }); }
    if crushed > 0 { diagnostics.push(NormalizationDiagnostic::CrushedShadows { samples: crushed }); }
    let linear = color_pyramid(level0, settings.max_levels, cancellation)?;
    let display_levels = linear.levels.iter().map(|level| map_plane(level, settings.tile_edge, cancellation, |p| {
        let rgb = p.rgb.map(|v| (linear_to_srgb(v).clamp(0.0, 1.0) * 255.0).round() as u8);
        SrgbDisplayColor([rgb[0], rgb[1], rgb[2], (p.alpha.clamp(0.0, 1.0) * 255.0).round() as u8])
    })).collect::<Result<Vec<_>, _>>()?;
    Ok(PreparedChannel::BaseColor { linear, srgb_display: ResolutionPyramid { levels: display_levels }, alpha_mode })
}

fn decode_scalar(role: MaterialChannelRole, image: DynamicImage, settings: &NormalizationSettings, cancellation: &CancellationToken) -> Result<PreparedChannel, NormalizationError> {
    let rgba = image.to_rgba32f();
    let level0 = plane_from_fn(rgba.width(), rgba.height(), settings.tile_edge, cancellation, |x, y| LinearScalar(rgba.get_pixel(x, y).0[0]))?;
    Ok(PreparedChannel::Scalar { role, pyramid: scalar_pyramid(level0, settings.max_levels, cancellation)? })
}

fn decode_mask(role: MaterialChannelRole, image: DynamicImage, settings: &NormalizationSettings, cancellation: &CancellationToken) -> Result<PreparedChannel, NormalizationError> {
    let rgba = image.to_rgba32f();
    let level0 = plane_from_fn(rgba.width(), rgba.height(), settings.tile_edge, cancellation, |x, y| MaskValue(rgba.get_pixel(x, y).0[0]))?;
    Ok(PreparedChannel::Mask { role, pyramid: mask_pyramid(level0, settings.max_levels, cancellation)? })
}

fn decode_ids(image: DynamicImage, settings: &NormalizationSettings, cancellation: &CancellationToken) -> Result<PreparedChannel, NormalizationError> {
    let rgb = image.to_rgb8();
    let level0 = plane_from_fn(rgb.width(), rgb.height(), settings.tile_edge, cancellation, |x, y| {
        let p = rgb.get_pixel(x, y).0;
        CategoryId((u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2]))
    })?;
    Ok(PreparedChannel::MaterialId { pyramid: id_pyramid(level0, settings.max_levels, cancellation)? })
}

fn decode_normal(channel: &RegisteredChannel, image: DynamicImage, settings: &NormalizationSettings, cancellation: &CancellationToken) -> Result<PreparedChannel, NormalizationError> {
    let convention = match channel.registration.normal_convention {
        NormalConvention::OpenGl => NormalConvention::OpenGl,
        NormalConvention::DirectX => NormalConvention::DirectX,
        NormalConvention::Unspecified => settings.decode_policy.unspecified_normal_convention.ok_or(NormalizationError::NormalConventionRequired)?,
        NormalConvention::NotApplicable => return Err(NormalizationError::InvalidRegistration { role: MaterialChannelRole::Normal }),
    };
    let rgba = image.to_rgba32f();
    let mut invalid = 0_u64;
    let level0 = plane_from_fn(rgba.width(), rgba.height(), settings.tile_edge, cancellation, |x, y| {
        let p = rgba.get_pixel(x, y).0;
        let mut xyz = [p[0] * 2.0 - 1.0, p[1] * 2.0 - 1.0, p[2] * 2.0 - 1.0];
        if convention == NormalConvention::DirectX { xyz[1] = -xyz[1]; }
        let length_sq = xyz.iter().map(|v| v * v).sum::<f32>();
        if !length_sq.is_finite() || length_sq <= MIN_NORMAL_LENGTH_SQUARED { invalid += 1; xyz = [0.0, 0.0, 1.0]; }
        else { let inverse = length_sq.sqrt().recip(); xyz = xyz.map(|v| v * inverse); }
        let alpha = match settings.decode_policy.normal_alpha {
            NormalAlphaPolicy::Ignore => 1.0,
            NormalAlphaPolicy::Preserve | NormalAlphaPolicy::ValidityMask => p[3],
        };
        TangentNormal { xyz, alpha }
    })?;
    if invalid > 0 { return Err(NormalizationError::InvalidNormalVectors { count: invalid }); }
    let pyramid = normal_pyramid(level0, settings.max_levels, cancellation)?;
    Ok(PreparedChannel::Normal { pyramid, source_convention: convention, canonical_convention: NormalConvention::OpenGl, alpha_policy: settings.decode_policy.normal_alpha })
}

fn plane_from_fn<T, F>(width: u32, height: u32, tile_edge: u32, cancellation: &CancellationToken, mut f: F) -> Result<ImagePlane<T>, NormalizationError>
where F: FnMut(u32, u32) -> T {
    let mut tiles = Vec::new();
    for y in (0..height).step_by(usize::try_from(tile_edge).expect("nonzero tile edge")) {
        for x in (0..width).step_by(usize::try_from(tile_edge).expect("nonzero tile edge")) {
            check_cancel(cancellation)?;
            let tile_width = tile_edge.min(width - x);
            let tile_height = tile_edge.min(height - y);
            let mut pixels = Vec::with_capacity(usize::try_from(u64::from(tile_width) * u64::from(tile_height)).expect("preflight bounded tile"));
            for py in y..y + tile_height { for px in x..x + tile_width { pixels.push(f(px, py)); } }
            tiles.push(ImageTile { bounds: PlaneBounds { x, y, width: tile_width, height: tile_height }, pixels });
        }
    }
    Ok(ImagePlane { width, height, tile_edge, tiles })
}

fn map_plane<A, B, F>(source: &ImagePlane<A>, tile_edge: u32, cancellation: &CancellationToken, f: F) -> Result<ImagePlane<B>, NormalizationError>
where F: Fn(&A) -> B {
    plane_from_fn(source.width, source.height, tile_edge, cancellation, |x, y| f(source.pixel(x, y)))
}

fn next_dimensions(width: u32, height: u32) -> (u32, u32) { (width.div_ceil(2), height.div_ceil(2)) }

fn should_add_level(width: u32, height: u32, current_levels: usize, max_levels: u8) -> bool {
    (width > 1 || height > 1) && (max_levels == 0 || current_levels < usize::from(max_levels))
}

fn scalar_pyramid(level0: ScalarPlane, max: u8, cancellation: &CancellationToken) -> Result<ResolutionPyramid<LinearScalar>, NormalizationError> {
    let mut levels = vec![level0];
    while should_add_level(levels.last().unwrap().width, levels.last().unwrap().height, levels.len(), max) {
        let source = levels.last().unwrap(); let (w, h) = next_dimensions(source.width, source.height);
        levels.push(plane_from_fn(w, h, source.tile_edge, cancellation, |x, y| LinearScalar(sample_average(source, x, y, |v| v.0)))?);
    }
    Ok(ResolutionPyramid { levels })
}

fn mask_pyramid(level0: MaskPlane, max: u8, cancellation: &CancellationToken) -> Result<ResolutionPyramid<MaskValue>, NormalizationError> {
    let mut levels = vec![level0];
    while should_add_level(levels.last().unwrap().width, levels.last().unwrap().height, levels.len(), max) {
        let source = levels.last().unwrap(); let (w, h) = next_dimensions(source.width, source.height);
        levels.push(plane_from_fn(w, h, source.tile_edge, cancellation, |x, y| MaskValue(sample_average(source, x, y, |v| v.0)))?);
    }
    Ok(ResolutionPyramid { levels })
}

fn id_pyramid(level0: IdPlane, max: u8, cancellation: &CancellationToken) -> Result<ResolutionPyramid<CategoryId>, NormalizationError> {
    let mut levels = vec![level0];
    while should_add_level(levels.last().unwrap().width, levels.last().unwrap().height, levels.len(), max) {
        let source = levels.last().unwrap(); let (w, h) = next_dimensions(source.width, source.height);
        // Categorical values are sampled, never averaged or voted into invented IDs.
        levels.push(plane_from_fn(w, h, source.tile_edge, cancellation, |x, y| *source.pixel((x * 2).min(source.width - 1), (y * 2).min(source.height - 1)))?);
    }
    Ok(ResolutionPyramid { levels })
}

fn color_pyramid(level0: LinearColorPlane, max: u8, cancellation: &CancellationToken) -> Result<ResolutionPyramid<LinearColor>, NormalizationError> {
    let mut levels = vec![level0];
    while should_add_level(levels.last().unwrap().width, levels.last().unwrap().height, levels.len(), max) {
        let source = levels.last().unwrap(); let (w, h) = next_dimensions(source.width, source.height);
        levels.push(plane_from_fn(w, h, source.tile_edge, cancellation, |x, y| {
            let samples = samples_2x2(source, x, y);
            let alpha = samples.iter().map(|p| p.alpha).sum::<f32>() / samples.len() as f32;
            let mut premultiplied = [0.0; 3];
            for p in &samples { for (sum, c) in premultiplied.iter_mut().zip(p.rgb) { *sum += c * p.alpha; } }
            let rgb = if alpha > 1.0e-8 { premultiplied.map(|v| v / (samples.len() as f32 * alpha)) } else { [0.0; 3] };
            LinearColor { rgb, alpha }
        })?);
    }
    Ok(ResolutionPyramid { levels })
}

fn normal_pyramid(level0: NormalPlane, max: u8, cancellation: &CancellationToken) -> Result<ResolutionPyramid<TangentNormal>, NormalizationError> {
    let mut levels = vec![level0];
    while should_add_level(levels.last().unwrap().width, levels.last().unwrap().height, levels.len(), max) {
        let level_number = levels.len();
        let source = levels.last().unwrap(); let (w, h) = next_dimensions(source.width, source.height);
        let mut invalid = false;
        let next = plane_from_fn(w, h, source.tile_edge, cancellation, |x, y| {
            let samples = samples_2x2(source, x, y);
            let mut xyz = [0.0; 3]; let mut alpha = 0.0;
            for p in &samples { for (sum, c) in xyz.iter_mut().zip(p.xyz) { *sum += c; } alpha += p.alpha; }
            let length_sq = xyz.iter().map(|v| v * v).sum::<f32>();
            if !length_sq.is_finite() || length_sq <= 1.0e-8 { invalid = true; xyz = [0.0, 0.0, 1.0]; }
            else { let inverse = length_sq.sqrt().recip(); xyz = xyz.map(|v| v * inverse); }
            TangentNormal { xyz, alpha: alpha / samples.len() as f32 }
        })?;
        if invalid { return Err(NormalizationError::InvalidFilteredNormal { level: level_number }); }
        levels.push(next);
    }
    Ok(ResolutionPyramid { levels })
}

fn samples_2x2<T>(source: &ImagePlane<T>, x: u32, y: u32) -> Vec<&T> {
    let sx = x * 2; let sy = y * 2; let mut result = Vec::with_capacity(4);
    for py in sy..(sy + 2).min(source.height) { for px in sx..(sx + 2).min(source.width) { result.push(source.pixel(px, py)); } }
    result
}

fn sample_average<T, F>(source: &ImagePlane<T>, x: u32, y: u32, f: F) -> f32 where F: Fn(&T) -> f32 {
    let samples = samples_2x2(source, x, y);
    samples.iter().map(|p| f(p)).sum::<f32>() / samples.len() as f32
}

fn level_dimensions(mut width: u32, mut height: u32, max_levels: u8) -> Vec<(u32, u32)> {
    let mut levels = vec![(width, height)];
    while should_add_level(width, height, levels.len(), max_levels) {
        (width, height) = next_dimensions(width, height); levels.push((width, height));
    }
    levels
}

fn estimate_peak_bytes(channels: &[RegisteredChannel], dimensions: &[(u32, u32)], tile_edge: u32) -> u64 {
    let pixels = dimensions.iter().map(|(w, h)| u64::from(*w) * u64::from(*h)).sum::<u64>();
    let retained_pixels = channels.iter().map(|channel| pixels.saturating_mul(bytes_per_retained_pixel(channel.registration.role))).sum::<u64>();
    let retained_tiles = channels.iter().map(|channel| {
        let plane_count = if channel.registration.role == MaterialChannelRole::BaseColor { 2 } else { 1 };
        dimensions.iter().map(|(width, height)| {
            u64::from(width.div_ceil(tile_edge)).saturating_mul(u64::from(height.div_ceil(tile_edge)))
        }).sum::<u64>().saturating_mul(plane_count)
    }).sum::<u64>();
    let tile_storage = retained_tiles.saturating_mul(TILE_ALLOCATION_OVERHEAD_BYTES);
    let largest_scratch = channels.iter().map(decoder_allocation_limit).max().unwrap_or(0);
    retained_pixels.saturating_add(tile_storage).saturating_add(largest_scratch)
}

const fn bytes_per_retained_pixel(role: MaterialChannelRole) -> u64 {
    match role {
        MaterialChannelRole::BaseColor => 20, // float RGBA plus retained display RGBA8
        MaterialChannelRole::Normal => 16,
        MaterialChannelRole::MaterialId => 4,
        _ => 4,
    }
}

fn decoder_allocation_limit(channel: &RegisteredChannel) -> u64 {
    let pixels = u64::from(channel.oriented_size.width).saturating_mul(u64::from(channel.oriented_size.height));
    pixels
        .saturating_mul(DECODE_SCRATCH_BYTES_PER_PIXEL)
        // ICC extraction may copy profile bytes while the encoded input remains live.
        .saturating_add(channel.original.encoded_bytes)
}

fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(value: f32) -> f32 {
    if value <= 0.003_130_8 { value * 12.92 } else { 1.055 * value.powf(1.0 / 2.4) - 0.055 }
}

fn check_cancel(cancellation: &CancellationToken) -> Result<(), NormalizationError> {
    if cancellation.is_cancelled() { Err(NormalizationError::Cancelled) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, io::Cursor};

    use hot_trimmer_domain::{
        AssignmentProvenance, ChannelRegistration, ContentDigest, MaterialChannelRole,
        NormalConvention, OrientedPixelSize, OriginalAssetProvenance, RegisteredChannel,
        RegisteredChannelSet, SourceId, SourceOwnershipIntent,
    };
    use image::{DynamicImage, ImageBuffer, ImageFormat};

    use super::*;

    fn png(pixels: Vec<[u8; 4]>, width: u32, height: u32) -> Vec<u8> {
        let raw = pixels.into_iter().flatten().collect::<Vec<_>>();
        let image = DynamicImage::ImageRgba8(ImageBuffer::from_raw(width, height, raw).unwrap());
        let mut bytes = Cursor::new(Vec::new()); image.write_to(&mut bytes, ImageFormat::Png).unwrap(); bytes.into_inner()
    }

    fn registered(role: MaterialChannelRole, bytes: &[u8], width: u32, height: u32, normal: NormalConvention) -> RegisteredChannel {
        RegisteredChannel {
            source_id: SourceId::new(),
            registration: ChannelRegistration {
                role, interpretation: role.required_interpretation(), normal_convention: normal,
                assignment_provenance: AssignmentProvenance::UserAssigned, confidence_milli: 1000,
            },
            oriented_size: OrientedPixelSize { width, height }, orientation: 1,
            original: OriginalAssetProvenance { original_path: "fixture.png".into(), immutable_digest: ContentDigest::sha256(bytes), encoded_bytes: bytes.len() as u64 },
            ownership: SourceOwnershipIntent::OwnedCopy,
        }
    }

    #[test]
    fn algorithm_stage_02_normalization() {
        let base = png(vec![[128, 64, 32, 255], [255, 0, 0, 128], [0, 0, 0, 255], [255, 255, 255, 255]], 2, 2);
        let scalar = png(vec![[128, 128, 128, 255]; 4], 2, 2);
        let normal = png(vec![[128, 64, 255, 255]; 4], 2, 2);
        let ids = png(vec![[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255], [255, 255, 0, 255]], 2, 2);
        let mut channels = vec![
            registered(MaterialChannelRole::BaseColor, &base, 2, 2, NormalConvention::NotApplicable),
            registered(MaterialChannelRole::Roughness, &scalar, 2, 2, NormalConvention::NotApplicable),
            registered(MaterialChannelRole::Normal, &normal, 2, 2, NormalConvention::DirectX),
            registered(MaterialChannelRole::MaterialId, &ids, 2, 2, NormalConvention::NotApplicable),
        ];
        let mut bytes = BTreeMap::new();
        for (channel, source) in channels.iter().zip([base, scalar, normal, ids]) { bytes.insert(channel.source_id, source); }
        let set = RegisteredChannelSet { oriented_size: OrientedPixelSize { width: 2, height: 2 }, orientation: 1, channels: channels.clone() };
        let prepared = prepare_registered_channel_set(&set, &bytes, &NormalizationSettings { tile_edge: 1, ..NormalizationSettings::default() }, &CancellationToken::new()).unwrap();
        assert!(prepared.channels.iter().all(|channel| channel.dimensions() == vec![(2, 2), (1, 1)]));
        let PreparedChannel::Scalar { pyramid, .. } = &prepared.channels[1] else { panic!() };
        assert!((pyramid.level(0).unwrap().pixel(0, 0).0 - 128.0 / 255.0).abs() < 0.001); // no display gamma
        let PreparedChannel::Normal { pyramid, canonical_convention, .. } = &prepared.channels[2] else { panic!() };
        assert_eq!(*canonical_convention, NormalConvention::OpenGl);
        let n = pyramid.level(1).unwrap().pixel(0, 0); assert!((n.xyz.iter().map(|v| v*v).sum::<f32>() - 1.0).abs() < 1e-5);
        assert!(n.xyz[1] > 0.0); // DirectX Y converted to OpenGL before filtering
        let PreparedChannel::MaterialId { pyramid } = &prepared.channels[3] else { panic!() };
        assert_eq!(pyramid.level(1).unwrap().pixel(0, 0), &CategoryId(0xff0000));

        let mut direct_x_set = set.clone();
        let direct_x_key = prepared_cache_key(&direct_x_set, &NormalizationSettings::default());
        direct_x_set.channels[2].registration.normal_convention = NormalConvention::OpenGl;
        assert_ne!(direct_x_key, prepared_cache_key(&direct_x_set, &NormalizationSettings::default()));

        let too_small = NormalizationSettings {
            tile_edge: 1,
            max_memory_bytes: prepared.report.peak_declared_bytes - 1,
            ..NormalizationSettings::default()
        };
        assert!(matches!(prepare_registered_channel_set(&set, &bytes, &too_small, &CancellationToken::new()), Err(NormalizationError::MemoryLimit { .. })));
        let cancelled = CancellationToken::new(); cancelled.cancel();
        assert!(matches!(prepare_registered_channel_set(&set, &bytes, &NormalizationSettings::default(), &cancelled), Err(NormalizationError::Cancelled)));

        let malformed = png(vec![[128, 128, 128, 255]; 4], 2, 2);
        channels[2] = registered(MaterialChannelRole::Normal, &malformed, 2, 2, NormalConvention::OpenGl);
        bytes.insert(channels[2].source_id, malformed);
        let malformed_set = RegisteredChannelSet { channels, ..set };
        assert!(matches!(prepare_registered_channel_set(&malformed_set, &bytes, &NormalizationSettings::default(), &CancellationToken::new()), Err(NormalizationError::InvalidNormalVectors { .. })));
    }
}
