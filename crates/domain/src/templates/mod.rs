use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{IdColor, PixelSize, TemplateIdentity, TemplateSnapshot};

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

/// Authored geometry-fit vocabulary consumed by Blender and the later placement stages.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TemplateFitSemantics {
    Planar,
    HorizontalStrip,
    VerticalStrip,
    UniqueContain,
    TrimCap,
    Radial,
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
    pub fit: TemplateFitSemantics,
    pub structural_profile: StructuralProfile,
    pub allocation: CanonicalRect,
    pub hotspot: CanonicalRect,
    pub id_color: IdColor,
    pub world_placement: WorldPlacement,
    pub source_mapping: TemplateSourceMapping,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radial_parameters: Option<RadialParameters>,
}

/// A template rectangle scaled to one requested output resolution.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledTemplateSlot {
    pub slot_key: String,
    pub allocation: CanonicalRect,
    pub hotspot: CanonicalRect,
}

/// Resolution-specific topology whose shared boundaries were compiled exactly once.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledTemplateTopology {
    pub identity: TemplateIdentity,
    pub output_size: PixelSize,
    pub slots: Vec<CompiledTemplateSlot>,
}

/// Recursive authored split grammar. Released templates persist the resulting integer rectangles.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WeightedTemplateGrammar {
    Slot {
        slot_key: String,
    },
    Horizontal {
        weights: Vec<u32>,
        children: Vec<Self>,
    },
    Vertical {
        weights: Vec<u32>,
        children: Vec<Self>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum TemplateCompatibilityDiagnostic {
    Compatible,
    ExplicitTopologyChange {
        from: TemplateIdentity,
        to: TemplateIdentity,
        reasons: Vec<String>,
    },
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

    /// Scales every distinct authored boundary once, then reuses it for allocation and hotspot edges.
    pub fn compile_for_output(
        &self,
        output_size: PixelSize,
    ) -> Result<CompiledTemplateTopology, TemplateRegistryError> {
        self.validate()?;
        if !output_size.is_nonzero() {
            return Err(TemplateRegistryError::InvalidOutputSize);
        }
        let mut x_boundaries = BTreeSet::new();
        let mut y_boundaries = BTreeSet::new();
        for slot in &self.slots {
            insert_rect_boundaries(slot.allocation, &mut x_boundaries, &mut y_boundaries);
            insert_rect_boundaries(slot.hotspot, &mut x_boundaries, &mut y_boundaries);
        }
        let scaled_x: BTreeMap<_, _> = x_boundaries
            .into_iter()
            .map(|boundary| (boundary, scale_boundary(boundary, output_size.width)))
            .collect();
        let scaled_y: BTreeMap<_, _> = y_boundaries
            .into_iter()
            .map(|boundary| (boundary, scale_boundary(boundary, output_size.height)))
            .collect();
        let slots = self
            .stable_order
            .iter()
            .map(|slot_key| {
                let slot = self
                    .slots
                    .iter()
                    .find(|slot| &slot.slot_key == slot_key)
                    .expect("validated stable order references every slot");
                CompiledTemplateSlot {
                    slot_key: slot.slot_key.clone(),
                    allocation: scale_rect(slot.allocation, &scaled_x, &scaled_y),
                    hotspot: scale_rect(slot.hotspot, &scaled_x, &scaled_y),
                }
            })
            .collect();
        Ok(CompiledTemplateTopology {
            identity: self.identity.clone(),
            output_size,
            slots,
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
            if !rect_contains(slot.allocation, slot.hotspot) {
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
            let fit_matches_role = matches!(
                (slot.role, slot.fit),
                (TemplateSlotRole::Planar, TemplateFitSemantics::Planar)
                    | (
                        TemplateSlotRole::RepeatingStrip,
                        TemplateFitSemantics::HorizontalStrip
                    )
                    | (
                        TemplateSlotRole::RepeatingStrip,
                        TemplateFitSemantics::VerticalStrip
                    )
                    | (
                        TemplateSlotRole::UniqueDetail,
                        TemplateFitSemantics::UniqueContain
                    )
                    | (TemplateSlotRole::TrimCap, TemplateFitSemantics::TrimCap)
                    | (TemplateSlotRole::Radial, TemplateFitSemantics::Radial)
            );
            if !fit_matches_role {
                return Err(TemplateRegistryError::InvalidFitSemantics(
                    slot.slot_key.clone(),
                ));
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
        for (index, slot) in self.slots.iter().enumerate() {
            if self.slots[index + 1..]
                .iter()
                .any(|other| rects_overlap(slot.allocation, other.allocation))
            {
                return Err(TemplateRegistryError::OverlappingAllocation(
                    slot.slot_key.clone(),
                ));
            }
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
        const BUILTIN_MANIFESTS: [&str; 8] = [
            include_str!("../../../../assets/templates/generic_architecture/1.0.0/template.json"),
            include_str!("../../../../assets/templates/horizontal_moulding/1.0.0/template.json"),
            include_str!("../../../../assets/templates/vertical_trim/1.0.0/template.json"),
            include_str!("../../../../assets/templates/wood_board_moulding/1.0.0/template.json"),
            include_str!(
                "../../../../assets/templates/detail_ribbon_microtrim/1.0.0/template.json"
            ),
            include_str!("../../../../assets/templates/hard_surface_panel/1.0.0/template.json"),
            include_str!("../../../../assets/templates/detail_heavy_props/1.0.0/template.json"),
            include_str!("../../../../assets/templates/radial_accents/1.0.0/template.json"),
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

    /// Stable registry iteration for an explicit workbench/manifest topology selector.
    pub fn definitions(&self) -> impl Iterator<Item = &TemplateDefinition> {
        self.definitions.values()
    }

    #[must_use]
    pub fn diagnose_compatibility(
        from: &TemplateDefinition,
        to: &TemplateDefinition,
    ) -> TemplateCompatibilityDiagnostic {
        if from.identity == to.identity && from.snapshot().ok() == to.snapshot().ok() {
            return TemplateCompatibilityDiagnostic::Compatible;
        }
        let mut reasons = Vec::new();
        if from.identity.compatibility_key != to.identity.compatibility_key {
            reasons.push("compatibility_key_changed".into());
        }
        if from.stable_order != to.stable_order {
            reasons.push("stable_slot_order_changed".into());
        }
        if from
            .slots
            .iter()
            .map(|slot| (&slot.slot_key, slot.allocation, slot.hotspot))
            .ne(to
                .slots
                .iter()
                .map(|slot| (&slot.slot_key, slot.allocation, slot.hotspot)))
        {
            reasons.push("pinned_geometry_changed".into());
        }
        if reasons.is_empty() {
            reasons.push("template_version_or_semantics_changed".into());
        }
        TemplateCompatibilityDiagnostic::ExplicitTopologyChange {
            from: from.identity.clone(),
            to: to.identity.clone(),
            reasons,
        }
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
    #[error("template slot fit semantics are incompatible with its role: {0}")]
    InvalidFitSemantics(String),
    #[error("template allocations overlap at slot: {0}")]
    OverlappingAllocation(String),
    #[error("template radial parameters are invalid: {0}")]
    InvalidRadialParameters(String),
    #[error("template is duplicated: {template_id}@{template_version}")]
    DuplicateTemplate {
        template_id: String,
        template_version: String,
    },
    #[error("template output dimensions must be nonzero")]
    InvalidOutputSize,
    #[error("weighted template grammar is malformed")]
    InvalidWeightedGrammar,
}

/// Compiles an authored weighted split grammar with exact largest-remainder allocation.
/// The returned rectangles are authoring output and must be persisted before material compilation.
pub fn compile_weighted_grammar(
    grammar: &WeightedTemplateGrammar,
) -> Result<BTreeMap<String, CanonicalRect>, TemplateRegistryError> {
    let mut output = BTreeMap::new();
    compile_grammar_node(
        grammar,
        CanonicalRect {
            x: 0,
            y: 0,
            width: CANONICAL_TEMPLATE_EDGE,
            height: CANONICAL_TEMPLATE_EDGE,
        },
        0,
        &mut output,
    )?;
    Ok(output)
}

fn compile_grammar_node(
    grammar: &WeightedTemplateGrammar,
    rect: CanonicalRect,
    depth: usize,
    output: &mut BTreeMap<String, CanonicalRect>,
) -> Result<(), TemplateRegistryError> {
    if depth > 32 || output.len() > 4_096 {
        return Err(TemplateRegistryError::InvalidWeightedGrammar);
    }
    match grammar {
        WeightedTemplateGrammar::Slot { slot_key } => {
            if slot_key.trim().is_empty() || output.insert(slot_key.clone(), rect).is_some() {
                return Err(TemplateRegistryError::InvalidWeightedGrammar);
            }
        }
        WeightedTemplateGrammar::Horizontal { weights, children }
        | WeightedTemplateGrammar::Vertical { weights, children } => {
            if weights.len() != children.len() || weights.is_empty() || weights.contains(&0) {
                return Err(TemplateRegistryError::InvalidWeightedGrammar);
            }
            let horizontal = matches!(grammar, WeightedTemplateGrammar::Horizontal { .. });
            let lengths =
                largest_remainder(if horizontal { rect.width } else { rect.height }, weights)?;
            let mut cursor = if horizontal { rect.x } else { rect.y };
            for (child, length) in children.iter().zip(lengths) {
                let child_rect = if horizontal {
                    CanonicalRect {
                        x: cursor,
                        width: length,
                        ..rect
                    }
                } else {
                    CanonicalRect {
                        y: cursor,
                        height: length,
                        ..rect
                    }
                };
                cursor = cursor.saturating_add(length);
                compile_grammar_node(child, child_rect, depth + 1, output)?;
            }
        }
    }
    Ok(())
}

fn largest_remainder(total: u32, weights: &[u32]) -> Result<Vec<u32>, TemplateRegistryError> {
    let weight_sum: u64 = weights.iter().map(|weight| u64::from(*weight)).sum();
    if total == 0 || weight_sum == 0 {
        return Err(TemplateRegistryError::InvalidWeightedGrammar);
    }
    let mut allocations = Vec::with_capacity(weights.len());
    let mut assigned = 0_u32;
    let mut remainders = Vec::with_capacity(weights.len());
    for (index, weight) in weights.iter().copied().enumerate() {
        let numerator = u64::from(total) * u64::from(weight);
        let base = u32::try_from(numerator / weight_sum)
            .map_err(|_| TemplateRegistryError::InvalidWeightedGrammar)?;
        allocations.push(base);
        assigned += base;
        remainders.push((numerator % weight_sum, index));
    }
    remainders.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    for (_, index) in remainders.into_iter().take((total - assigned) as usize) {
        allocations[index] += 1;
    }
    if allocations.contains(&0) {
        return Err(TemplateRegistryError::InvalidWeightedGrammar);
    }
    Ok(allocations)
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

fn insert_rect_boundaries(
    rect: CanonicalRect,
    x_boundaries: &mut BTreeSet<u32>,
    y_boundaries: &mut BTreeSet<u32>,
) {
    x_boundaries.insert(rect.x);
    x_boundaries.insert(rect.x + rect.width);
    y_boundaries.insert(rect.y);
    y_boundaries.insert(rect.y + rect.height);
}

fn scale_boundary(boundary: u32, output_edge: u32) -> u32 {
    u32::try_from(
        (u64::from(boundary) * u64::from(output_edge) + u64::from(CANONICAL_TEMPLATE_EDGE / 2))
            / u64::from(CANONICAL_TEMPLATE_EDGE),
    )
    .expect("scaled u32 boundary remains u32")
}

fn scale_rect(
    rect: CanonicalRect,
    x_boundaries: &BTreeMap<u32, u32>,
    y_boundaries: &BTreeMap<u32, u32>,
) -> CanonicalRect {
    let left = x_boundaries[&rect.x];
    let right = x_boundaries[&(rect.x + rect.width)];
    let top = y_boundaries[&rect.y];
    let bottom = y_boundaries[&(rect.y + rect.height)];
    CanonicalRect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    }
}

fn rect_contains(outer: CanonicalRect, inner: CanonicalRect) -> bool {
    inner.width > 0
        && inner.height > 0
        && inner.x >= outer.x
        && inner.y >= outer.y
        && inner.x + inner.width <= outer.x + outer.width
        && inner.y + inner.height <= outer.y + outer.height
}

fn rects_overlap(left: CanonicalRect, right: CanonicalRect) -> bool {
    left.x < right.x + right.width
        && right.x < left.x + left.width
        && left.y < right.y + right.height
        && right.y < left.y + left.height
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
            "compatibilityKey": "wall-v1",             "materialGroup": "architecture",             "variationGroup": "neutral",             "role": "planar", "fit": "planar",             "structuralProfile": "flat",
            "allocation": {"x": 0, "y": 0, "width": 2048, "height": 4096},
            "hotspot": {"x": 8, "y": 8, "width": 2032, "height": 4080},
            "idColor": [64, 65, 66],
            "worldPlacement": {"width": 4.0, "height": 3.0, "rotationDegrees": 0.0},
            "sourceMapping": {"crop": {"x": 0.0, "y": 0.0, "width": 0.5, "height": 1.0}, "addressMode": "clamp"}
          },
          {
            "slotKey": "floor",
            "compatibilityKey": "floor-v1",             "materialGroup": "architecture",             "variationGroup": "radial_fixture",             "role": "radial", "fit": "radial",             "structuralProfile": "radialDisc",
            "allocation": {"x": 2048, "y": 0, "width": 2048, "height": 2048},
            "hotspot": {"x": 2056, "y": 8, "width": 2032, "height": 2032},
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
            "ht.hard_surface_panel",
            "ht.detail_heavy_props",
            "ht.radial_accents",
        ] {
            assert!(
                registry.get(template_id, "1.0.0").is_some(),
                "{template_id}"
            );
        }
    }
}
