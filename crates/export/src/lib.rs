#![doc = "Snapshot-based, validated, atomic export boundary."]

use std::{collections::BTreeMap, fs, path::Path};

use hot_trimmer_domain::{CanonicalRect, RadialParameters, TemplateDefinition};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const EXPORT_PRESET_VERSION: u16 = 1;
pub const HOTTRIM_MANIFEST_FILE_NAME: &str = "manifest.hottrim.json";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapRecord {
    pub role: String,
    pub relative_path: String,
    pub dimensions: [u32; 2],
    pub bit_depth: u8,
    pub color_space: String,
    pub checksum: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UvFitKind {
    Rectangular,
    Radial,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FitAxis {
    Automatic,
    None,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UvFit {
    pub kind: UvFitKind,
    pub fit_axis: FitAxis,
    pub keep_proportion: bool,
    pub allowed_rotations: Vec<u16>,
    pub mirror_allowed: bool,
    pub classification_tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HottrimSlot {
    pub slot_id: String,
    pub region_id: String,
    pub name: String,
    pub allocation_rect: PixelRect,
    pub pixel_hotspot_rect: PixelRect,
    pub normalized_hotspot_rect: NormalizedRect,
    pub role: String,
    pub uv_fit: UvFit,
    pub world_size_meters: [f64; 2],
    pub variation_group: String,
    pub enabled: bool,
    pub region_id_color: [u8; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radial_parameters: Option<RadialParameters>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HottrimManifest {
    pub schema_version: u16,
    pub project_id: String,
    pub material_id: String,
    pub material_name: String,
    pub material_revision: u64,
    pub template_id: String,
    pub template_version: String,
    pub compatibility_key: String,
    pub template_snapshot_hash: String,
    pub output_size: [u32; 2],
    pub normal_orientation: String,
    pub maps: BTreeMap<String, MapRecord>,
    pub slots: Vec<HottrimSlot>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ManifestExportInput {
    pub project_id: String,
    pub material_id: String,
    pub material_name: String,
    pub material_revision: u64,
    pub output_size: [u32; 2],
    pub normal_orientation: String,
    pub maps: BTreeMap<String, MapRecord>,
}

#[derive(Debug, Error)]
pub enum ManifestExportError {
    #[error("manifest output size must be nonzero")]
    InvalidOutputSize,
    #[error("template snapshot failed: {0}")]
    Template(#[from] hot_trimmer_domain::TemplateRegistryError),
    #[error("manifest serialization failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("manifest package write failed: {0}")]
    Io(#[from] std::io::Error),
}

/// Builds manifest semantics from template metadata and never from an ID map.
pub fn manifest_from_template(
    template: &TemplateDefinition,
    input: ManifestExportInput,
) -> Result<HottrimManifest, ManifestExportError> {
    if input.output_size.contains(&0) {
        return Err(ManifestExportError::InvalidOutputSize);
    }
    let snapshot = template.snapshot()?;
    let slots = template
        .slots
        .iter()
        .map(|slot| slot_manifest(slot, template.canonical_width, template.canonical_height))
        .collect();
    Ok(HottrimManifest {
        schema_version: 1,
        project_id: input.project_id,
        material_id: input.material_id,
        material_name: input.material_name,
        material_revision: input.material_revision,
        template_id: snapshot.identity.template_id,
        template_version: snapshot.identity.template_version,
        compatibility_key: snapshot.identity.compatibility_key,
        template_snapshot_hash: snapshot.snapshot_hash,
        output_size: input.output_size,
        normal_orientation: input.normal_orientation,
        maps: input.maps,
        slots,
    })
}

/// Serializes with a terminal newline so identical revisions are byte-identical.
pub fn manifest_json(manifest: &HottrimManifest) -> Result<String, ManifestExportError> {
    Ok(format!("{}\n", serde_json::to_string_pretty(manifest)?))
}

pub fn write_package_manifest(
    package_directory: &Path,
    manifest: &HottrimManifest,
) -> Result<(), ManifestExportError> {
    fs::write(
        package_directory.join(HOTTRIM_MANIFEST_FILE_NAME),
        manifest_json(manifest)?,
    )?;
    Ok(())
}

fn slot_manifest(slot: &hot_trimmer_domain::TemplateSlot, width: u32, height: u32) -> HottrimSlot {
    let allocation_rect = PixelRect {
        x: slot.allocation.x,
        y: slot.allocation.y,
        width: slot.allocation.width,
        height: slot.allocation.height,
    };
    let radial = slot.radial_parameters;
    let is_radial = radial.is_some();
    let (kind, fit_axis, allowed_rotations, mirror_allowed, classification_tags) = if is_radial {
        (
            UvFitKind::Radial,
            FitAxis::None,
            vec![0],
            false,
            vec!["HOTSPOT".to_owned(), "Radial".to_owned()],
        )
    } else {
        (
            UvFitKind::Rectangular,
            FitAxis::Automatic,
            vec![0, 90, 180, 270],
            true,
            vec!["HOTSPOT".to_owned()],
        )
    };
    HottrimSlot {
        slot_id: slot.slot_key.clone(),
        region_id: format!("{}:{}", slot.compatibility_key, slot.slot_key),
        name: slot.slot_key.clone(),
        allocation_rect,
        pixel_hotspot_rect: allocation_rect,
        normalized_hotspot_rect: normalized_rect(slot.allocation, width, height),
        role: if is_radial { "radial" } else { "rectangular" }.to_owned(),
        uv_fit: UvFit {
            kind,
            fit_axis,
            keep_proportion: true,
            allowed_rotations,
            mirror_allowed,
            classification_tags,
        },
        world_size_meters: [slot.world_placement.width, slot.world_placement.height],
        variation_group: slot.variation_group.clone(),
        enabled: true,
        region_id_color: slot.id_color.0,
        radial_parameters: radial,
    }
}

fn normalized_rect(rect: CanonicalRect, width: u32, height: u32) -> NormalizedRect {
    NormalizedRect {
        x: f64::from(rect.x) / f64::from(width),
        y: f64::from(rect.y) / f64::from(height),
        width: f64::from(rect.width) / f64::from(width),
        height: f64::from(rect.height) / f64::from(height),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hot_trimmer_domain::TemplateRegistry;
    const GENERIC_ARCHITECTURE: &str =
        include_str!("../../../assets/templates/generic_architecture/1.0.0/template.json");
    fn input() -> ManifestExportInput {
        ManifestExportInput {
            project_id: "project-fixture".to_owned(),
            material_id: "material-fixture".to_owned(),
            material_name: "Fixture Concrete".to_owned(),
            material_revision: 7,
            output_size: [2048, 2048],
            normal_orientation: "OpenGL".to_owned(),
            maps: BTreeMap::new(),
        }
    }
    #[test]
    fn generic_architecture_manifest_is_deterministic_and_carries_fit_semantics() {
        let registry =
            TemplateRegistry::from_json(GENERIC_ARCHITECTURE).expect("template registry");
        let template = registry
            .get("ht.generic_architecture", "1.0.0")
            .expect("generic architecture template");
        let first = manifest_from_template(template, input()).expect("manifest");
        assert_eq!(
            manifest_json(&first).expect("json"),
            manifest_json(&manifest_from_template(template, input()).expect("manifest"))
                .expect("json")
        );
        let rectangular = first
            .slots
            .iter()
            .find(|slot| slot.slot_id == "wall_primary")
            .expect("rectangular fixture");
        assert_eq!(rectangular.uv_fit.kind, UvFitKind::Rectangular);
        assert_eq!(
            rectangular.region_id_color,
            template.slots.iter().find(|slot| slot.slot_key == "wall_primary").unwrap().id_color.0,
        );
        let radial = first
            .slots
            .iter()
            .find(|slot| slot.slot_id == "radial_fixture_a")
            .expect("radial fixture");
        assert_eq!(radial.uv_fit.kind, UvFitKind::Radial);
        assert_eq!(
            radial.region_id_color,
            template.slots.iter().find(|slot| slot.slot_key == "radial_fixture_a").unwrap().id_color.0,
        );
        assert!(radial.radial_parameters.is_some());
    }
}
