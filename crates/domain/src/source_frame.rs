use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

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
pub enum MacroStyle { MixedHierarchy, PanelCascade, HorizontalTrims, VerticalTrims, FacadeHalving, ClassicSourceHotspot, MechanicalRadial }

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecursivePolicy { Cascade, Balanced }

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
}

impl HierarchicalLayoutRecipe {
    pub fn mixed_hierarchy_default() -> Self {
        Self {
            schema_version: 1, macro_style: MacroStyle::MixedHierarchy, recursive_policy: RecursivePolicy::Cascade,
            target_region_min: 30, target_region_max: 40,
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
        }
    }

    fn validate(&self) -> Result<(), PartitionError> {
        let shares = u32::from(self.large_share_milli) + u32::from(self.medium_share_milli)
            + u32::from(self.small_share_milli) + u32::from(self.strip_share_milli) + u32::from(self.radial_share_milli);
        if self.schema_version == 0 || shares != 1_000 {
            return Err(PartitionError::InvalidHierarchicalShares { total_milli: shares });
        }
        if self.target_region_min == 0 || self.target_region_min > self.target_region_max || self.target_region_max > MAX_PARTITION_REGIONS
            || self.hierarchy_depth == 0 || self.scale_falloff_milli == 0 || self.scale_falloff_milli >= 1_000
            || self.protected_parent_count.saturating_add(self.subdividable_parent_count) > self.macro_parent_count
            || self.allowed_split_ratios.is_empty() || self.alignment_strength_milli > 1_000 || self.variation_milli > 1_000
            || u32::from(self.horizontal_strip_weight_milli) + u32::from(self.vertical_strip_weight_milli) != 1_000
            || (self.strip_share_milli > 0 && self.strip_thickness_ladder.iter().any(|value| *value == 0))
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
enum HierarchicalZoneKind { Panels, HorizontalLadder, VerticalLadder, Cascade, Radial }

#[derive(Clone, Copy, Debug)]
struct HierarchicalZoneRect { rect: GridRect, kind: HierarchicalZoneKind }

#[derive(Clone, Copy, Debug)]
struct HierarchicalLeaf {
    rect: GridRect,
    family: PartitionFamily,
    lineage: PartitionLineage,
    splittable: bool,
}

struct HierarchicalContext<'a> {
    recipe: &'a PartitionRecipe,
    hierarchy: &'a HierarchicalLayoutRecipe,
    cuts_x: BTreeSet<u32>,
    cuts_y: BTreeSet<u32>,
    split_ordinal: u32,
}

fn generate_hierarchical_partition(recipe: &PartitionRecipe) -> Result<Vec<PartitionRegion>, PartitionError> {
    recipe.grid.validate()?;
    let hierarchy = recipe.hierarchical.as_ref().ok_or(PartitionError::InvalidHierarchicalRecipe)?;
    hierarchy.validate()?;
    let mut context = HierarchicalContext { recipe, hierarchy, cuts_x: BTreeSet::new(), cuts_y: BTreeSet::new(), split_ordinal: 0 };
    let zones = hierarchical_macro_zones(recipe.grid, hierarchy)?;
    let mut leaves = Vec::<HierarchicalLeaf>::new();
    let mut parent_hosts = Vec::<(u32, GridRect)>::new();
    for zone in zones {
        match zone.kind {
            HierarchicalZoneKind::Panels => generate_panel_zone(zone.rect, &mut context, &mut leaves, &mut parent_hosts)?,
            HierarchicalZoneKind::Cascade => generate_cascade_zone(zone.rect, &mut context, &mut leaves, &mut parent_hosts)?,
            HierarchicalZoneKind::HorizontalLadder => generate_strip_ladder(zone.rect, true, &mut context, &mut leaves),
            HierarchicalZoneKind::VerticalLadder => generate_strip_ladder(zone.rect, false, &mut context, &mut leaves),
            HierarchicalZoneKind::Radial => generate_radial_zone(zone.rect, &mut context, &mut leaves)?,
        }
    }
    if leaves.len() > hierarchy.target_region_max as usize { return Err(PartitionError::ImpossibleTarget); }
    constrained_hierarchical_cleanup(&mut leaves, &mut context);
    if leaves.len() > hierarchy.target_region_max as usize { return Err(PartitionError::ImpossibleTarget); }
    leaves.sort_by_key(|leaf| (leaf.rect.y, leaf.rect.x, leaf.rect.height, leaf.rect.width, leaf.family));
    Ok(leaves.into_iter().enumerate().map(|(index, leaf)| PartitionRegion {
        id: region_id(recipe, leaf.rect, index as u32), grid_rect: leaf.rect, family: leaf.family, lineage: leaf.lineage,
    }).collect())
}

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

fn generate_cascade_zone(rect: GridRect, context: &mut HierarchicalContext<'_>, leaves: &mut Vec<HierarchicalLeaf>, parent_hosts: &mut Vec<(u32, GridRect)>) -> Result<(), PartitionError> {
    let parent_ordinal = parent_hosts.len() as u32;
    parent_hosts.push((parent_ordinal, rect));
    subdivide_parent(rect, parent_ordinal, context, leaves)
}

fn subdivide_parent(rect: GridRect, parent_ordinal: u32, context: &mut HierarchicalContext<'_>, leaves: &mut Vec<HierarchicalLeaf>) -> Result<(), PartitionError> {
    match context.hierarchy.recursive_policy {
        RecursivePolicy::Cascade => cascade_subdivide(rect, parent_ordinal, context, leaves),
        RecursivePolicy::Balanced => balanced_subdivide(rect, parent_ordinal, context, leaves),
    }
}

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
    use std::collections::{BTreeMap, BTreeSet};
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

    #[test]
    fn source_frame_partition_counts_are_not_template_contracts() {
        for target in [16, 63, 103] { assert_partition(LogicalGridSpec::DEFAULT, target); }
    }

    #[test]
    fn hierarchical_default_has_lineage_ladders_radials_and_soft_count() {
        let mut recipe = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, 40, 17);
        recipe.recipe_version = 3;
        recipe.hierarchical = Some(HierarchicalLayoutRecipe::mixed_hierarchy_default());
        let first = generate_partition(&recipe).expect("hierarchical default");
        let second = generate_partition(&recipe).expect("hierarchical default deterministic");
        assert_eq!(first, second);
        assert!((30..=40).contains(&(first.len() as u32)));
        assert_eq!(first.iter().filter(|region| region.lineage.protected_parent && region.family == PartitionFamily::BroadPanel).count(), 2);
        assert!(first.iter().any(|region| region.family == PartitionFamily::MediumBlock && region.lineage.parent_ordinal.is_some()));
        assert!(first.iter().any(|region| region.family == PartitionFamily::SmallDetail && region.lineage.depth >= 2));
        assert!(first.iter().filter(|region| matches!(region.family, PartitionFamily::MediumBlock | PartitionFamily::SmallDetail)).all(|region| {
            let host = region.lineage.host_rect.expect("hierarchical child host");
            region.grid_rect.x >= host.x && region.grid_rect.y >= host.y
                && region.grid_rect.x + region.grid_rect.width <= host.x + host.width
                && region.grid_rect.y + region.grid_rect.height <= host.y + host.height
        }), "medium/detail leaves remain inside their recorded parent lineage");
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
            ("panel-cascade", { let mut value = HierarchicalLayoutRecipe::mixed_hierarchy_default(); value.macro_style = MacroStyle::PanelCascade; value.large_share_milli = 640; value.medium_share_milli = 180; value.small_share_milli = 60; value.strip_share_milli = 120; value.radial_share_milli = 0; value.target_region_min = 28; value.target_region_max = 38; value.radial_count = 0; value }),
            ("horizontal-trims", { let mut value = HierarchicalLayoutRecipe::mixed_hierarchy_default(); value.macro_style = MacroStyle::HorizontalTrims; value.recursive_policy = RecursivePolicy::Balanced; value.large_share_milli = 480; value.medium_share_milli = 160; value.small_share_milli = 60; value.strip_share_milli = 300; value.radial_share_milli = 0; value.target_region_min = 34; value.target_region_max = 48; value.horizontal_strip_weight_milli = 800; value.vertical_strip_weight_milli = 200; value.strip_thickness_ladder = vec![1,1,1,2,2,3,4,6]; value.radial_count = 0; value }),
            ("facade-halving", { let mut value = HierarchicalLayoutRecipe::mixed_hierarchy_default(); value.macro_style = MacroStyle::FacadeHalving; value.recursive_policy = RecursivePolicy::Balanced; value.large_share_milli = 720; value.medium_share_milli = 200; value.small_share_milli = 40; value.strip_share_milli = 40; value.radial_share_milli = 0; value.target_region_min = 12; value.target_region_max = 22; value.hierarchy_depth = 2; value.allowed_split_ratios = vec![SplitRatio::Half]; value.alignment_strength_milli = 1_000; value.variation_milli = 0; value.horizontal_strip_weight_milli = 1_000; value.vertical_strip_weight_milli = 0; value.radial_count = 0; value }),
            ("classic", { let mut value = HierarchicalLayoutRecipe::mixed_hierarchy_default(); value.macro_style = MacroStyle::ClassicSourceHotspot; value.large_share_milli = 540; value.medium_share_milli = 180; value.small_share_milli = 60; value.strip_share_milli = 220; value.radial_share_milli = 0; value.target_region_min = 30; value.target_region_max = 46; value.horizontal_strip_weight_milli = 545; value.vertical_strip_weight_milli = 455; value.radial_count = 0; value }),
            ("mechanical", { let mut value = HierarchicalLayoutRecipe::mixed_hierarchy_default(); value.macro_style = MacroStyle::MechanicalRadial; value.recursive_policy = RecursivePolicy::Balanced; value.large_share_milli = 480; value.medium_share_milli = 180; value.small_share_milli = 100; value.strip_share_milli = 140; value.radial_share_milli = 100; value.target_region_min = 30; value.target_region_max = 44; value.radial_count = 4; value.radial_max_diameter = 12; value }),
        ];
        for (name, hierarchy) in presets {
            let mut recipe = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, hierarchy.target_region_max, 5);
            recipe.recipe_version = 3;
            recipe.hierarchical = Some(hierarchy.clone());
            let regions = generate_partition(&recipe).unwrap_or_else(|error| panic!("{name}: {error}"));
            assert!((hierarchy.target_region_min..=hierarchy.target_region_max).contains(&(regions.len() as u32)), "{name} soft count");
            assert!(regions.iter().filter(|region| region.lineage.protected_parent).count() >= hierarchy.protected_parent_count.min(2) as usize, "{name} protected parents");
            if hierarchy.radial_count > 0 { assert_eq!(regions.iter().filter(|region| region.family == PartitionFamily::RadialReservation).count(), hierarchy.radial_count as usize, "{name} radial count"); }
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
    fn hierarchical_product_controls_change_geometry_without_cutting_protected_parents() {
        let recipe_for_share = |large_share_milli: u16| {
            let mut hierarchy = HierarchicalLayoutRecipe::mixed_hierarchy_default();
            hierarchy.large_share_milli = large_share_milli;
            hierarchy.medium_share_milli = 900 - large_share_milli;
            hierarchy.small_share_milli = 100;
            hierarchy.strip_share_milli = 0;
            hierarchy.radial_share_milli = 0;
            hierarchy.macro_parent_count = 2;
            hierarchy.protected_parent_count = 2;
            hierarchy.subdividable_parent_count = 0;
            hierarchy.target_region_min = 2;
            hierarchy.target_region_max = 2;
            hierarchy.variation_milli = 0;
            hierarchy.radial_count = 0;
            let mut recipe = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, 2, 0);
            recipe.recipe_version = 3;
            recipe.hierarchical = Some(hierarchy);
            recipe
        };
        let low = generate_partition(&recipe_for_share(400)).expect("low large share");
        let high = generate_partition(&recipe_for_share(700)).expect("high large share");
        assert_ne!(low.iter().map(|region| region.grid_rect).collect::<Vec<_>>(), high.iter().map(|region| region.grid_rect).collect::<Vec<_>>(), "large-panel share changes macro cut geometry");
        assert!(low.iter().chain(high.iter()).all(|region| region.lineage.protected_parent && region.family == PartitionFamily::BroadPanel));

        let mut base = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, 40, 17);
        base.recipe_version = 3;
        base.hierarchical = Some(HierarchicalLayoutRecipe::mixed_hierarchy_default());
        let protected = generate_partition(&base).expect("base complexity").into_iter().filter(|region| region.lineage.protected_parent).map(|region| region.grid_rect).collect::<Vec<_>>();
        let hierarchy = base.hierarchical.as_mut().unwrap();
        hierarchy.target_region_min = 44;
        hierarchy.target_region_max = 50;
        base.target_region_count = 50;
        let denser = generate_partition(&base).expect("higher soft complexity");
        let dense_protected = denser.into_iter().filter(|region| region.lineage.protected_parent).map(|region| region.grid_rect).collect::<Vec<_>>();
        assert_eq!(dense_protected, protected, "soft complexity only refines eligible descendants");
    }

    #[test]
    fn high_alignment_reuses_global_cut_coordinates_across_parent_branches() {
        let mut hierarchy = HierarchicalLayoutRecipe::mixed_hierarchy_default();
        hierarchy.strip_share_milli = 0;
        hierarchy.radial_share_milli = 0;
        hierarchy.large_share_milli = 650;
        hierarchy.medium_share_milli = 250;
        hierarchy.small_share_milli = 100;
        hierarchy.radial_count = 0;
        hierarchy.alignment_strength_milli = 1_000;
        hierarchy.variation_milli = 0;
        hierarchy.target_region_min = 20;
        hierarchy.target_region_max = 32;
        let mut recipe = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, 32, 0);
        recipe.recipe_version = 3;
        recipe.hierarchical = Some(hierarchy);
        let regions = generate_partition(&recipe).expect("aligned hierarchy");
        let mut vertical_cuts = BTreeMap::<u32, BTreeSet<u32>>::new();
        let mut horizontal_cuts = BTreeMap::<u32, BTreeSet<u32>>::new();
        for region in regions.iter().filter(|region| region.lineage.parent_ordinal.is_some() && !region.lineage.protected_parent) {
            let parent = region.lineage.parent_ordinal.unwrap();
            for x in [region.grid_rect.x, region.grid_rect.x + region.grid_rect.width] { vertical_cuts.entry(x).or_default().insert(parent); }
            for y in [region.grid_rect.y, region.grid_rect.y + region.grid_rect.height] { horizontal_cuts.entry(y).or_default().insert(parent); }
        }
        assert!(vertical_cuts.values().chain(horizontal_cuts.values()).any(|parents| parents.len() >= 2), "aligned recursive branches reuse an absolute X or Y cut");
    }

    #[test]
    fn hierarchical_aspect_palette_and_classic_grammar_are_applied() {
        let recipe_for_aspect = |aspect: AspectClass| {
            let mut hierarchy = HierarchicalLayoutRecipe::mixed_hierarchy_default();
            hierarchy.large_share_milli = 1_000;
            hierarchy.medium_share_milli = 0;
            hierarchy.small_share_milli = 0;
            hierarchy.strip_share_milli = 0;
            hierarchy.radial_share_milli = 0;
            hierarchy.major_aspects = vec![aspect];
            hierarchy.macro_parent_count = 2;
            hierarchy.protected_parent_count = 2;
            hierarchy.subdividable_parent_count = 0;
            hierarchy.target_region_min = 2;
            hierarchy.target_region_max = 2;
            hierarchy.allowed_split_ratios = vec![SplitRatio::Half];
            hierarchy.variation_milli = 0;
            hierarchy.radial_count = 0;
            let mut recipe = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, 2, 0);
            recipe.recipe_version = 3;
            recipe.hierarchical = Some(hierarchy);
            recipe
        };
        let tall = generate_partition(&recipe_for_aspect(AspectClass::Tall2)).expect("tall palette");
        let wide = generate_partition(&recipe_for_aspect(AspectClass::Wide2)).expect("wide palette");
        assert!(tall.iter().all(|region| region.grid_rect.height > region.grid_rect.width));
        assert!(wide.iter().all(|region| region.grid_rect.width > region.grid_rect.height));

        let mut hierarchy = HierarchicalLayoutRecipe::mixed_hierarchy_default();
        hierarchy.macro_style = MacroStyle::ClassicSourceHotspot;
        hierarchy.large_share_milli = 540;
        hierarchy.medium_share_milli = 180;
        hierarchy.small_share_milli = 60;
        hierarchy.strip_share_milli = 220;
        hierarchy.radial_share_milli = 0;
        hierarchy.radial_count = 0;
        hierarchy.target_region_min = 30;
        hierarchy.target_region_max = 46;
        let mut classic = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, 46, 0);
        classic.recipe_version = 3;
        classic.hierarchical = Some(hierarchy);
        let regions = generate_partition(&classic).expect("classic authored grammar");
        let lower = regions.iter().filter(|region| region.lineage.protected_parent && region.grid_rect.y >= 32).collect::<Vec<_>>();
        assert_eq!(lower.len(), 4, "Classic preserves four large lower-half panels");
        assert!(lower.iter().all(|region| region.grid_rect.y + region.grid_rect.height <= 64));
    }

    fn golden_hierarchy(name: &str) -> HierarchicalLayoutRecipe {
        let mut value = HierarchicalLayoutRecipe::mixed_hierarchy_default();
        match name {
            "mixed-hierarchy" => {}
            "panel-cascade" => { value.macro_style = MacroStyle::PanelCascade; value.large_share_milli = 640; value.medium_share_milli = 180; value.small_share_milli = 60; value.strip_share_milli = 120; value.radial_share_milli = 0; value.target_region_min = 28; value.target_region_max = 38; value.radial_count = 0; }
            "horizontal-trim-sheet" => { value.macro_style = MacroStyle::HorizontalTrims; value.recursive_policy = RecursivePolicy::Balanced; value.large_share_milli = 480; value.medium_share_milli = 160; value.small_share_milli = 60; value.strip_share_milli = 300; value.radial_share_milli = 0; value.target_region_min = 34; value.target_region_max = 48; value.horizontal_strip_weight_milli = 800; value.vertical_strip_weight_milli = 200; value.strip_thickness_ladder = vec![1,1,1,2,2,3,4,6]; value.radial_count = 0; }
            "facade-halving" => { value.macro_style = MacroStyle::FacadeHalving; value.recursive_policy = RecursivePolicy::Balanced; value.large_share_milli = 720; value.medium_share_milli = 200; value.small_share_milli = 40; value.strip_share_milli = 40; value.radial_share_milli = 0; value.target_region_min = 12; value.target_region_max = 22; value.hierarchy_depth = 2; value.allowed_split_ratios = vec![SplitRatio::Half]; value.alignment_strength_milli = 1_000; value.variation_milli = 0; value.horizontal_strip_weight_milli = 1_000; value.vertical_strip_weight_milli = 0; value.radial_count = 0; }
            "classic-source-hotspot" => { value.macro_style = MacroStyle::ClassicSourceHotspot; value.large_share_milli = 540; value.medium_share_milli = 180; value.small_share_milli = 60; value.strip_share_milli = 220; value.radial_share_milli = 0; value.target_region_min = 30; value.target_region_max = 46; value.horizontal_strip_weight_milli = 545; value.vertical_strip_weight_milli = 455; value.radial_count = 0; }
            "mechanical-radial" => { value.macro_style = MacroStyle::MechanicalRadial; value.recursive_policy = RecursivePolicy::Balanced; value.large_share_milli = 480; value.medium_share_milli = 180; value.small_share_milli = 100; value.strip_share_milli = 140; value.radial_share_milli = 100; value.target_region_min = 30; value.target_region_max = 44; value.radial_count = 4; value.radial_max_diameter = 12; }
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
