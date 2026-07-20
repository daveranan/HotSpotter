use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    Channel, DecorationBinding, GridRect, IdColor, LayerId, LayoutId, LogicalGridSpec,
    NormalizedBounds, NormalizedPoint, PartitionFamily, PartitionProvenance, PartitionRecipe,
    PartitionTreeNode, Patch, PatchId, PixelSize, RegionId, RegionSourceOverride, SourceBlend,
    SourceFrame, SourceSamplingMode, SourceSetId, SourceWarp, StructuralProfile,
    TemplateDefinition, TemplateFitSemantics, TemplateRegistry, TemplateSlotRole, TemplateSnapshot,
    layout::source_warp_is_valid,
    source_frame_region_id,
    templates::{CanonicalRect, RadialParameters, TemplateSourceMapping},
};

pub type TrimSheetId = LayoutId;

pub const TRIM_SHEET_DOCUMENT_SCHEMA_VERSION: u32 = 1;
pub const WARP_OPERATION_VERSION: u16 = 1;
pub const MAX_MAPPING_MAGNITUDE: f64 = 16.0;

/// An authored source-frame divider is always moved as one shared lattice boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartitionAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialMapKind {
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialMapContent {
    pub kind: MaterialMapKind,
    pub sha256: String,
}

/// Renderer-relevant identity and immutable content digests for one registered material source set.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialSourceSet {
    pub id: SourceSetId,
    pub name: String,
    pub maps: Vec<MaterialMapContent>,
}

/// A typed registration for a future procedural recipe. This slice stores no executable algorithm.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProceduralMaterial {
    pub id: LayerId,
    pub recipe_id: String,
    pub recipe_version: u16,
    pub content_hash: DocumentHash,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SolidChannelValues {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_color: Option<[u8; 4]>,
    #[serde(default)]
    pub scalar_channels: BTreeMap<Channel, f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum ContentReference {
    InheritPrimaryMaterial,
    MaterialSource(SourceSetId),
    Patch(PatchId),
    Solid(SolidChannelValues),
    Procedural(LayerId),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum Projection {
    Crop {
        bounds: NormalizedBounds,
        focus: NormalizedPoint,
    },
    Perspective {
        quad: [NormalizedPoint; 4],
    },
}

/// Whether a region's source crop was authored by the user or is still waiting
/// for source placement. This is deliberately separate from template allocation
/// coordinates; an unplaced region samples the complete prepared source domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceCropIntent {
    Unplaced,
    Authored,
}

impl Default for Projection {
    fn default() -> Self {
        Self::Crop {
            bounds: NormalizedBounds {
                x: crate::NormalizedScalar::new(0.0).expect("normalized zero"),
                y: crate::NormalizedScalar::new(0.0).expect("normalized zero"),
                width: crate::NormalizedScalar::new(1.0).expect("normalized one"),
                height: crate::NormalizedScalar::new(1.0).expect("normalized one"),
            },
            focus: NormalizedPoint::new(0.5, 0.5).expect("normalized center"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MappingTransform {
    pub scale: [f64; 2],
    pub rotation_degrees: f64,
    pub mirror_x: bool,
    pub mirror_y: bool,
    pub offset: [f64; 2],
}

impl Default for MappingTransform {
    fn default() -> Self {
        Self {
            scale: [1.0, 1.0],
            rotation_degrees: 0.0,
            mirror_x: false,
            mirror_y: false,
            offset: [0.0, 0.0],
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AddressMode {
    #[default]
    Clamp,
    Repeat,
    MirroredRepeat,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SamplingPolicy {
    pub filter: SourceSamplingMode,
    pub scale: f64,
    pub correct_tangent_normals: bool,
}

impl Default for SamplingPolicy {
    fn default() -> Self {
        Self {
            filter: SourceSamplingMode::Linear,
            scale: 1.0,
            correct_tangent_normals: true,
        }
    }
}

/// One stable, versioned operation in evaluation order. Parameters remain the typed `SourceWarp`
/// variants already consumed by the renderer; no generic JSON recipe is accepted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WarpOperation {
    pub id: LayerId,
    pub version: u16,
    pub enabled: bool,
    pub operation: SourceWarp,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadialMappingSettings {
    pub center_x: f64,
    pub center_y: f64,
    pub inner_radius: f64,
    pub outer_radius: f64,
    pub falloff: f64,
    #[serde(default)]
    pub blend_width: f64,
    #[serde(default)]
    pub seam_blend_width: f64,
}

pub const REGION_BEHAVIOR_VERSION: u16 = 2;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManualRegionRole {
    #[default]
    Panel,
    HorizontalStrip,
    VerticalStrip,
    Unique,
    Radial,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionContinuity {
    #[default]
    None,
    X,
    Y,
    Xy,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionSampling {
    #[default]
    OneShot,
    LoopX,
    LoopY,
    LoopXy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeEligibility {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

impl EdgeEligibility {
    #[must_use]
    pub const fn for_continuity(continuity: RegionContinuity) -> Self {
        Self {
            left: !matches!(continuity, RegionContinuity::X | RegionContinuity::Xy),
            right: !matches!(continuity, RegionContinuity::X | RegionContinuity::Xy),
            top: !matches!(continuity, RegionContinuity::Y | RegionContinuity::Xy),
            bottom: !matches!(continuity, RegionContinuity::Y | RegionContinuity::Xy),
        }
    }
}

impl Default for EdgeEligibility {
    fn default() -> Self {
        Self::for_continuity(RegionContinuity::None)
    }
}

/// The sole persisted manual behavior contract for one region. `continuity` describes
/// destination structural seams; `sampling` independently describes source addressing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionBehavior {
    pub version: u16,
    pub role: ManualRegionRole,
    pub continuity: RegionContinuity,
    pub sampling: RegionSampling,
    /// Authored source-space repeat period. `None` means the exact assigned crop extent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period_pixels: Option<[u32; 2]>,
    pub orientation: QuarterTurn,
    pub edge_eligibility: EdgeEligibility,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radial: Option<RadialMappingSettings>,
}

impl RegionBehavior {
    #[must_use]
    pub fn new(role: ManualRegionRole) -> Self {
        let radial = (role == ManualRegionRole::Radial).then_some(RadialMappingSettings {
            center_x: 0.5,
            center_y: 0.5,
            inner_radius: 0.0,
            outer_radius: 0.5,
            falloff: 1.0,
            blend_width: 0.0,
            seam_blend_width: 0.03,
        });
        Self {
            role,
            radial,
            ..Self::default()
        }
    }

    #[must_use]
    pub const fn supports_sampling(&self) -> bool {
        matches!(
            (self.role, self.sampling),
            (ManualRegionRole::Panel, _)
                | (
                    ManualRegionRole::HorizontalStrip,
                    RegionSampling::OneShot | RegionSampling::LoopX
                )
                | (
                    ManualRegionRole::VerticalStrip,
                    RegionSampling::OneShot | RegionSampling::LoopY
                )
                | (
                    ManualRegionRole::Unique | ManualRegionRole::Radial,
                    RegionSampling::OneShot
                )
        )
    }

    pub fn synchronize_derived_fields(&mut self) {
        self.version = REGION_BEHAVIOR_VERSION;
        self.edge_eligibility = EdgeEligibility::for_continuity(self.continuity);
        if self.role == ManualRegionRole::Radial {
            self.radial.get_or_insert_with(|| {
                RegionBehavior::new(ManualRegionRole::Radial)
                    .radial
                    .unwrap()
            });
        } else {
            self.radial = None;
        }
    }
}

impl Default for RegionBehavior {
    fn default() -> Self {
        Self {
            version: REGION_BEHAVIOR_VERSION,
            role: ManualRegionRole::Panel,
            continuity: RegionContinuity::None,
            sampling: RegionSampling::OneShot,
            period_pixels: None,
            orientation: QuarterTurn::Zero,
            edge_eligibility: EdgeEligibility::default(),
            radial: None,
        }
    }
}

impl From<RadialParameters> for RadialMappingSettings {
    fn from(value: RadialParameters) -> Self {
        Self {
            center_x: value.center_x,
            center_y: value.center_y,
            inner_radius: value.inner_radius,
            outer_radius: value.outer_radius,
            falloff: 0.5,
            blend_width: 0.0,
            seam_blend_width: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionMapping {
    pub projection: Projection,
    /// `None` is the legacy meaning: preserve the persisted projection as authored.
    /// New fixed-template regions use `Some(Unplaced)` so template atlas rectangles
    /// can never be interpreted as raw source-image crops.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_crop_intent: Option<SourceCropIntent>,
    pub warps: Vec<WarpOperation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radial: Option<RadialMappingSettings>,
    pub transform: MappingTransform,
    pub address_mode: AddressMode,
    pub sampling: SamplingPolicy,
    #[serde(default)]
    pub behavior: RegionBehavior,
}

/// Global source framing applied before a region's own content mapping.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetFraming {
    pub projection: Projection,
    pub address_mode: AddressMode,
}

impl Default for SheetFraming {
    fn default() -> Self {
        Self {
            projection: Projection::default(),
            address_mode: AddressMode::Clamp,
        }
    }
}

impl Default for RegionMapping {
    fn default() -> Self {
        Self {
            projection: Projection::default(),
            source_crop_intent: None,
            warps: Vec::new(),
            radial: None,
            transform: MappingTransform::default(),
            address_mode: AddressMode::Clamp,
            sampling: SamplingPolicy::default(),
            behavior: RegionBehavior::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariationSettings {
    pub seed: u64,
    pub offset: [f64; 2],
}

impl Default for VariationSettings {
    fn default() -> Self {
        Self {
            seed: 0,
            offset: [0.0, 0.0],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlendPolicy {
    pub mode: SourceBlend,
    pub opacity: f64,
}

impl Default for BlendPolicy {
    fn default() -> Self {
        Self {
            mode: SourceBlend::Replace,
            opacity: 1.0,
        }
    }
}

/// Exactly one instance is keyed by the same `region_id` in `TrimSheetDocument::region_bindings`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionBinding {
    pub region_id: RegionId,
    pub content: ContentReference,
    pub mapping: RegionMapping,
    pub variation: VariationSettings,
    pub blend: BlendPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionOrientation {
    Horizontal,
    Vertical,
    Unspecified,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UvFitKind {
    Rectangular,
    Strip,
    Radial,
    Unique,
    Cap,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FitAxis {
    None,
    Horizontal,
    Vertical,
    Automatic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuarterTurn {
    Zero,
    Ninety,
    OneEighty,
    TwoSeventy,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UvFitPolicy {
    pub kind: UvFitKind,
    pub fit_axis: FitAxis,
    pub keep_proportion: bool,
    pub allowed_rotations: Vec<QuarterTurn>,
    pub mirror_allowed: bool,
    pub world_size_meters: [f64; 2],
    #[serde(default)]
    pub classification_tags: Vec<String>,
}

/// The single authoritative topology and profile record for one stable region.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionDefinition {
    pub id: RegionId,
    pub display_name: String,
    pub id_color: IdColor,
    pub allocation_rect: CanonicalRect,
    pub hotspot_rect: CanonicalRect,
    pub role: TemplateSlotRole,
    pub orientation: RegionOrientation,
    pub uv_fit: UvFitPolicy,
    pub structural_profile: StructuralProfile,
    pub material_group: String,
    pub weathering_group: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radial_parameters: Option<RadialParameters>,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid_rect: Option<GridRect>,
}

pub const AUTHORED_LAYOUT_PRESET_SCHEMA_VERSION: u16 = 1;

/// A reusable layout asset contains topology and semantic defaults only. Project content
/// references deliberately remain in `RegionBinding` on the instantiated document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthoredLayoutPresetRegion {
    pub preset_region_key: String,
    pub display_name: String,
    pub grid_rect: GridRect,
    pub role: TemplateSlotRole,
    pub orientation: RegionOrientation,
    pub uv_fit: UvFitPolicy,
    pub structural_profile: StructuralProfile,
    #[serde(default)]
    pub default_behavior: RegionBehavior,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthoredLayoutPreset {
    pub preset_id: String,
    pub schema_version: u16,
    pub name: String,
    pub logical_grid: LogicalGridSpec,
    pub canonical_aspect: [u32; 2],
    pub regions: Vec<AuthoredLayoutPresetRegion>,
    pub provenance: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TopologyKind {
    StandardTemplate,
    GeneratedTemplate,
    CustomTemplate,
    CustomAtlas,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopologySnapshot {
    pub schema_version: u32,
    pub canonical_size: PixelSize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<TemplateSnapshot>,
}

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DocumentHash(pub [u8; 32]);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionTopologyHashInput {
    pub id: RegionId,
    pub grid_rect: Option<GridRect>,
    pub id_color: IdColor,
    pub allocation_rect: CanonicalRect,
    pub hotspot_rect: CanonicalRect,
    pub role: TemplateSlotRole,
    pub orientation: RegionOrientation,
    pub uv_fit: UvFitPolicy,
    pub structural_profile: StructuralProfile,
    pub material_group: String,
    pub variation_group: String,
    pub radial_parameters: Option<RadialParameters>,
    pub enabled: bool,
}

/// Deliberately excludes source content, crop/projection, mapping transforms, warps, profiles,
/// decorations, treatments, and render/map appearance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopologyHashInputs {
    pub kind: TopologyKind,
    pub compatibility_key: String,
    pub canonical_size: PixelSize,
    pub regions: Vec<RegionTopologyHashInput>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedTopology {
    pub kind: TopologyKind,
    pub snapshot: TopologySnapshot,
    pub topology_hash: DocumentHash,
    pub compatibility_key: String,
    pub regions: Vec<RegionDefinition>,
}

impl AcceptedTopology {
    /// Pins a snapshot and computes its compatibility hash from topology-only inputs.
    pub fn new(
        kind: TopologyKind,
        snapshot: TopologySnapshot,
        compatibility_key: String,
        regions: Vec<RegionDefinition>,
    ) -> Result<Self, TrimSheetDocumentError> {
        let mut topology = Self {
            kind,
            snapshot,
            topology_hash: DocumentHash([0; 32]),
            compatibility_key,
            regions,
        };
        topology.topology_hash = hash_serializable(&topology.topology_hash_inputs())?;
        Ok(topology)
    }

    #[must_use]
    pub fn topology_hash_inputs(&self) -> TopologyHashInputs {
        TopologyHashInputs {
            kind: self.kind,
            compatibility_key: self.compatibility_key.clone(),
            canonical_size: self.snapshot.canonical_size,
            regions: self
                .regions
                .iter()
                .map(|region| RegionTopologyHashInput {
                    id: region.id,
                    grid_rect: region.grid_rect,
                    id_color: region.id_color,
                    allocation_rect: region.allocation_rect,
                    hotspot_rect: region.hotspot_rect,
                    role: region.role,
                    orientation: region.orientation,
                    uv_fit: region.uv_fit.clone(),
                    structural_profile: region.structural_profile,
                    material_group: region.material_group.clone(),
                    variation_group: region.weathering_group.clone(),
                    radial_parameters: region.radial_parameters,
                    enabled: region.enabled,
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratorProvenance {
    pub generator_id: String,
    pub generator_version: u16,
    pub recipe_version: u16,
    pub recipe_hash: DocumentHash,
    pub seed: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelBitDepth {
    Eight,
    #[default]
    Sixteen,
    ThirtyTwoFloat,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelRenderPolicy {
    pub enabled: bool,
    pub bit_depth: ChannelBitDepth,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderSettings {
    pub output_size: PixelSize,
    /// Owning-edge dilation reserved inside each region allocation, expressed at the
    /// authoritative output resolution. Preview profiles derive their own scaled value.
    #[serde(default)]
    pub atlas_padding_px: u32,
    pub renderer_version: String,
    pub channels: BTreeMap<Channel, ChannelRenderPolicy>,
}

impl Default for RenderSettings {
    fn default() -> Self {
        let channels = [
            Channel::BaseColor,
            Channel::Normal,
            Channel::Height,
            Channel::Roughness,
            Channel::Metallic,
            Channel::AmbientOcclusion,
            Channel::RegionId,
            Channel::MaterialId,
        ]
        .into_iter()
        .map(|channel| {
            (
                channel,
                ChannelRenderPolicy {
                    enabled: true,
                    bit_depth: if matches!(
                        channel,
                        Channel::BaseColor
                            | Channel::Normal
                            | Channel::RegionId
                            | Channel::MaterialId
                    ) {
                        ChannelBitDepth::Eight
                    } else {
                        ChannelBitDepth::Sixteen
                    },
                },
            )
        })
        .collect();
        Self {
            output_size: PixelSize {
                width: 2_048,
                height: 2_048,
            },
            atlas_padding_px: 0,
            renderer_version: "1".into(),
            channels,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TreatmentParameter {
    Amount,
    Contrast,
    Roughness,
    Scale,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TreatmentLayer {
    pub id: LayerId,
    pub version: u16,
    pub enabled: bool,
    pub recipe_id: String,
    pub recipe_version: u16,
    pub seed: u64,
    pub parameters: BTreeMap<TreatmentParameter, f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionAppearanceHashInput {
    pub id: RegionId,
    pub structural_profile: StructuralProfile,
    pub material_group: String,
    pub weathering_group: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceHashInputs {
    pub topology_hash: DocumentHash,
    pub source_frame: Option<SourceFrame>,
    pub source_overrides: BTreeMap<RegionId, RegionSourceOverride>,
    pub materials: Vec<MaterialSourceSet>,
    pub patches: Vec<Patch>,
    pub procedural_materials: Vec<ProceduralMaterial>,
    pub region_appearance: Vec<RegionAppearanceHashInput>,
    pub region_bindings: BTreeMap<RegionId, RegionBinding>,
    pub decorations: Vec<DecorationBinding>,
    pub treatments: Vec<TreatmentLayer>,
    pub sheet_framing: SheetFraming,
    pub render_settings: RenderSettings,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangeClassification {
    Topology,
    Appearance,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrimSheetChange {
    AcceptedRegionGeometry,
    AcceptedHotspotGeometry,
    RegionPopulation,
    RegionOrder,
    UvFitMetadata,
    SourceContent,
    RegionContent,
    RegionMapping,
    StructuralProfile,
    Decoration,
    Treatment,
    RenderSettings,
}

impl TrimSheetChange {
    #[must_use]
    pub const fn classification(self) -> ChangeClassification {
        match self {
            Self::AcceptedRegionGeometry
            | Self::AcceptedHotspotGeometry
            | Self::RegionPopulation
            | Self::RegionOrder
            | Self::UvFitMetadata => ChangeClassification::Topology,
            Self::SourceContent
            | Self::RegionContent
            | Self::RegionMapping
            | Self::StructuralProfile
            | Self::Decoration
            | Self::Treatment
            | Self::RenderSettings => ChangeClassification::Appearance,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrimSheetDocument {
    pub id: TrimSheetId,
    pub document_revision: u64,
    pub topology_revision: u64,
    pub appearance_revision: u64,
    pub topology: AcceptedTopology,
    pub primary_material: Option<SourceSetId>,
    pub materials: Vec<MaterialSourceSet>,
    pub patches: Vec<Patch>,
    pub procedural_materials: Vec<ProceduralMaterial>,
    pub region_bindings: BTreeMap<RegionId, RegionBinding>,
    pub decorations: Vec<DecorationBinding>,
    pub treatments: Vec<TreatmentLayer>,
    #[serde(default)]
    pub sheet_framing: SheetFraming,
    pub render_settings: RenderSettings,
    pub generator_provenance: Option<GeneratorProvenance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_frame: Option<SourceFrame>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub source_overrides: BTreeMap<RegionId, RegionSourceOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logical_grid: Option<LogicalGridSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partition_provenance: Option<PartitionProvenance>,
    /// The exact applied asset snapshot. Reopening never consults a mutable preset library.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authored_layout_preset: Option<AuthoredLayoutPreset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authored_layout_instance_id: Option<String>,
}

impl TrimSheetDocument {
    #[must_use]
    pub fn topology_hash_inputs(&self) -> TopologyHashInputs {
        self.topology.topology_hash_inputs()
    }

    #[must_use]
    pub fn appearance_hash_inputs(&self) -> AppearanceHashInputs {
        let mut materials = self.materials.clone();
        materials.sort_by_key(|material| material.id);
        for material in &mut materials {
            material.maps.sort_by_key(|map| map.kind);
        }
        let mut patches = self.patches.clone();
        patches.sort_by_key(|patch| patch.id);
        let mut procedural_materials = self.procedural_materials.clone();
        procedural_materials.sort_by_key(|material| material.id);
        AppearanceHashInputs {
            topology_hash: self.topology.topology_hash,
            source_frame: self.source_frame.clone(),
            source_overrides: self.source_overrides.clone(),
            materials,
            patches,
            procedural_materials,
            region_appearance: self
                .topology
                .regions
                .iter()
                .map(|region| RegionAppearanceHashInput {
                    id: region.id,
                    structural_profile: region.structural_profile,
                    material_group: region.material_group.clone(),
                    weathering_group: region.weathering_group.clone(),
                })
                .collect(),
            region_bindings: self.region_bindings.clone(),
            decorations: self.decorations.clone(),
            treatments: self.treatments.clone(),
            sheet_framing: self.sheet_framing.clone(),
            render_settings: self.render_settings.clone(),
        }
    }

    /// Computes the deterministic hash of every current compiler appearance input.
    pub fn appearance_hash(&self) -> Result<DocumentHash, TrimSheetDocumentError> {
        hash_serializable(&self.appearance_hash_inputs())
    }

    /// Validates identity, geometry, exact binding cardinality, content references, source
    /// geometry, finite/bounded mapping programs, and the stored topology hash.
    pub fn validate(&self) -> Result<(), TrimSheetDocumentError> {
        if self.topology.snapshot.schema_version == 0
            || !self.topology.snapshot.canonical_size.is_nonzero()
        {
            return Err(TrimSheetDocumentError::InvalidCanonicalSize);
        }
        if self.topology.compatibility_key.trim().is_empty() {
            return Err(TrimSheetDocumentError::InvalidCompatibilityKey);
        }
        if self.topology_revision > self.document_revision
            || self.appearance_revision > self.document_revision
        {
            return Err(TrimSheetDocumentError::InvalidRevisions);
        }
        if let Some(frame) = &self.source_frame {
            if frame.schema_version == 0
                || frame.oriented_dimensions.width == 0
                || frame.oriented_dimensions.height == 0
                || frame.output_aspect.contains(&0)
                || !valid_normalized_rect(frame.bounds)
                || frame.identity != frame.compute_identity()
            {
                return Err(TrimSheetDocumentError::InvalidSourceFrame);
            }
            if !aspect_matches(
                frame.bounds.width.get() * f64::from(frame.oriented_dimensions.width),
                frame.bounds.height.get() * f64::from(frame.oriented_dimensions.height),
                f64::from(frame.output_aspect[0]),
                f64::from(frame.output_aspect[1]),
            ) {
                return Err(TrimSheetDocumentError::InvalidSourceFrameAspect);
            }
            for (region_id, value) in &self.source_overrides {
                let Some(region) = self
                    .topology
                    .regions
                    .iter()
                    .find(|region| region.id == *region_id)
                else {
                    return Err(TrimSheetDocumentError::InvalidSourceOverride(*region_id));
                };
                if value.schema_version == 0
                    || !valid_normalized_rect(value.source_bounds)
                    || value.identity != value.compute_identity()
                {
                    return Err(TrimSheetDocumentError::InvalidSourceOverride(*region_id));
                }
                if !aspect_matches(
                    value.source_bounds.width.get() * f64::from(frame.oriented_dimensions.width),
                    value.source_bounds.height.get() * f64::from(frame.oriented_dimensions.height),
                    f64::from(region.allocation_rect.width),
                    f64::from(region.allocation_rect.height),
                ) {
                    return Err(TrimSheetDocumentError::InvalidSourceOverrideAspect(
                        *region_id,
                    ));
                }
            }
        } else if let Some(region_id) = self.source_overrides.keys().next() {
            return Err(TrimSheetDocumentError::InvalidSourceOverride(*region_id));
        }
        let canonical_size = self.topology.snapshot.canonical_size;
        let mut region_ids = BTreeSet::new();
        let mut colors = BTreeSet::new();
        for region in &self.topology.regions {
            if !region_ids.insert(region.id) {
                return Err(TrimSheetDocumentError::DuplicateRegionId(region.id));
            }
            if !region.id_color.is_valid() || !colors.insert(region.id_color) {
                return Err(TrimSheetDocumentError::DuplicateIdColor(region.id_color));
            }
            if region.display_name.trim().is_empty()
                || region.material_group.trim().is_empty()
                || region.weathering_group.trim().is_empty()
            {
                return Err(TrimSheetDocumentError::InvalidRegionMetadata(region.id));
            }
            if !valid_rect(region.allocation_rect, canonical_size) {
                return Err(TrimSheetDocumentError::InvalidAllocationRect(region.id));
            }
            if !rect_contains(region.allocation_rect, region.hotspot_rect) {
                return Err(TrimSheetDocumentError::InvalidHotspotRect(region.id));
            }
            if region.uv_fit.allowed_rotations.is_empty()
                || region
                    .uv_fit
                    .world_size_meters
                    .iter()
                    .any(|value| !value.is_finite() || *value <= 0.0)
            {
                return Err(TrimSheetDocumentError::InvalidUvFit(region.id));
            }
        }
        for (index, region) in self.topology.regions.iter().enumerate() {
            if self.topology.regions[index + 1..]
                .iter()
                .any(|other| rects_overlap(region.allocation_rect, other.allocation_rect))
            {
                return Err(TrimSheetDocumentError::OverlappingAllocationRect(region.id));
            }
        }
        let expected_hash = hash_serializable(&self.topology.topology_hash_inputs())?;
        if expected_hash != self.topology.topology_hash {
            return Err(TrimSheetDocumentError::TopologyHashMismatch);
        }
        if matches!(
            self.topology.kind,
            TopologyKind::StandardTemplate | TopologyKind::CustomTemplate
        ) {
            let snapshot = self
                .topology
                .snapshot
                .template
                .as_ref()
                .ok_or(TrimSheetDocumentError::InvalidTemplateSnapshot)?;
            let template: TemplateDefinition = serde_json::from_str(&snapshot.snapshot_json)
                .map_err(|_| TrimSheetDocumentError::InvalidTemplateSnapshot)?;
            let canonical_snapshot = template
                .snapshot()
                .map_err(|_| TrimSheetDocumentError::InvalidTemplateSnapshot)?;
            if &canonical_snapshot != snapshot
                || self.topology.compatibility_key != template.identity.compatibility_key
                || self.topology.regions != regions_from_template(&template)?
            {
                return Err(TrimSheetDocumentError::TemplateTopologyMutation);
            }
            if self.topology.kind == TopologyKind::StandardTemplate {
                let registry = TemplateRegistry::built_in()
                    .map_err(|_| TrimSheetDocumentError::InvalidTemplateSnapshot)?;
                let built_in = registry
                    .get(
                        &template.identity.template_id,
                        &template.identity.template_version,
                    )
                    .ok_or(TrimSheetDocumentError::StandardTemplateRegistryMismatch)?;
                if built_in != &template {
                    return Err(TrimSheetDocumentError::StandardTemplateRegistryMismatch);
                }
            }
        }

        let mut material_ids = BTreeSet::new();
        for material in &self.materials {
            if !material_ids.insert(material.id) {
                return Err(TrimSheetDocumentError::DuplicateMaterialId(material.id));
            }
            if material.name.trim().is_empty() {
                return Err(TrimSheetDocumentError::InvalidMaterial(material.id));
            }
            let mut map_kinds = BTreeSet::new();
            if material
                .maps
                .iter()
                .any(|map| !map_kinds.insert(map.kind) || !valid_sha256(&map.sha256))
            {
                return Err(TrimSheetDocumentError::InvalidMaterial(material.id));
            }
        }
        if let Some(primary) = self.primary_material
            && !material_ids.contains(&primary)
        {
            return Err(TrimSheetDocumentError::MissingPrimaryMaterial(primary));
        }

        let mut patch_ids = BTreeSet::new();
        for patch in &self.patches {
            if !patch_ids.insert(patch.id) {
                return Err(TrimSheetDocumentError::DuplicatePatchId(patch.id));
            }
            if !patch.has_valid_metadata() || !valid_quad(patch.geometry.corners) {
                return Err(TrimSheetDocumentError::InvalidPatchGeometry(patch.id));
            }
        }

        let mut procedural_ids = BTreeSet::new();
        for material in &self.procedural_materials {
            if !procedural_ids.insert(material.id) {
                return Err(TrimSheetDocumentError::DuplicateProceduralId(material.id));
            }
            if material.recipe_id.trim().is_empty() || material.recipe_version == 0 {
                return Err(TrimSheetDocumentError::InvalidProcedural(material.id));
            }
        }

        for region in &self.topology.regions {
            let binding = self
                .region_bindings
                .get(&region.id)
                .ok_or(TrimSheetDocumentError::MissingRegionBinding(region.id))?;
            if binding.region_id != region.id {
                return Err(TrimSheetDocumentError::BindingRegionMismatch {
                    key: region.id,
                    value: binding.region_id,
                });
            }
            match &binding.content {
                ContentReference::InheritPrimaryMaterial => {
                    if self.primary_material.is_none() {
                        return Err(TrimSheetDocumentError::MissingInheritedContent(region.id));
                    }
                }
                ContentReference::MaterialSource(id) => {
                    if !material_ids.contains(id) {
                        return Err(TrimSheetDocumentError::MissingMaterialContent {
                            region_id: region.id,
                            material_id: *id,
                        });
                    }
                }
                ContentReference::Patch(id) => {
                    if !patch_ids.contains(id) {
                        return Err(TrimSheetDocumentError::MissingPatchContent {
                            region_id: region.id,
                            patch_id: *id,
                        });
                    }
                }
                ContentReference::Solid(values) => validate_solid(region.id, values)?,
                ContentReference::Procedural(id) => {
                    if !procedural_ids.contains(id) {
                        return Err(TrimSheetDocumentError::MissingProceduralContent {
                            region_id: region.id,
                            procedural_id: *id,
                        });
                    }
                }
            }
            validate_mapping(region.id, &binding.mapping)?;
            if binding
                .variation
                .offset
                .iter()
                .any(|value| !value.is_finite() || value.abs() > MAX_MAPPING_MAGNITUDE)
                || !binding.blend.opacity.is_finite()
                || !(0.0..=1.0).contains(&binding.blend.opacity)
            {
                return Err(TrimSheetDocumentError::InvalidNumericValue(region.id));
            }
        }
        if let Some(extra) = self
            .region_bindings
            .keys()
            .find(|region_id| !region_ids.contains(region_id))
        {
            return Err(TrimSheetDocumentError::UnexpectedRegionBinding(*extra));
        }

        let mut treatment_ids = BTreeSet::new();
        for treatment in &self.treatments {
            if !treatment_ids.insert(treatment.id)
                || treatment.version == 0
                || treatment.recipe_version == 0
                || treatment.recipe_id.trim().is_empty()
                || treatment
                    .parameters
                    .values()
                    .any(|value| !value.is_finite() || value.abs() > MAX_MAPPING_MAGNITUDE)
            {
                return Err(TrimSheetDocumentError::InvalidTreatment(treatment.id));
            }
        }
        if !self.render_settings.output_size.is_nonzero()
            || self.render_settings.atlas_padding_px > 4_096
            || self.render_settings.renderer_version.trim().is_empty()
            || self.render_settings.channels.is_empty()
        {
            return Err(TrimSheetDocumentError::InvalidRenderSettings);
        }
        if let Some(provenance) = &self.generator_provenance
            && (provenance.generator_id.trim().is_empty()
                || provenance.generator_version == 0
                || provenance.recipe_version == 0)
        {
            return Err(TrimSheetDocumentError::InvalidGeneratorProvenance);
        }
        Ok(())
    }

    /// Creates a document directly from an accepted, version-pinned template definition.
    pub fn from_template(
        id: TrimSheetId,
        template: &TemplateDefinition,
        materials: Vec<MaterialSourceSet>,
        patches: Vec<Patch>,
    ) -> Result<Self, TrimSheetDocumentError> {
        let registry = TemplateRegistry::built_in()
            .map_err(|_| TrimSheetDocumentError::InvalidTemplateSnapshot)?;
        let built_in = registry
            .get(
                &template.identity.template_id,
                &template.identity.template_version,
            )
            .ok_or(TrimSheetDocumentError::StandardTemplateRegistryMismatch)?;
        if built_in != template {
            return Err(TrimSheetDocumentError::StandardTemplateRegistryMismatch);
        }
        Self::from_pinned_template(
            id,
            template,
            materials,
            patches,
            TopologyKind::StandardTemplate,
        )
    }

    /// Creates the primary source-frame document. The accepted partition is persisted up front;
    /// compilation consumes it and never regenerates topology implicitly.
    pub fn from_source_frame(
        id: TrimSheetId,
        source_frame: SourceFrame,
        recipe: PartitionRecipe,
        output_size: PixelSize,
        materials: Vec<MaterialSourceSet>,
        patches: Vec<Patch>,
    ) -> Result<Self, TrimSheetDocumentError> {
        let partitions = crate::generate_partition(&recipe)
            .map_err(TrimSheetDocumentError::InvalidSourcePartition)?;
        let output_x = crate::resolve_boundaries(0, output_size.width, recipe.grid.width);
        let output_y = crate::resolve_boundaries(0, output_size.height, recipe.grid.height);
        let regions = partitions
            .iter()
            .map(|partition| {
                let rect = CanonicalRect {
                    x: output_x[partition.grid_rect.x as usize],
                    y: output_y[partition.grid_rect.y as usize],
                    width: output_x[(partition.grid_rect.x + partition.grid_rect.width) as usize]
                        - output_x[partition.grid_rect.x as usize],
                    height: output_y[(partition.grid_rect.y + partition.grid_rect.height) as usize]
                        - output_y[partition.grid_rect.y as usize],
                };
                let orientation = if rect.width > rect.height {
                    RegionOrientation::Horizontal
                } else if rect.height > rect.width {
                    RegionOrientation::Vertical
                } else {
                    RegionOrientation::Unspecified
                };
                RegionDefinition {
                    id: partition.id,
                    display_name: format!(
                        "Region {:03}",
                        partition.grid_rect.y * recipe.grid.width + partition.grid_rect.x
                    ),
                    id_color: IdColor::for_region(partition.id),
                    allocation_rect: rect,
                    hotspot_rect: rect,
                    role: TemplateSlotRole::Planar,
                    orientation,
                    uv_fit: UvFitPolicy {
                        kind: UvFitKind::Rectangular,
                        fit_axis: FitAxis::Automatic,
                        keep_proportion: true,
                        allowed_rotations: vec![QuarterTurn::Zero],
                        mirror_allowed: false,
                        world_size_meters: [
                            f64::from(rect.width.max(1)),
                            f64::from(rect.height.max(1)),
                        ],
                        classification_tags: vec![
                            "SOURCE_FRAME".into(),
                            format!("SOURCE_FRAME_{:?}", partition.family).to_uppercase(),
                        ],
                    },
                    structural_profile: StructuralProfile::Flat,
                    material_group: "primary".into(),
                    weathering_group: "neutral".into(),
                    radial_parameters: None,
                    enabled: true,
                    grid_rect: Some(partition.grid_rect),
                }
            })
            .collect::<Vec<_>>();
        let topology = AcceptedTopology::new(
            TopologyKind::CustomAtlas,
            TopologySnapshot {
                schema_version: TRIM_SHEET_DOCUMENT_SCHEMA_VERSION,
                canonical_size: output_size,
                template: None,
            },
            format!(
                "source-frame:{}:{}",
                source_frame
                    .identity
                    .0
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
                recipe
                    .hash()
                    .0
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            ),
            regions,
        )?;
        let provenance = PartitionProvenance {
            schema_version: crate::PARTITION_RECIPE_SCHEMA_VERSION,
            recipe: recipe.clone(),
            recipe_hash: recipe.hash(),
            accepted_region_ids: topology.regions.iter().map(|region| region.id).collect(),
            tree: partitions
                .iter()
                .enumerate()
                .map(|(ordinal, region)| crate::PartitionTreeNode {
                    grid_rect: region.grid_rect,
                    family: region.family,
                    ordinal: ordinal as u32,
                    lineage: region.lineage,
                })
                .collect(),
            topology_hash: topology.topology_hash,
        };
        let document = Self {
            id,
            document_revision: 1,
            topology_revision: 1,
            appearance_revision: 1,
            topology,
            primary_material: materials.first().map(|material| material.id),
            materials,
            patches,
            procedural_materials: Vec::new(),
            region_bindings: partitions
                .iter()
                .map(|partition| {
                    (
                        partition.id,
                        RegionBinding {
                            region_id: partition.id,
                            content: ContentReference::InheritPrimaryMaterial,
                            mapping: RegionMapping::default(),
                            variation: VariationSettings::default(),
                            blend: BlendPolicy::default(),
                        },
                    )
                })
                .collect(),
            decorations: Vec::new(),
            treatments: Vec::new(),
            sheet_framing: SheetFraming::default(),
            render_settings: RenderSettings {
                output_size,
                ..RenderSettings::default()
            },
            generator_provenance: None,
            source_frame: Some(source_frame),
            source_overrides: BTreeMap::new(),
            logical_grid: Some(recipe.grid),
            partition_provenance: Some(provenance),
            authored_layout_preset: None,
            authored_layout_instance_id: None,
        };
        document.validate()?;
        Ok(document)
    }

    /// Instantiates a versioned authored snapshot without consulting the partition generator.
    pub fn from_authored_layout_preset(
        id: TrimSheetId,
        source_frame: SourceFrame,
        preset: AuthoredLayoutPreset,
        instance_id: String,
        output_size: PixelSize,
        materials: Vec<MaterialSourceSet>,
        patches: Vec<Patch>,
    ) -> Result<Self, TrimSheetDocumentError> {
        validate_authored_layout_preset(&preset)?;
        let mut regions = Vec::with_capacity(preset.regions.len());
        let mut bindings = BTreeMap::new();
        for authored in &preset.regions {
            let region_id = deterministic_region_id(
                &format!("{}:{}", preset.preset_id, instance_id),
                &authored.preset_region_key,
            );
            let allocation =
                grid_rect_to_output(authored.grid_rect, preset.logical_grid, output_size);
            regions.push(RegionDefinition {
                id: region_id,
                display_name: authored.display_name.clone(),
                id_color: IdColor::for_region(region_id),
                allocation_rect: allocation,
                hotspot_rect: allocation,
                role: authored.role,
                orientation: authored.orientation,
                uv_fit: authored.uv_fit.clone(),
                structural_profile: authored.structural_profile,
                material_group: "primary".into(),
                weathering_group: "neutral".into(),
                radial_parameters: None,
                enabled: true,
                grid_rect: Some(authored.grid_rect),
            });
            bindings.insert(
                region_id,
                RegionBinding {
                    region_id,
                    content: ContentReference::InheritPrimaryMaterial,
                    mapping: RegionMapping {
                        radial: authored.default_behavior.radial,
                        address_mode: if authored.default_behavior.sampling
                            == RegionSampling::OneShot
                        {
                            AddressMode::Clamp
                        } else {
                            AddressMode::Repeat
                        },
                        behavior: authored.default_behavior.clone(),
                        ..RegionMapping::default()
                    },
                    variation: VariationSettings::default(),
                    blend: BlendPolicy::default(),
                },
            );
        }
        let compatibility_key = format!("authored-layout:{}:{}", preset.preset_id, instance_id);
        let topology = AcceptedTopology::new(
            TopologyKind::CustomAtlas,
            TopologySnapshot {
                schema_version: TRIM_SHEET_DOCUMENT_SCHEMA_VERSION,
                canonical_size: output_size,
                template: None,
            },
            compatibility_key,
            regions,
        )?;
        // Direct editing still uses the old tree journal as an implementation detail. It is a
        // snapshot of authored rectangles and is never evaluated as a generator recipe.
        let recipe =
            PartitionRecipe::default_for(preset.logical_grid, preset.regions.len() as u32, 0);
        let tree = preset
            .regions
            .iter()
            .enumerate()
            .map(|(ordinal, region)| PartitionTreeNode {
                grid_rect: region.grid_rect,
                family: PartitionFamily::Remainder,
                ordinal: ordinal as u32,
                lineage: crate::PartitionLineage::default(),
            })
            .collect::<Vec<_>>();
        let partition_provenance = PartitionProvenance {
            schema_version: crate::PARTITION_RECIPE_SCHEMA_VERSION,
            recipe: recipe.clone(),
            recipe_hash: recipe.hash(),
            accepted_region_ids: topology.regions.iter().map(|region| region.id).collect(),
            tree,
            topology_hash: topology.topology_hash,
        };
        let document = Self {
            id,
            document_revision: 1,
            topology_revision: 1,
            appearance_revision: 1,
            topology,
            primary_material: materials.first().map(|material| material.id),
            materials,
            patches,
            procedural_materials: Vec::new(),
            region_bindings: bindings,
            decorations: Vec::new(),
            treatments: Vec::new(),
            sheet_framing: SheetFraming::default(),
            render_settings: RenderSettings {
                output_size,
                ..RenderSettings::default()
            },
            generator_provenance: None,
            source_frame: Some(source_frame),
            source_overrides: BTreeMap::new(),
            logical_grid: Some(preset.logical_grid),
            partition_provenance: Some(partition_provenance),
            authored_layout_preset: Some(preset),
            authored_layout_instance_id: Some(instance_id),
        };
        document.validate()?;
        Ok(document)
    }

    pub fn apply_authored_layout_preset(
        &self,
        preset: AuthoredLayoutPreset,
        instance_id: String,
    ) -> Result<Self, TrimSheetDocumentError> {
        let mut next = Self::from_authored_layout_preset(
            self.id,
            self.source_frame
                .clone()
                .ok_or(TrimSheetDocumentError::InvalidSourceFrame)?,
            preset,
            instance_id,
            self.render_settings.output_size,
            self.materials.clone(),
            self.patches.clone(),
        )?;
        next.document_revision = self.document_revision.saturating_add(1);
        next.topology_revision = self.topology_revision.saturating_add(1);
        next.appearance_revision = next.document_revision;
        next.primary_material = self.primary_material;
        next.validate()?;
        Ok(next)
    }

    fn from_pinned_template(
        id: TrimSheetId,
        template: &TemplateDefinition,
        materials: Vec<MaterialSourceSet>,
        patches: Vec<Patch>,
        kind: TopologyKind,
    ) -> Result<Self, TrimSheetDocumentError> {
        let template_snapshot = template
            .snapshot()
            .map_err(|_| TrimSheetDocumentError::InvalidTemplateSnapshot)?;
        let regions = regions_from_template(template)?;
        let mut bindings = BTreeMap::new();
        for slot_key in &template.stable_order {
            let slot = template
                .slots
                .iter()
                .find(|slot| &slot.slot_key == slot_key)
                .ok_or(TrimSheetDocumentError::InvalidTemplateSnapshot)?;
            let region_id = deterministic_region_id(
                &template.identity.compatibility_key,
                &slot.compatibility_key,
            );
            let behavior_role = match (slot.role, slot.allocation.height > slot.allocation.width) {
                (TemplateSlotRole::RepeatingStrip, true) => ManualRegionRole::VerticalStrip,
                (TemplateSlotRole::RepeatingStrip, _) => ManualRegionRole::HorizontalStrip,
                (TemplateSlotRole::UniqueDetail | TemplateSlotRole::TrimCap, _) => {
                    ManualRegionRole::Unique
                }
                (TemplateSlotRole::Radial, _) => ManualRegionRole::Radial,
                _ => ManualRegionRole::Panel,
            };
            let mut behavior = RegionBehavior::new(behavior_role);
            behavior.radial = slot.radial_parameters.map(Into::into).or(behavior.radial);
            behavior.synchronize_derived_fields();
            bindings.insert(
                region_id,
                RegionBinding {
                    region_id,
                    content: ContentReference::InheritPrimaryMaterial,
                    mapping: RegionMapping {
                        radial: behavior.radial,
                        behavior,
                        ..template_region_mapping(slot.source_mapping)
                    },
                    variation: VariationSettings::default(),
                    blend: BlendPolicy::default(),
                },
            );
        }

        let topology = AcceptedTopology::new(
            kind,
            TopologySnapshot {
                schema_version: TRIM_SHEET_DOCUMENT_SCHEMA_VERSION,
                canonical_size: PixelSize {
                    width: template.canonical_width,
                    height: template.canonical_height,
                },
                template: Some(template_snapshot.clone()),
            },
            template_snapshot.identity.compatibility_key,
            regions,
        )?;
        let document = Self {
            id,
            document_revision: 1,
            topology_revision: 1,
            appearance_revision: 1,
            topology,
            primary_material: materials.first().map(|material| material.id),
            materials,
            patches,
            procedural_materials: Vec::new(),
            region_bindings: bindings,
            decorations: Vec::new(),
            treatments: Vec::new(),
            sheet_framing: SheetFraming::default(),
            render_settings: RenderSettings::default(),
            generator_provenance: None,
            source_overrides: BTreeMap::new(),
            source_frame: None,
            logical_grid: None,
            partition_provenance: None,
            authored_layout_preset: None,
            authored_layout_instance_id: None,
        };
        document.validate()?;
        Ok(document)
    }

    /// Accepts a custom authored template only after all canonical integer rectangles are pinned.
    pub fn from_custom_template(
        id: TrimSheetId,
        template: &TemplateDefinition,
        materials: Vec<MaterialSourceSet>,
        patches: Vec<Patch>,
    ) -> Result<Self, TrimSheetDocumentError> {
        Self::from_pinned_template(
            id,
            template,
            materials,
            patches,
            TopologyKind::CustomTemplate,
        )
    }

    /// Applies one accepted command to a clone, validates it, and advances revisions exactly once.
    pub fn apply_command(
        &self,
        command: &TrimSheetDocumentCommand,
    ) -> Result<Self, TrimSheetDocumentError> {
        let mut next = self.clone();
        match command {
            TrimSheetDocumentCommand::ApplyAuthoredLayoutPreset {
                preset,
                instance_id,
            } => {
                return self.apply_authored_layout_preset(preset.clone(), instance_id.clone());
            }
            TrimSheetDocumentCommand::SetAuthoredLayoutPresetSnapshot { preset } => {
                validate_authored_layout_preset(preset)?;
                if next.logical_grid != Some(preset.logical_grid) {
                    return Err(TrimSheetDocumentError::InvalidPartitionEdit(
                        "saved preset grid does not match the current document".into(),
                    ));
                }
                let mut current = next
                    .topology
                    .regions
                    .iter()
                    .map(|region| {
                        region
                            .grid_rect
                            .map(|rect| (rect.x, rect.y, rect.width, rect.height))
                    })
                    .collect::<Vec<_>>();
                let mut saved = preset
                    .regions
                    .iter()
                    .map(|region| {
                        Some((
                            region.grid_rect.x,
                            region.grid_rect.y,
                            region.grid_rect.width,
                            region.grid_rect.height,
                        ))
                    })
                    .collect::<Vec<_>>();
                current.sort_unstable();
                saved.sort_unstable();
                if current != saved {
                    return Err(TrimSheetDocumentError::InvalidPartitionEdit(
                        "saved preset rectangles do not match the current topology".into(),
                    ));
                }
                next.authored_layout_preset = Some(preset.clone());
            }
            TrimSheetDocumentCommand::AcceptSourceFramePartition { recipe } => {
                return self.accept_source_frame_partition(recipe.clone());
            }
            TrimSheetDocumentCommand::SplitSourceFrameRegion { region_id, axis } => {
                return self.split_source_frame_region(*region_id, *axis);
            }
            TrimSheetDocumentCommand::MergeSourceFrameRegions {
                region_id,
                sibling_id,
            } => {
                return self.merge_source_frame_regions(*region_id, *sibling_id);
            }
            TrimSheetDocumentCommand::MoveSourceFrameBoundary {
                region_id,
                axis,
                coordinate,
            } => {
                return self.move_source_frame_boundary(*region_id, *axis, *coordinate);
            }
            TrimSheetDocumentCommand::DrawSourceFrameRegion { grid_rect } => {
                return self.draw_source_frame_region(*grid_rect);
            }
            TrimSheetDocumentCommand::ResizeSourceFrameRegion {
                region_id,
                grid_rect,
            } => {
                return self.resize_source_frame_region(*region_id, *grid_rect);
            }
            TrimSheetDocumentCommand::SetPrimaryMaterial { material_id } => {
                next.primary_material = Some(*material_id);
            }
            TrimSheetDocumentCommand::SetRegionContent { region_id, content } => {
                next.region_bindings
                    .get_mut(region_id)
                    .ok_or(TrimSheetDocumentError::MissingRegionBinding(*region_id))?
                    .content = content.clone();
            }
            TrimSheetDocumentCommand::SetRegionAddressMode {
                region_id,
                address_mode,
            } => {
                let mapping = &mut next
                    .region_bindings
                    .get_mut(region_id)
                    .ok_or(TrimSheetDocumentError::MissingRegionBinding(*region_id))?
                    .mapping;
                mapping.address_mode = *address_mode;
                mapping.behavior.sampling = match address_mode {
                    AddressMode::Clamp => RegionSampling::OneShot,
                    AddressMode::Repeat => RegionSampling::LoopXy,
                    AddressMode::MirroredRepeat => {
                        return Err(TrimSheetDocumentError::UnsupportedRegionBehavior(
                            *region_id,
                        ));
                    }
                };
                mapping.behavior.synchronize_derived_fields();
            }
            TrimSheetDocumentCommand::SetRegionBehavior {
                region_id,
                behavior,
            } => {
                let mut behavior = behavior.clone();
                behavior.synchronize_derived_fields();
                validate_region_behavior(*region_id, &behavior)?;
                let mapping = &mut next
                    .region_bindings
                    .get_mut(region_id)
                    .ok_or(TrimSheetDocumentError::MissingRegionBinding(*region_id))?
                    .mapping;
                mapping.address_mode = if behavior.sampling == RegionSampling::OneShot {
                    AddressMode::Clamp
                } else {
                    AddressMode::Repeat
                };
                mapping.radial = behavior.radial;
                mapping.behavior = behavior;
            }
            TrimSheetDocumentCommand::SetSheetFraming { framing } => {
                next.sheet_framing = framing.clone();
            }
            TrimSheetDocumentCommand::SetRegionProjection {
                region_id,
                projection,
            } => {
                if next.source_frame.is_some() {
                    if !next.source_overrides.contains_key(region_id) {
                        return Err(TrimSheetDocumentError::SourceCellMustBeDetached(*region_id));
                    }
                    let Projection::Crop { bounds, .. } = projection else {
                        return Err(TrimSheetDocumentError::InvalidSourceGeometry(*region_id));
                    };
                    next.source_overrides
                        .insert(*region_id, RegionSourceOverride::new(*bounds));
                }
                next.region_bindings
                    .get_mut(region_id)
                    .ok_or(TrimSheetDocumentError::MissingRegionBinding(*region_id))?
                    .mapping
                    .projection = projection.clone();
                next.region_bindings
                    .get_mut(region_id)
                    .ok_or(TrimSheetDocumentError::MissingRegionBinding(*region_id))?
                    .mapping
                    .source_crop_intent = Some(SourceCropIntent::Authored);
            }
            TrimSheetDocumentCommand::SetSourceFrame { bounds } => {
                let frame = next
                    .source_frame
                    .as_ref()
                    .ok_or(TrimSheetDocumentError::InvalidSourceFrame)?;
                next.source_frame = Some(frame.with_bounds(*bounds));
            }
            TrimSheetDocumentCommand::DetachSourceCell { region_id } => {
                let frame = next
                    .source_frame
                    .as_ref()
                    .ok_or(TrimSheetDocumentError::InvalidSourceFrame)?;
                let grid = next
                    .logical_grid
                    .ok_or(TrimSheetDocumentError::InvalidSourceFrame)?;
                let region = next
                    .topology
                    .regions
                    .iter()
                    .find(|region| region.id == *region_id)
                    .ok_or(TrimSheetDocumentError::MissingRegionBinding(*region_id))?;
                let rect = region
                    .grid_rect
                    .ok_or(TrimSheetDocumentError::InvalidSourceFrame)?;
                let bounds = source_frame_region_bounds(frame, grid, rect);
                next.source_overrides
                    .insert(*region_id, RegionSourceOverride::new(bounds));
                let binding = next
                    .region_bindings
                    .get_mut(region_id)
                    .ok_or(TrimSheetDocumentError::MissingRegionBinding(*region_id))?;
                binding.mapping.projection = Projection::Crop {
                    bounds,
                    focus: NormalizedPoint::new(
                        bounds.x.get() + bounds.width.get() * 0.5,
                        bounds.y.get() + bounds.height.get() * 0.5,
                    )
                    .expect("override focus"),
                };
                binding.mapping.source_crop_intent = Some(SourceCropIntent::Authored);
            }
            TrimSheetDocumentCommand::ResetSourceCell { region_id } => {
                next.source_overrides.remove(region_id);
                let binding = next
                    .region_bindings
                    .get_mut(region_id)
                    .ok_or(TrimSheetDocumentError::MissingRegionBinding(*region_id))?;
                reset_source_mapping_preserving_behavior(&mut binding.mapping);
            }
            TrimSheetDocumentCommand::SetRegionRadial { region_id, radial } => {
                let mapping = &mut next
                    .region_bindings
                    .get_mut(region_id)
                    .ok_or(TrimSheetDocumentError::MissingRegionBinding(*region_id))?
                    .mapping;
                if mapping.behavior.role != ManualRegionRole::Radial {
                    return Err(TrimSheetDocumentError::InvalidRadialMapping(*region_id));
                }
                mapping.radial = Some(*radial);
                mapping.behavior.radial = Some(*radial);
            }
            TrimSheetDocumentCommand::SetOutputResolution { output_size } => {
                next.render_settings.output_size = *output_size;
            }
            TrimSheetDocumentCommand::SetAtlasPadding { padding_px } => {
                next.render_settings.atlas_padding_px = *padding_px;
            }
            TrimSheetDocumentCommand::SetChannelRenderPolicy { channel, policy } => {
                next.render_settings.channels.insert(*channel, *policy);
            }
        }
        next.document_revision = next.document_revision.saturating_add(1);
        next.appearance_revision = next.document_revision;
        next.validate()?;
        Ok(next)
    }

    /// Pins one already-previewed recipe as a single topology command.  The generator is never
    /// rerun by loading or compiling: this command stores its complete recipe, leaf tree and IDs.
    pub fn accept_source_frame_partition(
        &self,
        recipe: PartitionRecipe,
    ) -> Result<Self, TrimSheetDocumentError> {
        let frame = self
            .source_frame
            .clone()
            .ok_or(TrimSheetDocumentError::InvalidSourceFrame)?;
        let mut next = Self::from_source_frame(
            self.id,
            frame,
            recipe,
            self.render_settings.output_size,
            self.materials.clone(),
            self.patches.clone(),
        )?;
        next.document_revision = self.document_revision.saturating_add(1);
        next.topology_revision = self.topology_revision.saturating_add(1);
        next.appearance_revision = next.document_revision;
        next.validate()?;
        Ok(next)
    }

    /// Splits one source-frame leaf on the logical lattice.  The existing region retains its
    /// identity; the newly created sibling receives a deterministic new identity.  Source crop
    /// overrides are cleared because partition-owned crops follow the new rectangles.
    pub fn split_source_frame_region(
        &self,
        region_id: RegionId,
        axis: PartitionAxis,
    ) -> Result<Self, TrimSheetDocumentError> {
        let mut next = self.source_frame_editable_clone()?;
        let index = next
            .topology
            .regions
            .iter()
            .position(|region| region.id == region_id)
            .ok_or(TrimSheetDocumentError::MissingRegionBinding(region_id))?;
        let rect = next.topology.regions[index]
            .grid_rect
            .ok_or(TrimSheetDocumentError::InvalidSourceFrame)?;
        let extent = match axis {
            PartitionAxis::Vertical => rect.width,
            PartitionAxis::Horizontal => rect.height,
        };
        if extent < 2 {
            return Err(TrimSheetDocumentError::InvalidPartitionEdit(
                "region is one lattice cell wide on that axis".into(),
            ));
        }
        let split = extent / 2;
        let (first, second) = match axis {
            PartitionAxis::Vertical => (
                GridRect {
                    width: split,
                    ..rect
                },
                GridRect {
                    x: rect.x + split,
                    width: rect.width - split,
                    ..rect
                },
            ),
            PartitionAxis::Horizontal => (
                GridRect {
                    height: split,
                    ..rect
                },
                GridRect {
                    y: rect.y + split,
                    height: rect.height - split,
                    ..rect
                },
            ),
        };
        let recipe = next
            .partition_provenance
            .as_ref()
            .expect("editable provenance")
            .recipe
            .clone();
        let new_id = unique_partition_region_id(
            &recipe,
            second,
            next.topology.regions.iter().map(|region| region.id),
        );
        let mut sibling = next.topology.regions[index].clone();
        sibling.id = new_id;
        sibling.id_color = IdColor::for_region(new_id);
        sibling.display_name = format!("Region {:03}", next.topology.regions.len());
        next.topology.regions[index].grid_rect = Some(first);
        sibling.grid_rect = Some(second);
        next.topology.regions.push(sibling);
        let binding = next
            .region_bindings
            .get(&region_id)
            .cloned()
            .ok_or(TrimSheetDocumentError::MissingRegionBinding(region_id))?;
        let mut sibling_binding = binding;
        sibling_binding.region_id = new_id;
        sibling_binding.mapping = RegionMapping::default();
        next.region_bindings.insert(new_id, sibling_binding);
        next.source_overrides.remove(&region_id);
        reset_source_mapping_preserving_behavior(
            &mut next
                .region_bindings
                .get_mut(&region_id)
                .expect("existing binding")
                .mapping,
        );
        next.repin_source_frame_topology()?;
        Ok(next)
    }

    /// Removes a divider only when the two selected leaves share its complete span.  The first
    /// region keeps its ID; the sibling binding/override is removed.  This makes "delete" a
    /// safe Merge/Remove Divider operation and never leaves an empty atlas cell.
    pub fn merge_source_frame_regions(
        &self,
        region_id: RegionId,
        sibling_id: RegionId,
    ) -> Result<Self, TrimSheetDocumentError> {
        if region_id == sibling_id {
            return Err(TrimSheetDocumentError::InvalidPartitionEdit(
                "choose two adjacent regions".into(),
            ));
        }
        let mut next = self.source_frame_editable_clone()?;
        let first_index = next
            .topology
            .regions
            .iter()
            .position(|region| region.id == region_id)
            .ok_or(TrimSheetDocumentError::MissingRegionBinding(region_id))?;
        let second_index = next
            .topology
            .regions
            .iter()
            .position(|region| region.id == sibling_id)
            .ok_or(TrimSheetDocumentError::MissingRegionBinding(sibling_id))?;
        let first = next.topology.regions[first_index]
            .grid_rect
            .ok_or(TrimSheetDocumentError::InvalidSourceFrame)?;
        let second = next.topology.regions[second_index]
            .grid_rect
            .ok_or(TrimSheetDocumentError::InvalidSourceFrame)?;
        let merged = mergeable_grid_rect(first, second).ok_or_else(|| {
            TrimSheetDocumentError::InvalidPartitionEdit(
                "regions must share one complete divider".into(),
            )
        })?;
        next.topology.regions[first_index].grid_rect = Some(merged);
        next.topology.regions.remove(second_index);
        next.region_bindings.remove(&sibling_id);
        next.source_overrides.remove(&region_id);
        next.source_overrides.remove(&sibling_id);
        reset_source_mapping_preserving_behavior(
            &mut next
                .region_bindings
                .get_mut(&region_id)
                .expect("existing binding")
                .mapping,
        );
        next.repin_source_frame_topology()?;
        Ok(next)
    }

    /// Moves only a full shared divider.  Both adjacent rectangles are updated atomically and
    /// retain their IDs, so undo/redo and mapping ownership remain stable.
    pub fn move_source_frame_boundary(
        &self,
        region_id: RegionId,
        axis: PartitionAxis,
        coordinate: u32,
    ) -> Result<Self, TrimSheetDocumentError> {
        let mut next = self.source_frame_editable_clone()?;
        let index = next
            .topology
            .regions
            .iter()
            .position(|region| region.id == region_id)
            .ok_or(TrimSheetDocumentError::MissingRegionBinding(region_id))?;
        let rect = next.topology.regions[index]
            .grid_rect
            .ok_or(TrimSheetDocumentError::InvalidSourceFrame)?;
        let sibling_index = next
            .topology
            .regions
            .iter()
            .enumerate()
            .find_map(|(candidate_index, candidate)| {
                if candidate_index == index {
                    return None;
                }
                let other = candidate.grid_rect?;
                let full_vertical = axis == PartitionAxis::Vertical
                    && rect.y == other.y
                    && rect.height == other.height
                    && (rect.x + rect.width == other.x || other.x + other.width == rect.x);
                let full_horizontal = axis == PartitionAxis::Horizontal
                    && rect.x == other.x
                    && rect.width == other.width
                    && (rect.y + rect.height == other.y || other.y + other.height == rect.y);
                (full_vertical || full_horizontal).then_some(candidate_index)
            })
            .ok_or_else(|| {
                TrimSheetDocumentError::InvalidPartitionEdit(
                    "this edge is not a full shared divider".into(),
                )
            })?;
        let other = next.topology.regions[sibling_index]
            .grid_rect
            .expect("source-frame region");
        let (start, end) = match axis {
            PartitionAxis::Vertical => (
                rect.x.min(other.x),
                (rect.x + rect.width).max(other.x + other.width),
            ),
            PartitionAxis::Horizontal => (
                rect.y.min(other.y),
                (rect.y + rect.height).max(other.y + other.height),
            ),
        };
        if coordinate <= start || coordinate >= end {
            return Err(TrimSheetDocumentError::InvalidPartitionEdit(
                "boundary must remain inside its adjacent regions".into(),
            ));
        }
        let (left_rect, right_rect) = match axis {
            PartitionAxis::Vertical if rect.x < other.x => (
                GridRect {
                    width: coordinate - rect.x,
                    ..rect
                },
                GridRect {
                    x: coordinate,
                    width: other.x + other.width - coordinate,
                    ..other
                },
            ),
            PartitionAxis::Vertical => (
                GridRect {
                    x: coordinate,
                    width: rect.x + rect.width - coordinate,
                    ..rect
                },
                GridRect {
                    width: coordinate - other.x,
                    ..other
                },
            ),
            PartitionAxis::Horizontal if rect.y < other.y => (
                GridRect {
                    height: coordinate - rect.y,
                    ..rect
                },
                GridRect {
                    y: coordinate,
                    height: other.y + other.height - coordinate,
                    ..other
                },
            ),
            PartitionAxis::Horizontal => (
                GridRect {
                    y: coordinate,
                    height: rect.y + rect.height - coordinate,
                    ..rect
                },
                GridRect {
                    height: coordinate - other.y,
                    ..other
                },
            ),
        };
        next.topology.regions[index].grid_rect = Some(left_rect);
        next.topology.regions[sibling_index].grid_rect = Some(right_rect);
        for id in [region_id, next.topology.regions[sibling_index].id] {
            next.source_overrides.remove(&id);
            let binding = next.region_bindings.get_mut(&id).expect("existing binding");
            reset_source_mapping_preserving_behavior(&mut binding.mapping);
        }
        next.repin_source_frame_topology()?;
        Ok(next)
    }

    /// Inserts one directly-authored snapped rectangle as a single atomic partition edit.
    /// Every intersected leaf is clipped into non-overlapping remainder rectangles, so the
    /// authored rectangle and remainder still form an exact cover with no texture regeneration.
    pub fn draw_source_frame_region(
        &self,
        grid_rect: GridRect,
    ) -> Result<Self, TrimSheetDocumentError> {
        let mut next = self.source_frame_editable_clone()?;
        let grid = next
            .logical_grid
            .ok_or(TrimSheetDocumentError::InvalidSourceFrame)?;
        if grid_rect.width == 0
            || grid_rect.height == 0
            || grid_rect.x.saturating_add(grid_rect.width) > grid.width
            || grid_rect.y.saturating_add(grid_rect.height) > grid.height
        {
            return Err(TrimSheetDocumentError::InvalidPartitionEdit(
                "drawn rectangle must be inside the logical grid and at least one cell wide".into(),
            ));
        }
        let recipe = next
            .partition_provenance
            .as_ref()
            .expect("editable provenance")
            .recipe
            .clone();
        let original_regions = next.topology.regions.clone();
        let mut rebuilt = Vec::with_capacity(original_regions.len() + 4);
        let mut occupied_ids = original_regions
            .iter()
            .map(|region| region.id)
            .collect::<BTreeSet<_>>();
        let mut drawn_template = None;
        let mut drawn_binding = None;

        for region in original_regions {
            let rect = region
                .grid_rect
                .ok_or(TrimSheetDocumentError::InvalidSourceFrame)?;
            let Some(intersection) = grid_rect_intersection(rect, grid_rect) else {
                rebuilt.push(region);
                continue;
            };
            drawn_template.get_or_insert_with(|| region.clone());
            drawn_binding.get_or_insert(
                next.region_bindings
                    .get(&region.id)
                    .cloned()
                    .ok_or(TrimSheetDocumentError::MissingRegionBinding(region.id))?,
            );
            next.source_overrides.remove(&region.id);
            let mut pieces = subtract_grid_rect(rect, intersection);
            pieces.sort_by_key(|piece| {
                std::cmp::Reverse((
                    u64::from(piece.width) * u64::from(piece.height),
                    piece.y,
                    piece.x,
                ))
            });
            if let Some(first) = pieces.first().copied() {
                let mut retained = region.clone();
                retained.grid_rect = Some(first);
                rebuilt.push(retained);
                let binding = next
                    .region_bindings
                    .get_mut(&region.id)
                    .expect("intersected binding");
                reset_source_mapping_preserving_behavior(&mut binding.mapping);
                for piece in pieces.into_iter().skip(1) {
                    let new_id =
                        unique_partition_region_id(&recipe, piece, occupied_ids.iter().copied());
                    occupied_ids.insert(new_id);
                    let mut sibling = region.clone();
                    sibling.id = new_id;
                    sibling.id_color = IdColor::for_region(new_id);
                    sibling.display_name = format!("Region {:03}", rebuilt.len());
                    sibling.grid_rect = Some(piece);
                    rebuilt.push(sibling);
                    let mut binding = next
                        .region_bindings
                        .get(&region.id)
                        .cloned()
                        .expect("intersected binding");
                    binding.region_id = new_id;
                    binding.mapping = RegionMapping::default();
                    next.region_bindings.insert(new_id, binding);
                }
            } else {
                next.region_bindings.remove(&region.id);
                occupied_ids.remove(&region.id);
            }
        }

        let mut drawn = drawn_template.ok_or_else(|| {
            TrimSheetDocumentError::InvalidPartitionEdit(
                "drawn rectangle does not intersect the atlas".into(),
            )
        })?;
        let drawn_id = unique_partition_region_id(&recipe, grid_rect, occupied_ids.iter().copied());
        drawn.id = drawn_id;
        drawn.id_color = IdColor::for_region(drawn_id);
        drawn.display_name = format!("Region {:03}", rebuilt.len());
        drawn.grid_rect = Some(grid_rect);
        rebuilt.push(drawn);
        let mut binding = drawn_binding.expect("drawn intersection binding");
        binding.region_id = drawn_id;
        binding.mapping = RegionMapping::default();
        next.region_bindings.insert(drawn_id, binding);
        next.topology.regions = rebuilt;
        next.repin_source_frame_topology()?;
        Ok(next)
    }

    /// Resizes one existing source-frame region while preserving its stable identity. Only cells
    /// inside the union of the old and requested rectangles can change owner. Gained cells move
    /// from intersected neighbors to the selected region; released cells move to the nearest
    /// neighbor that touched the old region. The result is rectangularized and validated as one
    /// atomic exact-cover edit, with any unavoidable neighbor fragments receiving deterministic
    /// IDs while the selected region keeps `region_id`.
    pub fn resize_source_frame_region(
        &self,
        region_id: RegionId,
        grid_rect: GridRect,
    ) -> Result<Self, TrimSheetDocumentError> {
        let mut next = self.source_frame_editable_clone()?;
        let grid = next
            .logical_grid
            .ok_or(TrimSheetDocumentError::InvalidSourceFrame)?;
        if grid_rect.width == 0
            || grid_rect.height == 0
            || grid_rect.x.saturating_add(grid_rect.width) > grid.width
            || grid_rect.y.saturating_add(grid_rect.height) > grid.height
        {
            return Err(TrimSheetDocumentError::InvalidPartitionEdit(
                "resized rectangle must be inside the logical grid and at least one cell wide"
                    .into(),
            ));
        }
        let selected_index = next
            .topology
            .regions
            .iter()
            .position(|region| region.id == region_id)
            .ok_or(TrimSheetDocumentError::MissingRegionBinding(region_id))?;
        let original_regions = next.topology.regions.clone();
        let original_rect = original_regions[selected_index]
            .grid_rect
            .ok_or(TrimSheetDocumentError::InvalidSourceFrame)?;
        if original_rect == grid_rect {
            return Ok(next);
        }

        let cell_count =
            usize::try_from(u64::from(grid.width) * u64::from(grid.height)).map_err(|_| {
                TrimSheetDocumentError::InvalidPartitionEdit(
                    "logical grid is too large to resize safely".into(),
                )
            })?;
        let mut owners = vec![region_id; cell_count];
        for region in &original_regions {
            let rect = region
                .grid_rect
                .ok_or(TrimSheetDocumentError::InvalidSourceFrame)?;
            for y in rect.y..rect.y + rect.height {
                for x in rect.x..rect.x + rect.width {
                    owners[grid_cell_index(grid, x, y)] = region.id;
                }
            }
        }

        let touching_neighbors = original_regions
            .iter()
            .enumerate()
            .filter_map(|(index, region)| {
                let rect = region.grid_rect?;
                (region.id != region_id && grid_rects_touch(original_rect, rect))
                    .then_some((index, region.id, rect))
            })
            .collect::<Vec<_>>();
        let retained_overlap = grid_rect_intersection(original_rect, grid_rect)
            .ok_or_else(|| TrimSheetDocumentError::InvalidPartitionEdit("resize must retain part of the selected region; use Draw Region to place a separate rectangle".into()))?;
        let released = subtract_grid_rect(original_rect, retained_overlap);
        if !released.is_empty() && touching_neighbors.is_empty() {
            return Err(TrimSheetDocumentError::InvalidPartitionEdit("the only atlas region cannot be made smaller because no neighbor can receive the released area".into()));
        }
        for piece in released {
            // A resize releases at most four rectangular strips. Transfer each strip as one
            // ownership unit; choosing a recipient per cell creates diagonal Voronoi stairs
            // which then explode into dozens of thin rectangular regions.
            let (_, owner, _) = touching_neighbors
                .iter()
                .min_by_key(|(ordinal, _, rect)| {
                    (
                        mergeable_grid_rect(piece, *rect).is_none(),
                        std::cmp::Reverse(grid_rect_touch_span(piece, *rect)),
                        grid_rect_distance(piece, *rect),
                        *ordinal,
                    )
                })
                .expect("released cells require a touching neighbor");
            for y in piece.y..piece.y + piece.height {
                for x in piece.x..piece.x + piece.width {
                    owners[grid_cell_index(grid, x, y)] = *owner;
                }
            }
        }
        for y in grid_rect.y..grid_rect.y + grid_rect.height {
            for x in grid_rect.x..grid_rect.x + grid_rect.width {
                owners[grid_cell_index(grid, x, y)] = region_id;
            }
        }

        let pieces_by_owner = rectangularize_grid_owners(grid, &owners);
        let recipe = next
            .partition_provenance
            .as_ref()
            .expect("editable provenance")
            .recipe
            .clone();
        let mut occupied_ids = original_regions
            .iter()
            .map(|region| region.id)
            .collect::<BTreeSet<_>>();
        let original_bindings = next.region_bindings.clone();
        let mut rebuilt = Vec::new();
        next.region_bindings.clear();
        for region in original_regions {
            let owner = region.id;
            let original = region.grid_rect.expect("source-frame region");
            let Some(mut pieces) = pieces_by_owner.get(&owner).cloned() else {
                next.source_overrides.remove(&owner);
                occupied_ids.remove(&owner);
                continue;
            };
            pieces.sort_by_key(|piece| {
                std::cmp::Reverse((
                    u64::from(piece.width) * u64::from(piece.height),
                    piece.y,
                    piece.x,
                ))
            });
            if owner == region_id {
                pieces.sort_by_key(|piece| if *piece == grid_rect { 0 } else { 1 });
            }
            let changed = pieces.len() != 1 || pieces[0] != original;
            for (piece_index, piece) in pieces.into_iter().enumerate() {
                let mut definition = region.clone();
                let mut binding = original_bindings
                    .get(&owner)
                    .cloned()
                    .ok_or(TrimSheetDocumentError::MissingRegionBinding(owner))?;
                if piece_index == 0 {
                    definition.grid_rect = Some(piece);
                    if changed {
                        reset_source_mapping_preserving_behavior(&mut binding.mapping);
                        next.source_overrides.remove(&owner);
                    }
                    next.region_bindings.insert(owner, binding);
                } else {
                    let new_id =
                        unique_partition_region_id(&recipe, piece, occupied_ids.iter().copied());
                    occupied_ids.insert(new_id);
                    definition.id = new_id;
                    definition.id_color = IdColor::for_region(new_id);
                    definition.display_name = format!("Region {:03}", rebuilt.len());
                    definition.grid_rect = Some(piece);
                    binding.region_id = new_id;
                    binding.mapping = RegionMapping::default();
                    next.region_bindings.insert(new_id, binding);
                }
                rebuilt.push(definition);
            }
        }
        if rebuilt.len() > crate::source_frame::MAX_PARTITION_REGIONS as usize {
            return Err(TrimSheetDocumentError::InvalidPartitionEdit(
                "resize would fragment neighboring ownership beyond the 256-region safety limit"
                    .into(),
            ));
        }
        next.topology.regions = rebuilt;
        next.repin_source_frame_topology()?;
        Ok(next)
    }

    fn source_frame_editable_clone(&self) -> Result<Self, TrimSheetDocumentError> {
        if self.source_frame.is_none()
            || self.logical_grid.is_none()
            || self.partition_provenance.is_none()
            || self.topology.kind != TopologyKind::CustomAtlas
        {
            return Err(TrimSheetDocumentError::InvalidPartitionEdit(
                "layout editing is available only for an accepted source-frame atlas".into(),
            ));
        }
        Ok(self.clone())
    }

    fn repin_source_frame_topology(&mut self) -> Result<(), TrimSheetDocumentError> {
        let grid = self
            .logical_grid
            .ok_or(TrimSheetDocumentError::InvalidSourceFrame)?;
        let output = self.render_settings.output_size;
        for region in &mut self.topology.regions {
            let rect = region
                .grid_rect
                .ok_or(TrimSheetDocumentError::InvalidSourceFrame)?;
            let allocation = grid_rect_to_output(rect, grid, output);
            region.allocation_rect = allocation;
            region.hotspot_rect = allocation;
            region.orientation = if allocation.width > allocation.height {
                RegionOrientation::Horizontal
            } else if allocation.height > allocation.width {
                RegionOrientation::Vertical
            } else {
                RegionOrientation::Unspecified
            };
        }
        self.topology.regions.sort_by_key(|region| {
            let rect = region.grid_rect.expect("source-frame region");
            (rect.y, rect.x, rect.height, rect.width)
        });
        let prior_families = self
            .partition_provenance
            .as_ref()
            .expect("editable provenance")
            .accepted_region_ids
            .iter()
            .copied()
            .zip(
                self.partition_provenance
                    .as_ref()
                    .expect("editable provenance")
                    .tree
                    .iter()
                    .map(|node| (node.family, node.lineage)),
            )
            .collect::<BTreeMap<_, _>>();
        let provenance = self
            .partition_provenance
            .as_mut()
            .expect("editable provenance");
        provenance.accepted_region_ids = self
            .topology
            .regions
            .iter()
            .map(|region| region.id)
            .collect();
        provenance.tree = self
            .topology
            .regions
            .iter()
            .enumerate()
            .map(|(ordinal, region)| PartitionTreeNode {
                grid_rect: region.grid_rect.expect("source-frame region"),
                family: prior_families
                    .get(&region.id)
                    .map(|value| value.0)
                    .unwrap_or(PartitionFamily::Remainder),
                ordinal: ordinal as u32,
                lineage: prior_families
                    .get(&region.id)
                    .map(|value| value.1)
                    .unwrap_or_default(),
            })
            .collect();
        let signature = hash_serializable(&provenance.tree)?;
        let frame = self.source_frame.as_ref().expect("source frame");
        self.topology = AcceptedTopology::new(
            TopologyKind::CustomAtlas,
            self.topology.snapshot.clone(),
            format!(
                "source-frame:{}:{}:{}",
                hex_hash(frame.identity),
                hex_hash(provenance.recipe_hash),
                hex_hash(signature)
            ),
            self.topology.regions.clone(),
        )?;
        provenance.topology_hash = self.topology.topology_hash;
        self.document_revision = self.document_revision.saturating_add(1);
        self.topology_revision = self.topology_revision.saturating_add(1);
        self.appearance_revision = self.document_revision;
        self.validate()?;
        Ok(())
    }

    /// Reconciles imported material and patch catalogs without consulting any layout state.
    pub fn with_assets(
        &self,
        materials: Vec<MaterialSourceSet>,
        patches: Vec<Patch>,
    ) -> Result<Self, TrimSheetDocumentError> {
        let mut next = self.clone();
        next.materials = materials;
        next.patches = patches;
        next.document_revision = next.document_revision.saturating_add(1);
        next.appearance_revision = next.document_revision;
        next.validate()?;
        Ok(next)
    }
}

fn regions_from_template(
    template: &TemplateDefinition,
) -> Result<Vec<RegionDefinition>, TrimSheetDocumentError> {
    template
        .snapshot()
        .map_err(|_| TrimSheetDocumentError::InvalidTemplateSnapshot)?;
    template
        .stable_order
        .iter()
        .map(|slot_key| {
            let slot = template
                .slots
                .iter()
                .find(|slot| &slot.slot_key == slot_key)
                .ok_or(TrimSheetDocumentError::InvalidTemplateSnapshot)?;
            let (uv_kind, fit_axis) = match slot.fit {
                TemplateFitSemantics::Planar => (UvFitKind::Rectangular, FitAxis::Automatic),
                TemplateFitSemantics::HorizontalStrip => (UvFitKind::Strip, FitAxis::Vertical),
                TemplateFitSemantics::VerticalStrip => (UvFitKind::Strip, FitAxis::Horizontal),
                TemplateFitSemantics::UniqueContain => (UvFitKind::Unique, FitAxis::None),
                TemplateFitSemantics::TrimCap => (UvFitKind::Cap, FitAxis::Automatic),
                TemplateFitSemantics::Radial => (UvFitKind::Radial, FitAxis::None),
            };
            let rect = slot.allocation;
            let orientation = if rect.width > rect.height {
                RegionOrientation::Horizontal
            } else if rect.height > rect.width {
                RegionOrientation::Vertical
            } else {
                RegionOrientation::Unspecified
            };
            Ok(RegionDefinition {
                id: deterministic_region_id(
                    &template.identity.compatibility_key,
                    &slot.compatibility_key,
                ),
                display_name: title_from_key(&slot.slot_key),
                id_color: slot.id_color,
                allocation_rect: slot.allocation,
                hotspot_rect: slot.hotspot,
                role: slot.role,
                orientation,
                uv_fit: UvFitPolicy {
                    kind: uv_kind,
                    fit_axis,
                    keep_proportion: true,
                    allowed_rotations: vec![QuarterTurn::Zero],
                    mirror_allowed: !matches!(slot.role, TemplateSlotRole::Radial),
                    world_size_meters: [slot.world_placement.width, slot.world_placement.height],
                    classification_tags: Vec::new(),
                },
                structural_profile: slot.structural_profile,
                material_group: slot.material_group.clone(),
                weathering_group: slot.variation_group.clone(),
                radial_parameters: slot.radial_parameters,
                enabled: true,
                grid_rect: None,
            })
        })
        .collect()
}

fn template_region_mapping(_mapping: TemplateSourceMapping) -> RegionMapping {
    RegionMapping {
        // The manifest's sourceMapping was authored for the fixed atlas layout,
        // not for an arbitrary imported photograph. Start from the complete
        // source and retain the explicit unplaced state until the user crops it.
        projection: Projection::default(),
        source_crop_intent: Some(SourceCropIntent::Unplaced),
        warps: Vec::new(),
        radial: None,
        transform: MappingTransform::default(),
        address_mode: AddressMode::Clamp,
        sampling: SamplingPolicy::default(),
        behavior: RegionBehavior::default(),
    }
}

fn reset_source_mapping(behavior: RegionBehavior) -> RegionMapping {
    RegionMapping {
        radial: behavior.radial,
        address_mode: if behavior.sampling == RegionSampling::OneShot {
            AddressMode::Clamp
        } else {
            AddressMode::Repeat
        },
        behavior,
        ..RegionMapping::default()
    }
}

fn reset_source_mapping_preserving_behavior(mapping: &mut RegionMapping) {
    *mapping = reset_source_mapping(mapping.behavior.clone());
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum TrimSheetDocumentCommand {
    ApplyAuthoredLayoutPreset {
        preset: AuthoredLayoutPreset,
        instance_id: String,
    },
    SetAuthoredLayoutPresetSnapshot {
        preset: AuthoredLayoutPreset,
    },
    AcceptSourceFramePartition {
        recipe: PartitionRecipe,
    },
    SplitSourceFrameRegion {
        region_id: RegionId,
        axis: PartitionAxis,
    },
    MergeSourceFrameRegions {
        region_id: RegionId,
        sibling_id: RegionId,
    },
    MoveSourceFrameBoundary {
        region_id: RegionId,
        axis: PartitionAxis,
        coordinate: u32,
    },
    DrawSourceFrameRegion {
        grid_rect: GridRect,
    },
    ResizeSourceFrameRegion {
        region_id: RegionId,
        grid_rect: GridRect,
    },
    SetPrimaryMaterial {
        material_id: SourceSetId,
    },
    SetRegionContent {
        region_id: RegionId,
        content: ContentReference,
    },
    SetRegionAddressMode {
        region_id: RegionId,
        address_mode: AddressMode,
    },
    SetRegionBehavior {
        region_id: RegionId,
        behavior: RegionBehavior,
    },
    SetSheetFraming {
        framing: SheetFraming,
    },
    SetRegionProjection {
        region_id: RegionId,
        projection: Projection,
    },
    SetRegionRadial {
        region_id: RegionId,
        radial: RadialMappingSettings,
    },
    SetOutputResolution {
        output_size: PixelSize,
    },
    SetAtlasPadding {
        padding_px: u32,
    },
    SetChannelRenderPolicy {
        channel: Channel,
        policy: ChannelRenderPolicy,
    },
    SetSourceFrame {
        bounds: NormalizedBounds,
    },
    DetachSourceCell {
        region_id: RegionId,
    },
    ResetSourceCell {
        region_id: RegionId,
    },
}

fn deterministic_region_id(template_key: &str, region_key: &str) -> RegionId {
    let mut hasher = Sha256::new();
    hasher.update(b"hot-trimmer-region-v1");
    hasher.update(template_key.as_bytes());
    hasher.update([0]);
    hasher.update(region_key.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    RegionId::from_bytes(bytes)
}

fn authored_region(ordinal: u32, rect: GridRect) -> AuthoredLayoutPresetRegion {
    let orientation = if rect.width > rect.height {
        RegionOrientation::Horizontal
    } else if rect.height > rect.width {
        RegionOrientation::Vertical
    } else {
        RegionOrientation::Unspecified
    };
    AuthoredLayoutPresetRegion {
        preset_region_key: format!("cascade-{ordinal:02}"),
        display_name: format!("Region {:03}", ordinal + 1),
        grid_rect: rect,
        role: TemplateSlotRole::Planar,
        orientation,
        uv_fit: UvFitPolicy {
            kind: UvFitKind::Rectangular,
            fit_axis: FitAxis::Automatic,
            keep_proportion: true,
            allowed_rotations: vec![QuarterTurn::Zero],
            mirror_allowed: false,
            world_size_meters: [f64::from(rect.width), f64::from(rect.height)],
            classification_tags: vec!["AUTHORED_LAYOUT".into()],
        },
        structural_profile: StructuralProfile::Flat,
        default_behavior: RegionBehavior::default(),
    }
}

#[must_use]
pub fn diagonal_cascade_authored_preset() -> AuthoredLayoutPreset {
    let rects = [
        GridRect {
            x: 0,
            y: 0,
            width: 16,
            height: 32,
        },
        GridRect {
            x: 16,
            y: 0,
            width: 16,
            height: 16,
        },
        GridRect {
            x: 32,
            y: 0,
            width: 32,
            height: 16,
        },
        GridRect {
            x: 16,
            y: 16,
            width: 16,
            height: 16,
        },
        GridRect {
            x: 32,
            y: 16,
            width: 16,
            height: 16,
        },
        GridRect {
            x: 48,
            y: 16,
            width: 16,
            height: 16,
        },
        GridRect {
            x: 0,
            y: 32,
            width: 16,
            height: 8,
        },
        GridRect {
            x: 16,
            y: 32,
            width: 8,
            height: 16,
        },
        GridRect {
            x: 24,
            y: 32,
            width: 1,
            height: 24,
        },
        GridRect {
            x: 25,
            y: 32,
            width: 1,
            height: 24,
        },
        GridRect {
            x: 26,
            y: 32,
            width: 2,
            height: 24,
        },
        GridRect {
            x: 28,
            y: 32,
            width: 4,
            height: 24,
        },
        GridRect {
            x: 32,
            y: 32,
            width: 32,
            height: 32,
        },
        GridRect {
            x: 0,
            y: 40,
            width: 8,
            height: 8,
        },
        GridRect {
            x: 8,
            y: 40,
            width: 8,
            height: 8,
        },
        GridRect {
            x: 0,
            y: 48,
            width: 8,
            height: 8,
        },
        GridRect {
            x: 8,
            y: 48,
            width: 8,
            height: 4,
        },
        GridRect {
            x: 16,
            y: 48,
            width: 4,
            height: 8,
        },
        GridRect {
            x: 20,
            y: 48,
            width: 4,
            height: 8,
        },
        GridRect {
            x: 8,
            y: 52,
            width: 8,
            height: 4,
        },
        GridRect {
            x: 0,
            y: 56,
            width: 32,
            height: 1,
        },
        GridRect {
            x: 0,
            y: 57,
            width: 32,
            height: 1,
        },
        GridRect {
            x: 0,
            y: 58,
            width: 32,
            height: 2,
        },
        GridRect {
            x: 0,
            y: 60,
            width: 32,
            height: 4,
        },
    ];
    AuthoredLayoutPreset {
        preset_id: "builtin.diagonal-cascade".into(),
        schema_version: AUTHORED_LAYOUT_PRESET_SCHEMA_VERSION,
        name: "Diagonal Cascade".into(),
        logical_grid: LogicalGridSpec::DEFAULT,
        canonical_aspect: [1, 1],
        regions: rects
            .into_iter()
            .enumerate()
            .map(|(i, rect)| authored_region(i as u32, rect))
            .collect(),
        provenance: "checked_in_authored_fixture".into(),
    }
}

#[must_use]
pub fn new_blank_authored_preset(grid: LogicalGridSpec) -> AuthoredLayoutPreset {
    AuthoredLayoutPreset {
        preset_id: "builtin.new-blank".into(),
        schema_version: AUTHORED_LAYOUT_PRESET_SCHEMA_VERSION,
        name: "New Blank".into(),
        logical_grid: grid,
        canonical_aspect: [1, 1],
        regions: vec![authored_region(
            0,
            GridRect {
                x: 0,
                y: 0,
                width: grid.width,
                height: grid.height,
            },
        )],
        provenance: "built_in_blank".into(),
    }
}

pub fn validate_authored_layout_preset(
    preset: &AuthoredLayoutPreset,
) -> Result<(), TrimSheetDocumentError> {
    if preset.schema_version != AUTHORED_LAYOUT_PRESET_SCHEMA_VERSION
        || preset.preset_id.trim().is_empty()
        || preset.name.trim().is_empty()
        || preset.canonical_aspect.contains(&0)
        || preset.regions.is_empty()
        || preset.logical_grid.width == 0
        || preset.logical_grid.height == 0
    {
        return Err(TrimSheetDocumentError::InvalidPartitionEdit(
            "authored layout preset metadata is invalid".into(),
        ));
    }
    let cell_count =
        (preset.logical_grid.width as usize).saturating_mul(preset.logical_grid.height as usize);
    let mut owners = vec![0_u8; cell_count];
    let mut keys = BTreeSet::new();
    for region in &preset.regions {
        let rect = region.grid_rect;
        if !keys.insert(region.preset_region_key.as_str())
            || region.preset_region_key.trim().is_empty()
            || rect.width == 0
            || rect.height == 0
            || rect.x + rect.width > preset.logical_grid.width
            || rect.y + rect.height > preset.logical_grid.height
        {
            return Err(TrimSheetDocumentError::InvalidPartitionEdit(
                "authored layout preset region is invalid".into(),
            ));
        }
        validate_region_behavior(RegionId::from_bytes([0; 16]), &region.default_behavior)?;
        for y in rect.y..rect.y + rect.height {
            for x in rect.x..rect.x + rect.width {
                let index = (y * preset.logical_grid.width + x) as usize;
                owners[index] = owners[index].saturating_add(1);
            }
        }
    }
    if owners.iter().any(|count| *count != 1) {
        return Err(TrimSheetDocumentError::InvalidPartitionEdit(
            "authored layout preset must exactly cover its grid".into(),
        ));
    }
    Ok(())
}

fn rects_overlap(left: CanonicalRect, right: CanonicalRect) -> bool {
    left.x < right.x.saturating_add(right.width)
        && right.x < left.x.saturating_add(left.width)
        && left.y < right.y.saturating_add(right.height)
        && right.y < left.y.saturating_add(left.height)
}

fn title_from_key(key: &str) -> String {
    key.split(['_', '-', ':'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().collect::<String>() + chars.as_str()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn hash_serializable<T: Serialize>(value: &T) -> Result<DocumentHash, TrimSheetDocumentError> {
    let bytes = serde_json::to_vec(value).map_err(|_| TrimSheetDocumentError::HashSerialization)?;
    Ok(DocumentHash(Sha256::digest(bytes).into()))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_rect(rect: CanonicalRect, size: PixelSize) -> bool {
    rect.width > 0
        && rect.height > 0
        && rect
            .x
            .checked_add(rect.width)
            .is_some_and(|right| right <= size.width)
        && rect
            .y
            .checked_add(rect.height)
            .is_some_and(|bottom| bottom <= size.height)
}

fn rect_contains(outer: CanonicalRect, inner: CanonicalRect) -> bool {
    inner.width > 0
        && inner.height > 0
        && inner.x >= outer.x
        && inner.y >= outer.y
        && inner
            .x
            .checked_add(inner.width)
            .zip(outer.x.checked_add(outer.width))
            .is_some_and(|(inner_right, outer_right)| inner_right <= outer_right)
        && inner
            .y
            .checked_add(inner.height)
            .zip(outer.y.checked_add(outer.height))
            .is_some_and(|(inner_bottom, outer_bottom)| inner_bottom <= outer_bottom)
}

fn valid_quad(quad: [NormalizedPoint; 4]) -> bool {
    let points = quad.map(|point| (point.x.get(), point.y.get()));
    let area = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(4)
        .map(|(&(x1, y1), &(x2, y2))| x1 * y2 - x2 * y1)
        .sum::<f64>()
        .abs();
    area > 1e-8
        && !segments_intersect(points[0], points[1], points[2], points[3])
        && !segments_intersect(points[1], points[2], points[3], points[0])
}

fn segments_intersect(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> bool {
    fn cross(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
        (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
    }
    cross(a, b, c) * cross(a, b, d) < 0.0 && cross(c, d, a) * cross(c, d, b) < 0.0
}

fn validate_solid(
    region_id: RegionId,
    values: &SolidChannelValues,
) -> Result<(), TrimSheetDocumentError> {
    if values.base_color.is_none() && values.scalar_channels.is_empty() {
        return Err(TrimSheetDocumentError::MissingSolidContent(region_id));
    }
    if values.scalar_channels.iter().any(|(channel, value)| {
        !value.is_finite()
            || !(0.0..=1.0).contains(value)
            || matches!(
                channel.data_kind(),
                crate::ChannelDataKind::Color | crate::ChannelDataKind::Vector
            )
    }) {
        return Err(TrimSheetDocumentError::InvalidNumericValue(region_id));
    }
    Ok(())
}

fn validate_mapping(
    region_id: RegionId,
    mapping: &RegionMapping,
) -> Result<(), TrimSheetDocumentError> {
    validate_region_behavior(region_id, &mapping.behavior)?;
    if mapping.radial != mapping.behavior.radial
        || (mapping.behavior.sampling == RegionSampling::OneShot
            && mapping.address_mode != AddressMode::Clamp)
        || (mapping.behavior.sampling != RegionSampling::OneShot
            && mapping.address_mode != AddressMode::Repeat)
    {
        return Err(TrimSheetDocumentError::UnsupportedRegionBehavior(region_id));
    }
    match mapping.projection {
        Projection::Crop { bounds, .. } => {
            if bounds.width.get() <= 0.0
                || bounds.height.get() <= 0.0
                || bounds.x.get() + bounds.width.get() > 1.0 + f64::EPSILON
                || bounds.y.get() + bounds.height.get() > 1.0 + f64::EPSILON
            {
                return Err(TrimSheetDocumentError::InvalidSourceGeometry(region_id));
            }
        }
        Projection::Perspective { quad } => {
            if !valid_quad(quad) {
                return Err(TrimSheetDocumentError::InvalidSourceGeometry(region_id));
            }
        }
    }
    if let Some(radial) = mapping.radial
        && (!radial.center_x.is_finite()
            || !radial.center_y.is_finite()
            || !radial.inner_radius.is_finite()
            || !radial.outer_radius.is_finite()
            || !radial.falloff.is_finite()
            || !radial.blend_width.is_finite()
            || !radial.seam_blend_width.is_finite()
            || !(0.0..=1.0).contains(&radial.center_x)
            || !(0.0..=1.0).contains(&radial.center_y)
            || radial.inner_radius < 0.0
            || radial.outer_radius <= radial.inner_radius
            || radial.outer_radius > 2.0
            || !(0.0..=1.0).contains(&radial.blend_width)
            || !(0.0..=0.25).contains(&radial.seam_blend_width)
            || !(0.1..=4.0).contains(&radial.falloff))
    {
        return Err(TrimSheetDocumentError::InvalidRadialMapping(region_id));
    }
    if mapping.transform.scale.iter().any(|value| {
        !value.is_finite() || value.abs() <= 1e-6 || value.abs() > MAX_MAPPING_MAGNITUDE
    }) || mapping
        .transform
        .offset
        .iter()
        .any(|value| !value.is_finite() || value.abs() > MAX_MAPPING_MAGNITUDE)
        || !mapping.transform.rotation_degrees.is_finite()
        || !mapping.sampling.scale.is_finite()
        || !(0.0..=MAX_MAPPING_MAGNITUDE).contains(&mapping.sampling.scale)
    {
        return Err(TrimSheetDocumentError::InvalidNumericValue(region_id));
    }
    if mapping.warps.len() > crate::MAX_SOURCE_LAYER_WARPS {
        return Err(TrimSheetDocumentError::TooManyWarps(region_id));
    }
    let mut operation_ids = BTreeSet::new();
    for (index, operation) in mapping.warps.iter().enumerate() {
        if !operation_ids.insert(operation.id) {
            return Err(TrimSheetDocumentError::DuplicateWarpId {
                region_id,
                operation_id: operation.id,
            });
        }
        if operation.version != WARP_OPERATION_VERSION {
            return Err(TrimSheetDocumentError::UnsupportedWarpVersion {
                region_id,
                index,
                version: operation.version,
            });
        }
        if !source_warp_is_valid(&operation.operation) {
            return Err(TrimSheetDocumentError::InvalidWarp { region_id, index });
        }
    }
    Ok(())
}

fn validate_region_behavior(
    region_id: RegionId,
    behavior: &RegionBehavior,
) -> Result<(), TrimSheetDocumentError> {
    if behavior.version != REGION_BEHAVIOR_VERSION
        || behavior.edge_eligibility != EdgeEligibility::for_continuity(behavior.continuity)
        || !behavior.supports_sampling()
        || (behavior.role == ManualRegionRole::Radial) != behavior.radial.is_some()
        || behavior
            .period_pixels
            .is_some_and(|period| period.contains(&0))
    {
        return Err(TrimSheetDocumentError::UnsupportedRegionBehavior(region_id));
    }
    if let Some(radial) = behavior.radial
        && (!radial.center_x.is_finite()
            || !radial.center_y.is_finite()
            || !radial.inner_radius.is_finite()
            || !radial.outer_radius.is_finite()
            || !radial.falloff.is_finite()
            || !radial.blend_width.is_finite()
            || !radial.seam_blend_width.is_finite()
            || !(0.0..=1.0).contains(&radial.center_x)
            || !(0.0..=1.0).contains(&radial.center_y)
            || radial.inner_radius < 0.0
            || radial.outer_radius <= radial.inner_radius
            || radial.outer_radius > 2.0
            || !(0.0..=1.0).contains(&radial.blend_width)
            || !(0.0..=0.25).contains(&radial.seam_blend_width)
            || !(0.1..=4.0).contains(&radial.falloff))
    {
        return Err(TrimSheetDocumentError::InvalidRadialMapping(region_id));
    }
    Ok(())
}

fn valid_normalized_rect(bounds: NormalizedBounds) -> bool {
    bounds.x.get().is_finite()
        && bounds.y.get().is_finite()
        && bounds.width.get().is_finite()
        && bounds.height.get().is_finite()
        && bounds.width.get() > 0.0
        && bounds.height.get() > 0.0
        && bounds.x.get() >= 0.0
        && bounds.y.get() >= 0.0
        && bounds.x.get() + bounds.width.get() <= 1.0 + f64::EPSILON
        && bounds.y.get() + bounds.height.get() <= 1.0 + f64::EPSILON
}

fn aspect_matches(width: f64, height: f64, expected_width: f64, expected_height: f64) -> bool {
    if !width.is_finite()
        || !height.is_finite()
        || !expected_width.is_finite()
        || !expected_height.is_finite()
        || width <= 0.0
        || height <= 0.0
        || expected_width <= 0.0
        || expected_height <= 0.0
    {
        return false;
    }
    let left = width * expected_height;
    let right = height * expected_width;
    (left - right).abs() <= 1e-9 * left.abs().max(right.abs()).max(1.0)
}

fn source_frame_region_bounds(
    frame: &SourceFrame,
    grid: LogicalGridSpec,
    rect: GridRect,
) -> NormalizedBounds {
    let source_x = crate::resolve_boundaries(
        (frame.bounds.x.get() * f64::from(frame.oriented_dimensions.width)).round() as u32,
        (frame.bounds.width.get() * f64::from(frame.oriented_dimensions.width)).round() as u32,
        grid.width,
    );
    let source_y = crate::resolve_boundaries(
        (frame.bounds.y.get() * f64::from(frame.oriented_dimensions.height)).round() as u32,
        (frame.bounds.height.get() * f64::from(frame.oriented_dimensions.height)).round() as u32,
        grid.height,
    );
    let x = f64::from(source_x[rect.x as usize]) / f64::from(frame.oriented_dimensions.width);
    let y = f64::from(source_y[rect.y as usize]) / f64::from(frame.oriented_dimensions.height);
    let right = f64::from(source_x[(rect.x + rect.width) as usize])
        / f64::from(frame.oriented_dimensions.width);
    let bottom = f64::from(source_y[(rect.y + rect.height) as usize])
        / f64::from(frame.oriented_dimensions.height);
    NormalizedBounds {
        x: crate::NormalizedScalar::new(x).expect("resolved source x"),
        y: crate::NormalizedScalar::new(y).expect("resolved source y"),
        width: crate::NormalizedScalar::new(right - x).expect("resolved source width"),
        height: crate::NormalizedScalar::new(bottom - y).expect("resolved source height"),
    }
}

fn grid_rect_to_output(rect: GridRect, grid: LogicalGridSpec, output: PixelSize) -> CanonicalRect {
    let xs = crate::resolve_boundaries(0, output.width, grid.width);
    let ys = crate::resolve_boundaries(0, output.height, grid.height);
    CanonicalRect {
        x: xs[rect.x as usize],
        y: ys[rect.y as usize],
        width: xs[(rect.x + rect.width) as usize] - xs[rect.x as usize],
        height: ys[(rect.y + rect.height) as usize] - ys[rect.y as usize],
    }
}

fn grid_rect_intersection(first: GridRect, second: GridRect) -> Option<GridRect> {
    let x = first.x.max(second.x);
    let y = first.y.max(second.y);
    let right = first
        .x
        .saturating_add(first.width)
        .min(second.x.saturating_add(second.width));
    let bottom = first
        .y
        .saturating_add(first.height)
        .min(second.y.saturating_add(second.height));
    if right > x && bottom > y {
        Some(GridRect {
            x,
            y,
            width: right - x,
            height: bottom - y,
        })
    } else {
        None
    }
}

fn subtract_grid_rect(rect: GridRect, cut: GridRect) -> Vec<GridRect> {
    let mut pieces = Vec::with_capacity(4);
    let rect_right = rect.x + rect.width;
    let rect_bottom = rect.y + rect.height;
    let cut_right = cut.x + cut.width;
    let cut_bottom = cut.y + cut.height;
    if cut.y > rect.y {
        pieces.push(GridRect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: cut.y - rect.y,
        });
    }
    if cut_bottom < rect_bottom {
        pieces.push(GridRect {
            x: rect.x,
            y: cut_bottom,
            width: rect.width,
            height: rect_bottom - cut_bottom,
        });
    }
    if cut.x > rect.x {
        pieces.push(GridRect {
            x: rect.x,
            y: cut.y,
            width: cut.x - rect.x,
            height: cut.height,
        });
    }
    if cut_right < rect_right {
        pieces.push(GridRect {
            x: cut_right,
            y: cut.y,
            width: rect_right - cut_right,
            height: cut.height,
        });
    }
    pieces
}

fn grid_cell_index(grid: LogicalGridSpec, x: u32, y: u32) -> usize {
    y as usize * grid.width as usize + x as usize
}

fn grid_rects_touch(first: GridRect, second: GridRect) -> bool {
    let vertical = (first.x + first.width == second.x || second.x + second.width == first.x)
        && first.y < second.y + second.height
        && first.y + first.height > second.y;
    let horizontal = (first.y + first.height == second.y || second.y + second.height == first.y)
        && first.x < second.x + second.width
        && first.x + first.width > second.x;
    vertical || horizontal
}

fn grid_rect_distance(first: GridRect, second: GridRect) -> u32 {
    let dx = if first.x + first.width <= second.x {
        second.x - (first.x + first.width)
    } else if second.x + second.width <= first.x {
        first.x - (second.x + second.width)
    } else {
        0
    };
    let dy = if first.y + first.height <= second.y {
        second.y - (first.y + first.height)
    } else if second.y + second.height <= first.y {
        first.y - (second.y + second.height)
    } else {
        0
    };
    dx.saturating_add(dy)
}

fn grid_rect_touch_span(first: GridRect, second: GridRect) -> u32 {
    if first.x + first.width == second.x || second.x + second.width == first.x {
        return (first.y + first.height)
            .min(second.y + second.height)
            .saturating_sub(first.y.max(second.y));
    }
    if first.y + first.height == second.y || second.y + second.height == first.y {
        return (first.x + first.width)
            .min(second.x + second.width)
            .saturating_sub(first.x.max(second.x));
    }
    0
}

fn rectangularize_grid_owners(
    grid: LogicalGridSpec,
    owners: &[RegionId],
) -> BTreeMap<RegionId, Vec<GridRect>> {
    let mut visited = vec![false; owners.len()];
    let mut rectangles = BTreeMap::<RegionId, Vec<GridRect>>::new();
    for y in 0..grid.height {
        for x in 0..grid.width {
            let start = grid_cell_index(grid, x, y);
            if visited[start] {
                continue;
            }
            let owner = owners[start];
            let mut width = 1;
            while x + width < grid.width {
                let index = grid_cell_index(grid, x + width, y);
                if visited[index] || owners[index] != owner {
                    break;
                }
                width += 1;
            }
            let mut height = 1;
            'rows: while y + height < grid.height {
                for offset in 0..width {
                    let index = grid_cell_index(grid, x + offset, y + height);
                    if visited[index] || owners[index] != owner {
                        break 'rows;
                    }
                }
                height += 1;
            }
            for row in y..y + height {
                for column in x..x + width {
                    visited[grid_cell_index(grid, column, row)] = true;
                }
            }
            rectangles.entry(owner).or_default().push(GridRect {
                x,
                y,
                width,
                height,
            });
        }
    }
    rectangles
}

fn mergeable_grid_rect(first: GridRect, second: GridRect) -> Option<GridRect> {
    if first.y == second.y
        && first.height == second.height
        && (first.x + first.width == second.x || second.x + second.width == first.x)
    {
        return Some(GridRect {
            x: first.x.min(second.x),
            y: first.y,
            width: first.width + second.width,
            height: first.height,
        });
    }
    if first.x == second.x
        && first.width == second.width
        && (first.y + first.height == second.y || second.y + second.height == first.y)
    {
        return Some(GridRect {
            x: first.x,
            y: first.y.min(second.y),
            width: first.width,
            height: first.height + second.height,
        });
    }
    None
}

fn unique_partition_region_id(
    recipe: &PartitionRecipe,
    rect: GridRect,
    existing: impl Iterator<Item = RegionId>,
) -> RegionId {
    let existing = existing.collect::<BTreeSet<_>>();
    (0..u32::MAX)
        .map(|ordinal| source_frame_region_id(recipe, rect, ordinal))
        .find(|id| !existing.contains(id))
        .expect("a SHA-256 region identity is available")
}

fn hex_hash(value: DocumentHash) -> String {
    value.0.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum TrimSheetDocumentError {
    #[error("canonical topology size or schema version is invalid")]
    InvalidCanonicalSize,
    #[error("topology compatibility key is empty")]
    InvalidCompatibilityKey,
    #[error("document, topology, and appearance revisions are inconsistent")]
    InvalidRevisions,
    #[error("stored topology hash does not match topology-only inputs")]
    TopologyHashMismatch,
    #[error("region ID is duplicated: {0}")]
    DuplicateRegionId(RegionId),
    #[error("region ID color is invalid or duplicated: {0:?}")]
    DuplicateIdColor(IdColor),
    #[error("region metadata is invalid: {0}")]
    InvalidRegionMetadata(RegionId),
    #[error("region allocation rectangle is invalid: {0}")]
    InvalidAllocationRect(RegionId),
    #[error("region allocation rectangle overlaps another region: {0}")]
    OverlappingAllocationRect(RegionId),
    #[error("region hotspot rectangle is invalid: {0}")]
    InvalidHotspotRect(RegionId),
    #[error("region UV-fit metadata is invalid: {0}")]
    InvalidUvFit(RegionId),
    #[error("material source ID is duplicated: {0}")]
    DuplicateMaterialId(SourceSetId),
    #[error("material source is invalid: {0}")]
    InvalidMaterial(SourceSetId),
    #[error("primary material is missing: {0}")]
    MissingPrimaryMaterial(SourceSetId),
    #[error("patch ID is duplicated: {0}")]
    DuplicatePatchId(PatchId),
    #[error("patch source geometry is invalid: {0}")]
    InvalidPatchGeometry(PatchId),
    #[error("procedural material ID is duplicated: {0}")]
    DuplicateProceduralId(LayerId),
    #[error("procedural material registration is invalid: {0}")]
    InvalidProcedural(LayerId),
    #[error("region has no binding: {0}")]
    MissingRegionBinding(RegionId),
    #[error("binding exists for a region outside the accepted topology: {0}")]
    UnexpectedRegionBinding(RegionId),
    #[error("binding map key {key} disagrees with binding region ID {value}")]
    BindingRegionMismatch { key: RegionId, value: RegionId },
    #[error("region inherits primary material but no primary material exists: {0}")]
    MissingInheritedContent(RegionId),
    #[error("region {region_id} references missing material {material_id}")]
    MissingMaterialContent {
        region_id: RegionId,
        material_id: SourceSetId,
    },
    #[error("region {region_id} references missing patch {patch_id}")]
    MissingPatchContent {
        region_id: RegionId,
        patch_id: PatchId,
    },
    #[error("region {region_id} references missing procedural material {procedural_id}")]
    MissingProceduralContent {
        region_id: RegionId,
        procedural_id: LayerId,
    },
    #[error("solid content contains no channels: {0}")]
    MissingSolidContent(RegionId),
    #[error("region source geometry is empty, out of bounds, or singular: {0}")]
    InvalidSourceGeometry(RegionId),
    #[error("source-frame region must be detached before editing: {0}")]
    SourceCellMustBeDetached(RegionId),
    #[error("region radial mapping is invalid or unsupported for this region: {0}")]
    InvalidRadialMapping(RegionId),
    #[error("region behavior is internally inconsistent or unsupported by Stage 14: {0}")]
    UnsupportedRegionBehavior(RegionId),
    #[error("region mapping contains a non-finite or out-of-range numeric value: {0}")]
    InvalidNumericValue(RegionId),
    #[error("region has too many warp operations: {0}")]
    TooManyWarps(RegionId),
    #[error("region {region_id} repeats warp operation ID {operation_id}")]
    DuplicateWarpId {
        region_id: RegionId,
        operation_id: LayerId,
    },
    #[error("region {region_id} warp {index} uses unsupported version {version}")]
    UnsupportedWarpVersion {
        region_id: RegionId,
        index: usize,
        version: u16,
    },
    #[error("region {region_id} warp {index} contains invalid or unbounded parameters")]
    InvalidWarp { region_id: RegionId, index: usize },
    #[error("treatment layer is duplicated or invalid: {0}")]
    InvalidTreatment(LayerId),
    #[error("render settings are invalid")]
    InvalidRenderSettings,
    #[error("generator provenance is invalid")]
    InvalidGeneratorProvenance,
    #[error("source frame is invalid")]
    InvalidSourceFrame,
    #[error("source frame bounds do not preserve its output aspect")]
    InvalidSourceFrameAspect,
    #[error("region source override is invalid: {0}")]
    InvalidSourceOverride(RegionId),
    #[error("region source override does not preserve its destination aspect: {0}")]
    InvalidSourceOverrideAspect(RegionId),
    #[error("deterministic hash serialization failed")]
    HashSerialization,
    #[error("the accepted template snapshot is invalid")]
    InvalidTemplateSnapshot,
    #[error("source-frame partition recipe is invalid: {0}")]
    InvalidSourcePartition(crate::PartitionError),
    #[error("source-frame layout edit is invalid: {0}")]
    InvalidPartitionEdit(String),
    #[error("a pinned standard or custom template topology was mutated")]
    TemplateTopologyMutation,
    #[error("standard template does not exactly match its shipped registry definition")]
    StandardTemplateRegistryMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        NormalizedScalar, PatchGeometry, PatchProperties, RectificationSettings, SourceId,
        TemplateRegistry, WeightedTemplateGrammar, compile_weighted_grammar,
    };

    fn material(id: SourceSetId) -> MaterialSourceSet {
        MaterialSourceSet {
            id,
            name: "primary".into(),
            maps: vec![MaterialMapContent {
                kind: MaterialMapKind::BaseColor,
                sha256: "a".repeat(64),
            }],
        }
    }

    fn patch(id: PatchId) -> Patch {
        Patch {
            id,
            source_id: SourceId::new(),
            name: "detail".into(),
            enabled: true,
            geometry: PatchGeometry {
                corners: [
                    NormalizedPoint::new(0.1, 0.1).expect("corner"),
                    NormalizedPoint::new(0.9, 0.1).expect("corner"),
                    NormalizedPoint::new(0.9, 0.9).expect("corner"),
                    NormalizedPoint::new(0.1, 0.9).expect("corner"),
                ],
                assistance_mask: None,
            },
            properties: PatchProperties::default(),
            rectification: RectificationSettings::default(),
        }
    }

    fn region(id: RegionId, color: IdColor) -> RegionDefinition {
        let rect = CanonicalRect {
            x: 0,
            y: 0,
            width: 4_096,
            height: 4_096,
        };
        RegionDefinition {
            id,
            display_name: "surface".into(),
            id_color: color,
            allocation_rect: rect,
            hotspot_rect: rect,
            role: TemplateSlotRole::Planar,
            orientation: RegionOrientation::Unspecified,
            uv_fit: UvFitPolicy {
                kind: UvFitKind::Rectangular,
                fit_axis: FitAxis::Automatic,
                keep_proportion: true,
                allowed_rotations: vec![QuarterTurn::Zero],
                mirror_allowed: true,
                world_size_meters: [2.0, 2.0],
                classification_tags: vec!["HOTSPOT".into()],
            },
            structural_profile: StructuralProfile::Flat,
            material_group: "primary".into(),
            weathering_group: "neutral".into(),
            radial_parameters: None,
            enabled: true,
            grid_rect: None,
        }
    }

    fn document() -> TrimSheetDocument {
        let region_id = RegionId::from_bytes([1; 16]);
        let material_id = SourceSetId::from_bytes([2; 16]);
        let topology = AcceptedTopology::new(
            TopologyKind::CustomAtlas,
            TopologySnapshot {
                schema_version: 1,
                canonical_size: PixelSize {
                    width: 4_096,
                    height: 4_096,
                },
                template: None,
            },
            "test.topology.v1".into(),
            vec![region(region_id, IdColor([64, 65, 66]))],
        )
        .expect("topology");
        TrimSheetDocument {
            id: LayoutId::from_bytes([3; 16]),
            document_revision: 1,
            topology_revision: 1,
            appearance_revision: 1,
            topology,
            primary_material: Some(material_id),
            materials: vec![material(material_id)],
            patches: Vec::new(),
            procedural_materials: Vec::new(),
            region_bindings: BTreeMap::from([(
                region_id,
                RegionBinding {
                    region_id,
                    content: ContentReference::InheritPrimaryMaterial,
                    mapping: RegionMapping::default(),
                    variation: VariationSettings::default(),
                    blend: BlendPolicy::default(),
                },
            )]),
            decorations: Vec::new(),
            treatments: Vec::new(),
            sheet_framing: SheetFraming::default(),
            render_settings: RenderSettings::default(),
            generator_provenance: None,
            source_frame: None,
            source_overrides: BTreeMap::new(),
            logical_grid: None,
            partition_provenance: None,
            authored_layout_preset: None,
            authored_layout_instance_id: None,
        }
    }

    #[test]
    fn prompt_1a_same_region_binding_accepts_primary_material_or_patch() {
        let mut document = document();
        document.validate().expect("primary material binding");
        let region_id = document.topology.regions[0].id;
        let patch_id = PatchId::from_bytes([4; 16]);
        document.patches.push(patch(patch_id));
        document
            .region_bindings
            .get_mut(&region_id)
            .expect("binding")
            .content = ContentReference::Patch(patch_id);
        document.validate().expect("patch binding");
    }

    #[test]
    fn algorithm_stage_09_fixed_topology() {
        let registry = TemplateRegistry::built_in().expect("templates");
        let family_ids = [
            "ht.generic_architecture",
            "ht.horizontal_moulding",
            "ht.vertical_trim",
            "ht.hard_surface_panel",
            "ht.detail_heavy_props",
            "ht.radial_accents",
        ];
        for family_id in family_ids {
            let template = registry.get(family_id, "1.0.0").expect("fixed family");
            let first = TrimSheetDocument::from_template(
                LayoutId::new(),
                template,
                vec![material(SourceSetId::from_bytes([7; 16]))],
                Vec::new(),
            )
            .expect("first corpus material");
            let second = TrimSheetDocument::from_template(
                LayoutId::new(),
                template,
                vec![material(SourceSetId::from_bytes([8; 16]))],
                Vec::new(),
            )
            .expect("second corpus material");
            assert_eq!(first.topology.topology_hash, second.topology.topology_hash);
            assert_eq!(first.topology.regions, second.topology.regions);
            assert_eq!(first.topology.kind, TopologyKind::StandardTemplate);

            for edge in [1_024, 2_048, 4_096, 8_192] {
                let compiled = template
                    .compile_for_output(PixelSize {
                        width: edge,
                        height: edge,
                    })
                    .expect("shared boundary compilation");
                assert_eq!(compiled.slots.len(), template.slots.len());
                for compiled_slot in &compiled.slots {
                    let authored = template
                        .slots
                        .iter()
                        .find(|slot| slot.slot_key == compiled_slot.slot_key)
                        .unwrap();
                    assert_eq!(
                        compiled_slot.allocation.x,
                        authored.allocation.x * edge / 4_096
                    );
                    assert_eq!(
                        compiled_slot.allocation.y,
                        authored.allocation.y * edge / 4_096
                    );
                }
            }
        }

        let generic = registry.get("ht.generic_architecture", "1.0.0").unwrap();
        let hard_surface = registry.get("ht.hard_surface_panel", "1.0.0").unwrap();
        assert!(matches!(
            TemplateRegistry::diagnose_compatibility(generic, hard_surface),
            crate::TemplateCompatibilityDiagnostic::ExplicitTopologyChange { .. }
        ));

        let material_id = SourceSetId::from_bytes([9; 16]);
        let original = TrimSheetDocument::from_template(
            LayoutId::new(),
            generic,
            vec![material(material_id)],
            Vec::new(),
        )
        .unwrap();
        let mut appearance_changed = original
            .apply_command(&TrimSheetDocumentCommand::SetOutputResolution {
                output_size: PixelSize {
                    width: 8_192,
                    height: 8_192,
                },
            })
            .unwrap();
        appearance_changed.generator_provenance = Some(GeneratorProvenance {
            generator_id: "source-analysis".into(),
            generator_version: 1,
            recipe_version: 1,
            recipe_hash: DocumentHash([3; 32]),
            seed: 999,
        });
        assert_eq!(
            original.topology.topology_hash,
            appearance_changed.topology.topology_hash
        );
        assert_eq!(
            original.topology.regions,
            appearance_changed.topology.regions
        );

        let mut mutated = original.clone();
        mutated.topology.regions[0].allocation_rect.x += 1;
        mutated.topology.topology_hash =
            hash_serializable(&mutated.topology.topology_hash_inputs()).unwrap();
        assert_eq!(
            mutated.validate(),
            Err(TrimSheetDocumentError::TemplateTopologyMutation)
        );

        let mut forged = generic.clone();
        forged.slots[0].hotspot.x += 1;
        assert_eq!(
            TrimSheetDocument::from_template(
                LayoutId::new(),
                &forged,
                vec![material(SourceSetId::from_bytes([10; 16]))],
                Vec::new(),
            ),
            Err(TrimSheetDocumentError::StandardTemplateRegistryMismatch),
        );
        let mut forged_standard = TrimSheetDocument::from_custom_template(
            LayoutId::new(),
            &forged,
            vec![material(SourceSetId::from_bytes([11; 16]))],
            Vec::new(),
        )
        .expect("custom authoring accepts pinned non-registry geometry");
        forged_standard.topology.kind = TopologyKind::StandardTemplate;
        forged_standard.topology.topology_hash =
            hash_serializable(&forged_standard.topology.topology_hash_inputs()).unwrap();
        assert_eq!(
            forged_standard.validate(),
            Err(TrimSheetDocumentError::StandardTemplateRegistryMismatch),
        );

        let weighted = WeightedTemplateGrammar::Horizontal {
            weights: vec![1, 2],
            children: vec![
                WeightedTemplateGrammar::Slot {
                    slot_key: "one".into(),
                },
                WeightedTemplateGrammar::Slot {
                    slot_key: "two".into(),
                },
            ],
        };
        let compiled = compile_weighted_grammar(&weighted).expect("largest remainder grammar");
        assert_eq!(
            compiled["one"],
            CanonicalRect {
                x: 0,
                y: 0,
                width: 1_365,
                height: 4_096
            }
        );
        assert_eq!(
            compiled["two"],
            CanonicalRect {
                x: 1_365,
                y: 0,
                width: 2_731,
                height: 4_096
            }
        );
    }

    #[test]
    fn prompt_1a_appearance_mapping_does_not_change_topology_hash_inputs() {
        let mut document = document();
        let before_inputs = document.topology_hash_inputs();
        let before_hash = document.appearance_hash().expect("appearance hash");
        let region_id = document.topology.regions[0].id;
        document
            .region_bindings
            .get_mut(&region_id)
            .expect("binding")
            .mapping
            .transform
            .rotation_degrees = 37.5;
        assert_eq!(document.topology_hash_inputs(), before_inputs);
        assert_ne!(
            document.appearance_hash().expect("changed hash"),
            before_hash
        );
        assert_eq!(
            TrimSheetChange::RegionMapping.classification(),
            ChangeClassification::Appearance
        );
        assert_eq!(
            TrimSheetChange::AcceptedRegionGeometry.classification(),
            ChangeClassification::Topology
        );
    }

    #[test]
    fn prompt_1a_validation_rejects_duplicate_geometry_content_and_warp_errors() {
        let document = document();
        let region_id = document.topology.regions[0].id;

        let mut duplicate = document.clone();
        duplicate.topology.regions.push(region(
            RegionId::from_bytes([9; 16]),
            duplicate.topology.regions[0].id_color,
        ));
        assert!(matches!(
            duplicate.validate(),
            Err(TrimSheetDocumentError::DuplicateIdColor(_))
        ));

        let mut bad_rect = document.clone();
        bad_rect.topology.regions[0].allocation_rect.width = 0;
        assert_eq!(
            bad_rect.validate(),
            Err(TrimSheetDocumentError::InvalidAllocationRect(region_id))
        );

        let mut missing = document.clone();
        missing.region_bindings.clear();
        assert_eq!(
            missing.validate(),
            Err(TrimSheetDocumentError::MissingRegionBinding(region_id))
        );

        let mut bad_source = document.clone();
        if let Projection::Crop { bounds, .. } = &mut bad_source
            .region_bindings
            .get_mut(&region_id)
            .expect("binding")
            .mapping
            .projection
        {
            bounds.width = crate::NormalizedScalar::new(0.0).expect("zero");
        }
        assert_eq!(
            bad_source.validate(),
            Err(TrimSheetDocumentError::InvalidSourceGeometry(region_id))
        );

        let mut bad_warp = document;
        bad_warp
            .region_bindings
            .get_mut(&region_id)
            .expect("binding")
            .mapping
            .warps
            .push(WarpOperation {
                id: LayerId::from_bytes([5; 16]),
                version: WARP_OPERATION_VERSION,
                enabled: true,
                operation: SourceWarp::Perspective { strength: f64::NAN },
            });
        assert_eq!(
            bad_warp.validate(),
            Err(TrimSheetDocumentError::InvalidWarp {
                region_id,
                index: 0
            })
        );
    }

    #[test]
    fn source_frame_validation_rejects_stretching_for_frame_and_detached_crop() {
        let source_set_id = SourceSetId::from_bytes([7; 16]);
        let frame = SourceFrame::centered_largest(
            source_set_id,
            crate::OrientedPixelSize {
                width: 8_000,
                height: 4_000,
            },
            [1, 1],
            1,
        );
        let recipe = PartitionRecipe::default_for(
            LogicalGridSpec {
                schema_version: 1,
                width: 1,
                height: 1,
            },
            1,
            5,
        );
        let output_size = PixelSize {
            width: 100,
            height: 100,
        };
        let invalid_frame = frame.with_bounds(NormalizedBounds {
            x: NormalizedScalar::new(0.1).expect("x"),
            y: NormalizedScalar::new(0.1).expect("y"),
            width: NormalizedScalar::new(0.4).expect("width"),
            height: NormalizedScalar::new(0.4).expect("height"),
        });
        assert!(matches!(
            TrimSheetDocument::from_source_frame(
                LayoutId::from_bytes([8; 16]),
                invalid_frame,
                recipe.clone(),
                output_size,
                vec![material(source_set_id)],
                vec![],
            ),
            Err(TrimSheetDocumentError::InvalidSourceFrameAspect)
        ));

        let document = TrimSheetDocument::from_source_frame(
            LayoutId::from_bytes([8; 16]),
            frame,
            recipe,
            output_size,
            vec![material(source_set_id)],
            vec![],
        )
        .expect("valid source-frame document");
        let region_id = document.topology.regions[0].id;
        let detached = document
            .apply_command(&TrimSheetDocumentCommand::DetachSourceCell { region_id })
            .expect("valid square detached crop");
        let invalid_crop = NormalizedBounds {
            x: NormalizedScalar::new(0.1).expect("x"),
            y: NormalizedScalar::new(0.1).expect("y"),
            width: NormalizedScalar::new(0.2).expect("width"),
            height: NormalizedScalar::new(0.1).expect("height"),
        };
        let projection = Projection::Crop {
            bounds: invalid_crop,
            focus: NormalizedPoint::new(0.2, 0.15).expect("focus"),
        };
        assert!(matches!(
            detached.apply_command(&TrimSheetDocumentCommand::SetRegionProjection { region_id, projection }),
            Err(TrimSheetDocumentError::InvalidSourceOverrideAspect(id)) if id == region_id
        ));
    }

    #[test]
    fn source_frame_layout_edits_are_atomic_and_preserve_boundary_ids() {
        let source_set_id = SourceSetId::from_bytes([9; 16]);
        let frame = SourceFrame::centered_largest(
            source_set_id,
            crate::OrientedPixelSize {
                width: 8_000,
                height: 4_000,
            },
            [1, 1],
            1,
        );
        let document = TrimSheetDocument::from_source_frame(
            LayoutId::from_bytes([10; 16]),
            frame,
            PartitionRecipe::default_for(
                LogicalGridSpec {
                    schema_version: 1,
                    width: 8,
                    height: 8,
                },
                2,
                3,
            ),
            PixelSize {
                width: 256,
                height: 256,
            },
            vec![material(source_set_id)],
            vec![],
        )
        .expect("source frame document");
        let region_id = document.topology.regions[0].id;
        let rect = document.topology.regions[0]
            .grid_rect
            .expect("grid rectangle");
        let axis = if rect.width >= 2 {
            PartitionAxis::Vertical
        } else {
            PartitionAxis::Horizontal
        };
        let split = document
            .apply_command(&TrimSheetDocumentCommand::SplitSourceFrameRegion { region_id, axis })
            .expect("split shared source-frame leaf");
        assert_eq!(split.topology.regions.len(), 3);
        assert!(
            split
                .topology
                .regions
                .iter()
                .any(|region| region.id == region_id),
            "the existing leaf keeps its ID"
        );
        split.validate().expect("split keeps complete coverage");
        let sibling_id = split
            .topology
            .regions
            .iter()
            .find(|region| {
                region.id != region_id
                    && mergeable_grid_rect(
                        region.grid_rect.expect("grid"),
                        split
                            .topology
                            .regions
                            .iter()
                            .find(|candidate| candidate.id == region_id)
                            .unwrap()
                            .grid_rect
                            .expect("grid"),
                    )
                    .is_some()
            })
            .expect("new sibling")
            .id;
        let first = split
            .topology
            .regions
            .iter()
            .find(|region| region.id == region_id)
            .unwrap()
            .grid_rect
            .unwrap();
        let second = split
            .topology
            .regions
            .iter()
            .find(|region| region.id == sibling_id)
            .unwrap()
            .grid_rect
            .unwrap();
        let coordinate = match axis {
            PartitionAxis::Vertical => first.x.min(second.x) + 1,
            PartitionAxis::Horizontal => first.y.min(second.y) + 1,
        };
        let moved = split
            .apply_command(&TrimSheetDocumentCommand::MoveSourceFrameBoundary {
                region_id,
                axis,
                coordinate,
            })
            .expect("move shared divider atomically");
        assert!(
            moved
                .topology
                .regions
                .iter()
                .any(|region| region.id == region_id)
        );
        assert!(
            moved
                .topology
                .regions
                .iter()
                .any(|region| region.id == sibling_id)
        );
        moved
            .validate()
            .expect("boundary move keeps coverage and bindings");
        let merged = moved
            .apply_command(&TrimSheetDocumentCommand::MergeSourceFrameRegions {
                region_id,
                sibling_id,
            })
            .expect("remove divider without an empty cell");
        assert_eq!(merged.topology.regions.len(), 2);
        assert!(
            merged
                .topology
                .regions
                .iter()
                .any(|region| region.id == region_id)
        );
        assert!(!merged.region_bindings.contains_key(&sibling_id));
        merged
            .validate()
            .expect("merge keeps complete coverage and bindings");
        let authored_rect = GridRect {
            x: 1,
            y: 1,
            width: 6,
            height: 3,
        };
        let drawn = merged
            .apply_command(&TrimSheetDocumentCommand::DrawSourceFrameRegion {
                grid_rect: authored_rect,
            })
            .expect("directly draw one rectangle across existing leaves");
        assert_eq!(
            drawn.document_revision,
            merged.document_revision + 1,
            "draw is one atomic command"
        );
        assert!(
            drawn
                .topology
                .regions
                .iter()
                .any(|region| region.grid_rect == Some(authored_rect))
        );
        drawn
            .validate()
            .expect("drawn rectangle and clipped remainder remain an exact cover");
        let drawn_id = drawn
            .topology
            .regions
            .iter()
            .find(|region| region.grid_rect == Some(authored_rect))
            .expect("drawn region")
            .id;
        let binding_before_resize = drawn
            .region_bindings
            .get(&drawn_id)
            .expect("drawn binding")
            .content
            .clone();
        let resized_rect = GridRect {
            x: 2,
            y: 1,
            width: 5,
            height: 4,
        };
        let resized = drawn
            .apply_command(&TrimSheetDocumentCommand::ResizeSourceFrameRegion {
                region_id: drawn_id,
                grid_rect: resized_rect,
            })
            .expect("resize transfers only gained and released ownership");
        assert_eq!(
            resized.document_revision,
            drawn.document_revision + 1,
            "resize is one atomic command"
        );
        assert_eq!(
            resized
                .topology
                .regions
                .iter()
                .filter(|region| region.id == drawn_id)
                .count(),
            1,
            "selected identity is never replaced or duplicated"
        );
        assert_eq!(
            resized
                .topology
                .regions
                .iter()
                .find(|region| region.id == drawn_id)
                .expect("resized region")
                .grid_rect,
            Some(resized_rect)
        );
        assert_eq!(
            resized
                .region_bindings
                .get(&drawn_id)
                .expect("resized binding")
                .content,
            binding_before_resize,
            "selected content ownership survives resize"
        );
        resized
            .validate()
            .expect("resized region and redistributed neighbors remain an exact cover");
    }

    #[test]
    fn source_frame_resize_does_not_turn_released_strips_into_cell_staircases() {
        let source_set_id = SourceSetId::from_bytes([19; 16]);
        let frame = SourceFrame::centered_largest(
            source_set_id,
            crate::OrientedPixelSize {
                width: 8_000,
                height: 4_000,
            },
            [1, 1],
            1,
        );
        let document = TrimSheetDocument::from_source_frame(
            LayoutId::from_bytes([20; 16]),
            frame,
            PartitionRecipe::default_for(
                LogicalGridSpec {
                    schema_version: 1,
                    width: 64,
                    height: 64,
                },
                63,
                7,
            ),
            PixelSize {
                width: 512,
                height: 512,
            },
            vec![material(source_set_id)],
            vec![],
        )
        .expect("dense source-frame document");
        let selected = document
            .topology
            .regions
            .iter()
            .filter_map(|region| region.grid_rect.map(|rect| (region.id, rect)))
            .filter(|(_, rect)| rect.width > 2 && rect.height > 2)
            .max_by_key(|(_, rect)| u64::from(rect.width) * u64::from(rect.height))
            .expect("a resizable major region");
        let target = GridRect {
            x: selected.1.x + 1,
            y: selected.1.y + 1,
            width: selected.1.width - 2,
            height: selected.1.height - 2,
        };
        let resized = document
            .apply_command(&TrimSheetDocumentCommand::ResizeSourceFrameRegion {
                region_id: selected.0,
                grid_rect: target,
            })
            .expect("released strips transfer atomically");
        assert_eq!(
            resized
                .topology
                .regions
                .iter()
                .find(|region| region.id == selected.0)
                .and_then(|region| region.grid_rect),
            Some(target)
        );
        assert!(
            resized.topology.regions.len() <= document.topology.regions.len() + 12,
            "a four-strip contraction must not create a cell-level staircase"
        );
        resized
            .validate()
            .expect("compact resize remains an exact cover");
    }

    #[test]
    fn prompt_1a_serialization_is_deterministic_and_round_trips() {
        let document = document();
        document.validate().expect("valid document");
        let first = serde_json::to_string(&document).expect("serialize");
        let second = serde_json::to_string(&document).expect("serialize again");
        assert_eq!(first, second);
        assert_eq!(
            serde_json::from_str::<TrimSheetDocument>(&first).expect("deserialize"),
            document
        );
    }
}
