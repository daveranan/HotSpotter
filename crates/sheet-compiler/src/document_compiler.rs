use std::{collections::{BTreeMap, BTreeSet}, sync::Arc};

use hot_trimmer_domain::{
    AddressMode, CanonicalRect, CompiledTemplateTopology, ContentReference, DocumentHash, IdColor, MaterialMapKind,
    PatchId, PixelBounds, PixelSize, Projection, RadialMappingSettings, RegionDefinition, RegionId, RegionMapping,
    RegionBehavior,
    SourceId, SourceSetId, NormalizedPoint,
    GridRect, MappingOrigin, NormalizedBounds,
    StructuralProfile, TemplateSlotRole, TrimSheetDocument, TrimSheetDocumentError,
};
use hot_trimmer_render_core::{
    NormalConvention, ProfileKind, StructuralProfile as RenderProfile, StructuralProfileRequest,
    compile_structural_profile,
};
use hot_trimmer_geometry::{Point, Quadrilateral};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const RENDERER_VERSION: &str = "document-1";

#[derive(Clone, Debug, PartialEq)]
pub struct RegisteredMaterialMap {
    pub source_id: SourceId,
    pub material_id: SourceSetId,
    pub kind: MaterialMapKind,
    pub sha256: String,
    pub width: u32,
    pub height: u32,
    pub rgba8: Arc<[u8]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedRegion {
    pub region_id: RegionId,
    pub display_name: String,
    pub allocation_bounds: PixelBounds,
    pub hotspot_bounds: PixelBounds,
    pub id_color: IdColor,
    pub material_id: SourceSetId,
    pub material_id_color: IdColor,
    pub source_patch_id: Option<PatchId>,
    pub source_id: Option<SourceId>,
    pub mapping: RegionMapping,
    pub role: TemplateSlotRole,
    pub behavior: RegionBehavior,
    pub grid_rect: Option<GridRect>,
    pub source_crop: Option<PixelBounds>,
    pub source_bounds: Option<NormalizedBounds>,
    pub mapping_origin: Option<MappingOrigin>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedCompilePlan {
    pub document_revision: u64,
    pub topology_hash: DocumentHash,
    pub appearance_hash: DocumentHash,
    pub dimensions: PixelSize,
    pub regions: Vec<ResolvedRegion>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledMapSet {
    pub base_color: Vec<u8>,
    pub normal: Vec<u8>,
    pub height: Vec<u8>,
    pub roughness: Vec<u8>,
    pub metallic: Vec<u8>,
    pub ambient_occlusion: Vec<u8>,
    pub region_id: Vec<u8>,
    pub material_id: Vec<u8>,
    pub additional: BTreeMap<MaterialMapKind, Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileDiagnostic {
    pub region_id: Option<RegionId>,
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledSheet {
    pub document_revision: u64,
    pub topology_hash: DocumentHash,
    pub appearance_hash: DocumentHash,
    pub renderer_version: String,
    pub dimensions: PixelSize,
    pub maps: CompiledMapSet,
    pub regions: Vec<ResolvedRegion>,
    pub diagnostics: Vec<CompileDiagnostic>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PreviewMapKind {
    BaseColor,
    Normal,
    Height,
    Roughness,
    Metallic,
    AmbientOcclusion,
    RegionId,
    MaterialId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledPreviewMap {
    pub document_revision: u64,
    pub topology_hash: DocumentHash,
    pub appearance_hash: DocumentHash,
    pub dimensions: PixelSize,
    pub pixels: Vec<u8>,
    pub regions: Vec<ResolvedRegion>,
}

#[derive(Debug, Error)]
pub enum SheetCompileError {
    #[error("the trim-sheet document is invalid: {0}")]
    InvalidDocument(#[from] TrimSheetDocumentError),
    #[error("registered map pixels do not match their dimensions")]
    InvalidMapPixels,
    #[error("registered map checksum does not match document material metadata")]
    MapChecksumMismatch,
    #[error("material {0} has no registered Base Color map")]
    MissingBaseColor(SourceSetId),
    #[error("region {0} refers to content unavailable to the compiler")]
    UnsupportedRegionContent(RegionId),
    #[error("region {0} uses mapping outside the planar/crop slice")]
    UnsupportedRegionMapping(RegionId),
    #[error("compiled output exceeds the bounded allocation limit")]
    OutputTooLarge,
    #[error("preview compilation was superseded")]
    Cancelled,
    #[error("structural profile compilation failed: {0}")]
    Structural(#[from] hot_trimmer_render_core::StructuralProfileError),
}

pub fn resolve_compile_plan(
    document: &TrimSheetDocument,
    maps: &[RegisteredMaterialMap],
) -> Result<ResolvedCompilePlan, SheetCompileError> {
    document.validate()?;
    validate_maps(document, maps)?;
    let dimensions = document.render_settings.output_size;
    let mut regions = Vec::new();
    for region in document
        .topology
        .regions
        .iter()
        .filter(|region| region.enabled)
    {
        let binding = document.region_bindings.get(&region.id).unwrap();
        let (material_id, source_patch_id, source_id, mapping) = match binding.content {
            ContentReference::InheritPrimaryMaterial => (
                document.primary_material.unwrap(), None, None, binding.mapping.clone()
            ),
            ContentReference::MaterialSource(id) => (id, None, None, binding.mapping.clone()),
            ContentReference::Patch(id) => {
                let patch = document.patches.iter().find(|patch| patch.id == id && patch.enabled)
                    .ok_or(SheetCompileError::UnsupportedRegionContent(region.id))?;
                let mut mapping = binding.mapping.clone();
                mapping.projection = Projection::Perspective { quad: patch.geometry.corners };
                (document.primary_material.unwrap(), Some(id), Some(patch.source_id), mapping)
            }
            _ => return Err(SheetCompileError::UnsupportedRegionContent(region.id)),
        };
        if !matches!(mapping.projection, Projection::Crop { .. } | Projection::Perspective { .. })
            || !binding.mapping.warps.is_empty()
            || !matches!(document.sheet_framing.projection, Projection::Crop { .. })
        {
            return Err(SheetCompileError::UnsupportedRegionMapping(region.id));
        }
        regions.push(ResolvedRegion {
            region_id: region.id,
            display_name: region.display_name.clone(),
            allocation_bounds: scale_rect(
                region.allocation_rect,
                document.topology.snapshot.canonical_size,
                dimensions,
            )?,
            hotspot_bounds: scale_rect(
                region.hotspot_rect,
                document.topology.snapshot.canonical_size,
                dimensions,
            )?,
            id_color: region.id_color,
            material_id,
            material_id_color: material_id_color(material_id),
            source_patch_id,
            source_id,
            mapping,
            role: region.role,
            behavior: binding.mapping.behavior.clone(),
            grid_rect: region.grid_rect,
            source_crop: resolved_source_crop(document, region),
            source_bounds: resolved_source_bounds(document, region),
            mapping_origin: resolved_source_bounds(document, region).map(|_| {
                if document.source_overrides.contains_key(&region.id) { MappingOrigin::ExplicitOverride } else { MappingOrigin::Partition }
            }),
        });
    }
    Ok(ResolvedCompilePlan {
        document_revision: document.document_revision,
        topology_hash: document.topology.topology_hash,
        appearance_hash: document.appearance_hash()?,
        dimensions,
        regions,
    })
}

/// Resolves the overlay records for a profile-sized compiled topology.
///
/// This deliberately does not consult registered pixels. The SourceFrame compiler has
/// already established the direct crop and profile-local allocation; this function only
/// materializes the same stable identities beside that compiled topology.
pub fn resolve_profile_regions(
    document: &TrimSheetDocument,
    topology: &CompiledTemplateTopology,
) -> Result<Vec<ResolvedRegion>, SheetCompileError> {
    document.validate()?;
    let mut regions = Vec::with_capacity(topology.slots.len());
    for slot in &topology.slots {
        let region = document.topology.regions.iter().find(|region| region.id.to_string() == slot.slot_key)
            .ok_or(SheetCompileError::UnsupportedRegionContent(RegionId::new()))?;
        let binding = document.region_bindings.get(&region.id)
            .ok_or(SheetCompileError::UnsupportedRegionContent(region.id))?;
        let (material_id, source_patch_id, source_id, mut mapping) = match binding.content {
            ContentReference::InheritPrimaryMaterial =>
                (document.primary_material.ok_or(SheetCompileError::UnsupportedRegionContent(region.id))?, None, None, binding.mapping.clone()),
            ContentReference::MaterialSource(id) => (id, None, None, binding.mapping.clone()),
            ContentReference::Patch(id) => {
                let patch = document.patches.iter().find(|patch| patch.id == id && patch.enabled)
                    .ok_or(SheetCompileError::UnsupportedRegionContent(region.id))?;
                let mut mapping = binding.mapping.clone();
                mapping.projection = Projection::Perspective { quad: patch.geometry.corners };
                (document.primary_material.ok_or(SheetCompileError::UnsupportedRegionContent(region.id))?, Some(id), Some(patch.source_id), mapping)
            }
            _ => return Err(SheetCompileError::UnsupportedRegionContent(region.id)),
        };
        let source_bounds = resolved_source_bounds(document, region);
        if let Some(bounds) = source_bounds {
            let focus = NormalizedPoint::new(
                bounds.x.get() + bounds.width.get() * 0.5,
                bounds.y.get() + bounds.height.get() * 0.5,
            ).map_err(|_| SheetCompileError::UnsupportedRegionMapping(region.id))?;
            mapping.projection = Projection::Crop { bounds, focus };
        }
        regions.push(ResolvedRegion {
            region_id: region.id,
            display_name: region.display_name.clone(),
            allocation_bounds: PixelBounds { x: slot.allocation.x, y: slot.allocation.y, width: slot.allocation.width, height: slot.allocation.height },
            hotspot_bounds: PixelBounds { x: slot.hotspot.x, y: slot.hotspot.y, width: slot.hotspot.width, height: slot.hotspot.height },
            id_color: region.id_color,
            material_id,
            material_id_color: material_id_color(material_id),
            source_patch_id,
            source_id,
            mapping,
            role: region.role,
            behavior: binding.mapping.behavior.clone(),
            grid_rect: region.grid_rect,
            source_crop: resolved_source_crop(document, region),
            source_bounds,
            mapping_origin: source_bounds.map(|_| {
                if document.source_overrides.contains_key(&region.id) { MappingOrigin::ExplicitOverride } else { MappingOrigin::Partition }
            }),
        });
    }
    Ok(regions)
}

fn resolved_source_bounds(document: &TrimSheetDocument, region: &RegionDefinition) -> Option<NormalizedBounds> {
    if let Some(override_value) = document.source_overrides.get(&region.id) {
        return Some(override_value.source_bounds);
    }
    let frame = document.source_frame.as_ref()?;
    let grid = document.logical_grid?;
    Some(frame.region_bounds(grid, region.grid_rect?))
}

fn resolved_source_crop(document: &TrimSheetDocument, region: &RegionDefinition) -> Option<PixelBounds> {
    let bounds = resolved_source_bounds(document, region)?;
    let dimensions = document.source_frame.as_ref()?.oriented_dimensions;
    Some(PixelBounds {
        x: (bounds.x.get() * f64::from(dimensions.width)).round() as u32,
        y: (bounds.y.get() * f64::from(dimensions.height)).round() as u32,
        width: (bounds.width.get() * f64::from(dimensions.width)).round() as u32,
        height: (bounds.height.get() * f64::from(dimensions.height)).round() as u32,
    })
}

/// The sole compiler entry point. Its returned regions are the overlay plan used for its pixels.
pub(crate) fn compile_document(
    document: &TrimSheetDocument,
    registered_maps: &[RegisteredMaterialMap],
) -> Result<CompiledSheet, SheetCompileError> {
    let plan = resolve_compile_plan(document, registered_maps)?;
    let bytes = pixel_bytes(plan.dimensions)?;
    let mut maps = CompiledMapSet {
        base_color: solid_map(bytes, [0, 0, 0, 255]),
        normal: solid_map(bytes, [128, 128, 255, 255]),
        height: solid_map(bytes, [128, 128, 128, 255]),
        roughness: solid_map(bytes, [170, 170, 170, 255]),
        metallic: solid_map(bytes, [0, 0, 0, 255]),
        ambient_occlusion: solid_map(bytes, [255, 255, 255, 255]),
        region_id: solid_map(bytes, [0, 0, 0, 255]),
        material_id: solid_map(bytes, [0, 0, 0, 255]),
        additional: BTreeMap::new(),
    };
    let extra_kinds: BTreeSet<_> = registered_maps
        .iter()
        .map(|map| map.kind)
        .filter(|kind| !is_standard_kind(*kind))
        .collect();
    for kind in extra_kinds {
        maps.additional
            .insert(kind, solid_map(bytes, [0, 0, 0, 255]));
    }
    for resolved in &plan.regions {
        let definition = document
            .topology
            .regions
            .iter()
            .find(|region| region.id == resolved.region_id)
            .unwrap();
        let binding = document.region_bindings.get(&resolved.region_id).unwrap();
        render_region_map(
            &mut maps.base_color,
            plan.dimensions,
            resolved,
            registered_maps,
            MaterialMapKind::BaseColor,
            binding,
            document,
        )?;
        for (kind, output) in [
            (MaterialMapKind::Normal, &mut maps.normal),
            (MaterialMapKind::Height, &mut maps.height),
            (MaterialMapKind::Roughness, &mut maps.roughness),
            (MaterialMapKind::Metallic, &mut maps.metallic),
            (
                MaterialMapKind::AmbientOcclusion,
                &mut maps.ambient_occlusion,
            ),
        ] {
            if has_map(registered_maps, resolved.material_id, kind) {
                render_region_map(
                    output,
                    plan.dimensions,
                    resolved,
                    registered_maps,
                    kind,
                    binding,
                    document,
                )?;
            }
        }
        for (kind, output) in &mut maps.additional {
            if has_map(registered_maps, resolved.material_id, *kind) {
                render_region_map(
                    output,
                    plan.dimensions,
                    resolved,
                    registered_maps,
                    *kind,
                    binding,
                    document,
                )?;
            }
        }
        paint_ids(
            &mut maps.region_id,
            plan.dimensions.width,
            resolved.allocation_bounds,
            resolved.id_color.0,
        );
        paint_ids(
            &mut maps.material_id,
            plan.dimensions.width,
            resolved.allocation_bounds,
            resolved.material_id_color.0,
        );
        apply_structure(&mut maps, plan.dimensions, resolved, definition)?;
        for output in [
            &mut maps.base_color,
            &mut maps.normal,
            &mut maps.height,
            &mut maps.roughness,
            &mut maps.metallic,
            &mut maps.ambient_occlusion,
        ] {
            dilate_region(output, plan.dimensions.width, resolved.allocation_bounds, resolved.hotspot_bounds);
        }
        for output in maps.additional.values_mut() {
            dilate_region(output, plan.dimensions.width, resolved.allocation_bounds, resolved.hotspot_bounds);
        }
    }
    Ok(CompiledSheet {
        document_revision: plan.document_revision,
        topology_hash: plan.topology_hash,
        appearance_hash: plan.appearance_hash,
        renderer_version: RENDERER_VERSION.into(),
        dimensions: plan.dimensions,
        maps,
        regions: plan.regions,
        diagnostics: Vec::new(),
    })
}

/// Compiles only the map displayed by the editor at a bounded resolution. This intentionally
/// shares plan resolution, mapping, sampling, and structural shading with final compilation.
pub fn compile_preview_map(
    document: &TrimSheetDocument,
    registered_maps: &[RegisteredMaterialMap],
    kind: PreviewMapKind,
    max_edge: u32,
) -> Result<CompiledPreviewMap, SheetCompileError> {
    compile_preview_map_incremental(
        document,
        registered_maps,
        kind,
        max_edge,
        None,
        None,
        || false,
    )
}

/// Reuses a settled preview surface and rerenders only one dirty region when supplied.
/// Cancellation is observed before every expensive region phase.
pub fn compile_preview_map_incremental<F>(
    document: &TrimSheetDocument,
    registered_maps: &[RegisteredMaterialMap],
    kind: PreviewMapKind,
    max_edge: u32,
    base_pixels: Option<Vec<u8>>,
    dirty_region: Option<RegionId>,
    is_cancelled: F,
) -> Result<CompiledPreviewMap, SheetCompileError>
where
    F: Fn() -> bool,
{
    // A region-only render is meaningful only as a composite over a complete sheet. Fall back to
    // a full bounded preview whenever no base exists; never manufacture a partial black surface.
    let dirty_region = dirty_region.filter(|_| base_pixels.is_some());
    let mut preview_document = document.clone();
    let source_size = preview_document.render_settings.output_size;
    let edge = source_size.width.max(source_size.height);
    if edge > max_edge.max(1) {
        let scale = f64::from(max_edge.max(1)) / f64::from(edge);
        preview_document.render_settings.output_size = PixelSize {
            width: (f64::from(source_size.width) * scale).round().max(1.0) as u32,
            height: (f64::from(source_size.height) * scale).round().max(1.0) as u32,
        };
    }
    let plan = resolve_compile_plan(&preview_document, registered_maps)?;
    let bytes = pixel_bytes(plan.dimensions)?;
    let background = match kind {
        PreviewMapKind::BaseColor | PreviewMapKind::Metallic | PreviewMapKind::RegionId | PreviewMapKind::MaterialId => [0, 0, 0, 255],
        PreviewMapKind::Normal => [128, 128, 255, 255],
        PreviewMapKind::Height => [128, 128, 128, 255],
        PreviewMapKind::Roughness => [170, 170, 170, 255],
        PreviewMapKind::AmbientOcclusion => [255, 255, 255, 255],
    };
    let mut pixels = base_pixels.filter(|pixels| pixels.len() == bytes)
        .unwrap_or_else(|| solid_map(bytes, background));
    let selected: Vec<_> = plan.regions.iter()
        .filter(|region| dirty_region.is_none_or(|dirty| region.region_id == dirty))
        .collect();
    for resolved in selected {
        if is_cancelled() {
            return Err(SheetCompileError::Cancelled);
        }
        if dirty_region.is_some() {
            fill_bounds(&mut pixels, plan.dimensions.width, resolved.allocation_bounds, background);
        }
        let definition = preview_document.topology.regions.iter()
            .find(|region| region.id == resolved.region_id).unwrap();
        let binding = preview_document.region_bindings.get(&resolved.region_id).unwrap();
        match kind {
            PreviewMapKind::RegionId => paint_ids(&mut pixels, plan.dimensions.width, resolved.allocation_bounds, resolved.id_color.0),
            PreviewMapKind::MaterialId => paint_ids(&mut pixels, plan.dimensions.width, resolved.allocation_bounds, resolved.material_id_color.0),
            map_kind => {
                let material_kind = match map_kind {
                    PreviewMapKind::BaseColor => MaterialMapKind::BaseColor,
                    PreviewMapKind::Normal => MaterialMapKind::Normal,
                    PreviewMapKind::Height => MaterialMapKind::Height,
                    PreviewMapKind::Roughness => MaterialMapKind::Roughness,
                    PreviewMapKind::Metallic => MaterialMapKind::Metallic,
                    PreviewMapKind::AmbientOcclusion => MaterialMapKind::AmbientOcclusion,
                    PreviewMapKind::RegionId | PreviewMapKind::MaterialId => unreachable!(),
                };
                if material_kind == MaterialMapKind::BaseColor || has_map(registered_maps, resolved.material_id, material_kind) {
                    render_region_map(&mut pixels, plan.dimensions, resolved, registered_maps, material_kind, binding, &preview_document)?;
                }
                if is_cancelled() {
                    return Err(SheetCompileError::Cancelled);
                }
                apply_structure_to_preview(&mut pixels, plan.dimensions, resolved, definition, map_kind)?;
                dilate_region(&mut pixels, plan.dimensions.width, resolved.allocation_bounds, resolved.hotspot_bounds);
            }
        }
    }
    Ok(CompiledPreviewMap {
        document_revision: document.document_revision,
        topology_hash: plan.topology_hash,
        appearance_hash: document.appearance_hash()?,
        dimensions: plan.dimensions,
        pixels,
        regions: plan.regions,
    })
}

fn fill_bounds(output: &mut [u8], width: u32, bounds: PixelBounds, color: [u8; 4]) {
    for y in bounds.y..bounds.y + bounds.height {
        for x in bounds.x..bounds.x + bounds.width {
            let offset = ((y * width + x) * 4) as usize;
            output[offset..offset + 4].copy_from_slice(&color);
        }
    }
}

fn apply_structure_to_preview(
    output: &mut [u8],
    sheet: PixelSize,
    resolved: &ResolvedRegion,
    region: &RegionDefinition,
    kind: PreviewMapKind,
) -> Result<(), SheetCompileError> {
    let compiled = compile_structural_profile(StructuralProfileRequest {
        profile: RenderProfile::for_kind(profile_kind(region.structural_profile)),
        hotspot: resolved.hotspot_bounds,
        sheet_size: sheet,
        normal_convention: NormalConvention::OpenGl,
    })?;
    let bounds = resolved.hotspot_bounds;
    for y in 0..bounds.height {
        for x in 0..bounds.width {
            let local = (y * bounds.width + x) as usize;
            let offset = (((bounds.y + y) * sheet.width + bounds.x + x) * 4) as usize;
            let height = (0.5 + f64::from(compiled.height_f32[local])).clamp(0.0, 1.0);
            match kind {
                PreviewMapKind::BaseColor => {
                    let shade = (0.80 + height * 0.38).clamp(0.0, 1.15);
                    for component in &mut output[offset..offset + 3] {
                        *component = (f64::from(*component) * shade).round().clamp(0.0, 255.0) as u8;
                    }
                }
                PreviewMapKind::Normal => output[offset..offset + 4]
                    .copy_from_slice(&compiled.normal_rgba8[local * 4..local * 4 + 4]),
                PreviewMapKind::Height => {
                    let channel = (height * 255.0).round() as u8;
                    output[offset..offset + 4].copy_from_slice(&[channel, channel, channel, 255]);
                }
                PreviewMapKind::Roughness => {
                    let value = (170.0 + (0.5 - height).max(0.0) * 70.0).round().clamp(0.0, 255.0) as u8;
                    output[offset..offset + 4].copy_from_slice(&[value, value, value, 255]);
                }
                PreviewMapKind::Metallic => output[offset..offset + 4].copy_from_slice(&[0, 0, 0, 255]),
                PreviewMapKind::AmbientOcclusion => {
                    let value = (255.0 - (0.5 - height).max(0.0) * 130.0).round().clamp(0.0, 255.0) as u8;
                    output[offset..offset + 4].copy_from_slice(&[value, value, value, 255]);
                }
                PreviewMapKind::RegionId | PreviewMapKind::MaterialId => {}
            }
        }
    }
    Ok(())
}

fn validate_maps(
    document: &TrimSheetDocument,
    maps: &[RegisteredMaterialMap],
) -> Result<(), SheetCompileError> {
    for map in maps {
        if map.width == 0
            || map.height == 0
            || map.rgba8.len()
                != pixel_bytes(PixelSize {
                    width: map.width,
                    height: map.height,
                })?
        {
            return Err(SheetCompileError::InvalidMapPixels);
        }
        let expected = document
            .materials
            .iter()
            .find(|material| material.id == map.material_id)
            .and_then(|material| material.maps.iter().find(|entry| entry.kind == map.kind))
            .map(|entry| entry.sha256.as_str());
        if expected != Some(map.sha256.as_str()) {
            return Err(SheetCompileError::MapChecksumMismatch);
        }
    }
    for material in &document.materials {
        if !has_map(maps, material.id, MaterialMapKind::BaseColor) {
            return Err(SheetCompileError::MissingBaseColor(material.id));
        }
    }
    Ok(())
}

fn render_region_map(
    output: &mut [u8],
    sheet: PixelSize,
    region: &ResolvedRegion,
    maps: &[RegisteredMaterialMap],
    kind: MaterialMapKind,
    binding: &hot_trimmer_domain::RegionBinding,
    document: &TrimSheetDocument,
) -> Result<(), SheetCompileError> {
    let source = maps.iter().find(|map| {
        if kind == MaterialMapKind::BaseColor && let Some(source_id) = region.source_id {
            map.source_id == source_id
        } else {
            map.material_id == region.material_id && map.kind == kind
        }
    })
        .ok_or(SheetCompileError::MissingBaseColor(region.material_id))?;
    let bounds = region.hotspot_bounds;
    for y in 0..bounds.height {
        for x in 0..bounds.width {
            let u = (f64::from(x) + 0.5) / f64::from(bounds.width);
            let v = (f64::from(y) + 0.5) / f64::from(bounds.height);
            // Role warping happens in allocation-local coordinates. The declared crop is
            // applied afterwards, so changing destination packing can never move source bounds.
            let (u, v) = role_local_uv(region.role, binding.mapping.radial.as_ref(), u, v);
            let (u, v) = preserve_crop_aspect(
                u,
                v,
                &binding.mapping.projection,
                bounds,
                source.width,
                source.height,
            );
            let (u, v) = mapped_uv(
                u,
                v,
                &document.sheet_framing.projection,
                document.sheet_framing.address_mode,
            );
            let (u, v) = mapped_uv(
                u,
                v,
                &binding.mapping.projection,
                binding.mapping.address_mode,
            );
            let (u, v) = transform_uv(u, v, &binding.mapping);
            let sx = sample_index(u, source.width, binding.mapping.address_mode);
            let sy = sample_index(v, source.height, binding.mapping.address_mode);
            let source_offset = ((sy * source.width + sx) * 4) as usize;
            let output_offset = (((bounds.y + y) * sheet.width + bounds.x + x) * 4) as usize;
            output[output_offset..output_offset + 4]
                .copy_from_slice(&source.rgba8[source_offset..source_offset + 4]);
        }
    }
    Ok(())
}

fn role_local_uv(
    role: TemplateSlotRole,
    radial: Option<&RadialMappingSettings>,
    u: f64,
    v: f64,
) -> (f64, f64) {
    let Some(radial) = radial.filter(|_| role == TemplateSlotRole::Radial) else {
        return (u, v);
    };
    let dx = u - radial.center_x;
    let dy = v - radial.center_y;
    let normalized = ((dx * dx + dy * dy).sqrt() * 2.0).clamp(0.0, 1.0);
    let radius = radial.inner_radius
        + (radial.outer_radius - radial.inner_radius) * normalized.powf(radial.falloff);
    let angle = dy.atan2(dx) / std::f64::consts::TAU + 0.5;
    (radius, angle)
}

/// Center-crops a source rectangle to the destination aspect. This is UV-space `cover`: no
/// source texel is stretched merely because packing produced a differently shaped destination.
fn preserve_crop_aspect(
    u: f64,
    v: f64,
    projection: &Projection,
    destination: PixelBounds,
    source_width: u32,
    source_height: u32,
) -> (f64, f64) {
    let Projection::Crop { bounds, .. } = projection else { return (u, v) };
    if destination.width == 0 || destination.height == 0 || source_width == 0 || source_height == 0 {
        return (u, v);
    }
    let source_aspect = bounds.width.get() * f64::from(source_width)
        / (bounds.height.get() * f64::from(source_height));
    let destination_aspect = f64::from(destination.width) / f64::from(destination.height);
    if destination_aspect > source_aspect {
        (u, 0.5 + (v - 0.5) * source_aspect / destination_aspect)
    } else {
        (0.5 + (u - 0.5) * destination_aspect / source_aspect, v)
    }
}

fn mapped_uv(u: f64, v: f64, projection: &Projection, mode: AddressMode) -> (f64, f64) {
    match projection {
        Projection::Crop { bounds, .. } => (
            address(bounds.x.get() + u * bounds.width.get(), mode),
            address(bounds.y.get() + v * bounds.height.get(), mode),
        ),
        Projection::Perspective { quad } => Quadrilateral::new(*quad)
            .and_then(Quadrilateral::source_from_output)
            .ok()
            .and_then(|transform| transform.transform(Point { x: u, y: v }))
            .map_or((u, v), |point| (address(point.x, mode), address(point.y, mode))),
    }
}

fn transform_uv(u: f64, v: f64, mapping: &RegionMapping) -> (f64, f64) {
    let mut x = if mapping.transform.mirror_x {
        1.0 - u
    } else {
        u
    } - 0.5;
    let mut y = if mapping.transform.mirror_y {
        1.0 - v
    } else {
        v
    } - 0.5;
    x /= mapping.transform.scale[0];
    y /= mapping.transform.scale[1];
    let angle = (-mapping.transform.rotation_degrees).to_radians();
    let rotated = (
        x * angle.cos() - y * angle.sin(),
        x * angle.sin() + y * angle.cos(),
    );
    (
        address(
            rotated.0 + 0.5 + mapping.transform.offset[0],
            mapping.address_mode,
        ),
        address(
            rotated.1 + 0.5 + mapping.transform.offset[1],
            mapping.address_mode,
        ),
    )
}

fn address(value: f64, mode: AddressMode) -> f64 {
    match mode {
        AddressMode::Clamp => value.clamp(0.0, 1.0),
        AddressMode::Repeat => value.rem_euclid(1.0),
        AddressMode::MirroredRepeat => {
            let period = value.rem_euclid(2.0);
            if period <= 1.0 { period } else { 2.0 - period }
        }
    }
}

fn sample_index(value: f64, edge: u32, mode: AddressMode) -> u32 {
    ((address(value, mode) * f64::from(edge)).floor() as u32).min(edge - 1)
}

fn apply_structure(
    maps: &mut CompiledMapSet,
    sheet: PixelSize,
    resolved: &ResolvedRegion,
    region: &RegionDefinition,
) -> Result<(), SheetCompileError> {
    let compiled = compile_structural_profile(StructuralProfileRequest {
        profile: RenderProfile::for_kind(profile_kind(region.structural_profile)),
        hotspot: resolved.hotspot_bounds,
        sheet_size: sheet,
        normal_convention: NormalConvention::OpenGl,
    })?;
    let bounds = resolved.hotspot_bounds;
    for y in 0..bounds.height {
        for x in 0..bounds.width {
            let local = (y * bounds.width + x) as usize;
            let offset = (((bounds.y + y) * sheet.width + bounds.x + x) * 4) as usize;
            let height = (0.5 + f64::from(compiled.height_f32[local])).clamp(0.0, 1.0);
            let channel = (height * 255.0).round() as u8;
            maps.height[offset..offset + 4].copy_from_slice(&[channel, channel, channel, 255]);
            maps.normal[offset..offset + 4]
                .copy_from_slice(&compiled.normal_rgba8[local * 4..local * 4 + 4]);
            let shade = (0.80 + height * 0.38).clamp(0.0, 1.15);
            for component in &mut maps.base_color[offset..offset + 3] {
                *component = (f64::from(*component) * shade).round().clamp(0.0, 255.0) as u8;
            }
            let cavity = (0.5 - height).max(0.0);
            let roughness = (170.0 + cavity * 70.0).round().clamp(0.0, 255.0) as u8;
            maps.roughness[offset..offset + 4]
                .copy_from_slice(&[roughness, roughness, roughness, 255]);
            maps.metallic[offset..offset + 4].copy_from_slice(&[0, 0, 0, 255]);
            let ao = (255.0 - cavity * 130.0).round().clamp(0.0, 255.0) as u8;
            maps.ambient_occlusion[offset..offset + 4].copy_from_slice(&[ao, ao, ao, 255]);
        }
    }
    Ok(())
}

fn profile_kind(profile: StructuralProfile) -> ProfileKind {
    match profile {
        StructuralProfile::Flat => ProfileKind::Flat,
        StructuralProfile::Bevel => ProfileKind::ConvexBevel45,
        StructuralProfile::Groove => ProfileKind::ConcaveGroove45,
        StructuralProfile::RoundedBevel => ProfileKind::RoundedBevel,
        StructuralProfile::PanelFrame => ProfileKind::PanelFrame,
        StructuralProfile::RadialDisc => ProfileKind::RadialDisc,
        StructuralProfile::Annulus => ProfileKind::Annulus,
    }
}

fn paint_ids(output: &mut [u8], width: u32, bounds: PixelBounds, color: [u8; 3]) {
    for y in bounds.y..bounds.y + bounds.height {
        for x in bounds.x..bounds.x + bounds.width {
            let offset = ((y * width + x) * 4) as usize;
            output[offset..offset + 4].copy_from_slice(&[color[0], color[1], color[2], 255]);
        }
    }
}

fn dilate_region(
    output: &mut [u8],
    width: u32,
    allocation: PixelBounds,
    content: PixelBounds,
) {
    for y in allocation.y..allocation.y + allocation.height {
        for x in allocation.x..allocation.x + allocation.width {
            if x >= content.x && x < content.x + content.width
                && y >= content.y && y < content.y + content.height
            {
                continue;
            }
            let source_x = x.clamp(content.x, content.x + content.width - 1);
            let source_y = y.clamp(content.y, content.y + content.height - 1);
            let source = ((source_y * width + source_x) * 4) as usize;
            let target = ((y * width + x) * 4) as usize;
            let pixel = [output[source], output[source + 1], output[source + 2], output[source + 3]];
            output[target..target + 4].copy_from_slice(&pixel);
        }
    }
}

fn scale_rect(
    rect: CanonicalRect,
    canonical: PixelSize,
    output: PixelSize,
) -> Result<PixelBounds, SheetCompileError> {
    let scale = |value: u32, source: u32, target: u32| {
        (f64::from(value) * f64::from(target) / f64::from(source)).round() as u32
    };
    let x = scale(rect.x, canonical.width, output.width);
    let y = scale(rect.y, canonical.height, output.height);
    let right = scale(rect.x + rect.width, canonical.width, output.width);
    let bottom = scale(rect.y + rect.height, canonical.height, output.height);
    if right <= x || bottom <= y || right > output.width || bottom > output.height {
        return Err(SheetCompileError::OutputTooLarge);
    }
    Ok(PixelBounds {
        x,
        y,
        width: right - x,
        height: bottom - y,
    })
}

fn material_id_color(id: SourceSetId) -> IdColor {
    let bytes = id.to_bytes();
    IdColor([
        64 | (bytes[0] & 0xbf),
        64 | (bytes[5] & 0xbf),
        64 | (bytes[10] & 0xbf),
    ])
}

fn has_map(maps: &[RegisteredMaterialMap], id: SourceSetId, kind: MaterialMapKind) -> bool {
    maps.iter()
        .any(|map| map.material_id == id && map.kind == kind)
}

fn is_standard_kind(kind: MaterialMapKind) -> bool {
    matches!(
        kind,
        MaterialMapKind::BaseColor
            | MaterialMapKind::Normal
            | MaterialMapKind::Height
            | MaterialMapKind::Roughness
            | MaterialMapKind::Metallic
            | MaterialMapKind::AmbientOcclusion
            | MaterialMapKind::MaterialId
    )
}

fn solid_map(bytes: usize, color: [u8; 4]) -> Vec<u8> {
    let mut output = Vec::with_capacity(bytes);
    for _ in 0..bytes / 4 {
        output.extend_from_slice(&color);
    }
    output
}

fn pixel_bytes(size: PixelSize) -> Result<usize, SheetCompileError> {
    usize::try_from(u64::from(size.width) * u64::from(size.height) * 4)
        .map_err(|_| SheetCompileError::OutputTooLarge)
}

#[cfg(test)]
mod tests {
    use hot_trimmer_domain::{
        LayoutId, MaterialMapContent, MaterialSourceSet, SourceSetId, TemplateRegistry,
        SourceCropIntent, TrimSheetDocumentCommand,
    };

    use super::*;

    #[test]
    fn trim_sheet_vertical_compiles_pixels_and_overlays_from_one_plan() {
        let template = TemplateRegistry::built_in()
            .unwrap()
            .get("ht.generic_architecture", "1.0.0")
            .unwrap()
            .clone();
        let material_id = SourceSetId::new();
        let checksum = "a".repeat(64);
        let document = TrimSheetDocument::from_template(
            LayoutId::new(),
            &template,
            vec![MaterialSourceSet {
                id: material_id,
                name: "Concrete".into(),
                maps: vec![MaterialMapContent {
                    kind: MaterialMapKind::BaseColor,
                    sha256: checksum.clone(),
                }],
            }],
            Vec::new(),
        )
        .unwrap()
        .apply_command(&TrimSheetDocumentCommand::SetOutputResolution {
            output_size: PixelSize {
                width: 64,
                height: 64,
            },
        })
        .unwrap();
        let mut pixels = Vec::new();
        for _ in 0..4 {
            pixels.extend_from_slice(&[90, 100, 110, 255]);
        }
        let source = RegisteredMaterialMap {
            source_id: SourceId::new(),
            material_id,
            kind: MaterialMapKind::BaseColor,
            sha256: checksum,
            width: 2,
            height: 2,
            rgba8: pixels.into(),
        };
        let compiled = compile_document(&document, &[source]).unwrap();
        assert_eq!(compiled.document_revision, document.document_revision);
        assert_eq!(compiled.topology_hash, document.topology.topology_hash);
        assert_eq!(
            compiled.appearance_hash,
            document.appearance_hash().unwrap()
        );
        assert_eq!(compiled.regions.len(), document.topology.regions.len());
        assert!(compiled.maps.base_color.iter().any(|value| *value != 0));
        for region in &compiled.regions {
            for y in region.allocation_bounds.y..region.allocation_bounds.y + region.allocation_bounds.height {
                for x in region.allocation_bounds.x..region.allocation_bounds.x + region.allocation_bounds.width {
                    let offset = ((y * compiled.dimensions.width + x) * 4) as usize;
                    let pixel = &compiled.maps.base_color[offset..offset + 4];
                    assert_eq!(pixel[3], 255, "region {} contains transparent output", region.display_name);
                    assert!(
                        pixel[..3].iter().any(|component| *component != 0),
                        "region {} contains black output at ({x}, {y})",
                        region.display_name
                    );
                }
            }
        }
    }

    #[test]
    fn source_first_document_template_regions_sample_declared_distinct_crops_for_registered_maps() {
        let template = TemplateRegistry::built_in()
            .unwrap()
            .get("ht.generic_architecture", "1.0.0")
            .unwrap()
            .clone();
        let material_id = SourceSetId::new();
        let base_checksum = "b".repeat(64);
        let specular_checksum = "c".repeat(64);
        let edge_checksum = "d".repeat(64);
        let document = TrimSheetDocument::from_template(
            LayoutId::new(),
            &template,
            vec![MaterialSourceSet {
                id: material_id,
                name: "Grid".into(),
                maps: vec![
                    MaterialMapContent {
                        kind: MaterialMapKind::BaseColor,
                        sha256: base_checksum.clone(),
                    },
                    MaterialMapContent {
                        kind: MaterialMapKind::Specular,
                        sha256: specular_checksum.clone(),
                    },
                    MaterialMapContent {
                        kind: MaterialMapKind::EdgeMask,
                        sha256: edge_checksum.clone(),
                    },
                ],
            }],
            Vec::new(),
        )
        .unwrap()
        .apply_command(&TrimSheetDocumentCommand::SetOutputResolution {
            output_size: PixelSize {
                width: 128,
                height: 128,
            },
        })
        .unwrap();
        let base = RegisteredMaterialMap {
            source_id: SourceId::new(),
            material_id,
            kind: MaterialMapKind::BaseColor,
            sha256: base_checksum,
            width: 16,
            height: 16,
            rgba8: coordinate_grid(16, 16, 0).into(),
        };
        let specular = RegisteredMaterialMap {
            source_id: SourceId::new(),
            material_id,
            kind: MaterialMapKind::Specular,
            sha256: specular_checksum,
            width: 16,
            height: 16,
            rgba8: coordinate_grid(16, 16, 64).into(),
        };
        let edge = RegisteredMaterialMap {
            source_id: SourceId::new(),
            material_id,
            kind: MaterialMapKind::EdgeMask,
            sha256: edge_checksum,
            width: 16,
            height: 16,
            rgba8: coordinate_grid(16, 16, 128).into(),
        };
        let registered_maps = [base, specular, edge];
        let compiled = compile_document(&document, &registered_maps).unwrap();
        let specular_map = compiled
            .maps
            .additional
            .get(&MaterialMapKind::Specular)
            .expect("specular compiled");
        let edge_map = compiled
            .maps
            .additional
            .get(&MaterialMapKind::EdgeMask)
            .expect("edge mask compiled");
        let sampled_slots = [
            template.stable_order[0].as_str(),
            template.stable_order[9].as_str(),
            template.stable_order[21].as_str(),
        ];
        let sample_points = [(0.25, 0.25), (0.5, 0.5), (0.75, 0.75)];
        for slot_key in sampled_slots {
            let region = compiled
                .regions
                .iter()
                .find(|candidate| candidate.display_name == title_from_slot_key(slot_key))
                .expect("compiled region exists for sampled template slot");
            assert_eq!(region.mapping.source_crop_intent, Some(SourceCropIntent::Unplaced));
            assert_eq!(region.mapping.projection, Projection::default());
            for local in sample_points {
                let sheet_point = point_in_region(region.allocation_bounds, local);
                assert_eq!(
                    pixel_at(specular_map, compiled.dimensions.width, sheet_point),
                    expected_sample_at_point(region, sheet_point, 16, 16, 64)
                );
                assert_eq!(
                    pixel_at(edge_map, compiled.dimensions.width, sheet_point),
                    expected_sample_at_point(region, sheet_point, 16, 16, 128)
                );
            }
        }
        let full_preview = compile_preview_map(
            &document,
            &registered_maps,
            PreviewMapKind::BaseColor,
            64,
        )
        .unwrap();
        let preview_without_a_complete_base = compile_preview_map_incremental(
            &document,
            &registered_maps,
            PreviewMapKind::BaseColor,
            64,
            None,
            Some(compiled.regions[0].region_id),
            || false,
        )
        .unwrap();
        assert_eq!(
            preview_without_a_complete_base.pixels, full_preview.pixels,
            "a dirty preview without a complete base must render the complete sheet"
        );
    }

    fn coordinate_grid(width: u32, height: u32, marker: u8) -> Vec<u8> {
        let mut pixels = Vec::new();
        for y in 0..height {
            for x in 0..width {
                pixels.extend_from_slice(&[
                    (x * 16).min(255) as u8,
                    (y * 16).min(255) as u8,
                    marker,
                    255,
                ]);
            }
        }
        pixels
    }

    fn expected_sample_at_point(
        region: &ResolvedRegion,
        point: (u32, u32),
        width: u32,
        height: u32,
        marker: u8,
    ) -> [u8; 4] {
        let local_u = (f64::from(point.0 - region.allocation_bounds.x) + 0.5)
            / f64::from(region.allocation_bounds.width);
        let local_v = (f64::from(point.1 - region.allocation_bounds.y) + 0.5)
            / f64::from(region.allocation_bounds.height);
        let (local_u, local_v) = role_local_uv(region.role, region.mapping.radial.as_ref(), local_u, local_v);
        let (local_u, local_v) = preserve_crop_aspect(
            local_u,
            local_v,
            &region.mapping.projection,
            region.allocation_bounds,
            width,
            height,
        );
        let (u, v) = mapped_uv(
            local_u,
            local_v,
            &region.mapping.projection,
            region.mapping.address_mode,
        );
        let x = sample_index(u, width, region.mapping.address_mode);
        let y = sample_index(v, height, region.mapping.address_mode);
        [
            (x * 16).min(255) as u8,
            (y * 16).min(255) as u8,
            marker,
            255,
        ]
    }

    fn point_in_region(bounds: PixelBounds, local: (f64, f64)) -> (u32, u32) {
        (
            bounds.x + ((f64::from(bounds.width) * local.0).floor() as u32).min(bounds.width - 1),
            bounds.y + ((f64::from(bounds.height) * local.1).floor() as u32).min(bounds.height - 1),
        )
    }

    fn title_from_slot_key(key: &str) -> String {
        let mut words = key.split(['_', '-']).filter(|part| !part.is_empty());
        let mut title = String::new();
        while let Some(word) = words.next() {
            if !title.is_empty() {
                title.push(' ');
            }
            let mut chars = word.chars();
            if let Some(first) = chars.next() {
                title.extend(first.to_uppercase());
                title.extend(chars);
            }
        }
        title
    }

    fn pixel_at(pixels: &[u8], width: u32, point: (u32, u32)) -> [u8; 4] {
        let offset = ((point.1 * width + point.0) * 4) as usize;
        [
            pixels[offset],
            pixels[offset + 1],
            pixels[offset + 2],
            pixels[offset + 3],
        ]
    }
}
