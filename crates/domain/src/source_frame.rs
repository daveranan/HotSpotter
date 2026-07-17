use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{DocumentHash, RegionId};

pub const SOURCE_FRAME_SCHEMA_VERSION: u16 = 1;
pub const LOGICAL_GRID_SCHEMA_VERSION: u16 = 1;
pub const PARTITION_RECIPE_SCHEMA_VERSION: u16 = 1;
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
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartitionProvenance {
    pub schema_version: u16,
    pub recipe: PartitionRecipe,
    pub recipe_hash: DocumentHash,
    pub accepted_region_ids: Vec<RegionId>,
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
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PartitionError {
    #[error("logical grid dimensions must be within 1..={MAX_LOGICAL_GRID_EDGE}")]
    InvalidGrid,
    #[error("target region count must be within 1..={MAX_PARTITION_REGIONS}")]
    InvalidTarget,
    #[error("target region count cannot fit the requested minimum logical region size")]
    ImpossibleTarget,
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
        }
    }

    pub fn validate(&self) -> Result<(), PartitionError> {
        self.grid.validate()?;
        if self.schema_version == 0 || self.recipe_version == 0
            || self.target_region_count == 0 || self.target_region_count > MAX_PARTITION_REGIONS {
            return Err(PartitionError::InvalidTarget);
        }
        let capacity = (self.grid.width / self.minimum_logical_width.max(1))
            .saturating_mul(self.grid.height / self.minimum_logical_height.max(1));
        if capacity < self.target_region_count { return Err(PartitionError::ImpossibleTarget); }
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
    recipe.validate()?;
    let mut leaves = vec![GridRect { x: 0, y: 0, width: recipe.grid.width, height: recipe.grid.height }];
    while leaves.len() < recipe.target_region_count as usize {
        let index = leaves.iter().enumerate().max_by_key(|(index, rect)| {
            (u64::from(rect.width) * u64::from(rect.height), u64::from(rect.width.max(rect.height)), u64::MAX - *index as u64)
        }).map(|(index, _)| index).expect("non-empty partition");
        let rect = leaves.remove(index);
        let can_vertical = rect.width >= recipe.minimum_logical_width.saturating_mul(2);
        let can_horizontal = rect.height >= recipe.minimum_logical_height.saturating_mul(2);
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
        let minimum = if vertical { recipe.minimum_logical_width } else { recipe.minimum_logical_height };
        split = split.clamp(minimum, extent.saturating_sub(minimum));
        let (first, second) = if vertical {
            (GridRect { x: rect.x, y: rect.y, width: split, height: rect.height },
             GridRect { x: rect.x + split, y: rect.y, width: rect.width - split, height: rect.height })
        } else {
            (GridRect { x: rect.x, y: rect.y, width: rect.width, height: split },
             GridRect { x: rect.x, y: rect.y + split, width: rect.width, height: rect.height - split })
        };
        leaves.extend([first, second]);
    }
    leaves.sort_by_key(|rect| (rect.y, rect.x, rect.height, rect.width));
    Ok(leaves.into_iter().enumerate().map(|(index, grid_rect)| PartitionRegion {
        id: region_id(recipe, grid_rect, index as u32), grid_rect,
    }).collect())
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
    fn boundary_tables_are_shared_and_reconstruct_a_frame() {
        let xs = resolve_boundaries(1_000, 4_000, 64);
        assert_eq!(xs[0], 1_000); assert_eq!(*xs.last().unwrap(), 5_000);
        assert_eq!(xs[1], 1_063);
        assert!(xs.windows(2).all(|pair| pair[0] <= pair[1]));
    }
}
