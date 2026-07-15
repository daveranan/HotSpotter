#![doc = "Bounded, color-aware raster inspection and thumbnail decode boundary."]

use std::{
    fs,
    io::{Cursor, Read, Seek},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use exif::{In, Reader as ExifReader, Tag};
use image::{
    DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits,
    codecs::{jpeg::JpegDecoder, png::PngDecoder, tiff::TiffDecoder},
};
use moxcms::{ColorProfile, Layout, TransformOptions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const DEFAULT_MAX_IMAGE_DIMENSION: u32 = 16_384;
pub const DEFAULT_MAX_DECODED_BYTES: u64 = 1_073_741_824;
pub const DEFAULT_MAX_ENCODED_BYTES: u64 = 536_870_912;
pub const THUMBNAIL_MAX_EDGE: u32 = 1_280;
pub const THUMBNAIL_MIP_EDGES: [u32; 3] = [320, 640, THUMBNAIL_MAX_EDGE];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorPolicy {
    ConvertToSrgb,
    PreserveLinearData,
}

#[derive(Clone, Debug, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn same_job(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecodeLimits {
    pub max_dimension: u32,
    pub max_decoded_bytes: u64,
    pub max_encoded_bytes: u64,
}

impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            max_dimension: DEFAULT_MAX_IMAGE_DIMENSION,
            max_decoded_bytes: DEFAULT_MAX_DECODED_BYTES,
            max_encoded_bytes: DEFAULT_MAX_ENCODED_BYTES,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceImageInfo {
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub color_type: String,
    pub has_alpha: bool,
    pub exif_orientation: u16,
    pub has_embedded_icc_profile: bool,
    pub icc_converted_to_srgb: bool,
    pub encoded_bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug)]
pub struct InspectedImage {
    pub info: SourceImageInfo,
    pub source_bytes: Vec<u8>,
    pub thumbnail_png: Vec<u8>,
    pub thumbnail_mipmaps: Vec<ThumbnailMipmap>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedRgba8 {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct ThumbnailMipmap {
    pub max_edge: u32,
    pub png: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum ImageIoError {
    #[error("the source image could not be read: {0}")]
    Read(#[from] std::io::Error),
    #[error("the source is {actual} bytes; the import limit is {limit} bytes")]
    EncodedSizeLimit { actual: u64, limit: u64 },
    #[error("the source format is not supported; use PNG, JPEG, or TIFF")]
    UnsupportedFormat,
    #[error("the source dimensions are invalid or exceed {limit} pixels per edge")]
    DimensionLimit { limit: u32 },
    #[error("the decoded source would exceed the {limit} byte memory limit")]
    DecodedSizeLimit { limit: u64 },
    #[error("the source image is malformed or truncated: {0}")]
    Decode(#[from] image::ImageError),
    #[error("a bounded viewport thumbnail could not be encoded: {0}")]
    ThumbnailEncode(image::ImageError),
    #[error("the embedded ICC profile could not be applied safely: {0}")]
    ColorProfile(String),
    #[error("the image import was cancelled")]
    Cancelled,
}

/// Reads, validates, orients, and thumbnails a supported source image.
///
/// # Errors
///
/// Returns a typed error when the file cannot be read, exceeds a configured bound, uses an unsupported
/// format, or does not decode completely.
pub fn inspect_path(path: &Path, limits: DecodeLimits) -> Result<InspectedImage, ImageIoError> {
    inspect_path_with_policy(path, limits, ColorPolicy::ConvertToSrgb)
}

/// Reads a source under an explicit color policy for Base Color or linear PBR data.
///
/// # Errors
///
/// Returns a typed error when the file or its color profile cannot be processed within the configured bounds.
pub fn inspect_path_with_policy(
    path: &Path,
    limits: DecodeLimits,
    color_policy: ColorPolicy,
) -> Result<InspectedImage, ImageIoError> {
    inspect_path_cancellable(path, limits, color_policy, &CancellationToken::new())
}

/// Reads and inspects a source while checking cancellation between bounded work units.
///
/// # Errors
///
/// Returns a typed error for file, bounds, decode, color-profile, or cancellation failures.
pub fn inspect_path_cancellable(
    path: &Path,
    limits: DecodeLimits,
    color_policy: ColorPolicy,
    cancellation: &CancellationToken,
) -> Result<InspectedImage, ImageIoError> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > limits.max_encoded_bytes {
        return Err(ImageIoError::EncodedSizeLimit {
            actual: metadata.len(),
            limit: limits.max_encoded_bytes,
        });
    }

    let mut file = fs::File::open(path)?;
    let capacity = usize::try_from(metadata.len()).map_err(|_| ImageIoError::EncodedSizeLimit {
        actual: metadata.len(),
        limit: limits.max_encoded_bytes,
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    let mut block = vec![0_u8; 64 * 1024];
    loop {
        ensure_not_cancelled(cancellation)?;
        let read = file.read(&mut block)?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&block[..read]);
    }
    inspect_bytes_cancellable(bytes, limits, color_policy, cancellation)
}

/// Validates and decodes already-owned source bytes under explicit resource limits.
///
/// # Errors
///
/// Returns a typed error when the buffer exceeds a configured bound, uses an unsupported format, or does
/// not decode completely.
pub fn inspect_bytes(
    source_bytes: Vec<u8>,
    limits: DecodeLimits,
) -> Result<InspectedImage, ImageIoError> {
    inspect_bytes_with_policy(source_bytes, limits, ColorPolicy::ConvertToSrgb)
}

/// Validates owned bytes under an explicit Base Color or linear-data color policy.
///
/// # Errors
///
/// Returns a typed error when bounds, decoding, or an applicable embedded profile fails.
pub fn inspect_bytes_with_policy(
    source_bytes: Vec<u8>,
    limits: DecodeLimits,
    color_policy: ColorPolicy,
) -> Result<InspectedImage, ImageIoError> {
    inspect_bytes_cancellable(
        source_bytes,
        limits,
        color_policy,
        &CancellationToken::new(),
    )
}

/// Inspects owned bytes while checking cancellation between decode, profile, and mipmap stages.
///
/// # Errors
///
/// Returns a typed error for bounds, decode, color-profile, or cancellation failures.
pub fn inspect_bytes_cancellable(
    source_bytes: Vec<u8>,
    limits: DecodeLimits,
    color_policy: ColorPolicy,
    cancellation: &CancellationToken,
) -> Result<InspectedImage, ImageIoError> {
    ensure_not_cancelled(cancellation)?;
    let encoded_bytes = u64::try_from(source_bytes.len()).unwrap_or(u64::MAX);
    if encoded_bytes > limits.max_encoded_bytes {
        return Err(ImageIoError::EncodedSizeLimit {
            actual: encoded_bytes,
            limit: limits.max_encoded_bytes,
        });
    }

    let reader = ImageReader::new(Cursor::new(&source_bytes)).with_guessed_format()?;
    let format = reader.format().ok_or(ImageIoError::UnsupportedFormat)?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Tiff
    ) {
        return Err(ImageIoError::UnsupportedFormat);
    }

    let (width, height) = reader.into_dimensions()?;
    if width == 0 || height == 0 || width > limits.max_dimension || height > limits.max_dimension {
        return Err(ImageIoError::DimensionLimit {
            limit: limits.max_dimension,
        });
    }
    let decoded_estimate = u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(16);
    if decoded_estimate > limits.max_decoded_bytes {
        return Err(ImageIoError::DecodedSizeLimit {
            limit: limits.max_decoded_bytes,
        });
    }

    let mut reader = ImageReader::with_format(Cursor::new(&source_bytes), format);
    let mut image_limits = Limits::default();
    image_limits.max_image_width = Some(limits.max_dimension);
    image_limits.max_image_height = Some(limits.max_dimension);
    image_limits.max_alloc = Some(limits.max_decoded_bytes);
    reader.limits(image_limits);
    let decoded = reader.decode().map_err(|error| match error {
        image::ImageError::Limits(limit_error) => {
            let message = limit_error.to_string();
            if message.contains("dimension")
                || message.contains("width")
                || message.contains("height")
            {
                ImageIoError::DimensionLimit {
                    limit: limits.max_dimension,
                }
            } else {
                ImageIoError::DecodedSizeLimit {
                    limit: limits.max_decoded_bytes,
                }
            }
        }
        other => ImageIoError::Decode(other),
    })?;

    ensure_not_cancelled(cancellation)?;
    let icc_profile = extract_icc_profile(&source_bytes, format)?;
    let orientation = read_orientation(Cursor::new(&source_bytes));
    let oriented = apply_orientation(decoded, orientation);
    let mut thumbnail_mipmaps = Vec::with_capacity(THUMBNAIL_MIP_EDGES.len());
    for edge in THUMBNAIL_MIP_EDGES {
        ensure_not_cancelled(cancellation)?;
        let thumbnail = oriented.thumbnail(edge, edge);
        let managed = apply_color_policy(thumbnail, icc_profile.as_deref(), color_policy)?;
        let mut png = Cursor::new(Vec::new());
        managed
            .write_to(&mut png, ImageFormat::Png)
            .map_err(ImageIoError::ThumbnailEncode)?;
        thumbnail_mipmaps.push(ThumbnailMipmap {
            max_edge: edge,
            png: png.into_inner(),
        });
    }
    let thumbnail_png = thumbnail_mipmaps
        .last()
        .map_or_else(Vec::new, |mipmap| mipmap.png.clone());

    let has_alpha = oriented.color().has_alpha();
    let info = SourceImageInfo {
        width: oriented.width(),
        height: oriented.height(),
        format: format_name(format).to_owned(),
        color_type: format!("{:?}", oriented.color()),
        has_alpha,
        exif_orientation: orientation,
        has_embedded_icc_profile: icc_profile.is_some(),
        icc_converted_to_srgb: icc_profile.is_some() && color_policy == ColorPolicy::ConvertToSrgb,
        encoded_bytes,
        sha256: format!("{:x}", Sha256::digest(&source_bytes)),
    };

    Ok(InspectedImage {
        info,
        source_bytes,
        thumbnail_png,
        thumbnail_mipmaps,
    })
}

/// Decodes oriented, color-policy-aware RGBA pixels for authoritative patch rectification.
///
/// # Errors
///
/// Returns a typed failure before unbounded allocation when encoded bytes, dimensions, decoded memory, color
/// profile, or cancellation violate the declared import limits.
pub fn decode_rgba8_bytes_cancellable(
    source_bytes: &[u8],
    limits: DecodeLimits,
    color_policy: ColorPolicy,
    cancellation: &CancellationToken,
) -> Result<DecodedRgba8, ImageIoError> {
    ensure_not_cancelled(cancellation)?;
    let encoded_bytes = u64::try_from(source_bytes.len()).unwrap_or(u64::MAX);
    if encoded_bytes > limits.max_encoded_bytes {
        return Err(ImageIoError::EncodedSizeLimit {
            actual: encoded_bytes,
            limit: limits.max_encoded_bytes,
        });
    }
    let reader = ImageReader::new(Cursor::new(source_bytes)).with_guessed_format()?;
    let format = reader.format().ok_or(ImageIoError::UnsupportedFormat)?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Tiff
    ) {
        return Err(ImageIoError::UnsupportedFormat);
    }
    let (width, height) = reader.into_dimensions()?;
    if width == 0 || height == 0 || width > limits.max_dimension || height > limits.max_dimension {
        return Err(ImageIoError::DimensionLimit {
            limit: limits.max_dimension,
        });
    }
    let decoded_estimate = u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(16);
    if decoded_estimate > limits.max_decoded_bytes {
        return Err(ImageIoError::DecodedSizeLimit {
            limit: limits.max_decoded_bytes,
        });
    }
    let mut reader = ImageReader::with_format(Cursor::new(source_bytes), format);
    let mut image_limits = Limits::default();
    image_limits.max_image_width = Some(limits.max_dimension);
    image_limits.max_image_height = Some(limits.max_dimension);
    image_limits.max_alloc = Some(limits.max_decoded_bytes);
    reader.limits(image_limits);
    let decoded = reader.decode()?;
    ensure_not_cancelled(cancellation)?;
    let orientation = read_orientation(Cursor::new(source_bytes));
    let oriented = apply_orientation(decoded, orientation);
    let icc_profile = extract_icc_profile(source_bytes, format)?;
    let managed = apply_color_policy(oriented, icc_profile.as_deref(), color_policy)?;
    ensure_not_cancelled(cancellation)?;
    let rgba = managed.into_rgba8();
    Ok(DecodedRgba8 {
        width: rgba.width(),
        height: rgba.height(),
        pixels: rgba.into_raw(),
    })
}

fn ensure_not_cancelled(cancellation: &CancellationToken) -> Result<(), ImageIoError> {
    if cancellation.is_cancelled() {
        Err(ImageIoError::Cancelled)
    } else {
        Ok(())
    }
}

fn read_orientation<R: Read + Seek>(reader: R) -> u16 {
    ExifReader::new()
        .read_from_container(&mut std::io::BufReader::new(reader))
        .ok()
        .and_then(|exif| {
            exif.get_field(Tag::Orientation, In::PRIMARY)
                .and_then(|field| field.value.get_uint(0))
        })
        .and_then(|value| u16::try_from(value).ok())
        .filter(|value| (1..=8).contains(value))
        .unwrap_or(1)
}

fn apply_orientation(image: DynamicImage, orientation: u16) -> DynamicImage {
    match orientation {
        2 => image.fliph(),
        3 => image.rotate180(),
        4 => image.flipv(),
        5 => image.rotate90().fliph(),
        6 => image.rotate90(),
        7 => image.rotate270().fliph(),
        8 => image.rotate270(),
        _ => image,
    }
}

const fn format_name(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "PNG",
        ImageFormat::Jpeg => "JPEG",
        ImageFormat::Tiff => "TIFF",
        _ => "Unsupported",
    }
}

fn extract_icc_profile(bytes: &[u8], format: ImageFormat) -> Result<Option<Vec<u8>>, ImageIoError> {
    match format {
        ImageFormat::Png => PngDecoder::new(Cursor::new(bytes))?.icc_profile(),
        ImageFormat::Jpeg => JpegDecoder::new(Cursor::new(bytes))?.icc_profile(),
        ImageFormat::Tiff => TiffDecoder::new(Cursor::new(bytes))?.icc_profile(),
        _ => Ok(None),
    }
    .map_err(ImageIoError::Decode)
}

fn apply_color_policy(
    thumbnail: DynamicImage,
    icc_profile: Option<&[u8]>,
    color_policy: ColorPolicy,
) -> Result<DynamicImage, ImageIoError> {
    let Some(icc_profile) = icc_profile else {
        return Ok(thumbnail);
    };
    if color_policy == ColorPolicy::PreserveLinearData {
        return Ok(thumbnail);
    }
    let source = ColorProfile::new_from_slice(icc_profile)
        .map_err(|error| ImageIoError::ColorProfile(error.to_string()))?;
    let destination = ColorProfile::new_srgb();
    let transform = source
        .create_transform_8bit(
            Layout::Rgba,
            &destination,
            Layout::Rgba,
            TransformOptions::default(),
        )
        .map_err(|error| ImageIoError::ColorProfile(error.to_string()))?;
    let source_pixels = thumbnail.to_rgba8();
    let mut destination_pixels = vec![0; source_pixels.len()];
    transform
        .transform(source_pixels.as_raw(), &mut destination_pixels)
        .map_err(|error| ImageIoError::ColorProfile(error.to_string()))?;
    let converted = image::RgbaImage::from_raw(
        source_pixels.width(),
        source_pixels.height(),
        destination_pixels,
    )
    .ok_or_else(|| ImageIoError::ColorProfile("converted pixel dimensions are invalid".into()))?;
    Ok(DynamicImage::ImageRgba8(converted))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::{
        DynamicImage, ExtendedColorType, ImageBuffer, ImageEncoder, ImageFormat, Luma, Rgba,
        codecs::png::PngEncoder,
    };
    use moxcms::ColorProfile;

    use super::{
        CancellationToken, ColorPolicy, DecodeLimits, ImageIoError, apply_orientation,
        decode_rgba8_bytes_cancellable, inspect_bytes, inspect_bytes_cancellable,
        inspect_bytes_with_policy,
    };

    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let image = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(
            width,
            height,
            Rgba([20, 40, 60, 128]),
        ));
        let mut bytes = Cursor::new(Vec::new());
        image
            .write_to(&mut bytes, ImageFormat::Png)
            .expect("encode fixture PNG");
        bytes.into_inner()
    }

    #[test]
    fn imports_png_with_alpha_and_a_bounded_thumbnail() {
        let result = inspect_bytes(png_bytes(12, 8), DecodeLimits::default()).expect("valid PNG");
        assert_eq!((result.info.width, result.info.height), (12, 8));
        assert_eq!(result.info.format, "PNG");
        assert!(result.info.has_alpha);
        assert!(!result.thumbnail_png.is_empty());
        assert_eq!(result.thumbnail_mipmaps.len(), 3);
        assert_eq!(result.info.sha256.len(), 64);
    }

    #[test]
    fn embedded_rgb_profile_is_detected_and_converted_for_display() {
        let pixels = [20_u8, 40, 60, 255];
        let mut bytes = Vec::new();
        let mut encoder = PngEncoder::new(&mut bytes);
        encoder
            .set_icc_profile(
                ColorProfile::new_srgb()
                    .encode()
                    .expect("encode sRGB profile"),
            )
            .expect("attach ICC profile");
        encoder
            .write_image(&pixels, 1, 1, ExtendedColorType::Rgba8)
            .expect("encode profiled PNG");
        let result = inspect_bytes(bytes, DecodeLimits::default()).expect("profiled PNG");
        assert!(result.info.has_embedded_icc_profile);
        assert!(result.info.icc_converted_to_srgb);
        let data_result = inspect_bytes_with_policy(
            result.source_bytes,
            DecodeLimits::default(),
            ColorPolicy::PreserveLinearData,
        )
        .expect("linear data PNG");
        assert!(data_result.info.has_embedded_icc_profile);
        assert!(!data_result.info.icc_converted_to_srgb);
    }

    #[test]
    fn imports_required_jpeg_and_tiff_formats() {
        for format in [ImageFormat::Jpeg, ImageFormat::Tiff] {
            let image =
                DynamicImage::ImageRgb8(ImageBuffer::from_pixel(7, 5, image::Rgb([20, 40, 60])));
            let mut bytes = Cursor::new(Vec::new());
            image.write_to(&mut bytes, format).expect("encode fixture");
            let result = inspect_bytes(bytes.into_inner(), DecodeLimits::default())
                .expect("import supported format");
            assert_eq!((result.info.width, result.info.height), (7, 5));
        }
    }

    #[test]
    fn jpeg_exif_orientation_is_applied_to_imported_dimensions() {
        let image =
            DynamicImage::ImageRgb8(ImageBuffer::from_pixel(9, 4, image::Rgb([20, 40, 60])));
        let mut encoded = Cursor::new(Vec::new());
        image
            .write_to(&mut encoded, ImageFormat::Jpeg)
            .expect("encode JPEG fixture");
        let mut jpeg = encoded.into_inner();
        let app1 = [
            0xff, 0xe1, 0x00, 0x22, b'E', b'x', b'i', b'f', 0, 0, b'I', b'I', 0x2a, 0, 8, 0, 0, 0,
            1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0,
        ];
        jpeg.splice(2..2, app1);
        let result = inspect_bytes(jpeg, DecodeLimits::default()).expect("oriented JPEG");
        assert_eq!(result.info.exif_orientation, 6);
        assert_eq!((result.info.width, result.info.height), (4, 9));
    }

    #[test]
    fn encoded_and_decoded_memory_limits_fail_before_decode() {
        let bytes = png_bytes(12, 8);
        assert!(matches!(
            inspect_bytes(
                bytes.clone(),
                DecodeLimits {
                    max_encoded_bytes: 8,
                    ..DecodeLimits::default()
                }
            ),
            Err(ImageIoError::EncodedSizeLimit { .. })
        ));
        assert!(matches!(
            inspect_bytes(
                bytes,
                DecodeLimits {
                    max_decoded_bytes: 100,
                    ..DecodeLimits::default()
                }
            ),
            Err(ImageIoError::DecodedSizeLimit { .. })
        ));
    }

    #[test]
    fn cancellation_stops_before_decode_or_persistence() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert!(matches!(
            inspect_bytes_cancellable(
                png_bytes(12, 8),
                DecodeLimits::default(),
                ColorPolicy::ConvertToSrgb,
                &cancellation,
            ),
            Err(ImageIoError::Cancelled)
        ));
    }

    #[test]
    fn authoritative_rgba_decode_preserves_oriented_pixels_and_cancellation() {
        let bytes = png_bytes(12, 8);
        let decoded = decode_rgba8_bytes_cancellable(
            &bytes,
            DecodeLimits::default(),
            ColorPolicy::ConvertToSrgb,
            &CancellationToken::new(),
        )
        .expect("decode RGBA source");
        assert_eq!((decoded.width, decoded.height), (12, 8));
        assert_eq!(decoded.pixels.len(), 12 * 8 * 4);
        assert_eq!(&decoded.pixels[..4], &[20, 40, 60, 128]);

        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert!(matches!(
            decode_rgba8_bytes_cancellable(
                &bytes,
                DecodeLimits::default(),
                ColorPolicy::ConvertToSrgb,
                &cancelled,
            ),
            Err(ImageIoError::Cancelled)
        ));
    }

    #[test]
    fn representative_8k_grayscale_source_is_bounded_and_thumbnails_successfully() {
        let image = DynamicImage::ImageLuma8(ImageBuffer::from_pixel(8192, 8192, Luma([128])));
        let mut bytes = Cursor::new(Vec::new());
        image
            .write_to(&mut bytes, ImageFormat::Png)
            .expect("encode 8K fixture");
        drop(image);
        let started = std::time::Instant::now();
        let result = inspect_bytes(bytes.into_inner(), DecodeLimits::default()).expect("import 8K");
        assert_eq!((result.info.width, result.info.height), (8192, 8192));
        assert!(started.elapsed() < std::time::Duration::from_secs(30));
    }

    #[test]
    fn rejects_oversized_dimensions_before_unbounded_allocation() {
        let result = inspect_bytes(
            png_bytes(12, 8),
            DecodeLimits {
                max_dimension: 10,
                ..DecodeLimits::default()
            },
        );
        assert!(matches!(result, Err(ImageIoError::DimensionLimit { .. })));
    }

    #[test]
    fn rejects_truncated_images() {
        let mut bytes = png_bytes(12, 8);
        bytes.truncate(bytes.len() / 2);
        assert!(matches!(
            inspect_bytes(bytes, DecodeLimits::default()),
            Err(ImageIoError::Decode(_))
        ));
    }

    #[test]
    fn orientation_six_swaps_display_dimensions() {
        let image = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(9, 4, Rgba([0, 0, 0, 255])));
        let oriented = apply_orientation(image, 6);
        assert_eq!((oriented.width(), oriented.height()), (4, 9));
    }
}
