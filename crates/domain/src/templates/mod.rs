use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{IdColor, TemplateIdentity, TemplateSnapshot};

pub const CANONICAL_TEMPLATE_EDGE: u32 = 4_096;

/// Integer bounds in the fixed 4096 by 4096 template coordinate space.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// A canonical anchor point used by optional template hotspots.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hotspot {
    pub x: u32,
    pub y: u32,
}

/// Renderer-neutral physical placement metadata for a template slot.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldPlacement {
    pub width: f64,
    pub height: f64,
    pub rotation_degrees: f64,
}

/// Optional radial styling metadata for slots such as circular fixtures.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadialParameters {
    pub center_x: f64,
    pub center_y: f64,
    pub inner_radius: f64,
    pub outer_radius: f64,
}

/// Template-authored source-space crop used to instantiate a region's first material mapping.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateSourceRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateSourceAddressMode {
    Clamp,
    Repeat,
    MirroredRepeat,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateSourceMapping {
    pub crop: TemplateSourceRect,
    pub address_mode: TemplateSourceAddressMode,
}

/// Manifest-owned intended usage of a fixed template allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TemplateSlotRole {
    Planar,
    RepeatingStrip,
    UniqueDetail,
    TrimCap,
    Radial,
}

/// Manifest-owned structural shading profile for a fixed template allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StructuralProfile {
    Flat,
    Bevel,
    Groove,
    RoundedBevel,
    PanelFrame,
    RadialDisc,
    Annulus,
}

/// Optional destination-packing guidance. Source mapping remains a separate, immutable concern.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplatePackingIntent {
    pub preferred_aspect: f64,
    pub area_weight: f64,
    pub minimum_size: [u32; 2],
    pub allow_rotation: bool,
    pub padding: u32,
}

/// One fixed slot in a versioned layout template.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateSlot {
    pub slot_key: String,
    pub compatibility_key: String,
    pub material_group: String,
    pub variation_group: String,
    pub role: TemplateSlotRole,
    pub structural_profile: StructuralProfile,
    pub allocation: CanonicalRect,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotspot: Option<Hotspot>,
    pub id_color: IdColor,
    pub world_placement: WorldPlacement,
    pub source_mapping: TemplateSourceMapping,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packing_intent: Option<TemplatePackingIntent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radial_parameters: Option<RadialParameters>,
}

/// A self-contained template definition loaded from the registry catalog.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateDefinition {
    pub identity: TemplateIdentity,
    pub schema_version: u32,
    pub canonical_width: u32,
    pub canonical_height: u32,
    pub stable_order: Vec<String>,
    pub slots: Vec<TemplateSlot>,
}

impl TemplateDefinition {
    /// Produces the canonical project-pinned representation and SHA-256 digest of this definition.
    ///
    /// # Errors
    ///
    /// Returns a typed error when serialization cannot produce the canonical JSON payload.
    pub fn snapshot(&self) -> Result<TemplateSnapshot, TemplateRegistryError> {
        self.validate()?;
        let snapshot_json = serde_json::to_string(self)?;
        let snapshot_hash = format!("{:x}", Sha256::digest(snapshot_json.as_bytes()));
        Ok(TemplateSnapshot {
            identity: self.identity.clone(),
            schema_version: self.schema_version,
            canonical_width: self.canonical_width,
            canonical_height: self.canonical_height,
            snapshot_json,
            snapshot_hash,
        })
    }

    fn validate(&self) -> Result<(), TemplateRegistryError> {
        if self.canonical_width != CANONICAL_TEMPLATE_EDGE
            || self.canonical_height != CANONICAL_TEMPLATE_EDGE
        {
            return Err(TemplateRegistryError::InvalidCanonicalSize {
                width: self.canonical_width,
                height: self.canonical_height,
            });
        }
        if self.identity.template_id.trim().is_empty()
            || self.identity.template_version.trim().is_empty()
            || self.identity.compatibility_key.trim().is_empty()
        {
            return Err(TemplateRegistryError::InvalidIdentity);
        }
        let mut order = BTreeSet::new();
        for slot_key in &self.stable_order {
            if slot_key.trim().is_empty() || !order.insert(slot_key.as_str()) {
                return Err(TemplateRegistryError::InvalidStableOrder);
            }
        }
        let mut slot_keys = BTreeSet::new();
        let mut compatibility_keys = BTreeSet::new();
        let mut colors = BTreeSet::new();
        for slot in &self.slots {
            if slot.slot_key.trim().is_empty() || !slot_keys.insert(slot.slot_key.as_str()) {
                return Err(TemplateRegistryError::DuplicateSlotKey(
                    slot.slot_key.clone(),
                ));
            }
            if slot.compatibility_key.trim().is_empty()
                || !compatibility_keys.insert(slot.compatibility_key.as_str())
            {
                return Err(TemplateRegistryError::DuplicateSlotCompatibilityKey(
                    slot.compatibility_key.clone(),
                ));
            }
            if slot.material_group.trim().is_empty() {
                return Err(TemplateRegistryError::InvalidMaterialGroup(
                    slot.slot_key.clone(),
                ));
            }
            if slot.variation_group.trim().is_empty() {
                return Err(TemplateRegistryError::InvalidVariationGroup(
                    slot.slot_key.clone(),
                ));
            }
            let profile_matches_role = match slot.role {
                TemplateSlotRole::Planar => {
                    matches!(slot.structural_profile, StructuralProfile::Flat)
                }
                TemplateSlotRole::RepeatingStrip => matches!(
                    slot.structural_profile,
                    StructuralProfile::Flat
                        | StructuralProfile::Bevel
                        | StructuralProfile::Groove
                        | StructuralProfile::RoundedBevel
                ),
                TemplateSlotRole::UniqueDetail => matches!(
                    slot.structural_profile,
                    StructuralProfile::Flat
                        | StructuralProfile::Bevel
                        | StructuralProfile::Groove
                        | StructuralProfile::RoundedBevel
                        | StructuralProfile::PanelFrame
                ),
                TemplateSlotRole::TrimCap => matches!(
                    slot.structural_profile,
                    StructuralProfile::Bevel
                        | StructuralProfile::RoundedBevel
                        | StructuralProfile::PanelFrame
                ),
                TemplateSlotRole::Radial => matches!(
                    slot.structural_profile,
                    StructuralProfile::RadialDisc | StructuralProfile::Annulus
                ),
            };
            if !profile_matches_role {
                return Err(TemplateRegistryError::InvalidRoleProfile(
                    slot.slot_key.clone(),
                ));
            }
            if !slot.id_color.is_valid() || !colors.insert(slot.id_color) {
                return Err(TemplateRegistryError::DuplicateIdColor(slot.id_color));
            }
            let right = slot.allocation.x.saturating_add(slot.allocation.width);
            let bottom = slot.allocation.y.saturating_add(slot.allocation.height);
            if slot.allocation.width == 0
                || slot.allocation.height == 0
                || right > self.canonical_width
                || bottom > self.canonical_height
            {
                return Err(TemplateRegistryError::InvalidAllocation(
                    slot.slot_key.clone(),
                ));
            }
            if let Some(hotspot) = slot.hotspot
                && (hotspot.x < slot.allocation.x
                    || hotspot.x >= right
                    || hotspot.y < slot.allocation.y
                    || hotspot.y >= bottom)
            {
                return Err(TemplateRegistryError::InvalidHotspot(slot.slot_key.clone()));
            }
            if !slot.world_placement.width.is_finite()
                || !slot.world_placement.height.is_finite()
                || !slot.world_placement.rotation_degrees.is_finite()
                || slot.world_placement.width <= 0.0
                || slot.world_placement.height <= 0.0
            {
                return Err(TemplateRegistryError::InvalidWorldPlacement(
                    slot.slot_key.clone(),
                ));
            }
            if !valid_source_mapping(slot.source_mapping) {
                return Err(TemplateRegistryError::InvalidSourceMapping(
                    slot.slot_key.clone(),
                ));
            }
            if let Some(intent) = slot.packing_intent
                && (!intent.preferred_aspect.is_finite()
                    || intent.preferred_aspect <= 0.0
                    || !intent.area_weight.is_finite()
                    || intent.area_weight <= 0.0
                    || intent.minimum_size.contains(&0))
            {
                return Err(TemplateRegistryError::InvalidPackingIntent(slot.slot_key.clone()));
            }
            match (slot.role, slot.radial_parameters) {
                (TemplateSlotRole::Radial, Some(radial)) => {
                    let valid_parameters = radial.center_x.is_finite()
                        && radial.center_y.is_finite()
                        && radial.inner_radius.is_finite()
                        && radial.outer_radius.is_finite()
                        && (0.0..=1.0).contains(&radial.center_x)
                        && (0.0..=1.0).contains(&radial.center_y)
                        && radial.inner_radius >= 0.0
                        && radial.outer_radius > radial.inner_radius
                        && slot.allocation.width == slot.allocation.height;
                    let profile_matches_radii = match slot.structural_profile {
                        StructuralProfile::RadialDisc => radial.inner_radius == 0.0,
                        StructuralProfile::Annulus => radial.inner_radius > 0.0,
                        _ => false,
                    };
                    if !valid_parameters || !profile_matches_radii {
                        return Err(TemplateRegistryError::InvalidRadialParameters(
                            slot.slot_key.clone(),
                        ));
                    }
                }
                (TemplateSlotRole::Radial, None) | (_, Some(_)) => {
                    return Err(TemplateRegistryError::InvalidRadialParameters(
                        slot.slot_key.clone(),
                    ));
                }
                (_, None) => {}
            }
        }
        if order != slot_keys {
            return Err(TemplateRegistryError::InvalidStableOrder);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct TemplateRegistry {
    definitions: BTreeMap<(String, String), TemplateDefinition>,
}

impl TemplateRegistry {
    /// Loads the version-pinned template families shipped with Hot Trimmer.
    ///
    /// The individual manifest files remain independently consumable by export
    /// tooling; this registry is the desktop-facing catalog for choosing a
    /// family by stable template ID.
    pub fn built_in() -> Result<Self, TemplateRegistryError> {
        const BUILTIN_MANIFESTS: [&str; 5] = [
            include_str!("../../../../assets/templates/generic_architecture/1.0.0/template.json"),
            include_str!("../../../../assets/templates/horizontal_moulding/1.0.0/template.json"),
            include_str!("../../../../assets/templates/vertical_trim/1.0.0/template.json"),
            include_str!("../../../../assets/templates/wood_board_moulding/1.0.0/template.json"),
            include_str!(
                "../../../../assets/templates/detail_ribbon_microtrim/1.0.0/template.json"
            ),
        ];

        let mut definitions = BTreeMap::new();
        for json in BUILTIN_MANIFESTS {
            let catalog: TemplateCatalog = serde_json::from_str(json)?;
            for definition in catalog.templates {
                definition.validate()?;
                let key = (
                    definition.identity.template_id.clone(),
                    definition.identity.template_version.clone(),
                );
                if definitions.insert(key.clone(), definition).is_some() {
                    return Err(TemplateRegistryError::DuplicateTemplate {
                        template_id: key.0,
                        template_version: key.1,
                    });
                }
            }
        }
        Ok(Self { definitions })
    }

    /// Loads and validates a JSON `{ "templates": [...] }` catalog.
    ///
    /// # Errors
    ///
    /// Returns a typed diagnostic for malformed JSON, invalid template contracts, or duplicate versions.
    pub fn from_json(json: &str) -> Result<Self, TemplateRegistryError> {
        let catalog: TemplateCatalog = serde_json::from_str(json)?;
        let mut definitions = BTreeMap::new();
        for definition in catalog.templates {
            definition.validate()?;
            let key = (
                definition.identity.template_id.clone(),
                definition.identity.template_version.clone(),
            );
            if definitions.insert(key.clone(), definition).is_some() {
                return Err(TemplateRegistryError::DuplicateTemplate {
                    template_id: key.0,
                    template_version: key.1,
                });
            }
        }
        Ok(Self { definitions })
    }

    #[must_use]
    pub fn get(&self, template_id: &str, template_version: &str) -> Option<&TemplateDefinition> {
        self.definitions
            .get(&(template_id.to_owned(), template_version.to_owned()))
    }
}

#[derive(Deserialize)]
struct TemplateCatalog {
    templates: Vec<TemplateDefinition>,
}

#[derive(Debug, Error)]
pub enum TemplateRegistryError {
    #[error("template registry JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("template canonical size must be 4096 by 4096; found {width} by {height}")]
    InvalidCanonicalSize { width: u32, height: u32 },
    #[error("template identity fields must be nonempty")]
    InvalidIdentity,
    #[error("template stable order must contain each slot key exactly once")]
    InvalidStableOrder,
    #[error("template slot key is duplicated or empty: {0}")]
    DuplicateSlotKey(String),
    #[error("template slot compatibility key is duplicated or empty: {0}")]
    DuplicateSlotCompatibilityKey(String),
    #[error("template slot material group is empty: {0}")]
    InvalidMaterialGroup(String),
    #[error("template slot variation group is empty: {0}")]
    InvalidVariationGroup(String),
    #[error("template slot role and structural profile are incompatible: {0}")]
    InvalidRoleProfile(String),
    #[error("template slot ID color is duplicated or invalid: {0:?}")]
    DuplicateIdColor(IdColor),
    #[error("template slot allocation is invalid: {0}")]
    InvalidAllocation(String),
    #[error("template hotspot lies outside slot allocation: {0}")]
    InvalidHotspot(String),
    #[error("template world placement is invalid: {0}")]
    InvalidWorldPlacement(String),
    #[error("template source mapping is invalid: {0}")]
    InvalidSourceMapping(String),
    #[error("template slot packing intent is invalid: {0}")]
    InvalidPackingIntent(String),
    #[error("template radial parameters are invalid: {0}")]
    InvalidRadialParameters(String),
    #[error("template is duplicated: {template_id}@{template_version}")]
    DuplicateTemplate {
        template_id: String,
        template_version: String,
    },
}

fn valid_source_mapping(mapping: TemplateSourceMapping) -> bool {
    let crop = mapping.crop;
    crop.x.is_finite()
        && crop.y.is_finite()
        && crop.width.is_finite()
        && crop.height.is_finite()
        && crop.width > 0.0
        && crop.height > 0.0
        && crop.x >= 0.0
        && crop.y >= 0.0
        && crop.x + crop.width <= 1.0 + f64::EPSILON
        && crop.y + crop.height <= 1.0 + f64::EPSILON
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOG: &str = r#"{
      "templates": [{
        "identity": {"templateId": "hotspot", "templateVersion": "1.0.0", "compatibilityKey": "hotspot-v1"},
        "schemaVersion": 1,
        "canonicalWidth": 4096,
        "canonicalHeight": 4096,
        "stableOrder": ["wall", "floor"],
        "slots": [
          {
            "slotKey": "wall",
            "compatibilityKey": "wall-v1",             "materialGroup": "architecture",             "variationGroup": "neutral",             "role": "planar",             "structuralProfile": "flat",
            "allocation": {"x": 0, "y": 0, "width": 2048, "height": 4096},
            "hotspot": {"x": 1024, "y": 2048},
            "idColor": [64, 65, 66],
            "worldPlacement": {"width": 4.0, "height": 3.0, "rotationDegrees": 0.0},
            "sourceMapping": {"crop": {"x": 0.0, "y": 0.0, "width": 0.5, "height": 1.0}, "addressMode": "clamp"}
          },
          {
            "slotKey": "floor",
            "compatibilityKey": "floor-v1",             "materialGroup": "architecture",             "variationGroup": "radial_fixture",             "role": "radial",             "structuralProfile": "radialDisc",
            "allocation": {"x": 2048, "y": 0, "width": 2048, "height": 2048},
            "idColor": [67, 68, 69],
            "worldPlacement": {"width": 4.0, "height": 4.0, "rotationDegrees": 90.0},
            "sourceMapping": {"crop": {"x": 0.5, "y": 0.0, "width": 0.5, "height": 0.5}, "addressMode": "clamp"},
            "radialParameters": {"centerX": 0.5, "centerY": 0.5, "innerRadius": 0.0, "outerRadius": 1.0}
          }
        ]
      }]
    }"#;

    #[test]
    fn registry_loads_lookup_and_snapshot_is_canonical() {
        let registry = TemplateRegistry::from_json(CATALOG).expect("registry");
        let definition = registry.get("hotspot", "1.0.0").expect("definition");
        let snapshot = definition.snapshot().expect("snapshot");
        assert_eq!(snapshot.identity.template_id, "hotspot");
        assert_eq!(snapshot.canonical_width, CANONICAL_TEMPLATE_EDGE);
        assert_eq!(snapshot.snapshot_hash.len(), 64);
        assert_eq!(
            snapshot.snapshot_json,
            serde_json::to_string(definition).expect("canonical JSON")
        );
    }

    #[test]
    fn registry_rejects_noncanonical_size_and_duplicate_slot_color() {
        let invalid_size = CATALOG.replace("\"canonicalWidth\": 4096", "\"canonicalWidth\": 2048");
        assert!(matches!(
            TemplateRegistry::from_json(&invalid_size),
            Err(TemplateRegistryError::InvalidCanonicalSize { .. })
        ));
        let duplicate_color = CATALOG.replace("[67, 68, 69]", "[64, 65, 66]");
        assert!(matches!(
            TemplateRegistry::from_json(&duplicate_color),
            Err(TemplateRegistryError::DuplicateIdColor(_))
        ));
        let missing_group = CATALOG.replace(
            "\"materialGroup\": \"architecture\"",
            "\"materialGroup\": \"\"",
        );
        assert!(matches!(
            TemplateRegistry::from_json(&missing_group),
            Err(TemplateRegistryError::InvalidMaterialGroup(_))
        ));
    }

    #[test]
    fn built_in_catalog_exposes_each_trim_family() {
        let registry = TemplateRegistry::built_in().expect("built-in catalog");
        for template_id in [
            "ht.generic_architecture",
            "ht.horizontal_moulding",
            "ht.vertical_trim",
            "ht.wood_board_moulding",
            "ht.detail_ribbon_microtrim",
        ] {
            assert!(
                registry.get(template_id, "1.0.0").is_some(),
                "{template_id}"
            );
        }
    }
}
