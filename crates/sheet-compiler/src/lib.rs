#![doc = "Sole orchestration boundary for the twenty-stage material compiler."]

mod algorithm_compiler;
mod document_compiler;
mod slot_synthesis;

pub use algorithm_compiler::*;
pub use document_compiler::{
    CompiledMapSet, CompiledPreviewMap, PreviewMapKind, RegisteredMaterialMap, ResolvedRegion,
    SheetCompileError, compile_preview_map, compile_preview_map_incremental, resolve_compile_plan,
};
pub use slot_synthesis::*;

#[cfg(any())]
mod removed_legacy {

    use hot_trimmer_domain::{
        PixelBounds, PixelSize, RegionSourceLayer, SourceBlend, SourceFraming, SourceFramingMode,
        StructuralProfile as TemplateProfile, TemplateDefinition, TemplateSlot,
    };
    use hot_trimmer_render_core::{
        NormalConvention, ProfileKind, RegionLayerRenderRequest, RenderCancellationToken,
        RenderError, SampleSpace, StructuralProfile, StructuralProfileRequest,
        compile_structural_profile, render_region_layer_rgba8,
    };
    use thiserror::Error;

    #[derive(Clone, Copy, Debug)]
    pub struct SheetCompileRequest<'a> {
        pub source_rgba8: &'a [u8],
        pub source_width: u32,
        pub source_height: u32,
        pub template: &'a TemplateDefinition,
        pub sheet_size: PixelSize,
        pub normal_convention: NormalConvention,
        /// Applied once across the whole sheet before template profiles are layered over it.
        pub source_framing: SourceFraming,
    }

    /// Complete PBR and diagnostic output derived from one continuous source transform.
    #[derive(Clone, Debug, PartialEq)]
    pub struct SheetCompileOutput {
        pub width: u32,
        pub height: u32,
        /// Base Color, retained under this existing field name for preview compatibility.
        pub rgba8: Vec<u8>,
        pub height_f32: Vec<f32>,
        pub height_rgba8: Vec<u8>,
        pub normal_rgba8: Vec<u8>,
        pub roughness_rgba8: Vec<u8>,
        pub metallic_rgba8: Vec<u8>,
        pub ambient_occlusion_rgba8: Vec<u8>,
        pub region_id_rgba8: Vec<u8>,
        pub material_id_rgba8: Vec<u8>,
    }

    /// One executable source-layer override keyed by the stable template region key.
    #[derive(Clone, Copy, Debug)]
    pub struct RegionSourceLayerBinding<'a> {
        pub slot_key: &'a str,
        pub layer: &'a RegionSourceLayer,
    }

    #[derive(Debug, Error)]
    pub enum SheetCompileError {
        #[error("source and output dimensions must be nonzero")]
        InvalidDimensions,
        #[error("source pixels do not match its declared dimensions")]
        SourceLength,
        #[error("the compiled sheet would exceed the bounded allocation limit")]
        OutputTooLarge,
        #[error("a template allocation rounds outside the output sheet")]
        InvalidAllocation,
        #[error("source layer refers to unknown region '{slot_key}'")]
        UnknownRegionSourceLayer { slot_key: String },
        #[error("region '{slot_key}' has more than one source layer")]
        DuplicateRegionSourceLayer { slot_key: String },
        #[error("region '{slot_key}' source mapping failed: {source}")]
        RegionSourceMapping {
            slot_key: String,
            #[source]
            source: RenderError,
        },
        #[error("sheet compilation was cancelled")]
        Cancelled,
        #[error("structural profile compilation failed: {0}")]
        Structural(#[from] hot_trimmer_render_core::StructuralProfileError),
    }

    /// Frames the source once across the entire sheet, then layers fixed semantic structure over it.
    ///
    /// Slots are never independent crops. Their role is to define UV semantics, profile shape, and
    /// diagnostic ID output over a continuous material layer.
    pub fn compile_template_sheet(
        request: SheetCompileRequest<'_>,
    ) -> Result<SheetCompileOutput, SheetCompileError> {
        compile_template_sheet_with_source_layers(request, &[], &RenderCancellationToken::new())
    }

    /// Compiles a sheet with selected region source layers.  Source overrides are rendered in an
    /// isolated buffer and blended only after success, preserving cancellation's no-partial-output
    /// guarantee.
    pub fn compile_template_sheet_with_source_layers(
        request: SheetCompileRequest<'_>,
        source_layers: &[RegionSourceLayerBinding<'_>],
        cancellation: &RenderCancellationToken,
    ) -> Result<SheetCompileOutput, SheetCompileError> {
        if request.source_width == 0
            || request.source_height == 0
            || !request.sheet_size.is_nonzero()
        {
            return Err(SheetCompileError::InvalidDimensions);
        }
        validate_source_layers(request.template, source_layers)?;
        if cancellation.is_cancelled() {
            return Err(SheetCompileError::Cancelled);
        }
        let source_len = pixel_bytes(request.source_width, request.source_height)?;
        if request.source_rgba8.len() != source_len {
            return Err(SheetCompileError::SourceLength);
        }
        let output_len = pixel_bytes(request.sheet_size.width, request.sheet_size.height)?;
        let pixel_count = output_len / 4;
        let mut rgba8 = vec![0; output_len];
        let mut height_f32 = vec![0.5; pixel_count];
        let mut normal_rgba8 = Vec::with_capacity(output_len);
        let mut roughness_rgba8 = Vec::with_capacity(output_len);
        let mut metallic_rgba8 = Vec::with_capacity(output_len);
        let mut ambient_occlusion_rgba8 = Vec::with_capacity(output_len);
        let mut region_id_rgba8 = Vec::with_capacity(output_len);
        let mut material_id_rgba8 = Vec::with_capacity(output_len);
        for _ in 0..pixel_count {
            normal_rgba8.extend_from_slice(&[128, 128, 255, 255]);
            roughness_rgba8.extend_from_slice(&[145, 145, 145, 255]);
            metallic_rgba8.extend_from_slice(&[0, 0, 0, 255]);
            ambient_occlusion_rgba8.extend_from_slice(&[255, 255, 255, 255]);
            region_id_rgba8.extend_from_slice(&[0, 0, 0, 255]);
            material_id_rgba8.extend_from_slice(&[0, 0, 0, 255]);
        }
        render_continuous_source(&mut rgba8, request, cancellation)?;
        for slot_key in &request.template.stable_order {
            let slot = request
                .template
                .slots
                .iter()
                .find(|slot| &slot.slot_key == slot_key)
                .expect("validated template stable order");
            let bounds = checked_scaled_bounds(slot, request.template, request.sheet_size)?;
            if cancellation.is_cancelled() {
                return Err(SheetCompileError::Cancelled);
            }
            if let Some(binding) = source_layers
                .iter()
                .find(|binding| binding.slot_key == slot.slot_key)
            {
                let layer = render_region_layer_rgba8(
                    RegionLayerRenderRequest {
                        source_rgba8: request.source_rgba8,
                        source_width: request.source_width,
                        source_height: request.source_height,
                        layer: binding.layer,
                        output_width: bounds.width,
                        output_height: bounds.height,
                        sample_space: SampleSpace::SrgbColor,
                    },
                    cancellation,
                )
                .map_err(|source| match source {
                    RenderError::Cancelled => SheetCompileError::Cancelled,
                    source => SheetCompileError::RegionSourceMapping {
                        slot_key: slot.slot_key.clone(),
                        source,
                    },
                })?;
                composite_source_layer(
                    &mut rgba8,
                    request.sheet_size.width,
                    bounds,
                    &layer.rgba8,
                    binding.layer.blend,
                    binding.layer.opacity,
                );
            }
            composite_slot(
                &mut rgba8,
                &mut height_f32,
                &mut normal_rgba8,
                &mut roughness_rgba8,
                &mut metallic_rgba8,
                &mut ambient_occlusion_rgba8,
                &mut region_id_rgba8,
                &mut material_id_rgba8,
                request,
                slot,
                bounds,
            )?;
        }
        let height_rgba8 = grayscale_height(&height_f32);
        Ok(SheetCompileOutput {
            width: request.sheet_size.width,
            height: request.sheet_size.height,
            rgba8,
            height_f32,
            height_rgba8,
            normal_rgba8,
            roughness_rgba8,
            metallic_rgba8,
            ambient_occlusion_rgba8,
            region_id_rgba8,
            material_id_rgba8,
        })
    }

    fn validate_source_layers(
        template: &TemplateDefinition,
        source_layers: &[RegionSourceLayerBinding<'_>],
    ) -> Result<(), SheetCompileError> {
        for (index, binding) in source_layers.iter().enumerate() {
            if !template
                .slots
                .iter()
                .any(|slot| slot.slot_key == binding.slot_key)
            {
                return Err(SheetCompileError::UnknownRegionSourceLayer {
                    slot_key: binding.slot_key.to_owned(),
                });
            }
            if source_layers[..index]
                .iter()
                .any(|other| other.slot_key == binding.slot_key)
            {
                return Err(SheetCompileError::DuplicateRegionSourceLayer {
                    slot_key: binding.slot_key.to_owned(),
                });
            }
        }
        Ok(())
    }

    fn render_continuous_source(
        output: &mut [u8],
        request: SheetCompileRequest<'_>,
        cancellation: &RenderCancellationToken,
    ) -> Result<(), SheetCompileError> {
        for y in 0..request.sheet_size.height {
            if cancellation.is_cancelled() {
                return Err(SheetCompileError::Cancelled);
            }
            for x in 0..request.sheet_size.width {
                let u = (f64::from(x) + 0.5) / f64::from(request.sheet_size.width);
                let v = (f64::from(y) + 0.5) / f64::from(request.sheet_size.height);
                let (source_u, source_v) = framed_coordinates(u, v, request);
                let source_x = fractional_index(source_u, request.source_width);
                let source_y = fractional_index(source_v, request.source_height);
                let source_offset = pixel_offset(request.source_width, source_x, source_y);
                let output_offset = pixel_offset(request.sheet_size.width, x, y);
                output[output_offset..output_offset + 4]
                    .copy_from_slice(&request.source_rgba8[source_offset..source_offset + 4]);
            }
        }
        Ok(())
    }

    fn composite_source_layer(
        base: &mut [u8],
        sheet_width: u32,
        bounds: PixelBounds,
        source: &[u8],
        blend: SourceBlend,
        opacity: f64,
    ) {
        for local_y in 0..bounds.height {
            for local_x in 0..bounds.width {
                let base_offset = pixel_offset(sheet_width, bounds.x + local_x, bounds.y + local_y);
                let source_offset = usize::try_from(
                    (u64::from(local_y) * u64::from(bounds.width) + u64::from(local_x)) * 4,
                )
                .expect("bounded rendered region offset");
                let alpha =
                    (f64::from(source[source_offset + 3]) / 255.0 * opacity).clamp(0.0, 1.0);
                for channel in 0..3 {
                    let below = f64::from(base[base_offset + channel]) / 255.0;
                    let above = f64::from(source[source_offset + channel]) / 255.0;
                    let composed = match blend {
                        SourceBlend::Replace | SourceBlend::Normal => above,
                        SourceBlend::Multiply => below * above,
                        SourceBlend::Overlay => {
                            if below <= 0.5 {
                                2.0 * below * above
                            } else {
                                1.0 - 2.0 * (1.0 - below) * (1.0 - above)
                            }
                        }
                    };
                    base[base_offset + channel] =
                        ((below + (composed - below) * alpha) * 255.0).round() as u8;
                }
                base[base_offset + 3] = ((f64::from(base[base_offset + 3]) / 255.0
                    + alpha * (1.0 - f64::from(base[base_offset + 3]) / 255.0))
                    * 255.0)
                    .round() as u8;
            }
        }
    }

    fn framed_coordinates(u: f64, v: f64, request: SheetCompileRequest<'_>) -> (f64, f64) {
        let crop = request.source_framing.crop_bounds;
        let crop_x = crop.x.get();
        let crop_y = crop.y.get();
        let crop_width = crop.width.get();
        let crop_height = crop.height.get();
        let within_crop = |x: f64, y: f64| (crop_x + x * crop_width, crop_y + y * crop_height);
        match request.source_framing.mode {
            SourceFramingMode::Stretch => within_crop(u, v),
            SourceFramingMode::Repeat => within_crop((u * 2.0).fract(), (v * 2.0).fract()),
            SourceFramingMode::Cover => {
                let source_aspect = (f64::from(request.source_width) * crop_width)
                    / (f64::from(request.source_height) * crop_height).max(f64::EPSILON);
                let sheet_aspect =
                    f64::from(request.sheet_size.width) / f64::from(request.sheet_size.height);
                let focus_x = request.source_framing.crop_focus.x.get();
                let focus_y = request.source_framing.crop_focus.y.get();
                if source_aspect > sheet_aspect {
                    let visible_width = sheet_aspect / source_aspect;
                    let left = (focus_x * (1.0 - visible_width)).clamp(0.0, 1.0 - visible_width);
                    within_crop(left + u * visible_width, v)
                } else {
                    let visible_height = source_aspect / sheet_aspect;
                    let top = (focus_y * (1.0 - visible_height)).clamp(0.0, 1.0 - visible_height);
                    within_crop(u, top + v * visible_height)
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn composite_slot(
        base_color: &mut [u8],
        height: &mut [f32],
        normal: &mut [u8],
        roughness: &mut [u8],
        metallic: &mut [u8],
        ambient_occlusion: &mut [u8],
        region_id: &mut [u8],
        material_id: &mut [u8],
        request: SheetCompileRequest<'_>,
        slot: &TemplateSlot,
        bounds: PixelBounds,
    ) -> Result<(), SheetCompileError> {
        let structural = compile_structural_profile(StructuralProfileRequest {
            profile: StructuralProfile::for_kind(profile_kind(slot.structural_profile)),
            hotspot: bounds,
            sheet_size: request.sheet_size,
            normal_convention: request.normal_convention,
        })?;
        let material_colour = material_id_colour(&slot.material_group);
        for local_y in 0..bounds.height {
            for local_x in 0..bounds.width {
                let local = usize::try_from(
                    u64::from(local_y) * u64::from(bounds.width) + u64::from(local_x),
                )
                .map_err(|_| SheetCompileError::OutputTooLarge)?;
                let offset = pixel_offset(
                    request.sheet_size.width,
                    bounds.x + local_x,
                    bounds.y + local_y,
                );
                let height_value =
                    (0.5 + f64::from(structural.height_f32[local])).clamp(0.0, 1.0) as f32;
                height[offset / 4] = height_value;
                normal[offset..offset + 4]
                    .copy_from_slice(&structural.normal_rgba8[local * 4..local * 4 + 4]);
                // This is deliberately subtle: structure reads as a baked edge before weathering recipes arrive.
                let shade = (0.78 + f64::from(height_value) * 0.44).clamp(0.0, 1.2);
                for channel in 0..3 {
                    base_color[offset + channel] = (f64::from(base_color[offset + channel]) * shade)
                        .round()
                        .clamp(0.0, 255.0) as u8;
                }
                let cavity = (0.5 - f64::from(height_value)).max(0.0);
                let highlight = (f64::from(height_value) - 0.5).max(0.0);
                let rough = (145.0 + cavity * 90.0 - highlight * 24.0)
                    .round()
                    .clamp(0.0, 255.0) as u8;
                roughness[offset..offset + 4].copy_from_slice(&[rough, rough, rough, 255]);
                metallic[offset..offset + 4].copy_from_slice(&[0, 0, 0, 255]);
                let ao = (255.0 - cavity * 150.0).round().clamp(0.0, 255.0) as u8;
                ambient_occlusion[offset..offset + 4].copy_from_slice(&[ao, ao, ao, 255]);
                region_id[offset..offset + 4].copy_from_slice(&[
                    slot.id_color.0[0],
                    slot.id_color.0[1],
                    slot.id_color.0[2],
                    255,
                ]);
                material_id[offset..offset + 4].copy_from_slice(&material_colour);
            }
        }
        Ok(())
    }

    fn profile_kind(profile: TemplateProfile) -> ProfileKind {
        match profile {
            TemplateProfile::Flat => ProfileKind::Flat,
            TemplateProfile::Bevel => ProfileKind::ConvexBevel45,
            TemplateProfile::Groove => ProfileKind::ConcaveGroove45,
            TemplateProfile::RoundedBevel => ProfileKind::RoundedBevel,
            TemplateProfile::PanelFrame => ProfileKind::PanelFrame,
            TemplateProfile::RadialDisc => ProfileKind::RadialDisc,
            TemplateProfile::Annulus => ProfileKind::Annulus,
        }
    }

    fn checked_scaled_bounds(
        slot: &TemplateSlot,
        template: &TemplateDefinition,
        sheet: PixelSize,
    ) -> Result<PixelBounds, SheetCompileError> {
        let bounds = scaled_bounds(slot, template, sheet);
        if bounds.width == 0
            || bounds.height == 0
            || bounds.x + bounds.width > sheet.width
            || bounds.y + bounds.height > sheet.height
        {
            return Err(SheetCompileError::InvalidAllocation);
        }
        Ok(bounds)
    }

    fn material_id_colour(group: &str) -> [u8; 4] {
        let hash = group.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
            (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
        });
        [
            64 | (hash as u8 & 0xbf),
            64 | ((hash >> 8) as u8 & 0xbf),
            64 | ((hash >> 16) as u8 & 0xbf),
            255,
        ]
    }

    fn grayscale_height(values: &[f32]) -> Vec<u8> {
        let mut output = Vec::with_capacity(values.len() * 4);
        for value in values {
            let channel = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
            output.extend_from_slice(&[channel, channel, channel, 255]);
        }
        output
    }

    #[allow(dead_code)]
    fn render_slot_legacy(
        output: &mut [u8],
        request: SheetCompileRequest<'_>,
        slot: &TemplateSlot,
        bounds: PixelBounds,
        slot_index: usize,
    ) -> Result<(), SheetCompileError> {
        let profile = StructuralProfile::for_kind(ProfileKind::Flat);
        let structural = compile_structural_profile(StructuralProfileRequest {
            profile,
            hotspot: bounds,
            sheet_size: request.sheet_size,
            normal_convention: request.normal_convention,
        })?;
        let physical_x = (f64::from(bounds.width) / slot.world_placement.width).max(1.0);
        let physical_y = (f64::from(bounds.height) / slot.world_placement.height).max(1.0);
        for local_y in 0..bounds.height {
            for local_x in 0..bounds.width {
                let (u, v, opacity) = if let Some(radial) = slot.radial_parameters {
                    radial_coordinates(
                        local_x,
                        local_y,
                        bounds,
                        radial.center_x,
                        radial.center_y,
                        radial.inner_radius,
                        radial.outer_radius,
                    )
                } else {
                    let u = f64::from(local_x) / physical_x;
                    let v = f64::from(local_y) / physical_y;
                    (u, v, 255)
                };
                let (u, v) = vary_coordinates(u, v, slot_index);
                let source_x = fractional_index(u, request.source_width);
                let source_y = fractional_index(v, request.source_height);
                let source_offset = pixel_offset(request.source_width, source_x, source_y);
                let output_offset = pixel_offset(
                    request.sheet_size.width,
                    bounds.x + local_x,
                    bounds.y + local_y,
                );
                let shade = 0.84
                    + f64::from(structural.height_f32[(local_y * bounds.width + local_x) as usize])
                        * 0.16;
                output[output_offset] =
                    (f64::from(request.source_rgba8[source_offset]) * shade).round() as u8;
                output[output_offset + 1] =
                    (f64::from(request.source_rgba8[source_offset + 1]) * shade).round() as u8;
                output[output_offset + 2] =
                    (f64::from(request.source_rgba8[source_offset + 2]) * shade).round() as u8;
                output[output_offset + 3] = opacity.min(request.source_rgba8[source_offset + 3]);
            }
        }
        Ok(())
    }

    fn scaled_bounds(
        slot: &TemplateSlot,
        template: &TemplateDefinition,
        sheet: PixelSize,
    ) -> PixelBounds {
        let scale = |value: u32, canonical: u32, output: u32| {
            (f64::from(value) * f64::from(output) / f64::from(canonical)).round() as u32
        };
        let x = scale(slot.allocation.x, template.canonical_width, sheet.width);
        let y = scale(slot.allocation.y, template.canonical_height, sheet.height);
        let right = scale(
            slot.allocation.x + slot.allocation.width,
            template.canonical_width,
            sheet.width,
        );
        let bottom = scale(
            slot.allocation.y + slot.allocation.height,
            template.canonical_height,
            sheet.height,
        );
        PixelBounds {
            x,
            y,
            width: right - x,
            height: bottom - y,
        }
    }

    fn radial_coordinates(
        x: u32,
        y: u32,
        bounds: PixelBounds,
        center_x: f64,
        center_y: f64,
        inner: f64,
        outer: f64,
    ) -> (f64, f64, u8) {
        let nx = (f64::from(x) + 0.5) / f64::from(bounds.width) - center_x;
        let ny = (f64::from(y) + 0.5) / f64::from(bounds.height) - center_y;
        let radius = (nx * nx + ny * ny).sqrt();
        if radius < inner || radius > outer {
            return (0.0, 0.0, 0);
        }
        let angle = ny.atan2(nx) / std::f64::consts::TAU + 0.5;
        ((radius - inner) / (outer - inner), angle, 255)
    }

    fn vary_coordinates(u: f64, v: f64, slot_index: usize) -> (f64, f64) {
        match slot_index % 4 {
            0 => (u, v),
            1 => (1.0 - u, v),
            2 => (v, 1.0 - u),
            _ => (1.0 - v, u),
        }
    }

    fn fractional_index(value: f64, edge: u32) -> u32 {
        let value = value.rem_euclid(1.0);
        ((value * f64::from(edge)).floor() as u32).min(edge - 1)
    }

    fn pixel_offset(width: u32, x: u32, y: u32) -> usize {
        ((y * width + x) * 4) as usize
    }

    fn pixel_bytes(width: u32, height: u32) -> Result<usize, SheetCompileError> {
        usize::try_from(u64::from(width) * u64::from(height) * 4)
            .map_err(|_| SheetCompileError::OutputTooLarge)
    }

    #[cfg(test)]
    mod tests {
        use hot_trimmer_domain::{
            NormalizedBounds, NormalizedScalar, RegionSourceLayer, SourceMapping, TemplateRegistry,
        };

        use super::*;

        fn definition() -> TemplateDefinition {
            TemplateRegistry::from_json(include_str!(
                "../../../assets/templates/generic_architecture/1.0.0/template.json"
            ))
            .expect("template registry")
            .get("ht.generic_architecture", "1.0.0")
            .expect("generic architecture")
            .clone()
        }

        fn request(framing: SourceFraming) -> SheetCompileRequest<'static> {
            let template = Box::leak(Box::new(definition()));
            let source = Box::leak(Box::new(vec![
                0, 10, 20, 255, 30, 40, 50, 255, 60, 70, 80, 255, 90, 100, 110, 255, 120, 130, 140,
                255, 150, 160, 170, 255, 180, 190, 200, 255, 210, 220, 230, 255,
            ]));
            SheetCompileRequest {
                source_rgba8: source,
                source_width: 4,
                source_height: 2,
                template,
                sheet_size: PixelSize {
                    width: 512,
                    height: 512,
                },
                normal_convention: NormalConvention::OpenGl,
                source_framing: framing,
            }
        }

        #[test]
        fn source_coordinates_are_continuous_across_template_boundaries() {
            let request = request(SourceFraming {
                mode: SourceFramingMode::Stretch,
                ..SourceFraming::default()
            });
            let before = framed_coordinates(0.499, 0.5, request);
            let after = framed_coordinates(0.501, 0.5, request);
            assert!(after.0 > before.0);
            assert!((after.0 - before.0) < 0.01);
        }

        #[test]
        fn cover_and_stretch_have_different_framing_for_a_wide_source() {
            let cover = framed_coordinates(0.1, 0.5, request(SourceFraming::default()));
            let stretch = framed_coordinates(
                0.1,
                0.5,
                request(SourceFraming {
                    mode: SourceFramingMode::Stretch,
                    ..SourceFraming::default()
                }),
            );
            assert_ne!(cover, stretch);
        }

        #[test]
        fn stretch_samples_inside_the_edited_source_crop() {
            let framing = SourceFraming {
                mode: SourceFramingMode::Stretch,
                crop_bounds: NormalizedBounds {
                    x: NormalizedScalar::new(0.25).unwrap(),
                    y: NormalizedScalar::new(0.2).unwrap(),
                    width: NormalizedScalar::new(0.5).unwrap(),
                    height: NormalizedScalar::new(0.6).unwrap(),
                },
                ..SourceFraming::default()
            };
            let top_left = framed_coordinates(0.0, 0.0, request(framing));
            let bottom_right = framed_coordinates(1.0, 1.0, request(framing));
            assert_eq!(top_left, (0.25, 0.2));
            assert_eq!(bottom_right, (0.75, 0.8));
        }

        #[test]
        fn compiles_pbr_and_exact_manifest_id_maps() {
            let request = request(SourceFraming {
                mode: SourceFramingMode::Repeat,
                ..SourceFraming::default()
            });
            let first_slot = request.template.slots.first().expect("first slot");
            let bounds = checked_scaled_bounds(first_slot, request.template, request.sheet_size)
                .expect("scaled bounds");
            let expected = first_slot.id_color.0;
            let output = compile_template_sheet(request).expect("compiled sheet");
            let expected_bytes =
                usize::try_from(u64::from(output.width) * u64::from(output.height) * 4)
                    .expect("bounded bytes");
            for map in [
                &output.rgba8,
                &output.height_rgba8,
                &output.normal_rgba8,
                &output.roughness_rgba8,
                &output.metallic_rgba8,
                &output.ambient_occlusion_rgba8,
                &output.region_id_rgba8,
                &output.material_id_rgba8,
            ] {
                assert_eq!(map.len(), expected_bytes);
            }
            assert!(
                output
                    .metallic_rgba8
                    .chunks_exact(4)
                    .all(|pixel| pixel[..3] == [0, 0, 0])
            );
            let offset = pixel_offset(
                output.width,
                bounds.x + bounds.width / 2,
                bounds.y + bounds.height / 2,
            );
            assert_eq!(&output.region_id_rgba8[offset..offset + 3], &expected);
        }

        #[test]
        fn selected_region_source_layer_changes_compiled_pixels_deterministically() {
            let request = request(SourceFraming {
                mode: SourceFramingMode::Stretch,
                ..SourceFraming::default()
            });
            let slot = request.template.slots.first().expect("first slot");
            let bounds =
                checked_scaled_bounds(slot, request.template, request.sheet_size).expect("bounds");
            let layer = RegionSourceLayer {
                mapping: SourceMapping::Bounds {
                    bounds: NormalizedBounds {
                        x: NormalizedScalar::new(0.0).unwrap(),
                        y: NormalizedScalar::new(0.0).unwrap(),
                        width: NormalizedScalar::new(0.25).unwrap(),
                        height: NormalizedScalar::new(1.0).unwrap(),
                    },
                },
                ..RegionSourceLayer::default()
            };
            let cancellation = RenderCancellationToken::new();
            let bindings = [RegionSourceLayerBinding {
                slot_key: &slot.slot_key,
                layer: &layer,
            }];
            let overridden =
                compile_template_sheet_with_source_layers(request, &bindings, &cancellation)
                    .expect("overridden sheet");
            let repeated =
                compile_template_sheet_with_source_layers(request, &bindings, &cancellation)
                    .expect("repeat overridden sheet");
            let baseline = compile_template_sheet(request).expect("baseline sheet");
            let offset = pixel_offset(
                overridden.width,
                bounds.x + bounds.width / 2,
                bounds.y + bounds.height / 2,
            );
            assert_ne!(
                &overridden.rgba8[offset..offset + 4],
                &baseline.rgba8[offset..offset + 4]
            );
            assert_eq!(overridden, repeated);
        }

        #[test]
        fn invalid_or_cancelled_region_layer_publishes_no_sheet() {
            let request = request(SourceFraming::default());
            let slot = request.template.slots.first().expect("first slot");
            let mut invalid = RegionSourceLayer::default();
            invalid.rotation_degrees = f64::NAN;
            let binding = [RegionSourceLayerBinding {
                slot_key: &slot.slot_key,
                layer: &invalid,
            }];
            assert!(matches!(
                compile_template_sheet_with_source_layers(
                    request,
                    &binding,
                    &RenderCancellationToken::new()
                ),
                Err(SheetCompileError::RegionSourceMapping { .. })
            ));
            let cancellation = RenderCancellationToken::new();
            cancellation.cancel();
            assert!(matches!(
                compile_template_sheet_with_source_layers(request, &[], &cancellation),
                Err(SheetCompileError::Cancelled)
            ));
        }
    }
}
