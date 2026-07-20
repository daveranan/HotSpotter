use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use thiserror::Error;

use hot_trimmer_domain::{
    ContentDigest, DocumentHash, EdgeEligibility, ManualRegionRole, MappingTransform,
    MaterialChannelRole, OrientedPixelSize, PatchId, PixelBounds, PixelSize, RadialMappingSettings,
    RegionContinuity, RegionId, RegionSampling, SourceSetId, StructuralProfile,
};
use hot_trimmer_placement_solver::SamplingPlan;

pub const COMPILED_ATLAS_PLAN_SCHEMA_VERSION: u16 = 1;
pub const COMPILED_ATLAS_ALGORITHM_VERSION: &str = "gpu-prompt-1-cpu-stage14-v1";

#[derive(Debug, Error)]
pub enum CompiledAtlasPlanValidationError {
    #[error("output size is zero: {width}x{height}")]
    ZeroOutputSize { width: u32, height: u32 },

    #[error("region {region_id} references missing source {source_set_id}/{source_id}")]
    MissingSource {
        region_id: RegionId,
        source_set_id: SourceSetId,
        source_id: String,
    },

    #[error("plan contains duplicate compact index {compact_index} for region {region_id}")]
    DuplicateCompactIndex {
        compact_index: u32,
        region_id: RegionId,
        previous_region_id: RegionId,
    },

    #[error("plan contains duplicate region id {region_id}")]
    DuplicateRegionId {
        region_id: RegionId,
        previous_region_id: RegionId,
    },

    #[error(
        "region {region_id} source crop is zero or out of bounds: {crop:?} against source {source_width}x{source_height}"
    )]
    SourceCropOutOfBounds {
        region_id: RegionId,
        crop: SourcePixelRect,
        source_width: u32,
        source_height: u32,
    },

    #[error(
        "region {region_id} destination is zero or out of bounds: {destination:?} against output {output:?}"
    )]
    DestinationOutOfBounds {
        region_id: RegionId,
        destination: OutputPixelRect,
        output: PixelSize,
    },

    #[error("region {region_id} has non-finite transform values")]
    NonFiniteTransform { region_id: RegionId },

    #[error("region {region_id} has non-finite radial values")]
    NonFiniteRadialParameters { region_id: RegionId },

    #[error(
        "region {region_id} has unsupported binding/sampling pair: {sampling:?} with patch binding"
    )]
    UnsupportedBindingSamplingPair {
        region_id: RegionId,
        sampling: RegionSampling,
    },

    #[error("integer overflow while validating region {region_id} at {context}")]
    ArithmeticOverflow {
        region_id: RegionId,
        context: &'static str,
    },

    #[error("final plan hash mismatch: expected {expected:?} but computed {computed:?}")]
    FinalPlanHashMismatch {
        expected: ContentDigest,
        computed: ContentDigest,
    },

    #[error("failed to serialize atlas plan identity payload: {0}")]
    IdentitySerialization(#[from] serde_json::Error),

    #[error("source {source_set_id}/{source_id} has missing identity or zero oriented dimensions")]
    InvalidSourceIdentity {
        source_set_id: SourceSetId,
        source_id: String,
    },

    #[error("region {region_id} has an inconsistent executor command: {reason}")]
    InvalidExecutionCommand {
        region_id: RegionId,
        reason: &'static str,
    },

    #[error("tile valid rect must be contained by the requested tile output rect")]
    InvalidTileBounds,
}

#[derive(Serialize)]
struct CanonicalAtlasPlanIdentity<'a> {
    schema_version: u16,
    algorithm_version: &'a str,
    document_revision: u64,
    request_generation: Option<u64>,
    topology_hash: &'a DocumentHash,
    appearance_hash: &'a DocumentHash,
    output_size: &'a PixelSize,
    preview_profile: CompiledAtlasPreviewProfile,
    normal_convention: CompiledNormalConvention,
    color_space_policy: CompiledColorSpacePolicy,
    tile_request: &'a CompiledTileRequest,
    requested_maps: &'a [hot_trimmer_domain::MaterialMapKind],
    ordered_sources: &'a [CompiledSourceCommandV1],
    ordered_regions: &'a [CompiledRegionCommandV1],
}

#[derive(Serialize)]
struct CanonicalAtlasPixelPlanIdentity<'a> {
    schema_version: u16,
    algorithm_version: &'a str,
    topology_hash: &'a DocumentHash,
    output_size: &'a PixelSize,
    preview_profile: CompiledAtlasPreviewProfile,
    normal_convention: Option<CompiledNormalConvention>,
    color_space_policy: CompiledColorSpacePolicy,
    tile_request: CanonicalAtlasPixelTileRequest,
    ordered_sources: Vec<CompiledSourceCommandV1>,
    ordered_regions: Vec<CompiledRegionCommandV1>,
}

#[derive(Serialize)]
struct CanonicalAtlasPixelTileRequest {
    kind: CompiledTileRequestKind,
    output_rect: OutputPixelRect,
    mip_level: u32,
    halo_px: u32,
    valid_rect: OutputPixelRect,
}

fn pixel_source_roles(map: hot_trimmer_domain::MaterialMapKind) -> &'static [MaterialChannelRole] {
    use hot_trimmer_domain::MaterialMapKind;
    match map {
        MaterialMapKind::BaseColor => &[MaterialChannelRole::BaseColor],
        MaterialMapKind::Height => &[MaterialChannelRole::Height],
        MaterialMapKind::Normal => &[MaterialChannelRole::Height, MaterialChannelRole::Normal],
        MaterialMapKind::Roughness => &[MaterialChannelRole::Roughness],
        MaterialMapKind::Metallic => &[MaterialChannelRole::Metallic],
        MaterialMapKind::AmbientOcclusion => &[MaterialChannelRole::AmbientOcclusion],
        MaterialMapKind::Specular => &[MaterialChannelRole::Specular],
        MaterialMapKind::Opacity => &[MaterialChannelRole::Opacity],
        MaterialMapKind::EdgeMask => &[MaterialChannelRole::EdgeMask],
        MaterialMapKind::MaterialId => &[MaterialChannelRole::MaterialId],
        MaterialMapKind::RegionId => &[],
    }
}

fn stable_pixel_source_id(source_set_id: SourceSetId, patch_id: Option<PatchId>) -> ContentDigest {
    ContentDigest::sha256(
        format!("stage-14-source-binding-v1|{source_set_id}|{patch_id:?}").as_bytes(),
    )
}

fn checked_add_u32(
    first: u32,
    second: u32,
    region_id: RegionId,
    context: &'static str,
) -> Result<u32, CompiledAtlasPlanValidationError> {
    first
        .checked_add(second)
        .ok_or_else(|| CompiledAtlasPlanValidationError::ArithmeticOverflow { region_id, context })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CompiledAtlasPreviewProfile {
    Draft512,
    Refinement1024,
    Preview2048,
    Preview4096,
    Preview8192,
    Authoritative,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CompiledNormalConvention {
    OpenGl,
    DirectX,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CompiledColorSpacePolicy {
    SrgbColorUnassociatedAlpha,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledTileRequest {
    pub kind: CompiledTileRequestKind,
    /// The native scheduler rejects publications older than this generation.
    pub generation: u64,
    pub output_rect: OutputPixelRect,
    pub mip_level: u32,
    pub halo_px: u32,
    pub valid_rect: OutputPixelRect,
}

/// The caller's intent for a tile request.  All variants are compiled through the
/// same persisted pipeline and therefore share atlas coordinates and sampling.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CompiledTileRequestKind {
    CompleteDraft512,
    CompleteRefinement1024,
    CompletePreview2048,
    CompletePreview4096,
    CompletePreview8192,
    ExactViewport,
    ExactSelectedRegion,
}

/// A deterministic cache and publication identity for one rendered atlas tile.
/// Selection state is deliberately excluded: it changes scheduling, not pixels.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledAtlasTileIdentity {
    pub plan_hash: ContentDigest,
    pub map: hot_trimmer_domain::MaterialMapKind,
    pub pixel_format: CompiledTilePixelFormat,
    pub mip_level: u32,
    pub output_rect: OutputPixelRect,
    pub halo_px: u32,
    pub valid_rect: OutputPixelRect,
    pub profile: CompiledAtlasPreviewProfile,
    pub shader_version: String,
    pub structural_plan_id: ContentDigest,
    pub scale_microunits: u64,
    pub normal_convention: Option<CompiledNormalConvention>,
    pub generation: u64,
}

/// Reusable pixel identity for a rendered tile. Publication generation is not
/// part of this key because native jobs receive a new generation for the same
/// pixels during ordinary panning or repeated inspection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledAtlasPixelIdentity {
    pub plan_hash: ContentDigest,
    pub map: hot_trimmer_domain::MaterialMapKind,
    pub pixel_format: CompiledTilePixelFormat,
    pub mip_level: u32,
    pub output_rect: OutputPixelRect,
    pub halo_px: u32,
    pub valid_rect: OutputPixelRect,
    pub profile: CompiledAtlasPreviewProfile,
    pub shader_version: String,
    pub structural_plan_id: ContentDigest,
    pub scale_microunits: u64,
    pub normal_convention: Option<CompiledNormalConvention>,
}

impl CompiledAtlasTileIdentity {
    #[must_use]
    pub fn pixel_identity(&self) -> CompiledAtlasPixelIdentity {
        CompiledAtlasPixelIdentity {
            plan_hash: self.plan_hash.clone(),
            map: self.map,
            pixel_format: self.pixel_format,
            mip_level: self.mip_level,
            output_rect: self.output_rect,
            halo_px: self.halo_px,
            valid_rect: self.valid_rect,
            profile: self.profile,
            shader_version: self.shader_version.clone(),
            structural_plan_id: self.structural_plan_id.clone(),
            scale_microunits: self.scale_microunits,
            normal_convention: self.normal_convention,
        }
    }
}

/// Stable metadata for a compact GPU `R32Uint` region-index tile.  Pixel payloads
/// contain only `compact_index`; region UUIDs remain in this lookup table.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledCompactRegionIdLookup {
    pub compact_index: u32,
    pub region_id: RegionId,
    pub role: ManualRegionRole,
    pub continuity: RegionContinuity,
    pub sampling: RegionSampling,
    pub edge_eligibility: EdgeEligibility,
    pub structural_profile: StructuralProfile,
    pub classification: CompiledRegionClassification,
    pub display_rgba8: [u8; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CompiledRegionClassification {
    Panel,
    Horizontal,
    Vertical,
    Unique,
    Radial,
    ContinuousXy,
}

impl CompiledRegionClassification {
    #[must_use]
    pub const fn gpu_code(self) -> u32 {
        match self {
            Self::Panel => 0,
            Self::Horizontal => 1,
            Self::Vertical => 2,
            Self::Unique => 3,
            Self::Radial => 4,
            Self::ContinuousXy => 5,
        }
    }

    #[must_use]
    pub const fn display_rgba8(self, compact_index: u32) -> [u8; 4] {
        // Reserve one RGB high-bit octant for each semantic class, then use
        // all 21 remaining bits for region identity. Multiplication by an odd
        // constant is a bijection modulo 2^21, so compact IDs cannot collide
        // within a class until it contains more than 2,097,152 regions.
        let mixed = compact_index
            .wrapping_mul(0x0015_a4e3)
            .wrapping_add(0x0006_d2b5)
            & 0x001f_ffff;
        let class_high_bits = match self {
            Self::Panel => [0x80, 0x80, 0x80],
            Self::Horizontal => [0x80, 0x00, 0x00],
            Self::Vertical => [0x00, 0x80, 0x00],
            Self::Unique => [0x80, 0x80, 0x00],
            Self::Radial => [0x00, 0x80, 0x80],
            Self::ContinuousXy => [0x80, 0x00, 0x80],
        };
        [
            class_high_bits[0] | (mixed & 0x7f) as u8,
            class_high_bits[1] | ((mixed >> 7) & 0x7f) as u8,
            class_high_bits[2] | ((mixed >> 14) & 0x7f) as u8,
            255,
        ]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CompiledTilePixelFormat {
    Rgba8UnormSrgb,
    Rgba8UnormLinear,
    R32Float,
    R32Uint,
}

/// Metadata for a cache-owned raw tile. `opaque_handle` is intentionally not a
/// filesystem path or data URL: callers can resolve it only through the cache.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledAtlasTileManifest {
    pub identity: CompiledAtlasTileIdentity,
    pub map: hot_trimmer_domain::MaterialMapKind,
    pub mip_level: u32,
    pub output_rect: OutputPixelRect,
    pub valid_rect: OutputPixelRect,
    pub halo_px: u32,
    pub generation: u64,
    pub pixel_format: CompiledTilePixelFormat,
    pub width: u32,
    pub height: u32,
    pub row_stride: u32,
    pub opaque_handle: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SourcePixelRect(pub PixelBounds);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OutputPixelRect(pub PixelBounds);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledAtlasPlanV1 {
    pub schema_version: u16,
    pub algorithm_version: String,
    pub document_revision: u64,
    pub request_generation: Option<u64>,
    pub topology_hash: DocumentHash,
    pub appearance_hash: DocumentHash,
    pub output_size: PixelSize,
    pub preview_profile: CompiledAtlasPreviewProfile,
    pub normal_convention: CompiledNormalConvention,
    pub color_space_policy: CompiledColorSpacePolicy,
    pub tile_request: CompiledTileRequest,
    pub requested_maps: Vec<hot_trimmer_domain::MaterialMapKind>,
    pub ordered_sources: Vec<CompiledSourceCommandV1>,
    pub ordered_regions: Vec<CompiledRegionCommandV1>,
    pub final_plan_hash: ContentDigest,
}

impl CompiledAtlasPlanV1 {
    #[must_use]
    pub fn tile_identity(
        &self,
        map: hot_trimmer_domain::MaterialMapKind,
        shader_version: impl Into<String>,
    ) -> CompiledAtlasTileIdentity {
        let pixel_format = match map {
            hot_trimmer_domain::MaterialMapKind::BaseColor => {
                CompiledTilePixelFormat::Rgba8UnormSrgb
            }
            hot_trimmer_domain::MaterialMapKind::Height
            | hot_trimmer_domain::MaterialMapKind::Roughness
            | hot_trimmer_domain::MaterialMapKind::Metallic
            | hot_trimmer_domain::MaterialMapKind::AmbientOcclusion => {
                CompiledTilePixelFormat::R32Float
            }
            hot_trimmer_domain::MaterialMapKind::RegionId => CompiledTilePixelFormat::R32Uint,
            _ => CompiledTilePixelFormat::Rgba8UnormLinear,
        };
        let plan_hash = self
            .pixel_plan_hash(map)
            .unwrap_or_else(|_| self.final_plan_hash.clone());
        CompiledAtlasTileIdentity {
            plan_hash,
            map,
            pixel_format,
            mip_level: self.tile_request.mip_level,
            output_rect: self.tile_request.output_rect,
            halo_px: self.tile_request.halo_px,
            valid_rect: self.tile_request.valid_rect,
            profile: self.preview_profile,
            shader_version: shader_version.into(),
            structural_plan_id: self.structural_plan_id(),
            scale_microunits: self.scale_microunits(),
            normal_convention: (map == hot_trimmer_domain::MaterialMapKind::Normal)
                .then_some(self.normal_convention),
            generation: self.tile_request.generation,
        }
    }

    pub fn pixel_plan_hash(
        &self,
        map: hot_trimmer_domain::MaterialMapKind,
    ) -> Result<ContentDigest, CompiledAtlasPlanValidationError> {
        let source_roles = pixel_source_roles(map);
        let mut ordered_sources = self
            .ordered_sources
            .iter()
            .filter(|source| source_roles.contains(&source.channel_role))
            .cloned()
            .collect::<Vec<_>>();
        for source in &mut ordered_sources {
            let patch_id = self
                .ordered_regions
                .iter()
                .find(|region| {
                    region.source_set_id == source.source_set_id
                        && region.source_id == source.source_id
                })
                .and_then(|region| region.patch_id);
            source.source_id = stable_pixel_source_id(source.source_set_id, patch_id);
        }
        let mut ordered_regions = self.ordered_regions.clone();
        for region in &mut ordered_regions {
            let stable = stable_pixel_source_id(region.source_set_id, region.patch_id);
            region.source_id = stable.clone();
            region.sampling_plan.candidate.source_id = stable.clone();
            region.sampling_plan.candidate.domain_id = stable.clone();
            region.sampling_plan.candidate.correspondence_reference = stable;
        }
        let identity = CanonicalAtlasPixelPlanIdentity {
            schema_version: self.schema_version,
            algorithm_version: &self.algorithm_version,
            topology_hash: &self.topology_hash,
            output_size: &self.output_size,
            preview_profile: self.preview_profile,
            normal_convention: (map == hot_trimmer_domain::MaterialMapKind::Normal)
                .then_some(self.normal_convention),
            color_space_policy: self.color_space_policy,
            tile_request: CanonicalAtlasPixelTileRequest {
                kind: self.tile_request.kind,
                output_rect: self.tile_request.output_rect,
                mip_level: self.tile_request.mip_level,
                halo_px: self.tile_request.halo_px,
                valid_rect: self.tile_request.valid_rect,
            },
            ordered_sources,
            ordered_regions,
        };
        Ok(ContentDigest::sha256(&serde_json::to_vec(&identity)?))
    }

    #[must_use]
    pub fn structural_plan_id(&self) -> ContentDigest {
        let payload = self
            .ordered_regions
            .iter()
            .map(|region| {
                format!(
                    "{}:{:?}:{:?}:{:?}:{}x{};",
                    region.region_id,
                    region.structural_profile,
                    region.continuity,
                    region.edge_eligibility,
                    region.destination_rect.0.width,
                    region.destination_rect.0.height
                )
            })
            .collect::<String>();
        ContentDigest::sha256(payload.as_bytes())
    }

    #[must_use]
    pub fn scale_microunits(&self) -> u64 {
        self.ordered_regions
            .iter()
            .map(|region| {
                (region.sampling_plan.source_pixels_per_physical_unit * 1_000_000.0)
                    .round()
                    .max(0.0) as u64
            })
            .max()
            .unwrap_or(0)
    }

    #[must_use]
    pub fn compact_region_id_lookup(&self) -> Vec<CompiledCompactRegionIdLookup> {
        self.ordered_regions
            .iter()
            .map(|region| CompiledCompactRegionIdLookup {
                compact_index: region.compact_index,
                region_id: region.region_id,
                role: region.region_role,
                continuity: region.continuity,
                sampling: region.sampling,
                edge_eligibility: region.edge_eligibility,
                structural_profile: region.structural_profile,
                classification: region.region_classification(),
                display_rgba8: region
                    .region_classification()
                    .display_rgba8(region.compact_index),
            })
            .collect()
    }

    pub fn validate(&self) -> Result<(), CompiledAtlasPlanValidationError> {
        if !self.output_size.is_nonzero() {
            return Err(CompiledAtlasPlanValidationError::ZeroOutputSize {
                width: self.output_size.width,
                height: self.output_size.height,
            });
        }

        let mut seen_compact_indices = HashMap::new();
        let mut seen_regions = HashSet::new();

        for source in &self.ordered_sources {
            if source.source_id.0.is_empty()
                || source.digest.0.is_empty()
                || source.decoder_version.is_empty()
                || source.decoded_format.is_empty()
                || source.oriented_dimensions.width == 0
                || source.oriented_dimensions.height == 0
            {
                return Err(CompiledAtlasPlanValidationError::InvalidSourceIdentity {
                    source_set_id: source.source_set_id,
                    source_id: source.source_id.0.clone(),
                });
            }
        }

        validate_output_rect(
            self.tile_request.output_rect,
            self.output_size,
            "tile output",
        )?;
        validate_output_rect(self.tile_request.valid_rect, self.output_size, "tile valid")?;
        let tile = self.tile_request.output_rect.0;
        let valid = self.tile_request.valid_rect.0;
        if valid.x < tile.x
            || valid.y < tile.y
            || valid.x.saturating_add(valid.width) > tile.x.saturating_add(tile.width)
            || valid.y.saturating_add(valid.height) > tile.y.saturating_add(tile.height)
        {
            return Err(CompiledAtlasPlanValidationError::InvalidTileBounds);
        }

        for region in &self.ordered_regions {
            if !seen_regions.insert(region.region_id) {
                return Err(CompiledAtlasPlanValidationError::DuplicateRegionId {
                    region_id: region.region_id,
                    previous_region_id: region.region_id,
                });
            }
            if let Some(previous) =
                seen_compact_indices.insert(region.compact_index, region.region_id)
            {
                return Err(CompiledAtlasPlanValidationError::DuplicateCompactIndex {
                    compact_index: region.compact_index,
                    region_id: region.region_id,
                    previous_region_id: previous,
                });
            }

            let source = self
                .ordered_sources
                .iter()
                .find(|source| {
                    source.source_set_id == region.source_set_id
                        && source.source_id == region.source_id
                })
                .ok_or_else(|| CompiledAtlasPlanValidationError::MissingSource {
                    region_id: region.region_id,
                    source_set_id: region.source_set_id,
                    source_id: region.source_id.0.clone(),
                })?;
            validate_region(region, source, self.output_size)?;
        }

        Ok(())
    }

    fn identity_payload(
        &self,
    ) -> Result<CanonicalAtlasPlanIdentity<'_>, CompiledAtlasPlanValidationError> {
        Ok(CanonicalAtlasPlanIdentity {
            schema_version: self.schema_version,
            algorithm_version: &self.algorithm_version,
            document_revision: self.document_revision,
            request_generation: self.request_generation,
            topology_hash: &self.topology_hash,
            appearance_hash: &self.appearance_hash,
            output_size: &self.output_size,
            preview_profile: self.preview_profile,
            normal_convention: self.normal_convention,
            color_space_policy: self.color_space_policy,
            tile_request: &self.tile_request,
            requested_maps: &self.requested_maps,
            ordered_sources: &self.ordered_sources,
            ordered_regions: &self.ordered_regions,
        })
    }

    pub fn identity_hash(&self) -> Result<ContentDigest, CompiledAtlasPlanValidationError> {
        let identity = self.identity_payload()?;
        Ok(ContentDigest::sha256(&serde_json::to_vec(&identity)?))
    }

    pub fn finalize(mut self) -> Result<Self, CompiledAtlasPlanValidationError> {
        self.validate()?;
        let computed = self.identity_hash()?;
        if self.final_plan_hash == ContentDigest(String::new()) || self.final_plan_hash.0.is_empty()
        {
            self.final_plan_hash = computed;
            return Ok(self);
        }

        if self.final_plan_hash != computed {
            return Err(CompiledAtlasPlanValidationError::FinalPlanHashMismatch {
                expected: self.final_plan_hash,
                computed,
            });
        }

        Ok(self)
    }
}

fn validate_output_rect(
    rect: OutputPixelRect,
    output: PixelSize,
    context: &'static str,
) -> Result<(), CompiledAtlasPlanValidationError> {
    let right = rect.0.x.checked_add(rect.0.width).ok_or(
        CompiledAtlasPlanValidationError::ArithmeticOverflow {
            region_id: RegionId::from_bytes([0; 16]),
            context,
        },
    )?;
    let bottom = rect.0.y.checked_add(rect.0.height).ok_or(
        CompiledAtlasPlanValidationError::ArithmeticOverflow {
            region_id: RegionId::from_bytes([0; 16]),
            context,
        },
    )?;
    if rect.0.width == 0 || rect.0.height == 0 || right > output.width || bottom > output.height {
        return Err(CompiledAtlasPlanValidationError::DestinationOutOfBounds {
            region_id: RegionId::from_bytes([0; 16]),
            destination: rect,
            output,
        });
    }
    Ok(())
}

fn validate_region(
    region: &CompiledRegionCommandV1,
    source: &CompiledSourceCommandV1,
    output_size: PixelSize,
) -> Result<(), CompiledAtlasPlanValidationError> {
    if region.sampling_plan.slot_id != region.region_id {
        return Err(CompiledAtlasPlanValidationError::InvalidExecutionCommand {
            region_id: region.region_id,
            reason: "sampling plan slot id differs from the compiled region id",
        });
    }
    if region.sampling_plan.candidate.source_id != region.source_id {
        return Err(CompiledAtlasPlanValidationError::InvalidExecutionCommand {
            region_id: region.region_id,
            reason: "sampling plan source id differs from the compiled source id",
        });
    }
    let crop = region.source_crop.0;
    let compiled_crop = hot_trimmer_placement_solver::SourceCrop {
        x: crop.x,
        y: crop.y,
        width: crop.width,
        height: crop.height,
    };
    let basis_matches = match region.sampling_plan.sampling_basis {
        hot_trimmer_placement_solver::SamplingBasis::SelectedCrop => {
            region.sampling_plan.candidate.crop == Some(compiled_crop)
        }
        hot_trimmer_placement_solver::SamplingBasis::PreparedDomain { window } => {
            region.sampling_plan.candidate.crop.is_none()
                && window == compiled_crop
                && region.sampling_plan.candidate.mapping_mode
                    == hot_trimmer_domain::SamplingMode::TextureSynthesis
        }
    };
    if !basis_matches {
        return Err(CompiledAtlasPlanValidationError::InvalidExecutionCommand {
            region_id: region.region_id,
            reason: "sampling basis/window differs from the compiled source region",
        });
    }
    if region.sampling_plan.radial_mapping != region.radial_parameters {
        return Err(CompiledAtlasPlanValidationError::InvalidExecutionCommand {
            region_id: region.region_id,
            reason: "sampling plan radial parameters differ from the compiled region command",
        });
    }
    if region.render_cache_key.0.is_empty() {
        return Err(CompiledAtlasPlanValidationError::InvalidExecutionCommand {
            region_id: region.region_id,
            reason: "render cache identity is empty",
        });
    }
    if region.radial_parameters.is_some() && !matches!(region.sampling, RegionSampling::OneShot) {
        return Err(
            CompiledAtlasPlanValidationError::UnsupportedBindingSamplingPair {
                region_id: region.region_id,
                sampling: region.sampling,
            },
        );
    }

    if !region
        .source_to_region_transform
        .scale
        .iter()
        .all(|value| value.is_finite())
        || !region
            .source_to_region_transform
            .rotation_degrees
            .is_finite()
        || !region
            .source_to_region_transform
            .offset
            .iter()
            .all(|value| value.is_finite())
    {
        return Err(CompiledAtlasPlanValidationError::NonFiniteTransform {
            region_id: region.region_id,
        });
    }

    if let Some(radial) = region.radial_parameters
        && !(radial.center_x.is_finite()
            && radial.center_y.is_finite()
            && radial.inner_radius.is_finite()
            && radial.outer_radius.is_finite()
            && radial.falloff.is_finite()
            && radial.blend_width.is_finite()
            && radial.seam_blend_width.is_finite())
    {
        return Err(
            CompiledAtlasPlanValidationError::NonFiniteRadialParameters {
                region_id: region.region_id,
            },
        );
    }

    let crop = region.source_crop.0;
    let crop_right = checked_add_u32(crop.x, crop.width, region.region_id, "source_crop.right")?;
    let crop_bottom = checked_add_u32(crop.y, crop.height, region.region_id, "source_crop.bottom")?;
    if crop.width == 0
        || crop.height == 0
        || crop_right > source.oriented_dimensions.width
        || crop_bottom > source.oriented_dimensions.height
    {
        return Err(CompiledAtlasPlanValidationError::SourceCropOutOfBounds {
            region_id: region.region_id,
            crop: region.source_crop,
            source_width: source.oriented_dimensions.width,
            source_height: source.oriented_dimensions.height,
        });
    }

    let destination = region.destination_rect.0;
    let destination_right = checked_add_u32(
        destination.x,
        destination.width,
        region.region_id,
        "destination.right",
    )?;
    let destination_bottom = checked_add_u32(
        destination.y,
        destination.height,
        region.region_id,
        "destination.bottom",
    )?;
    if destination.width == 0
        || destination.height == 0
        || destination_right > output_size.width
        || destination_bottom > output_size.height
    {
        return Err(CompiledAtlasPlanValidationError::DestinationOutOfBounds {
            region_id: region.region_id,
            destination: region.destination_rect,
            output: output_size,
        });
    }

    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledSourceCommandV1 {
    pub source_set_id: SourceSetId,
    pub source_id: ContentDigest,
    pub digest: ContentDigest,
    pub oriented_dimensions: OrientedPixelSize,
    pub decoder_version: String,
    pub decoded_format: String,
    pub color_version: String,
    pub channel_role: MaterialChannelRole,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledRegionCommandV1 {
    pub region_id: RegionId,
    pub compact_index: u32,
    pub region_role: ManualRegionRole,
    pub source_set_id: SourceSetId,
    pub source_id: ContentDigest,
    pub patch_id: Option<PatchId>,
    pub source_crop: SourcePixelRect,
    pub destination_rect: OutputPixelRect,
    pub sampling: RegionSampling,
    pub source_to_region_transform: MappingTransform,
    pub radial_parameters: Option<RadialMappingSettings>,
    pub structural_profile: StructuralProfile,
    pub continuity: RegionContinuity,
    pub padding_px: u32,
    pub edge_eligibility: EdgeEligibility,
    /// Exact Stage 14 instruction consumed by both the Prompt 1 CPU executor and
    /// later GPU parity implementations. It is compiled before pixel execution.
    pub sampling_plan: SamplingPlan,
    pub render_cache_key: ContentDigest,
}

impl CompiledRegionCommandV1 {
    #[must_use]
    pub const fn region_classification(&self) -> CompiledRegionClassification {
        match self.region_role {
            ManualRegionRole::Radial => CompiledRegionClassification::Radial,
            ManualRegionRole::Unique => CompiledRegionClassification::Unique,
            _ => match self.continuity {
                RegionContinuity::X => CompiledRegionClassification::Horizontal,
                RegionContinuity::Y => CompiledRegionClassification::Vertical,
                RegionContinuity::Xy => CompiledRegionClassification::ContinuousXy,
                RegionContinuity::None => match self.region_role {
                    ManualRegionRole::HorizontalStrip => CompiledRegionClassification::Horizontal,
                    ManualRegionRole::VerticalStrip => CompiledRegionClassification::Vertical,
                    ManualRegionRole::Panel => CompiledRegionClassification::Panel,
                    ManualRegionRole::Unique => CompiledRegionClassification::Unique,
                    ManualRegionRole::Radial => CompiledRegionClassification::Radial,
                },
            },
        }
    }
}
