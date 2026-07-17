//! Stage 3 registered geometry preparation. One immutable coordinate field drives every channel.

use std::collections::{BTreeMap, BTreeSet};

use hot_trimmer_domain::{
    AlgorithmProvenance, CompilationDiagnostic, ContentDigest, DiagnosticCode, MaterialChannelRole,
    NormalConvention, PatchId, RecoveryChoice, RectificationSettings, SourceSetId, StageResult,
};
use hot_trimmer_geometry::{
    GeometryError, Point, Quadrilateral, RectificationLimits, assist_polygon,
    rectified_dimensions,
};
use hot_trimmer_image_io::{
    CategoryId, ImagePlane, LinearColor, LinearScalar, MaskValue, NormalAlphaPolicy,
    PreparedChannel, PreparedChannelCacheKey, PreparedChannelSet, ResolvedAlphaMode,
    TangentNormal,
};
use thiserror::Error;

use crate::RenderCancellationToken;

pub const STAGE_03_ALGORITHM_ID: &str = "hot_trimmer.registered_rectification";
pub const STAGE_03_ALGORITHM_VERSION: &str = "3.0.0";
pub const MAX_OUTLINE_POINTS: usize = 8;
const MAX_LENS_DISPLACEMENT: f64 = 0.35;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PassThroughReason {
    AuthoredPlanarTexture,
    FrontFacingScan,
    UserConfirmedPlanar,
}

impl PassThroughReason {
    fn description(&self) -> &'static str {
        match self {
            Self::AuthoredPlanarTexture => "authored texture is already planar",
            Self::FrontFacingScan => "front-facing scan is already planar",
            Self::UserConfirmedPlanar => "user confirmed that perspective correction is unnecessary",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlanarArea {
    PassThrough { reason: PassThroughReason },
    FourPoint { corners: [hot_trimmer_domain::NormalizedPoint; 4] },
    OutlineAssisted {
        points: Vec<hot_trimmer_domain::NormalizedPoint>,
        retain_mask: bool,
    },
    FullFrame {
        usable_area: Option<[hot_trimmer_domain::NormalizedPoint; 4]>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LensCorrection {
    pub center: hot_trimmer_domain::NormalizedPoint,
    pub radial_k1: f64,
    pub radial_k2: f64,
}

impl LensCorrection {
    fn validate(self) -> Result<(), RectificationError> {
        if !self.radial_k1.is_finite()
            || !self.radial_k2.is_finite()
            || !(-0.5..=0.5).contains(&self.radial_k1)
            || !(-0.25..=0.25).contains(&self.radial_k2)
        {
            return Err(RectificationError::LensCorrectionOutOfBounds);
        }
        let maximum_radius_squared = 2.0;
        let displacement = (self.radial_k1 * maximum_radius_squared
            + self.radial_k2 * maximum_radius_squared * maximum_radius_squared)
            .abs()
            * maximum_radius_squared.sqrt();
        if displacement > MAX_LENS_DISPLACEMENT {
            return Err(RectificationError::LensCorrectionOutOfBounds);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExemplarMaskIntent {
    /// Optional source-coordinate crop polygon. Three or more points are required.
    pub crop_polygon: Option<Vec<hot_trimmer_domain::NormalizedPoint>>,
    /// Retain only pixels whose rectified Base Color alpha meets this threshold.
    pub minimum_alpha: Option<f32>,
}

impl Default for ExemplarMaskIntent {
    fn default() -> Self { Self { crop_polygon: None, minimum_alpha: None } }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RectificationQuality {
    Preview,
    Authoritative,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RectificationWorkLimits {
    pub preview_max_edge: u32,
    pub authoritative_max_edge: u32,
    pub max_pixels: u64,
    pub tile_edge: u32,
}

impl Default for RectificationWorkLimits {
    fn default() -> Self {
        Self { preview_max_edge: 2_048, authoritative_max_edge: 8_192, max_pixels: 67_108_864, tile_edge: 128 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreparedExemplarRequest {
    pub exemplar_id: String,
    pub area: PlanarArea,
    pub lens_correction: Option<LensCorrection>,
    pub mask: ExemplarMaskIntent,
    pub rectification: RectificationSettings,
    /// When known, this physical width/height ratio controls output shape without stretching channels.
    pub physical_aspect_ratio: Option<f64>,
    pub quality: RectificationQuality,
    pub limits: RectificationWorkLimits,
    pub scope: PreparedExemplarScope,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PreparedExemplarScope {
    pub source_set_id: SourceSetId,
    pub source_revision: u64,
    pub patch_id: Option<PatchId>,
    pub patch_revision: u64,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PreparedExemplarCacheKey(pub ContentDigest);

#[derive(Clone, Debug, PartialEq)]
pub enum PreparedExemplarChannel {
    BaseColor { plane: ImagePlane<LinearColor>, alpha_mode: ResolvedAlphaMode },
    Scalar { role: MaterialChannelRole, plane: ImagePlane<LinearScalar> },
    Normal {
        plane: ImagePlane<TangentNormal>,
        source_convention: NormalConvention,
        canonical_convention: NormalConvention,
        alpha_policy: NormalAlphaPolicy,
    },
    MaterialId { plane: ImagePlane<CategoryId> },
    Mask { role: MaterialChannelRole, plane: ImagePlane<MaskValue> },
}

impl PreparedExemplarChannel {
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
    pub const fn dimensions(&self) -> (u32, u32) {
        match self {
            Self::BaseColor { plane, .. } => (plane.width(), plane.height()),
            Self::Scalar { plane, .. } => (plane.width(), plane.height()),
            Self::Normal { plane, .. } => (plane.width(), plane.height()),
            Self::MaterialId { plane } => (plane.width(), plane.height()),
            Self::Mask { plane, .. } => (plane.width(), plane.height()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreparedExemplar {
    pub exemplar_id: String,
    pub cache_key: PreparedExemplarCacheKey,
    pub scope: PreparedExemplarScope,
    pub width: u32,
    pub height: u32,
    pub channels: Vec<PreparedExemplarChannel>,
    pub usable_mask: Option<ImagePlane<MaskValue>>,
    pub perspective_confidence_milli: u16,
    /// Resolution-independent geometry shared by preview and authoritative work.
    pub geometry_digest: ContentDigest,
    pub coordinate_field_digest: ContentDigest,
    pub stage_result: StageResult,
}

#[derive(Clone, Debug, Default)]
pub struct PreparedExemplarCache {
    entries: BTreeMap<PreparedExemplarCacheKey, PreparedExemplar>,
}

impl PreparedExemplarCache {
    #[must_use]
    pub fn get(&self, key: &PreparedExemplarCacheKey) -> Option<&PreparedExemplar> { self.entries.get(key) }

    pub fn insert_complete(&mut self, exemplar: PreparedExemplar) {
        self.entries.insert(exemplar.cache_key.clone(), exemplar);
    }

    /// Invalidates only the edited patch and its downstream key lineage.
    pub fn invalidate_patch(&mut self, patch_id: PatchId) -> usize {
        let before = self.entries.len();
        self.entries.retain(|_, value| value.scope.patch_id != Some(patch_id));
        before - self.entries.len()
    }

    /// Source replacement invalidates exemplars derived from that source set, not unrelated sources.
    pub fn invalidate_source(&mut self, source_set_id: SourceSetId) -> usize {
        let before = self.entries.len();
        self.entries.retain(|_, value| value.scope.source_set_id != source_set_id);
        before - self.entries.len()
    }
}

#[derive(Clone, Copy, Debug)]
struct CoordinateSample {
    source: Point,
    jacobian: [[f64; 2]; 2],
}

#[derive(Clone, Debug)]
struct CoordinateField {
    width: u32,
    height: u32,
    samples: Vec<Option<CoordinateSample>>,
    digest: ContentDigest,
}

#[derive(Debug, Error, PartialEq)]
pub enum RectificationError {
    #[error("prepared source has no level-zero Base Color channel")]
    BaseColorRequired,
    #[error("prepared channels are not registered at level zero")]
    RegistrationDrift,
    #[error("pass-through cannot include lens correction, crop, alpha masking, scaling, or aspect changes")]
    PassThroughWouldResample,
    #[error("lens correction exceeds the bounded stable model")]
    LensCorrectionOutOfBounds,
    #[error("crop mask must contain three through 64 finite source points")]
    InvalidCropMask,
    #[error("alpha threshold must be finite and in zero-to-one")]
    InvalidAlphaThreshold,
    #[error("rectification work limits are invalid or excessive")]
    InvalidWorkLimits,
    #[error("rectification operation was cancelled")]
    Cancelled,
    #[error("typed image plane could not be constructed")]
    PlaneConstruction,
    #[error(transparent)]
    Geometry(#[from] GeometryError),
}

impl RectificationError {
    #[must_use]
    pub fn recovery_choices(&self) -> Vec<RecoveryChoice> {
        match self {
            Self::BaseColorRequired | Self::RegistrationDrift => vec![RecoveryChoice::ChooseAnotherSource],
            Self::Cancelled => vec![RecoveryChoice::AdjustSettings],
            _ => vec![RecoveryChoice::AdjustSettings, RecoveryChoice::ChooseAnotherSource],
        }
    }

    #[must_use]
    pub fn failed_stage_result(&self) -> StageResult {
        StageResult::FailedWithRecovery {
            reason: CompilationDiagnostic {
                code: match self {
                    Self::InvalidWorkLimits | Self::LensCorrectionOutOfBounds => DiagnosticCode::ResourceLimitExceeded,
                    Self::BaseColorRequired | Self::RegistrationDrift => DiagnosticCode::InsufficientInput,
                    Self::Cancelled => DiagnosticCode::Cancelled,
                    _ => DiagnosticCode::MalformedInput,
                },
                stage: Some(3),
                message: self.to_string(),
                context: BTreeMap::new(),
            },
            recovery_choices: self.recovery_choices(),
        }
    }
}

/// Prepares a batch atomically: an invalid member returns no partially prepared batch.
pub fn prepare_registered_exemplars(
    source: &PreparedChannelSet,
    requests: &[PreparedExemplarRequest],
    cancellation: &RenderCancellationToken,
) -> Result<Vec<PreparedExemplar>, RectificationError> {
    let mut ids = BTreeSet::new();
    let mut result = Vec::with_capacity(requests.len());
    for request in requests {
        if request.exemplar_id.is_empty() || !ids.insert(request.exemplar_id.as_str()) {
            return Err(RectificationError::InvalidWorkLimits);
        }
        result.push(prepare_registered_exemplar(source, request, cancellation)?);
    }
    Ok(result)
}

/// Produces one planar exemplar using one coordinate field for all registered channels.
pub fn prepare_registered_exemplar(
    source: &PreparedChannelSet,
    request: &PreparedExemplarRequest,
    cancellation: &RenderCancellationToken,
) -> Result<PreparedExemplar, RectificationError> {
    let (source_width, source_height) = validate_source(source)?;
    validate_request(request)?;
    let cache_key = exemplar_cache_key(&source.cache_key, request);

    if let PlanarArea::PassThrough { reason } = &request.area {
        if request.lens_correction.is_some()
            || request.mask.crop_polygon.is_some()
            || request.mask.minimum_alpha.is_some()
            || request.rectification.scale != 1.0
            || request.rectification.aspect_ratio.is_some()
            || request.physical_aspect_ratio.is_some()
        {
            return Err(RectificationError::PassThroughWouldResample);
        }
        let channels = clone_level_zero(source)?;
        return Ok(PreparedExemplar {
            exemplar_id: request.exemplar_id.clone(),
            cache_key,
            scope: request.scope,
            width: source_width,
            height: source_height,
            channels,
            usable_mask: None,
            perspective_confidence_milli: 1000,
            geometry_digest: geometry_digest(request),
            coordinate_field_digest: ContentDigest::sha256(b"stage-03-pass-through-no-coordinate-field"),
            stage_result: StageResult::PassThrough { reason: reason.description().into() },
        });
    }

    let (quadrilateral, assistance_mask, confidence) = resolve_geometry(&request.area)?;
    let mut settings = request.rectification;
    if settings.aspect_ratio.is_none() { settings.aspect_ratio = request.physical_aspect_ratio; }
    let natural = rectified_dimensions(
        quadrilateral,
        source_width,
        source_height,
        settings,
        RectificationLimits {
            max_edge: request.limits.authoritative_max_edge,
            max_pixels: request.limits.max_pixels,
        },
    )?;
    let (width, height) = fit_work_dimensions(natural.width, natural.height, request)?;
    let field = build_coordinate_field(width, height, quadrilateral, request.lens_correction, cancellation)?;
    let channels = rectify_channels(source, &field, request.limits.tile_edge, cancellation)?;
    let effective_crop = request.mask.crop_polygon.as_deref().or(assistance_mask.as_deref());
    let usable_mask = build_usable_mask(
        &field,
        source,
        effective_crop,
        request.mask.minimum_alpha,
        request.lens_correction.is_some(),
        request.limits.tile_edge,
        cancellation,
    )?;
    Ok(PreparedExemplar {
        exemplar_id: request.exemplar_id.clone(),
        cache_key,
        scope: request.scope,
        width,
        height,
        channels,
        usable_mask,
        perspective_confidence_milli: confidence,
        geometry_digest: geometry_digest(request),
        coordinate_field_digest: field.digest,
        stage_result: StageResult::Executed {
            algorithm: AlgorithmProvenance { algorithm_id: STAGE_03_ALGORITHM_ID.into(), version: STAGE_03_ALGORITHM_VERSION.into() },
            settings_hash: settings_digest(request),
            diagnostics: Vec::new(),
        },
    })
}

fn validate_source(source: &PreparedChannelSet) -> Result<(u32, u32), RectificationError> {
    let base = source.channels.iter().find_map(|channel| match channel {
        PreparedChannel::BaseColor { linear, .. } => linear.level(0),
        _ => None,
    }).ok_or(RectificationError::BaseColorRequired)?;
    let dimensions = (base.width(), base.height());
    for channel in &source.channels {
        if channel.dimensions().first().copied() != Some(dimensions) {
            return Err(RectificationError::RegistrationDrift);
        }
    }
    Ok(dimensions)
}

fn validate_request(request: &PreparedExemplarRequest) -> Result<(), RectificationError> {
    if request.limits.tile_edge == 0
        || request.limits.preview_max_edge == 0
        || request.limits.authoritative_max_edge == 0
        || request.limits.preview_max_edge > request.limits.authoritative_max_edge
        || request.limits.max_pixels == 0
        || !request.rectification.is_valid()
        || request.physical_aspect_ratio.is_some_and(|v| !v.is_finite() || !(0.01..=100.0).contains(&v))
    {
        return Err(RectificationError::InvalidWorkLimits);
    }
    if let Some(lens) = request.lens_correction { lens.validate()?; }
    if let Some(points) = &request.mask.crop_polygon
        && (!(3..=64).contains(&points.len()))
    {
        return Err(RectificationError::InvalidCropMask);
    }
    if request.mask.minimum_alpha.is_some_and(|v| !v.is_finite() || !(0.0..=1.0).contains(&v)) {
        return Err(RectificationError::InvalidAlphaThreshold);
    }
    Ok(())
}

fn resolve_geometry(area: &PlanarArea) -> Result<(Quadrilateral, Option<Vec<hot_trimmer_domain::NormalizedPoint>>, u16), RectificationError> {
    match area {
        PlanarArea::PassThrough { .. } => unreachable!("handled before geometry resolution"),
        PlanarArea::FourPoint { corners } => {
            let quad = Quadrilateral::new(*corners)?;
            Ok((quad, None, perspective_confidence(quad, false)))
        }
        PlanarArea::OutlineAssisted { points, retain_mask } => {
            if points.len() > MAX_OUTLINE_POINTS { return Err(RectificationError::InvalidCropMask); }
            let assisted = assist_polygon(points, *retain_mask)?;
            let confidence = perspective_confidence(assisted.quadrilateral, assisted.approximate);
            Ok((assisted.quadrilateral, assisted.mask, confidence))
        }
        PlanarArea::FullFrame { usable_area } => {
            let corners = usable_area.unwrap_or([
                normalized(0.0, 0.0), normalized(1.0, 0.0),
                normalized(1.0, 1.0), normalized(0.0, 1.0),
            ]);
            let quad = Quadrilateral::new(corners)?;
            Ok((quad, None, perspective_confidence(quad, false)))
        }
    }
}

fn normalized(x: f64, y: f64) -> hot_trimmer_domain::NormalizedPoint {
    hot_trimmer_domain::NormalizedPoint::new(x, y).expect("unit corners are valid")
}

fn perspective_confidence(quad: Quadrilateral, approximate: bool) -> u16 {
    let points = quad.corners().map(Point::from);
    let lengths = [distance(points[0], points[1]), distance(points[1], points[2]), distance(points[2], points[3]), distance(points[3], points[0])];
    let opposite_balance = (lengths[0].min(lengths[2]) / lengths[0].max(lengths[2]))
        * (lengths[1].min(lengths[3]) / lengths[1].max(lengths[3]));
    let area_score = (quad.signed_area() / 0.05).clamp(0.0, 1.0);
    let approximation = if approximate { 0.85 } else { 1.0 };
    (1000.0 * opposite_balance.sqrt() * area_score * approximation).round().clamp(1.0, 1000.0) as u16
}

fn distance(a: Point, b: Point) -> f64 { ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt() }

fn fit_work_dimensions(width: u32, height: u32, request: &PreparedExemplarRequest) -> Result<(u32, u32), RectificationError> {
    let max_edge = match request.quality {
        RectificationQuality::Preview => request.limits.preview_max_edge,
        RectificationQuality::Authoritative => request.limits.authoritative_max_edge,
    };
    let edge_scale = (f64::from(max_edge) / f64::from(width.max(height))).min(1.0);
    let pixel_scale = (request.limits.max_pixels as f64 / (f64::from(width) * f64::from(height))).sqrt().min(1.0);
    let scale = edge_scale.min(pixel_scale);
    let output_width = (f64::from(width) * scale).round().max(1.0) as u32;
    let output_height = (f64::from(height) * scale).round().max(1.0) as u32;
    Ok((output_width, output_height))
}

fn map_coordinate(homography: hot_trimmer_geometry::Homography, lens: Option<LensCorrection>, output: Point) -> Option<Point> {
    let mut source = homography.transform(output)?;
    if let Some(lens) = lens {
        let center = Point::from(lens.center);
        let dx = source.x - center.x;
        let dy = source.y - center.y;
        let radius_squared = dx.mul_add(dx, dy * dy);
        let factor = 1.0 + lens.radial_k1 * radius_squared + lens.radial_k2 * radius_squared * radius_squared;
        source = Point { x: center.x + dx * factor, y: center.y + dy * factor };
    }
    (source.x.is_finite()
        && source.y.is_finite()
        && (0.0..=1.0).contains(&source.x)
        && (0.0..=1.0).contains(&source.y))
        .then_some(source)
}

fn coordinate_derivative(
    source: Point,
    before: Option<Point>,
    after: Option<Point>,
    step: f64,
) -> Option<[f64; 2]> {
    match (before, after) {
        (Some(before), Some(after)) => Some([
            (after.x - before.x) / (2.0 * step),
            (after.y - before.y) / (2.0 * step),
        ]),
        (None, Some(after)) => Some([
            (after.x - source.x) / step,
            (after.y - source.y) / step,
        ]),
        (Some(before), None) => Some([
            (source.x - before.x) / step,
            (source.y - before.y) / step,
        ]),
        (None, None) => None,
    }
}

fn build_coordinate_field(
    width: u32,
    height: u32,
    quad: Quadrilateral,
    lens: Option<LensCorrection>,
    cancellation: &RenderCancellationToken,
) -> Result<CoordinateField, RectificationError> {
    let homography = quad.source_from_output()?;
    let count = usize::try_from(u64::from(width) * u64::from(height)).map_err(|_| RectificationError::InvalidWorkLimits)?;
    let mut samples = Vec::with_capacity(count);
    let mut digest_bytes = Vec::with_capacity(count.saturating_mul(16));
    let du = 1.0 / f64::from(width);
    let dv = 1.0 / f64::from(height);
    for y in 0..height {
        if cancellation.is_cancelled() { return Err(RectificationError::Cancelled); }
        for x in 0..width {
            let output = Point { x: (f64::from(x) + 0.5) / f64::from(width), y: (f64::from(y) + 0.5) / f64::from(height) };
            let sample = map_coordinate(homography, lens, output).and_then(|source| {
                let derivative_x = coordinate_derivative(
                    source,
                    (x > 0).then(|| map_coordinate(homography, lens, Point { x: output.x - du, y: output.y })).flatten(),
                    (x + 1 < width).then(|| map_coordinate(homography, lens, Point { x: output.x + du, y: output.y })).flatten(),
                    du,
                )?;
                let derivative_y = coordinate_derivative(
                    source,
                    (y > 0).then(|| map_coordinate(homography, lens, Point { x: output.x, y: output.y - dv })).flatten(),
                    (y + 1 < height).then(|| map_coordinate(homography, lens, Point { x: output.x, y: output.y + dv })).flatten(),
                    dv,
                )?;
                Some(CoordinateSample {
                    source,
                    jacobian: [derivative_x, derivative_y],
                })
            });
            if let Some(sample) = sample {
                digest_bytes.extend_from_slice(&sample.source.x.to_bits().to_le_bytes());
                digest_bytes.extend_from_slice(&sample.source.y.to_bits().to_le_bytes());
            } else {
                digest_bytes.extend_from_slice(&[0; 16]);
            }
            samples.push(sample);
        }
    }
    Ok(CoordinateField { width, height, samples, digest: ContentDigest::sha256(&digest_bytes) })
}

fn clone_level_zero(source: &PreparedChannelSet) -> Result<Vec<PreparedExemplarChannel>, RectificationError> {
    source.channels.iter().map(|channel| match channel {
        PreparedChannel::BaseColor { linear, alpha_mode, .. } => Ok(PreparedExemplarChannel::BaseColor { plane: level(linear)?.clone(), alpha_mode: *alpha_mode }),
        PreparedChannel::Scalar { role, pyramid } => Ok(PreparedExemplarChannel::Scalar { role: *role, plane: level(pyramid)?.clone() }),
        PreparedChannel::Normal { pyramid, source_convention, canonical_convention, alpha_policy } => Ok(PreparedExemplarChannel::Normal { plane: level(pyramid)?.clone(), source_convention: *source_convention, canonical_convention: *canonical_convention, alpha_policy: *alpha_policy }),
        PreparedChannel::MaterialId { pyramid } => Ok(PreparedExemplarChannel::MaterialId { plane: level(pyramid)?.clone() }),
        PreparedChannel::Mask { role, pyramid } => Ok(PreparedExemplarChannel::Mask { role: *role, plane: level(pyramid)?.clone() }),
    }).collect()
}

/// Returns the already-oriented, registered level-zero channels without rectification.
///
/// SourceFrame DirectCrop is defined in original oriented source coordinates.  It must
/// not manufacture a Stage 3 coordinate field merely to reach Stage 14.
pub fn registered_level_zero_channels(source: &PreparedChannelSet) -> Result<Vec<PreparedExemplarChannel>, RectificationError> {
    clone_level_zero(source)
}

fn level<T>(pyramid: &hot_trimmer_image_io::ResolutionPyramid<T>) -> Result<&ImagePlane<T>, RectificationError> {
    pyramid.level(0).ok_or(RectificationError::RegistrationDrift)
}

fn rectify_channels(
    source: &PreparedChannelSet,
    field: &CoordinateField,
    tile_edge: u32,
    cancellation: &RenderCancellationToken,
) -> Result<Vec<PreparedExemplarChannel>, RectificationError> {
    source.channels.iter().map(|channel| {
        if cancellation.is_cancelled() { return Err(RectificationError::Cancelled); }
        match channel {
            PreparedChannel::BaseColor { linear, alpha_mode, .. } => Ok(PreparedExemplarChannel::BaseColor {
                plane: map_field(field, level(linear)?, tile_edge, LinearColor { rgb: [0.0; 3], alpha: 0.0 }, sample_color)?, alpha_mode: *alpha_mode,
            }),
            PreparedChannel::Scalar { role, pyramid } => Ok(PreparedExemplarChannel::Scalar {
                role: *role, plane: map_field(field, level(pyramid)?, tile_edge, LinearScalar(0.0), sample_scalar)?,
            }),
            PreparedChannel::Normal { pyramid, source_convention, canonical_convention, alpha_policy } => Ok(PreparedExemplarChannel::Normal {
                plane: map_field(field, level(pyramid)?, tile_edge, TangentNormal { xyz: [0.0, 0.0, 1.0], alpha: 0.0 }, sample_normal)?,
                source_convention: *source_convention, canonical_convention: *canonical_convention, alpha_policy: *alpha_policy,
            }),
            PreparedChannel::MaterialId { pyramid } => Ok(PreparedExemplarChannel::MaterialId {
                plane: map_field(field, level(pyramid)?, tile_edge, CategoryId(0), sample_id)?,
            }),
            PreparedChannel::Mask { role, pyramid } => Ok(PreparedExemplarChannel::Mask {
                role: *role, plane: map_field(field, level(pyramid)?, tile_edge, MaskValue(0.0), sample_mask)?,
            }),
        }
    }).collect()
}

fn map_field<T: Clone>(
    field: &CoordinateField,
    source: &ImagePlane<T>,
    tile_edge: u32,
    fallback: T,
    sampler: fn(&ImagePlane<T>, CoordinateSample) -> T,
) -> Result<ImagePlane<T>, RectificationError> {
    let pixels: Vec<T> = field.samples.iter().map(|sample| sample.map_or_else(|| fallback.clone(), |value| sampler(source, value))).collect();
    ImagePlane::from_row_major(field.width, field.height, tile_edge, &pixels).map_err(|_| RectificationError::PlaneConstruction)
}

fn source_indices<T>(plane: &ImagePlane<T>, point: Point) -> (u32, u32, u32, u32, f32, f32) {
    let x = point.x.mul_add(f64::from(plane.width()), -0.5).clamp(0.0, f64::from(plane.width() - 1));
    let y = point.y.mul_add(f64::from(plane.height()), -0.5).clamp(0.0, f64::from(plane.height() - 1));
    let x0 = x.floor() as u32; let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(plane.width() - 1); let y1 = (y0 + 1).min(plane.height() - 1);
    (x0, y0, x1, y1, (x - f64::from(x0)) as f32, (y - f64::from(y0)) as f32)
}

fn weights(tx: f32, ty: f32) -> [f32; 4] { [(1.0 - tx) * (1.0 - ty), tx * (1.0 - ty), (1.0 - tx) * ty, tx * ty] }

fn sample_color(plane: &ImagePlane<LinearColor>, sample: CoordinateSample) -> LinearColor {
    let (x0, y0, x1, y1, tx, ty) = source_indices(plane, sample.source);
    let values = [plane.pixel(x0,y0), plane.pixel(x1,y0), plane.pixel(x0,y1), plane.pixel(x1,y1)];
    let weights = weights(tx,ty);
    let alpha = values.iter().zip(weights).map(|(p,w)| p.alpha*w).sum::<f32>();
    let mut rgb = [0.0;3];
    for (pixel, weight) in values.iter().zip(weights) { for (channel, value) in rgb.iter_mut().zip(pixel.rgb) { *channel += value * pixel.alpha * weight; } }
    if alpha > f32::EPSILON { for value in &mut rgb { *value /= alpha; } }
    LinearColor { rgb, alpha }
}

fn sample_scalar(plane: &ImagePlane<LinearScalar>, sample: CoordinateSample) -> LinearScalar {
    let (x0,y0,x1,y1,tx,ty)=source_indices(plane,sample.source); let w=weights(tx,ty);
    let p=[plane.pixel(x0,y0).0,plane.pixel(x1,y0).0,plane.pixel(x0,y1).0,plane.pixel(x1,y1).0];
    LinearScalar(p.into_iter().zip(w).map(|(v,w)|v*w).sum())
}

fn sample_mask(plane: &ImagePlane<MaskValue>, sample: CoordinateSample) -> MaskValue {
    let value = sample_scalar_like(plane, sample); MaskValue(value)
}

fn sample_scalar_like(plane: &ImagePlane<MaskValue>, sample: CoordinateSample) -> f32 {
    let (x0,y0,x1,y1,tx,ty)=source_indices(plane,sample.source); let w=weights(tx,ty);
    let p=[plane.pixel(x0,y0).0,plane.pixel(x1,y0).0,plane.pixel(x0,y1).0,plane.pixel(x1,y1).0];
    p.into_iter().zip(w).map(|(v,w)|v*w).sum()
}

fn sample_id(plane: &ImagePlane<CategoryId>, sample: CoordinateSample) -> CategoryId {
    let x=(sample.source.x*f64::from(plane.width())).floor().clamp(0.0,f64::from(plane.width()-1)) as u32;
    let y=(sample.source.y*f64::from(plane.height())).floor().clamp(0.0,f64::from(plane.height()-1)) as u32;
    *plane.pixel(x,y)
}

fn sample_normal(plane: &ImagePlane<TangentNormal>, sample: CoordinateSample) -> TangentNormal {
    let (x0,y0,x1,y1,tx,ty)=source_indices(plane,sample.source); let w=weights(tx,ty);
    let p=[plane.pixel(x0,y0),plane.pixel(x1,y0),plane.pixel(x0,y1),plane.pixel(x1,y1)];
    let mut xyz=[0.0_f32;3]; let mut alpha=0.0;
    for (value,weight) in p.into_iter().zip(w) { for (out,input) in xyz.iter_mut().zip(value.xyz) { *out += input*weight; } alpha += value.alpha*weight; }
    let j=sample.jacobian;
    let x=f64::from(xyz[0]).mul_add(j[0][0],f64::from(xyz[1])*j[0][1]);
    let y=f64::from(xyz[0]).mul_add(j[1][0],f64::from(xyz[1])*j[1][1]);
    let z=f64::from(xyz[2]); let length=(x*x+y*y+z*z).sqrt().max(f64::EPSILON);
    TangentNormal { xyz:[(x/length) as f32,(y/length) as f32,(z/length) as f32], alpha }
}

fn build_usable_mask(
    field: &CoordinateField,
    source: &PreparedChannelSet,
    polygon: Option<&[hot_trimmer_domain::NormalizedPoint]>,
    minimum_alpha: Option<f32>,
    retain_field_coverage: bool,
    tile_edge: u32,
    cancellation: &RenderCancellationToken,
) -> Result<Option<ImagePlane<MaskValue>>, RectificationError> {
    if polygon.is_none() && minimum_alpha.is_none() && !retain_field_coverage { return Ok(None); }
    let base = source.channels.iter().find_map(|channel| match channel { PreparedChannel::BaseColor { linear, .. } => linear.level(0), _ => None }).ok_or(RectificationError::BaseColorRequired)?;
    let mut values=Vec::with_capacity(field.samples.len());
    for (index,sample) in field.samples.iter().enumerate() {
        if index % usize::try_from(field.width).unwrap_or(1) == 0 && cancellation.is_cancelled() { return Err(RectificationError::Cancelled); }
        let included=sample.is_some_and(|sample| {
            let crop=polygon.is_none_or(|points| point_in_polygon(sample.source,points));
            let alpha=minimum_alpha.is_none_or(|threshold| sample_color(base,sample).alpha>=threshold);
            crop&&alpha
        });
        values.push(MaskValue(if included {1.0}else{0.0}));
    }
    ImagePlane::from_row_major(field.width,field.height,tile_edge,&values).map(Some).map_err(|_|RectificationError::PlaneConstruction)
}

fn point_in_polygon(point: Point, polygon: &[hot_trimmer_domain::NormalizedPoint]) -> bool {
    let mut inside=false; let mut previous=polygon.len()-1;
    for current in 0..polygon.len() {
        let a=Point::from(polygon[current]); let b=Point::from(polygon[previous]);
        if ((a.y>point.y)!=(b.y>point.y)) && point.x < (b.x-a.x)*(point.y-a.y)/(b.y-a.y)+a.x { inside=!inside; }
        previous=current;
    }
    inside
}

fn settings_digest(request: &PreparedExemplarRequest) -> ContentDigest {
    ContentDigest::sha256(format!("{request:?}").as_bytes())
}

fn geometry_digest(request: &PreparedExemplarRequest) -> ContentDigest {
    ContentDigest::sha256(
        format!("{:?}|{:?}|{:?}", request.area, request.lens_correction, request.mask).as_bytes(),
    )
}

#[must_use]
pub fn exemplar_cache_key(source: &PreparedChannelCacheKey, request: &PreparedExemplarRequest) -> PreparedExemplarCacheKey {
    let mut bytes=Vec::new(); bytes.extend_from_slice(STAGE_03_ALGORITHM_ID.as_bytes()); bytes.extend_from_slice(STAGE_03_ALGORITHM_VERSION.as_bytes());
    bytes.extend_from_slice(source.0.0.as_bytes()); bytes.extend_from_slice(&request.scope.source_set_id.to_bytes()); bytes.extend_from_slice(&request.scope.source_revision.to_le_bytes());
    if let Some(id)=request.scope.patch_id { bytes.extend_from_slice(&id.to_bytes()); } bytes.extend_from_slice(&request.scope.patch_revision.to_le_bytes()); bytes.extend_from_slice(format!("{request:?}").as_bytes());
    PreparedExemplarCacheKey(ContentDigest::sha256(&bytes))
}

#[cfg(test)]
mod tests {
    use hot_trimmer_domain::{MaterialChannelRole, NormalConvention, NormalizedPoint};
    use hot_trimmer_image_io::{
        ImagePlane, LinearColor, LinearScalar, NormalizationReport, PreparedChannel,
        PreparedChannelCacheKey, PreparedChannelSet, ResolutionPyramid, ResolvedAlphaMode,
        TangentNormal,
    };

    use super::*;

    fn point(x: f64, y: f64) -> NormalizedPoint {
        NormalizedPoint::new(x, y).expect("fixture point")
    }

    fn pyramid<T>(pixels: Vec<T>, width: u32, height: u32) -> ResolutionPyramid<T>
    where
        T: Clone,
    {
        ResolutionPyramid::from_levels(vec![
            ImagePlane::from_row_major(width, height, 4, &pixels).expect("fixture plane"),
        ])
        .expect("fixture pyramid")
    }

    fn prepared_source() -> PreparedChannelSet {
        let width = 16;
        let height = 12;
        let mut colors = Vec::new();
        let mut scalars = Vec::new();
        let mut normals = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let grid = f32::from(((x / 2 + y / 2) % 2) as u8);
                colors.push(LinearColor {
                    rgb: [grid, f32::from(x as u16) / 15.0, f32::from(y as u16) / 11.0],
                    alpha: if x > 1 && y > 1 { 1.0 } else { 0.0 },
                });
                scalars.push(LinearScalar(grid));
                let normal_x = grid * 0.2;
                let normal_length = normal_x.mul_add(normal_x, 1.0).sqrt();
                normals.push(TangentNormal { xyz: [normal_x / normal_length, 0.0, 1.0 / normal_length], alpha: 1.0 });
            }
        }
        PreparedChannelSet {
            cache_key: PreparedChannelCacheKey(ContentDigest::sha256(b"registered-grid")),
            channels: vec![
                PreparedChannel::BaseColor {
                    linear: pyramid(colors, width, height),
                    srgb_display: pyramid(
                        vec![hot_trimmer_image_io::SrgbDisplayColor([0, 0, 0, 255]);
                            usize::try_from(width * height).expect("bounded")],
                        width,
                        height,
                    ),
                    alpha_mode: ResolvedAlphaMode::Straight,
                },
                PreparedChannel::Scalar {
                    role: MaterialChannelRole::Roughness,
                    pyramid: pyramid(scalars, width, height),
                },
                PreparedChannel::Normal {
                    pyramid: pyramid(normals, width, height),
                    source_convention: NormalConvention::OpenGl,
                    canonical_convention: NormalConvention::OpenGl,
                    alpha_policy: NormalAlphaPolicy::Preserve,
                },
            ],
            report: NormalizationReport {
                diagnostics: Vec::new(),
                peak_declared_bytes: 0,
                level_dimensions: vec![(width, height)],
            },
        }
    }

    fn request(quality: RectificationQuality) -> PreparedExemplarRequest {
        PreparedExemplarRequest {
            exemplar_id: "grid".into(),
            area: PlanarArea::FourPoint {
                corners: [
                    point(0.08, 0.12),
                    point(0.92, 0.04),
                    point(0.84, 0.94),
                    point(0.14, 0.86),
                ],
            },
            lens_correction: Some(LensCorrection {
                center: point(0.5, 0.5),
                radial_k1: 0.03,
                radial_k2: -0.01,
            }),
            mask: ExemplarMaskIntent {
                crop_polygon: Some(vec![
                    point(0.12, 0.12), point(0.88, 0.08),
                    point(0.82, 0.88), point(0.16, 0.84),
                ]),
                minimum_alpha: Some(0.25),
            },
            rectification: RectificationSettings { aspect_ratio: None, scale: 2.0 },
            physical_aspect_ratio: None,
            quality,
            limits: RectificationWorkLimits {
                preview_max_edge: 12,
                authoritative_max_edge: 128,
                max_pixels: 16_384,
                tile_edge: 4,
            },
            scope: PreparedExemplarScope {
                source_set_id: SourceSetId::from_bytes([1; 16]),
                source_revision: 4,
                patch_id: Some(PatchId::from_bytes([2; 16])),
                patch_revision: 7,
            },
        }
    }

    #[test]
    fn algorithm_stage_03_rectification() {
        let source = prepared_source();
        let cancellation = RenderCancellationToken::new();
        let authoritative = prepare_registered_exemplar(
            &source,
            &request(RectificationQuality::Authoritative),
            &cancellation,
        )
        .expect("registered rectification");
        assert!(matches!(authoritative.stage_result, StageResult::Executed { .. }));
        assert!(authoritative.perspective_confidence_milli > 0);
        assert!(authoritative.usable_mask.as_ref().is_some_and(|mask| {
            let values = mask.to_row_major();
            values.iter().any(|value| value.0 == 0.0)
                && values.iter().any(|value| value.0 == 1.0)
        }));

        let base = authoritative.channels.iter().find_map(|channel| match channel {
            PreparedExemplarChannel::BaseColor { plane, .. } => Some(plane),
            _ => None,
        }).expect("base color");
        let roughness = authoritative.channels.iter().find_map(|channel| match channel {
            PreparedExemplarChannel::Scalar { role: MaterialChannelRole::Roughness, plane } => Some(plane),
            _ => None,
        }).expect("roughness");
        let mut compared_opaque_samples = 0_u32;
        for y in 0..authoritative.height {
            for x in 0..authoritative.width {
                if base.pixel(x, y).alpha > 0.999 {
                    assert!((base.pixel(x, y).rgb[0] - roughness.pixel(x, y).0).abs() < 1.0e-5);
                    compared_opaque_samples += 1;
                }
            }
        }
        assert!(compared_opaque_samples > 0);
        let normal = authoritative.channels.iter().find_map(|channel| match channel {
            PreparedExemplarChannel::Normal { plane, .. } => Some(plane),
            _ => None,
        }).expect("normal");
        assert!(normal.to_row_major().iter().all(|value| {
            let length = value.xyz.iter().map(|component| component * component).sum::<f32>().sqrt();
            (length - 1.0).abs() < 1.0e-4
        }));

        let preview = prepare_registered_exemplar(
            &source,
            &request(RectificationQuality::Preview),
            &cancellation,
        )
        .expect("preview rectification");
        assert_eq!(preview.geometry_digest, authoritative.geometry_digest);
        assert!(preview.width < authoritative.width || preview.height < authoritative.height);

        let mut identity_request = request(RectificationQuality::Authoritative);
        identity_request.area = PlanarArea::FullFrame { usable_area: None };
        identity_request.lens_correction = None;
        identity_request.mask = ExemplarMaskIntent::default();
        identity_request.rectification = RectificationSettings::default();
        let identity = prepare_registered_exemplar(&source, &identity_request, &cancellation)
            .expect("identity rectification");
        let original_normal = match &source.channels[2] {
            PreparedChannel::Normal { pyramid, .. } => pyramid.level(0).expect("normal level"),
            _ => panic!("normal channel"),
        };
        let identity_normal = match &identity.channels[2] {
            PreparedExemplarChannel::Normal { plane, .. } => plane,
            _ => panic!("normal channel"),
        };
        for y in 0..identity.height {
            for x in 0..identity.width {
                let expected = original_normal.pixel(x, y).xyz;
                let actual = identity_normal.pixel(x, y).xyz;
                assert!(expected.into_iter().zip(actual).all(|(a, b)| (a - b).abs() < 1.0e-5));
            }
        }

        let mut lens_boundary_request = identity_request.clone();
        lens_boundary_request.lens_correction = Some(LensCorrection {
            center: point(0.0, 0.0), radial_k1: 0.1, radial_k2: 0.0,
        });
        let lens_boundary = prepare_registered_exemplar(&source, &lens_boundary_request, &cancellation)
            .expect("bounded lens correction");
        let lens_mask = lens_boundary.usable_mask.as_ref().expect("lens coverage mask");
        assert!(lens_mask.to_row_major().iter().any(|value| value.0 == 0.0));
        let lens_base = match &lens_boundary.channels[0] {
            PreparedExemplarChannel::BaseColor { plane, .. } => plane,
            _ => panic!("base color"),
        };
        assert!(lens_mask.to_row_major().iter().zip(lens_base.to_row_major()).all(|(mask, color)| {
            mask.0 != 0.0 || color == LinearColor { rgb: [0.0; 3], alpha: 0.0 }
        }));

        let mut pass_request = request(RectificationQuality::Authoritative);
        pass_request.area = PlanarArea::PassThrough { reason: PassThroughReason::AuthoredPlanarTexture };
        pass_request.lens_correction = None;
        pass_request.mask = ExemplarMaskIntent::default();
        pass_request.rectification = RectificationSettings::default();
        let passed = prepare_registered_exemplar(&source, &pass_request, &cancellation)
            .expect("byte-stable pass through");
        assert!(matches!(passed.stage_result, StageResult::PassThrough { .. }));
        let original = match &source.channels[0] {
            PreparedChannel::BaseColor { linear, .. } => linear.level(0).expect("level zero"),
            _ => panic!("base color first"),
        };
        let passed_base = match &passed.channels[0] {
            PreparedExemplarChannel::BaseColor { plane, .. } => plane,
            _ => panic!("base color first"),
        };
        assert_eq!(passed_base, original);

        let mut crossed = request(RectificationQuality::Authoritative);
        crossed.area = PlanarArea::FourPoint {
            corners: [point(0.1, 0.1), point(0.9, 0.9), point(0.9, 0.1), point(0.1, 0.9)],
        };
        let failure = prepare_registered_exemplar(&source, &crossed, &cancellation)
            .expect_err("crossed geometry must not publish");
        assert!(matches!(failure, RectificationError::Geometry(_)));
        assert!(matches!(failure.failed_stage_result(), StageResult::FailedWithRecovery { .. }));

        let mut excessive = request(RectificationQuality::Authoritative);
        excessive.lens_correction = Some(LensCorrection {
            center: point(0.5, 0.5), radial_k1: 0.5, radial_k2: 0.25,
        });
        assert_eq!(
            prepare_registered_exemplar(&source, &excessive, &cancellation),
            Err(RectificationError::LensCorrectionOutOfBounds),
        );

        let mut cache = PreparedExemplarCache::default();
        cache.insert_complete(authoritative);
        cache.insert_complete(preview);
        assert_eq!(cache.invalidate_patch(PatchId::from_bytes([2; 16])), 2);
    }
}
