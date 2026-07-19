use std::{
    io::Cursor,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};

use hot_trimmer_domain::{
    CancellationToken, ContentDigest, DocumentHash, EdgeEligibility, MaterialChannelRole,
    MaterialMapKind, NormalizedBounds, NormalizedPoint, NormalizedScalar, OrientedPixelSize,
    PixelBounds, PixelSize, Projection, QuarterTurn, RadialMappingSettings, RegionBehavior,
    RegionContinuity, RegionId, RegionSampling, SamplingMode, SamplingPolicy, SourceCropIntent,
    SourceId, SourceSamplingMode, SourceSetId, TemplateSlotRole, TrimSheetDocumentCommand,
};
use hot_trimmer_image_io::{ImagePlane, LinearColor, ResolvedAlphaMode};
use hot_trimmer_material_synthesis::PreparedMaterialDomain;
use hot_trimmer_placement_solver::{
    CandidateDescriptors, CandidateFamily, CandidateRoute, CandidateTransform, CropCandidate,
    EligibilityEvidence, MirrorTransform, PositionStrategy, SamplingPlan, SliceGeometry,
    SourceCrop, StretchOverrideProvenance,
};
use hot_trimmer_project_store::{ProjectStore, SourceChannel, SourceInput, SourceOwnership};
use hot_trimmer_render_core::PreparedExemplarChannel;
use hot_trimmer_sheet_compiler::{
    AlgorithmCompiler, AtlasFinalAtlasOutput, AtlasPreparedSource, AtlasRenderExecutionInput,
    AtlasRenderExecutor, AtlasRenderExecutorOutput, CompiledAtlasPlanV1,
    CompiledAtlasPreviewProfile, CompiledColorSpacePolicy, CompiledNormalConvention,
    CompiledRegionCommandV1, CompiledSourceCommandV1, CompiledTileRequest,
    CompiledTileRequestKind, GpuAtlasRenderExecutor, GpuAtlasSourceTextureCache, OutputPixelRect,
    PersistedStage14PreviewRequest, SlotSynthesisLimits, SlotSynthesisRequest, SourceFramePreviewCache,
    SourceFramePreviewProfile, SourcePixelRect, atlas_cpu_execution_counters,
    clear_atlas_cpu_execution_counters, synthesize_slot_material_with_guard,
};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use uuid::Uuid;

static GPU_TEST_SERVICE: OnceLock<hot_trimmer_preview::GpuCapabilityService> = OnceLock::new();
static GPU_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn gpu_test_service() -> &'static hot_trimmer_preview::GpuCapabilityService {
    GPU_TEST_SERVICE.get_or_init(hot_trimmer_preview::GpuCapabilityService::default)
}

fn linear_to_srgb(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let encoded = if value <= 0.003_130_8 {
        12.92 * value
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (encoded.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn srgb_to_linear(value: u8) -> f32 {
    let value = f32::from(value) / 255.0;
    if value <= 0.040_45 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn output_pixel(output: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let offset = ((y * width + x) * 4) as usize;
    [
        output[offset],
        output[offset + 1],
        output[offset + 2],
        output[offset + 3],
    ]
}

fn expected_domain_pixel(x: u32, y: u32) -> [u8; 4] {
    expected_domain_pixel_for(4, 4, x, y, 255)
}

fn expected_domain_pixel_for(width: u32, height: u32, x: u32, y: u32, alpha: u8) -> [u8; 4] {
    let x_denominator = width.saturating_sub(1).max(1) as f32;
    let y_denominator = height.saturating_sub(1).max(1) as f32;
    let xy_denominator = (width + height).saturating_sub(2).max(1) as f32;
    [
        linear_to_srgb(x as f32 / x_denominator),
        linear_to_srgb(y as f32 / y_denominator),
        linear_to_srgb((x + y) as f32 / xy_denominator),
        alpha,
    ]
}

fn normalized_bounds(x: f64, y: f64, width: f64, height: f64) -> NormalizedBounds {
    NormalizedBounds {
        x: NormalizedScalar::new(x).unwrap(),
        y: NormalizedScalar::new(y).unwrap(),
        width: NormalizedScalar::new(width).unwrap(),
        height: NormalizedScalar::new(height).unwrap(),
    }
}

fn linear_rgba8(pixel: LinearColor) -> [u8; 4] {
    [
        linear_to_srgb(pixel.rgb[0]),
        linear_to_srgb(pixel.rgb[1]),
        linear_to_srgb(pixel.rgb[2]),
        (pixel.alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

fn sample_bounds<T>(plane: &ImagePlane<T>, at: [f32; 2]) -> (u32, u32, u32, u32, f32, f32) {
    let x = (at[0] - 0.5).clamp(0.0, (plane.width() - 1) as f32);
    let y = (at[1] - 0.5).clamp(0.0, (plane.height() - 1) as f32);
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    (
        x0,
        y0,
        (x0 + 1).min(plane.width() - 1),
        (y0 + 1).min(plane.height() - 1),
        x - x.floor(),
        y - y.floor(),
    )
}

fn sample_f32<T: Copy>(
    plane: &ImagePlane<T>,
    at: [f32; 2],
    linear: bool,
    value: impl Fn(&T) -> f32,
) -> f32 {
    let (x0, y0, x1, y1, tx, ty) = sample_bounds(plane, at);
    if !linear {
        return value(plane.pixel(if tx < 0.5 { x0 } else { x1 }, if ty < 0.5 { y0 } else { y1 }));
    }
    let a = value(plane.pixel(x0, y0)) * (1.0 - tx) + value(plane.pixel(x1, y0)) * tx;
    let b = value(plane.pixel(x0, y1)) * (1.0 - tx) + value(plane.pixel(x1, y1)) * tx;
    a * (1.0 - ty) + b * ty
}

fn sample_linear_color(
    plane: &ImagePlane<LinearColor>,
    at: [f32; 2],
    linear: bool,
) -> LinearColor {
    LinearColor {
        rgb: std::array::from_fn(|index| sample_f32(plane, at, linear, |pixel| pixel.rgb[index])),
        alpha: sample_f32(plane, at, linear, |pixel| pixel.alpha),
    }
}

fn expected_domain_sample(domain: &PreparedMaterialDomain, at: [f32; 2], linear: bool) -> [u8; 4] {
    let PreparedExemplarChannel::BaseColor { plane, .. } = &domain.registered_channels()[0] else {
        panic!("test domain must publish Base Color");
    };
    linear_rgba8(sample_linear_color(plane, at, linear))
}

fn expected_encoded_gradient_sample(width: u32, height: u32, at: [f32; 2], linear: bool) -> [u8; 4] {
    let pixels = (0..height)
        .flat_map(|y| {
            (0..width).map(move |x| LinearColor {
                rgb: [
                    srgb_to_linear((x * 255 / width.saturating_sub(1).max(1)) as u8),
                    srgb_to_linear((y * 255 / height.saturating_sub(1).max(1)) as u8),
                    srgb_to_linear(
                        ((x + y) * 255 / (width + height).saturating_sub(2).max(1)) as u8,
                    ),
                ],
                alpha: 1.0,
            })
        })
        .collect::<Vec<_>>();
    let plane = ImagePlane::from_row_major(width, height, width.min(128).max(1), &pixels).unwrap();
    linear_rgba8(sample_linear_color(&plane, at, linear))
}

fn domain() -> Arc<PreparedMaterialDomain> {
    domain_with_size(b"gpu-stage-14-domain", b"gpu-stage-14-source", 4, 4, 255)
}

fn domain_with_size(
    domain_seed: &[u8],
    source_seed: &[u8],
    width: u32,
    height: u32,
    alpha: u8,
) -> Arc<PreparedMaterialDomain> {
    let pixels = (0..height)
        .flat_map(|y| {
            (0..width).map(move |x| LinearColor {
                rgb: [
                    x as f32 / (width.saturating_sub(1).max(1)) as f32,
                    y as f32 / (height.saturating_sub(1).max(1)) as f32,
                    (x + y) as f32 / (width + height).saturating_sub(2).max(1) as f32,
                ],
                alpha: f32::from(alpha) / 255.0,
            })
        })
        .collect::<Vec<_>>();
    let plane = ImagePlane::from_row_major(width, height, width.min(4).max(1), &pixels).unwrap();
    Arc::new(
        PreparedMaterialDomain::from_registered_channels(
            ContentDigest::sha256(domain_seed),
            ContentDigest::sha256(source_seed),
            vec![PreparedExemplarChannel::BaseColor {
                plane,
                alpha_mode: ResolvedAlphaMode::Opaque,
            }],
        )
        .unwrap(),
    )
}

fn solid_domain(seed: &[u8], rgba: [u8; 4]) -> Arc<PreparedMaterialDomain> {
    let color = LinearColor {
        rgb: [
            srgb_to_linear(rgba[0]),
            srgb_to_linear(rgba[1]),
            srgb_to_linear(rgba[2]),
        ],
        alpha: f32::from(rgba[3]) / 255.0,
    };
    let pixels = vec![color; 4];
    let plane = ImagePlane::from_row_major(2, 2, 2, &pixels).unwrap();
    Arc::new(
        PreparedMaterialDomain::from_registered_channels(
            ContentDigest::sha256(seed),
            ContentDigest::sha256(seed),
            vec![PreparedExemplarChannel::BaseColor {
                plane,
                alpha_mode: ResolvedAlphaMode::Opaque,
            }],
        )
        .unwrap(),
    )
}

fn sampling_plan(
    region_id: RegionId,
    source_id: ContentDigest,
    domain_id: ContentDigest,
    prepared_dimensions: [u32; 2],
    crop: SourceCrop,
    mode: SamplingMode,
) -> SamplingPlan {
    SamplingPlan {
        slot_id: region_id,
        role: TemplateSlotRole::Planar,
        variation_group: "gpu-stage-14".into(),
        prepared_domain_dimensions: prepared_dimensions,
        candidate: CropCandidate {
            candidate_id: ContentDigest::sha256(
                format!("candidate-{region_id}-{mode:?}").as_bytes(),
            ),
            source_id,
            domain_id: domain_id.clone(),
            slot_id: region_id,
            crop: Some(crop),
            transform: CandidateTransform {
                rotation: QuarterTurn::Zero,
                mirror: MirrorTransform::None,
            },
            isotropic_scale: 1.0,
            mapping_mode: mode,
            family: CandidateFamily::PanelDirect,
            route: CandidateRoute::Direct,
            position_strategy: PositionStrategy::DenseLowResolution,
            period_pixels: Some([2, 2]),
            seam_indices: Vec::new(),
            correspondence_reference: domain_id,
            descriptors: CandidateDescriptors {
                saliency_milli: 0,
                stationarity_milli: 1000,
                feature_strength_milli: 500,
                usability_milli: 1000,
            },
            seed: 2,
            eligibility: EligibilityEvidence {
                mapping_permitted: true,
                transform_permitted: true,
                isotropic_scale: true,
                exact_aspect: true,
                entire_crop_usable: Some(true),
                cross_axis_preserved: Some(true),
                lattice_aligned: Some(true),
                direct_crop_applicable: true,
                direct_crop_rejection: None,
                reasons: Vec::new(),
            },
        },
        slot_physical_size: [crop.width as f64, crop.height as f64],
        source_pixels_per_physical_unit: 1.0,
        sampling_policy: SamplingPolicy {
            filter: SourceSamplingMode::Nearest,
            scale: 1.0,
            correct_tangent_normals: true,
        },
        radial_mapping: None,
        stretch_override: StretchOverrideProvenance::NotAuthorized,
        slice_geometry: SliceGeometry::None,
        maximum_seam_cost_milli: 0,
        unary_cost: 0.0,
    }
}

fn plan(domain: &PreparedMaterialDomain) -> CompiledAtlasPlanV1 {
    let source_set_id = SourceSetId::from_bytes([7; 16]);
    let source_id = domain.prepared_source_digest.clone();
    let source = CompiledSourceCommandV1 {
        source_set_id,
        source_id: source_id.clone(),
        digest: domain.prepared_source_digest.clone(),
        oriented_dimensions: OrientedPixelSize {
            width: domain.width,
            height: domain.height,
        },
        decoder_version: "test-decoder".into(),
        decoded_format: "rgba8".into(),
        color_version: "test-color".into(),
        channel_role: MaterialChannelRole::BaseColor,
    };
    let make_region = |index: u32, id: RegionId, crop: SourceCrop, dst: PixelBounds, mode| {
        let sampling_plan =
            sampling_plan(id, source_id.clone(), domain.cache_key.clone(), [domain.width, domain.height], crop, mode);
        CompiledRegionCommandV1 {
            region_id: id,
            compact_index: index,
            source_set_id,
            source_id: source_id.clone(),
            patch_id: None,
            source_crop: SourcePixelRect(PixelBounds {
                x: crop.x,
                y: crop.y,
                width: crop.width,
                height: crop.height,
            }),
            destination_rect: OutputPixelRect(dst),
            sampling: if mode == SamplingMode::PeriodicTile {
                RegionSampling::LoopXy
            } else {
                RegionSampling::OneShot
            },
            source_to_region_transform: Default::default(),
            radial_parameters: None,
            continuity: RegionContinuity::None,
            padding_px: 0,
            edge_eligibility: EdgeEligibility::default(),
            sampling_plan,
            render_cache_key: ContentDigest::sha256(format!("render-{id}").as_bytes()),
        }
    };
    CompiledAtlasPlanV1 {
        schema_version: 1,
        algorithm_version: "gpu-stage-14-test".into(),
        document_revision: 1,
        request_generation: Some(1),
        topology_hash: DocumentHash([1; 32]),
        appearance_hash: DocumentHash([2; 32]),
        output_size: PixelSize {
            width: 4,
            height: 2,
        },
        preview_profile: CompiledAtlasPreviewProfile::Authoritative,
        normal_convention: CompiledNormalConvention::OpenGl,
        color_space_policy: CompiledColorSpacePolicy::SrgbColorUnassociatedAlpha,
        tile_request: CompiledTileRequest {
            kind: CompiledTileRequestKind::ExactViewport,
            generation: 1,
            output_rect: OutputPixelRect(PixelBounds {
                x: 0,
                y: 0,
                width: 4,
                height: 2,
            }),
            mip_level: 0,
            halo_px: 0,
            valid_rect: OutputPixelRect(PixelBounds {
                x: 0,
                y: 0,
                width: 4,
                height: 2,
            }),
        },
        requested_maps: vec![MaterialMapKind::BaseColor],
        ordered_sources: vec![source],
        ordered_regions: vec![
            make_region(
                0,
                RegionId::from_bytes([1; 16]),
                SourceCrop {
                    x: 0,
                    y: 0,
                    width: 2,
                    height: 2,
                },
                PixelBounds {
                    x: 0,
                    y: 0,
                    width: 2,
                    height: 2,
                },
                SamplingMode::DirectCrop,
            ),
            make_region(
                1,
                RegionId::from_bytes([2; 16]),
                SourceCrop {
                    x: 0,
                    y: 0,
                    width: 2,
                    height: 2,
                },
                PixelBounds {
                    x: 2,
                    y: 0,
                    width: 2,
                    height: 2,
                },
                SamplingMode::PeriodicTile,
            ),
        ],
        final_plan_hash: ContentDigest(String::new()),
    }
    .finalize()
    .unwrap()
}

fn execute_final_atlas(
    plan: &CompiledAtlasPlanV1,
    prepared_sources: Vec<AtlasPreparedSource>,
) -> AtlasFinalAtlasOutput {
    let cache = Mutex::new(GpuAtlasSourceTextureCache::default());
    execute_final_atlas_with_cache(plan, prepared_sources, &cache)
}

fn execute_final_atlas_with_cache(
    plan: &CompiledAtlasPlanV1,
    prepared_sources: Vec<AtlasPreparedSource>,
    cache: &Mutex<GpuAtlasSourceTextureCache>,
) -> AtlasFinalAtlasOutput {
    let _gpu_guard = GPU_TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("GPU focused tests must serialize shared service access");
    let executor = GpuAtlasRenderExecutor {
        service: gpu_test_service(),
        source_texture_cache: cache,
    };
    let input = AtlasRenderExecutionInput {
        prepared_sources,
        source_frame_cache: None,
    };
    let output = match executor.execute(plan, &input, &CancellationToken::new(), &|| true) {
        Ok(output) => output,
        Err(error) => panic!("{error}"),
    };
    let AtlasRenderExecutorOutput::FinalAtlas(output) = output else {
        panic!("supported GPU route must not return CPU region buffers");
    };
    output
}

fn with_gpu_executor<T>(
    cache: &Mutex<GpuAtlasSourceTextureCache>,
    run: impl FnOnce(&GpuAtlasRenderExecutor<'_>) -> T,
) -> T {
    let _gpu_guard = GPU_TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("GPU focused tests must serialize shared service access");
    let executor = GpuAtlasRenderExecutor {
        service: gpu_test_service(),
        source_texture_cache: cache,
    };
    run(&executor)
}

fn prepared_source(
    source: &CompiledSourceCommandV1,
    domain: Arc<PreparedMaterialDomain>,
) -> AtlasPreparedSource {
    AtlasPreparedSource {
        source_set_id: source.source_set_id,
        source_id: source.source_id.clone(),
        channel_role: MaterialChannelRole::BaseColor,
        domain,
    }
}

fn region_command(
    index: u32,
    region_id: RegionId,
    source: &CompiledSourceCommandV1,
    domain: &PreparedMaterialDomain,
    crop: SourceCrop,
    dst: PixelBounds,
    mode: SamplingMode,
    sampling: RegionSampling,
) -> CompiledRegionCommandV1 {
    let mut sampling_plan = sampling_plan(
        region_id,
        source.source_id.clone(),
        domain.cache_key.clone(),
        [domain.width, domain.height],
        crop,
        mode,
    );
    sampling_plan.slot_physical_size = [f64::from(dst.width), f64::from(dst.height)];
    CompiledRegionCommandV1 {
        region_id,
        compact_index: index,
        source_set_id: source.source_set_id,
        source_id: source.source_id.clone(),
        patch_id: None,
        source_crop: SourcePixelRect(PixelBounds {
            x: crop.x,
            y: crop.y,
            width: crop.width,
            height: crop.height,
        }),
        destination_rect: OutputPixelRect(dst),
        sampling,
        source_to_region_transform: hot_trimmer_domain::MappingTransform::default(),
        radial_parameters: None,
        continuity: RegionContinuity::None,
        padding_px: 0,
        edge_eligibility: EdgeEligibility::default(),
        sampling_plan,
        render_cache_key: ContentDigest::sha256(format!("render-{region_id}").as_bytes()),
    }
}

fn single_source_plan(
    domain: &PreparedMaterialDomain,
    output_size: PixelSize,
    regions: Vec<CompiledRegionCommandV1>,
) -> CompiledAtlasPlanV1 {
    let source_set_id = SourceSetId::from_bytes([7; 16]);
    let source = CompiledSourceCommandV1 {
        source_set_id,
        source_id: domain.prepared_source_digest.clone(),
        digest: domain.prepared_source_digest.clone(),
        oriented_dimensions: OrientedPixelSize {
            width: domain.width,
            height: domain.height,
        },
        decoder_version: "test-decoder".into(),
        decoded_format: "rgba8".into(),
        color_version: "test-color".into(),
        channel_role: MaterialChannelRole::BaseColor,
    };
    CompiledAtlasPlanV1 {
        schema_version: 1,
        algorithm_version: "gpu-stage-14-test".into(),
        document_revision: 1,
        request_generation: Some(1),
        topology_hash: DocumentHash([1; 32]),
        appearance_hash: DocumentHash([2; 32]),
        output_size,
        preview_profile: CompiledAtlasPreviewProfile::Authoritative,
        normal_convention: CompiledNormalConvention::OpenGl,
        color_space_policy: CompiledColorSpacePolicy::SrgbColorUnassociatedAlpha,
        tile_request: CompiledTileRequest {
            kind: CompiledTileRequestKind::ExactViewport,
            generation: 1,
            output_rect: OutputPixelRect(PixelBounds {
                x: 0,
                y: 0,
                width: output_size.width,
                height: output_size.height,
            }),
            mip_level: 0,
            halo_px: 0,
            valid_rect: OutputPixelRect(PixelBounds {
                x: 0,
                y: 0,
                width: output_size.width,
                height: output_size.height,
            }),
        },
        requested_maps: vec![MaterialMapKind::BaseColor],
        ordered_sources: vec![source],
        ordered_regions: regions,
        final_plan_hash: ContentDigest(String::new()),
    }
    .finalize()
    .unwrap()
}

fn cpu_expected_base_color(
    plan: &SamplingPlan,
    domain: &PreparedMaterialDomain,
    output_dimensions: [u32; 2],
) -> Vec<u8> {
    let synthesized = synthesize_slot_material_with_guard(
        SlotSynthesisRequest {
            plan,
            domain,
            output_dimensions,
            limits: SlotSynthesisLimits::default(),
        },
        &|| false,
    )
    .unwrap();
    let PreparedExemplarChannel::BaseColor { plane, .. } = &synthesized.channels[0] else {
        panic!("CPU oracle must produce Base Color");
    };
    plane.to_row_major()
        .iter()
        .flat_map(|pixel| {
            [
                linear_to_srgb(pixel.rgb[0]),
                linear_to_srgb(pixel.rgb[1]),
                linear_to_srgb(pixel.rgb[2]),
                (pixel.alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
            ]
        })
        .collect()
}

fn encoded_gradient_png(width: u32, height: u32) -> Vec<u8> {
    let mut image = RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            image.put_pixel(
                x,
                y,
                Rgba([
                    (x * 255 / width.saturating_sub(1).max(1)) as u8,
                    (y * 255 / height.saturating_sub(1).max(1)) as u8,
                    ((x + y) * 255 / (width + height).saturating_sub(2).max(1)) as u8,
                    255,
                ]),
            );
        }
    }
    let mut encoded = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut encoded, ImageFormat::Png)
        .expect("encode production route fixture");
    encoded.into_inner()
}

fn base_color_rgba8(
    artifact: &hot_trimmer_sheet_compiler::IntermediateAtlasArtifact,
) -> &[u8] {
    artifact
        .channels
        .iter()
        .find(|channel| channel.role == MaterialChannelRole::BaseColor)
        .expect("compiled artifact must publish Base Color")
        .rgba8
        .as_slice()
}

fn artifact_base_color_pixel(
    artifact: &hot_trimmer_sheet_compiler::IntermediateAtlasArtifact,
    x: u32,
    y: u32,
) -> [u8; 4] {
    output_pixel(
        base_color_rgba8(artifact),
        artifact.topology.output_size.width,
        x,
        y,
    )
}

#[test]
fn gpu_stage_14_base_color() {
    let domain = domain();
    let plan = plan(&domain);
    let cache = Mutex::new(GpuAtlasSourceTextureCache::default());
    let input = AtlasRenderExecutionInput {
        prepared_sources: vec![AtlasPreparedSource {
            source_set_id: plan.ordered_sources[0].source_set_id,
            source_id: plan.ordered_sources[0].source_id.clone(),
            channel_role: MaterialChannelRole::BaseColor,
            domain: Arc::clone(&domain),
        }],
        source_frame_cache: None,
    };
    let first = with_gpu_executor(&cache, |executor| {
        match executor.execute(&plan, &input, &CancellationToken::new(), &|| true) {
            Ok(output) => output,
            Err(error) => panic!("{error}"),
        }
    });
    let AtlasRenderExecutorOutput::FinalAtlas(first) = first else {
        panic!("supported GPU route must not return CPU region buffers");
    };
    assert_eq!(first.command_count, 2);
    assert_eq!(first.upload_bytes, 4 * 4 * 4);
    assert_eq!(first.base_color_rgba8.len(), 4 * 2 * 4);
    assert_eq!(output_pixel(&first.base_color_rgba8, 4, 0, 0), expected_domain_pixel(0, 0));
    assert_eq!(output_pixel(&first.base_color_rgba8, 4, 1, 0), expected_domain_pixel(1, 0));
    assert_eq!(output_pixel(&first.base_color_rgba8, 4, 0, 1), expected_domain_pixel(0, 1));
    assert_eq!(output_pixel(&first.base_color_rgba8, 4, 1, 1), expected_domain_pixel(1, 1));
    assert_eq!(output_pixel(&first.base_color_rgba8, 4, 2, 0), expected_domain_pixel(0, 0));
    assert_eq!(output_pixel(&first.base_color_rgba8, 4, 3, 0), expected_domain_pixel(1, 0));
    assert_eq!(output_pixel(&first.base_color_rgba8, 4, 2, 1), expected_domain_pixel(0, 1));
    assert_eq!(output_pixel(&first.base_color_rgba8, 4, 3, 1), expected_domain_pixel(1, 1));
    assert!(first.region_valid_pixel_counts.iter().any(|(region, count)| {
        *region == RegionId::from_bytes([1; 16]) && *count == 4
    }));
    assert!(
        first
            .telemetry
            .iter()
            .any(|line| line.contains("executor=gpu"))
    );

    let warm = with_gpu_executor(&cache, |executor| {
        executor
            .execute(&plan, &input, &CancellationToken::new(), &|| true)
            .unwrap()
    });
    let AtlasRenderExecutorOutput::FinalAtlas(warm) = warm else {
        panic!("warm supported GPU route must not return CPU region buffers");
    };
    assert_eq!(warm.upload_bytes, 0);
    assert!(warm.telemetry.iter().any(|line| line.contains("gpu_tile_cache=hit")));
}

#[test]
fn gpu_stage_14_base_color_repeat_transform_and_radial_pixels() {
    let domain = domain();
    let source = plan(&domain).ordered_sources[0].clone();
    let crop = SourceCrop {
        x: 0,
        y: 0,
        width: 2,
        height: 2,
    };
    let repeat_plan = single_source_plan(
        &domain,
        PixelSize {
            width: 6,
            height: 6,
        },
        vec![
            region_command(
                0,
                RegionId::from_bytes([3; 16]),
                &source,
                &domain,
                crop,
                PixelBounds {
                    x: 0,
                    y: 0,
                    width: 4,
                    height: 2,
                },
                SamplingMode::RepeatX,
                RegionSampling::LoopX,
            ),
            region_command(
                1,
                RegionId::from_bytes([4; 16]),
                &source,
                &domain,
                crop,
                PixelBounds {
                    x: 0,
                    y: 2,
                    width: 2,
                    height: 4,
                },
                SamplingMode::RepeatY,
                RegionSampling::LoopY,
            ),
            region_command(
                2,
                RegionId::from_bytes([5; 16]),
                &source,
                &domain,
                crop,
                PixelBounds {
                    x: 2,
                    y: 2,
                    width: 4,
                    height: 4,
                },
                SamplingMode::PeriodicTile,
                RegionSampling::LoopXy,
            ),
        ],
    );
    let repeat = execute_final_atlas(
        &repeat_plan,
        vec![prepared_source(&repeat_plan.ordered_sources[0], Arc::clone(&domain))],
    );
    assert_eq!(output_pixel(&repeat.base_color_rgba8, 6, 0, 0), expected_domain_pixel(1, 0));
    assert_eq!(output_pixel(&repeat.base_color_rgba8, 6, 1, 0), expected_domain_pixel(0, 0));
    assert_eq!(output_pixel(&repeat.base_color_rgba8, 6, 3, 1), expected_domain_pixel(0, 1));
    assert_eq!(output_pixel(&repeat.base_color_rgba8, 6, 0, 2), expected_domain_pixel(0, 1));
    assert_eq!(output_pixel(&repeat.base_color_rgba8, 6, 1, 3), expected_domain_pixel(1, 0));
    assert_eq!(output_pixel(&repeat.base_color_rgba8, 6, 2, 2), expected_domain_pixel(1, 1));
    assert_eq!(output_pixel(&repeat.base_color_rgba8, 6, 3, 3), expected_domain_pixel(0, 0));
    assert_eq!(output_pixel(&repeat.base_color_rgba8, 6, 5, 5), expected_domain_pixel(0, 0));

    let non_square_domain =
        domain_with_size(b"gpu-stage-14-non-square-domain", b"gpu-stage-14-non-square-source", 3, 2, 173);
    let non_square_source = plan(&non_square_domain).ordered_sources[0].clone();
    let non_square_region = region_command(
        0,
        RegionId::from_bytes([6; 16]),
        &non_square_source,
        &non_square_domain,
        SourceCrop {
            x: 0,
            y: 0,
            width: 3,
            height: 2,
        },
        PixelBounds {
            x: 0,
            y: 0,
            width: 3,
            height: 2,
        },
        SamplingMode::DirectCrop,
        RegionSampling::OneShot,
    );
    let non_square_plan = single_source_plan(
        &non_square_domain,
        PixelSize {
            width: 3,
            height: 2,
        },
        vec![non_square_region],
    );
    let non_square = execute_final_atlas(
        &non_square_plan,
        vec![prepared_source(
            &non_square_plan.ordered_sources[0],
            Arc::clone(&non_square_domain),
        )],
    );
    assert_eq!(
        output_pixel(&non_square.base_color_rgba8, 3, 2, 1),
        expected_domain_pixel_for(3, 2, 2, 1, 173)
    );

    let mut transform_region = region_command(
        0,
        RegionId::from_bytes([7; 16]),
        &source,
        &domain,
        SourceCrop {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        },
        PixelBounds {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        },
        SamplingMode::DirectCrop,
        RegionSampling::OneShot,
    );
    transform_region.sampling_plan.candidate.transform = CandidateTransform {
        rotation: QuarterTurn::Ninety,
        mirror: MirrorTransform::X,
    };
    transform_region.source_to_region_transform = hot_trimmer_domain::MappingTransform {
        scale: [1.75, 1.75],
        rotation_degrees: 180.0,
        mirror_x: true,
        mirror_y: true,
        offset: [0.0, 0.0],
    };
    let transform_plan = single_source_plan(
        &domain,
        PixelSize {
            width: 4,
            height: 4,
        },
        vec![transform_region],
    );
    let transformed = execute_final_atlas(
        &transform_plan,
        vec![prepared_source(&transform_plan.ordered_sources[0], Arc::clone(&domain))],
    );
    assert_eq!(
        transformed.base_color_rgba8,
        cpu_expected_base_color(&transform_plan.ordered_regions[0].sampling_plan, &domain, [4, 4]).into()
    );

    let offset_domain = domain_with_size(
        b"gpu-stage-14-offset-domain",
        b"gpu-stage-14-offset-source",
        5,
        5,
        255,
    );
    let offset_source = plan(&offset_domain).ordered_sources[0].clone();
    let mut baseline_offset_region = region_command(
        0,
        RegionId::from_bytes([17; 16]),
        &offset_source,
        &offset_domain,
        SourceCrop {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        },
        PixelBounds {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        },
        SamplingMode::DirectCrop,
        RegionSampling::OneShot,
    );
    baseline_offset_region.sampling_plan.sampling_policy.filter = SourceSamplingMode::Linear;
    let baseline_offset_plan = single_source_plan(
        &offset_domain,
        PixelSize {
            width: 4,
            height: 4,
        },
        vec![baseline_offset_region.clone()],
    );
    let baseline_offset = execute_final_atlas(
        &baseline_offset_plan,
        vec![prepared_source(
            &baseline_offset_plan.ordered_sources[0],
            Arc::clone(&offset_domain),
        )],
    );
    let mut fractional_offset_region = baseline_offset_region;
    fractional_offset_region.source_to_region_transform.offset = [0.125, 0.0];
    let fractional_offset_plan = single_source_plan(
        &offset_domain,
        PixelSize {
            width: 4,
            height: 4,
        },
        vec![fractional_offset_region],
    );
    let fractional_offset = execute_final_atlas(
        &fractional_offset_plan,
        vec![prepared_source(
            &fractional_offset_plan.ordered_sources[0],
            Arc::clone(&offset_domain),
        )],
    );
    let shifted = output_pixel(&fractional_offset.base_color_rgba8, 4, 0, 0);
    assert_eq!(shifted, expected_domain_sample(&offset_domain, [1.0, 0.5], true));
    assert_ne!(
        shifted,
        output_pixel(&baseline_offset.base_color_rgba8, 4, 0, 0),
        "fractional authored offset must visibly move the sampled source position"
    );

    let mut edge_region = fractional_offset_plan.ordered_regions[0].clone();
    edge_region.source_to_region_transform.offset = [0.5, 0.0];
    edge_region.render_cache_key = ContentDigest::sha256(b"render-edge-offset");
    let edge_plan = single_source_plan(
        &offset_domain,
        PixelSize {
            width: 4,
            height: 4,
        },
        vec![edge_region],
    );
    let edge_offset = execute_final_atlas(
        &edge_plan,
        vec![prepared_source(
            &edge_plan.ordered_sources[0],
            Arc::clone(&offset_domain),
        )],
    );
    assert_eq!(
        output_pixel(&edge_offset.base_color_rgba8, 4, 3, 0),
        [0, 0, 0, 0],
        "out-of-domain authored offset samples must invalidate instead of clamping"
    );

    let radial = RadialMappingSettings {
        center_x: 0.5,
        center_y: 0.5,
        inner_radius: 0.0,
        outer_radius: 1.0,
        falloff: 1.0,
        blend_width: 0.0,
        seam_blend_width: 0.0,
    };
    let mut planar_region = region_command(
        0,
        RegionId::from_bytes([8; 16]),
        &source,
        &domain,
        SourceCrop {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        },
        PixelBounds {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        },
        SamplingMode::PlanarRadial,
        RegionSampling::OneShot,
    );
    planar_region.radial_parameters = Some(radial);
    planar_region.sampling_plan.radial_mapping = Some(radial);
    let planar_plan = single_source_plan(
        &domain,
        PixelSize {
            width: 4,
            height: 4,
        },
        vec![planar_region],
    );
    let planar = execute_final_atlas(
        &planar_plan,
        vec![prepared_source(&planar_plan.ordered_sources[0], Arc::clone(&domain))],
    );
    assert_eq!(
        planar.base_color_rgba8,
        cpu_expected_base_color(&planar_plan.ordered_regions[0].sampling_plan, &domain, [4, 4]).into()
    );

    let mut polar_region = region_command(
        0,
        RegionId::from_bytes([9; 16]),
        &source,
        &domain,
        SourceCrop {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        },
        PixelBounds {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        },
        SamplingMode::PolarRadial,
        RegionSampling::OneShot,
    );
    let masked_radial = RadialMappingSettings {
        outer_radius: 0.25,
        ..radial
    };
    polar_region.radial_parameters = Some(masked_radial);
    polar_region.sampling_plan.radial_mapping = Some(masked_radial);
    let polar_plan = single_source_plan(
        &domain,
        PixelSize {
            width: 4,
            height: 4,
        },
        vec![polar_region],
    );
    let polar = execute_final_atlas(
        &polar_plan,
        vec![prepared_source(&polar_plan.ordered_sources[0], Arc::clone(&domain))],
    );
    assert_eq!(output_pixel(&polar.base_color_rgba8, 4, 0, 0), [0, 0, 0, 0]);
    assert_ne!(output_pixel(&polar.base_color_rgba8, 4, 1, 1), [0, 0, 0, 0]);
    assert_eq!(
        polar
            .region_valid_pixel_counts
            .iter()
            .find_map(|(region, count)| (*region == RegionId::from_bytes([9; 16])).then_some(*count)),
        Some(4)
    );

    let mut no_seam_region = region_command(
        0,
        RegionId::from_bytes([10; 16]),
        &source,
        &domain,
        SourceCrop {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        },
        PixelBounds {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        },
        SamplingMode::PolarRadial,
        RegionSampling::OneShot,
    );
    no_seam_region.radial_parameters = Some(radial);
    no_seam_region.sampling_plan.radial_mapping = Some(radial);
    let no_seam_plan = single_source_plan(
        &domain,
        PixelSize {
            width: 4,
            height: 4,
        },
        vec![no_seam_region],
    );
    let no_seam = execute_final_atlas(
        &no_seam_plan,
        vec![prepared_source(&no_seam_plan.ordered_sources[0], Arc::clone(&domain))],
    );

    let mut seam_region = no_seam_plan.ordered_regions[0].clone();
    let seam_radial = RadialMappingSettings {
        outer_radius: 1.0,
        seam_blend_width: 0.25,
        ..radial
    };
    seam_region.radial_parameters = Some(seam_radial);
    seam_region.sampling_plan.radial_mapping = Some(seam_radial);
    let seam_plan = single_source_plan(
        &domain,
        PixelSize {
            width: 4,
            height: 4,
        },
        vec![seam_region],
    );
    let seam = execute_final_atlas(
        &seam_plan,
        vec![prepared_source(&seam_plan.ordered_sources[0], Arc::clone(&domain))],
    );
    assert_ne!(
        output_pixel(&seam.base_color_rgba8, 4, 3, 1),
        output_pixel(&no_seam.base_color_rgba8, 4, 3, 1)
    );
}

#[test]
fn gpu_stage_14_base_color_multiple_sources_and_cache_invalidation() {
    let first_domain = domain();
    let second_domain = solid_domain(b"gpu-stage-14-second-source", [201, 17, 93, 128]);
    let mut plan = plan(&first_domain);
    let second_source_set_id = SourceSetId::from_bytes([8; 16]);
    let second_source = CompiledSourceCommandV1 {
        source_set_id: second_source_set_id,
        source_id: second_domain.prepared_source_digest.clone(),
        digest: second_domain.prepared_source_digest.clone(),
        oriented_dimensions: OrientedPixelSize {
            width: 2,
            height: 2,
        },
        decoder_version: "test-decoder".into(),
        decoded_format: "rgba8".into(),
        color_version: "test-color".into(),
        channel_role: MaterialChannelRole::BaseColor,
    };
    plan.ordered_sources.push(second_source);
    plan.ordered_regions[1].source_set_id = second_source_set_id;
    plan.ordered_regions[1].source_id = second_domain.prepared_source_digest.clone();
    plan.ordered_regions[1].source_crop = SourcePixelRect(PixelBounds {
        x: 0,
        y: 0,
        width: 2,
        height: 2,
    });
    plan.ordered_regions[1].sampling = RegionSampling::OneShot;
    plan.ordered_regions[1].sampling_plan = sampling_plan(
        plan.ordered_regions[1].region_id,
        second_domain.prepared_source_digest.clone(),
        second_domain.cache_key.clone(),
        [second_domain.width, second_domain.height],
        SourceCrop {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        },
        SamplingMode::DirectCrop,
    );
    plan.final_plan_hash = ContentDigest(String::new());
    plan = plan.finalize().unwrap();

    let cache = Mutex::new(GpuAtlasSourceTextureCache::default());
    let input = AtlasRenderExecutionInput {
        prepared_sources: vec![
            AtlasPreparedSource {
                source_set_id: plan.ordered_sources[0].source_set_id,
                source_id: plan.ordered_sources[0].source_id.clone(),
                channel_role: MaterialChannelRole::BaseColor,
                domain: Arc::clone(&first_domain),
            },
            AtlasPreparedSource {
                source_set_id: second_source_set_id,
                source_id: second_domain.prepared_source_digest.clone(),
                channel_role: MaterialChannelRole::BaseColor,
                domain: Arc::clone(&second_domain),
            },
        ],
        source_frame_cache: None,
    };

    let first = with_gpu_executor(&cache, |executor| {
        executor
            .execute(&plan, &input, &CancellationToken::new(), &|| true)
            .unwrap()
    });
    let AtlasRenderExecutorOutput::FinalAtlas(first) = first else {
        panic!("GPU route must return a final atlas");
    };
    assert_eq!(output_pixel(&first.base_color_rgba8, 4, 0, 0), expected_domain_pixel(0, 0));
    assert_eq!(output_pixel(&first.base_color_rgba8, 4, 1, 1), expected_domain_pixel(1, 1));
    assert_eq!(output_pixel(&first.base_color_rgba8, 4, 2, 0), [201, 17, 93, 128]);
    assert_eq!(output_pixel(&first.base_color_rgba8, 4, 3, 1), [201, 17, 93, 128]);

    let warm = with_gpu_executor(&cache, |executor| {
        executor
            .execute(&plan, &input, &CancellationToken::new(), &|| true)
            .unwrap()
    });
    let AtlasRenderExecutorOutput::FinalAtlas(warm) = warm else {
        panic!("warm GPU route must return a final atlas");
    };
    assert_eq!(warm.upload_bytes, 0);
    assert!(warm.telemetry.iter().any(|line| line.contains("gpu_tile_cache=hit")));

    let changed_domain = solid_domain(b"gpu-stage-14-second-source-mutated", [19, 211, 41, 255]);
    let mut changed_plan = plan.clone();
    changed_plan.ordered_sources[1].source_id = changed_domain.prepared_source_digest.clone();
    changed_plan.ordered_sources[1].digest = changed_domain.prepared_source_digest.clone();
    changed_plan.ordered_regions[1].source_id = changed_domain.prepared_source_digest.clone();
    changed_plan.ordered_regions[1].sampling_plan.candidate.source_id =
        changed_domain.prepared_source_digest.clone();
    changed_plan.ordered_regions[1].sampling_plan.candidate.domain_id =
        changed_domain.cache_key.clone();
    changed_plan.ordered_regions[1]
        .sampling_plan
        .candidate
        .correspondence_reference = changed_domain.cache_key.clone();
    changed_plan.final_plan_hash = ContentDigest(String::new());
    changed_plan = changed_plan.finalize().unwrap();
    let changed_input = AtlasRenderExecutionInput {
        prepared_sources: vec![
            AtlasPreparedSource {
                source_set_id: changed_plan.ordered_sources[0].source_set_id,
                source_id: changed_plan.ordered_sources[0].source_id.clone(),
                channel_role: MaterialChannelRole::BaseColor,
                domain: Arc::clone(&first_domain),
            },
            AtlasPreparedSource {
                source_set_id: second_source_set_id,
                source_id: changed_domain.prepared_source_digest.clone(),
                channel_role: MaterialChannelRole::BaseColor,
                domain: Arc::clone(&changed_domain),
            },
        ],
        source_frame_cache: None,
    };
    let changed = with_gpu_executor(&cache, |executor| {
        executor
            .execute(
                &changed_plan,
                &changed_input,
                &CancellationToken::new(),
                &|| true,
            )
            .unwrap()
    });
    let AtlasRenderExecutorOutput::FinalAtlas(changed) = changed else {
        panic!("changed GPU route must return a final atlas");
    };
    assert!(changed.upload_bytes > 0);
    assert_eq!(output_pixel(&changed.base_color_rgba8, 4, 2, 0), [19, 211, 41, 255]);
}

#[test]
fn gpu_stage_14_base_color_compile_persisted_route_counters_and_transform_parity() {
    let root = std::env::temp_dir().join(format!(
        "hot-trimmer-gpu-stage-14-route-{}",
        Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).expect("create production route fixture directory");
    let project_path = root.join("gpu-stage-14-route.hottrimmer");
    let mut store =
        ProjectStore::create(&project_path, "GPU Stage 14 Route").expect("create route project");
    let encoded = encoded_gradient_png(256, 256);
    let initial = store.summary().expect("initial route summary");
    let source_set_id = Uuid::from_bytes(initial.source_sets[0].id.to_bytes());
    store
        .replace_source_in_set(
            source_set_id,
            SourceChannel::BaseColor,
            &SourceInput {
                id: SourceId::new(),
                ownership: SourceOwnership::OwnedCopy,
                external_path: None,
                origin_path: PathBuf::from("gpu-stage-14-route.png"),
                sha256: ContentDigest::sha256(&encoded).0,
                width: 256,
                height: 256,
                format: "PNG".into(),
                color_type: "Rgba8".into(),
                has_alpha: true,
                exif_orientation: 1,
                has_embedded_icc_profile: false,
                encoded_bytes: encoded.len() as u64,
                owned_bytes: Some(encoded),
            },
        )
        .expect("register production route source");
    store
        .create_source_frame_document()
        .expect("create source-frame route document");
    store
        .execute_document_command(&TrimSheetDocumentCommand::SetOutputResolution {
            output_size: PixelSize {
                width: 256,
                height: 256,
            },
        })
        .expect("set route output resolution");

    let mut document = store.document().expect("route document").clone();
    let zero_offset_document = document.clone();
    let region_id = document.topology.regions[0].id;
    let binding = document
        .region_bindings
        .get_mut(&region_id)
        .expect("route region binding");
    binding.mapping.transform.offset = [0.1, 0.0];

    let compiler = AlgorithmCompiler::new();
    let mut zero_offset_project = store.summary().expect("zero-offset route project summary");
    zero_offset_project.document = Some(zero_offset_document.clone());
    let zero_offset_request = || PersistedStage14PreviewRequest {
        project: &zero_offset_project,
        revision: zero_offset_document.document_revision,
        draft_id: None,
        input_hash: None,
        profile: SourceFramePreviewProfile::Authoritative,
        view_intent: None,
    };
    let zero_offset_artifact = compiler
        .compile_persisted_stage_14_preview(zero_offset_request(), &CancellationToken::new(), || true)
        .expect("zero-offset CPU production route baseline");
    let mut project = store.summary().expect("route project summary");
    project.document = Some(document.clone());
    let request = || PersistedStage14PreviewRequest {
        project: &project,
        revision: document.document_revision,
        draft_id: None,
        input_hash: None,
        profile: SourceFramePreviewProfile::Authoritative,
        view_intent: None,
    };
    let cpu_artifact = compiler
        .compile_persisted_stage_14_preview(request(), &CancellationToken::new(), || true)
        .expect("CPU production route oracle");

    clear_atlas_cpu_execution_counters();
    let source_frame_cache = Mutex::new(SourceFramePreviewCache::default());
    let gpu_source_cache = Mutex::new(GpuAtlasSourceTextureCache::default());
    let gpu_artifact = with_gpu_executor(&gpu_source_cache, |gpu_executor| {
        compiler
            .compile_persisted_stage_14_preview_with_cache_and_executor(
                request(),
                &CancellationToken::new(),
                || true,
                Some(&source_frame_cache),
                Some(gpu_executor),
            )
            .expect("GPU production route")
    });
    assert_eq!(
        atlas_cpu_execution_counters(),
        hot_trimmer_sheet_compiler::AtlasCpuExecutionCounters {
            stage14_calls: 0,
            atlas_composition_calls: 0,
        }
    );
    assert_eq!(base_color_rgba8(&gpu_artifact), base_color_rgba8(&cpu_artifact));
    let slot = gpu_artifact
        .slots
        .iter()
        .find(|slot| slot.region_id == region_id)
        .expect("offset route artifact must publish selected slot metadata");
    let crop = slot
        .source_crop
        .expect("offset route artifact must retain selected source crop");
    let shifted_pixel =
        artifact_base_color_pixel(&gpu_artifact, slot.allocation.x, slot.allocation.y);
    let baseline_pixel =
        artifact_base_color_pixel(&zero_offset_artifact, slot.allocation.x, slot.allocation.y);
    assert_ne!(
        shifted_pixel, baseline_pixel,
        "production authored offset must change the first gradient sample"
    );
    assert_eq!(
        shifted_pixel,
        expected_encoded_gradient_sample(
            256,
            256,
            [
                crop.x as f32 + 0.5 + (0.1_f32 * crop.width as f32),
                crop.y as f32 + 0.5,
            ],
            true,
        )
    );
    assert!(gpu_artifact.telemetry.iter().any(|line| {
        line.contains("executor=gpu")
            && line.contains("cpu_stage14_calls=0")
            && line.contains("cpu_atlas_composition_calls=0")
    }));
}

#[test]
fn gpu_stage_14_base_color_source_frame_authored_crop_and_radial_preview_metadata() {
    let root = std::env::temp_dir().join(format!(
        "hot-trimmer-gpu-stage-14-source-frame-crop-{}",
        Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).expect("create source-frame crop fixture directory");
    let project_path = root.join("gpu-stage-14-source-frame-crop.hottrimmer");
    let mut store =
        ProjectStore::create(&project_path, "GPU Stage 14 SourceFrame Crop").expect("create project");
    let encoded = encoded_gradient_png(128, 128);
    let initial = store.summary().expect("initial crop summary");
    let source_set_id = Uuid::from_bytes(initial.source_sets[0].id.to_bytes());
    store
        .replace_source_in_set(
            source_set_id,
            SourceChannel::BaseColor,
            &SourceInput {
                id: SourceId::new(),
                ownership: SourceOwnership::OwnedCopy,
                external_path: None,
                origin_path: PathBuf::from("gpu-stage-14-source-frame-crop.png"),
                sha256: ContentDigest::sha256(&encoded).0,
                width: 128,
                height: 128,
                format: "PNG".into(),
                color_type: "Rgba8".into(),
                has_alpha: true,
                exif_orientation: 1,
                has_embedded_icc_profile: false,
                encoded_bytes: encoded.len() as u64,
                owned_bytes: Some(encoded),
            },
        )
        .expect("register crop source");
    store
        .create_source_frame_document()
        .expect("create source-frame crop document");
    store
        .execute_document_command(&TrimSheetDocumentCommand::SetOutputResolution {
            output_size: PixelSize {
                width: 128,
                height: 128,
            },
        })
        .expect("set crop output resolution");

    let mut document = store.document().expect("crop document").clone();
    let region_id = document.topology.regions[0].id;
    let authored_bounds = normalized_bounds(0.25, 0.125, 0.5, 0.5);
    let binding = document
        .region_bindings
        .get_mut(&region_id)
        .expect("crop region binding");
    binding.mapping.projection = Projection::Crop {
        bounds: authored_bounds,
        focus: NormalizedPoint::new(0.5, 0.375).unwrap(),
    };
    binding.mapping.source_crop_intent = Some(SourceCropIntent::Authored);
    let radial_settings = RadialMappingSettings {
        center_x: 0.5,
        center_y: 0.5,
        inner_radius: 0.05,
        outer_radius: 0.5,
        falloff: 1.0,
        blend_width: 0.0,
        seam_blend_width: 0.0,
    };
    let mut radial_behavior = RegionBehavior::new(hot_trimmer_domain::ManualRegionRole::Radial);
    radial_behavior.radial = Some(radial_settings);
    radial_behavior.synchronize_derived_fields();
    binding.mapping.radial = Some(radial_settings);
    binding.mapping.behavior = radial_behavior;

    let mut project = store.summary().expect("crop project summary");
    project.document = Some(document.clone());
    let compiler = AlgorithmCompiler::new();
    let source_frame_cache = Mutex::new(SourceFramePreviewCache::default());
    let gpu_source_cache = Mutex::new(GpuAtlasSourceTextureCache::default());
    let artifact = with_gpu_executor(&gpu_source_cache, |gpu_executor| {
        compiler
            .compile_persisted_stage_14_preview_with_cache_and_executor(
                PersistedStage14PreviewRequest {
                    project: &project,
                    revision: document.document_revision,
                    draft_id: None,
                    input_hash: None,
                    profile: SourceFramePreviewProfile::Authoritative,
                    view_intent: None,
                },
                &CancellationToken::new(),
                || true,
                Some(&source_frame_cache),
                Some(gpu_executor),
            )
            .expect("GPU source-frame authored crop route")
    });
    let slot = artifact
        .slots
        .iter()
        .find(|slot| slot.region_id == region_id)
        .expect("authored crop slot");
    assert_eq!(
        slot.source_crop,
        Some(SourceCrop {
            x: 32,
            y: 16,
            width: 64,
            height: 64,
        }),
        "source-frame preview must consume transient/authored crop projection before falling back to partition bounds"
    );
    assert_eq!(slot.mapping_mode, SamplingMode::PolarRadial);
    assert_eq!(slot.executed_mode, SamplingMode::PolarRadial);
    assert!(artifact.telemetry.iter().any(|line| line.contains("executor=gpu")));
}

#[test]
fn gpu_stage_14_base_color_rejects_unsupported_mode() {
    let domain = domain();
    let mut plan = plan(&domain);
    plan.ordered_regions[0].sampling_plan.candidate.mapping_mode = SamplingMode::TextureSynthesis;
    let cache = Mutex::new(GpuAtlasSourceTextureCache::default());
    let input = AtlasRenderExecutionInput {
        prepared_sources: vec![AtlasPreparedSource {
            source_set_id: plan.ordered_sources[0].source_set_id,
            source_id: plan.ordered_sources[0].source_id.clone(),
            channel_role: MaterialChannelRole::BaseColor,
            domain,
        }],
        source_frame_cache: None,
    };
    let error = with_gpu_executor(&cache, |executor| {
        executor
            .execute(&plan, &input, &CancellationToken::new(), &|| true)
            .expect_err("unsupported GPU sampling mode must fail before dispatch")
    });
    assert!(error.to_string().contains("unsupported GPU sampling mode"));
}
