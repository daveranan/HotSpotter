use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    Channel, ChannelDataKind, ErrorCode, LayoutId, NormalizedPoint, NormalizedScalar, PatchId,
    RegionId, SourceSetId, UserFacingError,
};

pub const MAX_LAYOUT_EDGE: u32 = 16_384;
pub const MAX_LAYOUT_PIXELS: u64 = 268_435_456;
pub const MAX_LAYOUT_REGIONS: usize = 4_096;
pub const MAX_REGION_KEY_BYTES: usize = 255;
pub const MAX_REGION_INSET: u32 = 4_096;
const ID_COLOR_COMPONENT_VALUES: u64 = 192;
const ID_COLOR_VALUES: u64 =
    ID_COLOR_COMPONENT_VALUES * ID_COLOR_COMPONENT_VALUES * ID_COLOR_COMPONENT_VALUES;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PixelSize {
    pub width: u32,
    pub height: u32,
}

impl PixelSize {
    #[must_use]
    pub const fn is_nonzero(self) -> bool {
        self.width > 0 && self.height > 0
    }
}

/// Authoritative integer content bounds. Padding and bleed are outside these bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PixelBounds {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelBounds {
    #[must_use]
    pub const fn size(self) -> PixelSize {
        PixelSize {
            width: self.width,
            height: self.height,
        }
    }

    #[must_use]
    pub fn normalized(self, output: PixelSize) -> Option<NormalizedBounds> {
        if !output.is_nonzero() {
            return None;
        }
        Some(NormalizedBounds {
            x: NormalizedScalar::new(f64::from(self.x) / f64::from(output.width)).ok()?,
            y: NormalizedScalar::new(f64::from(self.y) / f64::from(output.height)).ok()?,
            width: NormalizedScalar::new(f64::from(self.width) / f64::from(output.width)).ok()?,
            height: NormalizedScalar::new(f64::from(self.height) / f64::from(output.height))
                .ok()?,
        })
    }
}

/// Resolution-independent coordinates derived from authoritative integer bounds.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedBounds {
    pub x: NormalizedScalar,
    pub y: NormalizedScalar,
    pub width: NormalizedScalar,
    pub height: NormalizedScalar,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutPreset {
    Balanced,
    HorizontalTrims,
    VerticalTrims,
    ModularKit,
    Atlas,
}

/// Describes whether a layout is a free-form atlas or uses a pinned template contract.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutKind {
    Template,
    #[default]
    CustomAtlas,
}

/// Versioned template identity. Compatibility-key interpretation belongs to the template registry.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateIdentity {
    pub template_id: String,
    pub template_version: String,
    pub compatibility_key: String,
}

/// Complete pinned template source retained by a project independently of any registry.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateSnapshot {
    pub identity: TemplateIdentity,
    pub schema_version: u32,
    pub canonical_width: u32,
    pub canonical_height: u32,
    pub snapshot_json: String,
    pub snapshot_hash: String,
}

/// A template slot's durable assignment to an authored layout item and stable ID-map region.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotBinding {
    pub slot_key: String,
    pub item_key: String,
    pub region_id: RegionId,
    pub id_color: IdColor,
}

/// A persisted decoration assignment owned by a style recipe.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecorationBinding {
    pub decoration_key: String,
    pub value: String,
}

/// Renderer-independent style metadata and ordered decorations.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleRecipe {
    #[serde(default)]
    pub recipe_key: String,
    #[serde(default)]
    pub decorations: Vec<DecorationBinding>,
}

/// How a source image is framed within a template allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceFramingMode {
    Cover,
    Stretch,
    Repeat,
}

/// Persisted source framing choice shared by template workbenches and renderers.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceFraming {
    pub mode: SourceFramingMode,
    pub crop_focus: NormalizedPoint,
}

impl Eq for SourceFraming {}

impl Default for SourceFraming {
    fn default() -> Self {
        Self {
            mode: SourceFramingMode::Cover,
            crop_focus: NormalizedPoint::new(0.5, 0.5).expect("default crop focus is normalized"),
        }
    }
}

/// Durable template-specific metadata stored beside a solver layout.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateLayoutContract {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<TemplateSnapshot>,
    #[serde(default)]
    pub slot_bindings: Vec<SlotBinding>,
    #[serde(default)]
    pub style_recipe: StyleRecipe,
    #[serde(default)]
    pub source_framing: SourceFraming,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutOrder {
    Input,
    LargestFirst,
    HorizontalFirst,
    VerticalFirst,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackPriority {
    Balanced,
    HorizontalStrips,
    VerticalStrips,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoPackSettings {
    pub enabled: bool,
    pub priority: PackPriority,
    /// A persisted seed. It only affects deterministic ID derivation; ties never use ambient randomness.
    pub seed: u64,
}

impl Default for AutoPackSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            priority: PackPriority::Balanced,
            seed: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixedRegionSize {
    pub region_id: RegionId,
    pub size: PixelSize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutSettings {
    pub output: PixelSize,
    pub padding_px: u32,
    pub bleed_px: u32,
    pub order: LayoutOrder,
    pub auto_pack: AutoPackSettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_selected_size: Option<FixedRegionSize>,
}

impl Default for LayoutSettings {
    fn default() -> Self {
        Self {
            output: PixelSize {
                width: 2_048,
                height: 2_048,
            },
            padding_px: 4,
            bleed_px: 8,
            order: LayoutOrder::Input,
            auto_pack: AutoPackSettings::default(),
            fixed_selected_size: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FillBehavior {
    HorizontalLoop,
    VerticalLoop,
    Tile,
    Stretch,
    UniqueDetail,
    TrimCap,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrimAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrimCaps {
    pub axis: TrimAxis,
    pub leading_px: u32,
    pub trailing_px: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimpleDataInput {
    pub channel: Channel,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum RegionFill {
    WholeSourceSet {
        source_set_id: SourceSetId,
    },
    RectifiedPatch {
        source_set_id: SourceSetId,
        patch_id: PatchId,
    },
    SimpleColor {
        rgba: [u8; 4],
    },
    SimpleData {
        input: SimpleDataInput,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionConstraints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_width_px: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_height_px: Option<u32>,
    /// Deterministic normalized bounds used by Hotspot and Trim templates.
    /// Atlas entries leave this absent and use the packing engine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_bounds: Option<NormalizedBounds>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionLocks {
    pub position: bool,
    pub width: bool,
    pub height: bool,
}

/// One solver participant. Disabled or non-participating entries are retained in the request but omitted
/// from the result. Thus every enabled, participating patch maps one-to-one to a region.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutItem {
    pub key: String,
    pub fill: RegionFill,
    pub behavior: FillBehavior,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trim_caps: Option<TrimCaps>,
    pub natural_size: PixelSize,
    pub enabled: bool,
    pub participates: bool,
    #[serde(default)]
    pub constraints: RegionConstraints,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_px: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bleed_px: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region_id: Option<RegionId>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IdColor(pub [u8; 3]);

impl IdColor {
    /// Produces a vivid, non-black flat color from stable region-ID bytes.
    #[must_use]
    pub fn for_region(region_id: RegionId) -> Self {
        let bytes = region_id.to_bytes();
        Self([
            64 | (bytes[0] ^ bytes[7]) & 0xbf,
            64 | (bytes[3] ^ bytes[11]) & 0xbf,
            64 | (bytes[5] ^ bytes[15]) & 0xbf,
        ])
    }

    /// Deterministically walks the valid ID-color space without changing the region ID.
    /// Salt zero is the canonical color; later salts support stable collision resolution.
    #[must_use]
    pub fn for_region_with_salt(region_id: RegionId, salt: u32) -> Self {
        let canonical = Self::for_region(region_id);
        if salt == 0 {
            return canonical;
        }
        let base =
            u64::from(canonical.0[0] - 64) * ID_COLOR_COMPONENT_VALUES * ID_COLOR_COMPONENT_VALUES
                + u64::from(canonical.0[1] - 64) * ID_COLOR_COMPONENT_VALUES
                + u64::from(canonical.0[2] - 64);
        let resolved = (base + u64::from(salt)) % ID_COLOR_VALUES;
        Self([
            resolved_id_component(
                resolved / (ID_COLOR_COMPONENT_VALUES * ID_COLOR_COMPONENT_VALUES),
            ),
            resolved_id_component(
                (resolved / ID_COLOR_COMPONENT_VALUES) % ID_COLOR_COMPONENT_VALUES,
            ),
            resolved_id_component(resolved % ID_COLOR_COMPONENT_VALUES),
        ])
    }

    /// Resolved ID colors remain deliberately away from black for visible flat-ID output.
    #[must_use]
    pub fn is_valid(self) -> bool {
        self.0.into_iter().all(|component| component >= 64)
    }
}

fn resolved_id_component(value: u64) -> u8 {
    debug_assert!(value < ID_COLOR_COMPONENT_VALUES);
    #[allow(clippy::cast_possible_truncation)]
    let component = value as u8;
    64 + component
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutRegion {
    pub id: RegionId,
    pub item_key: String,
    pub fill: RegionFill,
    pub behavior: FillBehavior,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trim_caps: Option<TrimCaps>,
    pub bounds: PixelBounds,
    pub padding_px: u32,
    pub bleed_px: u32,
    pub order_index: u32,
    pub locks: RegionLocks,
    pub id_color: IdColor,
}

impl LayoutRegion {
    #[must_use]
    pub fn normalized_bounds(&self, output: PixelSize) -> Option<NormalizedBounds> {
        self.bounds.normalized(output)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub id: LayoutId,
    pub preset: LayoutPreset,
    pub settings: LayoutSettings,
    pub regions: Vec<LayoutRegion>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutRequest {
    pub layout_id: LayoutId,
    pub preset: LayoutPreset,
    pub settings: LayoutSettings,
    pub items: Vec<LayoutItem>,
    #[serde(default)]
    pub existing_regions: Vec<LayoutRegion>,
}

impl LayoutRequest {
    /// Checks bounded metadata and references available at this pure-domain boundary.
    ///
    /// # Errors
    ///
    /// Returns a typed error before geometry work for malformed dimensions, duplicate stable keys/IDs,
    /// invalid simple data, cap contracts, or a selected-region reference absent from existing state.
    pub fn validate(&self) -> Result<(), LayoutContractError> {
        validate_settings(&self.settings)?;
        if self.items.len() > MAX_LAYOUT_REGIONS || self.existing_regions.len() > MAX_LAYOUT_REGIONS
        {
            return Err(LayoutContractError::TooManyRegions {
                found: self.items.len().max(self.existing_regions.len()),
                maximum: MAX_LAYOUT_REGIONS,
            });
        }
        let mut keys = BTreeSet::new();
        let mut requested_ids = BTreeSet::new();
        for item in &self.items {
            if item.key.trim().is_empty() || item.key.len() > MAX_REGION_KEY_BYTES {
                return Err(LayoutContractError::InvalidItemKey(item.key.clone()));
            }
            if !keys.insert(item.key.as_str()) {
                return Err(LayoutContractError::DuplicateItemKey(item.key.clone()));
            }
            if !item.natural_size.is_nonzero() {
                return Err(LayoutContractError::InvalidItemSize(item.key.clone()));
            }
            if item.constraints.fixed_width_px == Some(0)
                || item.constraints.fixed_height_px == Some(0)
            {
                return Err(LayoutContractError::InvalidItemSize(item.key.clone()));
            }
            if item.constraints.template_bounds.is_some_and(|bounds| {
                bounds.width.get() <= 0.0
                    || bounds.height.get() <= 0.0
                    || bounds.x.get() + bounds.width.get() > 1.0 + f64::EPSILON
                    || bounds.y.get() + bounds.height.get() > 1.0 + f64::EPSILON
            }) {
                return Err(LayoutContractError::InvalidItemSize(item.key.clone()));
            }
            if item
                .padding_px
                .is_some_and(|value| value > MAX_REGION_INSET)
                || item.bleed_px.is_some_and(|value| value > MAX_REGION_INSET)
            {
                return Err(LayoutContractError::InsetTooLarge);
            }
            validate_fill(&item.fill)?;
            validate_caps(
                item.behavior,
                item.trim_caps,
                PixelSize {
                    width: item
                        .constraints
                        .fixed_width_px
                        .unwrap_or(item.natural_size.width),
                    height: item
                        .constraints
                        .fixed_height_px
                        .unwrap_or(item.natural_size.height),
                },
                &item.key,
            )?;
            if let Some(id) = item.region_id
                && !requested_ids.insert(id)
            {
                return Err(LayoutContractError::DuplicateRegionId(id));
            }
        }
        let mut existing_ids = BTreeSet::new();
        let mut existing_keys = BTreeSet::new();
        let mut existing_colors = BTreeSet::new();
        for region in &self.existing_regions {
            if region.item_key.trim().is_empty() || region.item_key.len() > MAX_REGION_KEY_BYTES {
                return Err(LayoutContractError::InvalidItemKey(region.item_key.clone()));
            }
            if !existing_ids.insert(region.id) {
                return Err(LayoutContractError::DuplicateRegionId(region.id));
            }
            if !existing_keys.insert(region.item_key.as_str()) {
                return Err(LayoutContractError::DuplicateItemKey(
                    region.item_key.clone(),
                ));
            }
            validate_fill(&region.fill)?;
            if !region.id_color.is_valid() {
                return Err(LayoutContractError::InvalidIdColor(region.id));
            }
            if !existing_colors.insert(region.id_color) {
                return Err(LayoutContractError::DuplicateIdColor(region.id_color));
            }
            validate_caps(
                region.behavior,
                region.trim_caps,
                region.bounds.size(),
                &region.item_key,
            )?;
        }
        if let Some(fixed) = self.settings.fixed_selected_size {
            if !fixed.size.is_nonzero() {
                return Err(LayoutContractError::InvalidSelectedSize);
            }
            let known = self
                .existing_regions
                .iter()
                .any(|region| region.id == fixed.region_id)
                || self
                    .items
                    .iter()
                    .any(|item| item.region_id == Some(fixed.region_id));
            if !known {
                return Err(LayoutContractError::UnknownSelectedRegion(fixed.region_id));
            }
        }
        Ok(())
    }
}

fn validate_settings(settings: &LayoutSettings) -> Result<(), LayoutContractError> {
    if !settings.output.is_nonzero()
        || settings.output.width > MAX_LAYOUT_EDGE
        || settings.output.height > MAX_LAYOUT_EDGE
        || u64::from(settings.output.width) * u64::from(settings.output.height) > MAX_LAYOUT_PIXELS
    {
        return Err(LayoutContractError::InvalidOutputResolution);
    }
    if settings.padding_px > MAX_REGION_INSET || settings.bleed_px > MAX_REGION_INSET {
        return Err(LayoutContractError::InsetTooLarge);
    }
    Ok(())
}

fn validate_fill(fill: &RegionFill) -> Result<(), LayoutContractError> {
    if let RegionFill::SimpleData { input } = fill {
        if !input.value.is_finite()
            || !(0.0..=1.0).contains(&input.value)
            || matches!(
                input.channel.data_kind(),
                ChannelDataKind::Color | ChannelDataKind::Vector
            )
        {
            return Err(LayoutContractError::InvalidSimpleData);
        }
    }
    Ok(())
}

fn validate_caps(
    behavior: FillBehavior,
    caps: Option<TrimCaps>,
    size: PixelSize,
    key: &str,
) -> Result<(), LayoutContractError> {
    match (behavior, caps) {
        (FillBehavior::TrimCap, Some(caps)) => {
            let span = match caps.axis {
                TrimAxis::Horizontal => size.width,
                TrimAxis::Vertical => size.height,
            };
            if caps.leading_px.saturating_add(caps.trailing_px) >= span {
                return Err(LayoutContractError::InvalidTrimCaps(key.into()));
            }
        }
        (FillBehavior::TrimCap, None) | (_, Some(_)) => {
            return Err(LayoutContractError::InvalidTrimCaps(key.into()));
        }
        (_, None) => {}
    }
    Ok(())
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum LayoutContractError {
    #[error("layout output resolution is zero or exceeds 16384 pixels per edge / 268435456 pixels")]
    InvalidOutputResolution,
    #[error("layout padding or bleed exceeds 4096 pixels")]
    InsetTooLarge,
    #[error("layout has {found} regions; the maximum is {maximum}")]
    TooManyRegions { found: usize, maximum: usize },
    #[error("layout item key is empty or too long: {0}")]
    InvalidItemKey(String),
    #[error("layout item key is repeated: {0}")]
    DuplicateItemKey(String),
    #[error("layout item dimensions are invalid: {0}")]
    InvalidItemSize(String),
    #[error("region ID is repeated: {0}")]
    DuplicateRegionId(RegionId),
    #[error("region {0} has an invalid ID color")]
    InvalidIdColor(RegionId),
    #[error("ID color {0:?} is assigned to more than one region")]
    DuplicateIdColor(IdColor),
    #[error("simple data values must be finite and between zero and one")]
    InvalidSimpleData,
    #[error("trim-cap spans are absent, attached to a non-cap behavior, or consume region {0}")]
    InvalidTrimCaps(String),
    #[error("fixed selected-region size must be nonzero")]
    InvalidSelectedSize,
    #[error("fixed selected-region size references unknown region {0}")]
    UnknownSelectedRegion(RegionId),
}

impl From<LayoutContractError> for UserFacingError {
    fn from(error: LayoutContractError) -> Self {
        let (message, recovery) = match &error {
            LayoutContractError::InvalidOutputResolution => (
                "The trim-sheet resolution is unsupported.",
                "Choose nonzero dimensions no larger than 16384 pixels per edge.",
            ),
            LayoutContractError::InsetTooLarge => (
                "Padding or bleed is too large.",
                "Reduce padding and bleed to 4096 pixels or less.",
            ),
            LayoutContractError::TooManyRegions { .. } => (
                "This layout contains too many regions.",
                "Disable unused patches or split the material into another project.",
            ),
            LayoutContractError::InvalidItemKey(_)
            | LayoutContractError::DuplicateItemKey(_)
            | LayoutContractError::DuplicateRegionId(_)
            | LayoutContractError::InvalidIdColor(_)
            | LayoutContractError::DuplicateIdColor(_) => (
                "The trim sheet contains conflicting stable region references.",
                "Regenerate the affected layout from its source sets.",
            ),
            LayoutContractError::InvalidItemSize(_) | LayoutContractError::InvalidSelectedSize => (
                "A trim region has invalid dimensions.",
                "Enter a positive width and height that fit the output resolution.",
            ),
            LayoutContractError::InvalidSimpleData => (
                "A simple data fill is outside its valid range.",
                "Enter a finite value between zero and one.",
            ),
            LayoutContractError::InvalidTrimCaps(_) => (
                "Trim caps consume the entire region or do not match its behavior.",
                "Reduce the cap widths or change the region behavior.",
            ),
            LayoutContractError::UnknownSelectedRegion(_) => (
                "The fixed-size target no longer exists.",
                "Select a current region and set its exact size again.",
            ),
        };
        Self {
            code: ErrorCode::LayoutInvalid,
            message: message.into(),
            recovery: recovery.into(),
            detail: Some(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> LayoutRequest {
        LayoutRequest {
            layout_id: LayoutId::new(),
            preset: LayoutPreset::Balanced,
            settings: LayoutSettings::default(),
            items: vec![LayoutItem {
                key: "source:brick".into(),
                fill: RegionFill::WholeSourceSet {
                    source_set_id: SourceSetId::new(),
                },
                behavior: FillBehavior::Stretch,
                trim_caps: None,
                natural_size: PixelSize {
                    width: 2_048,
                    height: 1_024,
                },
                enabled: true,
                participates: true,
                constraints: RegionConstraints::default(),
                padding_px: None,
                bleed_px: None,
                region_id: None,
            }],
            existing_regions: Vec::new(),
        }
    }

    #[test]
    fn production_wire_shape_round_trips() {
        let request = request();
        let json = serde_json::to_value(&request).expect("serialize request");
        assert_eq!(json["preset"], "balanced");
        assert_eq!(json["items"][0]["fill"]["type"], "whole_source_set");
        assert_eq!(json["items"][0]["naturalSize"]["width"], 2_048);
        assert_eq!(
            serde_json::from_value::<LayoutRequest>(json).expect("deserialize request"),
            request
        );
    }

    #[test]
    fn template_source_framing_defaults_for_legacy_contracts() {
        let contract: TemplateLayoutContract = serde_json::from_str(
            r#"{"slotBindings":[],"styleRecipe":{"recipeKey":"","decorations":[]}}"#,
        )
        .expect("legacy template contract");
        assert_eq!(contract.source_framing, SourceFraming::default());

        let json = serde_json::to_value(contract).expect("serialize framing");
        assert_eq!(json["sourceFraming"]["mode"], "cover");
        assert_eq!(json["sourceFraming"]["cropFocus"]["x"], 0.5);
        assert_eq!(json["sourceFraming"]["cropFocus"]["y"], 0.5);
    }

    #[test]
    fn rejects_non_finite_data_and_dangling_selected_region() {
        let mut request = request();
        request.items[0].fill = RegionFill::SimpleData {
            input: SimpleDataInput {
                channel: Channel::Height,
                value: f64::NAN,
            },
        };
        assert_eq!(
            request.validate(),
            Err(LayoutContractError::InvalidSimpleData)
        );

        request.items[0].fill = RegionFill::SimpleColor { rgba: [1, 2, 3, 4] };
        request.settings.fixed_selected_size = Some(FixedRegionSize {
            region_id: RegionId::new(),
            size: PixelSize {
                width: 1,
                height: 1,
            },
        });
        assert!(matches!(
            request.validate(),
            Err(LayoutContractError::UnknownSelectedRegion(_))
        ));
    }

    #[test]
    fn integer_bounds_derive_exact_normalized_coordinates() {
        let bounds = PixelBounds {
            x: 256,
            y: 128,
            width: 512,
            height: 256,
        };
        assert_eq!(
            bounds.normalized(PixelSize {
                width: 1_024,
                height: 512
            }),
            Some(NormalizedBounds {
                x: NormalizedScalar::new(0.25).expect("x"),
                y: NormalizedScalar::new(0.25).expect("y"),
                width: NormalizedScalar::new(0.5).expect("width"),
                height: NormalizedScalar::new(0.5).expect("height")
            })
        );
    }

    #[test]
    fn deterministic_id_color_is_vivid_and_stable() {
        let id = RegionId::from_bytes([7; 16]);
        let color = IdColor::for_region(id);
        assert_eq!(color, IdColor::for_region(id));
        assert!(color.0.into_iter().all(|component| component >= 64));
        assert!(IdColor::for_region_with_salt(id, 1).is_valid());
        assert_ne!(color, IdColor::for_region_with_salt(id, 1));
    }

    #[test]
    fn persisted_resolved_colors_are_valid_but_duplicates_are_rejected() {
        let mut request = request();
        let first_id = RegionId::from_bytes([1; 16]);
        let second_id = RegionId::from_bytes([2; 16]);
        let resolved = IdColor([65, 66, 67]);
        let region = |id, key: &str, color| LayoutRegion {
            id,
            item_key: key.into(),
            fill: RegionFill::SimpleColor {
                rgba: [1, 2, 3, 255],
            },
            behavior: FillBehavior::Stretch,
            trim_caps: None,
            bounds: PixelBounds {
                x: 16,
                y: 16,
                width: 128,
                height: 128,
            },
            padding_px: 1,
            bleed_px: 1,
            order_index: 0,
            locks: RegionLocks::default(),
            id_color: color,
        };
        request.existing_regions = vec![region(first_id, "resolved", resolved)];
        assert!(request.validate().is_ok());

        request
            .existing_regions
            .push(region(second_id, "duplicate", resolved));
        assert_eq!(
            request.validate(),
            Err(LayoutContractError::DuplicateIdColor(resolved))
        );

        request.existing_regions.truncate(1);
        request.existing_regions[0].id_color = IdColor([0, 64, 64]);
        assert_eq!(
            request.validate(),
            Err(LayoutContractError::InvalidIdColor(first_id))
        );
    }
}
