use serde::{Deserialize, Serialize};

use crate::DomainError;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NormalizedScalar(f64);

impl NormalizedScalar {
    /// Creates a finite scalar inside the inclusive normalized range.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidNormalizedCoordinate`] for non-finite or out-of-range values.
    pub fn new(value: f64) -> Result<Self, DomainError> {
        if value.is_finite() && (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(DomainError::InvalidNormalizedCoordinate { value })
        }
    }

    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedPoint {
    pub x: NormalizedScalar,
    pub y: NormalizedScalar,
}

impl NormalizedPoint {
    /// Creates a point whose axes both use normalized coordinates.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidNormalizedCoordinate`] when either axis is invalid.
    pub fn new(x: f64, y: f64) -> Result<Self, DomainError> {
        Ok(Self {
            x: NormalizedScalar::new(x)?,
            y: NormalizedScalar::new(y)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::NormalizedPoint;

    #[test]
    fn rejects_non_finite_and_out_of_range_coordinates() {
        assert!(NormalizedPoint::new(f64::NAN, 0.5).is_err());
        assert!(NormalizedPoint::new(-0.1, 0.5).is_err());
        assert!(NormalizedPoint::new(0.5, 1.1).is_err());
    }
}
