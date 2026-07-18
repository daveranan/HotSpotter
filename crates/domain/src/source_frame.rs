use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

use crate::{DocumentHash, RegionId};

pub const SOURCE_FRAME_SCHEMA_VERSION: u16 = 1;
pub const LOGICAL_GRID_SCHEMA_VERSION: u16 = 1;
pub const PARTITION_RECIPE_SCHEMA_VERSION: u16 = 3;
pub const MAX_LOGICAL_GRID_EDGE: u32 = 512;
pub const MAX_PARTITION_REGIONS: u32 = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GridRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceFrame {
    pub schema_version: u16,
    pub source_set_id: crate::SourceSetId,
    pub bounds: crate::NormalizedBounds,
    pub oriented_dimensions: crate::OrientedPixelSize,
    pub source_revision: u64,
    pub output_aspect: [u32; 2],
    pub identity: DocumentHash,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogicalGridSpec {
    pub schema_version: u16,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartitionRecipe {
    pub schema_version: u16,
    pub recipe_id: String,
    pub recipe_version: u16,
    pub grid: LogicalGridSpec,
    pub target_region_count: u32,
    pub seed: u64,
    pub horizontal_split_bias_milli: u16,
    pub vertical_split_bias_milli: u16,
    pub variance_milli: u16,
    pub minimum_logical_width: u32,
    pub minimum_logical_height: u32,
    /// Bounds apply to every generated leaf.  They are logical-grid units, never source or atlas pixels.
    #[serde(default = "default_minimum_aspect_milli")]
    pub minimum_aspect_milli: u16,
    #[serde(default = "default_maximum_aspect_milli")]
    pub maximum_aspect_milli: u16,
    #[serde(default = "default_work_limit")]
    pub work_limit: u32,
    #[serde(default = "default_depth_limit")]
    pub depth_limit: u16,
    #[serde(default)]
    pub composition: CompositionProfile,
    /// Present for the new hierarchical generator. Missing means the version-2
    /// Legacy Reserve + Remainder compatibility path.
    #[serde(default)]
    pub hierarchical: Option<HierarchicalLayoutRecipe>,
}

fn default_minimum_aspect_milli() -> u16 { 125 }
fn default_maximum_aspect_milli() -> u16 { 8_000 }
fn default_work_limit() -> u32 { 4_096 }
fn default_depth_limit() -> u16 { 32 }

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MacroStyle { MixedHierarchy, PanelCascade, HorizontalTrims, VerticalTrims, FacadeHalving, ClassicSourceHotspot, ClassicHotspotBasis, MechanicalRadial }

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecursivePolicy { Cascade, Balanced }

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymmetryTransform {
    #[default]
    Identity,
    Rotate90,
    Rotate180,
    Rotate270,
    MirrorX,
    MirrorY,
    MirrorDiagonal,
    MirrorAntiDiagonal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitRatio { Half, OneThird, #[serde(alias = "two_thirds")] TwoThird }

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AspectClass { Square, Wide2, Tall2, Wide4, Tall4, Wide8, Tall8, Wide16, Tall16 }

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HierarchicalLayoutRecipe {
    pub schema_version: u16,
    pub macro_style: MacroStyle,
    pub recursive_policy: RecursivePolicy,
    pub target_region_min: u32,
    pub target_region_max: u32,
    pub large_share_milli: u16,
    pub medium_share_milli: u16,
    pub small_share_milli: u16,
    pub strip_share_milli: u16,
    pub radial_share_milli: u16,
    pub macro_parent_count: u32,
    pub protected_parent_count: u32,
    pub subdividable_parent_count: u32,
    pub hierarchy_depth: u8,
    pub scale_falloff_milli: u16,
    pub allowed_split_ratios: Vec<SplitRatio>,
    pub alignment_strength_milli: u16,
    pub variation_milli: u16,
    pub horizontal_strip_weight_milli: u16,
    pub vertical_strip_weight_milli: u16,
    pub strip_thickness_ladder: Vec<u32>,
    pub radial_count: u32,
    pub radial_min_diameter: u32,
    pub radial_max_diameter: u32,
    pub major_aspects: Vec<AspectClass>,
    pub medium_aspects: Vec<AspectClass>,
    pub detail_aspects: Vec<AspectClass>,
    #[serde(default)]
    pub symmetry: SymmetryTransform,
}

impl HierarchicalLayoutRecipe {
    pub fn mixed_hierarchy_default() -> Self {
        Self {
            schema_version: 1, macro_style: MacroStyle::MixedHierarchy, recursive_policy: RecursivePolicy::Cascade,
            target_region_min: 29, target_region_max: 36,
            large_share_milli: 580, medium_share_milli: 200, small_share_milli: 80, strip_share_milli: 110, radial_share_milli: 30,
            macro_parent_count: 4, protected_parent_count: 2, subdividable_parent_count: 2,
            hierarchy_depth: 3, scale_falloff_milli: 500,
            allowed_split_ratios: vec![SplitRatio::Half, SplitRatio::OneThird, SplitRatio::TwoThird],
            alignment_strength_milli: 900, variation_milli: 80,
            horizontal_strip_weight_milli: 550, vertical_strip_weight_milli: 450,
            strip_thickness_ladder: vec![1, 1, 2, 2, 3, 4],
            radial_count: 2, radial_min_diameter: 6, radial_max_diameter: 10,
            major_aspects: vec![AspectClass::Square, AspectClass::Wide2, AspectClass::Tall2],
            medium_aspects: vec![AspectClass::Square, AspectClass::Wide2, AspectClass::Tall2, AspectClass::Wide4, AspectClass::Tall4],
            detail_aspects: vec![AspectClass::Square, AspectClass::Wide2, AspectClass::Tall2, AspectClass::Wide4, AspectClass::Tall4],
            symmetry: SymmetryTransform::Identity,
        }
    }

    fn validate(&self) -> Result<(), PartitionError> {
        let shares = u32::from(self.large_share_milli) + u32::from(self.medium_share_milli)
            + u32::from(self.small_share_milli) + u32::from(self.strip_share_milli) + u32::from(self.radial_share_milli);
        if self.schema_version == 0 || shares != 1_000 {
            return Err(PartitionError::InvalidHierarchicalShares { total_milli: shares });
        }
        if self.target_region_min == 0 || self.target_region_min > self.target_region_max || self.target_region_max < 24 || self.target_region_max > MAX_PARTITION_REGIONS
            || self.hierarchy_depth == 0 || self.scale_falloff_milli == 0 || self.scale_falloff_milli >= 1_000
            || self.protected_parent_count.saturating_add(self.subdividable_parent_count) > self.macro_parent_count
            || self.allowed_split_ratios.is_empty() || self.alignment_strength_milli > 1_000 || self.variation_milli > 1_000
            || u32::from(self.horizontal_strip_weight_milli) + u32::from(self.vertical_strip_weight_milli) != 1_000
            || (self.strip_share_milli > 0 && self.strip_thickness_ladder.iter().any(|value| *value == 0))
            || self.radial_count > 4
            || (self.radial_count > 0 && (self.radial_min_diameter == 0 || self.radial_min_diameter > self.radial_max_diameter))
            || self.major_aspects.is_empty() || self.medium_aspects.is_empty() || self.detail_aspects.is_empty() {
            return Err(PartitionError::InvalidHierarchicalRecipe);
        }
        Ok(())
    }
}

/// A versioned, user-directed composition recipe.  Counts are quotas, not weights: a request
/// either receives every requested family or returns a typed diagnostic.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompositionProfile {
    #[serde(default = "default_profile_id")]
    pub profile_id: String,
    #[serde(default = "default_profile_version")]
    pub version: u16,
    #[serde(default)]
    pub broad_panels: FamilyQuota,
    #[serde(default)]
    pub medium_blocks: FamilyQuota,
    #[serde(default)]
    pub horizontal_strips: StripQuota,
    #[serde(default)]
    pub vertical_strips: StripQuota,
    #[serde(default)]
    pub small_details: FamilyQuota,
    #[serde(default)]
    pub micro_strips: StripQuota,
    #[serde(default)]
    pub radial_reservations: RadialQuota,
}

fn default_profile_id() -> String { "balanced".into() }
fn default_profile_version() -> u16 { 1 }

impl Default for CompositionProfile {
    fn default() -> Self {
        Self { profile_id: default_profile_id(), version: default_profile_version(),
            broad_panels: FamilyQuota { count: 0, area_share_milli: 0, minimum_width: 8, minimum_height: 8, maximum_width: MAX_LOGICAL_GRID_EDGE, maximum_height: MAX_LOGICAL_GRID_EDGE, minimum_aspect_milli: 250, maximum_aspect_milli: 4_000, subdivision_budget: 0 },
            medium_blocks: FamilyQuota { count: 0, area_share_milli: 0, minimum_width: 4, minimum_height: 4, maximum_width: MAX_LOGICAL_GRID_EDGE, maximum_height: MAX_LOGICAL_GRID_EDGE, minimum_aspect_milli: 200, maximum_aspect_milli: 5_000, subdivision_budget: 0 },
            horizontal_strips: StripQuota { count: 0, minimum_thickness: 1, maximum_thickness: MAX_LOGICAL_GRID_EDGE },
            vertical_strips: StripQuota { count: 0, minimum_thickness: 1, maximum_thickness: MAX_LOGICAL_GRID_EDGE },
            small_details: FamilyQuota { count: 0, area_share_milli: 0, minimum_width: 1, minimum_height: 1, maximum_width: MAX_LOGICAL_GRID_EDGE, maximum_height: MAX_LOGICAL_GRID_EDGE, minimum_aspect_milli: 125, maximum_aspect_milli: 8_000, subdivision_budget: 0 },
            micro_strips: StripQuota { count: 0, minimum_thickness: 1, maximum_thickness: 2 },
            radial_reservations: RadialQuota { count: 0, allocation_min_diameter: 1, allocation_max_diameter: 64 }, }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FamilyQuota {
    pub count: u32,
    pub area_share_milli: u16,
    pub minimum_width: u32,
    pub minimum_height: u32,
    #[serde(default = "default_maximum_family_extent")]
    pub maximum_width: u32,
    #[serde(default = "default_maximum_family_extent")]
    pub maximum_height: u32,
    pub minimum_aspect_milli: u16,
    pub maximum_aspect_milli: u16,
    #[serde(default)]
    pub subdivision_budget: u16,
}

impl Default for FamilyQuota {
    fn default() -> Self { Self { count: 0, area_share_milli: 0, minimum_width: 1, minimum_height: 1, maximum_width: MAX_LOGICAL_GRID_EDGE, maximum_height: MAX_LOGICAL_GRID_EDGE, minimum_aspect_milli: 1, maximum_aspect_milli: 65_535, subdivision_budget: 0 } }
}

fn default_maximum_family_extent() -> u32 { MAX_LOGICAL_GRID_EDGE }

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StripQuota { pub count: u32, pub minimum_thickness: u32, pub maximum_thickness: u32 }

impl Default for StripQuota { fn default() -> Self { Self { count: 0, minimum_thickness: 1, maximum_thickness: 1 } } }

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadialQuota { pub count: u32, pub allocation_min_diameter: u32, pub allocation_max_diameter: u32 }

impl Default for RadialQuota { fn default() -> Self { Self { count: 0, allocation_min_diameter: 1, allocation_max_diameter: 1 } } }

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartitionFamily { Remainder, BroadPanel, MediumBlock, HorizontalStrip, VerticalStrip, SmallDetail, MicroStrip, RadialReservation }

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartitionProvenance {
    pub schema_version: u16,
    pub recipe: PartitionRecipe,
    pub recipe_hash: DocumentHash,
    pub accepted_region_ids: Vec<RegionId>,
    #[serde(default)]
    pub tree: Vec<PartitionTreeNode>,
    #[serde(default)]
    pub topology_hash: DocumentHash,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartitionTreeNode {
    pub grid_rect: GridRect,
    pub family: PartitionFamily,
    pub ordinal: u32,
    #[serde(default)]
    pub lineage: PartitionLineage,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionSourceOverride {
    pub schema_version: u16,
    pub source_bounds: crate::NormalizedBounds,
    pub identity: DocumentHash,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MappingOrigin {
    Partition,
    ExplicitOverride,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartitionRegion {
    pub id: RegionId,
    pub grid_rect: GridRect,
    pub family: PartitionFamily,
    #[serde(default)]
    pub lineage: PartitionLineage,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartitionLineage {
    pub parent_ordinal: Option<u32>,
    /// Immediate hierarchical host. Medium leaves point at their macro parent;
    /// detail leaves point at the medium/detail branch that was split.
    pub host_rect: Option<GridRect>,
    pub depth: u8,
    pub protected_parent: bool,
    pub zone: HierarchyZone,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HierarchyZone { #[default] Legacy, MacroPanel, HorizontalLadder, VerticalLadder, DetailHost, Radial }

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PartitionError {
    #[error("logical grid dimensions must be within 1..={MAX_LOGICAL_GRID_EDGE}")]
    InvalidGrid,
    #[error("target region count must be within 1..={MAX_PARTITION_REGIONS}")]
    InvalidTarget,
    #[error("target region count cannot fit the requested minimum logical region size")]
    ImpossibleTarget,
    #[error("composition quota for {family:?} cannot be satisfied: {reason}. Suggested correction: {suggestion}")]
    ImpossibleFamilyQuota { family: PartitionFamily, reason: String, suggestion: String },
    #[error("composition quotas request {requested} regions but the target is {target}. Suggested correction: increase target count or reduce family counts")]
    QuotaExceedsTarget { requested: u32, target: u32 },
    #[error("partition work/depth limit prevents this target. Suggested correction: raise the limit or reduce target count")]
    WorkLimit,
    #[error("hierarchical family shares must total 1000 milli; received {total_milli}")]
    InvalidHierarchicalShares { total_milli: u32 },
    #[error("hierarchical layout recipe fields are inconsistent or outside supported bounds")]
    InvalidHierarchicalRecipe,
    #[error("hotspot basis inventory is invalid: {reason}")]
    InvalidHotspotBasis { reason: String },
}

impl LogicalGridSpec {
    pub const DEFAULT: Self = Self { schema_version: LOGICAL_GRID_SCHEMA_VERSION, width: 64, height: 64 };

    pub fn validate(self) -> Result<(), PartitionError> {
        if self.schema_version == 0 || self.width == 0 || self.height == 0
            || self.width > MAX_LOGICAL_GRID_EDGE || self.height > MAX_LOGICAL_GRID_EDGE {
            return Err(PartitionError::InvalidGrid);
        }
        Ok(())
    }
}

impl SourceFrame {
    pub fn centered_largest(
        source_set_id: crate::SourceSetId,
        oriented_dimensions: crate::OrientedPixelSize,
        output_aspect: [u32; 2],
        source_revision: u64,
    ) -> Self {
        let source_width = u64::from(oriented_dimensions.width);
        let source_height = u64::from(oriented_dimensions.height);
        let aspect_width = u64::from(output_aspect[0].max(1));
        let aspect_height = u64::from(output_aspect[1].max(1));
        let (crop_width, crop_height) = if source_width.saturating_mul(aspect_height)
            >= source_height.saturating_mul(aspect_width)
        {
            (
                (source_height
                    .saturating_mul(aspect_width)
                    .saturating_add(aspect_height / 2)
                    / aspect_height)
                .clamp(1, source_width),
                source_height,
            )
        } else {
            (
                source_width,
                (source_width
                    .saturating_mul(aspect_height)
                    .saturating_add(aspect_width / 2)
                    / aspect_width)
                .clamp(1, source_height),
            )
        };
        let x = (source_width - crop_width) / 2;
        let y = (source_height - crop_height) / 2;
        let left = stable_normalized(x, source_width);
        let top = stable_normalized(y, source_height);
        let right = stable_normalized(x + crop_width, source_width);
        let bottom = stable_normalized(y + crop_height, source_height);
        let bounds = crate::NormalizedBounds {
            x: crate::NormalizedScalar::new(left).expect("frame x"),
            y: crate::NormalizedScalar::new(top).expect("frame y"),
            width: crate::NormalizedScalar::new(right - left).expect("frame width"),
            height: crate::NormalizedScalar::new(bottom - top).expect("frame height"),
        };
        let mut frame = Self { schema_version: SOURCE_FRAME_SCHEMA_VERSION, source_set_id, bounds,
            oriented_dimensions, source_revision, output_aspect, identity: DocumentHash([0; 32]) };
        frame.identity = frame.compute_identity();
        frame
    }

    pub fn compute_identity(&self) -> DocumentHash {
        let mut copy = self.clone(); copy.identity = DocumentHash([0; 32]);
        DocumentHash(Sha256::digest(serde_json::to_vec(&copy).expect("source frame serializable")).into())
    }

    pub fn with_bounds(&self, bounds: crate::NormalizedBounds) -> Self {
        let mut next = self.clone();
        next.bounds = bounds;
        next.identity = next.compute_identity();
        next
    }

    pub fn region_bounds(&self, grid: LogicalGridSpec, rect: GridRect) -> crate::NormalizedBounds {
        let source_x = resolve_boundaries(
            (self.bounds.x.get() * f64::from(self.oriented_dimensions.width)).round() as u32,
            (self.bounds.width.get() * f64::from(self.oriented_dimensions.width)).round() as u32,
            grid.width,
        );
        let source_y = resolve_boundaries(
            (self.bounds.y.get() * f64::from(self.oriented_dimensions.height)).round() as u32,
            (self.bounds.height.get() * f64::from(self.oriented_dimensions.height)).round() as u32,
            grid.height,
        );
        let x = f64::from(source_x[rect.x as usize]) / f64::from(self.oriented_dimensions.width);
        let y = f64::from(source_y[rect.y as usize]) / f64::from(self.oriented_dimensions.height);
        let right = f64::from(source_x[(rect.x + rect.width) as usize]) / f64::from(self.oriented_dimensions.width);
        let bottom = f64::from(source_y[(rect.y + rect.height) as usize]) / f64::from(self.oriented_dimensions.height);
        crate::NormalizedBounds {
            x: crate::NormalizedScalar::new(x).expect("resolved source x"),
            y: crate::NormalizedScalar::new(y).expect("resolved source y"),
            width: crate::NormalizedScalar::new(right - x).expect("resolved source width"),
            height: crate::NormalizedScalar::new(bottom - y).expect("resolved source height"),
        }
    }
}

fn stable_normalized(value: u64, extent: u64) -> f64 {
    let normalized = value as f64 / extent.max(1) as f64;
    (normalized * 1_000_000_000_000.0).round() / 1_000_000_000_000.0
}

impl RegionSourceOverride {
    pub fn new(source_bounds: crate::NormalizedBounds) -> Self {
        let mut value = Self { schema_version: 1, source_bounds, identity: DocumentHash([0; 32]) };
        value.identity = value.compute_identity();
        value
    }

    pub fn compute_identity(&self) -> DocumentHash {
        let mut copy = *self;
        copy.identity = DocumentHash([0; 32]);
        DocumentHash(Sha256::digest(serde_json::to_vec(&copy).expect("source override serializable")).into())
    }
}

impl PartitionRecipe {
    pub fn default_for(grid: LogicalGridSpec, target_region_count: u32, seed: u64) -> Self {
        Self {
            schema_version: PARTITION_RECIPE_SCHEMA_VERSION,
            recipe_id: "source-frame-recursive-bsp".into(),
            recipe_version: 1,
            grid,
            target_region_count,
            seed,
            horizontal_split_bias_milli: 500,
            vertical_split_bias_milli: 500,
            variance_milli: 0,
            minimum_logical_width: 1,
            minimum_logical_height: 1,
            minimum_aspect_milli: default_minimum_aspect_milli(),
            maximum_aspect_milli: default_maximum_aspect_milli(),
            work_limit: default_work_limit(),
            depth_limit: default_depth_limit(),
            composition: CompositionProfile::default(),
            hierarchical: None,
        }
    }

    pub fn validate(&self) -> Result<(), PartitionError> {
        self.grid.validate()?;
        if let Some(hierarchical) = &self.hierarchical {
            hierarchical.validate()?;
            if self.target_region_count != hierarchical.target_region_max {
                return Err(PartitionError::InvalidHierarchicalRecipe);
            }
            return Ok(());
        }
        if self.schema_version == 0 || self.recipe_version == 0
            || self.target_region_count == 0 || self.target_region_count > MAX_PARTITION_REGIONS {
            return Err(PartitionError::InvalidTarget);
        }
        if self.minimum_logical_width == 0 || self.minimum_logical_height == 0
            || self.minimum_aspect_milli == 0 || self.minimum_aspect_milli > self.maximum_aspect_milli
            || self.work_limit < self.target_region_count || self.depth_limit == 0
        { return Err(PartitionError::ImpossibleTarget); }
        let capacity = (self.grid.width / self.minimum_logical_width.max(1))
            .saturating_mul(self.grid.height / self.minimum_logical_height.max(1));
        if capacity < self.target_region_count { return Err(PartitionError::ImpossibleTarget); }
        let profile = &self.composition;
        if profile.profile_id.trim().is_empty() || profile.version == 0 { return Err(PartitionError::ImpossibleTarget); }
        let quotas = profile.broad_panels.count
            .saturating_add(profile.medium_blocks.count)
            .saturating_add(profile.horizontal_strips.count)
            .saturating_add(profile.vertical_strips.count)
            .saturating_add(profile.small_details.count)
            .saturating_add(profile.micro_strips.count)
            .saturating_add(profile.radial_reservations.count);
        let subdivision_floor = u32::from(profile.broad_panels.subdivision_budget).saturating_mul(profile.broad_panels.count)
            .saturating_add(u32::from(profile.medium_blocks.subdivision_budget).saturating_mul(profile.medium_blocks.count))
            .saturating_add(u32::from(profile.small_details.subdivision_budget).saturating_mul(profile.small_details.count));
        let requested_floor = quotas.saturating_add(subdivision_floor).saturating_add(u32::from(quotas > 0));
        if requested_floor > self.target_region_count { return Err(PartitionError::QuotaExceedsTarget { requested: requested_floor, target: self.target_region_count }); }
        let requested_area = u32::from(profile.broad_panels.area_share_milli)
            .saturating_add(u32::from(profile.medium_blocks.area_share_milli))
            .saturating_add(u32::from(profile.small_details.area_share_milli));
        if requested_area > 1_000 {
            return Err(PartitionError::ImpossibleFamilyQuota { family: PartitionFamily::BroadPanel, reason: "panel area shares exceed the complete frame".into(), suggestion: "reduce combined panel, block, and detail area share to 100% or less".into() });
        }
        for (family, quota) in [(PartitionFamily::BroadPanel, profile.broad_panels), (PartitionFamily::MediumBlock, profile.medium_blocks), (PartitionFamily::SmallDetail, profile.small_details)] {
            if quota.count > 0 && (quota.minimum_width == 0 || quota.minimum_height == 0
                || quota.maximum_width < quota.minimum_width || quota.maximum_height < quota.minimum_height
                || quota.minimum_aspect_milli == 0 || quota.minimum_aspect_milli > quota.maximum_aspect_milli) {
                return Err(PartitionError::ImpossibleFamilyQuota { family, reason: "dimensions or aspect bounds are invalid".into(), suggestion: "use positive dimensions and an ordered aspect range".into() });
            }
        }
        for (family, quota) in [(PartitionFamily::HorizontalStrip, profile.horizontal_strips), (PartitionFamily::VerticalStrip, profile.vertical_strips), (PartitionFamily::MicroStrip, profile.micro_strips)] {
            if quota.count > 0 && (quota.minimum_thickness == 0 || quota.minimum_thickness > quota.maximum_thickness) {
                return Err(PartitionError::ImpossibleFamilyQuota { family, reason: "strip thickness bounds are invalid".into(), suggestion: "use a positive minimum no greater than maximum".into() });
            }
        }
        let radial = profile.radial_reservations;
        if radial.count > 0 && (radial.allocation_min_diameter == 0 || radial.allocation_min_diameter > radial.allocation_max_diameter) {
            return Err(PartitionError::ImpossibleFamilyQuota { family: PartitionFamily::RadialReservation, reason: "allocation diameter bounds are invalid".into(), suggestion: "use a positive minimum no greater than maximum".into() });
        }
        Ok(())
    }

    pub fn hash(&self) -> DocumentHash {
        let bytes = serde_json::to_vec(self).expect("partition recipe is serializable");
        DocumentHash(Sha256::digest(bytes).into())
    }
}

/// Deterministic guillotine partitioning on the logical lattice. Each iteration splits one
/// existing leaf, so the result cannot contain gaps or overlaps.
pub fn generate_partition(recipe: &PartitionRecipe) -> Result<Vec<PartitionRegion>, PartitionError> {
    if recipe.hierarchical.is_some() { return generate_hierarchical_partition(recipe); }
    generate_legacy_partition(recipe)
}

/// Version-2 compatibility generator retained for saved projects and exact-count fixtures.
fn generate_legacy_partition(recipe: &PartitionRecipe) -> Result<Vec<PartitionRegion>, PartitionError> {
    recipe.validate()?;
    let mut leaves = vec![PartitionLeaf::remainder(GridRect { x: 0, y: 0, width: recipe.grid.width, height: recipe.grid.height }, 0, true)];
    reserve_composition(recipe, &mut leaves)?;
    while leaves.len() < recipe.target_region_count as usize {
        if leaves.len() as u32 >= recipe.work_limit { return Err(PartitionError::WorkLimit); }
        let index = leaves.iter().enumerate().filter(|(_, leaf)| leaf.depth < recipe.depth_limit && can_split_leaf(**leaf, recipe)).max_by_key(|(index, leaf)| {
            (leaf.subdivision_budget > 0, u64::from(leaf.rect.width) * u64::from(leaf.rect.height), u64::from(leaf.rect.width.max(leaf.rect.height)), u64::MAX - *index as u64)
        }).map(|(index, _)| index).ok_or(PartitionError::WorkLimit)?;
        let leaf = leaves.remove(index);
        let rect = leaf.rect;
        let family = leaf.family;
        let depth = leaf.depth;
        let (family_minimum_width, family_minimum_height) = match family {
            PartitionFamily::BroadPanel => (recipe.composition.broad_panels.minimum_width, recipe.composition.broad_panels.minimum_height),
            PartitionFamily::MediumBlock => (recipe.composition.medium_blocks.minimum_width, recipe.composition.medium_blocks.minimum_height),
            PartitionFamily::SmallDetail => (recipe.composition.small_details.minimum_width, recipe.composition.small_details.minimum_height),
            _ => (1, 1),
        };
        let can_vertical = rect.width >= recipe.minimum_logical_width.max(family_minimum_width).saturating_mul(2);
        let can_horizontal = rect.height >= recipe.minimum_logical_height.max(family_minimum_height).saturating_mul(2);
        if !can_vertical && !can_horizontal { return Err(PartitionError::ImpossibleTarget); }
        let prefer_vertical = if recipe.horizontal_split_bias_milli == recipe.vertical_split_bias_milli {
            (recipe.seed.wrapping_add(leaves.len() as u64) & 1) == 0
        } else { recipe.vertical_split_bias_milli > recipe.horizontal_split_bias_milli };
        let vertical = (prefer_vertical && can_vertical) || !can_horizontal;
        let extent = if vertical { rect.width } else { rect.height };
        let mut split = extent / 2;
        if recipe.variance_milli > 0 {
            let span = ((extent / 2).saturating_mul(u32::from(recipe.variance_milli).min(1000)) / 1000).max(1);
            let delta = (recipe.seed.wrapping_add(leaves.len() as u64) % u64::from(span * 2 + 1)) as u32;
            split = split.saturating_sub(span).saturating_add(delta);
        }
        let family_minimum = if vertical { family_minimum_width } else { family_minimum_height };
        let minimum = (if vertical { recipe.minimum_logical_width } else { recipe.minimum_logical_height }).max(family_minimum);
        split = split.clamp(minimum, extent.saturating_sub(minimum));
        let (first, second) = if vertical {
            (GridRect { x: rect.x, y: rect.y, width: split, height: rect.height },
             GridRect { x: rect.x + split, y: rect.y, width: rect.width - split, height: rect.height })
        } else {
            (GridRect { x: rect.x, y: rect.y, width: rect.width, height: split },
             GridRect { x: rect.x, y: rect.y + split, width: rect.width, height: rect.height - split })
        };
        if family == PartitionFamily::Remainder {
            leaves.extend([PartitionLeaf::remainder(first, depth + 1, leaf.fill_remainder), PartitionLeaf::remainder(second, depth + 1, leaf.fill_remainder)]);
        } else {
            leaves.extend([
                PartitionLeaf { rect: first, family, depth: depth + 1, subdivision_budget: leaf.subdivision_budget.saturating_sub(1), fill_remainder: false },
                PartitionLeaf::remainder(second, depth + 1, false),
            ]);
        }
    }
    leaves.sort_by_key(|leaf| (leaf.rect.y, leaf.rect.x, leaf.rect.height, leaf.rect.width));
    Ok(leaves.into_iter().enumerate().map(|(index, leaf)| PartitionRegion {
        id: region_id(recipe, leaf.rect, index as u32), grid_rect: leaf.rect, family: leaf.family, lineage: PartitionLineage::default(),
    }).collect())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
enum HierarchicalZoneKind { Panels, HorizontalLadder, VerticalLadder, Cascade, Radial }

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
struct HierarchicalZoneRect { rect: GridRect, kind: HierarchicalZoneKind }

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
struct HierarchicalLeaf {
    rect: GridRect,
    family: PartitionFamily,
    lineage: PartitionLineage,
    splittable: bool,
}

#[allow(dead_code)]
struct HierarchicalContext<'a> {
    recipe: &'a PartitionRecipe,
    hierarchy: &'a HierarchicalLayoutRecipe,
    cuts_x: BTreeSet<u32>,
    cuts_y: BTreeSet<u32>,
    split_ordinal: u32,
}

type DemandId = u32;
type PairGroupId = u32;
type VariantGroupId = u32;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum SizeTier { Macro, Medium, Small, Strip, Radial }

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum RegionRole { Square, Wide, Tall, HorizontalStrip, VerticalStrip, Radial }

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum ZonePreference { MacroSquare, HorizontalFamily, VerticalFamily, DetailCore, HorizontalLadder, VerticalLadder, RadialCluster }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QuadrantRole { MajorSquare, HorizontalFamily, VerticalFamily, DetailBasis }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RegionDemand {
    id: DemandId,
    width_cells: u32,
    height_cells: u32,
    tier: SizeTier,
    role: RegionRole,
    pair_group: Option<PairGroupId>,
    variant_group: Option<VariantGroupId>,
    required: bool,
    multiplicity: u32,
    zone_preference: ZonePreference,
}

#[derive(Clone, Copy)]
struct BasisGrid {
    width: u32,
    height: u32,
    cell_w: u32,
    cell_h: u32,
}

impl BasisGrid {
    fn new(grid: LogicalGridSpec) -> Result<Self, PartitionError> {
        if grid.width < 16 || grid.height < 16 || grid.width != grid.height {
            return Err(PartitionError::InvalidHotspotBasis { reason: "curated hotspot basis requires a square grid of at least 16 cells".into() });
        }
        let cell_w = grid.width / 8;
        let cell_h = grid.height / 8;
        if cell_w == 0 || cell_h == 0 {
            return Err(PartitionError::InvalidHotspotBasis { reason: "grid is too small for the canonical hotspot basis".into() });
        }
        Ok(Self { width: grid.width, height: grid.height, cell_w, cell_h })
    }

    fn rect(self, x0: u32, y0: u32, x1: u32, y1: u32) -> GridRect {
        let left = self.width.saturating_mul(x0) / 8;
        let top = self.height.saturating_mul(y0) / 8;
        let right = self.width.saturating_mul(x1) / 8;
        let bottom = self.height.saturating_mul(y1) / 8;
        GridRect { x: left, y: top, width: right - left, height: bottom - top }
    }
}

fn transform_basis_rect(rect: GridRect, grid: LogicalGridSpec, transform: SymmetryTransform) -> GridRect {
    match transform {
        SymmetryTransform::Identity => rect,
        SymmetryTransform::MirrorX => GridRect { x: grid.width - rect.x - rect.width, ..rect },
        SymmetryTransform::MirrorY => GridRect { y: grid.height - rect.y - rect.height, ..rect },
        SymmetryTransform::Rotate180 => GridRect { x: grid.width - rect.x - rect.width, y: grid.height - rect.y - rect.height, ..rect },
        SymmetryTransform::Rotate90 => GridRect { x: grid.height - rect.y - rect.height, y: rect.x, width: rect.height, height: rect.width },
        SymmetryTransform::Rotate270 => GridRect { x: rect.y, y: grid.width - rect.x - rect.width, width: rect.height, height: rect.width },
        SymmetryTransform::MirrorDiagonal => GridRect { x: rect.y, y: rect.x, width: rect.height, height: rect.width },
        SymmetryTransform::MirrorAntiDiagonal => GridRect { x: grid.height - rect.y - rect.height, y: grid.width - rect.x - rect.width, width: rect.height, height: rect.width },
    }
}

fn strip_ladder_for(extent: u32) -> Vec<u32> {
    if extent >= 8 { return vec![extent / 2, extent / 4, 1, extent - extent / 2 - extent / 4 - 1]; }
    if extent >= 4 { return vec![extent / 2, 1, extent - extent / 2 - 1]; }
    vec![1; extent as usize]
}

fn build_hotspot_basis_inventory(grid: LogicalGridSpec, hierarchy: &HierarchicalLayoutRecipe) -> Result<Vec<RegionDemand>, PartitionError> {
    let basis = BasisGrid::new(grid)?;
    let mut demands = Vec::new();
    let mut next_id = 0_u32;
    let mut push = |width_cells, height_cells, tier, role, pair_group, variant_group, required, multiplicity, zone_preference| {
        demands.push(RegionDemand { id: next_id, width_cells, height_cells, tier, role, pair_group, variant_group, required, multiplicity, zone_preference });
        next_id = next_id.saturating_add(1);
    };
    push(basis.cell_w * 4, basis.cell_h * 4, SizeTier::Macro, RegionRole::Square, None, Some(1), true, 1, ZonePreference::MacroSquare);
    push(basis.cell_w * 4, basis.cell_h * 2, SizeTier::Macro, RegionRole::Wide, Some(10), Some(2), true, 1, ZonePreference::HorizontalFamily);
    push(basis.cell_w * 2, basis.cell_h * 4, SizeTier::Macro, RegionRole::Tall, Some(10), Some(2), true, 1, ZonePreference::VerticalFamily);
    push(basis.cell_w * 2, basis.cell_h * 2, SizeTier::Medium, RegionRole::Square, None, Some(3), true, 4, ZonePreference::HorizontalFamily);
    push(basis.cell_w * 2, basis.cell_h, SizeTier::Medium, RegionRole::Wide, Some(20), Some(4), true, 1, ZonePreference::DetailCore);
    push(basis.cell_w, basis.cell_h * 2, SizeTier::Medium, RegionRole::Tall, Some(20), Some(4), true, 1, ZonePreference::DetailCore);
    push(basis.cell_w, basis.cell_h, SizeTier::Small, RegionRole::Square, None, Some(5), true, 1, ZonePreference::DetailCore);
    push(basis.cell_w, (basis.cell_h / 2).max(1), SizeTier::Small, RegionRole::Wide, Some(30), Some(6), true, 2, ZonePreference::DetailCore);
    push((basis.cell_w / 2).max(1), basis.cell_h, SizeTier::Small, RegionRole::Tall, Some(30), Some(6), true, 2, ZonePreference::DetailCore);
    for thickness in strip_ladder_for(basis.cell_h) {
        push(basis.cell_w * 4, thickness, SizeTier::Strip, RegionRole::HorizontalStrip, None, Some(7), true, 1, ZonePreference::HorizontalLadder);
    }
    for thickness in strip_ladder_for(basis.cell_w) {
        push(thickness, basis.cell_h * 3, SizeTier::Strip, RegionRole::VerticalStrip, None, Some(8), true, 1, ZonePreference::VerticalLadder);
    }
    if hierarchy.radial_count > 0 {
        push(basis.cell_w, basis.cell_h, SizeTier::Radial, RegionRole::Radial, None, Some(9), true, hierarchy.radial_count, ZonePreference::RadialCluster);
    }
    validate_region_demands(&demands)?;
    Ok(demands)
}

fn validate_region_demands(demands: &[RegionDemand]) -> Result<(), PartitionError> {
    let mut pair_counts = BTreeMap::<PairGroupId, (u32, u32)>::new();
    for demand in demands.iter().filter(|demand| demand.required) {
        if let Some(pair_group) = demand.pair_group {
            let entry = pair_counts.entry(pair_group).or_default();
            if demand.width_cells >= demand.height_cells { entry.0 = entry.0.saturating_add(demand.multiplicity); }
            else { entry.1 = entry.1.saturating_add(demand.multiplicity); }
        }
        if demand.role == RegionRole::Radial && demand.width_cells != demand.height_cells {
            return Err(PartitionError::InvalidHotspotBasis { reason: "radial demand is not square".into() });
        }
    }
    for (pair_group, (wide, tall)) in pair_counts {
        if wide == 0 || tall == 0 || wide != tall {
            return Err(PartitionError::InvalidHotspotBasis { reason: format!("orientation pair group {pair_group} is incomplete ({wide} wide / {tall} tall)") });
        }
    }
    Ok(())
}

fn generate_hierarchical_partition(recipe: &PartitionRecipe) -> Result<Vec<PartitionRegion>, PartitionError> {
    recipe.grid.validate()?;
    let hierarchy = recipe.hierarchical.as_ref().ok_or(PartitionError::InvalidHierarchicalRecipe)?;
    hierarchy.validate()?;
    let demands = build_hotspot_basis_inventory(recipe.grid, hierarchy)?;
    let mut leaves = place_hotspot_basis_inventory(recipe, hierarchy, &demands)?;
    apply_basis_complexity(&mut leaves, recipe, hierarchy);
    if leaves.len() < hierarchy.target_region_min as usize || leaves.len() > hierarchy.target_region_max as usize {
        return Err(PartitionError::InvalidHotspotBasis { reason: format!("basis produced {} regions outside requested {}..={}", leaves.len(), hierarchy.target_region_min, hierarchy.target_region_max) });
    }
    validate_hotspot_basis(recipe.grid, hierarchy, &demands, &leaves)?;
    leaves.sort_by_key(|leaf| (leaf.rect.y, leaf.rect.x, leaf.rect.height, leaf.rect.width, leaf.family));
    Ok(leaves.into_iter().enumerate().map(|(index, leaf)| PartitionRegion {
        id: region_id(recipe, leaf.rect, index as u32), grid_rect: leaf.rect, family: leaf.family, lineage: leaf.lineage,
    }).collect())
}

fn place_hotspot_basis_inventory(recipe: &PartitionRecipe, hierarchy: &HierarchicalLayoutRecipe, demands: &[RegionDemand]) -> Result<Vec<HierarchicalLeaf>, PartitionError> {
    let _required_inventory = demands.iter().filter(|demand| demand.required).count();
    let basis = BasisGrid::new(recipe.grid)?;
    let mut leaves = Vec::<HierarchicalLeaf>::new();
    let roles = quadrant_roles_for(hierarchy, recipe.seed);
    let quadrants = [basis.rect(0, 0, 4, 4), basis.rect(4, 0, 8, 4), basis.rect(0, 4, 4, 8), basis.rect(4, 4, 8, 8)];
    let mut radial_remaining = hierarchy.radial_count;
    for (ordinal, (rect, role)) in quadrants.into_iter().zip(roles).enumerate() {
        place_quadrant_role(rect, role, ordinal as u32, hierarchy, &mut radial_remaining, &mut leaves);
    }
    while radial_remaining > 0 {
        let added = replace_first_matching(&mut leaves, |leaf| {
            leaf.family == PartitionFamily::MediumBlock && leaf.rect.width == leaf.rect.height && leaf.rect.width >= basis.cell_w * 2
        }, |leaf| split_medium_square_for_radials(leaf, &mut radial_remaining));
        if !added { break; }
    }
    apply_symmetry_to_leaves(&mut leaves, recipe.grid, hierarchy.symmetry);
    Ok(leaves)
}

fn quadrant_roles_for(hierarchy: &HierarchicalLayoutRecipe, seed: u64) -> [QuadrantRole; 4] {
    let mut roles = match hierarchy.macro_style {
        MacroStyle::MixedHierarchy => [QuadrantRole::MajorSquare, QuadrantRole::HorizontalFamily, QuadrantRole::VerticalFamily, QuadrantRole::DetailBasis],
        MacroStyle::PanelCascade => [QuadrantRole::MajorSquare, QuadrantRole::DetailBasis, QuadrantRole::VerticalFamily, QuadrantRole::HorizontalFamily],
        MacroStyle::HorizontalTrims => [QuadrantRole::HorizontalFamily, QuadrantRole::MajorSquare, QuadrantRole::DetailBasis, QuadrantRole::VerticalFamily],
        MacroStyle::VerticalTrims => [QuadrantRole::VerticalFamily, QuadrantRole::DetailBasis, QuadrantRole::MajorSquare, QuadrantRole::HorizontalFamily],
        MacroStyle::FacadeHalving => [QuadrantRole::MajorSquare, QuadrantRole::VerticalFamily, QuadrantRole::HorizontalFamily, QuadrantRole::DetailBasis],
        MacroStyle::ClassicSourceHotspot | MacroStyle::ClassicHotspotBasis => [QuadrantRole::VerticalFamily, QuadrantRole::HorizontalFamily, QuadrantRole::DetailBasis, QuadrantRole::MajorSquare],
        MacroStyle::MechanicalRadial => [QuadrantRole::MajorSquare, QuadrantRole::VerticalFamily, QuadrantRole::HorizontalFamily, QuadrantRole::DetailBasis],
    };
    if hierarchy.variation_milli > 0 {
        match seed % 4 {
            1 => roles.swap(1, 2),
            2 => roles.swap(0, 3),
            3 => roles.rotate_left(1),
            _ => {}
        }
    }
    if hierarchy.large_share_milli >= 700 {
        roles.swap(0, 3);
    }
    roles
}

fn place_quadrant_role(
    rect: GridRect,
    role: QuadrantRole,
    ordinal: u32,
    hierarchy: &HierarchicalLayoutRecipe,
    radial_remaining: &mut u32,
    leaves: &mut Vec<HierarchicalLeaf>,
) {
    match role {
        QuadrantRole::MajorSquare => push_leaf(leaves, rect, PartitionFamily::BroadPanel, Some(ordinal), 0, true, HierarchyZone::MacroPanel, false),
        QuadrantRole::HorizontalFamily => {
            let half_h = rect.height / 2;
            let half_w = rect.width / 2;
            push_leaf(leaves, GridRect { height: half_h, ..rect }, PartitionFamily::BroadPanel, Some(ordinal), 0, true, HierarchyZone::MacroPanel, false);
            push_leaf(leaves, GridRect { y: rect.y + half_h, width: half_w, height: rect.height - half_h, ..rect }, PartitionFamily::MediumBlock, Some(ordinal), 1, false, HierarchyZone::MacroPanel, true);
            push_leaf(leaves, GridRect { x: rect.x + half_w, y: rect.y + half_h, width: rect.width - half_w, height: rect.height - half_h }, PartitionFamily::MediumBlock, Some(ordinal), 1, false, HierarchyZone::MacroPanel, true);
        }
        QuadrantRole::VerticalFamily => {
            let half_w = rect.width / 2;
            let half_h = rect.height / 2;
            push_leaf(leaves, GridRect { width: half_w, ..rect }, PartitionFamily::BroadPanel, Some(ordinal), 0, true, HierarchyZone::MacroPanel, false);
            push_leaf(leaves, GridRect { x: rect.x + half_w, width: rect.width - half_w, height: half_h, ..rect }, PartitionFamily::MediumBlock, Some(ordinal), 1, false, HierarchyZone::MacroPanel, true);
            push_leaf(leaves, GridRect { x: rect.x + half_w, y: rect.y + half_h, width: rect.width - half_w, height: rect.height - half_h }, PartitionFamily::MediumBlock, Some(ordinal), 1, false, HierarchyZone::MacroPanel, true);
        }
        QuadrantRole::DetailBasis => place_detail_quadrant(rect, ordinal, hierarchy, radial_remaining, leaves),
    }
}

fn place_detail_quadrant(
    rect: GridRect,
    ordinal: u32,
    hierarchy: &HierarchicalLayoutRecipe,
    radial_remaining: &mut u32,
    leaves: &mut Vec<HierarchicalLeaf>,
) {
    let quarter_w = rect.width / 4;
    let quarter_h = rect.height / 4;
    let half_w = rect.width / 2;
    let horizontal_ladder = if hierarchy.strip_thickness_ladder.is_empty() { strip_ladder_for(quarter_h) } else { normalized_ladder(&hierarchy.strip_thickness_ladder, quarter_h) };
    let mut y = rect.y + rect.height - quarter_h;
    for thickness in horizontal_ladder {
        push_leaf(leaves, GridRect { x: rect.x, y, width: rect.width, height: thickness }, PartitionFamily::HorizontalStrip, None, 1, false, HierarchyZone::HorizontalLadder, false);
        y += thickness;
    }
    let vertical_ladder = if hierarchy.strip_thickness_ladder.is_empty() { strip_ladder_for(quarter_w) } else { normalized_ladder(&hierarchy.strip_thickness_ladder, quarter_w) };
    let mut x = rect.x + rect.width - quarter_w;
    for thickness in vertical_ladder {
        push_leaf(leaves, GridRect { x, y: rect.y, width: thickness, height: quarter_h * 3 }, PartitionFamily::VerticalStrip, None, 1, false, HierarchyZone::VerticalLadder, false);
        x += thickness;
    }
    push_leaf(leaves, GridRect { x: rect.x, y: rect.y, width: half_w, height: quarter_h }, PartitionFamily::MediumBlock, Some(ordinal), 1, false, HierarchyZone::DetailHost, true);
    push_leaf(leaves, GridRect { x: rect.x + half_w, y: rect.y, width: quarter_w, height: half_w }, PartitionFamily::MediumBlock, Some(ordinal), 1, false, HierarchyZone::DetailHost, true);
    push_leaf(leaves, GridRect { x: rect.x, y: rect.y + quarter_h, width: quarter_w, height: quarter_h }, PartitionFamily::SmallDetail, Some(ordinal), 2, false, HierarchyZone::DetailHost, true);
    let radial_a = take_radial_family(radial_remaining);
    push_leaf(leaves, GridRect { x: rect.x + quarter_w, y: rect.y + quarter_h, width: quarter_w, height: quarter_h }, radial_a, Some(ordinal), 2, false, radial_zone(radial_a), false);
    let radial_b = take_radial_family(radial_remaining);
    push_leaf(leaves, GridRect { x: rect.x, y: rect.y + half_w, width: quarter_w, height: quarter_h }, radial_b, Some(ordinal), 2, false, radial_zone(radial_b), false);
    split_small_cell_into_wide_pair(GridRect { x: rect.x + quarter_w, y: rect.y + half_w, width: quarter_w, height: quarter_h }, leaves, Some(ordinal));
    split_small_cell_into_tall_pair(GridRect { x: rect.x + half_w, y: rect.y + half_w, width: quarter_w, height: quarter_h }, leaves, Some(ordinal));
}

fn normalized_ladder(requested: &[u32], extent: u32) -> Vec<u32> {
    let desired_segments = 4.min(extent.max(1)) as usize;
    let mut remaining = extent;
    let mut ladder = Vec::new();
    for value in requested.iter().take(desired_segments.saturating_sub(1)) {
        if remaining == 0 { break; }
        let reserved_tail = desired_segments.saturating_sub(ladder.len() + 1) as u32;
        let thickness = (*value).max(1).min(remaining.saturating_sub(reserved_tail).max(1));
        ladder.push(thickness);
        remaining -= thickness;
    }
    if remaining > 0 { ladder.push(remaining); }
    ladder
}

fn take_radial_family(radial_remaining: &mut u32) -> PartitionFamily {
    if *radial_remaining > 0 {
        *radial_remaining -= 1;
        PartitionFamily::RadialReservation
    } else {
        PartitionFamily::SmallDetail
    }
}

fn radial_zone(family: PartitionFamily) -> HierarchyZone {
    if family == PartitionFamily::RadialReservation { HierarchyZone::Radial } else { HierarchyZone::DetailHost }
}

fn push_leaf(
    leaves: &mut Vec<HierarchicalLeaf>,
    rect: GridRect,
    family: PartitionFamily,
    parent_ordinal: Option<u32>,
    depth: u8,
    protected_parent: bool,
    zone: HierarchyZone,
    splittable: bool,
) {
    leaves.push(HierarchicalLeaf { rect, family,
        lineage: PartitionLineage { parent_ordinal, host_rect: Some(rect), depth, protected_parent, zone }, splittable });
}

fn split_small_cell_into_wide_pair(rect: GridRect, leaves: &mut Vec<HierarchicalLeaf>, parent_ordinal: Option<u32>) {
    let top = rect.height / 2;
    push_leaf(leaves, GridRect { height: top, ..rect }, PartitionFamily::SmallDetail, parent_ordinal, 2, false, HierarchyZone::DetailHost, true);
    push_leaf(leaves, GridRect { y: rect.y + top, height: rect.height - top, ..rect }, PartitionFamily::SmallDetail, parent_ordinal, 2, false, HierarchyZone::DetailHost, true);
}

fn split_small_cell_into_tall_pair(rect: GridRect, leaves: &mut Vec<HierarchicalLeaf>, parent_ordinal: Option<u32>) {
    let left = rect.width / 2;
    push_leaf(leaves, GridRect { width: left, ..rect }, PartitionFamily::SmallDetail, parent_ordinal, 2, false, HierarchyZone::DetailHost, true);
    push_leaf(leaves, GridRect { x: rect.x + left, width: rect.width - left, ..rect }, PartitionFamily::SmallDetail, parent_ordinal, 2, false, HierarchyZone::DetailHost, true);
}

fn replace_first_matching<P, R>(leaves: &mut Vec<HierarchicalLeaf>, predicate: P, mut replacement: R) -> bool
where
    P: Fn(&HierarchicalLeaf) -> bool,
    R: FnMut(HierarchicalLeaf) -> Vec<HierarchicalLeaf>,
{
    if let Some(index) = leaves.iter().position(predicate) {
        let leaf = leaves.remove(index);
        leaves.extend(replacement(leaf));
        true
    } else {
        false
    }
}

fn split_medium_square_for_radials(leaf: HierarchicalLeaf, radial_remaining: &mut u32) -> Vec<HierarchicalLeaf> {
    let rect = leaf.rect;
    let half_w = rect.width / 2;
    let half_h = rect.height / 2;
    let mut output = Vec::with_capacity(4);
    for (x, y, width, height) in [
        (rect.x, rect.y, half_w, half_h),
        (rect.x + half_w, rect.y, rect.width - half_w, half_h),
        (rect.x, rect.y + half_h, half_w, rect.height - half_h),
        (rect.x + half_w, rect.y + half_h, rect.width - half_w, rect.height - half_h),
    ] {
        let family = take_radial_family(radial_remaining);
        output.push(HierarchicalLeaf { rect: GridRect { x, y, width, height }, family,
            lineage: PartitionLineage { parent_ordinal: leaf.lineage.parent_ordinal, host_rect: Some(rect), depth: 2, protected_parent: false, zone: radial_zone(family) },
            splittable: family != PartitionFamily::RadialReservation });
    }
    output
}

fn apply_symmetry_to_leaves(leaves: &mut [HierarchicalLeaf], grid: LogicalGridSpec, symmetry: SymmetryTransform) {
    if symmetry == SymmetryTransform::Identity { return; }
    for leaf in leaves {
        leaf.rect = transform_basis_rect(leaf.rect, grid, symmetry);
        leaf.lineage.host_rect = leaf.lineage.host_rect.map(|rect| transform_basis_rect(rect, grid, symmetry));
    }
}

fn apply_basis_complexity(leaves: &mut Vec<HierarchicalLeaf>, recipe: &PartitionRecipe, hierarchy: &HierarchicalLayoutRecipe) {
    let depth_floor = 24_usize.saturating_add(usize::from(hierarchy.hierarchy_depth.saturating_sub(2)).saturating_mul(3));
    let desired = (hierarchy.target_region_min as usize).max(depth_floor).min(hierarchy.target_region_max as usize);
    if leaves.len() >= desired { return; }
    split_medium_square_pair(leaves);
    if leaves.len() >= desired { return; }
    split_normal_small_square(leaves);
    if leaves.len() >= desired { return; }
    split_detail_squares_until(leaves, desired.min(recipe.target_region_count as usize), hierarchy.target_region_max as usize);
    if leaves.len() >= desired { return; }
    split_micro_pairs_until(leaves, desired.min(recipe.target_region_count as usize).min(hierarchy.target_region_max as usize));
}

fn split_medium_square_pair(leaves: &mut Vec<HierarchicalLeaf>) {
    let Some(first_index) = leaves.iter().position(|leaf| leaf.family == PartitionFamily::MediumBlock && leaf.rect.width == leaf.rect.height && leaf.rect.width >= 4) else { return; };
    let first = leaves.remove(first_index);
    let Some(second_index) = leaves.iter().position(|leaf| leaf.family == PartitionFamily::MediumBlock && leaf.rect.width == leaf.rect.height && leaf.rect.width >= 4) else {
        leaves.push(first);
        return;
    };
    let second = leaves.remove(second_index);
    let split_horizontal = |leaf: HierarchicalLeaf| {
        let half = leaf.rect.height / 2;
        let lineage = PartitionLineage { host_rect: Some(leaf.rect), depth: leaf.lineage.depth.saturating_add(1), zone: HierarchyZone::DetailHost, ..leaf.lineage };
        [
            HierarchicalLeaf { rect: GridRect { height: half, ..leaf.rect }, family: PartitionFamily::MediumBlock, lineage, splittable: true },
            HierarchicalLeaf { rect: GridRect { y: leaf.rect.y + half, height: leaf.rect.height - half, ..leaf.rect }, family: PartitionFamily::MediumBlock, lineage, splittable: true },
        ]
    };
    let split_vertical = |leaf: HierarchicalLeaf| {
        let half = leaf.rect.width / 2;
        let lineage = PartitionLineage { host_rect: Some(leaf.rect), depth: leaf.lineage.depth.saturating_add(1), zone: HierarchyZone::DetailHost, ..leaf.lineage };
        [
            HierarchicalLeaf { rect: GridRect { width: half, ..leaf.rect }, family: PartitionFamily::MediumBlock, lineage, splittable: true },
            HierarchicalLeaf { rect: GridRect { x: leaf.rect.x + half, width: leaf.rect.width - half, ..leaf.rect }, family: PartitionFamily::MediumBlock, lineage, splittable: true },
        ]
    };
    leaves.extend(split_horizontal(first));
    leaves.extend(split_vertical(second));
}

fn split_normal_small_square(leaves: &mut Vec<HierarchicalLeaf>) {
    let Some(index) = leaves.iter().position(|leaf| leaf.family == PartitionFamily::SmallDetail && leaf.rect.width == leaf.rect.height && leaf.rect.width >= 4) else { return; };
    let leaf = leaves.remove(index);
    let half_w = leaf.rect.width / 2;
    let half_h = leaf.rect.height / 2;
    let lineage = PartitionLineage { host_rect: Some(leaf.rect), depth: leaf.lineage.depth.saturating_add(1), zone: HierarchyZone::DetailHost, ..leaf.lineage };
    leaves.extend([
        HierarchicalLeaf { rect: GridRect { x: leaf.rect.x, y: leaf.rect.y, width: half_w, height: half_h }, family: PartitionFamily::SmallDetail, lineage, splittable: true },
        HierarchicalLeaf { rect: GridRect { x: leaf.rect.x + half_w, y: leaf.rect.y, width: leaf.rect.width - half_w, height: half_h }, family: PartitionFamily::SmallDetail, lineage, splittable: true },
        HierarchicalLeaf { rect: GridRect { x: leaf.rect.x, y: leaf.rect.y + half_h, width: half_w, height: leaf.rect.height - half_h }, family: PartitionFamily::SmallDetail, lineage, splittable: true },
        HierarchicalLeaf { rect: GridRect { x: leaf.rect.x + half_w, y: leaf.rect.y + half_h, width: leaf.rect.width - half_w, height: leaf.rect.height - half_h }, family: PartitionFamily::SmallDetail, lineage, splittable: true },
    ]);
}

fn split_detail_squares_until(leaves: &mut Vec<HierarchicalLeaf>, desired: usize, maximum: usize) {
    while leaves.len() < desired && leaves.len() + 3 <= maximum {
        let Some(index) = leaves.iter().position(|leaf| leaf.family == PartitionFamily::SmallDetail && leaf.rect.width == leaf.rect.height && leaf.rect.width >= 4) else { break; };
        let leaf = leaves.remove(index);
        let half_w = leaf.rect.width / 2;
        let half_h = leaf.rect.height / 2;
        let lineage = PartitionLineage { host_rect: Some(leaf.rect), depth: leaf.lineage.depth.saturating_add(1), zone: HierarchyZone::DetailHost, ..leaf.lineage };
        leaves.extend([
            HierarchicalLeaf { rect: GridRect { x: leaf.rect.x, y: leaf.rect.y, width: half_w, height: half_h }, family: PartitionFamily::SmallDetail, lineage, splittable: half_w >= 2 && half_h >= 2 },
            HierarchicalLeaf { rect: GridRect { x: leaf.rect.x + half_w, y: leaf.rect.y, width: leaf.rect.width - half_w, height: half_h }, family: PartitionFamily::SmallDetail, lineage, splittable: half_w >= 2 && half_h >= 2 },
            HierarchicalLeaf { rect: GridRect { x: leaf.rect.x, y: leaf.rect.y + half_h, width: half_w, height: leaf.rect.height - half_h }, family: PartitionFamily::SmallDetail, lineage, splittable: half_w >= 2 && half_h >= 2 },
            HierarchicalLeaf { rect: GridRect { x: leaf.rect.x + half_w, y: leaf.rect.y + half_h, width: leaf.rect.width - half_w, height: leaf.rect.height - half_h }, family: PartitionFamily::SmallDetail, lineage, splittable: half_w >= 2 && half_h >= 2 },
        ]);
    }
}

fn split_micro_pairs_until(leaves: &mut Vec<HierarchicalLeaf>, desired: usize) {
    while leaves.len() + 1 < desired {
        let Some(wide_index) = leaves.iter().position(|leaf| leaf.family == PartitionFamily::SmallDetail && leaf.rect.width >= leaf.rect.height.saturating_mul(2) && leaf.rect.height >= 2) else { break; };
        let wide = leaves.remove(wide_index);
        let Some(tall_index) = leaves.iter().position(|leaf| leaf.family == PartitionFamily::SmallDetail && leaf.rect.height >= leaf.rect.width.saturating_mul(2) && leaf.rect.width >= 2) else {
            leaves.push(wide);
            break;
        };
        let tall = leaves.remove(tall_index);
        let split_wide = split_leaf_horizontal(wide, PartitionFamily::SmallDetail);
        let split_tall = split_leaf_vertical(tall, PartitionFamily::SmallDetail);
        leaves.extend(split_wide);
        leaves.extend(split_tall);
    }
}

fn split_leaf_horizontal(leaf: HierarchicalLeaf, family: PartitionFamily) -> [HierarchicalLeaf; 2] {
    let half = leaf.rect.height / 2;
    let lineage = PartitionLineage { host_rect: Some(leaf.rect), depth: leaf.lineage.depth.saturating_add(1), zone: HierarchyZone::DetailHost, ..leaf.lineage };
    [
        HierarchicalLeaf { rect: GridRect { height: half, ..leaf.rect }, family, lineage, splittable: half >= 2 },
        HierarchicalLeaf { rect: GridRect { y: leaf.rect.y + half, height: leaf.rect.height - half, ..leaf.rect }, family, lineage, splittable: leaf.rect.height - half >= 2 },
    ]
}

fn split_leaf_vertical(leaf: HierarchicalLeaf, family: PartitionFamily) -> [HierarchicalLeaf; 2] {
    let half = leaf.rect.width / 2;
    let lineage = PartitionLineage { host_rect: Some(leaf.rect), depth: leaf.lineage.depth.saturating_add(1), zone: HierarchyZone::DetailHost, ..leaf.lineage };
    [
        HierarchicalLeaf { rect: GridRect { width: half, ..leaf.rect }, family, lineage, splittable: half >= 2 },
        HierarchicalLeaf { rect: GridRect { x: leaf.rect.x + half, width: leaf.rect.width - half, ..leaf.rect }, family, lineage, splittable: leaf.rect.width - half >= 2 },
    ]
}

fn validate_hotspot_basis(grid: LogicalGridSpec, hierarchy: &HierarchicalLayoutRecipe, demands: &[RegionDemand], leaves: &[HierarchicalLeaf]) -> Result<(), PartitionError> {
    validate_exact_cover(grid, leaves)?;
    validate_region_demands(demands)?;
    let mut counts = BTreeMap::<(u32, u32), u32>::new();
    for leaf in leaves {
        if leaf.family == PartitionFamily::RadialReservation && leaf.rect.width != leaf.rect.height {
            return Err(PartitionError::InvalidHotspotBasis { reason: format!("radial region {}x{} is not square", leaf.rect.width, leaf.rect.height) });
        }
        *counts.entry((leaf.rect.width, leaf.rect.height)).or_default() += 1;
    }
    let basis = BasisGrid::new(grid)?;
    let required = [
        (basis.cell_w * 4, basis.cell_h * 4, "macro square"),
        (basis.cell_w * 4, basis.cell_h * 2, "macro wide"),
        (basis.cell_w * 2, basis.cell_h * 4, "macro tall"),
        (basis.cell_w * 2, basis.cell_h * 2, "medium square"),
        (basis.cell_w * 2, basis.cell_h, "medium wide"),
        (basis.cell_w, basis.cell_h * 2, "medium tall"),
        (basis.cell_w, basis.cell_h, "small square"),
    ];
    for (width, height, label) in required {
        if counts.get(&(width, height)).copied().unwrap_or(0) == 0 {
            return Err(PartitionError::InvalidHotspotBasis { reason: format!("missing required {label} {width}x{height}") });
        }
    }
    for (wide, tall, label) in [
        ((basis.cell_w * 4, basis.cell_h * 2), (basis.cell_w * 2, basis.cell_h * 4), "macro pair"),
        ((basis.cell_w * 2, basis.cell_h), (basis.cell_w, basis.cell_h * 2), "medium pair"),
        ((basis.cell_w, (basis.cell_h / 2).max(1)), ((basis.cell_w / 2).max(1), basis.cell_h), "small pair"),
    ] {
        let wide_count = counts.get(&wide).copied().unwrap_or(0);
        let tall_count = counts.get(&tall).copied().unwrap_or(0);
        if wide_count == 0 || tall_count == 0 || wide_count != tall_count {
            return Err(PartitionError::InvalidHotspotBasis { reason: format!("{label} incomplete ({wide_count} wide / {tall_count} tall)") });
        }
    }
    if !leaves.iter().any(|leaf| leaf.family == PartitionFamily::HorizontalStrip)
        || !leaves.iter().any(|leaf| leaf.family == PartitionFamily::VerticalStrip) {
        return Err(PartitionError::InvalidHotspotBasis { reason: "horizontal and vertical strip ladders must both be present".into() });
    }
    if hierarchy.radial_count > 0 {
        let radial_count = leaves.iter().filter(|leaf| leaf.family == PartitionFamily::RadialReservation).count() as u32;
        if radial_count != hierarchy.radial_count {
            return Err(PartitionError::InvalidHotspotBasis { reason: format!("expected {} radial slots, found {radial_count}", hierarchy.radial_count) });
        }
    }
    Ok(())
}

fn validate_exact_cover(grid: LogicalGridSpec, leaves: &[HierarchicalLeaf]) -> Result<(), PartitionError> {
    let mut cells = vec![0_u8; (grid.width * grid.height) as usize];
    for leaf in leaves {
        if leaf.rect.width == 0 || leaf.rect.height == 0 || leaf.rect.x + leaf.rect.width > grid.width || leaf.rect.y + leaf.rect.height > grid.height {
            return Err(PartitionError::InvalidHotspotBasis { reason: format!("region is out of bounds: {:?}", leaf.rect) });
        }
        for y in leaf.rect.y..leaf.rect.y + leaf.rect.height {
            for x in leaf.rect.x..leaf.rect.x + leaf.rect.width {
                let cell = &mut cells[(y * grid.width + x) as usize];
                *cell = cell.saturating_add(1);
            }
        }
    }
    if cells.iter().any(|value| *value != 1) {
        return Err(PartitionError::InvalidHotspotBasis { reason: "regions must cover every logical cell exactly once".into() });
    }
    Ok(())
}

#[allow(dead_code)]
fn hierarchical_macro_zones(grid: LogicalGridSpec, hierarchy: &HierarchicalLayoutRecipe) -> Result<Vec<HierarchicalZoneRect>, PartitionError> {
    let root = GridRect { x: 0, y: 0, width: grid.width, height: grid.height };
    if hierarchy.macro_style == MacroStyle::ClassicSourceHotspot {
        let half_y = grid.height / 2;
        let half_x = grid.width / 2;
        let upper_half = half_y.max(2);
        let quarter_y = (upper_half / 2).max(1);
        return Ok(vec![
            HierarchicalZoneRect { rect: GridRect { x: 0, y: 0, width: half_x, height: quarter_y }, kind: HierarchicalZoneKind::VerticalLadder },
            HierarchicalZoneRect { rect: GridRect { x: 0, y: quarter_y, width: half_x, height: upper_half - quarter_y }, kind: HierarchicalZoneKind::HorizontalLadder },
            HierarchicalZoneRect { rect: GridRect { x: half_x, y: 0, width: grid.width - half_x, height: upper_half }, kind: HierarchicalZoneKind::Cascade },
            HierarchicalZoneRect { rect: GridRect { x: 0, y: upper_half, width: grid.width, height: grid.height - upper_half }, kind: HierarchicalZoneKind::Panels },
        ]);
    }
    let mut zones = Vec::new();
    let mut remainder = root;
    if hierarchy.radial_count > 0 && hierarchy.radial_share_milli > 0 {
        let requested_width = ((u64::from(grid.width) * u64::from(hierarchy.radial_share_milli) + 999) / 1_000) as u32;
        let width = requested_width.max(hierarchy.radial_min_diameter).min(remainder.width.saturating_sub(2).max(1));
        let radial = GridRect { x: remainder.x + remainder.width - width, y: remainder.y, width, height: remainder.height };
        remainder.width -= width;
        zones.push(HierarchicalZoneRect { rect: radial, kind: HierarchicalZoneKind::Radial });
    }
    if hierarchy.strip_share_milli > 0 {
        let vertical_dominant = matches!(hierarchy.macro_style, MacroStyle::VerticalTrims)
            || hierarchy.vertical_strip_weight_milli > hierarchy.horizontal_strip_weight_milli;
        if vertical_dominant {
            let width = ((u64::from(remainder.width) * u64::from(hierarchy.strip_share_milli) + 999) / 1_000) as u32;
            let width = width.max(1).min(remainder.width.saturating_sub(2).max(1));
            let strip = GridRect { x: remainder.x + remainder.width - width, y: remainder.y, width, height: remainder.height };
            remainder.width -= width;
            zones.push(HierarchicalZoneRect { rect: strip, kind: HierarchicalZoneKind::VerticalLadder });
        } else {
            let height = ((u64::from(remainder.height) * u64::from(hierarchy.strip_share_milli) + 999) / 1_000) as u32;
            let height = height.max(1).min(remainder.height.saturating_sub(2).max(1));
            let strip = GridRect { x: remainder.x, y: remainder.y + remainder.height - height, width: remainder.width, height };
            remainder.height -= height;
            if hierarchy.horizontal_strip_weight_milli > 0 && hierarchy.vertical_strip_weight_milli > 0 && strip.width >= 4 {
                let horizontal_width = ((u64::from(strip.width) * u64::from(hierarchy.horizontal_strip_weight_milli)) / 1_000) as u32;
                let horizontal_width = horizontal_width.clamp(2, strip.width - 2);
                zones.push(HierarchicalZoneRect { rect: GridRect { width: horizontal_width, ..strip }, kind: HierarchicalZoneKind::HorizontalLadder });
                zones.push(HierarchicalZoneRect { rect: GridRect { x: strip.x + horizontal_width, width: strip.width - horizontal_width, ..strip }, kind: HierarchicalZoneKind::VerticalLadder });
            } else {
                zones.push(HierarchicalZoneRect { rect: strip, kind: HierarchicalZoneKind::HorizontalLadder });
            }
        }
    }
    zones.push(HierarchicalZoneRect { rect: remainder, kind: HierarchicalZoneKind::Panels });
    Ok(zones)
}

#[allow(dead_code)]
fn generate_panel_zone(
    rect: GridRect,
    context: &mut HierarchicalContext<'_>,
    leaves: &mut Vec<HierarchicalLeaf>,
    parent_hosts: &mut Vec<(u32, GridRect)>,
) -> Result<(), PartitionError> {
    let count = context.hierarchy.macro_parent_count.max(1);
    let mut parents = vec![rect];
    while parents.len() < count as usize {
        let index = parents.iter().enumerate().max_by_key(|(index, value)| (u64::from(value.width) * u64::from(value.height), usize::MAX - *index)).map(|(index, _)| index).unwrap();
        let parent = parents.remove(index);
        let Some((first, second)) = best_hierarchical_split(parent, &context.hierarchy.major_aspects, context, 0) else { parents.push(parent); break; };
        parents.extend([first, second]);
    }
    parents.sort_by_key(|value| std::cmp::Reverse((u64::from(value.width) * u64::from(value.height), value.y, value.x)));
    let parent_base = parent_hosts.len() as u32;
    let preserve_classic_lower_panels = context.hierarchy.macro_style == MacroStyle::ClassicSourceHotspot
        && rect.y >= context.recipe.grid.height / 2;
    for (index, parent) in parents.into_iter().enumerate() {
        let parent_ordinal = parent_base + index as u32;
        parent_hosts.push((parent_ordinal, parent));
        let should_subdivide = !preserve_classic_lower_panels
            && index >= context.hierarchy.protected_parent_count as usize
            && index < context.hierarchy.protected_parent_count.saturating_add(context.hierarchy.subdividable_parent_count) as usize;
        if should_subdivide {
            subdivide_parent(parent, parent_ordinal, context, leaves)?;
        } else {
            leaves.push(HierarchicalLeaf { rect: parent, family: PartitionFamily::BroadPanel,
                lineage: PartitionLineage { parent_ordinal: Some(parent_ordinal), host_rect: Some(parent), depth: 0, protected_parent: true, zone: HierarchyZone::MacroPanel }, splittable: false });
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn generate_cascade_zone(rect: GridRect, context: &mut HierarchicalContext<'_>, leaves: &mut Vec<HierarchicalLeaf>, parent_hosts: &mut Vec<(u32, GridRect)>) -> Result<(), PartitionError> {
    let parent_ordinal = parent_hosts.len() as u32;
    parent_hosts.push((parent_ordinal, rect));
    subdivide_parent(rect, parent_ordinal, context, leaves)
}

#[allow(dead_code)]
fn subdivide_parent(rect: GridRect, parent_ordinal: u32, context: &mut HierarchicalContext<'_>, leaves: &mut Vec<HierarchicalLeaf>) -> Result<(), PartitionError> {
    match context.hierarchy.recursive_policy {
        RecursivePolicy::Cascade => cascade_subdivide(rect, parent_ordinal, context, leaves),
        RecursivePolicy::Balanced => balanced_subdivide(rect, parent_ordinal, context, leaves),
    }
}

#[allow(dead_code)]
fn cascade_subdivide(mut continuation: GridRect, parent_ordinal: u32, context: &mut HierarchicalContext<'_>, leaves: &mut Vec<HierarchicalLeaf>) -> Result<(), PartitionError> {
    let macro_host = continuation;
    for level in 1..=context.hierarchy.hierarchy_depth {
        let palette = if level == 1 { &context.hierarchy.medium_aspects } else { &context.hierarchy.detail_aspects };
        let immediate_host = continuation;
        let Some((first, second)) = best_hierarchical_split(continuation, palette, context, level) else { break; };
        let (terminal, next) = if u64::from(first.width) * u64::from(first.height) <= u64::from(second.width) * u64::from(second.height) { (first, second) } else { (second, first) };
        let last = level == context.hierarchy.hierarchy_depth;
        leaves.push(HierarchicalLeaf { rect: terminal, family: if last { PartitionFamily::SmallDetail } else { PartitionFamily::MediumBlock },
            lineage: PartitionLineage { parent_ordinal: Some(parent_ordinal), host_rect: Some(if last { immediate_host } else { macro_host }), depth: level, protected_parent: false, zone: if last { HierarchyZone::DetailHost } else { HierarchyZone::MacroPanel } }, splittable: !last });
        continuation = next;
        if last {
            leaves.push(HierarchicalLeaf { rect: continuation, family: PartitionFamily::SmallDetail,
                lineage: PartitionLineage { parent_ordinal: Some(parent_ordinal), host_rect: Some(immediate_host), depth: level, protected_parent: false, zone: HierarchyZone::DetailHost }, splittable: true });
            return Ok(());
        }
    }
    leaves.push(HierarchicalLeaf { rect: continuation, family: PartitionFamily::MediumBlock,
        lineage: PartitionLineage { parent_ordinal: Some(parent_ordinal), host_rect: Some(macro_host), depth: 1, protected_parent: false, zone: HierarchyZone::MacroPanel }, splittable: true });
    Ok(())
}

#[allow(dead_code)]
fn balanced_subdivide(rect: GridRect, parent_ordinal: u32, context: &mut HierarchicalContext<'_>, leaves: &mut Vec<HierarchicalLeaf>) -> Result<(), PartitionError> {
    let Some((first, second)) = best_hierarchical_split(rect, &context.hierarchy.medium_aspects, context, 1) else {
        leaves.push(HierarchicalLeaf { rect, family: PartitionFamily::MediumBlock, lineage: PartitionLineage { parent_ordinal: Some(parent_ordinal), host_rect: Some(rect), depth: 1, protected_parent: false, zone: HierarchyZone::MacroPanel }, splittable: true });
        return Ok(());
    };
    for (branch_index, branch) in [first, second].into_iter().enumerate() {
        if context.hierarchy.hierarchy_depth <= 1 || branch_index == 0 {
            leaves.push(HierarchicalLeaf { rect: branch, family: PartitionFamily::MediumBlock, lineage: PartitionLineage { parent_ordinal: Some(parent_ordinal), host_rect: Some(rect), depth: 1, protected_parent: false, zone: HierarchyZone::MacroPanel }, splittable: true });
        } else {
            cascade_subdivide(branch, parent_ordinal, context, leaves)?;
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn generate_strip_ladder(rect: GridRect, horizontal: bool, context: &mut HierarchicalContext<'_>, leaves: &mut Vec<HierarchicalLeaf>) {
    let ladder = &context.hierarchy.strip_thickness_ladder;
    if ladder.is_empty() {
        leaves.push(HierarchicalLeaf { rect, family: PartitionFamily::Remainder, lineage: PartitionLineage { zone: if horizontal { HierarchyZone::HorizontalLadder } else { HierarchyZone::VerticalLadder }, ..PartitionLineage::default() }, splittable: true });
        return;
    }
    let extent = if horizontal { rect.height } else { rect.width };
    let mut offset = 0;
    let mut index = 0;
    while offset < extent {
        let remaining = extent - offset;
        let desired = ladder[index % ladder.len()].min(remaining).max(1);
        let thickness = desired;
        let strip = if horizontal { GridRect { y: rect.y + offset, height: thickness, ..rect } }
            else { GridRect { x: rect.x + offset, width: thickness, ..rect } };
        leaves.push(HierarchicalLeaf { rect: strip, family: if thickness == 1 && index >= ladder.len() { PartitionFamily::MicroStrip } else if horizontal { PartitionFamily::HorizontalStrip } else { PartitionFamily::VerticalStrip },
            lineage: PartitionLineage { depth: 1, zone: if horizontal { HierarchyZone::HorizontalLadder } else { HierarchyZone::VerticalLadder }, ..PartitionLineage::default() }, splittable: false });
        offset += thickness;
        index += 1;
    }
}

#[allow(dead_code)]
fn generate_radial_zone(rect: GridRect, context: &mut HierarchicalContext<'_>, leaves: &mut Vec<HierarchicalLeaf>) -> Result<(), PartitionError> {
    let mut remainders = vec![rect];
    for ordinal in 0..context.hierarchy.radial_count {
        let index = remainders.iter().enumerate().max_by_key(|(_, value)| u64::from(value.width) * u64::from(value.height)).map(|(index, _)| index).ok_or(PartitionError::InvalidHierarchicalRecipe)?;
        let host = remainders.remove(index);
        let available = host.width.min(host.height).min(context.hierarchy.radial_max_diameter);
        if available < context.hierarchy.radial_min_diameter { return Err(PartitionError::InvalidHierarchicalRecipe); }
        let span = context.hierarchy.radial_max_diameter - context.hierarchy.radial_min_diameter + 1;
        let diameter = context.hierarchy.radial_min_diameter + ((context.recipe.seed + u64::from(ordinal)) % u64::from(span)) as u32;
        let diameter = diameter.min(available);
        let square = GridRect { width: diameter, height: diameter, ..host };
        leaves.push(HierarchicalLeaf { rect: square, family: PartitionFamily::RadialReservation,
            lineage: PartitionLineage { depth: 1, zone: HierarchyZone::Radial, ..PartitionLineage::default() }, splittable: false });
        if diameter < host.width { remainders.push(GridRect { x: host.x + diameter, width: host.width - diameter, ..host }); }
        if diameter < host.height { remainders.push(GridRect { y: host.y + diameter, width: diameter, height: host.height - diameter, ..host }); }
    }
    leaves.extend(remainders.into_iter().map(|value| HierarchicalLeaf { rect: value, family: PartitionFamily::Remainder,
        lineage: PartitionLineage { zone: HierarchyZone::Radial, ..PartitionLineage::default() }, splittable: true }));
    Ok(())
}

#[allow(dead_code)]
fn constrained_hierarchical_cleanup(leaves: &mut Vec<HierarchicalLeaf>, context: &mut HierarchicalContext<'_>) {
    while leaves.len() < context.hierarchy.target_region_min as usize && leaves.len() < context.hierarchy.target_region_max as usize {
        let candidate = leaves.iter().enumerate().filter(|(_, leaf)| leaf.splittable && !leaf.lineage.protected_parent
            && !matches!(leaf.family, PartitionFamily::HorizontalStrip | PartitionFamily::VerticalStrip | PartitionFamily::MicroStrip | PartitionFamily::RadialReservation))
            .max_by_key(|(index, leaf)| (u64::from(leaf.rect.width) * u64::from(leaf.rect.height), usize::MAX - *index)).map(|(index, _)| index);
        let Some(index) = candidate else { break; };
        let leaf = leaves.remove(index);
        let palette = if leaf.family == PartitionFamily::MediumBlock { &context.hierarchy.detail_aspects } else { &context.hierarchy.medium_aspects };
        let Some((first, second)) = best_hierarchical_split(leaf.rect, palette, context, leaf.lineage.depth.saturating_add(1)) else {
            leaves.push(HierarchicalLeaf { splittable: false, ..leaf });
            continue;
        };
        let family = if leaf.family == PartitionFamily::MediumBlock { PartitionFamily::SmallDetail } else { leaf.family };
        let lineage = PartitionLineage { host_rect: Some(leaf.rect), depth: leaf.lineage.depth.saturating_add(1), zone: if family == PartitionFamily::SmallDetail { HierarchyZone::DetailHost } else { leaf.lineage.zone }, ..leaf.lineage };
        leaves.extend([HierarchicalLeaf { rect: first, family, lineage, splittable: first.width >= 2 || first.height >= 2 }, HierarchicalLeaf { rect: second, family, lineage, splittable: second.width >= 2 || second.height >= 2 }]);
    }
}

#[allow(dead_code)]
fn best_hierarchical_split(rect: GridRect, palette: &[AspectClass], context: &mut HierarchicalContext<'_>, level: u8) -> Option<(GridRect, GridRect)> {
    let mut best: Option<(i64, bool, u32, GridRect, GridRect)> = None;
    for vertical in [true, false] {
        let extent = if vertical { rect.width } else { rect.height };
        if extent < 2 { continue; }
        for ratio in &context.hierarchy.allowed_split_ratios {
            let (numerator, denominator) = match ratio { SplitRatio::Half => (1, 2), SplitRatio::OneThird => (1, 3), SplitRatio::TwoThird => (2, 3) };
            let base = extent.saturating_mul(numerator) / denominator;
            let variation_span = extent.saturating_mul(u32::from(context.hierarchy.variation_milli)) / 2_000;
            let variation = if variation_span == 0 { 0_i32 } else {
                let range = variation_span * 2 + 1;
                ((context.recipe.seed.wrapping_add(u64::from(context.split_ordinal)).wrapping_add(u64::from(level)) % u64::from(range)) as i32) - variation_span as i32
            };
            let cut = (base as i32 + variation).clamp(1, extent as i32 - 1) as u32;
            let (first, second) = if vertical {
                (GridRect { width: cut, ..rect }, GridRect { x: rect.x + cut, width: rect.width - cut, ..rect })
            } else {
                (GridRect { height: cut, ..rect }, GridRect { y: rect.y + cut, height: rect.height - cut, ..rect })
            };
            if [first, second].iter().any(|value| value.width < context.recipe.minimum_logical_width || value.height < context.recipe.minimum_logical_height) { continue; }
            let aspect_error = i64::from(nearest_aspect_error(first, palette)) + i64::from(nearest_aspect_error(second, palette));
            let total_area = u64::from(rect.width) * u64::from(rect.height);
            let first_area = u64::from(first.width) * u64::from(first.height);
            // Macro cuts respond to the large-panel share; recursive cuts use their own
            // scale falloff. This keeps the simple product slider geometrically meaningful
            // without turning the soft complexity range into an exact leaf quota.
            let desired_milli = if level == 0 { context.hierarchy.large_share_milli.clamp(100, 900) }
                else { context.hierarchy.scale_falloff_milli };
            let desired = total_area * u64::from(desired_milli) / 1_000;
            let area_error = first_area.abs_diff(desired) as i64;
            let tiny_penalty = [first, second].iter().map(|value| if value.width.min(value.height) <= 1 { 20_000 } else if value.width.min(value.height) <= 2 { 2_000 } else { 0 }).sum::<i64>();
            let absolute_cut = if vertical { rect.x + cut } else { rect.y + cut };
            let aligned = if vertical { context.cuts_x.contains(&absolute_cut) } else { context.cuts_y.contains(&absolute_cut) };
            let alignment_bonus = if aligned { i64::from(context.hierarchy.alignment_strength_milli) * 20 } else { 0 };
            let score = aspect_error * 6 + area_error * 4 + tiny_penalty - alignment_bonus;
            let candidate = (score, !vertical, cut, first, second);
            if best.as_ref().is_none_or(|value| candidate.0 < value.0 || (candidate.0 == value.0 && (candidate.1, candidate.2) < (value.1, value.2))) { best = Some(candidate); }
        }
    }
    let (_, vertical_order, cut, first, second) = best?;
    let vertical = !vertical_order;
    if vertical { context.cuts_x.insert(rect.x + cut); } else { context.cuts_y.insert(rect.y + cut); }
    context.split_ordinal = context.split_ordinal.saturating_add(1);
    Some((first, second))
}

#[allow(dead_code)]
fn nearest_aspect_error(rect: GridRect, palette: &[AspectClass]) -> u32 {
    let aspect = u64::from(rect.width) * 1_000 / u64::from(rect.height.max(1));
    palette.iter().map(|class| {
        let target = match class {
            AspectClass::Square => 1_000, AspectClass::Wide2 => 2_000, AspectClass::Tall2 => 500,
            AspectClass::Wide4 => 4_000, AspectClass::Tall4 => 250, AspectClass::Wide8 => 8_000,
            AspectClass::Tall8 => 125, AspectClass::Wide16 => 16_000, AspectClass::Tall16 => 62,
        };
        aspect.abs_diff(target) as u32
    }).min().unwrap_or(u32::MAX)
}

#[derive(Clone, Copy)]
struct PartitionLeaf {
    rect: GridRect,
    family: PartitionFamily,
    depth: u16,
    subdivision_budget: u16,
    fill_remainder: bool,
}

impl PartitionLeaf {
    fn remainder(rect: GridRect, depth: u16, fill_remainder: bool) -> Self {
        Self { rect, family: PartitionFamily::Remainder, depth, subdivision_budget: 0, fill_remainder }
    }
}

fn can_split_leaf(leaf: PartitionLeaf, recipe: &PartitionRecipe) -> bool {
    if leaf.family == PartitionFamily::Remainder {
        return leaf.fill_remainder && (leaf.rect.width >= recipe.minimum_logical_width.saturating_mul(2)
            || leaf.rect.height >= recipe.minimum_logical_height.saturating_mul(2));
    }
    if leaf.subdivision_budget == 0 { return false; }
    let quota = family_quota(leaf.family, &recipe.composition);
    quota.is_none_or(|value| leaf.rect.width >= value.minimum_width.saturating_mul(2)
        || leaf.rect.height >= value.minimum_height.saturating_mul(2))
}

/// Reserve requested families before the deterministic remainder fill.  Each reservation is a
/// real guillotine split, so panel/strip/detail controls alter the persisted rectangles rather
/// than merely decorating an already-generated topology.
fn reserve_composition(recipe: &PartitionRecipe, leaves: &mut Vec<PartitionLeaf>) -> Result<(), PartitionError> {
    let profile = &recipe.composition;
    let requests = [
        (PartitionFamily::BroadPanel, profile.broad_panels.count),
        (PartitionFamily::MediumBlock, profile.medium_blocks.count),
        (PartitionFamily::HorizontalStrip, profile.horizontal_strips.count),
        (PartitionFamily::VerticalStrip, profile.vertical_strips.count),
        (PartitionFamily::SmallDetail, profile.small_details.count),
        (PartitionFamily::MicroStrip, profile.micro_strips.count),
        (PartitionFamily::RadialReservation, profile.radial_reservations.count),
    ];
    for (family, count) in requests {
        for ordinal in 0..count {
            let index = leaves.iter().enumerate().filter(|(_, leaf)| leaf.family == PartitionFamily::Remainder && leaf.fill_remainder)
                .max_by_key(|(_, leaf)| u64::from(leaf.rect.width) * u64::from(leaf.rect.height)).map(|(index, _)| index)
                .ok_or_else(|| PartitionError::ImpossibleFamilyQuota { family, reason: "earlier reservations consume the remaining frame".into(), suggestion: "reduce reserved family counts or increase Count".into() })?;
            let leaf = leaves.remove(index);
            let rect = leaf.rect;
            let depth = leaf.depth;
            let thickness = match family {
                PartitionFamily::HorizontalStrip => Some(varied_strip_thickness(profile.horizontal_strips, recipe.seed, ordinal)),
                PartitionFamily::VerticalStrip => Some(varied_strip_thickness(profile.vertical_strips, recipe.seed, ordinal)),
                PartitionFamily::MicroStrip => Some(varied_strip_thickness(profile.micro_strips, recipe.seed, ordinal)),
                _ => None,
            };
            let (reserved, remainders) = reserve_family_rect(recipe, rect, family, thickness, ordinal)
                .ok_or_else(|| PartitionError::ImpossibleFamilyQuota { family, reason: "no shared boundary can create an eligible reserved rectangle".into(), suggestion: "relax family dimensions/aspect/thickness or reduce requested families".into() })?;
            let subdivision_budget = family_quota(family, profile).map_or(0, |quota| quota.subdivision_budget);
            leaves.push(PartitionLeaf { rect: reserved, family, depth: depth + 1, subdivision_budget, fill_remainder: false });
            leaves.extend(remainders.into_iter().map(|remainder| PartitionLeaf::remainder(remainder, depth + 1, true)));
        }
    }
    Ok(())
}

fn varied_strip_thickness(quota: StripQuota, seed: u64, ordinal: u32) -> u32 {
    let span = quota.maximum_thickness.saturating_sub(quota.minimum_thickness).saturating_add(1);
    quota.minimum_thickness.saturating_add((seed.wrapping_add(u64::from(ordinal)) % u64::from(span.max(1))) as u32)
}

fn reserve_family_rect(recipe: &PartitionRecipe, rect: GridRect, family: PartitionFamily, thickness: Option<u32>, ordinal: u32) -> Option<(GridRect, Vec<GridRect>)> {
    let profile = &recipe.composition;
    let (width, height) = match family {
        PartitionFamily::HorizontalStrip => (rect.width, thickness?.clamp(1, rect.height.saturating_sub(1))),
        PartitionFamily::VerticalStrip => (thickness?.clamp(1, rect.width.saturating_sub(1)), rect.height),
        PartitionFamily::MicroStrip if ordinal % 2 == 0 => (rect.width, thickness?.clamp(1, rect.height.saturating_sub(1))),
        PartitionFamily::MicroStrip => (thickness?.clamp(1, rect.width.saturating_sub(1)), rect.height),
        PartitionFamily::RadialReservation => {
            let quota = profile.radial_reservations;
            let requested_slots = quota.count.saturating_add(1).max(2);
            let columns = (1..=requested_slots).find(|value| (*value).saturating_mul(*value) >= requested_slots).unwrap_or(requested_slots);
            let desired = (recipe.grid.width.min(recipe.grid.height) / columns).max(1);
            let available = rect.width.min(rect.height).min(quota.allocation_max_diameter);
            if available < quota.allocation_min_diameter { return None; }
            let diameter = desired.clamp(quota.allocation_min_diameter, available);
            (diameter, diameter)
        }
        PartitionFamily::BroadPanel | PartitionFamily::MediumBlock | PartitionFamily::SmallDetail => {
            let quota = family_quota(family, profile)?;
            let total_area = u64::from(recipe.grid.width) * u64::from(recipe.grid.height);
            let desired_area = if quota.area_share_milli > 0 && quota.count > 0 {
                total_area.saturating_mul(u64::from(quota.area_share_milli)) / 1_000 / u64::from(quota.count)
            } else {
                let divisor = if family == PartitionFamily::SmallDetail { 4 } else { 2 };
                u64::from(rect.width) * u64::from(rect.height) / divisor
            };
            (quota.minimum_width..=quota.maximum_width.min(rect.width)).flat_map(|width| {
                (quota.minimum_height..=quota.maximum_height.min(rect.height)).map(move |height| (width, height))
            }).filter(|(width, height)| (*width < rect.width || *height < rect.height)
                && family_matches(family, GridRect { x: rect.x, y: rect.y, width: *width, height: *height }, profile))
                .min_by_key(|(width, height)| {
                    let area = u64::from(*width) * u64::from(*height);
                    (area.abs_diff(desired_area), u64::MAX - area, width.abs_diff(*height))
                })?
        }
        PartitionFamily::Remainder => return None,
    };
    if width == 0 || height == 0 || width > rect.width || height > rect.height
        || (width == rect.width && height == rect.height && family != PartitionFamily::RadialReservation) { return None; }
    let reserved = GridRect { x: rect.x, y: rect.y, width, height };
    if !family_matches(family, reserved, profile) { return None; }
    let mut remainders = Vec::with_capacity(2);
    if width < rect.width {
        remainders.push(GridRect { x: rect.x + width, y: rect.y, width: rect.width - width, height: rect.height });
    }
    if height < rect.height {
        remainders.push(GridRect { x: rect.x, y: rect.y + height, width, height: rect.height - height });
    }
    Some((reserved, remainders))
}

fn family_matches(family: PartitionFamily, rect: GridRect, profile: &CompositionProfile) -> bool {
    let aspect_milli = (u64::from(rect.width) * 1_000 / u64::from(rect.height.max(1))) as u16;
    let block = |quota: FamilyQuota| rect.width >= quota.minimum_width && rect.height >= quota.minimum_height
        && rect.width <= quota.maximum_width && rect.height <= quota.maximum_height
        && aspect_milli >= quota.minimum_aspect_milli && aspect_milli <= quota.maximum_aspect_milli;
    match family {
        PartitionFamily::BroadPanel => block(profile.broad_panels),
        PartitionFamily::MediumBlock => block(profile.medium_blocks),
        PartitionFamily::SmallDetail => block(profile.small_details),
        PartitionFamily::HorizontalStrip => rect.height >= profile.horizontal_strips.minimum_thickness && rect.height <= profile.horizontal_strips.maximum_thickness,
        PartitionFamily::VerticalStrip => rect.width >= profile.vertical_strips.minimum_thickness && rect.width <= profile.vertical_strips.maximum_thickness,
        PartitionFamily::MicroStrip => rect.width.min(rect.height) >= profile.micro_strips.minimum_thickness && rect.width.min(rect.height) <= profile.micro_strips.maximum_thickness,
        PartitionFamily::RadialReservation => rect.width.min(rect.height) >= profile.radial_reservations.allocation_min_diameter && rect.width.min(rect.height) <= profile.radial_reservations.allocation_max_diameter,
        PartitionFamily::Remainder => true,
    }
}

fn family_quota(family: PartitionFamily, profile: &CompositionProfile) -> Option<FamilyQuota> {
    match family {
        PartitionFamily::BroadPanel => Some(profile.broad_panels),
        PartitionFamily::MediumBlock => Some(profile.medium_blocks),
        PartitionFamily::SmallDetail => Some(profile.small_details),
        _ => None,
    }
}

pub fn region_id(recipe: &PartitionRecipe, rect: GridRect, ordinal: u32) -> RegionId {
    let mut hasher = Sha256::new();
    hasher.update(b"hot-trimmer-source-frame-region-v1");
    hasher.update(recipe.hash().0);
    hasher.update(rect.x.to_le_bytes()); hasher.update(rect.y.to_le_bytes());
    hasher.update(rect.width.to_le_bytes()); hasher.update(rect.height.to_le_bytes());
    hasher.update(ordinal.to_le_bytes());
    let digest = hasher.finalize();
    RegionId::from_bytes(digest[..16].try_into().expect("sha256 prefix"))
}

pub fn resolve_boundaries(start: u32, extent: u32, cells: u32) -> Vec<u32> {
    (0..=cells).map(|index| {
        (f64::from(start) + f64::from(index) * f64::from(extent) / f64::from(cells)).round() as u32
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::fmt::Write as _;

    fn assert_partition(grid: LogicalGridSpec, target: u32) {
        let recipe = PartitionRecipe::default_for(grid, target, 7);
        let result = generate_partition(&recipe).expect("partition");
        assert_eq!(result.len(), target as usize);
        let mut cells = vec![0_u8; (grid.width * grid.height) as usize];
        for region in result {
            assert!(region.grid_rect.width > 0 && region.grid_rect.height > 0);
            for y in region.grid_rect.y..region.grid_rect.y + region.grid_rect.height {
                for x in region.grid_rect.x..region.grid_rect.x + region.grid_rect.width {
                    let cell = &mut cells[(y * grid.width + x) as usize];
                    *cell += 1;
                }
            }
        }
        assert!(cells.iter().all(|value| *value == 1));
    }

    fn dim_count(regions: &[PartitionRegion], width: u32, height: u32) -> usize {
        regions.iter().filter(|region| region.grid_rect.width == width && region.grid_rect.height == height).count()
    }

    fn assert_hotspot_basis_inventory(regions: &[PartitionRegion], expected_radials: usize) {
        for (width, height, label) in [
            (32, 32, "macro square"),
            (32, 16, "macro wide"),
            (16, 32, "macro tall"),
            (16, 16, "medium square"),
            (16, 8, "medium wide"),
            (8, 16, "medium tall"),
            (8, 8, "small/radial square"),
        ] {
            assert!(dim_count(regions, width, height) > 0, "missing {label} {width}x{height}");
        }
        for ((wide_w, wide_h), (tall_w, tall_h), label) in [
            ((32, 16), (16, 32), "macro pair"),
            ((16, 8), (8, 16), "medium pair"),
            ((8, 4), (4, 8), "small pair"),
        ] {
            let wide = dim_count(regions, wide_w, wide_h);
            let tall = dim_count(regions, tall_w, tall_h);
            assert_eq!(wide, tall, "{label} must stay orientation-balanced");
            assert!(wide > 0, "{label} must be present before duplicates");
        }
        assert!(regions.iter().any(|region| region.family == PartitionFamily::HorizontalStrip), "missing horizontal strip ladder");
        assert!(regions.iter().any(|region| region.family == PartitionFamily::VerticalStrip), "missing vertical strip ladder");
        let radial = regions.iter().filter(|region| region.family == PartitionFamily::RadialReservation).collect::<Vec<_>>();
        assert_eq!(radial.len(), expected_radials, "radial count");
        assert!(radial.iter().all(|region| region.grid_rect.width == region.grid_rect.height), "radial allocation regions must be square");
    }

    #[test]
    fn source_frame_partition_counts_are_not_template_contracts() {
        for target in [16, 63, 103] { assert_partition(LogicalGridSpec::DEFAULT, target); }
    }

    #[test]
    fn hierarchical_default_has_lineage_ladders_radials_and_soft_count() {
        let mut recipe = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, 36, 17);
        recipe.recipe_version = 3;
        recipe.hierarchical = Some(HierarchicalLayoutRecipe::mixed_hierarchy_default());
        let first = generate_partition(&recipe).expect("hierarchical default");
        let second = generate_partition(&recipe).expect("hierarchical default deterministic");
        assert_eq!(first, second);
        assert!((29..=36).contains(&(first.len() as u32)));
        assert_hotspot_basis_inventory(&first, 2);
        assert_eq!(first.iter().filter(|region| region.lineage.protected_parent && region.family == PartitionFamily::BroadPanel).count(), 3);
        assert!(first.iter().any(|region| region.family == PartitionFamily::MediumBlock && region.lineage.parent_ordinal.is_some()));
        assert!(first.iter().any(|region| region.family == PartitionFamily::SmallDetail && region.lineage.depth >= 2));
        let ladder = [1, 1, 2, 2, 3, 4];
        assert!(first.iter().filter(|region| matches!(region.family, PartitionFamily::HorizontalStrip | PartitionFamily::VerticalStrip | PartitionFamily::MicroStrip))
            .all(|region| ladder.contains(&region.grid_rect.width.min(region.grid_rect.height))));
        let radial = first.iter().filter(|region| region.family == PartitionFamily::RadialReservation).collect::<Vec<_>>();
        assert_eq!(radial.len(), 2);
        assert!(radial.iter().all(|region| region.grid_rect.width == region.grid_rect.height && region.lineage.zone == HierarchyZone::Radial));
        let mut cells = vec![0_u8; (recipe.grid.width * recipe.grid.height) as usize];
        for region in first {
            for y in region.grid_rect.y..region.grid_rect.y + region.grid_rect.height {
                for x in region.grid_rect.x..region.grid_rect.x + region.grid_rect.width { cells[(y * recipe.grid.width + x) as usize] += 1; }
            }
        }
        assert!(cells.iter().all(|value| *value == 1));
    }

    #[test]
    fn hierarchical_product_presets_generate_inside_their_soft_ranges() {
        let presets = [
            ("mixed", HierarchicalLayoutRecipe::mixed_hierarchy_default()),
            ("panel-cascade", { let mut value = HierarchicalLayoutRecipe::mixed_hierarchy_default(); value.macro_style = MacroStyle::PanelCascade; value.large_share_milli = 600; value.medium_share_milli = 180; value.small_share_milli = 60; value.strip_share_milli = 120; value.radial_share_milli = 40; value.target_region_min = 29; value.target_region_max = 36; value.radial_count = 2; value }),
            ("horizontal-trims", { let mut value = HierarchicalLayoutRecipe::mixed_hierarchy_default(); value.macro_style = MacroStyle::HorizontalTrims; value.recursive_policy = RecursivePolicy::Balanced; value.large_share_milli = 460; value.medium_share_milli = 160; value.small_share_milli = 60; value.strip_share_milli = 280; value.radial_share_milli = 40; value.target_region_min = 29; value.target_region_max = 38; value.horizontal_strip_weight_milli = 800; value.vertical_strip_weight_milli = 200; value.strip_thickness_ladder = vec![1,1,1,2,2,3,4,6]; value.radial_count = 2; value }),
            ("facade-halving", { let mut value = HierarchicalLayoutRecipe::mixed_hierarchy_default(); value.macro_style = MacroStyle::FacadeHalving; value.recursive_policy = RecursivePolicy::Balanced; value.large_share_milli = 680; value.medium_share_milli = 200; value.small_share_milli = 40; value.strip_share_milli = 40; value.radial_share_milli = 40; value.target_region_min = 24; value.target_region_max = 30; value.hierarchy_depth = 2; value.allowed_split_ratios = vec![SplitRatio::Half]; value.alignment_strength_milli = 1_000; value.variation_milli = 0; value.horizontal_strip_weight_milli = 1_000; value.vertical_strip_weight_milli = 0; value.radial_count = 2; value }),
            ("classic", { let mut value = HierarchicalLayoutRecipe::mixed_hierarchy_default(); value.macro_style = MacroStyle::ClassicHotspotBasis; value.large_share_milli = 540; value.medium_share_milli = 180; value.small_share_milli = 60; value.strip_share_milli = 180; value.radial_share_milli = 40; value.target_region_min = 24; value.target_region_max = 24; value.horizontal_strip_weight_milli = 545; value.vertical_strip_weight_milli = 455; value.radial_count = 2; value.variation_milli = 0; value }),
            ("mechanical", { let mut value = HierarchicalLayoutRecipe::mixed_hierarchy_default(); value.macro_style = MacroStyle::MechanicalRadial; value.recursive_policy = RecursivePolicy::Balanced; value.large_share_milli = 460; value.medium_share_milli = 180; value.small_share_milli = 100; value.strip_share_milli = 140; value.radial_share_milli = 120; value.target_region_min = 32; value.target_region_max = 40; value.radial_count = 4; value.radial_max_diameter = 12; value }),
        ];
        for (name, hierarchy) in presets {
            let mut recipe = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, hierarchy.target_region_max, 5);
            recipe.recipe_version = 3;
            recipe.hierarchical = Some(hierarchy.clone());
            let regions = generate_partition(&recipe).unwrap_or_else(|error| panic!("{name}: {error}"));
            assert!((hierarchy.target_region_min..=hierarchy.target_region_max).contains(&(regions.len() as u32)), "{name} soft count");
            assert_hotspot_basis_inventory(&regions, hierarchy.radial_count as usize);
        }
    }

    #[test]
    fn hierarchical_recipe_migrates_legacy_json_and_rejects_unified_share_errors() {
        assert_eq!(serde_json::to_value(SplitRatio::TwoThird).unwrap(), serde_json::json!("two_third"));
        assert_eq!(serde_json::from_value::<SplitRatio>(serde_json::json!("two_thirds")).unwrap(), SplitRatio::TwoThird, "plural preview payloads remain readable during migration");
        let legacy = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, 24, 3);
        let mut json = serde_json::to_value(&legacy).expect("legacy recipe JSON");
        let object = json.as_object_mut().expect("recipe object");
        object.remove("hierarchical");
        object.insert("schemaVersion".into(), serde_json::json!(2));
        object.insert("recipeVersion".into(), serde_json::json!(2));
        let decoded: PartitionRecipe = serde_json::from_value(json).expect("version-2 recipe remains readable");
        assert!(decoded.hierarchical.is_none());
        assert_eq!(generate_partition(&decoded).expect("legacy generator remains available").len(), 24);

        let mut invalid = HierarchicalLayoutRecipe::mixed_hierarchy_default();
        invalid.large_share_milli -= 1;
        assert!(matches!(invalid.validate(), Err(PartitionError::InvalidHierarchicalShares { total_milli: 999 })));
    }

    #[test]
    fn classic_hotspot_basis_has_exact_required_inventory() {
        let mut hierarchy = HierarchicalLayoutRecipe::mixed_hierarchy_default();
        hierarchy.macro_style = MacroStyle::ClassicHotspotBasis;
        hierarchy.large_share_milli = 540;
        hierarchy.medium_share_milli = 180;
        hierarchy.small_share_milli = 60;
        hierarchy.strip_share_milli = 180;
        hierarchy.radial_share_milli = 40;
        hierarchy.target_region_min = 24;
        hierarchy.target_region_max = 24;
        hierarchy.radial_count = 2;
        hierarchy.variation_milli = 0;
        let mut recipe = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, 24, 0);
        recipe.recipe_version = 3;
        recipe.hierarchical = Some(hierarchy);
        let regions = generate_partition(&recipe).expect("classic hotspot basis");
        assert_eq!(regions.len(), 24);
        assert_hotspot_basis_inventory(&regions, 2);
        assert_eq!(dim_count(&regions, 16, 16), 4, "classic basis carries four 16x16 material variants");
        assert_eq!(dim_count(&regions, 8, 4), 2);
        assert_eq!(dim_count(&regions, 4, 8), 2);
    }

    #[test]
    fn basis_complexity_preserves_required_orientation_pairs() {
        let mut low = HierarchicalLayoutRecipe::mixed_hierarchy_default();
        low.target_region_min = 24;
        low.target_region_max = 24;
        let mut medium = low.clone();
        medium.target_region_min = 29;
        medium.target_region_max = 36;
        let mut high = medium.clone();
        high.target_region_min = 34;
        high.target_region_max = 40;
        for (name, hierarchy) in [("low", low), ("medium", medium), ("high", high)] {
            let mut recipe = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, hierarchy.target_region_max, 11);
            recipe.recipe_version = 3;
            recipe.hierarchical = Some(hierarchy.clone());
            let regions = generate_partition(&recipe).unwrap_or_else(|error| panic!("{name}: {error}"));
            assert!((hierarchy.target_region_min..=hierarchy.target_region_max).contains(&(regions.len() as u32)), "{name} soft range");
            assert_hotspot_basis_inventory(&regions, 2);
        }
    }

    #[test]
    fn basis_rotations_mirroring_and_radial_variants_preserve_roles() {
        for style in [MacroStyle::MixedHierarchy, MacroStyle::PanelCascade, MacroStyle::HorizontalTrims, MacroStyle::FacadeHalving, MacroStyle::ClassicSourceHotspot] {
            let mut hierarchy = HierarchicalLayoutRecipe::mixed_hierarchy_default();
            hierarchy.macro_style = style;
            hierarchy.target_region_min = 29;
            hierarchy.target_region_max = 36;
            let mut recipe = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, 36, 13);
            recipe.recipe_version = 3;
            recipe.hierarchical = Some(hierarchy);
            let regions = generate_partition(&recipe).unwrap_or_else(|error| panic!("{style:?}: {error}"));
            assert_hotspot_basis_inventory(&regions, 2);
        }

        let mut mechanical = HierarchicalLayoutRecipe::mixed_hierarchy_default();
        mechanical.macro_style = MacroStyle::MechanicalRadial;
        mechanical.large_share_milli = 460;
        mechanical.medium_share_milli = 180;
        mechanical.small_share_milli = 100;
        mechanical.strip_share_milli = 140;
        mechanical.radial_share_milli = 120;
        mechanical.radial_count = 4;
        mechanical.target_region_min = 32;
        mechanical.target_region_max = 40;
        let mut recipe = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, 40, 0);
        recipe.recipe_version = 3;
        recipe.hierarchical = Some(mechanical);
        let regions = generate_partition(&recipe).expect("mechanical radial basis");
        assert_hotspot_basis_inventory(&regions, 4);
    }

    fn generated_for_golden(name: &str) -> Vec<PartitionRegion> {
        let hierarchy = golden_hierarchy(name);
        let mut recipe = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, hierarchy.target_region_max, 0);
        recipe.recipe_version = 3;
        recipe.hierarchical = Some(hierarchy);
        generate_partition(&recipe).unwrap_or_else(|error| panic!("{name}: {error}"))
    }

    fn topology_signature(regions: &[PartitionRegion], transform: SymmetryTransform) -> BTreeSet<(PartitionFamily, u32, u32, u32, u32)> {
        regions.iter().map(|region| {
            let rect = transform_basis_rect(region.grid_rect, LogicalGridSpec::DEFAULT, transform);
            (region.family, rect.x, rect.y, rect.width, rect.height)
        }).collect()
    }

    #[test]
    fn visible_layout_families_are_not_just_symmetry_variants() {
        let diagonal = generated_for_golden("mixed-hierarchy");
        let transforms = [
            SymmetryTransform::Identity, SymmetryTransform::Rotate90, SymmetryTransform::Rotate180, SymmetryTransform::Rotate270,
            SymmetryTransform::MirrorX, SymmetryTransform::MirrorY, SymmetryTransform::MirrorDiagonal, SymmetryTransform::MirrorAntiDiagonal,
        ];
        for name in ["panel-cascade", "horizontal-trim-sheet", "facade-halving", "classic-source-hotspot", "mechanical-radial"] {
            let other = topology_signature(&generated_for_golden(name), SymmetryTransform::Identity);
            assert!(transforms.iter().all(|transform| topology_signature(&diagonal, *transform) != other), "{name} collapsed to a rotated or mirrored Diagonal Cascade");
        }
    }

    #[test]
    fn active_hierarchical_controls_affect_topology() {
        let base_hierarchy = HierarchicalLayoutRecipe::mixed_hierarchy_default();
        let layout_for = |hierarchy: HierarchicalLayoutRecipe, seed: u64| {
            let mut recipe = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, hierarchy.target_region_max, seed);
            recipe.recipe_version = 3;
            recipe.hierarchical = Some(hierarchy);
            topology_signature(&generate_partition(&recipe).expect("control layout"), SymmetryTransform::Identity)
        };
        let base = layout_for(base_hierarchy.clone(), 0);
        let mut large_share = base_hierarchy.clone();
        large_share.large_share_milli = 700;
        large_share.medium_share_milli = 80;
        let mut deeper = base_hierarchy.clone();
        deeper.hierarchy_depth = 5;
        deeper.target_region_max = 40;
        let mut strips = base_hierarchy.clone();
        strips.strip_thickness_ladder = vec![4, 2, 1, 1];
        let mut radial = base_hierarchy.clone();
        radial.radial_count = 4;
        let mut oriented = base_hierarchy.clone();
        oriented.symmetry = SymmetryTransform::Rotate90;
        for (name, layout) in [
            ("seed", layout_for(base_hierarchy.clone(), 1)),
            ("large share", layout_for(large_share, 0)),
            ("hierarchy depth", layout_for(deeper, 0)),
            ("strip ladder", layout_for(strips, 0)),
            ("radial count", layout_for(radial, 0)),
            ("orientation", layout_for(oriented, 0)),
        ] {
            assert_ne!(base, layout, "{name} control had no topology effect");
        }
    }

    fn golden_hierarchy(name: &str) -> HierarchicalLayoutRecipe {
        let mut value = HierarchicalLayoutRecipe::mixed_hierarchy_default();
        match name {
            "mixed-hierarchy" => {}
            "panel-cascade" => { value.macro_style = MacroStyle::PanelCascade; value.large_share_milli = 600; value.medium_share_milli = 180; value.small_share_milli = 60; value.strip_share_milli = 120; value.radial_share_milli = 40; value.target_region_min = 29; value.target_region_max = 36; value.radial_count = 2; }
            "horizontal-trim-sheet" => { value.macro_style = MacroStyle::HorizontalTrims; value.recursive_policy = RecursivePolicy::Balanced; value.large_share_milli = 460; value.medium_share_milli = 160; value.small_share_milli = 60; value.strip_share_milli = 280; value.radial_share_milli = 40; value.target_region_min = 29; value.target_region_max = 38; value.horizontal_strip_weight_milli = 800; value.vertical_strip_weight_milli = 200; value.strip_thickness_ladder = vec![1,1,1,2,2,3,4,6]; value.radial_count = 2; }
            "facade-halving" => { value.macro_style = MacroStyle::FacadeHalving; value.recursive_policy = RecursivePolicy::Balanced; value.large_share_milli = 680; value.medium_share_milli = 200; value.small_share_milli = 40; value.strip_share_milli = 40; value.radial_share_milli = 40; value.target_region_min = 24; value.target_region_max = 30; value.hierarchy_depth = 2; value.allowed_split_ratios = vec![SplitRatio::Half]; value.alignment_strength_milli = 1_000; value.variation_milli = 0; value.horizontal_strip_weight_milli = 1_000; value.vertical_strip_weight_milli = 0; value.radial_count = 2; }
            "classic-source-hotspot" => { value.macro_style = MacroStyle::ClassicHotspotBasis; value.large_share_milli = 540; value.medium_share_milli = 180; value.small_share_milli = 60; value.strip_share_milli = 180; value.radial_share_milli = 40; value.target_region_min = 24; value.target_region_max = 24; value.horizontal_strip_weight_milli = 545; value.vertical_strip_weight_milli = 455; value.radial_count = 2; value.variation_milli = 0; }
            "mechanical-radial" => { value.macro_style = MacroStyle::MechanicalRadial; value.recursive_policy = RecursivePolicy::Balanced; value.large_share_milli = 460; value.medium_share_milli = 180; value.small_share_milli = 100; value.strip_share_milli = 140; value.radial_share_milli = 120; value.target_region_min = 32; value.target_region_max = 40; value.radial_count = 4; value.radial_max_diameter = 12; }
            _ => panic!("unknown golden preset {name}"),
        }
        value
    }

    fn hierarchy_svg(name: &str, regions: &[PartitionRegion]) -> String {
        let mut svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"768\" height=\"768\" viewBox=\"0 0 64 64\" aria-label=\"{name} hierarchical layout\">\n<rect width=\"64\" height=\"64\" fill=\"#10141b\"/>\n");
        for (index, region) in regions.iter().enumerate() {
            let (fill, code) = match region.family {
                PartitionFamily::BroadPanel => ("#385a7c", "L"), PartitionFamily::MediumBlock => ("#557d67", "M"),
                PartitionFamily::SmallDetail => ("#9b6c58", "D"), PartitionFamily::HorizontalStrip => ("#876a9e", "H"),
                PartitionFamily::VerticalStrip => ("#8b884b", "V"), PartitionFamily::MicroStrip => ("#b1778f", "u"),
                PartitionFamily::RadialReservation => ("#b0713d", "R"), PartitionFamily::Remainder => ("#343b46", "C"),
            };
            let rect = region.grid_rect;
            writeln!(svg, "<g><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\" fill-opacity=\"0.78\" stroke=\"#d9e2ee\" stroke-width=\"0.16\"/><title>{code}{index:02} {} parent={:?} depth={}</title>", rect.x, rect.y, rect.width, rect.height, fill, region.id, region.lineage.parent_ordinal, region.lineage.depth).unwrap();
            if rect.width >= 5 && rect.height >= 3 {
                writeln!(svg, "<text x=\"{}\" y=\"{}\" font-size=\"1.15\" fill=\"#ffffff\" font-family=\"monospace\">{code}{index:02}</text>", rect.x + 1, f64::from(rect.y) + 1.7).unwrap();
            }
            svg.push_str("</g>\n");
        }
        svg.push_str("</svg>\n");
        svg
    }

    #[test]
    fn hierarchical_layout_goldens_match_reviewed_line_and_id_fixtures() {
        let manifest_directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let update = std::env::var_os("UPDATE_HIERARCHICAL_GOLDENS").is_some();
        let directory = if update { manifest_directory.join("../../target/hierarchical-goldens") } else { manifest_directory.to_path_buf() };
        if update { std::fs::create_dir_all(&directory).expect("create golden directory"); }
        for name in ["mixed-hierarchy", "panel-cascade", "horizontal-trim-sheet", "facade-halving", "classic-source-hotspot", "mechanical-radial"] {
            let hierarchy = golden_hierarchy(name);
            let mut recipe = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, hierarchy.target_region_max, 0);
            recipe.recipe_version = 3;
            recipe.hierarchical = Some(hierarchy);
            let regions = generate_partition(&recipe).unwrap_or_else(|error| panic!("{name}: {error}"));
            let actual = hierarchy_svg(name, &regions);
            let path = directory.join(format!("hierarchical-{name}.golden.svg"));
            if update { std::fs::write(&path, &actual).expect("write golden"); }
            let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing golden {}; run with UPDATE_HIERARCHICAL_GOLDENS=1", path.display()));
            assert_eq!(actual.trim_end(), expected.trim_end(), "{name} golden changed");
        }
    }

    #[test]
    fn boundary_tables_are_shared_and_reconstruct_a_frame() {
        let xs = resolve_boundaries(1_000, 4_000, 64);
        assert_eq!(xs[0], 1_000); assert_eq!(*xs.last().unwrap(), 5_000);
        assert_eq!(xs[1], 1_063);
        assert!(xs.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    #[test]
    fn intentional_source_partition_composition_quotas_are_exact_and_deterministic() {
        let mut recipe = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, 64, 9);
        recipe.composition.broad_panels.count = 2;
        recipe.composition.broad_panels.area_share_milli = 240;
        recipe.composition.medium_blocks.count = 2;
        recipe.composition.medium_blocks.area_share_milli = 160;
        recipe.composition.horizontal_strips.count = 3;
        recipe.composition.vertical_strips.count = 3;
        recipe.composition.small_details.count = 4;
        recipe.composition.small_details.area_share_milli = 80;
        recipe.composition.radial_reservations.count = 2;
        let first = generate_partition(&recipe).expect("composition is feasible");
        let second = generate_partition(&recipe).expect("composition is deterministic");
        assert_eq!(first, second);
        for (family, expected) in [
            (PartitionFamily::BroadPanel, 2), (PartitionFamily::MediumBlock, 2),
            (PartitionFamily::HorizontalStrip, 3), (PartitionFamily::VerticalStrip, 3),
            (PartitionFamily::SmallDetail, 4), (PartitionFamily::RadialReservation, 2),
        ] {
            assert_eq!(first.iter().filter(|region| region.family == family).count(), expected);
        }
        assert!(first.iter().filter(|region| region.family == PartitionFamily::HorizontalStrip)
            .all(|region| region.grid_rect.height >= recipe.composition.horizontal_strips.minimum_thickness));
        assert!(first.iter().filter(|region| region.family == PartitionFamily::BroadPanel)
            .all(|region| region.grid_rect.width >= recipe.composition.broad_panels.minimum_width
                && region.grid_rect.height >= recipe.composition.broad_panels.minimum_height));
        assert_partition(recipe.grid, recipe.target_region_count);
    }

    #[test]
    fn intentional_source_partition_rejects_unsatisfied_quotas_without_partial_layout() {
        let mut recipe = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, 16, 4);
        recipe.composition.broad_panels.count = 17;
        assert!(matches!(generate_partition(&recipe), Err(PartitionError::QuotaExceedsTarget { .. })));
    }

    #[test]
    fn radial_slot_reservations_are_feasible_on_the_default_grid() {
        let mut recipe = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, 32, 15);
        recipe.composition.radial_reservations.count = 4;
        recipe.composition.radial_reservations.allocation_min_diameter = 1;
        recipe.composition.radial_reservations.allocation_max_diameter = 64;
        let regions = generate_partition(&recipe).expect("four radial allocation slots leave legal remainder");
        let radial = regions.iter().filter(|region| region.family == PartitionFamily::RadialReservation).collect::<Vec<_>>();
        assert_eq!(radial.len(), 4);
        assert!(radial.iter().all(|region| region.grid_rect.width == region.grid_rect.height));
        let mut cells = vec![0_u8; (recipe.grid.width * recipe.grid.height) as usize];
        for region in regions {
            for y in region.grid_rect.y..region.grid_rect.y + region.grid_rect.height {
                for x in region.grid_rect.x..region.grid_rect.x + region.grid_rect.width { cells[(y * recipe.grid.width + x) as usize] += 1; }
            }
        }
        assert!(cells.iter().all(|value| *value == 1));
    }

    #[test]
    fn product_default_composition_contains_major_medium_band_trim_and_detail_regions() {
        let mut recipe = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, 48, 0);
        recipe.horizontal_split_bias_milli = 600;
        recipe.vertical_split_bias_milli = 400;
        recipe.variance_milli = 80;
        recipe.composition.broad_panels = FamilyQuota { count: 4, area_share_milli: 480, minimum_width: 10, minimum_height: 9, maximum_width: 64, maximum_height: 64, minimum_aspect_milli: 250, maximum_aspect_milli: 4_000, subdivision_budget: 0 };
        recipe.composition.medium_blocks = FamilyQuota { count: 6, area_share_milli: 200, minimum_width: 4, minimum_height: 4, maximum_width: 32, maximum_height: 32, minimum_aspect_milli: 200, maximum_aspect_milli: 5_000, subdivision_budget: 0 };
        recipe.composition.horizontal_strips = StripQuota { count: 7, minimum_thickness: 1, maximum_thickness: 3 };
        recipe.composition.vertical_strips = StripQuota { count: 5, minimum_thickness: 1, maximum_thickness: 2 };
        recipe.composition.small_details = FamilyQuota { count: 4, area_share_milli: 60, minimum_width: 1, minimum_height: 1, maximum_width: 10, maximum_height: 9, minimum_aspect_milli: 125, maximum_aspect_milli: 8_000, subdivision_budget: 1 };
        recipe.composition.micro_strips = StripQuota { count: 4, minimum_thickness: 1, maximum_thickness: 1 };
        let regions = generate_partition(&recipe).expect("product default composition is feasible");
        assert_eq!(regions.len(), 48);
        for (family, minimum) in [
            (PartitionFamily::BroadPanel, 4), (PartitionFamily::MediumBlock, 6),
            (PartitionFamily::HorizontalStrip, 7), (PartitionFamily::VerticalStrip, 5),
            (PartitionFamily::SmallDetail, 4), (PartitionFamily::MicroStrip, 4),
        ] {
            assert!(regions.iter().filter(|region| region.family == family).count() >= minimum, "missing {family:?}");
        }
        assert!(regions.iter().filter(|region| region.family == PartitionFamily::BroadPanel)
            .all(|region| u64::from(region.grid_rect.width) * u64::from(region.grid_rect.height) >= 64));
        assert!(regions.iter().filter(|region| matches!(region.family, PartitionFamily::HorizontalStrip | PartitionFamily::VerticalStrip | PartitionFamily::MicroStrip))
            .any(|region| region.grid_rect.width.min(region.grid_rect.height) == 1), "one-cell trims remain legal and visible");
    }

    #[test]
    fn six_reference_template_recipes_generate_exact_cover_layouts() {
        struct Reference {
            name: &'static str, target: u32, horizontal_bias: u16, vertical_bias: u16, variance: u16,
            broad: (u32, u16, u32, u32), medium: (u32, u16), horizontal: u32, vertical: u32,
            detail: (u32, u16, u32, u32), micro: u32,
        }
        let references = [
            Reference { name: "sparse-structural", target: 20, horizontal_bias: 350, vertical_bias: 650, variance: 80, broad: (4, 700, 12, 12), medium: (2, 120), horizontal: 2, vertical: 4, detail: (0, 0, 12, 12), micro: 2 },
            Reference { name: "dense-banded", target: 64, horizontal_bias: 820, vertical_bias: 180, variance: 120, broad: (4, 360, 10, 8), medium: (6, 180), horizontal: 12, vertical: 4, detail: (8, 100, 10, 8), micro: 6 },
            Reference { name: "brick-macro", target: 18, horizontal_bias: 400, vertical_bias: 600, variance: 50, broad: (6, 850, 12, 12), medium: (0, 0), horizontal: 1, vertical: 3, detail: (0, 0, 12, 12), micro: 0 },
            Reference { name: "quadrant-hierarchy", target: 56, horizontal_bias: 500, vertical_bias: 500, variance: 0, broad: (5, 500, 10, 10), medium: (4, 150), horizontal: 10, vertical: 10, detail: (4, 50, 8, 8), micro: 4 },
            Reference { name: "progressive-blocks", target: 56, horizontal_bias: 650, vertical_bias: 350, variance: 100, broad: (4, 420, 10, 9), medium: (7, 220), horizontal: 9, vertical: 8, detail: (5, 60, 9, 9), micro: 5 },
            Reference { name: "balanced-mixed", target: 48, horizontal_bias: 600, vertical_bias: 400, variance: 80, broad: (4, 480, 10, 9), medium: (6, 200), horizontal: 7, vertical: 5, detail: (4, 60, 10, 9), micro: 4 },
        ];
        for reference in references {
            let mut recipe = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, reference.target, 0);
            recipe.horizontal_split_bias_milli = reference.horizontal_bias;
            recipe.vertical_split_bias_milli = reference.vertical_bias;
            recipe.variance_milli = reference.variance;
            recipe.composition.profile_id = format!("reference-{}", reference.name);
            recipe.composition.broad_panels = FamilyQuota { count: reference.broad.0, area_share_milli: reference.broad.1, minimum_width: reference.broad.2, minimum_height: reference.broad.3, maximum_width: 64, maximum_height: 64, minimum_aspect_milli: 250, maximum_aspect_milli: 4_000, subdivision_budget: 0 };
            recipe.composition.medium_blocks = FamilyQuota { count: reference.medium.0, area_share_milli: reference.medium.1, minimum_width: 4, minimum_height: 4, maximum_width: 32, maximum_height: 32, minimum_aspect_milli: 200, maximum_aspect_milli: 5_000, subdivision_budget: 0 };
            recipe.composition.horizontal_strips = StripQuota { count: reference.horizontal, minimum_thickness: 1, maximum_thickness: 3 };
            recipe.composition.vertical_strips = StripQuota { count: reference.vertical, minimum_thickness: 1, maximum_thickness: 2 };
            recipe.composition.small_details = FamilyQuota { count: reference.detail.0, area_share_milli: reference.detail.1, minimum_width: 1, minimum_height: 1, maximum_width: reference.detail.2, maximum_height: reference.detail.3, minimum_aspect_milli: 125, maximum_aspect_milli: 8_000, subdivision_budget: u16::from(reference.detail.0 > 0) };
            recipe.composition.micro_strips = StripQuota { count: reference.micro, minimum_thickness: 1, maximum_thickness: if reference.name == "sparse-structural" { 1 } else { 2 } };
            let regions = generate_partition(&recipe).unwrap_or_else(|error| panic!("{} must generate: {error}", reference.name));
            assert_eq!(regions.len(), reference.target as usize, "{} reaches its declared Count", reference.name);
            for (family, expected) in [
                (PartitionFamily::BroadPanel, reference.broad.0), (PartitionFamily::MediumBlock, reference.medium.0),
                (PartitionFamily::HorizontalStrip, reference.horizontal), (PartitionFamily::VerticalStrip, reference.vertical),
                (PartitionFamily::SmallDetail, reference.detail.0), (PartitionFamily::MicroStrip, reference.micro),
            ] {
                assert_eq!(regions.iter().filter(|region| region.family == family).count(), expected as usize, "{} reserves {family:?}", reference.name);
            }
            if reference.horizontal >= 3 {
                let thicknesses = regions.iter().filter(|region| region.family == PartitionFamily::HorizontalStrip).map(|region| region.grid_rect.height).collect::<BTreeSet<_>>();
                assert!(thicknesses.len() >= 2, "{} uses the configured horizontal thickness range", reference.name);
            }
            let mut cells = vec![0_u8; (recipe.grid.width * recipe.grid.height) as usize];
            for region in regions {
                for y in region.grid_rect.y..region.grid_rect.y + region.grid_rect.height {
                    for x in region.grid_rect.x..region.grid_rect.x + region.grid_rect.width { cells[(y * recipe.grid.width + x) as usize] += 1; }
                }
            }
            assert!(cells.iter().all(|value| *value == 1), "{} remains an exact cover", reference.name);
        }
    }

    #[test]
    fn macro_composition_fixtures_preserve_large_zones_and_localize_detail() {
        let mut panels = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, 20, 12);
        panels.composition.broad_panels.count = 4;
        panels.composition.broad_panels.area_share_milli = 800;
        panels.composition.horizontal_strips.count = 2;
        panels.composition.horizontal_strips.maximum_thickness = 3;
        panels.composition.vertical_strips.count = 2;
        panels.composition.vertical_strips.maximum_thickness = 3;
        let panel_regions = generate_partition(&panels).expect("four huge panels plus thin trims");
        let broad = panel_regions.iter().filter(|region| region.family == PartitionFamily::BroadPanel).map(|region| region.grid_rect).collect::<Vec<_>>();
        assert_eq!(broad.len(), 4);
        assert!(broad.iter().map(|rect| u64::from(rect.width) * u64::from(rect.height)).sum::<u64>() > 2_800);
        let mut denser = panels.clone();
        denser.target_region_count = 48;
        let dense_broad = generate_partition(&denser).expect("remainder accepts denser detail").into_iter()
            .filter(|region| region.family == PartitionFamily::BroadPanel).map(|region| region.grid_rect).collect::<Vec<_>>();
        assert_eq!(dense_broad, broad, "increasing Count must not recursively cut protected broad panels");

        let mut horizontal = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, 24, 13);
        horizontal.horizontal_split_bias_milli = 850;
        horizontal.vertical_split_bias_milli = 150;
        horizontal.composition.broad_panels.count = 2;
        horizontal.composition.broad_panels.area_share_milli = 520;
        horizontal.composition.horizontal_strips.count = 9;
        horizontal.composition.horizontal_strips.maximum_thickness = 3;
        let horizontal_regions = generate_partition(&horizontal).expect("horizontal-band-heavy fixture");
        assert_eq!(horizontal_regions.iter().filter(|region| region.family == PartitionFamily::HorizontalStrip).count(), 9);

        let mut vertical = horizontal.clone();
        vertical.horizontal_split_bias_milli = 150;
        vertical.vertical_split_bias_milli = 850;
        vertical.composition.horizontal_strips.count = 0;
        vertical.composition.vertical_strips.count = 9;
        vertical.composition.vertical_strips.maximum_thickness = 3;
        let vertical_regions = generate_partition(&vertical).expect("vertical-trim-heavy fixture");
        assert_eq!(vertical_regions.iter().filter(|region| region.family == PartitionFamily::VerticalStrip).count(), 9);

        let mut mixed = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, 32, 14);
        mixed.composition.broad_panels.count = 4;
        mixed.composition.broad_panels.area_share_milli = 600;
        mixed.composition.broad_panels.subdivision_budget = 1;
        mixed.composition.medium_blocks.count = 3;
        mixed.composition.medium_blocks.area_share_milli = 180;
        mixed.composition.horizontal_strips.count = 3;
        mixed.composition.vertical_strips.count = 4;
        mixed.composition.small_details.count = 3;
        mixed.composition.small_details.area_share_milli = 80;
        mixed.composition.small_details.subdivision_budget = 1;
        let mixed_regions = generate_partition(&mixed).expect("mixed intentional hierarchy fixture");
        assert_eq!(mixed_regions.len(), 32);
        assert!(mixed_regions.iter().any(|region| region.family == PartitionFamily::BroadPanel));
        assert!(mixed_regions.iter().any(|region| region.family == PartitionFamily::SmallDetail));
    }
}
