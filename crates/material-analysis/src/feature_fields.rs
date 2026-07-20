//! Stage 7 material-agnostic, registered feature-field extraction.

use std::collections::{BTreeMap, BTreeSet};

use hot_trimmer_domain::{
    AlgorithmProvenance, CompilationDiagnostic, ContentDigest, DiagnosticCode, MaterialChannelRole,
    StageResult,
};
use hot_trimmer_image_io::{ImagePlane, LinearScalar, ResolutionPyramid};
use hot_trimmer_render_core::{PreparedExemplarChannel, RenderCancellationToken};
use thiserror::Error;

use crate::{DelitPreparedExemplar, ScaleOrientationReport};

pub const STAGE_07_ALGORITHM_ID: &str = "hot_trimmer.registered_feature_fields";
pub const STAGE_07_ALGORITHM_VERSION: &str = "7.0.0";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FeatureFieldSettings {
    /// Includes level zero. Analysis stops earlier at 1x1.
    pub max_pyramid_levels: u8,
    pub tile_edge: u32,
    pub stationarity_radius: u8,
    pub max_period_pixels: u16,
    pub max_period_candidates: u8,
    pub min_period_confidence_milli: u16,
    /// Explicit permission to keep suspected text/logo/occluder regions fully usable.
    pub retain_distinctive_content: bool,
    pub max_working_bytes: u64,
    pub max_operations: u64,
}

impl Default for FeatureFieldSettings {
    fn default() -> Self {
        Self {
            max_pyramid_levels: 5,
            tile_edge: 128,
            stationarity_radius: 4,
            max_period_pixels: 64,
            max_period_candidates: 8,
            min_period_confidence_milli: 180,
            retain_distinctive_content: false,
            max_working_bytes: 1_073_741_824,
            max_operations: 2_000_000_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum StructureKind {
    Edge,
    Line,
    Boundary,
    Grid,
    Fiber,
    Intersection,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StructureFields {
    pub edge: ResolutionPyramid<LinearScalar>,
    pub line: ResolutionPyramid<LinearScalar>,
    pub boundary: ResolutionPyramid<LinearScalar>,
    pub grid: ResolutionPyramid<LinearScalar>,
    pub fiber: ResolutionPyramid<LinearScalar>,
    pub intersection: ResolutionPyramid<LinearScalar>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LatticeVector {
    pub dx_pixels: i32,
    pub dy_pixels: i32,
    pub confidence_milli: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LatticeCandidate {
    pub first: LatticeVector,
    pub second: Option<LatticeVector>,
    pub confidence_milli: u16,
    pub evidence_samples: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PeriodicityField {
    pub confidence: ResolutionPyramid<LinearScalar>,
    pub candidates: Vec<LatticeCandidate>,
    /// The search is correlation/DFT-equivalent bounded evidence, never a class-label route.
    pub evidence_method: PeriodicityEvidenceMethod,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeriodicityEvidenceMethod {
    BoundedNormalizedAutocorrelation,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SeamTerm {
    Color,
    Gradient,
    Height,
    VectorNormal,
    Roughness,
    StructuralCrossing,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SeamabilityField {
    pub confidence: ResolutionPyramid<LinearScalar>,
    pub available_terms: BTreeSet<SeamTerm>,
    pub horizontal_cost_milli: u16,
    pub vertical_cost_milli: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UsabilityReason(pub u16);

impl UsabilityReason {
    pub const NONE: Self = Self(0);
    pub const TRANSPARENT_OR_OUTSIDE: Self = Self(1 << 0);
    pub const CLIPPED: Self = Self(1 << 1);
    pub const HIGHLIGHT_UNCERTAINTY: Self = Self(1 << 2);
    pub const SHADOW_UNCERTAINTY: Self = Self(1 << 3);
    pub const SUSPECTED_OCCLUDER_OR_LOGO: Self = Self(1 << 4);
    pub const REGISTRATION_INVALID: Self = Self(1 << 5);
    const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct UsabilityField {
    /// Continuous confidence: uncertainty is retained rather than collapsed to a binary mask.
    pub confidence: ResolutionPyramid<LinearScalar>,
    /// Inspectable reasons, conservatively unioned while downsampling.
    pub reasons: Vec<ImagePlane<UsabilityReason>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FeatureDebugView {
    Saliency,
    Edge,
    Line,
    Boundary,
    Grid,
    Fiber,
    Intersection,
    Stationarity,
    PeriodicityConfidence,
    Seamability,
    UsabilityConfidence,
    UsabilityReasons,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeatureFieldQa {
    pub coordinate_space: &'static str,
    pub level_dimensions: Vec<(u32, u32)>,
    pub views: Vec<FeatureDebugView>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FeatureFieldReport {
    pub cache_key: ContentDigest,
    /// Exact Stage 3 prepared-source lineage; Stage 8 must require an exact match.
    pub prepared_source_digest: ContentDigest,
    /// Exact Stage 6 authority used to construct every registered feature field.
    pub stage_six_cache_key: ContentDigest,
    /// All field planes and source channels are registered to this immutable digest.
    pub registration_digest: ContentDigest,
    pub saliency: ResolutionPyramid<LinearScalar>,
    pub structure: StructureFields,
    pub stationarity: ResolutionPyramid<LinearScalar>,
    pub periodicity: PeriodicityField,
    pub seamability: SeamabilityField,
    pub usability: UsabilityField,
    pub qa: FeatureFieldQa,
    pub stage_result: StageResult,
}

#[derive(Clone, Debug, Default)]
pub struct FeatureFieldCache {
    entries: BTreeMap<ContentDigest, FeatureFieldReport>,
}

impl FeatureFieldCache {
    #[must_use]
    pub fn get(&self, key: &ContentDigest) -> Option<&FeatureFieldReport> {
        self.entries.get(key)
    }
    pub fn insert_complete(&mut self, report: FeatureFieldReport) {
        const MAX_ENTRIES: usize = 16;
        if self.entries.len() >= MAX_ENTRIES
            && !self.entries.contains_key(&report.cache_key)
            && let Some(oldest) = self.entries.keys().next().cloned()
        {
            self.entries.remove(&oldest);
        }
        self.entries.insert(report.cache_key.clone(), report);
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum FeatureFieldError {
    #[error("Stage 7 settings are outside bounded ranges")]
    InvalidSettings,
    #[error("Stage 7 requires at least a 5x5 Base Color")]
    EmptyInput,
    #[error("a registered channel or mask drifted from Base Color coordinates")]
    RegistrationDrift,
    #[error("Stage 7 analysis was cancelled")]
    Cancelled,
    #[error(
        "Stage 7 requires {required_bytes} bytes and {required_operations} operations, exceeding declared limits"
    )]
    ResourceLimitExceeded {
        required_bytes: u64,
        required_operations: u64,
    },
    #[error("Stage 7 could not construct a registered field plane")]
    PlaneConstruction,
}

#[derive(Clone)]
struct BaseEvidence {
    width: u32,
    height: u32,
    luminance: Vec<f32>,
    rgb: Vec<[f32; 3]>,
    gx: Vec<f32>,
    gy: Vec<f32>,
    gradient: Vec<f32>,
}

pub fn extract_feature_fields(
    source: &DelitPreparedExemplar,
    stage_six: &ScaleOrientationReport,
    settings: &FeatureFieldSettings,
    cancellation: &RenderCancellationToken,
) -> Result<FeatureFieldReport, FeatureFieldError> {
    validate_settings(settings)?;
    check_cancel(cancellation)?;
    let base = source.base_color();
    let (width, height) = (base.width(), base.height());
    if width < 5 || height < 5 {
        return Err(FeatureFieldError::EmptyInput);
    }
    validate_registration(source, width, height)?;
    preflight(width, height, source.channels.len(), settings)?;

    let evidence = base_evidence(source, cancellation)?;
    let edge = normalize(&evidence.gradient);
    let (line, intersection) = structure_responses(&evidence, &edge, cancellation)?;
    let boundary = boundary_response(source, &edge);
    let grid: Vec<_> = line
        .iter()
        .zip(&intersection)
        .map(|(l, i)| (l * 0.55 + i * 0.8).min(1.0))
        .collect();
    let fiber = directional_fiber(&line, &evidence, stage_six);
    let saliency = saliency(&evidence, &edge, settings, cancellation)?;
    let stationarity = stationarity(source, &evidence, settings, cancellation)?;
    let candidates = period_candidates(&evidence, settings, cancellation)?;
    let periodic_confidence = periodicity_map(&evidence, &candidates, cancellation)?;
    let seam = seamability(
        source,
        &evidence,
        &edge,
        &candidates,
        settings,
        cancellation,
    )?;
    let (usability, reasons) = usability(source, &saliency, &edge, settings, cancellation)?;
    let dimensions = level_dimensions(width, height, settings.max_pyramid_levels);
    let scalar =
        |values: Vec<f32>| scalar_pyramid(width, height, values, &dimensions, settings.tile_edge);

    let saliency = scalar(saliency)?;
    let edge = scalar(edge)?;
    let line = scalar(line)?;
    let boundary = scalar(boundary)?;
    let grid = scalar(grid)?;
    let fiber = scalar(fiber)?;
    let intersection = scalar(intersection)?;
    let stationarity = scalar(stationarity)?;
    let periodicity_confidence = scalar(periodic_confidence)?;
    let seamability_confidence = scalar(seam.values)?;
    let usability_confidence = scalar(usability)?;
    let reason_levels = reason_pyramid(width, height, reasons, &dimensions, settings.tile_edge)?;
    validate_field_registration(
        &dimensions,
        [
            &saliency,
            &edge,
            &line,
            &boundary,
            &grid,
            &fiber,
            &intersection,
            &stationarity,
            &periodicity_confidence,
            &seamability_confidence,
            &usability_confidence,
        ],
    )?;

    let registration_digest = ContentDigest::sha256(
        format!(
            "{}|{}|{}|{}|{}|{}",
            source.prepared_source_digest.0,
            source.exemplar_id,
            width,
            height,
            stage_six.cache_key.0,
            source
                .channels
                .iter()
                .map(|c| format!("{:?}", c.role()))
                .collect::<Vec<_>>()
                .join(",")
        )
        .as_bytes(),
    );
    let cache_key = feature_field_cache_key(&registration_digest, settings);
    let diagnostics =
        insufficiency_diagnostics(&candidates, &usability_confidence, &seam.available_terms);
    Ok(FeatureFieldReport {
        cache_key,
        prepared_source_digest: source.prepared_source_digest.clone(),
        stage_six_cache_key: stage_six.cache_key.clone(),
        registration_digest,
        saliency,
        structure: StructureFields {
            edge,
            line,
            boundary,
            grid,
            fiber,
            intersection,
        },
        stationarity,
        periodicity: PeriodicityField {
            confidence: periodicity_confidence,
            candidates,
            evidence_method: PeriodicityEvidenceMethod::BoundedNormalizedAutocorrelation,
        },
        seamability: SeamabilityField {
            confidence: seamability_confidence,
            available_terms: seam.available_terms,
            horizontal_cost_milli: score(seam.horizontal_cost),
            vertical_cost_milli: score(seam.vertical_cost),
        },
        usability: UsabilityField {
            confidence: usability_confidence,
            reasons: reason_levels,
        },
        qa: FeatureFieldQa {
            coordinate_space: "registered_source_pixels",
            level_dimensions: dimensions,
            views: vec![
                FeatureDebugView::Saliency,
                FeatureDebugView::Edge,
                FeatureDebugView::Line,
                FeatureDebugView::Boundary,
                FeatureDebugView::Grid,
                FeatureDebugView::Fiber,
                FeatureDebugView::Intersection,
                FeatureDebugView::Stationarity,
                FeatureDebugView::PeriodicityConfidence,
                FeatureDebugView::Seamability,
                FeatureDebugView::UsabilityConfidence,
                FeatureDebugView::UsabilityReasons,
            ],
        },
        stage_result: StageResult::Executed {
            algorithm: AlgorithmProvenance {
                algorithm_id: STAGE_07_ALGORITHM_ID.into(),
                version: STAGE_07_ALGORITHM_VERSION.into(),
            },
            settings_hash: settings_hash(settings),
            diagnostics,
        },
    })
}

#[must_use]
pub fn feature_field_cache_key(
    registration_digest: &ContentDigest,
    settings: &FeatureFieldSettings,
) -> ContentDigest {
    ContentDigest::sha256(
        format!(
            "{}|{}|{}",
            STAGE_07_ALGORITHM_VERSION,
            registration_digest.0,
            settings_hash(settings).0
        )
        .as_bytes(),
    )
}

fn validate_settings(s: &FeatureFieldSettings) -> Result<(), FeatureFieldError> {
    if s.max_pyramid_levels == 0
        || s.max_pyramid_levels > 12
        || s.tile_edge == 0
        || !(2..=32).contains(&s.stationarity_radius)
        || !(2..=512).contains(&s.max_period_pixels)
        || s.max_period_candidates == 0
        || s.max_period_candidates > 32
        || s.min_period_confidence_milli > 1000
        || s.max_working_bytes == 0
        || s.max_operations == 0
    {
        Err(FeatureFieldError::InvalidSettings)
    } else {
        Ok(())
    }
}

fn validate_registration(
    source: &DelitPreparedExemplar,
    width: u32,
    height: u32,
) -> Result<(), FeatureFieldError> {
    if source
        .channels
        .iter()
        .any(|channel| channel.dimensions() != (width, height))
        || source
            .coverage
            .as_ref()
            .is_some_and(|p| (p.width(), p.height()) != (width, height))
        || source.masks.as_ref().is_some_and(|m| {
            [&m.highlight, &m.shadow, &m.clipping, &m.confidence]
                .iter()
                .any(|p| (p.width(), p.height()) != (width, height))
        })
    {
        Err(FeatureFieldError::RegistrationDrift)
    } else {
        Ok(())
    }
}

fn preflight(
    width: u32,
    height: u32,
    channel_count: usize,
    s: &FeatureFieldSettings,
) -> Result<(), FeatureFieldError> {
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .unwrap_or(u64::MAX);
    let required_bytes = pixels
        .checked_mul(160 + channel_count as u64 * 8)
        .unwrap_or(u64::MAX);
    let lags = u64::from(s.max_period_pixels.min(width.max(height) as u16));
    let required_operations = pixels
        .checked_mul(260 + lags * 4 + channel_count as u64 * 16)
        .unwrap_or(u64::MAX);
    if required_bytes > s.max_working_bytes || required_operations > s.max_operations {
        Err(FeatureFieldError::ResourceLimitExceeded {
            required_bytes,
            required_operations,
        })
    } else {
        Ok(())
    }
}

fn base_evidence(
    source: &DelitPreparedExemplar,
    cancellation: &RenderCancellationToken,
) -> Result<BaseEvidence, FeatureFieldError> {
    let base = source.base_color();
    let (w, h) = (base.width(), base.height());
    let mut rgb = Vec::with_capacity((w * h) as usize);
    let mut luminance = Vec::with_capacity(rgb.capacity());
    for y in 0..h {
        if y % 32 == 0 {
            check_cancel(cancellation)?;
        }
        for x in 0..w {
            let c = base.pixel(x, y).rgb;
            rgb.push(c);
            luminance.push(c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722);
        }
    }
    let mut gx = vec![0.0; luminance.len()];
    let mut gy = vec![0.0; luminance.len()];
    let mut gradient = vec![0.0; luminance.len()];
    for y in 1..h - 1 {
        if y % 32 == 0 {
            check_cancel(cancellation)?;
        }
        for x in 1..w - 1 {
            let i = idx(w, x, y);
            gx[i] = (luminance[idx(w, x + 1, y)] - luminance[idx(w, x - 1, y)]) * 0.5;
            gy[i] = (luminance[idx(w, x, y + 1)] - luminance[idx(w, x, y - 1)]) * 0.5;
            gradient[i] = gx[i].hypot(gy[i]);
        }
    }
    Ok(BaseEvidence {
        width: w,
        height: h,
        luminance,
        rgb,
        gx,
        gy,
        gradient,
    })
}

fn structure_responses(
    e: &BaseEvidence,
    edge: &[f32],
    cancellation: &RenderCancellationToken,
) -> Result<(Vec<f32>, Vec<f32>), FeatureFieldError> {
    let mut line = vec![0.0; edge.len()];
    let mut corner = vec![0.0; edge.len()];
    for y in 1..e.height - 1 {
        if y % 32 == 0 {
            check_cancel(cancellation)?;
        }
        for x in 1..e.width - 1 {
            let mut xx = 0.0;
            let mut yy = 0.0;
            let mut xy = 0.0;
            for sy in y - 1..=y + 1 {
                for sx in x - 1..=x + 1 {
                    let j = idx(e.width, sx, sy);
                    xx += e.gx[j] * e.gx[j];
                    yy += e.gy[j] * e.gy[j];
                    xy += e.gx[j] * e.gy[j];
                }
            }
            let trace = xx + yy + 1.0e-8;
            let disc = ((xx - yy) * (xx - yy) + 4.0 * xy * xy).sqrt();
            let coherence = (disc / trace).clamp(0.0, 1.0);
            let i = idx(e.width, x, y);
            line[i] = edge[i] * coherence;
            let det = (xx * yy - xy * xy).max(0.0);
            corner[i] = (det / (trace * trace)).clamp(0.0, 0.25) * 4.0;
        }
    }
    Ok((normalize(&line), normalize(&corner)))
}

fn boundary_response(source: &DelitPreparedExemplar, edge: &[f32]) -> Vec<f32> {
    let explicit = source.channels.iter().find_map(|c| match c {
        PreparedExemplarChannel::Mask {
            role: MaterialChannelRole::EdgeMask,
            plane,
        } => Some(plane),
        _ => None,
    });
    edge.iter()
        .enumerate()
        .map(|(i, value)| {
            let x = i as u32 % source.base_color().width();
            let y = i as u32 / source.base_color().width();
            value.max(explicit.map_or(0.0, |p| p.pixel(x, y).0.clamp(0.0, 1.0)))
        })
        .collect()
}

fn directional_fiber(
    line: &[f32],
    e: &BaseEvidence,
    stage_six: &ScaleOrientationReport,
) -> Vec<f32> {
    let Some(axis) = stage_six.global_orientation.axis_millidegrees else {
        return line.iter().map(|v| v * 0.5).collect();
    };
    let a = axis as f32 / 1000.0 * std::f32::consts::PI / 180.0;
    let (sin, cos) = a.sin_cos();
    line.iter()
        .enumerate()
        .map(|(i, v)| {
            let magnitude = e.gradient[i];
            let alignment = if magnitude > 1.0e-6 {
                ((e.gx[i] * cos + e.gy[i] * sin) / magnitude).abs()
            } else {
                0.0
            };
            // Fibers run perpendicular to their strongest gradient.
            v * (1.0 - alignment) * f32::from(stage_six.global_orientation.confidence_milli)
                / 1000.0
        })
        .collect()
}

fn saliency(
    e: &BaseEvidence,
    _edge: &[f32],
    s: &FeatureFieldSettings,
    cancellation: &RenderCancellationToken,
) -> Result<Vec<f32>, FeatureFieldError> {
    // Oklab keeps the three compared quantities perceptual and like-for-like. In particular,
    // chroma is never compared with a scalar luminance neighborhood mean.
    let mut levels = vec![PerceptualLevel {
        width: e.width,
        height: e.height,
        pixels: e.rgb.iter().copied().map(linear_rgb_to_oklab).collect(),
    }];
    while levels.len() < usize::from(s.max_pyramid_levels) {
        check_cancel(cancellation)?;
        let previous = levels.last().expect("level zero exists");
        if previous.width == 1 && previous.height == 1 {
            break;
        }
        levels.push(downsample_perceptual(previous));
    }

    // Each level is compared with its registered parent. Responses are then fused back into
    // source coordinates, so a uniform mark interior can receive evidence at the scale where its
    // parent contains the surrounding surface.
    let mut responses = Vec::with_capacity(levels.len().saturating_sub(1));
    for level_index in 0..levels.len().saturating_sub(1) {
        let level = &levels[level_index];
        let surround = &levels[level_index + 1];
        let mut response = vec![0.0; level.pixels.len()];
        for y in 0..level.height {
            if y % 32 == 0 {
                check_cancel(cancellation)?;
            }
            for x in 0..level.width {
                let center = level.pixels[idx(level.width, x, y)];
                let parent = surround.pixels[idx(surround.width, x / 2, y / 2)];
                let scale_support = 1.0 + level_index as f32 * 0.10;
                response[idx(level.width, x, y)] =
                    perceptual_distance(center, parent) * scale_support;
            }
        }
        responses.push(response);
    }

    let mut raw = vec![0.0_f32; e.rgb.len()];
    for (level_index, response) in responses.iter().enumerate() {
        let level = &levels[level_index];
        let divisor = 1_u32 << level_index.min(30);
        for y in 0..e.height {
            if y % 32 == 0 {
                check_cancel(cancellation)?;
            }
            for x in 0..e.width {
                let lx = (x / divisor).min(level.width - 1);
                let ly = (y / divisor).min(level.height - 1);
                let i = idx(e.width, x, y);
                raw[i] = raw[i].max(response[idx(level.width, lx, ly)]);
            }
        }
    }

    // Calibrate an absolute confidence instead of forcing every nonconstant image to peak at 1.
    // The median adjacent perceptual difference is a deterministic robust noise estimate; weak
    // texture must clear both that source-relative floor and a fixed visible-contrast floor.
    let noise = adjacent_perceptual_noise(&levels[0]);
    let contrast_floor = 0.03 + noise * 2.5;
    Ok(raw
        .into_iter()
        .map(|value| ((value - contrast_floor) / 0.25).clamp(0.0, 1.0))
        .collect())
}

struct PerceptualLevel {
    width: u32,
    height: u32,
    pixels: Vec<[f32; 3]>,
}

fn linear_rgb_to_oklab(rgb: [f32; 3]) -> [f32; 3] {
    let l = 0.412_221_46_f32
        .mul_add(
            rgb[0],
            0.536_332_55_f32.mul_add(rgb[1], 0.051_445_995 * rgb[2]),
        )
        .max(0.0)
        .cbrt();
    let m = 0.211_903_5_f32
        .mul_add(
            rgb[0],
            0.680_699_5_f32.mul_add(rgb[1], 0.107_396_96 * rgb[2]),
        )
        .max(0.0)
        .cbrt();
    let s = 0.088_302_46_f32
        .mul_add(
            rgb[0],
            0.281_718_85_f32.mul_add(rgb[1], 0.629_978_7 * rgb[2]),
        )
        .max(0.0)
        .cbrt();
    [
        0.210_454_26_f32.mul_add(l, 0.793_617_8_f32.mul_add(m, -0.004_072_047 * s)),
        1.977_998_5_f32.mul_add(l, -2.428_592_2_f32.mul_add(m, 0.450_593_7 * s)),
        0.025_904_037_f32.mul_add(l, 0.782_771_77_f32.mul_add(m, -0.808_675_77 * s)),
    ]
}

fn downsample_perceptual(source: &PerceptualLevel) -> PerceptualLevel {
    let width = source.width.div_ceil(2);
    let height = source.height.div_ceil(2);
    let mut pixels = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            let mut sum = [0.0; 3];
            let mut samples = 0.0;
            for sy in y * 2..(y * 2 + 2).min(source.height) {
                for sx in x * 2..(x * 2 + 2).min(source.width) {
                    let value = source.pixels[idx(source.width, sx, sy)];
                    for channel in 0..3 {
                        sum[channel] += value[channel];
                    }
                    samples += 1.0;
                }
            }
            pixels.push([sum[0] / samples, sum[1] / samples, sum[2] / samples]);
        }
    }
    PerceptualLevel {
        width,
        height,
        pixels,
    }
}

fn perceptual_distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn adjacent_perceptual_noise(level: &PerceptualLevel) -> f32 {
    let mut differences = Vec::with_capacity((level.width * level.height * 2) as usize);
    for y in 0..level.height {
        for x in 0..level.width {
            let value = level.pixels[idx(level.width, x, y)];
            if x + 1 < level.width {
                differences.push(perceptual_distance(
                    value,
                    level.pixels[idx(level.width, x + 1, y)],
                ));
            }
            if y + 1 < level.height {
                differences.push(perceptual_distance(
                    value,
                    level.pixels[idx(level.width, x, y + 1)],
                ));
            }
        }
    }
    if differences.is_empty() {
        return 0.0;
    }
    differences.sort_by(f32::total_cmp);
    differences[differences.len() / 2]
}

fn stationarity(
    source: &DelitPreparedExemplar,
    e: &BaseEvidence,
    s: &FeatureFieldSettings,
    cancellation: &RenderCancellationToken,
) -> Result<Vec<f32>, FeatureFieldError> {
    let r = u32::from(s.stationarity_radius);
    let map_planes: Vec<&ImagePlane<hot_trimmer_image_io::LinearScalar>> = source
        .channels
        .iter()
        .filter_map(|c| match c {
            PreparedExemplarChannel::Scalar { plane, .. } => Some(plane),
            _ => None,
        })
        .collect();
    let mut out = vec![0.0; e.luminance.len()];
    for y in 0..e.height {
        if y % 16 == 0 {
            check_cancel(cancellation)?;
        }
        for x in 0..e.width {
            let (mean, variance) = local_moments(&e.luminance, e.width, e.height, x, y, r);
            let (_, grad_variance) = local_moments(&e.gradient, e.width, e.height, x, y, r);
            let offsets = [
                (r as i32, 0),
                (-(r as i32), 0),
                (0, r as i32),
                (0, -(r as i32)),
            ];
            let mut drift = 0.0;
            for (dx, dy) in offsets {
                let nx = (x as i32 + dx).clamp(0, e.width as i32 - 1) as u32;
                let ny = (y as i32 + dy).clamp(0, e.height as i32 - 1) as u32;
                let (m, v) = local_moments(&e.luminance, e.width, e.height, nx, ny, r);
                drift += (m - mean).abs() + (v - variance).abs().sqrt();
            }
            let mut registered_variance = 0.0;
            for plane in &map_planes {
                let values = scalar_neighborhood(plane, x, y, r);
                registered_variance += variance_slice(&values).sqrt();
            }
            let frequency = grad_variance.sqrt();
            out[idx(e.width, x, y)] =
                1.0 / (1.0 + drift * 3.0 + frequency * 0.5 + registered_variance * 0.5);
        }
    }
    Ok(out)
}

fn period_candidates(
    e: &BaseEvidence,
    s: &FeatureFieldSettings,
    cancellation: &RenderCancellationToken,
) -> Result<Vec<LatticeCandidate>, FeatureFieldError> {
    let max_x = u32::from(s.max_period_pixels).min(e.width / 2).max(2);
    let max_y = u32::from(s.max_period_pixels).min(e.height / 2).max(2);
    let mean = e.luminance.iter().sum::<f32>() / e.luminance.len() as f32;
    let centered: Vec<_> = e.luminance.iter().map(|v| v - mean).collect();
    let mut vectors = Vec::new();
    for (dx, dy, max_lag) in [(1, 0, max_x), (0, 1, max_y)] {
        for lag in 2..=max_lag {
            if lag % 8 == 0 {
                check_cancel(cancellation)?;
            }
            let (corr, samples) = autocorrelation(
                &centered,
                e.width,
                e.height,
                dx * lag as i32,
                dy * lag as i32,
            );
            let (previous, _) = autocorrelation(
                &centered,
                e.width,
                e.height,
                dx * (lag - 1) as i32,
                dy * (lag - 1) as i32,
            );
            let (next, _) = autocorrelation(
                &centered,
                e.width,
                e.height,
                dx * (lag + 1).min(max_lag) as i32,
                dy * (lag + 1).min(max_lag) as i32,
            );
            let prominence = (corr - previous.max(next) * 0.5).max(0.0);
            let confidence = score((corr.max(0.0) * 0.72 + prominence * 0.28).clamp(0.0, 1.0));
            if confidence >= s.min_period_confidence_milli {
                vectors.push((
                    LatticeVector {
                        dx_pixels: dx * lag as i32,
                        dy_pixels: dy * lag as i32,
                        confidence_milli: confidence,
                    },
                    samples,
                ));
            }
        }
    }
    vectors.sort_by_key(|(v, _)| {
        (
            std::cmp::Reverse(v.confidence_milli),
            v.dy_pixels.abs() + v.dx_pixels.abs(),
            v.dy_pixels,
            v.dx_pixels,
        )
    });
    // Non-maximum suppression prevents harmonics from exhausting the bounded candidate list.
    let mut selected: Vec<(LatticeVector, u64)> = Vec::new();
    for candidate in vectors {
        if selected.iter().any(|(v, _)| {
            v.dx_pixels == candidate.0.dx_pixels && v.dy_pixels == candidate.0.dy_pixels
        }) {
            continue;
        }
        let same_axis_harmonic = selected.iter().any(|(v, _)| {
            (v.dx_pixels == 0) == (candidate.0.dx_pixels == 0)
                && (v.dy_pixels == 0) == (candidate.0.dy_pixels == 0)
                && (v.dx_pixels.abs() + v.dy_pixels.abs())
                    .abs_diff(candidate.0.dx_pixels.abs() + candidate.0.dy_pixels.abs())
                    <= 1
        });
        if !same_axis_harmonic {
            selected.push(candidate);
        }
        if selected.len() >= usize::from(s.max_period_candidates) {
            break;
        }
    }
    let x = selected.iter().find(|(v, _)| v.dy_pixels == 0).copied();
    let y = selected.iter().find(|(v, _)| v.dx_pixels == 0).copied();
    let mut result = Vec::new();
    if let Some((first, samples)) = x.or(y) {
        let second = if first.dy_pixels == 0 {
            y.map(|p| p.0)
        } else {
            x.map(|p| p.0)
        };
        let confidence = second.map_or(first.confidence_milli, |v| {
            first.confidence_milli.min(v.confidence_milli)
        });
        result.push(LatticeCandidate {
            first,
            second,
            confidence_milli: confidence,
            evidence_samples: samples,
        });
    }
    for (v, samples) in selected {
        if result
            .first()
            .is_some_and(|c| c.first == v || c.second == Some(v))
        {
            continue;
        }
        result.push(LatticeCandidate {
            first: v,
            second: None,
            confidence_milli: v.confidence_milli,
            evidence_samples: samples,
        });
    }
    result.truncate(usize::from(s.max_period_candidates));
    Ok(result)
}

fn autocorrelation(values: &[f32], w: u32, h: u32, dx: i32, dy: i32) -> (f32, u64) {
    let mut ab = 0.0;
    let mut aa = 0.0;
    let mut bb = 0.0;
    let mut n = 0;
    for y in 0..h {
        for x in 0..w {
            let bx = x as i32 + dx;
            let by = y as i32 + dy;
            if bx < 0 || by < 0 || bx >= w as i32 || by >= h as i32 {
                continue;
            }
            let a = values[idx(w, x, y)];
            let b = values[idx(w, bx as u32, by as u32)];
            ab += a * b;
            aa += a * a;
            bb += b * b;
            n += 1;
        }
    }
    let denominator: f32 = aa * bb;
    (
        if denominator > 1.0e-12 {
            ab / denominator.sqrt()
        } else {
            0.0
        },
        n,
    )
}

fn periodicity_map(
    e: &BaseEvidence,
    candidates: &[LatticeCandidate],
    cancellation: &RenderCancellationToken,
) -> Result<Vec<f32>, FeatureFieldError> {
    let mut out = vec![0.0; e.luminance.len()];
    let vectors: Vec<_> = candidates
        .iter()
        .flat_map(|c| [Some(c.first), c.second].into_iter().flatten())
        .collect();
    for y in 0..e.height {
        if y % 32 == 0 {
            check_cancel(cancellation)?;
        }
        for x in 0..e.width {
            let mut evidence = 0.0;
            let mut count = 0.0;
            for v in &vectors {
                let nx = x as i32 + v.dx_pixels;
                let ny = y as i32 + v.dy_pixels;
                if nx >= 0 && ny >= 0 && nx < e.width as i32 && ny < e.height as i32 {
                    evidence += 1.0
                        - (e.luminance[idx(e.width, x, y)]
                            - e.luminance[idx(e.width, nx as u32, ny as u32)])
                        .abs()
                        .min(1.0);
                    count += 1.0;
                }
            }
            out[idx(e.width, x, y)] = if count > 0.0 { evidence / count } else { 0.0 };
        }
    }
    Ok(out)
}

struct SeamEvidence {
    values: Vec<f32>,
    available_terms: BTreeSet<SeamTerm>,
    horizontal_cost: f32,
    vertical_cost: f32,
}

fn seamability(
    source: &DelitPreparedExemplar,
    e: &BaseEvidence,
    edge: &[f32],
    candidates: &[LatticeCandidate],
    _s: &FeatureFieldSettings,
    cancellation: &RenderCancellationToken,
) -> Result<SeamEvidence, FeatureFieldError> {
    let mut terms = BTreeSet::from([
        SeamTerm::Color,
        SeamTerm::Gradient,
        SeamTerm::StructuralCrossing,
    ]);
    let height = source.channels.iter().find_map(|c| match c {
        PreparedExemplarChannel::Scalar {
            role: MaterialChannelRole::Height,
            plane,
        } => Some(plane),
        _ => None,
    });
    let roughness = source.channels.iter().find_map(|c| match c {
        PreparedExemplarChannel::Scalar {
            role: MaterialChannelRole::Roughness,
            plane,
        } => Some(plane),
        _ => None,
    });
    let normal = source.channels.iter().find_map(|c| match c {
        PreparedExemplarChannel::Normal { plane, .. } => Some(plane),
        _ => None,
    });
    if height.is_some() {
        terms.insert(SeamTerm::Height);
    }
    if roughness.is_some() {
        terms.insert(SeamTerm::Roughness);
    }
    if normal.is_some() {
        terms.insert(SeamTerm::VectorNormal);
    }
    let preferred = candidates.first().map(|c| (c.first, c.second));
    let xlag = preferred
        .and_then(|(a, b)| {
            [Some(a), b]
                .into_iter()
                .flatten()
                .find(|v| v.dy_pixels == 0)
        })
        .map_or(1, |v| v.dx_pixels.unsigned_abs().max(1));
    let ylag = preferred
        .and_then(|(a, b)| {
            [Some(a), b]
                .into_iter()
                .flatten()
                .find(|v| v.dx_pixels == 0)
        })
        .map_or(1, |v| v.dy_pixels.unsigned_abs().max(1));
    let mut values = vec![0.0; e.luminance.len()];
    let mut hcost = 0.0;
    let mut vcost = 0.0;
    let mut hn = 0_u64;
    let mut vn = 0_u64;
    for y in 0..e.height {
        if y % 32 == 0 {
            check_cancel(cancellation)?;
        }
        for x in 0..e.width {
            let mut costs = Vec::new();
            if x + xlag < e.width {
                let c = seam_cost(
                    source,
                    e,
                    edge,
                    x,
                    y,
                    x + xlag,
                    y,
                    height,
                    roughness,
                    normal,
                );
                hcost += c;
                hn += 1;
                costs.push(c);
            }
            if y + ylag < e.height {
                let c = seam_cost(
                    source,
                    e,
                    edge,
                    x,
                    y,
                    x,
                    y + ylag,
                    height,
                    roughness,
                    normal,
                );
                vcost += c;
                vn += 1;
                costs.push(c);
            }
            values[idx(e.width, x, y)] = if costs.is_empty() {
                0.0
            } else {
                1.0 - costs.iter().sum::<f32>() / costs.len() as f32
            };
        }
    }
    Ok(SeamEvidence {
        values,
        available_terms: terms,
        horizontal_cost: if hn == 0 { 1.0 } else { hcost / hn as f32 },
        vertical_cost: if vn == 0 { 1.0 } else { vcost / vn as f32 },
    })
}

#[allow(clippy::too_many_arguments)]
fn seam_cost(
    source: &DelitPreparedExemplar,
    e: &BaseEvidence,
    edge: &[f32],
    ax: u32,
    ay: u32,
    bx: u32,
    by: u32,
    height: Option<&ImagePlane<hot_trimmer_image_io::LinearScalar>>,
    roughness: Option<&ImagePlane<hot_trimmer_image_io::LinearScalar>>,
    normal: Option<&ImagePlane<hot_trimmer_image_io::TangentNormal>>,
) -> f32 {
    let a = idx(e.width, ax, ay);
    let b = idx(e.width, bx, by);
    let mut total = ((e.rgb[a][0] - e.rgb[b][0]).abs()
        + (e.rgb[a][1] - e.rgb[b][1]).abs()
        + (e.rgb[a][2] - e.rgb[b][2]).abs())
        / 3.0;
    total += (e.gradient[a] - e.gradient[b]).abs();
    total += (edge[a] - edge[b]).abs();
    let mut terms = 3.0;
    if let Some(p) = height {
        total += (p.pixel(ax, ay).0 - p.pixel(bx, by).0).abs();
        terms += 1.0;
    }
    if let Some(p) = roughness {
        total += (p.pixel(ax, ay).0 - p.pixel(bx, by).0).abs();
        terms += 1.0;
    }
    if let Some(p) = normal {
        let na = p.pixel(ax, ay).xyz;
        let nb = p.pixel(bx, by).xyz;
        total += ((na[0] - nb[0]).powi(2) + (na[1] - nb[1]).powi(2) + (na[2] - nb[2]).powi(2))
            .sqrt()
            * 0.5;
        terms += 1.0;
    }
    let _ = source;
    (total / terms).clamp(0.0, 1.0)
}

fn usability(
    source: &DelitPreparedExemplar,
    saliency: &[f32],
    edge: &[f32],
    s: &FeatureFieldSettings,
    cancellation: &RenderCancellationToken,
) -> Result<(Vec<f32>, Vec<UsabilityReason>), FeatureFieldError> {
    let (w, h) = (source.base_color().width(), source.base_color().height());
    let scalar_opacity = source.channels.iter().find_map(|c| match c {
        PreparedExemplarChannel::Scalar {
            role: MaterialChannelRole::Opacity,
            plane,
        } => Some(plane),
        _ => None,
    });
    let mask_opacity = source.channels.iter().find_map(|c| match c {
        PreparedExemplarChannel::Mask {
            role: MaterialChannelRole::Opacity,
            plane,
        } => Some(plane),
        _ => None,
    });
    let mut confidence = vec![1.0; saliency.len()];
    let mut reasons = vec![UsabilityReason::NONE; saliency.len()];
    for y in 0..h {
        if y % 32 == 0 {
            check_cancel(cancellation)?;
        }
        for x in 0..w {
            let i = idx(w, x, y);
            let mut c = 1.0;
            let mut r = UsabilityReason::NONE;
            let alpha = source.base_color().pixel(x, y).alpha.clamp(0.0, 1.0);
            let coverage = source
                .coverage
                .as_ref()
                .map_or(1.0, |p| p.pixel(x, y).0.clamp(0.0, 1.0));
            let opacity = scalar_opacity.map_or_else(
                || mask_opacity.map_or(1.0, |p| p.pixel(x, y).0.clamp(0.0, 1.0)),
                |p| p.pixel(x, y).0.clamp(0.0, 1.0),
            );
            let geometric = alpha.min(coverage).min(opacity);
            if geometric < 0.999 {
                r = r.with(UsabilityReason::TRANSPARENT_OR_OUTSIDE);
                c *= geometric;
            }
            if let Some(m) = &source.masks {
                let clipped = m.clipping.pixel(x, y).0.clamp(0.0, 1.0);
                let highlight = m.highlight.pixel(x, y).0.clamp(0.0, 1.0);
                let shadow = m.shadow.pixel(x, y).0.clamp(0.0, 1.0);
                let lighting_confidence = m.confidence.pixel(x, y).0.clamp(0.0, 1.0);
                if clipped > 0.0 {
                    r = r.with(UsabilityReason::CLIPPED);
                    c *= 1.0 - clipped;
                }
                if highlight > 0.05 {
                    r = r.with(UsabilityReason::HIGHLIGHT_UNCERTAINTY);
                    c *= 1.0 - highlight * 0.75;
                }
                if shadow > 0.05 {
                    r = r.with(UsabilityReason::SHADOW_UNCERTAINTY);
                    c *= 1.0 - shadow * 0.75;
                }
                c *= 0.25 + lighting_confidence * 0.75;
            }
            let distinctive = saliency[i] * (0.4 + edge[i] * 0.6);
            if distinctive > 0.72 {
                r = r.with(UsabilityReason::SUSPECTED_OCCLUDER_OR_LOGO);
                if !s.retain_distinctive_content {
                    c *= 0.65;
                }
            }
            confidence[i] = c.clamp(0.0, 1.0);
            reasons[i] = r;
        }
    }
    Ok((confidence, reasons))
}

fn scalar_pyramid(
    width: u32,
    height: u32,
    values: Vec<f32>,
    dimensions: &[(u32, u32)],
    tile_edge: u32,
) -> Result<ResolutionPyramid<LinearScalar>, FeatureFieldError> {
    let mut levels = Vec::with_capacity(dimensions.len());
    let mut current = values;
    let mut w = width;
    let mut h = height;
    for &(next_w, next_h) in dimensions {
        debug_assert_eq!((w, h), (next_w, next_h));
        let pixels: Vec<_> = current
            .iter()
            .map(|v| LinearScalar(v.clamp(0.0, 1.0)))
            .collect();
        levels.push(
            ImagePlane::from_row_major(w, h, tile_edge, &pixels)
                .map_err(|_| FeatureFieldError::PlaneConstruction)?,
        );
        if levels.len() < dimensions.len() {
            current = downsample_scalar(&current, w, h);
            w = w.div_ceil(2);
            h = h.div_ceil(2);
        }
    }
    ResolutionPyramid::from_levels(levels).map_err(|_| FeatureFieldError::PlaneConstruction)
}

fn reason_pyramid(
    width: u32,
    height: u32,
    values: Vec<UsabilityReason>,
    dimensions: &[(u32, u32)],
    tile_edge: u32,
) -> Result<Vec<ImagePlane<UsabilityReason>>, FeatureFieldError> {
    let mut levels = Vec::new();
    let mut current = values;
    let mut w = width;
    let mut h = height;
    for _ in dimensions {
        levels.push(
            ImagePlane::from_row_major(w, h, tile_edge, &current)
                .map_err(|_| FeatureFieldError::PlaneConstruction)?,
        );
        if levels.len() < dimensions.len() {
            let nw = w.div_ceil(2);
            let nh = h.div_ceil(2);
            let mut next = Vec::with_capacity((nw * nh) as usize);
            for y in 0..nh {
                for x in 0..nw {
                    let mut bits = 0;
                    for sy in y * 2..(y * 2 + 2).min(h) {
                        for sx in x * 2..(x * 2 + 2).min(w) {
                            bits |= current[idx(w, sx, sy)].0;
                        }
                    }
                    next.push(UsabilityReason(bits));
                }
            }
            current = next;
            w = nw;
            h = nh;
        }
    }
    Ok(levels)
}

fn downsample_scalar(values: &[f32], w: u32, h: u32) -> Vec<f32> {
    let nw = w.div_ceil(2);
    let nh = h.div_ceil(2);
    let mut out = Vec::with_capacity((nw * nh) as usize);
    for y in 0..nh {
        for x in 0..nw {
            let mut sum = 0.0;
            let mut n = 0.0;
            for sy in y * 2..(y * 2 + 2).min(h) {
                for sx in x * 2..(x * 2 + 2).min(w) {
                    sum += values[idx(w, sx, sy)];
                    n += 1.0;
                }
            }
            out.push(sum / n);
        }
    }
    out
}

fn level_dimensions(mut w: u32, mut h: u32, max: u8) -> Vec<(u32, u32)> {
    let mut result = Vec::new();
    for _ in 0..max {
        result.push((w, h));
        if w == 1 && h == 1 {
            break;
        }
        w = w.div_ceil(2);
        h = h.div_ceil(2);
    }
    result
}

fn validate_field_registration<const N: usize>(
    dimensions: &[(u32, u32)],
    fields: [&ResolutionPyramid<LinearScalar>; N],
) -> Result<(), FeatureFieldError> {
    if fields.iter().any(|p| {
        p.levels()
            .iter()
            .map(|l| (l.width(), l.height()))
            .collect::<Vec<_>>()
            != dimensions
    }) {
        Err(FeatureFieldError::RegistrationDrift)
    } else {
        Ok(())
    }
}

fn local_moments(values: &[f32], w: u32, h: u32, x: u32, y: u32, r: u32) -> (f32, f32) {
    let x0 = x.saturating_sub(r);
    let x1 = (x + r).min(w - 1);
    let y0 = y.saturating_sub(r);
    let y1 = (y + r).min(h - 1);
    let mut sum = 0.0;
    let mut square = 0.0;
    let mut n = 0.0;
    for sy in y0..=y1 {
        for sx in x0..=x1 {
            let v = values[idx(w, sx, sy)];
            sum += v;
            square += v * v;
            n += 1.0;
        }
    }
    let mean = sum / n;
    (mean, (square / n - mean * mean).max(0.0))
}

fn scalar_neighborhood(
    plane: &ImagePlane<hot_trimmer_image_io::LinearScalar>,
    x: u32,
    y: u32,
    r: u32,
) -> Vec<f32> {
    let mut out = Vec::new();
    for sy in y.saturating_sub(r)..=(y + r).min(plane.height() - 1) {
        for sx in x.saturating_sub(r)..=(x + r).min(plane.width() - 1) {
            out.push(plane.pixel(sx, sy).0);
        }
    }
    out
}

fn variance_slice(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    values.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / values.len() as f32
}

fn normalize(values: &[f32]) -> Vec<f32> {
    let max = values
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(0.0_f32, f32::max);
    if max <= 1.0e-8 {
        vec![0.0; values.len()]
    } else {
        values.iter().map(|v| (v / max).clamp(0.0, 1.0)).collect()
    }
}

fn insufficiency_diagnostics(
    candidates: &[LatticeCandidate],
    usability: &ResolutionPyramid<LinearScalar>,
    terms: &BTreeSet<SeamTerm>,
) -> Vec<CompilationDiagnostic> {
    let base = usability.level(0).expect("constructed pyramid");
    let usable = base.to_row_major().iter().filter(|v| v.0 >= 0.5).count();
    let fraction = usable as f32 / (base.width() * base.height()) as f32;
    let mut result = Vec::new();
    if fraction < 0.25 {
        result.push(diagnostic(format!("Only {:.1}% of registered source pixels have usability confidence >= 50%; reason fields retain the uncertainty.", fraction * 100.0)));
    }
    if candidates.is_empty() {
        result.push(diagnostic("No bounded autocorrelation peak met the periodicity confidence threshold; periodic alignment is unavailable.".into()));
    }
    if terms.len() == 3 {
        result.push(diagnostic("Seamability uses Base Color, gradient, and structural crossings only; optional Height, normal, and Roughness terms are absent.".into()));
    }
    result
}

fn diagnostic(message: String) -> CompilationDiagnostic {
    CompilationDiagnostic {
        code: DiagnosticCode::InsufficientInput,
        stage: Some(7),
        message,
        context: BTreeMap::new(),
    }
}

fn settings_hash(s: &FeatureFieldSettings) -> ContentDigest {
    ContentDigest::sha256(
        format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}",
            s.max_pyramid_levels,
            s.tile_edge,
            s.stationarity_radius,
            s.max_period_pixels,
            s.max_period_candidates,
            s.min_period_confidence_milli,
            s.retain_distinctive_content,
            s.max_working_bytes,
            s.max_operations
        )
        .as_bytes(),
    )
}

fn score(value: f32) -> u16 {
    (value.clamp(0.0, 1.0) * 1000.0).round() as u16
}
fn idx(width: u32, x: u32, y: u32) -> usize {
    (y * width + x) as usize
}
fn check_cancel(token: &RenderCancellationToken) -> Result<(), FeatureFieldError> {
    if token.is_cancelled() {
        Err(FeatureFieldError::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use hot_trimmer_domain::{
        DelightingPassThroughReason, MaterialCalibrationIntent, NormalConvention, StageResult,
    };
    use hot_trimmer_image_io::{
        ImagePlane, LinearColor, LinearScalar, MaskValue, NormalAlphaPolicy, ResolvedAlphaMode,
        TangentNormal,
    };

    use super::*;
    use crate::{
        AnalysisSettings, ReflectanceProvenance, RouteExecution, ScaleOrientationSettings,
        analyze_source, calibrate_scale_orientation,
    };

    fn source(width: u32, height: u32, values: &[f32], full_pbr: bool) -> DelitPreparedExemplar {
        let colors: Vec<_> = values
            .iter()
            .map(|v| LinearColor {
                rgb: [*v; 3],
                alpha: 1.0,
            })
            .collect();
        let base = ImagePlane::from_row_major(width, height, 16, &colors).unwrap();
        let mut channels = vec![PreparedExemplarChannel::BaseColor {
            plane: base.clone(),
            alpha_mode: ResolvedAlphaMode::Opaque,
        }];
        if full_pbr {
            let scalar = ImagePlane::from_row_major(
                width,
                height,
                16,
                &values.iter().map(|v| LinearScalar(*v)).collect::<Vec<_>>(),
            )
            .unwrap();
            channels.push(PreparedExemplarChannel::Scalar {
                role: MaterialChannelRole::Height,
                plane: scalar.clone(),
            });
            channels.push(PreparedExemplarChannel::Scalar {
                role: MaterialChannelRole::Roughness,
                plane: scalar,
            });
            let normals = vec![
                TangentNormal {
                    xyz: [0.0, 0.0, 1.0],
                    alpha: 1.0
                };
                values.len()
            ];
            channels.push(PreparedExemplarChannel::Normal {
                plane: ImagePlane::from_row_major(width, height, 16, &normals).unwrap(),
                source_convention: NormalConvention::OpenGl,
                canonical_convention: NormalConvention::OpenGl,
                alpha_policy: NormalAlphaPolicy::Preserve,
            });
        }
        DelitPreparedExemplar {
            exemplar_id: "content-only-id".into(),
            prepared_source_digest: ContentDigest::sha256(b"stage-7-fixture"),
            perspective_confidence_milli: 1000,
            original_prepared_base_color: base,
            channels,
            coverage: None,
            masks: None,
            reflectance_provenance: ReflectanceProvenance::ImportedPrepared,
            route_execution: RouteExecution::PassThrough(
                DelightingPassThroughReason::AuthoredTextureOrPbrSet,
            ),
            upstream_stage_result: StageResult::PassThrough {
                reason: "planar".into(),
            },
            stage_result: StageResult::PassThrough {
                reason: "unchanged".into(),
            },
        }
    }

    fn stage_six(source: &DelitPreparedExemplar) -> ScaleOrientationReport {
        let token = RenderCancellationToken::new();
        let five = analyze_source(source, &AnalysisSettings::default(), None, &token).unwrap();
        calibrate_scale_orientation(
            source,
            &five,
            &MaterialCalibrationIntent::default(),
            &ScaleOrientationSettings::default(),
            &token,
        )
        .unwrap()
    }

    fn mean(p: &ResolutionPyramid<LinearScalar>) -> f32 {
        let values = p.level(0).unwrap().to_row_major();
        values.iter().map(|v| v.0).sum::<f32>() / values.len() as f32
    }

    #[test]
    fn algorithm_stage_07_feature_fields() {
        let (w, h) = (64, 64);
        let token = RenderCancellationToken::new();
        let settings = FeatureFieldSettings::default();
        let periodic: Vec<_> = (0..h)
            .flat_map(|y| (0..w).map(move |x| if (x / 8 + y / 8) % 2 == 0 { 0.2 } else { 0.8 }))
            .collect();
        let periodic_source = source(w, h, &periodic, false);
        let periodic_report = extract_feature_fields(
            &periodic_source,
            &stage_six(&periodic_source),
            &settings,
            &token,
        )
        .unwrap();
        assert!(
            !periodic_report.periodicity.candidates.is_empty(),
            "image autocorrelation must expose periodic evidence"
        );
        assert!(
            periodic_report.periodicity.candidates[0].confidence_milli
                >= settings.min_period_confidence_milli
        );
        assert_eq!(
            periodic_report.periodicity.evidence_method,
            PeriodicityEvidenceMethod::BoundedNormalizedAutocorrelation
        );
        assert!(periodic_report.qa.level_dimensions.len() > 1);

        let directional: Vec<_> = (0..h)
            .flat_map(|y| (0..w).map(move |x| ((x + y * 3) % 17) as f32 / 17.0))
            .collect();
        let directional_source = source(w, h, &directional, false);
        let directional_report = extract_feature_fields(
            &directional_source,
            &stage_six(&directional_source),
            &settings,
            &token,
        )
        .unwrap();
        assert!(mean(&directional_report.structure.line) > 0.08);

        let stochastic: Vec<_> = (0..h)
            .flat_map(|y| {
                (0..w).map(move |x| {
                    let mut v = 19_u64 ^ (u64::from(x) << 32) ^ u64::from(y);
                    v ^= v >> 30;
                    v = v.wrapping_mul(0xbf58_476d_1ce4_e5b9);
                    v ^= v >> 27;
                    v = v.wrapping_mul(0x94d0_49bb_1331_11eb);
                    v ^= v >> 31;
                    (v & 0xffff) as f32 / 65_535.0
                })
            })
            .collect();
        let stochastic_source = source(w, h, &stochastic, false);
        let stochastic_report = extract_feature_fields(
            &stochastic_source,
            &stage_six(&stochastic_source),
            &settings,
            &token,
        )
        .unwrap();
        assert!(
            stochastic_report
                .periodicity
                .candidates
                .first()
                .map_or(true, |c| c.confidence_milli < 500)
        );

        let mut salient = vec![0.25; (w * h) as usize];
        for y in 27..37 {
            for x in 22..32 {
                salient[idx(w, x, y)] = 1.0;
            }
        }
        let salient_source = source(w, h, &salient, false);
        let salient_report = extract_feature_fields(
            &salient_source,
            &stage_six(&salient_source),
            &settings,
            &token,
        )
        .unwrap();
        let center = salient_report.saliency.level(0).unwrap().pixel(27, 32).0;
        let corner = salient_report.saliency.level(0).unwrap().pixel(2, 2).0;
        assert!(
            center > corner + 0.4,
            "one field must support later generic penalties and unique rewards"
        );

        let mut unusable_source = source(w, h, &periodic, false);
        let half: Vec<_> = (0..h)
            .flat_map(|_| (0..w).map(|x| MaskValue(if x < w / 2 { 0.0 } else { 1.0 })))
            .collect();
        unusable_source.coverage = Some(ImagePlane::from_row_major(w, h, 16, &half).unwrap());
        let unusable_report = extract_feature_fields(
            &unusable_source,
            &stage_six(&unusable_source),
            &settings,
            &token,
        )
        .unwrap();
        assert_eq!(
            unusable_report
                .usability
                .confidence
                .level(0)
                .unwrap()
                .pixel(2, 2)
                .0,
            0.0
        );
        assert!(
            unusable_report.usability.reasons[0]
                .pixel(2, 2)
                .contains(UsabilityReason::TRANSPARENT_OR_OUTSIDE)
        );

        let pbr_source = source(w, h, &periodic, true);
        let pbr_report =
            extract_feature_fields(&pbr_source, &stage_six(&pbr_source), &settings, &token)
                .unwrap();
        assert_eq!(
            periodic_report.qa.level_dimensions, pbr_report.qa.level_dimensions,
            "optional PBR maps cannot change coordinates"
        );
        assert_eq!(
            periodic_report.seamability.available_terms.len() + 3,
            pbr_report.seamability.available_terms.len()
        );

        let mut drift = pbr_source.clone();
        let bad = ImagePlane::from_row_major(8, 8, 8, &vec![LinearScalar(0.0); 64]).unwrap();
        drift.channels.push(PreparedExemplarChannel::Scalar {
            role: MaterialChannelRole::Metallic,
            plane: bad,
        });
        assert_eq!(
            extract_feature_fields(&drift, &stage_six(&pbr_source), &settings, &token),
            Err(FeatureFieldError::RegistrationDrift)
        );
        let cancelled = RenderCancellationToken::new();
        cancelled.cancel();
        assert_eq!(
            extract_feature_fields(
                &periodic_source,
                &stage_six(&periodic_source),
                &settings,
                &cancelled
            ),
            Err(FeatureFieldError::Cancelled)
        );
        let limited = FeatureFieldSettings {
            max_working_bytes: 1,
            ..settings
        };
        assert!(matches!(
            extract_feature_fields(
                &periodic_source,
                &stage_six(&periodic_source),
                &limited,
                &token
            ),
            Err(FeatureFieldError::ResourceLimitExceeded { .. })
        ));
        assert_eq!(
            extract_feature_fields(
                &periodic_source,
                &stage_six(&periodic_source),
                &settings,
                &token
            )
            .unwrap()
            .cache_key,
            periodic_report.cache_key
        );
    }
}
