#![doc = "Validated patch geometry, homography, polygon assistance, and rectified output sizing."]

mod layout;

pub use layout::{LayoutSolveError, solve_layout, validate_layout};

use std::cmp::Ordering;

use hot_trimmer_domain::{ErrorCode, NormalizedPoint, RectificationSettings, UserFacingError};
use thiserror::Error;

pub const NORMALIZED_COORDINATE_MIN: f64 = 0.0;
pub const NORMALIZED_COORDINATE_MAX: f64 = 1.0;
pub const DEFAULT_MINIMUM_PATCH_AREA: f64 = 1.0e-6;
const NUMERIC_EPSILON: f64 = 1.0e-10;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl From<NormalizedPoint> for Point {
    fn from(point: NormalizedPoint) -> Self {
        Self {
            x: point.x.get(),
            y: point.y.get(),
        }
    }
}

impl TryFrom<Point> for NormalizedPoint {
    type Error = GeometryError;

    fn try_from(point: Point) -> Result<Self, Self::Error> {
        NormalizedPoint::new(point.x, point.y).map_err(|_| GeometryError::OutOfBounds)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quadrilateral {
    corners: [NormalizedPoint; 4],
}

impl Quadrilateral {
    /// Validates a quadrilateral in top-left, top-right, bottom-right, bottom-left order.
    /// Coordinates use source-image axes, so the required positive signed area is clockwise on screen.
    ///
    /// # Errors
    ///
    /// Returns a local geometry failure when the corners cannot produce a stable perspective transform.
    pub fn new(corners: [NormalizedPoint; 4]) -> Result<Self, GeometryError> {
        Self::with_minimum_area(corners, DEFAULT_MINIMUM_PATCH_AREA)
    }

    /// Applies the same validation using a caller-selected normalized minimum area.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::InvalidMinimumArea`] when the threshold is not positive and finite, or a
    /// specific geometry error when the quadrilateral is invalid.
    pub fn with_minimum_area(
        corners: [NormalizedPoint; 4],
        minimum_area: f64,
    ) -> Result<Self, GeometryError> {
        if !minimum_area.is_finite() || minimum_area <= 0.0 {
            return Err(GeometryError::InvalidMinimumArea);
        }
        let points = corners.map(Point::from);
        for first in 0..4 {
            for second in (first + 1)..4 {
                if squared_distance(points[first], points[second]) <= NUMERIC_EPSILON {
                    return Err(GeometryError::DuplicateCorner);
                }
            }
        }
        if segments_intersect(points[0], points[1], points[2], points[3])
            || segments_intersect(points[1], points[2], points[3], points[0])
        {
            return Err(GeometryError::SelfIntersection);
        }
        let area = signed_area(&points);
        if area.abs() < minimum_area {
            return Err(GeometryError::AreaTooSmall {
                area: area.abs(),
                minimum: minimum_area,
            });
        }
        if area < 0.0 {
            return Err(GeometryError::WrongWinding);
        }
        let mut cross_sign = 0.0_f64;
        for index in 0..4 {
            let value = cross(
                points[(index + 1) % 4],
                points[(index + 2) % 4],
                points[index],
            );
            if value.abs() <= NUMERIC_EPSILON {
                return Err(GeometryError::Degenerate);
            }
            if cross_sign == 0.0 {
                cross_sign = value.signum();
            } else if value * cross_sign < 0.0 {
                return Err(GeometryError::NotConvex);
            }
        }
        Ok(Self { corners })
    }

    #[must_use]
    pub const fn corners(self) -> [NormalizedPoint; 4] {
        self.corners
    }

    #[must_use]
    pub fn signed_area(self) -> f64 {
        signed_area(&self.corners.map(Point::from))
    }

    /// Builds the inverse-mapping transform used to find a source sample for each normalized output point.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::SingularHomography`] when the accepted geometry is too numerically unstable
    /// to solve on the current platform.
    pub fn source_from_output(self) -> Result<Homography, GeometryError> {
        Homography::from_correspondences(
            [
                Point { x: 0.0, y: 0.0 },
                Point { x: 1.0, y: 0.0 },
                Point { x: 1.0, y: 1.0 },
                Point { x: 0.0, y: 1.0 },
            ],
            self.corners.map(Point::from),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Homography {
    matrix: [[f64; 3]; 3],
}

impl Homography {
    /// Estimates the exact projective transform for four point correspondences.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::SingularHomography`] for degenerate or numerically unstable input.
    pub fn from_correspondences(
        source: [Point; 4],
        destination: [Point; 4],
    ) -> Result<Self, GeometryError> {
        if source
            .iter()
            .chain(destination.iter())
            .any(|point| !point.x.is_finite() || !point.y.is_finite())
        {
            return Err(GeometryError::NonFinite);
        }

        let mut system = [[0.0_f64; 9]; 8];
        for (index, (from, to)) in source.into_iter().zip(destination).enumerate() {
            let first = index * 2;
            system[first] = [
                from.x,
                from.y,
                1.0,
                0.0,
                0.0,
                0.0,
                -to.x * from.x,
                -to.x * from.y,
                to.x,
            ];
            system[first + 1] = [
                0.0,
                0.0,
                0.0,
                from.x,
                from.y,
                1.0,
                -to.y * from.x,
                -to.y * from.y,
                to.y,
            ];
        }
        let solved = solve_eight_by_eight(system)?;
        Ok(Self {
            matrix: [
                [solved[0], solved[1], solved[2]],
                [solved[3], solved[4], solved[5]],
                [solved[6], solved[7], 1.0],
            ],
        })
    }

    #[must_use]
    pub fn transform(self, point: Point) -> Option<Point> {
        let denominator = self.matrix[2][0].mul_add(
            point.x,
            self.matrix[2][1].mul_add(point.y, self.matrix[2][2]),
        );
        if !denominator.is_finite() || denominator.abs() <= NUMERIC_EPSILON {
            return None;
        }
        let x = self.matrix[0][0].mul_add(
            point.x,
            self.matrix[0][1].mul_add(point.y, self.matrix[0][2]),
        ) / denominator;
        let y = self.matrix[1][0].mul_add(
            point.x,
            self.matrix[1][1].mul_add(point.y, self.matrix[1][2]),
        ) / denominator;
        (x.is_finite() && y.is_finite()).then_some(Point { x, y })
    }

    /// Inverts this projective transform.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::SingularHomography`] if the matrix has no stable inverse.
    pub fn inverse(self) -> Result<Self, GeometryError> {
        let matrix = self.matrix;
        let determinant = matrix[0][0]
            * (matrix[1][1] * matrix[2][2] - matrix[1][2] * matrix[2][1])
            - matrix[0][1] * (matrix[1][0] * matrix[2][2] - matrix[1][2] * matrix[2][0])
            + matrix[0][2] * (matrix[1][0] * matrix[2][1] - matrix[1][1] * matrix[2][0]);
        if !determinant.is_finite() || determinant.abs() <= NUMERIC_EPSILON {
            return Err(GeometryError::SingularHomography);
        }
        let reciprocal = determinant.recip();
        let inverse = [
            [
                (matrix[1][1] * matrix[2][2] - matrix[1][2] * matrix[2][1]) * reciprocal,
                (matrix[0][2] * matrix[2][1] - matrix[0][1] * matrix[2][2]) * reciprocal,
                (matrix[0][1] * matrix[1][2] - matrix[0][2] * matrix[1][1]) * reciprocal,
            ],
            [
                (matrix[1][2] * matrix[2][0] - matrix[1][0] * matrix[2][2]) * reciprocal,
                (matrix[0][0] * matrix[2][2] - matrix[0][2] * matrix[2][0]) * reciprocal,
                (matrix[0][2] * matrix[1][0] - matrix[0][0] * matrix[1][2]) * reciprocal,
            ],
            [
                (matrix[1][0] * matrix[2][1] - matrix[1][1] * matrix[2][0]) * reciprocal,
                (matrix[0][1] * matrix[2][0] - matrix[0][0] * matrix[2][1]) * reciprocal,
                (matrix[0][0] * matrix[1][1] - matrix[0][1] * matrix[1][0]) * reciprocal,
            ],
        ];
        Ok(Self { matrix: inverse })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PolygonAssistance {
    pub quadrilateral: Quadrilateral,
    pub mask: Option<Vec<NormalizedPoint>>,
    pub approximate: bool,
}

/// Fits an oriented rectangle to four through eight boundary samples. The source samples may optionally be
/// retained as a registration mask; the fitted result is always an editable quadrilateral.
///
/// # Errors
///
/// Returns a typed failure for an unsupported point count or degenerate point cloud.
pub fn assist_polygon(
    points: &[NormalizedPoint],
    retain_mask: bool,
) -> Result<PolygonAssistance, GeometryError> {
    if !(4..=8).contains(&points.len()) {
        return Err(GeometryError::PolygonPointCount {
            found: points.len(),
        });
    }
    let points_as_float: Vec<Point> = points.iter().copied().map(Point::from).collect();
    let count = f64::from(u32::try_from(points_as_float.len()).map_err(|_| {
        GeometryError::PolygonPointCount {
            found: points_as_float.len(),
        }
    })?);
    let center = Point {
        x: points_as_float.iter().map(|point| point.x).sum::<f64>() / count,
        y: points_as_float.iter().map(|point| point.y).sum::<f64>() / count,
    };
    let (xx, xy, yy) = points_as_float
        .iter()
        .fold((0.0, 0.0, 0.0), |(xx, xy, yy), point| {
            let dx = point.x - center.x;
            let dy = point.y - center.y;
            (dx.mul_add(dx, xx), dx.mul_add(dy, xy), dy.mul_add(dy, yy))
        });
    if xx + yy <= NUMERIC_EPSILON {
        return Err(GeometryError::Degenerate);
    }
    let angle = 0.5 * (2.0 * xy).atan2(xx - yy);
    let axis = Point {
        x: angle.cos(),
        y: angle.sin(),
    };
    let perpendicular = Point {
        x: -axis.y,
        y: axis.x,
    };
    let (min_axis, max_axis, min_perpendicular, max_perpendicular) = points_as_float.iter().fold(
        (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ),
        |bounds, point| {
            let relative = Point {
                x: point.x - center.x,
                y: point.y - center.y,
            };
            let along_axis = dot(relative, axis);
            let along_perpendicular = dot(relative, perpendicular);
            (
                bounds.0.min(along_axis),
                bounds.1.max(along_axis),
                bounds.2.min(along_perpendicular),
                bounds.3.max(along_perpendicular),
            )
        },
    );
    let fitted = [
        compose_axes(center, axis, min_axis, perpendicular, min_perpendicular),
        compose_axes(center, axis, max_axis, perpendicular, min_perpendicular),
        compose_axes(center, axis, max_axis, perpendicular, max_perpendicular),
        compose_axes(center, axis, min_axis, perpendicular, max_perpendicular),
    ];
    let quadrilateral = order_corners(fitted)
        .and_then(|ordered| {
            ordered
                .map(NormalizedPoint::try_from)
                .into_iter()
                .collect::<Result<Vec<_>, _>>()
        })
        .and_then(|ordered| {
            let corners: [NormalizedPoint; 4] =
                ordered.try_into().map_err(|_| GeometryError::Degenerate)?;
            Quadrilateral::new(corners)
        })
        .or_else(|_| axis_aligned_fit(&points_as_float))?;

    Ok(PolygonAssistance {
        quadrilateral,
        mask: retain_mask.then(|| points.to_vec()),
        approximate: points.len() > 4,
    })
}

/// Orders four arbitrary finite points into the canonical source-image corner order.
///
/// # Errors
///
/// Returns a geometry failure when points are repeated or cannot form a valid convex quadrilateral.
pub fn order_corners(points: [Point; 4]) -> Result<[Point; 4], GeometryError> {
    if points
        .iter()
        .any(|point| !point.x.is_finite() || !point.y.is_finite())
    {
        return Err(GeometryError::NonFinite);
    }
    let center = Point {
        x: points.iter().map(|point| point.x).sum::<f64>() / 4.0,
        y: points.iter().map(|point| point.y).sum::<f64>() / 4.0,
    };
    let mut ordered = points;
    ordered.sort_by(|left, right| {
        (left.y - center.y)
            .atan2(left.x - center.x)
            .partial_cmp(&(right.y - center.y).atan2(right.x - center.x))
            .unwrap_or(Ordering::Equal)
    });
    let first = ordered
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| {
            left.y
                .total_cmp(&right.y)
                .then_with(|| left.x.total_cmp(&right.x))
        })
        .map_or(0, |(index, _)| index);
    ordered.rotate_left(first);
    if signed_area(&ordered) < 0.0 {
        ordered.swap(1, 3);
    }
    let normalized = ordered
        .map(NormalizedPoint::try_from)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    let normalized: [NormalizedPoint; 4] = normalized
        .try_into()
        .map_err(|_| GeometryError::Degenerate)?;
    Quadrilateral::new(normalized)?;
    Ok(ordered)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputDimensions {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RectificationLimits {
    pub max_edge: u32,
    pub max_pixels: u64,
}

impl Default for RectificationLimits {
    fn default() -> Self {
        Self {
            max_edge: 16_384,
            max_pixels: 268_435_456,
        }
    }
}

/// Computes bounded rectified output dimensions from source-space footprint and user aspect/scale intent.
///
/// # Errors
///
/// Returns a typed failure when dimensions, settings, or resulting allocation exceed declared limits.
pub fn rectified_dimensions(
    quadrilateral: Quadrilateral,
    source_width: u32,
    source_height: u32,
    settings: RectificationSettings,
    limits: RectificationLimits,
) -> Result<OutputDimensions, GeometryError> {
    if source_width == 0 || source_height == 0 || limits.max_edge == 0 || limits.max_pixels == 0 {
        return Err(GeometryError::InvalidDimensions);
    }
    if !settings.is_valid() {
        return Err(GeometryError::InvalidRectificationSettings);
    }
    let corners = quadrilateral.corners.map(Point::from);
    let pixel = |point: Point| Point {
        x: point.x * f64::from(source_width),
        y: point.y * f64::from(source_height),
    };
    let top = distance(pixel(corners[0]), pixel(corners[1]));
    let bottom = distance(pixel(corners[3]), pixel(corners[2]));
    let left = distance(pixel(corners[0]), pixel(corners[3]));
    let right = distance(pixel(corners[1]), pixel(corners[2]));
    let measured_width = (top + bottom) * 0.5;
    let measured_height = (left + right) * 0.5;
    if measured_width <= NUMERIC_EPSILON || measured_height <= NUMERIC_EPSILON {
        return Err(GeometryError::Degenerate);
    }
    let aspect = settings
        .aspect_ratio
        .unwrap_or(measured_width / measured_height);
    let pixel_area = measured_width * measured_height * settings.scale * settings.scale;
    let width = (pixel_area * aspect).sqrt().round().max(1.0);
    let height = (pixel_area / aspect).sqrt().round().max(1.0);
    if width > f64::from(limits.max_edge) || height > f64::from(limits.max_edge) {
        return Err(GeometryError::OutputLimitExceeded);
    }
    let width = bounded_dimension(width, limits.max_edge)?;
    let height = bounded_dimension(height, limits.max_edge)?;
    if u64::from(width) * u64::from(height) > limits.max_pixels {
        return Err(GeometryError::OutputLimitExceeded);
    }
    Ok(OutputDimensions { width, height })
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum GeometryError {
    #[error("patch corners must remain inside the source image")]
    OutOfBounds,
    #[error("patch coordinates must be finite")]
    NonFinite,
    #[error("two patch corners occupy the same position")]
    DuplicateCorner,
    #[error("patch edges cross; move the corners into perimeter order")]
    SelfIntersection,
    #[error("patch winding is reversed; use top-left, top-right, bottom-right, bottom-left order")]
    WrongWinding,
    #[error("patch is concave; move every corner to the outside boundary")]
    NotConvex,
    #[error("patch edges are degenerate or collinear")]
    Degenerate,
    #[error("patch area {area} is below the minimum {minimum}")]
    AreaTooSmall { area: f64, minimum: f64 },
    #[error("minimum patch area must be positive and finite")]
    InvalidMinimumArea,
    #[error("perspective transform is singular")]
    SingularHomography,
    #[error("polygon assistance needs four through eight points; received {found}")]
    PolygonPointCount { found: usize },
    #[error("source or output dimensions are invalid")]
    InvalidDimensions,
    #[error("rectification aspect or scale is outside supported bounds")]
    InvalidRectificationSettings,
    #[error("rectified output exceeds the configured image limits")]
    OutputLimitExceeded,
}

impl From<GeometryError> for UserFacingError {
    fn from(error: GeometryError) -> Self {
        let (message, recovery) = match &error {
            GeometryError::OutOfBounds => (
                "A patch corner is outside the source image.",
                "Move every corner onto the image, then finish the patch.",
            ),
            GeometryError::NonFinite | GeometryError::InvalidDimensions => (
                "The patch coordinates cannot be measured.",
                "Fit the source in view and place the patch again.",
            ),
            GeometryError::DuplicateCorner
            | GeometryError::Degenerate
            | GeometryError::AreaTooSmall { .. } => (
                "The patch is too small or has overlapping corners.",
                "Move its corners apart to enclose a visible source area.",
            ),
            GeometryError::SelfIntersection | GeometryError::WrongWinding => (
                "The patch boundary crosses or folds over itself.",
                "Drag a point outward until the patch encloses one continuous area.",
            ),
            GeometryError::NotConvex => (
                "The patch bends inward at one corner.",
                "Move each corner to the outside edge of the area you want to capture.",
            ),
            GeometryError::SingularHomography => (
                "This perspective is too extreme to rectify reliably.",
                "Move the corners farther apart or choose a less edge-on area.",
            ),
            GeometryError::PolygonPointCount { .. } => (
                "Polygon assistance needs four through eight points.",
                "Add or remove boundary points, then fit the patch again.",
            ),
            GeometryError::InvalidMinimumArea | GeometryError::InvalidRectificationSettings => (
                "The patch output settings are invalid.",
                "Choose a positive aspect and a scale between 1% and 1600%.",
            ),
            GeometryError::OutputLimitExceeded => (
                "The rectified patch would exceed the image memory limit.",
                "Lower the patch scale or output aspect, then retry.",
            ),
        };
        Self {
            code: ErrorCode::PatchGeometryInvalid,
            message: message.into(),
            recovery: recovery.into(),
            detail: Some(error.to_string()),
        }
    }
}

fn solve_eight_by_eight(mut system: [[f64; 9]; 8]) -> Result<[f64; 8], GeometryError> {
    for pivot_column in 0..8 {
        let pivot_row = (pivot_column..8)
            .max_by(|left, right| {
                system[*left][pivot_column]
                    .abs()
                    .total_cmp(&system[*right][pivot_column].abs())
            })
            .ok_or(GeometryError::SingularHomography)?;
        if system[pivot_row][pivot_column].abs() <= NUMERIC_EPSILON {
            return Err(GeometryError::SingularHomography);
        }
        system.swap(pivot_column, pivot_row);
        let divisor = system[pivot_column][pivot_column];
        for value in system[pivot_column].iter_mut().skip(pivot_column) {
            *value /= divisor;
        }
        let normalized_pivot = system[pivot_column];
        for (row, row_values) in system.iter_mut().enumerate() {
            if row == pivot_column {
                continue;
            }
            let factor = row_values[pivot_column];
            for (column, value) in row_values.iter_mut().enumerate().skip(pivot_column) {
                *value -= factor * normalized_pivot[column];
            }
        }
    }
    Ok(std::array::from_fn(|row| system[row][8]))
}

fn bounded_dimension(value: f64, max_edge: u32) -> Result<u32, GeometryError> {
    if !value.is_finite() || value < 1.0 || value > f64::from(max_edge) {
        return Err(GeometryError::OutputLimitExceeded);
    }
    // The bounds above make this conversion exact for all supported image edges.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let dimension = value as u32;
    Ok(dimension)
}

fn signed_area(points: &[Point; 4]) -> f64 {
    points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let next = points[(index + 1) % 4];
            point.x * next.y - next.x * point.y
        })
        .sum::<f64>()
        * 0.5
}

fn cross(a: Point, b: Point, origin: Point) -> f64 {
    (a.x - origin.x) * (b.y - origin.y) - (a.y - origin.y) * (b.x - origin.x)
}

fn segments_intersect(first: Point, second: Point, third: Point, fourth: Point) -> bool {
    let orientation_a = cross(second, third, first);
    let orientation_b = cross(second, fourth, first);
    let orientation_c = cross(fourth, first, third);
    let orientation_d = cross(fourth, second, third);
    orientation_a * orientation_b < -NUMERIC_EPSILON
        && orientation_c * orientation_d < -NUMERIC_EPSILON
}

fn squared_distance(first: Point, second: Point) -> f64 {
    (first.x - second.x).mul_add(first.x - second.x, (first.y - second.y).powi(2))
}

fn distance(first: Point, second: Point) -> f64 {
    squared_distance(first, second).sqrt()
}

fn dot(first: Point, second: Point) -> f64 {
    first.x.mul_add(second.x, first.y * second.y)
}

fn compose_axes(
    center: Point,
    first: Point,
    first_scale: f64,
    second: Point,
    second_scale: f64,
) -> Point {
    Point {
        x: first
            .x
            .mul_add(first_scale, second.x.mul_add(second_scale, center.x)),
        y: first
            .y
            .mul_add(first_scale, second.y.mul_add(second_scale, center.y)),
    }
}

fn axis_aligned_fit(points: &[Point]) -> Result<Quadrilateral, GeometryError> {
    let bounds = points.iter().fold(
        (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ),
        |bounds, point| {
            (
                bounds.0.min(point.x),
                bounds.1.max(point.x),
                bounds.2.min(point.y),
                bounds.3.max(point.y),
            )
        },
    );
    Quadrilateral::new([
        NormalizedPoint::new(bounds.0, bounds.2).map_err(|_| GeometryError::OutOfBounds)?,
        NormalizedPoint::new(bounds.1, bounds.2).map_err(|_| GeometryError::OutOfBounds)?,
        NormalizedPoint::new(bounds.1, bounds.3).map_err(|_| GeometryError::OutOfBounds)?,
        NormalizedPoint::new(bounds.0, bounds.3).map_err(|_| GeometryError::OutOfBounds)?,
    ])
}

#[cfg(test)]
mod tests {
    use super::{
        GeometryError, Point, Quadrilateral, RectificationLimits, assist_polygon, order_corners,
        rectified_dimensions,
    };
    use hot_trimmer_domain::{NormalizedPoint, RectificationSettings};

    fn normalized(x: f64, y: f64) -> NormalizedPoint {
        NormalizedPoint::new(x, y).expect("normalized test point")
    }

    fn assert_near(left: f64, right: f64) {
        assert!((left - right).abs() < 1.0e-8, "{left} != {right}");
    }

    #[test]
    fn homography_round_trip_matches_skewed_corners_and_interior() {
        let quadrilateral = Quadrilateral::new([
            normalized(0.12, 0.08),
            normalized(0.91, 0.16),
            normalized(0.82, 0.88),
            normalized(0.18, 0.79),
        ])
        .expect("valid quadrilateral");
        let forward = quadrilateral.source_from_output().expect("homography");
        let inverse = forward.inverse().expect("inverse");
        for point in [
            Point { x: 0.0, y: 0.0 },
            Point { x: 1.0, y: 0.0 },
            Point { x: 1.0, y: 1.0 },
            Point { x: 0.0, y: 1.0 },
            Point { x: 0.37, y: 0.61 },
        ] {
            let source = forward.transform(point).expect("mapped source");
            let restored = inverse.transform(source).expect("mapped output");
            assert_near(restored.x, point.x);
            assert_near(restored.y, point.y);
        }
    }

    #[test]
    fn canonical_order_is_stable_for_rotated_input() {
        let ordered = order_corners([
            Point { x: 0.8, y: 0.8 },
            Point { x: 0.2, y: 0.2 },
            Point { x: 0.2, y: 0.8 },
            Point { x: 0.8, y: 0.2 },
        ])
        .expect("ordered corners");
        assert_eq!(ordered[0], Point { x: 0.2, y: 0.2 });
        assert!(super::signed_area(&ordered) > 0.0);
    }

    #[test]
    fn rejects_crossed_reversed_concave_and_tiny_geometry() {
        assert_eq!(
            Quadrilateral::new([
                normalized(0.1, 0.1),
                normalized(0.9, 0.9),
                normalized(0.1, 0.9),
                normalized(0.9, 0.1),
            ]),
            Err(GeometryError::SelfIntersection)
        );
        assert_eq!(
            Quadrilateral::new([
                normalized(0.1, 0.1),
                normalized(0.1, 0.9),
                normalized(0.9, 0.9),
                normalized(0.9, 0.1),
            ]),
            Err(GeometryError::WrongWinding)
        );
        assert_eq!(
            Quadrilateral::new([
                normalized(0.1, 0.1),
                normalized(0.9, 0.1),
                normalized(0.4, 0.4),
                normalized(0.1, 0.9),
            ]),
            Err(GeometryError::NotConvex)
        );
        assert!(matches!(
            Quadrilateral::new([
                normalized(0.1, 0.1),
                normalized(0.1001, 0.1),
                normalized(0.1001, 0.1001),
                normalized(0.1, 0.1001),
            ]),
            Err(GeometryError::AreaTooSmall { .. })
        ));
    }

    #[test]
    fn polygon_assistance_retains_optional_mask_and_fits_valid_quad() {
        let points = [
            normalized(0.2, 0.2),
            normalized(0.5, 0.15),
            normalized(0.8, 0.25),
            normalized(0.85, 0.6),
            normalized(0.7, 0.8),
            normalized(0.25, 0.75),
        ];
        let assistance = assist_polygon(&points, true).expect("polygon fit");
        assert_eq!(assistance.mask.as_deref(), Some(points.as_slice()));
        assert!(assistance.approximate);
        assert!(assistance.quadrilateral.signed_area() > 0.0);
    }

    #[test]
    fn output_dimensions_follow_source_footprint_aspect_and_limits() {
        let quadrilateral = Quadrilateral::new([
            normalized(0.25, 0.25),
            normalized(0.75, 0.25),
            normalized(0.75, 0.75),
            normalized(0.25, 0.75),
        ])
        .expect("quad");
        let dimensions = rectified_dimensions(
            quadrilateral,
            2000,
            1000,
            RectificationSettings::default(),
            RectificationLimits::default(),
        )
        .expect("dimensions");
        assert_eq!(dimensions.width, 1000);
        assert_eq!(dimensions.height, 500);

        assert_eq!(
            rectified_dimensions(
                quadrilateral,
                20_000,
                20_000,
                RectificationSettings {
                    aspect_ratio: None,
                    scale: 2.0,
                },
                RectificationLimits::default(),
            ),
            Err(GeometryError::OutputLimitExceeded)
        );
    }

    #[test]
    fn property_homography_round_trips_across_many_valid_skews() {
        let mut state = 0x4d59_5df4_u32;
        let mut next = || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            f64::from(state) / f64::from(u32::MAX)
        };
        for _ in 0..512 {
            let quadrilateral = Quadrilateral::new([
                normalized(0.05 + next() * 0.15, 0.05 + next() * 0.15),
                normalized(0.80 + next() * 0.15, 0.05 + next() * 0.15),
                normalized(0.80 + next() * 0.15, 0.80 + next() * 0.15),
                normalized(0.05 + next() * 0.15, 0.80 + next() * 0.15),
            ])
            .expect("generated convex quadrilateral");
            let forward = quadrilateral
                .source_from_output()
                .expect("forward transform");
            let inverse = forward.inverse().expect("inverse transform");
            for x in [0.0, 0.2, 0.5, 0.8, 1.0] {
                for y in [0.0, 0.3, 0.7, 1.0] {
                    let point = Point { x, y };
                    let restored = inverse
                        .transform(forward.transform(point).expect("forward point"))
                        .expect("inverse point");
                    assert_near(restored.x, x);
                    assert_near(restored.y, y);
                }
            }
        }
    }

    #[test]
    fn property_corner_order_is_invariant_across_all_permutations() {
        let corners = [
            Point { x: 0.1, y: 0.2 },
            Point { x: 0.9, y: 0.2 },
            Point { x: 0.9, y: 0.8 },
            Point { x: 0.1, y: 0.8 },
        ];
        let permutations = [
            [0, 1, 2, 3],
            [0, 1, 3, 2],
            [0, 2, 1, 3],
            [0, 2, 3, 1],
            [0, 3, 1, 2],
            [0, 3, 2, 1],
            [1, 0, 2, 3],
            [1, 0, 3, 2],
            [1, 2, 0, 3],
            [1, 2, 3, 0],
            [1, 3, 0, 2],
            [1, 3, 2, 0],
            [2, 0, 1, 3],
            [2, 0, 3, 1],
            [2, 1, 0, 3],
            [2, 1, 3, 0],
            [2, 3, 0, 1],
            [2, 3, 1, 0],
            [3, 0, 1, 2],
            [3, 0, 2, 1],
            [3, 1, 0, 2],
            [3, 1, 2, 0],
            [3, 2, 0, 1],
            [3, 2, 1, 0],
        ];
        for permutation in permutations {
            let input = permutation.map(|index| corners[index]);
            assert_eq!(order_corners(input).expect("ordered permutation"), corners);
        }
    }
}
