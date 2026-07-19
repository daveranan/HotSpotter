//! Immutable Stage 1 material-source intent. Pixel buffers belong to image I/O, not the domain.

use serde::{Deserialize, Serialize};

use crate::{ContentDigest, SourceId, SourceSetId};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialChannelRole {
    BaseColor,
    Normal,
    Height,
    Roughness,
    Metallic,
    AmbientOcclusion,
    Specular,
    Opacity,
    EdgeMask,
    RegionId,
    MaterialId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelInterpretation {
    ColorManagedBaseColor,
    TangentSpaceNormal,
    LinearScalar,
    LinearOpacity,
    BinaryMask,
    CategoricalId,
}

impl MaterialChannelRole {
    #[must_use]
    pub const fn required_interpretation(self) -> ChannelInterpretation {
        match self {
            Self::BaseColor => ChannelInterpretation::ColorManagedBaseColor,
            Self::Normal => ChannelInterpretation::TangentSpaceNormal,
            Self::Opacity => ChannelInterpretation::LinearOpacity,
            Self::EdgeMask => ChannelInterpretation::BinaryMask,
            Self::RegionId | Self::MaterialId => ChannelInterpretation::CategoricalId,
            Self::Height
            | Self::Roughness
            | Self::Metallic
            | Self::AmbientOcclusion
            | Self::Specular => ChannelInterpretation::LinearScalar,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NormalConvention {
    NotApplicable,
    OpenGl,
    DirectX,
    Unspecified,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceOwnershipIntent {
    OwnedCopy,
    VerifiedExternalReference,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssignmentProvenance {
    UserAssigned,
    FilenameSuggested,
    EmbeddedMetadata,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrientedPixelSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginalAssetProvenance {
    /// Informational import path. Owned storage is represented separately and may move.
    pub original_path: String,
    pub immutable_digest: ContentDigest,
    pub encoded_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelRegistration {
    pub role: MaterialChannelRole,
    pub interpretation: ChannelInterpretation,
    pub normal_convention: NormalConvention,
    pub assignment_provenance: AssignmentProvenance,
    /// Integer confidence in [0, 1000], avoiding unstable floats in persisted intent.
    pub confidence_milli: u16,
}

impl ChannelRegistration {
    #[must_use]
    pub const fn explicit(role: MaterialChannelRole) -> Self {
        Self {
            role,
            interpretation: role.required_interpretation(),
            normal_convention: if matches!(role, MaterialChannelRole::Normal) {
                NormalConvention::Unspecified
            } else {
                NormalConvention::NotApplicable
            },
            assignment_provenance: AssignmentProvenance::UserAssigned,
            confidence_milli: 1000,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredChannel {
    pub source_id: SourceId,
    pub registration: ChannelRegistration,
    pub oriented_size: OrientedPixelSize,
    /// Original EXIF orientation transform (1-8), applied before the oriented size was measured.
    pub orientation: u16,
    pub original: OriginalAssetProvenance,
    pub ownership: SourceOwnershipIntent,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredChannelSet {
    pub oriented_size: OrientedPixelSize,
    pub orientation: u16,
    /// Stable role order; Base Color is always first and anchors this set.
    pub channels: Vec<RegisteredChannel>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialSource {
    pub id: SourceSetId,
    pub name: String,
    pub exemplar_group: Option<String>,
    pub source_revision: u64,
    pub registration_digest: ContentDigest,
    /// Persisted Stage 4 intent. De-lighting is never inferred from source metadata.
    #[serde(default)]
    pub delighting: DelightingIntent,
    /// Persisted Stage 5 routing intent. `None` means route from measured analysis.
    #[serde(default)]
    pub classification: MaterialClassificationIntent,
    /// Persisted Stage 6 calibration intent. Derived orientation fields remain cache-owned.
    #[serde(default)]
    pub calibration: MaterialCalibrationIntent,
    pub registered_channels: Option<RegisteredChannelSet>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialBehaviorClass {
    AlreadyTileable,
    StochasticIsotropic,
    StochasticDirectional,
    PeriodicLatticeStructured,
    LayeredBanded,
    OrganicDirectional,
    ManufacturedPattern,
    UniqueDetail,
    RadialDetail,
    MixedUnknown,
}

impl MaterialBehaviorClass {
    pub const ALL: [Self; 10] = [
        Self::AlreadyTileable,
        Self::StochasticIsotropic,
        Self::StochasticDirectional,
        Self::PeriodicLatticeStructured,
        Self::LayeredBanded,
        Self::OrganicDirectional,
        Self::ManufacturedPattern,
        Self::UniqueDetail,
        Self::RadialDetail,
        Self::MixedUnknown,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::AlreadyTileable => "Already tileable",
            Self::StochasticIsotropic => "Stochastic isotropic",
            Self::StochasticDirectional => "Stochastic directional",
            Self::PeriodicLatticeStructured => "Periodic/lattice structured",
            Self::LayeredBanded => "Layered/banded",
            Self::OrganicDirectional => "Organic directional",
            Self::ManufacturedPattern => "Manufactured pattern",
            Self::UniqueDetail => "Unique detail",
            Self::RadialDetail => "Radial detail",
            Self::MixedUnknown => "Mixed/Unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialClassificationIntent {
    pub override_class: Option<MaterialBehaviorClass>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum MaterialClassificationCommand {
    Override { class: MaterialBehaviorClass },
    ResetToAnalysis,
}

impl MaterialClassificationIntent {
    pub fn apply(&mut self, command: MaterialClassificationCommand) {
        self.override_class = match command {
            MaterialClassificationCommand::Override { class } => Some(class),
            MaterialClassificationCommand::ResetToAnalysis => None,
        };
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleProvenance {
    Imported,
    UserMeasured,
    MotifDerived,
    Convention,
    PriorEstimated,
    #[default]
    RelativeOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldScaleAvailability {
    Available,
    UnavailablePriorEstimate,
    UnavailableRelativeOnly,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePixelPointMilli {
    /// Source-space coordinate in thousandths of a pixel, independent of display zoom.
    pub x: i64,
    /// Source-space coordinate in thousandths of a pixel, independent of display zoom.
    pub y: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalScaleEvidence {
    /// Thousandths of a source pixel per meter. `None` means no physical claim exists.
    pub source_pixels_per_meter_x_milli: Option<u64>,
    pub source_pixels_per_meter_y_milli: Option<u64>,
    pub provenance: ScaleProvenance,
    pub confidence_milli: u16,
    pub world_scale: WorldScaleAvailability,
}

impl Default for PhysicalScaleEvidence {
    fn default() -> Self {
        Self {
            source_pixels_per_meter_x_milli: None,
            source_pixels_per_meter_y_milli: None,
            provenance: ScaleProvenance::RelativeOnly,
            confidence_milli: 0,
            world_scale: WorldScaleAvailability::UnavailableRelativeOnly,
        }
    }
}

impl PhysicalScaleEvidence {
    #[must_use]
    pub const fn claims_world_accuracy(self) -> bool {
        matches!(self.world_scale, WorldScaleAvailability::Available)
            && self.source_pixels_per_meter_x_milli.is_some()
            && self.source_pixels_per_meter_y_milli.is_some()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialCalibrationIntent {
    pub scale: PhysicalScaleEvidence,
    /// An optional user-authored 180-degree-equivalent material axis in millidegrees `[0,180000)`.
    pub orientation_override_millidegrees: Option<u32>,
    /// Changes whenever a command changes a downstream physical footprint or orientation choice.
    pub revision: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum MaterialCalibrationCommand {
    SetImportedMetadata {
        source_pixels_per_meter_x_milli: u64,
        source_pixels_per_meter_y_milli: u64,
        confidence_milli: u16,
    },
    MeasureTwoPoints {
        start: SourcePixelPointMilli,
        end: SourcePixelPointMilli,
        distance_micrometers: u64,
    },
    SetKnownMotifSize {
        motif_width_pixels_milli: u64,
        motif_height_pixels_milli: u64,
        motif_width_micrometers: u64,
        motif_height_micrometers: u64,
        confidence_milli: u16,
    },
    OverrideScale {
        source_pixels_per_meter_x_milli: Option<u64>,
        source_pixels_per_meter_y_milli: Option<u64>,
        provenance: ScaleProvenance,
        confidence_milli: u16,
    },
    ResetScale,
    OverrideOrientation {
        axis_millidegrees: u32,
    },
    ResetOrientation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MaterialCalibrationError {
    InvalidScale,
    InvalidMeasurement,
    InvalidConfidence,
    InvalidOrientation,
}

impl MaterialCalibrationIntent {
    pub fn apply(
        &mut self,
        command: MaterialCalibrationCommand,
    ) -> Result<(), MaterialCalibrationError> {
        let next_scale = match command {
            MaterialCalibrationCommand::SetImportedMetadata {
                source_pixels_per_meter_x_milli: x,
                source_pixels_per_meter_y_milli: y,
                confidence_milli,
            } => Some(validated_scale(
                Some(x),
                Some(y),
                ScaleProvenance::Imported,
                confidence_milli,
            )?),
            MaterialCalibrationCommand::MeasureTwoPoints {
                start,
                end,
                distance_micrometers,
            } => {
                if distance_micrometers == 0 {
                    return Err(MaterialCalibrationError::InvalidMeasurement);
                }
                let dx = (end.x - start.x) as f64;
                let dy = (end.y - start.y) as f64;
                let distance_pixels_milli = dx.hypot(dy);
                if !distance_pixels_milli.is_finite() || distance_pixels_milli < 1.0 {
                    return Err(MaterialCalibrationError::InvalidMeasurement);
                }
                let ppm_milli =
                    (distance_pixels_milli * 1_000_000.0 / distance_micrometers as f64).round();
                if !(1.0..=u64::MAX as f64).contains(&ppm_milli) {
                    return Err(MaterialCalibrationError::InvalidMeasurement);
                }
                let value = ppm_milli as u64;
                Some(validated_scale(
                    Some(value),
                    Some(value),
                    ScaleProvenance::UserMeasured,
                    1000,
                )?)
            }
            MaterialCalibrationCommand::SetKnownMotifSize {
                motif_width_pixels_milli,
                motif_height_pixels_milli,
                motif_width_micrometers,
                motif_height_micrometers,
                confidence_milli,
            } => {
                if motif_width_micrometers == 0 || motif_height_micrometers == 0 {
                    return Err(MaterialCalibrationError::InvalidMeasurement);
                }
                let x = motif_width_pixels_milli
                    .checked_mul(1_000_000)
                    .and_then(|value| value.checked_div(motif_width_micrometers))
                    .ok_or(MaterialCalibrationError::InvalidMeasurement)?;
                let y = motif_height_pixels_milli
                    .checked_mul(1_000_000)
                    .and_then(|value| value.checked_div(motif_height_micrometers))
                    .ok_or(MaterialCalibrationError::InvalidMeasurement)?;
                Some(validated_scale(
                    Some(x),
                    Some(y),
                    ScaleProvenance::MotifDerived,
                    confidence_milli,
                )?)
            }
            MaterialCalibrationCommand::OverrideScale {
                source_pixels_per_meter_x_milli: x,
                source_pixels_per_meter_y_milli: y,
                provenance,
                confidence_milli,
            } => Some(validated_scale(x, y, provenance, confidence_milli)?),
            MaterialCalibrationCommand::ResetScale => Some(PhysicalScaleEvidence::default()),
            MaterialCalibrationCommand::OverrideOrientation { axis_millidegrees } => {
                if axis_millidegrees >= 180_000 {
                    return Err(MaterialCalibrationError::InvalidOrientation);
                }
                self.orientation_override_millidegrees = Some(axis_millidegrees);
                None
            }
            MaterialCalibrationCommand::ResetOrientation => {
                self.orientation_override_millidegrees = None;
                None
            }
        };
        if let Some(scale) = next_scale {
            self.scale = scale;
        }
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }
}

fn validated_scale(
    x: Option<u64>,
    y: Option<u64>,
    provenance: ScaleProvenance,
    confidence_milli: u16,
) -> Result<PhysicalScaleEvidence, MaterialCalibrationError> {
    if confidence_milli > 1000 {
        return Err(MaterialCalibrationError::InvalidConfidence);
    }
    let relative = provenance == ScaleProvenance::RelativeOnly;
    if relative {
        if x.is_some() || y.is_some() {
            return Err(MaterialCalibrationError::InvalidScale);
        }
        return Ok(PhysicalScaleEvidence::default());
    }
    if x.is_none() || y.is_none() || x == Some(0) || y == Some(0) {
        return Err(MaterialCalibrationError::InvalidScale);
    }
    let world_scale = if provenance == ScaleProvenance::PriorEstimated {
        WorldScaleAvailability::UnavailablePriorEstimate
    } else {
        WorldScaleAvailability::Available
    };
    Ok(PhysicalScaleEvidence {
        source_pixels_per_meter_x_milli: x,
        source_pixels_per_meter_y_milli: y,
        provenance,
        confidence_milli,
        world_scale,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelightingPassThroughReason {
    DefaultNewOrUnclassified,
    AuthoredTextureOrPbrSet,
    UserDisabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntrinsicProviderFallback {
    None,
    PassThrough,
    ClassicalLowFrequency,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelightingRadius {
    Pixels(u16),
    RelativeBasisPoints(u16),
    PhysicalMillimeters {
        millimeters_milli: u32,
        pixels_per_meter_milli: u32,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassicalDelightingSettings {
    pub strength_milli: u16,
    pub shadow_recovery_milli: u16,
    pub highlight_recovery_milli: u16,
    pub color_preservation_milli: u16,
    pub edge_preservation_milli: u16,
    pub radius: DelightingRadius,
    pub analyze_masks: bool,
}

impl Default for ClassicalDelightingSettings {
    fn default() -> Self {
        Self {
            strength_milli: 700,
            shadow_recovery_milli: 250,
            highlight_recovery_milli: 250,
            color_preservation_milli: 1000,
            edge_preservation_milli: 750,
            radius: DelightingRadius::RelativeBasisPoints(800),
            analyze_masks: true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "route", rename_all = "snake_case")]
pub enum DelightingRouteIntent {
    PassThrough {
        reason: DelightingPassThroughReason,
    },
    ClassicalLowFrequency,
    LocalIntrinsicProvider {
        provider_id: String,
        fallback: IntrinsicProviderFallback,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DelightingIntent {
    pub route: DelightingRouteIntent,
    pub classical: ClassicalDelightingSettings,
}

impl Default for DelightingIntent {
    fn default() -> Self {
        Self {
            route: DelightingRouteIntent::PassThrough {
                reason: DelightingPassThroughReason::DefaultNewOrUnclassified,
            },
            classical: ClassicalDelightingSettings::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationDiagnosticCode {
    BaseColorRequired,
    OrientedDimensionMismatch,
    OrientationMismatch,
    ChannelInterpretationMismatch,
    NormalConventionOnScalar,
    InvalidConfidence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationRecoveryChoice {
    AssignBaseColor,
    ChooseMatchingDimensions,
    ReorientCompanionExternally,
    ReassignChannelRole,
    ConfirmNormalConvention,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationDiagnostic {
    pub code: RegistrationDiagnosticCode,
    pub channel: MaterialChannelRole,
    pub message: String,
    pub recovery_choices: Vec<RegistrationRecoveryChoice>,
}

#[cfg(test)]
mod tests {
    use super::DelightingIntent;

    #[test]
    fn empty_legacy_delighting_payload_uses_the_default_route() {
        let intent: DelightingIntent = serde_json::from_str("{}").expect("default intent");

        assert_eq!(intent, DelightingIntent::default());
    }
}
