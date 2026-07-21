use std::{
    io::Cursor,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
    time::Instant,
};

use hot_trimmer_domain::{
    CancellationToken, ContentDigest, DocumentHash, EdgeEligibility, EdgeWearIntent,
    MaterialChannelRole, MaterialMapKind, NormalConvention, NormalizedBounds, NormalizedPoint, NormalizedScalar,
    OrientedPixelSize, PixelBounds, PixelSize, Projection, QuarterTurn, RadialMappingSettings,
    RegionBehavior, RegionContinuity, RegionId, RegionSampling, SamplingMode, SamplingPolicy,
    SourceCropIntent, SourceId, SourceSamplingMode, SourceSetId, StructuralProfile,
    TemplateSlotRole, TrimSheetDocumentCommand,
};
use hot_trimmer_image_io::{
    ImagePlane, LinearColor, LinearScalar, NormalAlphaPolicy, ResolvedAlphaMode, TangentNormal,
};
use hot_trimmer_material_synthesis::{DomainRoute, PreparedMaterialDomain};
use hot_trimmer_placement_solver::{
    CandidateDescriptors, CandidateFamily, CandidateRoute, CandidateTransform, CropCandidate,
    EligibilityEvidence, MirrorTransform, PositionStrategy, SamplingBasis, SamplingPlan,
    SliceCenterPolicy, SliceGeometry, SourceCrop, StretchOverrideProvenance,
};
use hot_trimmer_project_store::{ProjectStore, SourceChannel, SourceInput, SourceOwnership};
use hot_trimmer_render_core::PreparedExemplarChannel;
use hot_trimmer_sheet_compiler::{
    AlgorithmCompiler, AtlasFinalAtlasOutput, AtlasPreparedSource, AtlasRenderExecutionInput,
    AtlasRenderExecutor, AtlasRenderExecutorOutput, CompiledAtlasPlanV1,
    CompiledAtlasPreviewProfile, CompiledColorSpacePolicy, CompiledNormalConvention,
    CompiledRegionCommandV1, CompiledSourceCommandV1, CompiledTileRequest, CompiledTileRequestKind,
    GpuAtlasRenderExecutor, GpuAtlasSourceTextureCache, OutputPixelRect,
    PersistedStage14PreviewRequest, SlotSynthesisLimits, SlotSynthesisRequest,
    SourceFramePreviewCache, SourceFramePreviewProfile, SourceFramePreviewViewIntent,
    SourcePixelRect, atlas_cpu_execution_counters, clear_atlas_cpu_execution_counters,
    synthesize_slot_material_with_guard,
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

fn output_f32(output: &[u8], width: u32, x: u32, y: u32) -> f32 {
    let offset = ((y * width + x) * 4) as usize;
    f32::from_le_bytes(output[offset..offset + 4].try_into().expect("f32 pixel"))
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
        return value(plane.pixel(
            if tx < 0.5 { x0 } else { x1 },
            if ty < 0.5 { y0 } else { y1 },
        ));
    }
    let a = value(plane.pixel(x0, y0)) * (1.0 - tx) + value(plane.pixel(x1, y0)) * tx;
    let b = value(plane.pixel(x0, y1)) * (1.0 - tx) + value(plane.pixel(x1, y1)) * tx;
    a * (1.0 - ty) + b * ty
}

fn sample_linear_color(plane: &ImagePlane<LinearColor>, at: [f32; 2], linear: bool) -> LinearColor {
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

fn expected_encoded_gradient_sample(
    width: u32,
    height: u32,
    at: [f32; 2],
    linear: bool,
) -> [u8; 4] {
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

fn synthesis_domain() -> Arc<PreparedMaterialDomain> {
    let width = 4;
    let height = 4;
    let pixels = (0..height)
        .flat_map(|y| {
            (0..width).map(move |x| LinearColor {
                rgb: [x as f32 / 3.0, y as f32 / 3.0, (x + y) as f32 / 6.0],
                alpha: 1.0,
            })
        })
        .collect::<Vec<_>>();
    let plane = ImagePlane::from_row_major(width, height, 4, &pixels).unwrap();
    Arc::new(
        PreparedMaterialDomain::from_registered_channels_with_route_and_seams(
            ContentDigest::sha256(b"stage-14-synthesis-domain"),
            ContentDigest::sha256(b"stage-14-synthesis-source"),
            vec![PreparedExemplarChannel::BaseColor {
                plane,
                alpha_mode: ResolvedAlphaMode::Opaque,
            }],
            DomainRoute::TextureQuilting,
            Vec::new(),
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
        sampling_basis: hot_trimmer_placement_solver::SamplingBasis::SelectedCrop,
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
        let sampling_plan = sampling_plan(
            id,
            source_id.clone(),
            domain.cache_key.clone(),
            [domain.width, domain.height],
            crop,
            mode,
        );
        CompiledRegionCommandV1 {
            region_id: id,
            compact_index: index,
            region_role: hot_trimmer_domain::ManualRegionRole::Panel,
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
            structural_profile: StructuralProfile::Flat,
            compiled_profile: hot_trimmer_sheet_compiler::compile_profile_for_region(
                StructuralProfile::Flat,
                &sampling_plan,
                dst,
                &ContentDigest::sha256(format!("profile-{id}").as_bytes()),
            )
            .unwrap(),
            compiled_details: hot_trimmer_effect_compiler::empty_compiled_detail_set(),
            continuity: RegionContinuity::None,
            padding_px: 0,
            edge_eligibility: EdgeEligibility::default(),
            edge_detail: None,
            edge_wear: None,
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
        channel_role: source.channel_role,
        domain,
    }
}

fn material_domain() -> Arc<PreparedMaterialDomain> {
    let width = 4;
    let height = 4;
    let colors = (0..height)
        .flat_map(|y| {
            (0..width).map(move |x| LinearColor {
                rgb: [x as f32 / 3.0, y as f32 / 3.0, 0.0],
                alpha: 1.0,
            })
        })
        .collect::<Vec<_>>();
    let scalar = |value: f32| vec![LinearScalar(value); (width * height) as usize];
    Arc::new(
        PreparedMaterialDomain::from_registered_channels(
            ContentDigest::sha256(b"gpu-material-map-domain"),
            ContentDigest::sha256(b"gpu-material-map-source"),
            vec![
                PreparedExemplarChannel::BaseColor {
                    plane: ImagePlane::from_row_major(width, height, 4, &colors).unwrap(),
                    alpha_mode: ResolvedAlphaMode::Opaque,
                },
                PreparedExemplarChannel::Scalar {
                    role: MaterialChannelRole::Height,
                    plane: ImagePlane::from_row_major(width, height, 4, &scalar(0.75)).unwrap(),
                },
                PreparedExemplarChannel::Scalar {
                    role: MaterialChannelRole::Roughness,
                    plane: ImagePlane::from_row_major(width, height, 4, &scalar(0.25)).unwrap(),
                },
                PreparedExemplarChannel::Scalar {
                    role: MaterialChannelRole::AmbientOcclusion,
                    plane: ImagePlane::from_row_major(width, height, 4, &scalar(0.5)).unwrap(),
                },
                PreparedExemplarChannel::Scalar {
                    role: MaterialChannelRole::Metallic,
                    plane: ImagePlane::from_row_major(width, height, 4, &scalar(1.0)).unwrap(),
                },
            ],
        )
        .unwrap(),
    )
}

fn material_domain_with_transparent_outer_row() -> Arc<PreparedMaterialDomain> {
    let width = 4;
    let height = 4;
    let colors = (0..height)
        .flat_map(|y| {
            (0..width).map(move |_| LinearColor {
                rgb: [0.4, 0.6, 0.8],
                alpha: if y == height - 1 { 0.0 } else { 1.0 },
            })
        })
        .collect::<Vec<_>>();
    Arc::new(
        PreparedMaterialDomain::from_registered_channels(
            ContentDigest::sha256(b"gpu-radial-transparent-edge-domain"),
            ContentDigest::sha256(b"gpu-radial-transparent-edge-source"),
            vec![PreparedExemplarChannel::BaseColor {
                plane: ImagePlane::from_row_major(width, height, 4, &colors).unwrap(),
                alpha_mode: ResolvedAlphaMode::Straight,
            }],
        )
        .unwrap(),
    )
}

fn material_domain_with_authored_landmarks() -> Arc<PreparedMaterialDomain> {
    let width = 4;
    let height = 4;
    let colors = vec![
        LinearColor {
            rgb: [0.2, 0.3, 0.4],
            alpha: 1.0,
        };
        (width * height) as usize
    ];
    let scalars = |low: f32, high: f32| {
        (0..height)
            .flat_map(|y| {
                (0..width).map(move |x| LinearScalar(if (x + y) % 2 == 0 { low } else { high }))
            })
            .collect::<Vec<_>>()
    };
    let normals = (0..height)
        .flat_map(|y| {
            (0..width).map(move |x| {
                if x < 2 && y < 2 {
                    TangentNormal {
                        xyz: [0.6, 0.6, 0.529_150_25],
                        alpha: 1.0,
                    }
                } else {
                    TangentNormal {
                        xyz: [0.0, 0.0, 1.0],
                        alpha: 1.0,
                    }
                }
            })
        })
        .collect::<Vec<_>>();
    Arc::new(
        PreparedMaterialDomain::from_registered_channels(
            ContentDigest::sha256(b"gpu-material-map-landmark-domain"),
            ContentDigest::sha256(b"gpu-material-map-landmark-source"),
            vec![
                PreparedExemplarChannel::BaseColor {
                    plane: ImagePlane::from_row_major(width, height, 4, &colors).unwrap(),
                    alpha_mode: ResolvedAlphaMode::Opaque,
                },
                PreparedExemplarChannel::Normal {
                    plane: ImagePlane::from_row_major(width, height, 4, &normals).unwrap(),
                    source_convention: NormalConvention::OpenGl,
                    canonical_convention: NormalConvention::OpenGl,
                    alpha_policy: NormalAlphaPolicy::Ignore,
                },
                PreparedExemplarChannel::Scalar {
                    role: MaterialChannelRole::Height,
                    plane: ImagePlane::from_row_major(width, height, 4, &scalars(0.5, 0.5))
                        .unwrap(),
                },
                PreparedExemplarChannel::Scalar {
                    role: MaterialChannelRole::Roughness,
                    plane: ImagePlane::from_row_major(width, height, 4, &scalars(0.2, 0.8))
                        .unwrap(),
                },
                PreparedExemplarChannel::Scalar {
                    role: MaterialChannelRole::AmbientOcclusion,
                    plane: ImagePlane::from_row_major(width, height, 4, &scalars(0.35, 0.9))
                        .unwrap(),
                },
                PreparedExemplarChannel::Scalar {
                    role: MaterialChannelRole::Metallic,
                    plane: ImagePlane::from_row_major(width, height, 4, &scalars(0.1, 1.0))
                        .unwrap(),
                },
            ],
        )
        .unwrap(),
    )
}

fn material_source_record(
    domain: &PreparedMaterialDomain,
    source_set_id: SourceSetId,
    role: MaterialChannelRole,
) -> CompiledSourceCommandV1 {
    CompiledSourceCommandV1 {
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
        channel_role: role,
    }
}

fn material_map_plan(domain: &PreparedMaterialDomain, map: MaterialMapKind) -> CompiledAtlasPlanV1 {
    let source_set_id = SourceSetId::from_bytes([42; 16]);
    let base_source = material_source_record(domain, source_set_id, MaterialChannelRole::BaseColor);
    let mut region = region_command(
        0,
        RegionId::from_bytes([42; 16]),
        &base_source,
        domain,
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
    region.structural_profile = StructuralProfile::Flat;
    CompiledAtlasPlanV1 {
        schema_version: 1,
        algorithm_version: "gpu-material-map-test".into(),
        document_revision: 1,
        request_generation: Some(1),
        topology_hash: DocumentHash([1; 32]),
        appearance_hash: DocumentHash([2; 32]),
        output_size: PixelSize {
            width: 4,
            height: 4,
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
                height: 4,
            }),
            mip_level: 0,
            halo_px: 0,
            valid_rect: OutputPixelRect(PixelBounds {
                x: 0,
                y: 0,
                width: 4,
                height: 4,
            }),
        },
        requested_maps: vec![map],
        ordered_sources: domain
            .registered_channels()
            .iter()
            .map(PreparedExemplarChannel::role)
            .map(|role| material_source_record(domain, source_set_id, role))
            .collect(),
        ordered_regions: vec![region],
        final_plan_hash: ContentDigest(String::new()),
    }
    .finalize()
    .unwrap()
}

fn mvp_edge_wear_plan(domain: &PreparedMaterialDomain) -> CompiledAtlasPlanV1 {
    let mut plan = material_map_plan(domain, MaterialMapKind::BaseColor);
    plan.requested_maps = vec![
        MaterialMapKind::BaseColor,
        MaterialMapKind::EdgeMask,
        MaterialMapKind::Height,
        MaterialMapKind::Normal,
        MaterialMapKind::Roughness,
        MaterialMapKind::Metallic,
    ];
    let region = &mut plan.ordered_regions[0];
    region.sampling_plan.slot_physical_size = [0.016, 0.016];
    region.sampling_plan.source_pixels_per_physical_unit = 250.0;
    region.compiled_profile = hot_trimmer_sheet_compiler::compile_profile_for_region(
        StructuralProfile::Flat,
        &region.sampling_plan,
        region.destination_rect.0,
        &ContentDigest::sha256(b"mvp-edge-wear-gpu-profile"),
    )
    .unwrap();
    region.edge_wear = Some(EdgeWearIntent {
        enabled: true,
        target_region: None,
        coverage: 1.0,
        strength: 1.0,
        edge_width_m: 0.004,
        breakup_scale_m: 0.012,
        breakup_seed: 201_516,
        height_amplitude_m: -0.001,
        hue_shift_degrees: 0.0,
        saturation_multiplier: 1.0,
        value_offset: 0.3,
        roughness_offset: 0.25,
        exposed_metal_enabled: false,
        metallic_offset: 0.0,
    });
    region.render_cache_key = ContentDigest::sha256(b"mvp-edge-wear-gpu-region");
    plan.final_plan_hash = ContentDigest(String::new());
    plan.finalize().unwrap()
}

#[test]
fn mvp_edge_wear_executes_real_gpu_pixels_for_the_requested_maps() {
    let domain = material_domain();
    let baseline_plan = material_map_plan(&domain, MaterialMapKind::BaseColor);
    let baseline = execute_final_atlas(
        &baseline_plan,
        prepared_sources_for_plan(&baseline_plan, Arc::clone(&domain)),
    );
    let plan = mvp_edge_wear_plan(&domain);
    let output = execute_final_atlas(
        &plan,
        prepared_sources_for_plan(&plan, Arc::clone(&domain)),
    );

    for map in [
        MaterialMapKind::BaseColor,
        MaterialMapKind::EdgeMask,
        MaterialMapKind::Height,
        MaterialMapKind::Normal,
        MaterialMapKind::Roughness,
        MaterialMapKind::Metallic,
    ] {
        assert!(output.map_tiles.contains_key(&map), "missing real GPU {map:?} tile");
    }

    let edge_mask = output.map_tiles.get(&MaterialMapKind::EdgeMask).unwrap();
    let height = output.map_tiles.get(&MaterialMapKind::Height).unwrap();
    let roughness = output.map_tiles.get(&MaterialMapKind::Roughness).unwrap();
    let base_color = output.map_tiles.get(&MaterialMapKind::BaseColor).unwrap();
    let baseline_color = baseline.map_tiles.get(&MaterialMapKind::BaseColor).unwrap();
    let normal = output.map_tiles.get(&MaterialMapKind::Normal).unwrap();

    let edge_mask_value = output_f32(edge_mask.pixels(), 4, 0, 0);
    let center_mask_value = output_f32(edge_mask.pixels(), 4, 2, 2);
    assert!(edge_mask_value > center_mask_value + 0.1, "edge mask did not resolve an edge");
    assert!(
        output_f32(height.pixels(), 4, 0, 0) < output_f32(height.pixels(), 4, 2, 2),
        "negative Edge Wear height did not indent the edge",
    );
    assert!(
        output_f32(roughness.pixels(), 4, 0, 0) > output_f32(roughness.pixels(), 4, 2, 2),
        "Edge Wear roughness did not affect the edge",
    );
    assert_ne!(
        output_pixel(base_color.pixels(), 4, 0, 0),
        output_pixel(baseline_color.pixels(), 4, 0, 0),
        "Edge Wear did not modify Base Color",
    );
    assert!(
        normal.pixels().chunks_exact(4).any(|pixel| pixel[0].abs_diff(128) > 8 || pixel[1].abs_diff(128) > 8),
        "Normal was not derived from the worn final Height",
    );
    assert!(output.telemetry.iter().any(|line| {
        line.contains("requested_map=EdgeMask")
            && line.contains("logical_passes=hotspot-profile,edge-wear-mask,publish")
            && line.contains("executed_gpu_passes=material-r32float-publish")
    }));
}

fn material_map_modes_plan(domain: &PreparedMaterialDomain) -> CompiledAtlasPlanV1 {
    let source_set_id = SourceSetId::from_bytes([43; 16]);
    let base_source = material_source_record(domain, source_set_id, MaterialChannelRole::BaseColor);
    let modes = [
        (
            SamplingMode::DirectCrop,
            RegionSampling::OneShot,
            StructuralProfile::Flat,
            None,
        ),
        (
            SamplingMode::RepeatX,
            RegionSampling::LoopX,
            StructuralProfile::Bevel,
            None,
        ),
        (
            SamplingMode::RepeatY,
            RegionSampling::LoopY,
            StructuralProfile::Groove,
            None,
        ),
        (
            SamplingMode::PeriodicTile,
            RegionSampling::LoopXy,
            StructuralProfile::PanelFrame,
            None,
        ),
        (
            SamplingMode::PolarRadial,
            RegionSampling::OneShot,
            StructuralProfile::RadialDisc,
            Some(RadialMappingSettings {
                center_x: 0.5,
                center_y: 0.5,
                inner_radius: 0.0,
                outer_radius: 0.5,
                falloff: 1.0,
                blend_width: 0.05,
                seam_blend_width: 0.05,
            }),
        ),
    ];
    let ordered_regions = modes
        .into_iter()
        .enumerate()
        .map(|(index, (mode, sampling, profile, radial))| {
            let mut region = region_command(
                index as u32,
                RegionId::from_bytes([index as u8 + 50; 16]),
                &base_source,
                domain,
                SourceCrop {
                    x: 0,
                    y: 0,
                    width: 4,
                    height: 4,
                },
                PixelBounds {
                    x: (index as u32) * 4,
                    y: 0,
                    width: 4,
                    height: 4,
                },
                mode,
                sampling,
            );
            region.structural_profile = profile;
            region.region_role = match mode {
                SamplingMode::DirectCrop => hot_trimmer_domain::ManualRegionRole::Unique,
                SamplingMode::RepeatX => hot_trimmer_domain::ManualRegionRole::HorizontalStrip,
                SamplingMode::RepeatY => hot_trimmer_domain::ManualRegionRole::VerticalStrip,
                SamplingMode::PolarRadial | SamplingMode::PlanarRadial => {
                    hot_trimmer_domain::ManualRegionRole::Radial
                }
                _ => hot_trimmer_domain::ManualRegionRole::Panel,
            };
            region.continuity = match mode {
                SamplingMode::RepeatX => RegionContinuity::X,
                SamplingMode::RepeatY => RegionContinuity::Y,
                SamplingMode::PeriodicTile => RegionContinuity::Xy,
                _ => RegionContinuity::None,
            };
            region.edge_eligibility = EdgeEligibility::for_continuity(region.continuity);
            region.radial_parameters = radial;
            region.sampling_plan.radial_mapping = radial;
            region
        })
        .collect();
    CompiledAtlasPlanV1 {
        schema_version: 1,
        algorithm_version: "gpu-material-map-modes-test".into(),
        document_revision: 1,
        request_generation: Some(1),
        topology_hash: DocumentHash([3; 32]),
        appearance_hash: DocumentHash([4; 32]),
        output_size: PixelSize {
            width: 20,
            height: 4,
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
                width: 20,
                height: 4,
            }),
            mip_level: 0,
            halo_px: 0,
            valid_rect: OutputPixelRect(PixelBounds {
                x: 0,
                y: 0,
                width: 20,
                height: 4,
            }),
        },
        requested_maps: vec![
            MaterialMapKind::Height,
            MaterialMapKind::Normal,
            MaterialMapKind::Roughness,
            MaterialMapKind::AmbientOcclusion,
            MaterialMapKind::Metallic,
            MaterialMapKind::RegionId,
        ],
        ordered_sources: domain
            .registered_channels()
            .iter()
            .map(PreparedExemplarChannel::role)
            .map(|role| material_source_record(domain, source_set_id, role))
            .collect(),
        ordered_regions,
        final_plan_hash: ContentDigest(String::new()),
    }
    .finalize()
    .unwrap()
}

fn radial_base_color_plan(domain: &PreparedMaterialDomain) -> CompiledAtlasPlanV1 {
    let source_set_id = SourceSetId::from_bytes([44; 16]);
    let source = material_source_record(domain, source_set_id, MaterialChannelRole::BaseColor);
    let mut region = region_command(
        0,
        RegionId::from_bytes([77; 16]),
        &source,
        domain,
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
    region.region_role = hot_trimmer_domain::ManualRegionRole::Radial;
    region.structural_profile = StructuralProfile::RadialDisc;
    let radial = RadialMappingSettings {
        center_x: 0.5,
        center_y: 0.5,
        inner_radius: 0.0,
        outer_radius: 0.5,
        falloff: 1.0,
        blend_width: 0.0,
        seam_blend_width: 0.0,
    };
    region.radial_parameters = Some(radial);
    region.sampling_plan.radial_mapping = Some(radial);
    CompiledAtlasPlanV1 {
        schema_version: 1,
        algorithm_version: "gpu-radial-edge-extension-test".into(),
        document_revision: 1,
        request_generation: Some(1),
        topology_hash: DocumentHash([5; 32]),
        appearance_hash: DocumentHash([6; 32]),
        output_size: PixelSize {
            width: 4,
            height: 4,
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
                height: 4,
            }),
            mip_level: 0,
            halo_px: 0,
            valid_rect: OutputPixelRect(PixelBounds {
                x: 0,
                y: 0,
                width: 4,
                height: 4,
            }),
        },
        requested_maps: vec![MaterialMapKind::BaseColor],
        ordered_sources: vec![source],
        ordered_regions: vec![region],
        final_plan_hash: ContentDigest(String::new()),
    }
    .finalize()
    .unwrap()
}

fn prepared_sources_for_plan(
    plan: &CompiledAtlasPlanV1,
    domain: Arc<PreparedMaterialDomain>,
) -> Vec<AtlasPreparedSource> {
    plan.ordered_sources
        .iter()
        .map(|source| prepared_source(source, Arc::clone(&domain)))
        .collect()
}

#[test]
fn gpu_material_map_pipeline() {
    let domain = material_domain();
    let height_plan = material_map_plan(&domain, MaterialMapKind::Height);
    let height = execute_final_atlas(
        &height_plan,
        prepared_sources_for_plan(&height_plan, Arc::clone(&domain)),
    );
    assert_eq!(
        height.interactive_tile.manifest.map,
        MaterialMapKind::Height
    );
    assert_eq!(
        height.interactive_tile.manifest.pixel_format,
        hot_trimmer_sheet_compiler::CompiledTilePixelFormat::Rgba8UnormLinear
    );
    let height_typed = height
        .map_tiles
        .get(&MaterialMapKind::Height)
        .expect("Height request should retain its typed map tile");
    assert_eq!(
        height_typed.manifest.pixel_format,
        hot_trimmer_sheet_compiler::CompiledTilePixelFormat::R32Float
    );
    assert!((0.74..=0.76).contains(&output_f32(height_typed.pixels(), 4, 0, 0)));
    assert!(height.telemetry.iter().any(|line| {
        line.contains("requested_map=Height")
            && line.contains("executed_gpu_passes=material-r32float-publish")
            && line.contains("intermediate_cache=not-available")
    }));

    let normal_plan = material_map_plan(&domain, MaterialMapKind::Normal);
    let normal = execute_final_atlas(
        &normal_plan,
        prepared_sources_for_plan(&normal_plan, Arc::clone(&domain)),
    );
    assert_eq!(
        normal.interactive_tile.manifest.map,
        MaterialMapKind::Normal
    );
    assert!(
        normal
            .interactive_tile
            .pixels()
            .chunks_exact(4)
            .all(|pixel| pixel[2] >= 250),
        "flat material height should derive a flat final normal",
    );

    let prereq_cache = Mutex::new(GpuAtlasSourceTextureCache::default());
    let _height_prereq = execute_final_atlas_with_cache(
        &height_plan,
        prepared_sources_for_plan(&height_plan, Arc::clone(&domain)),
        &prereq_cache,
    );
    let mut normal_from_cached_height_plan = normal_plan.clone();
    normal_from_cached_height_plan.tile_request.generation = 2;
    normal_from_cached_height_plan.final_plan_hash = ContentDigest(String::new());
    normal_from_cached_height_plan = normal_from_cached_height_plan.finalize().unwrap();
    let normal_from_cached_height = execute_final_atlas_with_cache(
        &normal_from_cached_height_plan,
        prepared_sources_for_plan(&normal_from_cached_height_plan, Arc::clone(&domain)),
        &prereq_cache,
    );
    assert!(normal_from_cached_height.telemetry.iter().any(|line| {
        line.contains("requested_map=Normal")
            && line.contains(
                "executed_gpu_passes=height-r32float-gpu-resource-cache,normal-from-final-height",
            )
            && line.contains("intermediate_cache=final-height:persistent-gpu-resource-hit")
    }));

    let roughness_plan = material_map_plan(&domain, MaterialMapKind::Roughness);
    let roughness = execute_final_atlas(
        &roughness_plan,
        prepared_sources_for_plan(&roughness_plan, Arc::clone(&domain)),
    );
    assert_eq!(
        roughness.interactive_tile.manifest.pixel_format,
        hot_trimmer_sheet_compiler::CompiledTilePixelFormat::Rgba8UnormLinear
    );
    let roughness_typed = roughness
        .map_tiles
        .get(&MaterialMapKind::Roughness)
        .expect("Roughness request should retain its typed map tile");
    assert_eq!(
        roughness_typed.manifest.pixel_format,
        hot_trimmer_sheet_compiler::CompiledTilePixelFormat::R32Float
    );
    assert!((0.24..=0.26).contains(&output_f32(roughness_typed.pixels(), 4, 0, 0)));

    let ao_plan = material_map_plan(&domain, MaterialMapKind::AmbientOcclusion);
    let ao = execute_final_atlas(
        &ao_plan,
        prepared_sources_for_plan(&ao_plan, Arc::clone(&domain)),
    );
    assert_eq!(
        ao.interactive_tile.manifest.pixel_format,
        hot_trimmer_sheet_compiler::CompiledTilePixelFormat::Rgba8UnormLinear
    );
    let ao_typed = ao
        .map_tiles
        .get(&MaterialMapKind::AmbientOcclusion)
        .expect("AO request should retain its typed map tile");
    assert!((0.49..=0.51).contains(&output_f32(ao_typed.pixels(), 4, 0, 0)));

    let metallic_plan = material_map_plan(&domain, MaterialMapKind::Metallic);
    let metallic = execute_final_atlas(
        &metallic_plan,
        prepared_sources_for_plan(&metallic_plan, Arc::clone(&domain)),
    );
    assert_eq!(
        metallic.interactive_tile.manifest.pixel_format,
        hot_trimmer_sheet_compiler::CompiledTilePixelFormat::Rgba8UnormLinear
    );
    let metallic_typed = metallic
        .map_tiles
        .get(&MaterialMapKind::Metallic)
        .expect("Metallic request should retain its typed map tile");
    assert_eq!(output_f32(metallic_typed.pixels(), 4, 0, 0), 1.0);

    let region_id_plan = material_map_plan(&domain, MaterialMapKind::RegionId);
    let region_id = execute_final_atlas(
        &region_id_plan,
        prepared_sources_for_plan(&region_id_plan, Arc::clone(&domain)),
    );
    assert_eq!(
        region_id.interactive_tile.manifest.map,
        MaterialMapKind::RegionId
    );
    assert_eq!(
        region_id.interactive_tile.manifest.pixel_format,
        hot_trimmer_sheet_compiler::CompiledTilePixelFormat::Rgba8UnormLinear
    );
    let region_id_typed = region_id
        .map_tiles
        .get(&MaterialMapKind::RegionId)
        .expect("Region ID request should retain its typed map tile");
    assert_eq!(
        region_id_typed.manifest.pixel_format,
        hot_trimmer_sheet_compiler::CompiledTilePixelFormat::R32Uint
    );
    assert_eq!(
        u32::from_le_bytes(
            region_id_typed.pixels()[0..4]
                .try_into()
                .expect("compact index bytes")
        ),
        0
    );
    assert!(region_id.telemetry.iter().any(|line| {
        line.contains("requested_map=RegionId")
            && line.contains("executed_gpu_passes=compact-region-id-r32uint")
            && line.contains("pixel_format=R32Uint")
    }));

    let mut multi = height_plan.clone();
    multi.requested_maps = vec![MaterialMapKind::Height, MaterialMapKind::Normal];
    multi.final_plan_hash = ContentDigest(String::new());
    multi = multi.finalize().unwrap();
    let cache = Mutex::new(GpuAtlasSourceTextureCache::default());
    let result = with_gpu_executor(&cache, |executor| {
        executor.execute(
            &multi,
            &AtlasRenderExecutionInput {
                prepared_sources: prepared_sources_for_plan(&multi, Arc::clone(&domain)),
                source_frame_cache: None,
            },
            &CancellationToken::new(),
            &|| true,
        )
    });
    let multi_output = result
        .expect("multi-map GPU request should execute")
        .as_final_atlas()
        .expect("multi-map GPU request should publish final tiles")
        .clone();
    assert!(
        multi_output
            .map_tiles
            .contains_key(&MaterialMapKind::Height)
    );
    assert!(
        multi_output
            .map_tiles
            .contains_key(&MaterialMapKind::Normal)
    );
    assert_eq!(
        multi_output.interactive_tile.manifest.map,
        MaterialMapKind::Height
    );
    assert!(
        multi_output
            .intermediate_tiles
            .contains_key("normal.final-height")
    );
    assert!(multi_output.telemetry.iter().any(|line| {
        line.contains("dependency=Normal<-Height")
            && line.contains("intermediate_cache=final-height:live-gpu-hit")
            && line.contains("normal_publish=from-r32float-gpu-final-height")
    }));
    assert!(
        multi_output
            .telemetry
            .iter()
            .any(|line| line.contains("gpu_pass_timing=normal-from-final-height")),
        "real GPU work should report timestamp-query pass timing"
    );

    let mut cached_normal_plan = normal_plan.clone();
    cached_normal_plan.tile_request.generation = 2;
    cached_normal_plan.final_plan_hash = ContentDigest(String::new());
    cached_normal_plan = cached_normal_plan.finalize().unwrap();
    let cached_started = Instant::now();
    let cached_result = with_gpu_executor(&cache, |executor| {
        executor.execute(
            &cached_normal_plan,
            &AtlasRenderExecutionInput {
                prepared_sources: prepared_sources_for_plan(
                    &cached_normal_plan,
                    Arc::clone(&domain),
                ),
                source_frame_cache: None,
            },
            &CancellationToken::new(),
            &|| true,
        )
    });
    let cached_elapsed_ms = cached_started.elapsed().as_millis();
    let cached_output = cached_result
        .expect("cached Normal switch should execute")
        .as_final_atlas()
        .expect("cached Normal switch should publish a final tile")
        .clone();
    assert_eq!(cached_output.render_ms, 0);
    assert!(
        cached_elapsed_ms <= 50,
        "cached Normal switch should stay under 50 ms, observed {cached_elapsed_ms} ms"
    );
    assert!(cached_output.telemetry.iter().any(|line| {
        line.contains("requested_map=Normal")
            && line.contains("executed_gpu_passes=none")
            && line.contains("final_tile_cache=hit")
            && line.contains("readback_ms=0")
    }));
}

#[test]
fn gpu_material_map_imported_normal_landmark_and_cache_scope() {
    let domain = material_domain_with_authored_landmarks();
    let normal_plan = material_map_plan(&domain, MaterialMapKind::Normal);
    let normal = execute_final_atlas(
        &normal_plan,
        prepared_sources_for_plan(&normal_plan, Arc::clone(&domain)),
    );
    let open_gl_landmark = output_pixel(normal.interactive_tile.pixels(), 4, 0, 0);
    assert!(
        open_gl_landmark[0] > 180 && open_gl_landmark[1] > 180 && open_gl_landmark[2] < 230,
        "flat Height must preserve the unmistakable imported Normal landmark, got {open_gl_landmark:?}"
    );
    assert!(normal.telemetry.iter().any(|line| {
        line.contains(
            "executed_gpu_passes=height-r32float,authored-normal-sample,normal-from-final-height",
        )
    }));

    let mut direct_x_plan = normal_plan.clone();
    direct_x_plan.normal_convention = CompiledNormalConvention::DirectX;
    direct_x_plan.final_plan_hash = ContentDigest(String::new());
    direct_x_plan = direct_x_plan.finalize().unwrap();
    let direct_x = execute_final_atlas(
        &direct_x_plan,
        prepared_sources_for_plan(&direct_x_plan, Arc::clone(&domain)),
    );
    let direct_x_landmark = output_pixel(direct_x.interactive_tile.pixels(), 4, 0, 0);
    assert_eq!(direct_x_landmark[0], open_gl_landmark[0]);
    assert!(
        direct_x_landmark[1] < 80 && open_gl_landmark[1] > 180,
        "the OpenGL/DirectX Y convention must be applied exactly once at publication"
    );

    let modes_plan = material_map_modes_plan(&domain);
    let modes = execute_final_atlas(
        &modes_plan,
        prepared_sources_for_plan(&modes_plan, Arc::clone(&domain)),
    );
    let mapped_normals = modes
        .map_tiles
        .get(&MaterialMapKind::Normal)
        .expect("mapped Normal tile");
    for region in 0..5_u32 {
        let landmark_survived = (0..4_u32).any(|y| {
            (region * 4..region * 4 + 4).any(|x| {
                let pixel = output_pixel(mapped_normals.pixels(), 20, x, y);
                pixel[3] > 0
                    && (pixel[0].abs_diff(128) > 35 || pixel[1].abs_diff(128) > 35)
                    && pixel[2] < 245
            })
        });
        assert!(
            landmark_survived,
            "imported Normal landmark was lost in mapped region {region}"
        );
    }

    let base_hash = normal_plan
        .pixel_plan_hash(MaterialMapKind::BaseColor)
        .unwrap();
    let height_hash = normal_plan
        .pixel_plan_hash(MaterialMapKind::Height)
        .unwrap();
    let normal_hash = normal_plan
        .pixel_plan_hash(MaterialMapKind::Normal)
        .unwrap();
    let mut replaced_normal = normal_plan.clone();
    replaced_normal
        .ordered_sources
        .iter_mut()
        .find(|source| source.channel_role == MaterialChannelRole::Normal)
        .expect("Normal source record")
        .digest = ContentDigest::sha256(b"replacement-normal-only");
    assert_eq!(
        replaced_normal
            .pixel_plan_hash(MaterialMapKind::BaseColor)
            .unwrap(),
        base_hash,
        "replacing Normal must not invalidate Base Color pixels"
    );
    assert_eq!(
        replaced_normal
            .pixel_plan_hash(MaterialMapKind::Height)
            .unwrap(),
        height_hash,
        "replacing Normal must not invalidate final Height pixels"
    );
    assert_ne!(
        replaced_normal
            .pixel_plan_hash(MaterialMapKind::Normal)
            .unwrap(),
        normal_hash,
        "replacing Normal must invalidate Normal pixels"
    );
}

#[test]
fn gpu_material_maps_cover_direct_loop_and_radial_modes() {
    let domain = material_domain();
    let plan = material_map_modes_plan(&domain);
    let output = execute_final_atlas(&plan, prepared_sources_for_plan(&plan, Arc::clone(&domain)));
    for map in [
        MaterialMapKind::Height,
        MaterialMapKind::Normal,
        MaterialMapKind::Roughness,
        MaterialMapKind::AmbientOcclusion,
        MaterialMapKind::Metallic,
        MaterialMapKind::RegionId,
    ] {
        assert!(
            output.map_tiles.contains_key(&map),
            "multi-mode material fixture omitted {map:?}"
        );
        assert!(
            output.display_tiles.contains_key(&map),
            "multi-mode material fixture omitted display tile for {map:?}"
        );
    }

    let height = output.map_tiles.get(&MaterialMapKind::Height).unwrap();
    let roughness = output.map_tiles.get(&MaterialMapKind::Roughness).unwrap();
    let ao = output
        .map_tiles
        .get(&MaterialMapKind::AmbientOcclusion)
        .unwrap();
    let normal = output.map_tiles.get(&MaterialMapKind::Normal).unwrap();
    let region_id = output.map_tiles.get(&MaterialMapKind::RegionId).unwrap();
    let region_class = output
        .display_tiles
        .get(&MaterialMapKind::RegionId)
        .unwrap();
    let lookup = plan.compact_region_id_lookup();
    let expected_classes = lookup
        .iter()
        .map(|entry| entry.display_rgba8)
        .collect::<Vec<_>>();
    for index in 0..5_u32 {
        let x = index * 4 + 2;
        assert!(
            (0.0..=2.0).contains(&output_f32(height.pixels(), 20, x, 2)),
            "Height center for mapping fixture region {index} should be authored/structural"
        );
        assert!(
            (0.20..=0.30).contains(&output_f32(roughness.pixels(), 20, x, 2)),
            "Roughness center for mapping fixture region {index} should use authored scalar"
        );
        assert!(
            (0.45..=0.55).contains(&output_f32(ao.pixels(), 20, x, 2)),
            "AO center for mapping fixture region {index} should use authored scalar"
        );
        let normal_offset = ((2 * 20 + x) * 4) as usize;
        assert!(
            normal.pixels()[normal_offset + 3] > 0,
            "Normal center for mapping fixture region {index} should remain valid"
        );
        let id_offset = normal_offset;
        assert_eq!(
            u32::from_le_bytes(
                region_id.pixels()[id_offset..id_offset + 4]
                    .try_into()
                    .expect("compact region id")
            ),
            index,
            "Region ID center should resolve compact index for mapping fixture region {index}"
        );
        let classification = output_pixel(region_class.pixels(), 20, x, 2);
        assert!(
            classification
                .iter()
                .zip(expected_classes[index as usize])
                .all(|(actual, expected)| actual.abs_diff(expected) <= 1),
            "region classification palette mismatch for region {index}: {classification:?}"
        );
    }
    let radial_corner_height = output_f32(height.pixels(), 20, 16, 0);
    let radial_corner_normal = output_pixel(normal.pixels(), 20, 16, 0);
    assert!(
        radial_corner_height >= 0.0 && radial_corner_normal[3] > 0,
        "polar radial corners must receive ownership-constrained boundary extension: height={radial_corner_height}, normal={radial_corner_normal:?}"
    );
    assert_eq!(lookup[0].role, hot_trimmer_domain::ManualRegionRole::Unique);
    assert_eq!(lookup[1].continuity, RegionContinuity::X);
    assert_eq!(lookup[2].continuity, RegionContinuity::Y);
    assert_eq!(lookup[3].continuity, RegionContinuity::Xy);
    assert_eq!(lookup[4].role, hot_trimmer_domain::ManualRegionRole::Radial);
    assert_ne!(
        hot_trimmer_sheet_compiler::CompiledRegionClassification::Horizontal.display_rgba8(0),
        hot_trimmer_sheet_compiler::CompiledRegionClassification::Horizontal.display_rgba8(1),
        "different regions in the same semantic class need distinct display shades"
    );
}

#[test]
fn gpu_material_map_radial_extension_uses_opaque_interior_pixels() {
    let domain = material_domain_with_transparent_outer_row();
    let plan = radial_base_color_plan(&domain);
    let output = execute_final_atlas(&plan, prepared_sources_for_plan(&plan, Arc::clone(&domain)));
    let base_color = output
        .display_tiles
        .get(&MaterialMapKind::BaseColor)
        .expect("radial Base Color display tile");
    for (x, y) in [(0, 0), (3, 0), (0, 3), (3, 3)] {
        let pixel = output_pixel(base_color.pixels(), 4, x, y);
        assert_eq!(
            pixel[3], 255,
            "radial extension corner ({x}, {y}) must push the nearest opaque interior pixel, got {pixel:?}"
        );
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
        region_role: hot_trimmer_domain::ManualRegionRole::Panel,
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
        structural_profile: StructuralProfile::Flat,
        compiled_profile: hot_trimmer_sheet_compiler::compile_profile_for_region(
            StructuralProfile::Flat,
            &sampling_plan,
            dst,
            &ContentDigest::sha256(format!("profile-{region_id}").as_bytes()),
        )
        .unwrap(),
        compiled_details: hot_trimmer_effect_compiler::empty_compiled_detail_set(),
        continuity: RegionContinuity::None,
        padding_px: 0,
        edge_eligibility: EdgeEligibility::default(),
        edge_detail: None,
        edge_wear: None,
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
    plane
        .to_row_major()
        .iter()
        .zip(synthesized.validity.to_row_major())
        .flat_map(|(pixel, valid)| {
            if valid.0 < 0.5 {
                [0, 0, 0, 0]
            } else {
                [
                    linear_to_srgb(pixel.rgb[0]),
                    linear_to_srgb(pixel.rgb[1]),
                    linear_to_srgb(pixel.rgb[2]),
                    (pixel.alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
                ]
            }
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

fn encoded_channel_landmark_png(
    width: u32,
    height: u32,
    pixel: impl Fn(u32, u32) -> [u8; 4],
) -> Vec<u8> {
    let mut image = RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            image.put_pixel(x, y, Rgba(pixel(x, y)));
        }
    }
    let mut encoded = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut encoded, ImageFormat::Png)
        .expect("encode companion channel fixture");
    encoded.into_inner()
}

fn base_color_rgba8(artifact: &hot_trimmer_sheet_compiler::IntermediateAtlasArtifact) -> &[u8] {
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
    assert_eq!(first.upload_bytes, 4 * 4 * 8);
    assert_eq!(first.base_color_rgba8.len(), 4 * 2 * 4);
    assert_eq!(
        output_pixel(&first.base_color_rgba8, 4, 0, 0),
        expected_domain_pixel(0, 0)
    );
    assert_eq!(
        output_pixel(&first.base_color_rgba8, 4, 1, 0),
        expected_domain_pixel(1, 0)
    );
    assert_eq!(
        output_pixel(&first.base_color_rgba8, 4, 0, 1),
        expected_domain_pixel(0, 1)
    );
    assert_eq!(
        output_pixel(&first.base_color_rgba8, 4, 1, 1),
        expected_domain_pixel(1, 1)
    );
    assert_eq!(
        output_pixel(&first.base_color_rgba8, 4, 2, 0),
        expected_domain_pixel(0, 0)
    );
    assert_eq!(
        output_pixel(&first.base_color_rgba8, 4, 3, 0),
        expected_domain_pixel(1, 0)
    );
    assert_eq!(
        output_pixel(&first.base_color_rgba8, 4, 2, 1),
        expected_domain_pixel(0, 1)
    );
    assert_eq!(
        output_pixel(&first.base_color_rgba8, 4, 3, 1),
        expected_domain_pixel(1, 1)
    );
    assert!(
        first
            .region_valid_pixel_counts
            .iter()
            .any(|(region, count)| { *region == RegionId::from_bytes([1; 16]) && *count == 4 })
    );
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
    assert!(
        warm.telemetry
            .iter()
            .any(|line| line.contains("gpu_tile_cache=hit"))
    );
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
        vec![prepared_source(
            &repeat_plan.ordered_sources[0],
            Arc::clone(&domain),
        )],
    );
    assert_eq!(
        output_pixel(&repeat.base_color_rgba8, 6, 0, 0),
        expected_domain_pixel(1, 0)
    );
    assert_eq!(
        output_pixel(&repeat.base_color_rgba8, 6, 1, 0),
        expected_domain_pixel(0, 0)
    );
    assert_eq!(
        output_pixel(&repeat.base_color_rgba8, 6, 3, 1),
        expected_domain_pixel(0, 1)
    );
    assert_eq!(
        output_pixel(&repeat.base_color_rgba8, 6, 0, 2),
        expected_domain_pixel(0, 1)
    );
    assert_eq!(
        output_pixel(&repeat.base_color_rgba8, 6, 1, 3),
        expected_domain_pixel(1, 0)
    );
    assert_eq!(
        output_pixel(&repeat.base_color_rgba8, 6, 2, 2),
        expected_domain_pixel(1, 1)
    );
    assert_eq!(
        output_pixel(&repeat.base_color_rgba8, 6, 3, 3),
        expected_domain_pixel(0, 0)
    );
    assert_eq!(
        output_pixel(&repeat.base_color_rgba8, 6, 5, 5),
        expected_domain_pixel(0, 0)
    );

    let non_square_domain = domain_with_size(
        b"gpu-stage-14-non-square-domain",
        b"gpu-stage-14-non-square-source",
        3,
        2,
        173,
    );
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
        vec![prepared_source(
            &transform_plan.ordered_sources[0],
            Arc::clone(&domain),
        )],
    );
    assert_eq!(
        transformed.base_color_rgba8,
        cpu_expected_base_color(
            &transform_plan.ordered_regions[0].sampling_plan,
            &domain,
            [4, 4]
        )
        .into()
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
    assert_eq!(
        shifted,
        expected_domain_sample(&offset_domain, [1.0, 0.5], true)
    );
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
        vec![prepared_source(
            &planar_plan.ordered_sources[0],
            Arc::clone(&domain),
        )],
    );
    assert_eq!(
        planar.base_color_rgba8,
        cpu_expected_base_color(
            &planar_plan.ordered_regions[0].sampling_plan,
            &domain,
            [4, 4]
        )
        .into()
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
        vec![prepared_source(
            &polar_plan.ordered_sources[0],
            Arc::clone(&domain),
        )],
    );
    assert_ne!(output_pixel(&polar.base_color_rgba8, 4, 0, 0), [0, 0, 0, 0]);
    assert_ne!(output_pixel(&polar.base_color_rgba8, 4, 1, 1), [0, 0, 0, 0]);
    assert_eq!(
        polar
            .region_valid_pixel_counts
            .iter()
            .find_map(
                |(region, count)| (*region == RegionId::from_bytes([9; 16])).then_some(*count)
            ),
        Some(16)
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
        vec![prepared_source(
            &no_seam_plan.ordered_sources[0],
            Arc::clone(&domain),
        )],
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
        vec![prepared_source(
            &seam_plan.ordered_sources[0],
            Arc::clone(&domain),
        )],
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
    assert_eq!(
        output_pixel(&first.base_color_rgba8, 4, 0, 0),
        expected_domain_pixel(0, 0)
    );
    assert_eq!(
        output_pixel(&first.base_color_rgba8, 4, 1, 1),
        expected_domain_pixel(1, 1)
    );
    assert_eq!(
        output_pixel(&first.base_color_rgba8, 4, 2, 0),
        [201, 17, 93, 128]
    );
    assert_eq!(
        output_pixel(&first.base_color_rgba8, 4, 3, 1),
        [201, 17, 93, 128]
    );

    let warm = with_gpu_executor(&cache, |executor| {
        executor
            .execute(&plan, &input, &CancellationToken::new(), &|| true)
            .unwrap()
    });
    let AtlasRenderExecutorOutput::FinalAtlas(warm) = warm else {
        panic!("warm GPU route must return a final atlas");
    };
    assert_eq!(warm.upload_bytes, 0);
    assert!(
        warm.telemetry
            .iter()
            .any(|line| line.contains("gpu_tile_cache=hit"))
    );

    let changed_domain = solid_domain(b"gpu-stage-14-second-source-mutated", [19, 211, 41, 255]);
    let mut changed_plan = plan.clone();
    changed_plan.ordered_sources[1].source_id = changed_domain.prepared_source_digest.clone();
    changed_plan.ordered_sources[1].digest = changed_domain.prepared_source_digest.clone();
    changed_plan.ordered_regions[1].source_id = changed_domain.prepared_source_digest.clone();
    changed_plan.ordered_regions[1]
        .sampling_plan
        .candidate
        .source_id = changed_domain.prepared_source_digest.clone();
    changed_plan.ordered_regions[1]
        .sampling_plan
        .candidate
        .domain_id = changed_domain.cache_key.clone();
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
    assert_eq!(
        output_pixel(&changed.base_color_rgba8, 4, 2, 0),
        [19, 211, 41, 255]
    );
}

#[test]
fn gpu_stage_14_base_color_compile_persisted_route_counters_and_transform_parity() {
    let root =
        std::env::temp_dir().join(format!("hot-trimmer-gpu-stage-14-route-{}", Uuid::new_v4()));
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
    let companion_maps = [
        (
            SourceChannel::Normal,
            "gpu-stage-14-normal.png",
            encoded_channel_landmark_png(256, 256, |x, y| {
                if x < 96 && y < 96 {
                    [204, 204, 195, 255]
                } else {
                    [128, 128, 255, 255]
                }
            }),
        ),
        (
            SourceChannel::Height,
            "gpu-stage-14-height.png",
            encoded_channel_landmark_png(256, 256, |_, _| [128, 128, 128, 255]),
        ),
        (
            SourceChannel::Roughness,
            "gpu-stage-14-roughness.png",
            encoded_channel_landmark_png(256, 256, |x, y| {
                let value = if (x / 32 + y / 32) % 2 == 0 { 38 } else { 217 };
                [value, value, value, 255]
            }),
        ),
        (
            SourceChannel::Metallic,
            "gpu-stage-14-metallic.png",
            encoded_channel_landmark_png(256, 256, |x, _| {
                let value = if x < 128 { 20 } else { 235 };
                [value, value, value, 255]
            }),
        ),
        (
            SourceChannel::AmbientOcclusion,
            "gpu-stage-14-ao.png",
            encoded_channel_landmark_png(256, 256, |_, y| {
                let value = if y < 128 { 64 } else { 196 };
                [value, value, value, 255]
            }),
        ),
    ];
    for (channel, name, encoded) in companion_maps {
        let input = SourceInput {
            id: SourceId::new(),
            ownership: SourceOwnership::OwnedCopy,
            external_path: None,
            origin_path: PathBuf::from(name),
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
        };
        if channel == SourceChannel::Normal {
            let mut registration =
                hot_trimmer_domain::ChannelRegistration::explicit(MaterialChannelRole::Normal);
            registration.normal_convention = NormalConvention::OpenGl;
            store
                .replace_registered_source_in_set(source_set_id, &input, registration)
                .expect("register production OpenGL Normal companion channel");
        } else {
            store
                .replace_source_in_set(source_set_id, channel, &input)
                .expect("register production companion channel");
        }
    }
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
        .compile_persisted_stage_14_preview(
            zero_offset_request(),
            &CancellationToken::new(),
            || true,
        )
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
    let map_set_request = || PersistedStage14PreviewRequest {
        project: &project,
        revision: document.document_revision,
        draft_id: None,
        input_hash: None,
        profile: SourceFramePreviewProfile::Authoritative,
        view_intent: Some(SourceFramePreviewViewIntent::MaterialMaps(vec![
            MaterialMapKind::BaseColor,
            MaterialMapKind::Height,
            MaterialMapKind::Normal,
            MaterialMapKind::Roughness,
            MaterialMapKind::Metallic,
            MaterialMapKind::AmbientOcclusion,
            MaterialMapKind::RegionId,
        ])),
    };
    let gpu_map_set_artifact = with_gpu_executor(&gpu_source_cache, |gpu_executor| {
        compiler
            .compile_persisted_stage_14_preview_with_cache_and_executor(
                map_set_request(),
                &CancellationToken::new(),
                || true,
                Some(&source_frame_cache),
                Some(gpu_executor),
            )
            .expect("GPU production map-set route")
    });
    assert!(
        [
            MaterialMapKind::BaseColor,
            MaterialMapKind::Height,
            MaterialMapKind::Normal,
            MaterialMapKind::Roughness,
            MaterialMapKind::Metallic,
            MaterialMapKind::AmbientOcclusion,
            MaterialMapKind::RegionId,
        ]
        .into_iter()
        .all(|map| gpu_map_set_artifact.rendered_tiles.contains_key(&map))
    );
    let production_normal = gpu_map_set_artifact
        .rendered_tiles
        .get(&MaterialMapKind::Normal)
        .expect("production Normal tile");
    assert!(
        production_normal
            .pixels()
            .chunks_exact(4)
            .any(|pixel| { pixel[3] > 0 && pixel[0] > 180 && pixel[1] > 180 && pixel[2] < 230 }),
        "production Source Frame route must sample the imported tangent-space Normal landmark"
    );
    for (map, low, high) in [
        (MaterialMapKind::Roughness, 0.2_f32, 0.7_f32),
        (MaterialMapKind::Metallic, 0.12_f32, 0.8_f32),
        (MaterialMapKind::AmbientOcclusion, 0.3_f32, 0.7_f32),
    ] {
        let tile = gpu_map_set_artifact
            .rendered_tiles
            .get(&map)
            .expect("production scalar companion tile");
        let values = tile
            .pixels()
            .chunks_exact(4)
            .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
            .filter(|value| value.is_finite() && *value >= 0.0)
            .collect::<Vec<_>>();
        assert!(values.iter().any(|value| *value <= low));
        assert!(values.iter().any(|value| *value >= high));
    }
    assert_eq!(
        gpu_map_set_artifact
            .rendered_tiles
            .get(&MaterialMapKind::RegionId)
            .expect("Region ID rendered tile")
            .manifest
            .pixel_format,
        hot_trimmer_sheet_compiler::CompiledTilePixelFormat::R32Uint
    );
    assert!(
        gpu_map_set_artifact
            .unavailable_channels
            .iter()
            .all(|role| *role != MaterialChannelRole::RegionId)
    );
    assert_eq!(
        atlas_cpu_execution_counters(),
        hot_trimmer_sheet_compiler::AtlasCpuExecutionCounters {
            stage14_calls: 0,
            atlas_composition_calls: 0,
        }
    );
    assert_eq!(
        base_color_rgba8(&gpu_artifact),
        base_color_rgba8(&cpu_artifact)
    );
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
    let mut store = ProjectStore::create(&project_path, "GPU Stage 14 SourceFrame Crop")
        .expect("create project");
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
    assert!(
        artifact
            .telemetry
            .iter()
            .any(|line| line.contains("executor=gpu"))
    );
}

#[test]
fn gpu_stage_14_base_color_lowers_prepared_synthesis_nine_slice_and_unique_fit_modes() {
    let prepared_domain = synthesis_domain();
    let mut plan = material_map_plan(&prepared_domain, MaterialMapKind::BaseColor);
    let region = &mut plan.ordered_regions[0];
    region.sampling_plan.candidate.mapping_mode = SamplingMode::TextureSynthesis;
    region.sampling_plan.candidate.family = CandidateFamily::PanelQuiltedExpansion;
    region.sampling_plan.candidate.route = CandidateRoute::Synthesis;
    region.sampling_plan.candidate.crop = None;
    region.sampling_plan.sampling_basis = SamplingBasis::PreparedDomain {
        window: SourceCrop {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        },
    };
    plan.final_plan_hash = ContentDigest(String::new());
    let cache = Mutex::new(GpuAtlasSourceTextureCache::default());
    let synthesis = with_gpu_executor(&cache, |executor| {
        let input = AtlasRenderExecutionInput {
            prepared_sources: prepared_sources_for_plan(&plan, Arc::clone(&prepared_domain)),
            source_frame_cache: None,
        };
        let output = executor
            .execute(&plan, &input, &CancellationToken::new(), &|| true)
            .expect("prepared synthesis GPU lowering");
        let AtlasRenderExecutorOutput::FinalAtlas(output) = output else {
            panic!("prepared synthesis must remain on the GPU production route")
        };
        output
    });
    assert_eq!(
        output_pixel(&synthesis.base_color_rgba8, 4, 0, 0),
        expected_domain_pixel(0, 0)
    );
    let warm = execute_final_atlas_with_cache(
        &plan,
        prepared_sources_for_plan(&plan, Arc::clone(&prepared_domain)),
        &cache,
    );
    assert_eq!(warm.upload_bytes, 0);
    let cancelled_input = AtlasRenderExecutionInput {
        prepared_sources: prepared_sources_for_plan(&plan, Arc::clone(&prepared_domain)),
        source_frame_cache: None,
    };
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let error = with_gpu_executor(&cache, |executor| {
        executor
            .execute(&plan, &cancelled_input, &cancelled, &|| true)
            .expect_err("cancelled prepared synthesis must not publish")
    });
    assert!(matches!(
        error,
        hot_trimmer_sheet_compiler::AtlasRenderExecutionError::Cancelled
    ));

    let mut invalid_coverage = plan.clone();
    invalid_coverage.ordered_regions[0]
        .sampling_plan
        .slot_physical_size = [5.0, 4.0];
    invalid_coverage.final_plan_hash = ContentDigest(String::new());
    let invalid_input = AtlasRenderExecutionInput {
        prepared_sources: prepared_sources_for_plan(
            &invalid_coverage,
            Arc::clone(&prepared_domain),
        ),
        source_frame_cache: None,
    };
    let error = with_gpu_executor(&cache, |executor| {
        executor
            .execute(
                &invalid_coverage,
                &invalid_input,
                &CancellationToken::new(),
                &|| true,
            )
            .expect_err("insufficient prepared synthesis coverage must fail before dispatch")
    });
    assert!(
        error
            .to_string()
            .contains("lacks required physical coverage")
    );

    let mut mismatched_family = plan.clone();
    mismatched_family.ordered_regions[0]
        .sampling_plan
        .candidate
        .family = CandidateFamily::PanelPatchMatchExpansion;
    mismatched_family.final_plan_hash = ContentDigest(String::new());
    let mismatch_input = AtlasRenderExecutionInput {
        prepared_sources: prepared_sources_for_plan(
            &mismatched_family,
            Arc::clone(&prepared_domain),
        ),
        source_frame_cache: None,
    };
    let error = with_gpu_executor(&cache, |executor| {
        executor
            .execute(
                &mismatched_family,
                &mismatch_input,
                &CancellationToken::new(),
                &|| true,
            )
            .expect_err("PatchMatch family must reject a quilting prepared domain")
    });
    assert!(error.to_string().contains(
        "prepared synthesis domain identity, route, dimensions, or validity is incompatible"
    ));

    let direct_domain = domain_with_size(
        b"gpu-stage-14-fit-domain",
        b"gpu-stage-14-fit-source",
        6,
        4,
        255,
    );
    for mode in [
        SamplingMode::UniqueContain,
        SamplingMode::UniqueCover,
        SamplingMode::NineSlicePanel,
    ] {
        let mut exact = material_map_plan(&direct_domain, MaterialMapKind::BaseColor);
        exact.ordered_regions[0].source_crop = SourcePixelRect(PixelBounds {
            x: 0,
            y: 0,
            width: 6,
            height: 4,
        });
        exact.ordered_regions[0].sampling_plan.candidate.crop = Some(SourceCrop {
            x: 0,
            y: 0,
            width: 6,
            height: 4,
        });
        exact.ordered_regions[0]
            .sampling_plan
            .candidate
            .mapping_mode = mode;
        exact.ordered_regions[0].sampling_plan.candidate.family = match mode {
            SamplingMode::UniqueContain => CandidateFamily::UniqueContain,
            SamplingMode::UniqueCover => CandidateFamily::UniqueCover,
            SamplingMode::NineSlicePanel => CandidateFamily::NineSlicePanel,
            _ => unreachable!(),
        };
        exact.ordered_regions[0].sampling_plan.candidate.route = match mode {
            SamplingMode::NineSlicePanel => CandidateRoute::Cap,
            _ => CandidateRoute::Unique,
        };
        if mode == SamplingMode::NineSlicePanel {
            exact.ordered_regions[0].sampling_plan.slice_geometry = SliceGeometry::Nine {
                left_pixels: 1,
                right_pixels: 1,
                top_pixels: 1,
                bottom_pixels: 1,
                center: SliceCenterPolicy::Repeat,
            };
        }
        exact.final_plan_hash = ContentDigest(String::new());
        let output = execute_final_atlas(
            &exact,
            prepared_sources_for_plan(&exact, Arc::clone(&direct_domain)),
        );
        let oracle = cpu_expected_base_color(
            &exact.ordered_regions[0].sampling_plan,
            &direct_domain,
            [4, 4],
        );
        assert_eq!(
            output.base_color_rgba8.as_ref(),
            oracle.as_slice(),
            "{mode:?}"
        );
    }

    for (center_domain, center, stretch_override) in [
        (
            Arc::clone(&prepared_domain),
            SliceCenterPolicy::Synthesize,
            StretchOverrideProvenance::NotAuthorized,
        ),
        (
            Arc::clone(&direct_domain),
            SliceCenterPolicy::ExplicitStretch,
            StretchOverrideProvenance::UserOverride {
                settings_revision: 17,
            },
        ),
    ] {
        let mut exact = material_map_plan(&center_domain, MaterialMapKind::BaseColor);
        exact.ordered_regions[0]
            .sampling_plan
            .candidate
            .mapping_mode = SamplingMode::NineSlicePanel;
        exact.ordered_regions[0].sampling_plan.candidate.family = CandidateFamily::NineSlicePanel;
        exact.ordered_regions[0].sampling_plan.candidate.route = CandidateRoute::Cap;
        exact.ordered_regions[0].sampling_plan.slice_geometry = SliceGeometry::Nine {
            left_pixels: 1,
            right_pixels: 1,
            top_pixels: 1,
            bottom_pixels: 1,
            center,
        };
        exact.ordered_regions[0].sampling_plan.stretch_override = stretch_override;
        exact.final_plan_hash = ContentDigest(String::new());
        let output = execute_final_atlas(
            &exact,
            prepared_sources_for_plan(&exact, Arc::clone(&center_domain)),
        );
        let oracle = cpu_expected_base_color(
            &exact.ordered_regions[0].sampling_plan,
            &center_domain,
            [4, 4],
        );
        assert_eq!(
            output.base_color_rgba8.as_ref(),
            oracle.as_slice(),
            "{center:?}"
        );
    }

    let mut unsynthesized_center = material_map_plan(&direct_domain, MaterialMapKind::BaseColor);
    unsynthesized_center.ordered_regions[0]
        .sampling_plan
        .candidate
        .mapping_mode = SamplingMode::NineSlicePanel;
    unsynthesized_center.ordered_regions[0]
        .sampling_plan
        .candidate
        .family = CandidateFamily::NineSlicePanel;
    unsynthesized_center.ordered_regions[0]
        .sampling_plan
        .candidate
        .route = CandidateRoute::Cap;
    unsynthesized_center.ordered_regions[0]
        .sampling_plan
        .slice_geometry = SliceGeometry::Nine {
        left_pixels: 1,
        right_pixels: 1,
        top_pixels: 1,
        bottom_pixels: 1,
        center: SliceCenterPolicy::Synthesize,
    };
    unsynthesized_center.final_plan_hash = ContentDigest(String::new());
    let unsynthesized_input = AtlasRenderExecutionInput {
        prepared_sources: prepared_sources_for_plan(
            &unsynthesized_center,
            Arc::clone(&direct_domain),
        ),
        source_frame_cache: None,
    };
    let error = with_gpu_executor(&cache, |executor| {
        executor
            .execute(
                &unsynthesized_center,
                &unsynthesized_input,
                &CancellationToken::new(),
                &|| true,
            )
            .expect_err("synthesized center on a direct domain must fail")
    });
    assert!(
        error
            .to_string()
            .contains("requires a synthesis-capable prepared domain")
    );

    let mut insufficient_center = material_map_plan(&prepared_domain, MaterialMapKind::BaseColor);
    insufficient_center.ordered_regions[0]
        .sampling_plan
        .candidate
        .mapping_mode = SamplingMode::NineSlicePanel;
    insufficient_center.ordered_regions[0]
        .sampling_plan
        .candidate
        .family = CandidateFamily::NineSlicePanel;
    insufficient_center.ordered_regions[0]
        .sampling_plan
        .candidate
        .route = CandidateRoute::Cap;
    insufficient_center.ordered_regions[0]
        .sampling_plan
        .slice_geometry = SliceGeometry::Nine {
        left_pixels: 1,
        right_pixels: 1,
        top_pixels: 1,
        bottom_pixels: 1,
        center: SliceCenterPolicy::Synthesize,
    };
    insufficient_center.ordered_regions[0]
        .sampling_plan
        .slot_physical_size = [5.0, 4.0];
    insufficient_center.final_plan_hash = ContentDigest(String::new());
    let insufficient_input = AtlasRenderExecutionInput {
        prepared_sources: prepared_sources_for_plan(
            &insufficient_center,
            Arc::clone(&prepared_domain),
        ),
        source_frame_cache: None,
    };
    let error = with_gpu_executor(&cache, |executor| {
        executor
            .execute(
                &insufficient_center,
                &insufficient_input,
                &CancellationToken::new(),
                &|| true,
            )
            .expect_err("synthesized center beyond prepared coverage must fail")
    });
    assert!(
        error
            .to_string()
            .contains("exceeds prepared center coverage")
    );

    let mut mismatched_center = insufficient_center.clone();
    mismatched_center.ordered_regions[0]
        .sampling_plan
        .slot_physical_size = [4.0, 4.0];
    mismatched_center.final_plan_hash = ContentDigest(String::new());
    let mut stale_domain = (*prepared_domain).clone();
    stale_domain.cache_key = ContentDigest::sha256(b"stale-synthesized-center-domain");
    let stale_domain = Arc::new(stale_domain);
    let mismatched_input = AtlasRenderExecutionInput {
        prepared_sources: prepared_sources_for_plan(&mismatched_center, stale_domain),
        source_frame_cache: None,
    };
    let error = with_gpu_executor(&cache, |executor| {
        executor
            .execute(
                &mismatched_center,
                &mismatched_input,
                &CancellationToken::new(),
                &|| true,
            )
            .expect_err("stale synthesized-center domain identity must fail")
    });
    assert!(
        error
            .to_string()
            .contains("synthesized slice center prepared-domain identity is incompatible")
    );

    let mut illegal_slice = material_map_plan(&direct_domain, MaterialMapKind::BaseColor);
    illegal_slice.ordered_regions[0]
        .sampling_plan
        .candidate
        .mapping_mode = SamplingMode::NineSlicePanel;
    illegal_slice.ordered_regions[0]
        .sampling_plan
        .candidate
        .family = CandidateFamily::NineSlicePanel;
    illegal_slice.ordered_regions[0]
        .sampling_plan
        .candidate
        .route = CandidateRoute::Cap;
    illegal_slice.final_plan_hash = ContentDigest(String::new());
    let illegal_input = AtlasRenderExecutionInput {
        prepared_sources: prepared_sources_for_plan(&illegal_slice, Arc::clone(&direct_domain)),
        source_frame_cache: None,
    };
    let error = with_gpu_executor(&cache, |executor| {
        executor
            .execute(
                &illegal_slice,
                &illegal_input,
                &CancellationToken::new(),
                &|| true,
            )
            .expect_err("nine-slice without nine-slice geometry must fail")
    });
    assert!(error.to_string().contains("illegal GPU slice"));

    let mut unauthorized = illegal_slice.clone();
    unauthorized.ordered_regions[0].sampling_plan.slice_geometry = SliceGeometry::Nine {
        left_pixels: 1,
        right_pixels: 1,
        top_pixels: 1,
        bottom_pixels: 1,
        center: SliceCenterPolicy::ExplicitStretch,
    };
    unauthorized.final_plan_hash = ContentDigest(String::new());
    let unauthorized_input = AtlasRenderExecutionInput {
        prepared_sources: prepared_sources_for_plan(&unauthorized, Arc::clone(&direct_domain)),
        source_frame_cache: None,
    };
    let error = with_gpu_executor(&cache, |executor| {
        executor
            .execute(
                &unauthorized,
                &unauthorized_input,
                &CancellationToken::new(),
                &|| true,
            )
            .expect_err("unauthorized nine-slice center stretch must fail")
    });
    assert!(error.to_string().contains("illegal GPU slice"));
}
