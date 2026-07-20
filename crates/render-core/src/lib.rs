#![doc = "Deterministic authoritative CPU rendering, including inverse-mapped patch rectification."]

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use hot_trimmer_domain::{
    RegionSourceLayer, SourceLayerError, SourceMapping, SourceSamplingMode, SourceWarp,
};
use hot_trimmer_geometry::{GeometryError, Homography, Point, Quadrilateral};
use thiserror::Error;

mod registered_rectification;
mod structural_profile;

pub use registered_rectification::*;

pub use structural_profile::{
    NormalConvention, ProfileKind, StructuralProfile, StructuralProfileError,
    StructuralProfileMaps, StructuralProfileRequest, compile_structural_profile,
    profile_kind_for_slot_key, validate_hotspot_normal_sampling,
};

pub const RENDER_OPERATION_VERSION: u16 = 1;
pub const DEFAULT_TILE_EDGE: u32 = 256;
const MAX_RENDER_OPERATIONS: u64 = 1_073_741_824;

#[derive(Clone, Debug, Default)]
pub struct RenderCancellationToken(Arc<AtomicBool>);

impl RenderCancellationToken {
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SamplingFilter {
    Nearest,
    Bilinear,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SampleSpace {
    SrgbColor,
    LinearData,
}

#[derive(Clone, Copy, Debug)]
pub struct RectificationRequest<'a> {
    pub source_rgba8: &'a [u8],
    pub source_width: u32,
    pub source_height: u32,
    pub quadrilateral: Quadrilateral,
    pub output_width: u32,
    pub output_height: u32,
    pub sampling: SamplingFilter,
    pub sample_space: SampleSpace,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RectifiedImage {
    pub width: u32,
    pub height: u32,
    pub rgba8: Vec<u8>,
}

/// The executable source layer rendered into one already-sized sheet region.  The caller owns
/// compositing, so an error or cancellation can never publish an incompletely updated sheet.
#[derive(Clone, Copy, Debug)]
pub struct RegionLayerRenderRequest<'a> {
    pub source_rgba8: &'a [u8],
    pub source_width: u32,
    pub source_height: u32,
    pub layer: &'a RegionSourceLayer,
    pub output_width: u32,
    pub output_height: u32,
    pub sample_space: SampleSpace,
}

/// Renders a persisted region source recipe through its ordered mapping stack.
///
/// The layer is evaluated as: base mapping, variation/scale, mirror, rotation, then ordered
/// warps.  This is intentionally a standalone image: callers only composite it after the entire
/// operation succeeds.
pub fn render_region_layer_rgba8(
    request: RegionLayerRenderRequest<'_>,
    cancellation: &RenderCancellationToken,
) -> Result<RectifiedImage, RenderError> {
    if request.source_width == 0
        || request.source_height == 0
        || request.output_width == 0
        || request.output_height == 0
    {
        return Err(RenderError::InvalidDimensions);
    }
    if request.source_rgba8.len() != pixel_bytes(request.source_width, request.source_height)? {
        return Err(RenderError::InvalidSourceBuffer);
    }
    request.layer.validate()?;
    let output_bytes = pixel_bytes(request.output_width, request.output_height)?;
    let operation_count = u64::from(request.output_width)
        .checked_mul(u64::from(request.output_height))
        .and_then(|pixels| {
            pixels.checked_mul(u64::try_from(request.layer.warps.len() + 2).unwrap_or(u64::MAX))
        })
        .ok_or(RenderError::OperationTooLarge)?;
    if operation_count > MAX_RENDER_OPERATIONS {
        return Err(RenderError::OperationTooLarge);
    }
    if cancellation.is_cancelled() {
        return Err(RenderError::Cancelled);
    }

    let mapping = LayerMapping::new(request.layer)?;
    let filter = match request.layer.sampling.mode {
        SourceSamplingMode::Nearest => SamplingFilter::Nearest,
        // Cubic is deliberately sampled through the deterministic bilinear CPU path until a
        // cubic kernel is specified for both preview and final output.
        SourceSamplingMode::Linear | SourceSamplingMode::Cubic => SamplingFilter::Bilinear,
    };
    let mut rgba8 = vec![0; output_bytes];
    for y in 0..request.output_height {
        if cancellation.is_cancelled() {
            return Err(RenderError::Cancelled);
        }
        let v = (f64::from(y) + 0.5) / f64::from(request.output_height);
        for x in 0..request.output_width {
            let u = (f64::from(x) + 0.5) / f64::from(request.output_width);
            let source = mapping.map(Point { x: u, y: v }, request.layer)?;
            let sample = sample_rgba8(
                request.source_rgba8,
                request.source_width,
                request.source_height,
                source,
                filter,
                request.sample_space,
            );
            let offset = usize::try_from(
                (u64::from(y) * u64::from(request.output_width) + u64::from(x)) * 4,
            )
            .map_err(|_| RenderError::OutputTooLarge)?;
            rgba8[offset..offset + 4].copy_from_slice(&sample);
        }
    }
    Ok(RectifiedImage {
        width: request.output_width,
        height: request.output_height,
        rgba8,
    })
}

#[derive(Clone, Copy, Debug)]
enum LayerMapping {
    WholeSource,
    Bounds {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
    Perspective(Homography),
}

impl LayerMapping {
    fn new(layer: &RegionSourceLayer) -> Result<Self, RenderError> {
        match &layer.mapping {
            SourceMapping::WholeSource => Ok(Self::WholeSource),
            SourceMapping::Bounds { bounds } => Ok(Self::Bounds {
                x: bounds.x.get(),
                y: bounds.y.get(),
                width: bounds.width.get(),
                height: bounds.height.get(),
            }),
            SourceMapping::Perspective { quad } => Ok(Self::Perspective(
                Quadrilateral::new(*quad)?.source_from_output()?,
            )),
        }
    }

    fn map(self, output: Point, layer: &RegionSourceLayer) -> Result<Point, RenderError> {
        let mut point = match self {
            Self::WholeSource => output,
            Self::Bounds {
                x,
                y,
                width,
                height,
            } => Point {
                x: x + output.x * width,
                y: y + output.y * height,
            },
            Self::Perspective(homography) => {
                homography
                    .transform(output)
                    .ok_or(RenderError::NonInvertibleMapping {
                        operation: "perspective mapping",
                    })?
            }
        };
        point.x = (point.x - 0.5) * layer.sampling.scale + 0.5 + layer.variation_offset[0];
        point.y = (point.y - 0.5) * layer.sampling.scale + 0.5 + layer.variation_offset[1];
        if layer.mirror_x {
            point.x = 1.0 - point.x;
        }
        if layer.mirror_y {
            point.y = 1.0 - point.y;
        }
        let angle = layer.rotation_degrees.to_radians();
        let (sin, cos) = angle.sin_cos();
        let dx = point.x - 0.5;
        let dy = point.y - 0.5;
        point = Point {
            x: cos.mul_add(dx, -sin * dy) + 0.5,
            y: sin.mul_add(dx, cos * dy) + 0.5,
        };
        for (index, warp) in layer.warps.iter().enumerate() {
            point = apply_warp(point, warp, index)?;
        }
        (point.x.is_finite() && point.y.is_finite())
            .then_some(point)
            .ok_or(RenderError::NonInvertibleMapping {
                operation: "source layer transform",
            })
    }
}

fn apply_warp(point: Point, warp: &SourceWarp, index: usize) -> Result<Point, RenderError> {
    let fail = || RenderError::NonInvertibleWarp { index };
    let mapped = match *warp {
        SourceWarp::Planar {
            scale_x,
            scale_y,
            offset_x,
            offset_y,
        } => Point {
            x: point.x.mul_add(scale_x, offset_x),
            y: point.y.mul_add(scale_y, offset_y),
        },
        SourceWarp::Perspective { strength } => {
            let denominator = strength.mul_add(point.y - 0.5, 1.0);
            if denominator.abs() <= f64::EPSILON {
                return Err(fail());
            }
            Point {
                x: (point.x - 0.5) / denominator + 0.5,
                y: point.y / denominator,
            }
        }
        SourceWarp::Polar {
            center_x,
            center_y,
            radius,
        } => {
            let dx = point.x - center_x;
            let dy = point.y - center_y;
            Point {
                x: dy.atan2(dx) / std::f64::consts::TAU + 0.5,
                y: (dx * dx + dy * dy).sqrt() / radius,
            }
        }
        SourceWarp::SpiralTwirl {
            center_x,
            center_y,
            radius,
            strength,
            iterations,
        } => {
            let mut dx = point.x - center_x;
            let mut dy = point.y - center_y;
            let distance = (dx * dx + dy * dy).sqrt();
            if distance <= radius {
                for _ in 0..iterations.max(1) {
                    let angle = strength * (1.0 - distance / radius) / f64::from(iterations.max(1));
                    let (sin, cos) = angle.sin_cos();
                    (dx, dy) = (cos.mul_add(dx, -sin * dy), sin.mul_add(dx, cos * dy));
                }
            }
            Point {
                x: center_x + dx,
                y: center_y + dy,
            }
        }
        SourceWarp::RadialLens {
            center_x,
            center_y,
            radius,
            strength,
        } => {
            let dx = point.x - center_x;
            let dy = point.y - center_y;
            let distance = (dx * dx + dy * dy).sqrt();
            let factor = if distance <= radius {
                1.0 + strength * (1.0 - distance / radius)
            } else {
                1.0
            };
            if factor.abs() <= f64::EPSILON {
                return Err(fail());
            }
            Point {
                x: center_x + dx * factor,
                y: center_y + dy * factor,
            }
        }
        SourceWarp::CylindricalArc {
            radius,
            arc_degrees,
        } => {
            let angle = (point.x - 0.5) * arc_degrees.to_radians();
            Point {
                x: 0.5 + radius * angle.sin(),
                y: point.y,
            }
        }
    };
    (mapped.x.is_finite() && mapped.y.is_finite())
        .then_some(mapped)
        .ok_or_else(fail)
}

/// Reorients a sampled tangent-space normal through the local source mapping Jacobian.  A
/// reflected mapping therefore flips the corresponding tangent component instead of leaving a
/// visually inside-out normal map.
pub fn correct_tangent_space_normal_rgba8(
    normal: [u8; 4],
    jacobian: [[f64; 2]; 2],
) -> Result<[u8; 4], RenderError> {
    if jacobian
        .into_iter()
        .flatten()
        .any(|value| !value.is_finite())
    {
        return Err(RenderError::NonInvertibleMapping {
            operation: "normal mapping Jacobian",
        });
    }
    let x = f64::from(normal[0]) / 127.5 - 1.0;
    let y = f64::from(normal[1]) / 127.5 - 1.0;
    let z = f64::from(normal[2]) / 127.5 - 1.0;
    let mapped_x = jacobian[0][0].mul_add(x, jacobian[1][0] * y);
    let mapped_y = jacobian[0][1].mul_add(x, jacobian[1][1] * y);
    let length = (mapped_x * mapped_x + mapped_y * mapped_y + z * z).sqrt();
    if length <= f64::EPSILON || !length.is_finite() {
        return Err(RenderError::NonInvertibleMapping {
            operation: "normal mapping Jacobian",
        });
    }
    Ok([
        quantize(mapped_x / length * 0.5 + 0.5),
        quantize(mapped_y / length * 0.5 + 0.5),
        quantize(z / length * 0.5 + 0.5),
        normal[3],
    ])
}

/// Rectifies a patch by inverse-mapping output pixel centers through its homography. The operation is
/// deterministic, alpha-aware, and cooperatively cancelable between scanlines.
///
/// # Errors
///
/// Returns a typed error for malformed buffers, invalid dimensions, unstable geometry, excessive output, or
/// cancellation. No partial image is returned.
pub fn rectify_rgba8(
    request: RectificationRequest<'_>,
    cancellation: &RenderCancellationToken,
) -> Result<RectifiedImage, RenderError> {
    rectify_rgba8_with_progress(request, cancellation, |_| {})
}

/// The progress callback receives monotonically increasing values in the inclusive zero-to-one range.
///
/// # Errors
///
/// Has the same failure behavior as [`rectify_rgba8`].
pub fn rectify_rgba8_with_progress(
    request: RectificationRequest<'_>,
    cancellation: &RenderCancellationToken,
    mut report_progress: impl FnMut(f64),
) -> Result<RectifiedImage, RenderError> {
    validate_request(request)?;
    if cancellation.is_cancelled() {
        return Err(RenderError::Cancelled);
    }
    let output_bytes = pixel_bytes(request.output_width, request.output_height)?;
    let source_from_output = request.quadrilateral.source_from_output()?;
    let mut rgba8 = vec![0_u8; output_bytes];
    report_progress(0.0);
    for output_y in 0..request.output_height {
        if cancellation.is_cancelled() {
            return Err(RenderError::Cancelled);
        }
        let normalized_y = (f64::from(output_y) + 0.5) / f64::from(request.output_height);
        for output_x in 0..request.output_width {
            let normalized_x = (f64::from(output_x) + 0.5) / f64::from(request.output_width);
            let sample = source_from_output
                .transform(Point {
                    x: normalized_x,
                    y: normalized_y,
                })
                .map_or([0_u8; 4], |source| {
                    sample_rgba8(
                        request.source_rgba8,
                        request.source_width,
                        request.source_height,
                        source,
                        request.sampling,
                        request.sample_space,
                    )
                });
            let offset =
                (u64::from(output_y) * u64::from(request.output_width) + u64::from(output_x)) * 4;
            let offset = usize::try_from(offset).map_err(|_| RenderError::OutputTooLarge)?;
            rgba8[offset..offset + 4].copy_from_slice(&sample);
        }
        report_progress(f64::from(output_y + 1) / f64::from(request.output_height));
    }
    Ok(RectifiedImage {
        width: request.output_width,
        height: request.output_height,
        rgba8,
    })
}

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("source and output dimensions must be nonzero")]
    InvalidDimensions,
    #[error("source RGBA buffer length does not match its dimensions")]
    InvalidSourceBuffer,
    #[error("rectified output exceeds the bounded allocation limit")]
    OutputTooLarge,
    #[error("render operation count exceeds the bounded limit")]
    OperationTooLarge,
    #[error("rectification was cancelled")]
    Cancelled,
    #[error("patch geometry cannot be rectified: {0}")]
    Geometry(#[from] GeometryError),
    #[error("source layer is invalid: {0}")]
    SourceLayer(#[from] SourceLayerError),
    #[error("source mapping is non-invertible at {operation}")]
    NonInvertibleMapping { operation: &'static str },
    #[error("source warp {index} is non-invertible")]
    NonInvertibleWarp { index: usize },
}

fn validate_request(request: RectificationRequest<'_>) -> Result<(), RenderError> {
    if request.source_width == 0
        || request.source_height == 0
        || request.output_width == 0
        || request.output_height == 0
    {
        return Err(RenderError::InvalidDimensions);
    }
    let source_bytes = pixel_bytes(request.source_width, request.source_height)?;
    if request.source_rgba8.len() != source_bytes {
        return Err(RenderError::InvalidSourceBuffer);
    }
    pixel_bytes(request.output_width, request.output_height)?;
    Ok(())
}

fn pixel_bytes(width: u32, height: u32) -> Result<usize, RenderError> {
    let bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(RenderError::OutputTooLarge)?;
    // Bound a single operation to one GiB even on 64-bit systems.
    if bytes > 1_073_741_824 {
        return Err(RenderError::OutputTooLarge);
    }
    usize::try_from(bytes).map_err(|_| RenderError::OutputTooLarge)
}

fn sample_rgba8(
    pixels: &[u8],
    width: u32,
    height: u32,
    normalized: Point,
    filter: SamplingFilter,
    sample_space: SampleSpace,
) -> [u8; 4] {
    if normalized.x < 0.0 || normalized.x > 1.0 || normalized.y < 0.0 || normalized.y > 1.0 {
        return [0; 4];
    }
    let pixel_x = normalized.x.mul_add(f64::from(width), -0.5);
    let pixel_y = normalized.y.mul_add(f64::from(height), -0.5);
    match filter {
        SamplingFilter::Nearest => {
            let x = clamped_pixel_index(pixel_x.round(), width);
            let y = clamped_pixel_index(pixel_y.round(), height);
            read_pixel(pixels, width, x, y)
        }
        SamplingFilter::Bilinear => {
            let x_floor = pixel_x.floor();
            let y_floor = pixel_y.floor();
            let x_mix = pixel_x - x_floor;
            let y_mix = pixel_y - y_floor;
            let clamp_x = |value: f64| clamped_pixel_index(value, width);
            let clamp_y = |value: f64| clamped_pixel_index(value, height);
            let samples = [
                read_pixel(pixels, width, clamp_x(x_floor), clamp_y(y_floor)),
                read_pixel(pixels, width, clamp_x(x_floor + 1.0), clamp_y(y_floor)),
                read_pixel(pixels, width, clamp_x(x_floor), clamp_y(y_floor + 1.0)),
                read_pixel(
                    pixels,
                    width,
                    clamp_x(x_floor + 1.0),
                    clamp_y(y_floor + 1.0),
                ),
            ];
            let weights = [
                (1.0 - x_mix) * (1.0 - y_mix),
                x_mix * (1.0 - y_mix),
                (1.0 - x_mix) * y_mix,
                x_mix * y_mix,
            ];
            blend_samples(samples, weights, sample_space)
        }
    }
}

fn read_pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let Ok(offset) = usize::try_from((u64::from(y) * u64::from(width) + u64::from(x)) * 4) else {
        return [0; 4];
    };
    pixels[offset..offset + 4]
        .try_into()
        .expect("validated RGBA offset")
}

fn blend_samples(samples: [[u8; 4]; 4], weights: [f64; 4], sample_space: SampleSpace) -> [u8; 4] {
    let mut alpha = 0.0;
    let mut color = [0.0_f64; 3];
    for (sample, weight) in samples.into_iter().zip(weights) {
        let sample_alpha = f64::from(sample[3]) / 255.0;
        alpha += sample_alpha * weight;
        for channel in 0..3 {
            let encoded = f64::from(sample[channel]) / 255.0;
            let decoded = match sample_space {
                SampleSpace::SrgbColor => srgb_to_linear(encoded),
                SampleSpace::LinearData => encoded,
            };
            color[channel] += decoded * sample_alpha * weight;
        }
    }
    if alpha <= f64::EPSILON {
        return [0; 4];
    }
    let mut result = [0_u8; 4];
    for channel in 0..3 {
        let unassociated = (color[channel] / alpha).clamp(0.0, 1.0);
        let encoded = match sample_space {
            SampleSpace::SrgbColor => linear_to_srgb(unassociated),
            SampleSpace::LinearData => unassociated,
        };
        result[channel] = quantize(encoded);
    }
    result[3] = quantize(alpha);
    result
}

fn srgb_to_linear(value: f64) -> f64 {
    if value <= 0.040_45 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f64) -> f64 {
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

fn quantize(value: f64) -> u8 {
    // Clamping makes the rounded conversion fit exactly in an eight-bit channel.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let quantized = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    quantized
}

fn clamped_pixel_index(value: f64, extent: u32) -> u32 {
    let upper = f64::from(extent.saturating_sub(1));
    let clamped = value.clamp(0.0, upper);
    // The value is finite, nonnegative, integral, and no larger than a u32 image extent.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let index = clamped as u32;
    index
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };

    use hot_trimmer_domain::{
        NormalizedPoint, RegionSourceLayer, SourceMapping, SourceRectification,
        SourceRectificationMode, SourceWarp,
    };
    use hot_trimmer_geometry::{Point, Quadrilateral};
    use serde::Deserialize;
    use sha2::{Digest, Sha256};

    use super::{
        RectificationRequest, RegionLayerRenderRequest, RenderCancellationToken, RenderError,
        SampleSpace, SamplingFilter, correct_tangent_space_normal_rgba8, rectify_rgba8,
        rectify_rgba8_with_progress, render_region_layer_rgba8, sample_rgba8,
    };

    fn point(x: f64, y: f64) -> NormalizedPoint {
        NormalizedPoint::new(x, y).expect("test point")
    }

    fn full_source_quad() -> Quadrilateral {
        Quadrilateral::new([
            point(0.0, 0.0),
            point(1.0, 0.0),
            point(1.0, 1.0),
            point(0.0, 1.0),
        ])
        .expect("source quad")
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RectificationGolden {
        source_width: u32,
        source_height: u32,
        source_rgba8: Vec<u8>,
        output_width: u32,
        output_height: u32,
        expected_rgba8: Vec<u8>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RectificationCase {
        name: String,
        source_width: u32,
        source_height: u32,
        output_width: u32,
        output_height: u32,
        corners: [[f64; 2]; 4],
        sample_space: String,
        alpha_pattern: bool,
        expected_sha256: String,
    }

    #[test]
    fn frontal_rectification_is_byte_stable() {
        let golden: RectificationGolden = serde_json::from_str(include_str!(
            "../../../fixtures/renders/phase-2-frontal.json"
        ))
        .expect("valid golden fixture");
        let image = rectify_rgba8(
            RectificationRequest {
                source_rgba8: &golden.source_rgba8,
                source_width: golden.source_width,
                source_height: golden.source_height,
                quadrilateral: full_source_quad(),
                output_width: golden.output_width,
                output_height: golden.output_height,
                sampling: SamplingFilter::Bilinear,
                sample_space: SampleSpace::SrgbColor,
            },
            &RenderCancellationToken::new(),
        )
        .expect("rectification");
        assert_eq!(image.rgba8, golden.expected_rgba8);
    }

    #[test]
    fn skewed_rectification_maps_each_output_corner_region() {
        let pixels: Vec<u8> = (0..16).flat_map(|index| [index * 10, 0, 0, 255]).collect();
        let quad = Quadrilateral::new([
            point(0.25, 0.0),
            point(1.0, 0.25),
            point(0.75, 1.0),
            point(0.0, 0.75),
        ])
        .expect("skewed quad");
        let image = rectify_rgba8(
            RectificationRequest {
                source_rgba8: &pixels,
                source_width: 4,
                source_height: 4,
                quadrilateral: quad,
                output_width: 2,
                output_height: 2,
                sampling: SamplingFilter::Nearest,
                sample_space: SampleSpace::LinearData,
            },
            &RenderCancellationToken::new(),
        )
        .expect("rectification");
        assert_eq!(image.rgba8.len(), 16);
        assert!(image.rgba8.chunks_exact(4).all(|pixel| pixel[3] == 255));
    }

    #[test]
    fn sampling_outside_source_is_transparent() {
        assert_eq!(
            sample_rgba8(
                &[255, 0, 0, 255],
                1,
                1,
                Point { x: -0.01, y: 0.5 },
                SamplingFilter::Bilinear,
                SampleSpace::SrgbColor,
            ),
            [0, 0, 0, 0]
        );
    }

    #[test]
    fn cancellation_discards_partial_work_and_progress_is_monotonic() {
        let cancellation = RenderCancellationToken::new();
        cancellation.cancel();
        let request = RectificationRequest {
            source_rgba8: &[255, 255, 255, 255],
            source_width: 1,
            source_height: 1,
            quadrilateral: full_source_quad(),
            output_width: 8,
            output_height: 8,
            sampling: SamplingFilter::Nearest,
            sample_space: SampleSpace::LinearData,
        };
        assert!(matches!(
            rectify_rgba8(request, &cancellation),
            Err(RenderError::Cancelled)
        ));

        let progress = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&progress);
        rectify_rgba8_with_progress(request, &RenderCancellationToken::new(), move |value| {
            observed.lock().expect("progress lock").push(value);
        })
        .expect("completed render");
        let progress = progress.lock().expect("progress lock");
        assert_eq!(progress.first(), Some(&0.0));
        assert_eq!(progress.last(), Some(&1.0));
        assert!(progress.windows(2).all(|values| values[0] <= values[1]));
    }

    #[test]
    fn rejects_malformed_buffers_before_allocating_output() {
        let result = rectify_rgba8(
            RectificationRequest {
                source_rgba8: &[0; 3],
                source_width: 1,
                source_height: 1,
                quadrilateral: full_source_quad(),
                output_width: 1,
                output_height: 1,
                sampling: SamplingFilter::Nearest,
                sample_space: SampleSpace::LinearData,
            },
            &RenderCancellationToken::new(),
        );
        assert!(matches!(result, Err(RenderError::InvalidSourceBuffer)));
    }

    #[test]
    fn region_layer_planar_perspective_and_mirror_are_deterministic() {
        let source = [10, 0, 0, 255, 200, 0, 0, 255];
        let mut mirrored = RegionSourceLayer {
            mirror_x: true,
            ..RegionSourceLayer::default()
        };
        let render = |layer: &RegionSourceLayer| {
            render_region_layer_rgba8(
                RegionLayerRenderRequest {
                    source_rgba8: &source,
                    source_width: 2,
                    source_height: 1,
                    layer,
                    output_width: 2,
                    output_height: 1,
                    sample_space: SampleSpace::LinearData,
                },
                &RenderCancellationToken::new(),
            )
        };
        let planar = render(&RegionSourceLayer::default()).expect("planar layer");
        let reflected = render(&mirrored).expect("mirrored layer");
        assert_eq!(&planar.rgba8[..4], &[10, 0, 0, 255]);
        assert_eq!(&reflected.rgba8[..4], &[200, 0, 0, 255]);

        mirrored.mirror_x = false;
        mirrored.mapping = SourceMapping::Perspective {
            quad: full_source_quad().corners(),
        };
        mirrored.rectification = SourceRectification {
            mode: SourceRectificationMode::Perspective,
            ..SourceRectification::default()
        };
        assert_eq!(render(&mirrored).expect("perspective layer"), planar);
    }

    #[test]
    fn invalid_region_layer_and_cancellation_return_no_image() {
        let mut layer = RegionSourceLayer::default();
        layer.warps.push(SourceWarp::Polar {
            center_x: 0.5,
            center_y: 0.5,
            radius: 0.0,
        });
        let request = RegionLayerRenderRequest {
            source_rgba8: &[255, 255, 255, 255],
            source_width: 1,
            source_height: 1,
            layer: &layer,
            output_width: 1,
            output_height: 1,
            sample_space: SampleSpace::LinearData,
        };
        assert!(matches!(
            render_region_layer_rgba8(request, &RenderCancellationToken::new()),
            Err(RenderError::SourceLayer(_))
        ));
        let cancellation = RenderCancellationToken::new();
        cancellation.cancel();
        let valid_layer = RegionSourceLayer::default();
        assert!(matches!(
            render_region_layer_rgba8(
                RegionLayerRenderRequest {
                    layer: &valid_layer,
                    ..request
                },
                &cancellation
            ),
            Err(RenderError::Cancelled)
        ));
    }

    #[test]
    fn mirrored_jacobian_flips_the_normal_tangent_handedness() {
        let corrected =
            correct_tangent_space_normal_rgba8([255, 128, 128, 255], [[-1.0, 0.0], [0.0, 1.0]])
                .expect("mirrored normal");
        assert!(corrected[0] < 128);
        assert_eq!(corrected[3], 255);
    }

    #[test]
    fn golden_matrix_covers_rotation_skew_boundary_alpha_color_and_8k() {
        let cases: Vec<RectificationCase> = serde_json::from_str(include_str!(
            "../../../fixtures/renders/phase-2-rectification-cases.json"
        ))
        .expect("valid rectification matrix");
        let mut pending = Vec::new();
        for case in cases {
            let started = Instant::now();
            let mut source = Vec::with_capacity(
                usize::try_from(u64::from(case.source_width) * u64::from(case.source_height) * 4)
                    .expect("bounded fixture"),
            );
            for y in 0..case.source_height {
                for x in 0..case.source_width {
                    let gradient = |value: u32, extent: u32| {
                        u8::try_from(
                            u64::from(value) * 255 / u64::from(extent.saturating_sub(1).max(1)),
                        )
                        .expect("gradient channel")
                    };
                    source.extend_from_slice(&[
                        gradient(x, case.source_width),
                        gradient(y, case.source_height),
                        u8::try_from((x ^ y) & 255).expect("blue channel"),
                        if case.alpha_pattern {
                            u8::try_from(((x + y) % 6) * 51).expect("alpha channel")
                        } else {
                            255
                        },
                    ]);
                }
            }
            let corners = case.corners.map(|[x, y]| point(x, y));
            let output = rectify_rgba8(
                RectificationRequest {
                    source_rgba8: &source,
                    source_width: case.source_width,
                    source_height: case.source_height,
                    quadrilateral: Quadrilateral::new(corners).expect("golden quadrilateral"),
                    output_width: case.output_width,
                    output_height: case.output_height,
                    sampling: SamplingFilter::Bilinear,
                    sample_space: if case.sample_space == "srgb" {
                        SampleSpace::SrgbColor
                    } else {
                        SampleSpace::LinearData
                    },
                },
                &RenderCancellationToken::new(),
            )
            .expect("golden render");
            let digest = format!("{:x}", Sha256::digest(&output.rgba8));
            if case.expected_sha256 == "pending" {
                pending.push(format!("{}={digest}", case.name));
            } else {
                assert_eq!(digest, case.expected_sha256, "golden case {}", case.name);
            }
            if case.name == "high-resolution-8k" {
                assert!(
                    started.elapsed() < Duration::from_secs(5),
                    "8K preview exceeded five-second debug-test budget: {:?}",
                    started.elapsed()
                );
            }
        }
        assert!(
            pending.is_empty(),
            "record golden hashes: {}",
            pending.join(", ")
        );
    }
}
