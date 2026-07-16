use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    Channel, DecorationBinding, IdColor, LayerId, LayoutId, NormalizedBounds, NormalizedPoint,
    NormalizedScalar, Patch, PatchId, PixelSize, RegionId, SourceBlend, SourceSamplingMode,
    SourceSetId, SourceWarp, StructuralProfile, TemplateDefinition, TemplateSlotRole,
    TemplateSnapshot,
    layout::source_warp_is_valid,
    templates::{
        CanonicalRect, TemplateSlot, TemplateSourceAddressMode, TemplateSourceMapping,
        TemplateSourceRect,
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionMapping {
    pub projection: Projection,
    pub warps: Vec<WarpOperation>,
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
            warps: Vec::new(),
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
    pub enabled: bool,
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
    pub id_color: IdColor,
    pub allocation_rect: CanonicalRect,
    pub hotspot_rect: CanonicalRect,
    pub role: TemplateSlotRole,
    pub orientation: RegionOrientation,
    pub uv_fit: UvFitPolicy,
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
                    id_color: region.id_color,
                    allocation_rect: region.allocation_rect,
                    hotspot_rect: region.hotspot_rect,
                    role: region.role,
                    orientation: region.orientation,
                    uv_fit: region.uv_fit.clone(),
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutGridSettings {
    pub columns: u16,
    pub rows: u16,
    pub padding: u32,
}

impl Default for LayoutGridSettings {
    fn default() -> Self {
        Self { columns: 32, rows: 32, padding: 8 }
    }
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
    #[serde(default)]
    pub layout_grid: LayoutGridSettings,
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
        if !(8..=64).contains(&self.layout_grid.columns)
            || !(8..=64).contains(&self.layout_grid.rows)
            || self.layout_grid.padding > self.topology.snapshot.canonical_size.width / 4
        {
            return Err(TrimSheetDocumentError::InvalidLayoutGrid);
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
        let template_snapshot = template
            .snapshot()
            .map_err(|_| TrimSheetDocumentError::InvalidTemplateSnapshot)?;
        let mut regions = Vec::with_capacity(template.slots.len());
        let mut bindings = BTreeMap::new();
        let layout_grid = LayoutGridSettings::default();
        let packed = semantic_pack_template(template, layout_grid);
        for slot_key in &template.stable_order {
            let slot = template
                .slots
                .iter()
                .find(|slot| &slot.slot_key == slot_key)
                .ok_or(TrimSheetDocumentError::InvalidTemplateSnapshot)?;
            let rect = packed.get(slot_key).copied().unwrap_or(slot.allocation);
            let requested_padding = slot.packing_intent.map_or(layout_grid.padding, |intent| intent.padding);
            let padding = requested_padding.min(rect.width.saturating_sub(1) / 2)
                .min(rect.height.saturating_sub(1) / 2);
            let hotspot_rect = CanonicalRect {
                x: rect.x + padding,
                y: rect.y + padding,
                width: rect.width - padding * 2,
                height: rect.height - padding * 2,
            };
            let region_id = deterministic_region_id(
                &template.identity.compatibility_key,
                &slot.compatibility_key,
            );
            let (uv_kind, fit_axis) = match slot.role {
                TemplateSlotRole::Planar => (UvFitKind::Rectangular, FitAxis::Automatic),
                TemplateSlotRole::RepeatingStrip => (
                    UvFitKind::Strip,
                    if rect.width >= rect.height {
                        FitAxis::Vertical
                    } else {
                        FitAxis::Horizontal
                    },
                ),
                TemplateSlotRole::UniqueDetail => (UvFitKind::Unique, FitAxis::None),
                TemplateSlotRole::TrimCap => (UvFitKind::Cap, FitAxis::Automatic),
                TemplateSlotRole::Radial => (UvFitKind::Radial, FitAxis::None),
            };
            let orientation = if rect.width > rect.height {
                RegionOrientation::Horizontal
            } else if rect.height > rect.width {
                RegionOrientation::Vertical
            } else {
                RegionOrientation::Unspecified
            };
            regions.push(RegionDefinition {
                id: region_id,
                display_name: title_from_key(&slot.slot_key),
                id_color: slot.id_color,
                allocation_rect: rect,
                hotspot_rect,
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
                enabled: true,
            });

            bindings.insert(
                region_id,
                RegionBinding {
                    region_id,
                    content: ContentReference::InheritPrimaryMaterial,
                    mapping: template_region_mapping(slot.source_mapping),
                    variation: VariationSettings::default(),
                    blend: BlendPolicy::default(),
                },
            );
        }

        let topology = AcceptedTopology::new(
            TopologyKind::GeneratedTemplate,
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
            layout_grid,
        };
        document.validate()?;
        Ok(document)
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
                next.region_bindings
                    .get_mut(region_id)
                    .ok_or(TrimSheetDocumentError::MissingRegionBinding(*region_id))?
                    .mapping
                    .projection = projection.clone();
            }
            TrimSheetDocumentCommand::SetOutputResolution { output_size } => {
                next.render_settings.output_size = *output_size;
            }
            TrimSheetDocumentCommand::SetLayoutGrid { settings } => {
                if !(8..=64).contains(&settings.columns) || !(8..=64).contains(&settings.rows) {
                    return Err(TrimSheetDocumentError::InvalidLayoutGrid);
                }
                let snapshot = next.topology.snapshot.template.as_ref()
                    .ok_or(TrimSheetDocumentError::InvalidTemplateSnapshot)?;
                let template: TemplateDefinition = serde_json::from_str(&snapshot.snapshot_json)
                    .map_err(|_| TrimSheetDocumentError::InvalidTemplateSnapshot)?;
                let packed = semantic_pack_template(&template, *settings);
                for region in &mut next.topology.regions {
                    let slot = template.stable_order.iter().find_map(|key| {
                        let slot = template.slots.iter().find(|slot| &slot.slot_key == key)?;
                        (deterministic_region_id(&template.identity.compatibility_key, &slot.compatibility_key) == region.id).then_some(slot)
                    }).ok_or(TrimSheetDocumentError::InvalidTemplateSnapshot)?;
                    let rect = packed[&slot.slot_key];
                    let padding = settings.padding.min(rect.width.saturating_sub(1) / 2)
                        .min(rect.height.saturating_sub(1) / 2);
                    region.allocation_rect = rect;
                    region.hotspot_rect = inset_rect(rect, padding);
                }
                next.layout_grid = *settings;
                next.topology.kind = TopologyKind::GeneratedTemplate;
                next.topology.topology_hash = hash_serializable(&next.topology.topology_hash_inputs())?;
            }
            TrimSheetDocumentCommand::SetRegionDestination { region_id, allocation_rect, padding } => {
                let region = next.topology.regions.iter_mut().find(|region| region.id == *region_id)
                    .ok_or(TrimSheetDocumentError::MissingRegionBinding(*region_id))?;
                region.allocation_rect = *allocation_rect;
                region.hotspot_rect = inset_rect(*allocation_rect, *padding);
                next.topology.topology_hash = hash_serializable(&next.topology.topology_hash_inputs())?;
            }
        }
        next.document_revision = next.document_revision.saturating_add(1);
        if matches!(command, TrimSheetDocumentCommand::SetLayoutGrid { .. } | TrimSheetDocumentCommand::SetRegionDestination { .. }) {
            next.topology_revision = next.document_revision;
        } else {
            next.appearance_revision = next.document_revision;
        }
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

fn template_region_mapping(mapping: TemplateSourceMapping) -> RegionMapping {
    RegionMapping {
        projection: Projection::Crop {
            bounds: normalized_bounds(mapping.crop),
            focus: NormalizedPoint::new(
                canonical_decimal(mapping.crop.x + mapping.crop.width * 0.5),
                canonical_decimal(mapping.crop.y + mapping.crop.height * 0.5),
            )
            .expect("template crop focus is normalized"),
        },
        warps: Vec::new(),
        transform: MappingTransform::default(),
        address_mode: template_address_mode(mapping.address_mode),
        sampling: SamplingPolicy::default(),
    }
}

fn canonical_decimal(value: f64) -> f64 {
    (value * 1_000_000_000.0).round() / 1_000_000_000.0
}

fn template_address_mode(mode: TemplateSourceAddressMode) -> AddressMode {
    match mode {
        TemplateSourceAddressMode::Clamp => AddressMode::Clamp,
        TemplateSourceAddressMode::Repeat => AddressMode::Repeat,
        TemplateSourceAddressMode::MirroredRepeat => AddressMode::MirroredRepeat,
    }
}

fn normalized_bounds(rect: TemplateSourceRect) -> NormalizedBounds {
    NormalizedBounds {
        x: NormalizedScalar::new(rect.x).expect("template crop x is normalized"),
        y: NormalizedScalar::new(rect.y).expect("template crop y is normalized"),
        width: NormalizedScalar::new(rect.width).expect("template crop width is normalized"),
        height: NormalizedScalar::new(rect.height).expect("template crop height is normalized"),
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
    SetOutputResolution {
        output_size: PixelSize,
    },
    SetLayoutGrid {
        settings: LayoutGridSettings,
    },
    SetRegionDestination {
        region_id: RegionId,
        allocation_rect: CanonicalRect,
        padding: u32,
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

#[derive(Clone, Copy)]
struct SemanticPackItem<'a> {
    slot: &'a TemplateSlot,
    stable_index: usize,
    weight: f64,
}

/// Semantic bands first, followed by a deterministic weighted rectangle partition. The authored
/// allocation contributes only preferred area/aspect; source crops never participate in packing.
fn semantic_pack_template(template: &TemplateDefinition, settings: LayoutGridSettings) -> BTreeMap<String, CanonicalRect> {
    let columns = u32::from(settings.columns);
    let rows = u32::from(settings.rows);
    let mut items: Vec<_> = template.stable_order.iter().enumerate().filter_map(|(stable_index, key)| {
        template.slots.iter().find(|slot| &slot.slot_key == key).map(|slot| SemanticPackItem {
            slot,
            stable_index,
            weight: slot.packing_intent.map_or(
                f64::from(slot.allocation.width) * f64::from(slot.allocation.height),
                |intent| intent.area_weight,
            ),
        })
    }).collect();
    items.sort_by(|left, right| {
        semantic_priority(left.slot.role).cmp(&semantic_priority(right.slot.role))
            .then_with(|| right.weight.total_cmp(&left.weight))
            .then_with(|| left.stable_index.cmp(&right.stable_index))
    });

    let mut occupied = vec![false; (columns * rows) as usize];
    let mut result = BTreeMap::new();
    for item in items {
        let (mut width, mut height) = semantic_footprint(item.slot, columns, rows);
        let allow_rotation = item.slot.packing_intent.is_some_and(|intent| intent.allow_rotation);
        let placed = loop {
            let mut candidate = find_grid_space(&occupied, columns, rows, width, height)
                .map(|(x, y)| (x, y, width, height));
            if candidate.is_none() && allow_rotation && width != height {
                candidate = find_grid_space(&occupied, columns, rows, height, width)
                    .map(|(x, y)| (x, y, height, width));
            }
            if candidate.is_some() || (width == 1 && height == 1) { break candidate; }
            if width >= height && width > 1 { width = width.div_ceil(2); }
            else if height > 1 { height = height.div_ceil(2); }
        };
        let Some((x, y, width, height)) = placed else { continue };
        mark_grid(&mut occupied, columns, x, y, width, height);
        result.insert(item.slot.slot_key.clone(), grid_rect(
            x, y, width, height, columns, rows, template.canonical_width, template.canonical_height,
        ));
    }
    result
}

fn semantic_priority(role: TemplateSlotRole) -> u8 {
    match role {
        TemplateSlotRole::Planar => 0,
        TemplateSlotRole::TrimCap => 1,
        TemplateSlotRole::UniqueDetail => 2,
        TemplateSlotRole::Radial => 3,
        TemplateSlotRole::RepeatingStrip => 4,
    }
}

fn semantic_footprint(slot: &TemplateSlot, columns: u32, rows: u32) -> (u32, u32) {
    let scale_x = |units: u32| (units * columns).div_ceil(32).clamp(1, columns);
    let scale_y = |units: u32| (units * rows).div_ceil(32).clamp(1, rows);
    let aspect = slot.packing_intent.map_or(
        f64::from(slot.allocation.width) / f64::from(slot.allocation.height),
        |intent| intent.preferred_aspect,
    );
    match slot.role {
        TemplateSlotRole::Planar => if aspect >= 1.5 { (scale_x(12), scale_y(6)) } else { (scale_x(8), scale_y(8)) },
        TemplateSlotRole::RepeatingStrip => if aspect >= 1.0 { (scale_x(16), scale_y(1)) } else { (scale_x(1), scale_y(16)) },
        TemplateSlotRole::Radial => (scale_x(4).min(scale_y(4)), scale_x(4).min(scale_y(4))),
        TemplateSlotRole::TrimCap => (scale_x(4), scale_y(4)),
        TemplateSlotRole::UniqueDetail => if aspect >= 1.5 { (scale_x(4), scale_y(2)) } else { (scale_x(4), scale_y(4)) },
    }
}

fn find_grid_space(occupied: &[bool], columns: u32, rows: u32, width: u32, height: u32) -> Option<(u32, u32)> {
    if width > columns || height > rows { return None; }
    for y in 0..=rows - height {
        for x in 0..=columns - width {
            if (y..y + height).all(|row| (x..x + width).all(|column| !occupied[(row * columns + column) as usize])) {
                return Some((x, y));
            }
        }
    }
    None
}

fn mark_grid(occupied: &mut [bool], columns: u32, x: u32, y: u32, width: u32, height: u32) {
    for row in y..y + height {
        for column in x..x + width {
            occupied[(row * columns + column) as usize] = true;
        }
    }
}

fn rects_overlap(left: CanonicalRect, right: CanonicalRect) -> bool {
    left.x < right.x.saturating_add(right.width)
        && right.x < left.x.saturating_add(left.width)
        && left.y < right.y.saturating_add(right.height)
        && right.y < left.y.saturating_add(left.height)
}

fn grid_rect(x: u32, y: u32, width: u32, height: u32, columns: u32, rows: u32, canonical_width: u32, canonical_height: u32) -> CanonicalRect {
    let left = canonical_width * x / columns;
    let top = canonical_height * y / rows;
    let right = canonical_width * (x + width) / columns;
    let bottom = canonical_height * (y + height) / rows;
    CanonicalRect { x: left, y: top, width: right - left, height: bottom - top }
}

fn inset_rect(rect: CanonicalRect, padding: u32) -> CanonicalRect {
    let padding = padding.min(rect.width.saturating_sub(1) / 2).min(rect.height.saturating_sub(1) / 2);
    CanonicalRect { x: rect.x + padding, y: rect.y + padding, width: rect.width - padding * 2, height: rect.height - padding * 2 }
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

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum TrimSheetDocumentError {
    #[error("canonical topology size or schema version is invalid")]
    InvalidCanonicalSize,
    #[error("topology compatibility key is empty")]
    InvalidCompatibilityKey,
    #[error("document, topology, and appearance revisions are inconsistent")]
    InvalidRevisions,
    #[error("layout grid settings are invalid")]
    InvalidLayoutGrid,
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
    #[error("deterministic hash serialization failed")]
    HashSerialization,
    #[error("the accepted template snapshot is invalid")]
    InvalidTemplateSnapshot,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PatchGeometry, PatchProperties, RectificationSettings, SourceId};

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
            enabled: true,
        }
    }

    fn document() -> TrimSheetDocument {
        let region_id = RegionId::from_bytes([1; 16]);
        let material_id = SourceSetId::from_bytes([2; 16]);
        let topology = AcceptedTopology::new(
            TopologyKind::StandardTemplate,
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
            layout_grid: LayoutGridSettings::default(),
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
