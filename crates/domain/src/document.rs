use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    Channel, DecorationBinding, IdColor, LayerId, LayoutId, NormalizedBounds, NormalizedPoint,
    Patch, PatchId, PixelSize, RegionId, SourceBlend, SourceSamplingMode,
    SourceSetId, SourceWarp, StructuralProfile, TemplateDefinition, TemplateFitSemantics, TemplateRegistry,
    TemplateSlotRole,
    TemplateSnapshot,
    GridRect, LogicalGridSpec, PartitionProvenance, PartitionRecipe, RegionSourceOverride, SourceFrame,
    layout::source_warp_is_valid,
    templates::{
        CanonicalRect, RadialParameters, TemplateSourceMapping,
    },
};

pub type TrimSheetId = LayoutId;

pub const TRIM_SHEET_DOCUMENT_SCHEMA_VERSION: u32 = 1;
pub const WARP_OPERATION_VERSION: u16 = 1;
pub const MAX_MAPPING_MAGNITUDE: f64 = 16.0;

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
}

impl From<RadialParameters> for RadialMappingSettings {
    fn from(value: RadialParameters) -> Self {
        Self {
            center_x: value.center_x,
            center_y: value.center_y,
            inner_radius: value.inner_radius,
            outer_radius: value.outer_radius,
            falloff: 0.5,
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
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
                        Channel::BaseColor | Channel::RegionId | Channel::MaterialId
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
                let Some(region) = self.topology.regions.iter().find(|region| region.id == *region_id) else {
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
                    return Err(TrimSheetDocumentError::InvalidSourceOverrideAspect(*region_id));
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
        if matches!(self.topology.kind, TopologyKind::StandardTemplate | TopologyKind::CustomTemplate) {
            let snapshot = self.topology.snapshot.template.as_ref()
                .ok_or(TrimSheetDocumentError::InvalidTemplateSnapshot)?;
            let template: TemplateDefinition = serde_json::from_str(&snapshot.snapshot_json)
                .map_err(|_| TrimSheetDocumentError::InvalidTemplateSnapshot)?;
            let canonical_snapshot = template.snapshot()
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
                    .get(&template.identity.template_id, &template.identity.template_version)
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
            .get(&template.identity.template_id, &template.identity.template_version)
            .ok_or(TrimSheetDocumentError::StandardTemplateRegistryMismatch)?;
        if built_in != template {
            return Err(TrimSheetDocumentError::StandardTemplateRegistryMismatch);
        }
        Self::from_pinned_template(id, template, materials, patches, TopologyKind::StandardTemplate)
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
            .map_err(|_| TrimSheetDocumentError::InvalidTemplateSnapshot)?;
        let output_x = crate::resolve_boundaries(0, output_size.width, recipe.grid.width);
        let output_y = crate::resolve_boundaries(0, output_size.height, recipe.grid.height);
        let regions = partitions.iter().map(|partition| {
            let rect = CanonicalRect {
                x: output_x[partition.grid_rect.x as usize], y: output_y[partition.grid_rect.y as usize],
                width: output_x[(partition.grid_rect.x + partition.grid_rect.width) as usize] - output_x[partition.grid_rect.x as usize],
                height: output_y[(partition.grid_rect.y + partition.grid_rect.height) as usize] - output_y[partition.grid_rect.y as usize],
            };
            let orientation = if rect.width > rect.height { RegionOrientation::Horizontal }
                else if rect.height > rect.width { RegionOrientation::Vertical } else { RegionOrientation::Unspecified };
            RegionDefinition { id: partition.id, display_name: format!("Region {:03}", partition.grid_rect.y * recipe.grid.width + partition.grid_rect.x),
                id_color: IdColor::for_region(partition.id), allocation_rect: rect, hotspot_rect: rect,
                role: TemplateSlotRole::Planar, orientation,
                uv_fit: UvFitPolicy { kind: UvFitKind::Rectangular, fit_axis: FitAxis::Automatic,
                    keep_proportion: true, allowed_rotations: vec![QuarterTurn::Zero], mirror_allowed: false,
                    world_size_meters: [f64::from(rect.width.max(1)), f64::from(rect.height.max(1))], classification_tags: vec!["SOURCE_FRAME".into()] },
                structural_profile: StructuralProfile::Flat, material_group: "primary".into(),
                weathering_group: "neutral".into(), radial_parameters: None, enabled: true,
                grid_rect: Some(partition.grid_rect) }
        }).collect::<Vec<_>>();
        let topology = AcceptedTopology::new(TopologyKind::CustomAtlas,
            TopologySnapshot { schema_version: TRIM_SHEET_DOCUMENT_SCHEMA_VERSION, canonical_size: output_size, template: None },
            format!("source-frame:{}", source_frame.identity.0.iter().map(|byte| format!("{byte:02x}")).collect::<String>()), regions)?;
        let provenance = PartitionProvenance { schema_version: crate::PARTITION_RECIPE_SCHEMA_VERSION,
            recipe: recipe.clone(), recipe_hash: recipe.hash(), accepted_region_ids: topology.regions.iter().map(|region| region.id).collect() };
        let document = Self { id, document_revision: 1, topology_revision: 1, appearance_revision: 1, topology,
            primary_material: materials.first().map(|material| material.id), materials, patches,
            procedural_materials: Vec::new(), region_bindings: partitions.iter().map(|partition| (partition.id, RegionBinding {
                region_id: partition.id, content: ContentReference::InheritPrimaryMaterial, mapping: RegionMapping::default(),
                variation: VariationSettings::default(), blend: BlendPolicy::default() })).collect(), decorations: Vec::new(), treatments: Vec::new(),
            sheet_framing: SheetFraming::default(), render_settings: RenderSettings { output_size, ..RenderSettings::default() },
            generator_provenance: None, source_frame: Some(source_frame), source_overrides: BTreeMap::new(), logical_grid: Some(recipe.grid), partition_provenance: Some(provenance) };
        document.validate()?;
        Ok(document)
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
            bindings.insert(
                region_id,
                RegionBinding {
                    region_id,
                    content: ContentReference::InheritPrimaryMaterial,
                    mapping: RegionMapping {
                        radial: slot.radial_parameters.map(Into::into),
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
        Self::from_pinned_template(id, template, materials, patches, TopologyKind::CustomTemplate)
    }

    /// Applies one accepted command to a clone, validates it, and advances revisions exactly once.
    pub fn apply_command(
        &self,
        command: &TrimSheetDocumentCommand,
    ) -> Result<Self, TrimSheetDocumentError> {
        let mut next = self.clone();
        match command {
            TrimSheetDocumentCommand::SetPrimaryMaterial { material_id } => {
                next.primary_material = Some(*material_id);
            }
            TrimSheetDocumentCommand::SetRegionContent { region_id, content } => {
                next.region_bindings
                    .get_mut(region_id)
                    .ok_or(TrimSheetDocumentError::MissingRegionBinding(*region_id))?
                    .content = content.clone();
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
                    next.source_overrides.insert(*region_id, RegionSourceOverride::new(*bounds));
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
                let frame = next.source_frame.as_ref().ok_or(TrimSheetDocumentError::InvalidSourceFrame)?;
                next.source_frame = Some(frame.with_bounds(*bounds));
            }
            TrimSheetDocumentCommand::DetachSourceCell { region_id } => {
                let frame = next.source_frame.as_ref().ok_or(TrimSheetDocumentError::InvalidSourceFrame)?;
                let grid = next.logical_grid.ok_or(TrimSheetDocumentError::InvalidSourceFrame)?;
                let region = next.topology.regions.iter().find(|region| region.id == *region_id)
                    .ok_or(TrimSheetDocumentError::MissingRegionBinding(*region_id))?;
                let rect = region.grid_rect.ok_or(TrimSheetDocumentError::InvalidSourceFrame)?;
                let bounds = source_frame_region_bounds(frame, grid, rect);
                next.source_overrides.insert(*region_id, RegionSourceOverride::new(bounds));
                let binding = next.region_bindings.get_mut(region_id).ok_or(TrimSheetDocumentError::MissingRegionBinding(*region_id))?;
                binding.mapping.projection = Projection::Crop { bounds, focus: NormalizedPoint::new(bounds.x.get() + bounds.width.get() * 0.5, bounds.y.get() + bounds.height.get() * 0.5).expect("override focus") };
                binding.mapping.source_crop_intent = Some(SourceCropIntent::Authored);
            }
            TrimSheetDocumentCommand::ResetSourceCell { region_id } => {
                next.source_overrides.remove(region_id);
                let binding = next.region_bindings.get_mut(region_id).ok_or(TrimSheetDocumentError::MissingRegionBinding(*region_id))?;
                binding.mapping = RegionMapping::default();
            }
            TrimSheetDocumentCommand::SetRegionRadial { region_id, radial } => {
                let region = next.topology.regions.iter().find(|region| region.id == *region_id)
                    .ok_or(TrimSheetDocumentError::MissingRegionBinding(*region_id))?;
                if region.role != TemplateSlotRole::Radial {
                    return Err(TrimSheetDocumentError::InvalidRadialMapping(*region_id));
                }
                next.region_bindings
                    .get_mut(region_id)
                    .ok_or(TrimSheetDocumentError::MissingRegionBinding(*region_id))?
                    .mapping
                    .radial = Some(*radial);
            }
            TrimSheetDocumentCommand::SetOutputResolution { output_size } => {
                next.render_settings.output_size = *output_size;
            }
        }
        next.document_revision = next.document_revision.saturating_add(1);
        next.appearance_revision = next.document_revision;
        next.validate()?;
        Ok(next)
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
            let slot = template.slots.iter().find(|slot| &slot.slot_key == slot_key)
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
                id: deterministic_region_id(&template.identity.compatibility_key, &slot.compatibility_key),
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
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum TrimSheetDocumentCommand {
    SetPrimaryMaterial {
        material_id: SourceSetId,
    },
    SetRegionContent {
        region_id: RegionId,
        content: ContentReference,
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
            || !(0.0..=1.0).contains(&radial.center_x)
            || !(0.0..=1.0).contains(&radial.center_y)
            || radial.inner_radius < 0.0
            || radial.outer_radius <= radial.inner_radius
            || radial.outer_radius > 2.0
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
    if !width.is_finite() || !height.is_finite() || !expected_width.is_finite() || !expected_height.is_finite()
        || width <= 0.0 || height <= 0.0 || expected_width <= 0.0 || expected_height <= 0.0 {
        return false;
    }
    let left = width * expected_height;
    let right = height * expected_width;
    (left - right).abs() <= 1e-9 * left.abs().max(right.abs()).max(1.0)
}

fn source_frame_region_bounds(frame: &SourceFrame, grid: LogicalGridSpec, rect: GridRect) -> NormalizedBounds {
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
    let right = f64::from(source_x[(rect.x + rect.width) as usize]) / f64::from(frame.oriented_dimensions.width);
    let bottom = f64::from(source_y[(rect.y + rect.height) as usize]) / f64::from(frame.oriented_dimensions.height);
    NormalizedBounds {
        x: crate::NormalizedScalar::new(x).expect("resolved source x"),
        y: crate::NormalizedScalar::new(y).expect("resolved source y"),
        width: crate::NormalizedScalar::new(right - x).expect("resolved source width"),
        height: crate::NormalizedScalar::new(bottom - y).expect("resolved source height"),
    }
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
    #[error("a pinned standard or custom template topology was mutated")]
    TemplateTopologyMutation,
    #[error("standard template does not exactly match its shipped registry definition")]
    StandardTemplateRegistryMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        NormalizedScalar, PatchGeometry, PatchProperties, RectificationSettings, SourceId, TemplateRegistry,
        WeightedTemplateGrammar, compile_weighted_grammar,
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
                LayoutId::new(), template, vec![material(SourceSetId::from_bytes([7; 16]))], Vec::new(),
            ).expect("first corpus material");
            let second = TrimSheetDocument::from_template(
                LayoutId::new(), template, vec![material(SourceSetId::from_bytes([8; 16]))], Vec::new(),
            ).expect("second corpus material");
            assert_eq!(first.topology.topology_hash, second.topology.topology_hash);
            assert_eq!(first.topology.regions, second.topology.regions);
            assert_eq!(first.topology.kind, TopologyKind::StandardTemplate);

            for edge in [1_024, 2_048, 4_096, 8_192] {
                let compiled = template.compile_for_output(PixelSize { width: edge, height: edge })
                    .expect("shared boundary compilation");
                assert_eq!(compiled.slots.len(), template.slots.len());
                for compiled_slot in &compiled.slots {
                    let authored = template.slots.iter().find(|slot| slot.slot_key == compiled_slot.slot_key).unwrap();
                    assert_eq!(compiled_slot.allocation.x, authored.allocation.x * edge / 4_096);
                    assert_eq!(compiled_slot.allocation.y, authored.allocation.y * edge / 4_096);
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
            LayoutId::new(), generic, vec![material(material_id)], Vec::new(),
        ).unwrap();
        let mut appearance_changed = original.apply_command(&TrimSheetDocumentCommand::SetOutputResolution {
            output_size: PixelSize { width: 8_192, height: 8_192 },
        }).unwrap();
        appearance_changed.generator_provenance = Some(GeneratorProvenance {
            generator_id: "source-analysis".into(), generator_version: 1, recipe_version: 1,
            recipe_hash: DocumentHash([3; 32]), seed: 999,
        });
        assert_eq!(original.topology.topology_hash, appearance_changed.topology.topology_hash);
        assert_eq!(original.topology.regions, appearance_changed.topology.regions);

        let mut mutated = original.clone();
        mutated.topology.regions[0].allocation_rect.x += 1;
        mutated.topology.topology_hash = hash_serializable(&mutated.topology.topology_hash_inputs()).unwrap();
        assert_eq!(mutated.validate(), Err(TrimSheetDocumentError::TemplateTopologyMutation));

        let mut forged = generic.clone();
        forged.slots[0].hotspot.x += 1;
        assert_eq!(
            TrimSheetDocument::from_template(
                LayoutId::new(), &forged, vec![material(SourceSetId::from_bytes([10; 16]))], Vec::new(),
            ),
            Err(TrimSheetDocumentError::StandardTemplateRegistryMismatch),
        );
        let mut forged_standard = TrimSheetDocument::from_custom_template(
            LayoutId::new(), &forged, vec![material(SourceSetId::from_bytes([11; 16]))], Vec::new(),
        ).expect("custom authoring accepts pinned non-registry geometry");
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
                WeightedTemplateGrammar::Slot { slot_key: "one".into() },
                WeightedTemplateGrammar::Slot { slot_key: "two".into() },
            ],
        };
        let compiled = compile_weighted_grammar(&weighted).expect("largest remainder grammar");
        assert_eq!(compiled["one"], CanonicalRect { x: 0, y: 0, width: 1_365, height: 4_096 });
        assert_eq!(compiled["two"], CanonicalRect { x: 1_365, y: 0, width: 2_731, height: 4_096 });
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
            crate::OrientedPixelSize { width: 8_000, height: 4_000 },
            [1, 1],
            1,
        );
        let recipe = PartitionRecipe::default_for(
            LogicalGridSpec { schema_version: 1, width: 1, height: 1 },
            1,
            5,
        );
        let output_size = PixelSize { width: 100, height: 100 };
        let invalid_frame = frame.with_bounds(NormalizedBounds {
            x: NormalizedScalar::new(0.1).expect("x"),
            y: NormalizedScalar::new(0.1).expect("y"),
            width: NormalizedScalar::new(0.4).expect("width"),
            height: NormalizedScalar::new(0.4).expect("height"),
        });
        assert!(matches!(
            TrimSheetDocument::from_source_frame(
                LayoutId::from_bytes([8; 16]), invalid_frame, recipe.clone(), output_size,
                vec![material(source_set_id)], vec![],
            ),
            Err(TrimSheetDocumentError::InvalidSourceFrameAspect)
        ));

        let document = TrimSheetDocument::from_source_frame(
            LayoutId::from_bytes([8; 16]), frame, recipe, output_size,
            vec![material(source_set_id)], vec![],
        ).expect("valid source-frame document");
        let region_id = document.topology.regions[0].id;
        let detached = document.apply_command(&TrimSheetDocumentCommand::DetachSourceCell { region_id })
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
