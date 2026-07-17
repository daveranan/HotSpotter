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
            Self::MaterialId => ChannelInterpretation::CategoricalId,
            Self::Height | Self::Roughness | Self::Metallic | Self::AmbientOcclusion | Self::Specular => {
                ChannelInterpretation::LinearScalar
            }
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
    pub registered_channels: Option<RegisteredChannelSet>,
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
    PassThrough { reason: DelightingPassThroughReason },
    ClassicalLowFrequency,
    LocalIntrinsicProvider {
        provider_id: String,
        fallback: IntrinsicProviderFallback,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
