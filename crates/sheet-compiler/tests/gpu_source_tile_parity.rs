use std::sync::{Arc, Mutex, OnceLock};

use hot_trimmer_domain::{
    CancellationToken, ContentDigest, DocumentHash, EdgeEligibility, ManualRegionRole,
    MaterialChannelRole, MaterialMapKind, NormalConvention, OrientedPixelSize, PixelBounds,
    PixelSize, QuarterTurn, RadialMappingSettings, RegionContinuity, RegionId, RegionSampling,
    SamplingMode, SamplingPolicy, SourceSamplingMode, SourceSetId, StructuralProfile,
    TemplateSlotRole,
};
use hot_trimmer_export::ExportMemoryBudgets;
use hot_trimmer_image_io::{
    ImagePlane, LinearColor, LinearScalar, NormalAlphaPolicy, ResolvedAlphaMode, TangentNormal,
};
use hot_trimmer_material_synthesis::PreparedMaterialDomain;
use hot_trimmer_placement_solver::{
    CandidateDescriptors, CandidateFamily, CandidateRoute, CandidateTransform, CropCandidate,
    EligibilityEvidence, MirrorTransform, PositionStrategy, SamplingPlan, SliceCenterPolicy,
    SliceGeometry, SourceCrop, StretchOverrideProvenance,
};
use hot_trimmer_render_core::PreparedExemplarChannel;
use hot_trimmer_sheet_compiler::{
    AtlasFinalAtlasOutput, AtlasPreparedSource, AtlasRenderExecutionInput, AtlasRenderExecutor,
    AtlasRenderExecutorOutput, CompiledAtlasPlanV1, CompiledAtlasPreviewProfile,
    CompiledColorSpacePolicy, CompiledNormalConvention, CompiledRegionCommandV1,
    CompiledSourceCommandV1, CompiledTileRequest, CompiledTileRequestKind, CpuAtlasRenderExecutor,
    GpuAtlasRenderExecutor, GpuAtlasSourceTextureCache, OutputPixelRect, SourcePixelRect,
};

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

fn signed_unit(value: f32) -> u8 {
    ((value * 0.5 + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8
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

fn source_color(x: u32, y: u32) -> LinearColor {
    let r = ((x.wrapping_mul(37) + y.wrapping_mul(11)) & 0xff) as u8;
    let g = ((x.wrapping_mul(7) + y.wrapping_mul(53) + 19) & 0xff) as u8;
    let b = ((x.wrapping_mul(3) + y.wrapping_mul(5) + 101) & 0xff) as u8;
    LinearColor {
        rgb: [srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b)],
        alpha: 1.0,
    }
}

fn source_scalar(x: u32, y: u32) -> LinearScalar {
    LinearScalar(((x.wrapping_mul(13) + y.wrapping_mul(17)) % 100) as f32 / 99.0)
}

fn color_domain(seed: &'static [u8], width: u32, height: u32) -> Arc<PreparedMaterialDomain> {
    color_domain_with_tile_edge(seed, width, height, width.min(128).max(1))
}

fn color_domain_with_tile_edge(
    seed: &'static [u8],
    width: u32,
    height: u32,
    tile_edge: u32,
) -> Arc<PreparedMaterialDomain> {
    let pixels = (0..height)
        .flat_map(|y| (0..width).map(move |x| source_color(x, y)))
        .collect::<Vec<_>>();
    Arc::new(
        PreparedMaterialDomain::from_registered_channels(
            ContentDigest::sha256(seed),
            ContentDigest::sha256(seed),
            vec![PreparedExemplarChannel::BaseColor {
                plane: ImagePlane::from_row_major(width, height, tile_edge.max(1), &pixels)
                    .unwrap(),
                alpha_mode: ResolvedAlphaMode::Opaque,
            }],
        )
        .unwrap(),
    )
}

fn height_domain(seed: &'static [u8], width: u32, height: u32) -> Arc<PreparedMaterialDomain> {
    let colors = (0..height)
        .flat_map(|y| (0..width).map(move |x| source_color(x, y)))
        .collect::<Vec<_>>();
    let heights = (0..height)
        .flat_map(|y| (0..width).map(move |x| source_scalar(x, y)))
        .collect::<Vec<_>>();
    Arc::new(
        PreparedMaterialDomain::from_registered_channels(
            ContentDigest::sha256(seed),
            ContentDigest::sha256(seed),
            vec![
                PreparedExemplarChannel::BaseColor {
                    plane: ImagePlane::from_row_major(
                        width,
                        height,
                        width.min(128).max(1),
                        &colors,
                    )
                    .unwrap(),
                    alpha_mode: ResolvedAlphaMode::Opaque,
                },
                PreparedExemplarChannel::Scalar {
                    role: MaterialChannelRole::Height,
                    plane: ImagePlane::from_row_major(
                        width,
                        height,
                        width.min(128).max(1),
                        &heights,
                    )
                    .unwrap(),
                },
            ],
        )
        .unwrap(),
    )
}

fn normal_domain(
    seed: &'static [u8],
    width: u32,
    height: u32,
    authored_normal: Option<TangentNormal>,
) -> Arc<PreparedMaterialDomain> {
    let colors = (0..height)
        .flat_map(|y| (0..width).map(move |x| source_color(x, y)))
        .collect::<Vec<_>>();
    let heights = vec![LinearScalar(0.5); (width * height) as usize];
    let mut channels = vec![
        PreparedExemplarChannel::BaseColor {
            plane: ImagePlane::from_row_major(width, height, width.min(128).max(1), &colors)
                .unwrap(),
            alpha_mode: ResolvedAlphaMode::Opaque,
        },
        PreparedExemplarChannel::Scalar {
            role: MaterialChannelRole::Height,
            plane: ImagePlane::from_row_major(width, height, width.min(128).max(1), &heights)
                .unwrap(),
        },
    ];
    if let Some(normal) = authored_normal {
        let normals = vec![normal; (width * height) as usize];
        channels.push(PreparedExemplarChannel::Normal {
            plane: ImagePlane::from_row_major(width, height, width.min(128).max(1), &normals)
                .unwrap(),
            source_convention: NormalConvention::OpenGl,
            canonical_convention: NormalConvention::OpenGl,
            alpha_policy: NormalAlphaPolicy::Ignore,
        });
    }
    Arc::new(
        PreparedMaterialDomain::from_registered_channels(
            ContentDigest::sha256(seed),
            ContentDigest::sha256(seed),
            channels,
        )
        .unwrap(),
    )
}

fn source_record(
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
        decoder_version: "gpu-source-tile-parity-decoder".into(),
        decoded_format: "rgba8".into(),
        color_version: "gpu-source-tile-parity-color".into(),
        channel_role: role,
    }
}

fn sampling_plan(
    region_id: RegionId,
    source_id: ContentDigest,
    domain_id: ContentDigest,
    crop: SourceCrop,
    destination: PixelBounds,
    mode: SamplingMode,
    filter: SourceSamplingMode,
) -> SamplingPlan {
    SamplingPlan {
        slot_id: region_id,
        role: if mode == SamplingMode::ThreeSliceCap {
            TemplateSlotRole::TrimCap
        } else {
            TemplateSlotRole::Planar
        },
        variation_group: "gpu-source-tile-parity".into(),
        prepared_domain_dimensions: [crop.x + crop.width, crop.y + crop.height],
        candidate: CropCandidate {
            candidate_id: ContentDigest::sha256(
                format!("gpu-source-tile-parity-candidate-{region_id}-{mode:?}").as_bytes(),
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
            family: match mode {
                SamplingMode::PeriodicTile => CandidateFamily::PanelSeamlessTile,
                SamplingMode::RepeatX => CandidateFamily::RepeatXSegment,
                SamplingMode::RepeatY => CandidateFamily::RepeatYSegment,
                SamplingMode::ThreeSliceCap => CandidateFamily::ThreeSliceCap,
                SamplingMode::PlanarRadial => CandidateFamily::PlanarRadialSquare,
                SamplingMode::PolarRadial => CandidateFamily::PolarRadialSynthesis,
                _ => CandidateFamily::PanelDirect,
            },
            route: match mode {
                SamplingMode::PeriodicTile | SamplingMode::RepeatX | SamplingMode::RepeatY => {
                    CandidateRoute::Repeat
                }
                SamplingMode::ThreeSliceCap => CandidateRoute::Cap,
                SamplingMode::PlanarRadial => CandidateRoute::PlanarRadial,
                SamplingMode::PolarRadial => CandidateRoute::PolarRadial,
                _ => CandidateRoute::Direct,
            },
            position_strategy: PositionStrategy::DenseLowResolution,
            period_pixels: Some([crop.width.max(1) / 2, crop.height.max(1)]),
            seam_indices: Vec::new(),
            correspondence_reference: domain_id,
            descriptors: CandidateDescriptors {
                saliency_milli: 500,
                stationarity_milli: 1000,
                feature_strength_milli: 500,
                usability_milli: 1000,
            },
            seed: 5,
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
        slot_physical_size: [f64::from(destination.width), f64::from(destination.height)],
        source_pixels_per_physical_unit: 1.0,
        sampling_policy: SamplingPolicy {
            filter,
            scale: 1.0,
            correct_tangent_normals: true,
        },
        radial_mapping: None,
        stretch_override: StretchOverrideProvenance::NotAuthorized,
        slice_geometry: if mode == SamplingMode::ThreeSliceCap {
            SliceGeometry::Three {
                leading_cap_pixels: 4,
                trailing_cap_pixels: 4,
                center: SliceCenterPolicy::Repeat,
            }
        } else {
            SliceGeometry::None
        },
        maximum_seam_cost_milli: 0,
        unary_cost: 0.0,
    }
}

fn region_command(
    compact_index: u32,
    region_id: RegionId,
    source: &CompiledSourceCommandV1,
    domain: &PreparedMaterialDomain,
    crop: SourceCrop,
    destination: PixelBounds,
    mode: SamplingMode,
    sampling: RegionSampling,
    filter: SourceSamplingMode,
) -> CompiledRegionCommandV1 {
    let mut sampling_plan = sampling_plan(
        region_id,
        source.source_id.clone(),
        domain.cache_key.clone(),
        crop,
        destination,
        mode,
        filter,
    );
    sampling_plan.prepared_domain_dimensions = [domain.width, domain.height];
    let radial_parameters = (mode == SamplingMode::PolarRadial
        || mode == SamplingMode::PlanarRadial)
        .then_some(RadialMappingSettings {
            center_x: 0.5,
            center_y: 0.5,
            inner_radius: 0.05,
            outer_radius: 0.5,
            falloff: 1.0,
            blend_width: 0.0,
            seam_blend_width: if mode == SamplingMode::PolarRadial {
                0.18
            } else {
                0.0
            },
        });
    sampling_plan.radial_mapping = radial_parameters;
    CompiledRegionCommandV1 {
        region_id,
        compact_index,
        region_role: match mode {
            SamplingMode::PolarRadial | SamplingMode::PlanarRadial => ManualRegionRole::Radial,
            SamplingMode::RepeatX => ManualRegionRole::HorizontalStrip,
            SamplingMode::RepeatY => ManualRegionRole::VerticalStrip,
            SamplingMode::ThreeSliceCap => ManualRegionRole::Panel,
            _ => ManualRegionRole::Panel,
        },
        source_set_id: source.source_set_id,
        source_id: source.source_id.clone(),
        patch_id: None,
        source_crop: SourcePixelRect(PixelBounds {
            x: crop.x,
            y: crop.y,
            width: crop.width,
            height: crop.height,
        }),
        destination_rect: OutputPixelRect(destination),
        sampling,
        source_to_region_transform: Default::default(),
        radial_parameters,
        structural_profile: StructuralProfile::Flat,
        continuity: match sampling {
            RegionSampling::LoopX => RegionContinuity::X,
            RegionSampling::LoopY => RegionContinuity::Y,
            RegionSampling::LoopXy => RegionContinuity::Xy,
            RegionSampling::OneShot => RegionContinuity::None,
        },
        padding_px: 0,
        edge_eligibility: EdgeEligibility::default(),
        sampling_plan,
        render_cache_key: ContentDigest::sha256(
            format!("gpu-source-tile-parity-render-{region_id}").as_bytes(),
        ),
    }
}

fn plan(
    output_size: PixelSize,
    requested_maps: Vec<MaterialMapKind>,
    ordered_sources: Vec<CompiledSourceCommandV1>,
    ordered_regions: Vec<CompiledRegionCommandV1>,
) -> CompiledAtlasPlanV1 {
    CompiledAtlasPlanV1 {
        schema_version: 1,
        algorithm_version: "gpu-source-tile-parity-test".into(),
        document_revision: 1,
        request_generation: Some(1),
        topology_hash: DocumentHash([11; 32]),
        appearance_hash: DocumentHash([12; 32]),
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
        requested_maps,
        ordered_sources,
        ordered_regions,
        final_plan_hash: ContentDigest(String::new()),
    }
    .finalize()
    .unwrap()
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

fn execute(
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
    let output = executor
        .execute(
            plan,
            &AtlasRenderExecutionInput {
                prepared_sources,
                source_frame_cache: None,
            },
            &CancellationToken::new(),
            &|| true,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    let AtlasRenderExecutorOutput::FinalAtlas(output) = output else {
        panic!("GPU source-tile parity gate must execute the final-atlas route");
    };
    output
}

fn cpu_base_color(
    plan: &CompiledAtlasPlanV1,
    prepared_sources: Vec<AtlasPreparedSource>,
) -> Vec<u8> {
    let output = CpuAtlasRenderExecutor
        .execute(
            plan,
            &AtlasRenderExecutionInput {
                prepared_sources,
                source_frame_cache: None,
            },
            &CancellationToken::new(),
            &|| true,
        )
        .expect("CPU source-tile oracle");
    let AtlasRenderExecutorOutput::CpuRegions(output) = output else {
        panic!("CPU source-tile oracle must produce regions");
    };
    let region = output
        .regions
        .first()
        .expect("CPU source-tile oracle region");
    let PreparedExemplarChannel::BaseColor { plane, .. } = region
        .result
        .channels
        .iter()
        .find(|channel| channel.role() == MaterialChannelRole::BaseColor)
        .expect("CPU oracle Base Color")
    else {
        panic!("CPU oracle Base Color channel");
    };
    (0..plane.height())
        .flat_map(|y| {
            (0..plane.width()).flat_map(move |x| {
                let pixel = plane.pixel(x, y);
                [
                    linear_to_srgb(pixel.rgb[0]),
                    linear_to_srgb(pixel.rgb[1]),
                    linear_to_srgb(pixel.rgb[2]),
                    (pixel.alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
                ]
            })
        })
        .collect()
}

fn assert_pixels_close(actual: &[u8], expected: &[u8], label: &str) {
    assert_pixels_with_tolerance(actual, expected, label, 2);
}

fn assert_pixels_with_tolerance(actual: &[u8], expected: &[u8], label: &str, tolerance: i16) {
    assert_eq!(actual.len(), expected.len(), "{label} payload length");
    for (index, (actual, expected)) in actual.iter().zip(expected.iter()).enumerate() {
        let delta = i16::from(*actual) - i16::from(*expected);
        assert!(
            delta.abs() <= tolerance,
            "{label} byte {index} expected {expected} got {actual} delta {delta} tolerance {tolerance}"
        );
    }
}

fn caps_texture_edge() -> u32 {
    gpu_test_service()
        .initialize()
        .expect("GPU capability service")
        .capabilities()
        .maximum_texture_dimension_2d
}

fn source_page_interior(edge: u32) -> u32 {
    edge.saturating_sub(2).max(1)
}

fn full_source_page_layers(width: u32, height: u32, page_interior: u32) -> u32 {
    width
        .div_ceil(page_interior)
        .saturating_mul(height.div_ceil(page_interior))
}

fn full_source_page_array_bytes(width: u32, height: u32, page_interior: u32) -> u64 {
    let page_width = page_interior.saturating_add(2);
    let page_height = height.min(page_interior).saturating_add(2);
    u64::from(page_width)
        .saturating_mul(u64::from(page_height))
        .saturating_mul(4)
        .saturating_mul(u64::from(full_source_page_layers(
            width,
            height,
            page_interior,
        )))
}

fn assert_footprint_residency_bounded(
    cache: &Mutex<GpuAtlasSourceTextureCache>,
    full_source_layers: u32,
    full_source_bytes: u64,
    label: &str,
) {
    let cache = cache.lock().unwrap();
    let resident_layers = cache.source_layer_count();
    let resident_bytes = cache.source_resident_bytes();
    let budget = ExportMemoryBudgets::default().gpu_source_residency_bytes;
    assert!(
        resident_layers <= 2,
        "{label} should keep only the current footprint pages resident; resident_layers={resident_layers}, full_source_layers={full_source_layers}"
    );
    assert!(
        resident_layers <= full_source_layers,
        "{label} resident layer count must not exceed the complete source page array; resident_layers={resident_layers}, full_source_layers={full_source_layers}"
    );
    assert!(
        resident_bytes < full_source_bytes / 2,
        "{label} resident bytes must be significantly smaller than the complete source page array; resident_bytes={resident_bytes}, full_source_page_array_bytes={full_source_bytes}, delta={}",
        resident_bytes as i128 - full_source_bytes as i128
    );
    assert!(
        resident_bytes < budget,
        "{label} resident bytes must stay under the declared source residency budget; resident_bytes={resident_bytes}, budget={budget}"
    );
}

fn telemetry_u64(output: &AtlasFinalAtlasOutput, key: &str) -> u64 {
    output
        .telemetry
        .iter()
        .find_map(|line| {
            line.split(';').find_map(|field| {
                let field = field.trim();
                field
                    .strip_prefix(key)
                    .and_then(|value| value.strip_prefix('='))
                    .and_then(|value| value.parse::<u64>().ok())
            })
        })
        .unwrap_or_else(|| panic!("missing telemetry field {key} in {:?}", output.telemetry))
}

#[test]
fn gpu_source_tile_parity() {
    let edge = caps_texture_edge();
    let wide_width = edge + 64;
    let wide_height = 32;
    let x_boundary_crop = SourceCrop {
        x: edge - 6,
        y: 7,
        width: 18,
        height: 8,
    };

    let direct_domain = color_domain(b"gpu-source-tile-parity-direct", wide_width, wide_height);
    let source_set_id = SourceSetId::from_bytes([31; 16]);
    let direct_source = source_record(
        &direct_domain,
        source_set_id,
        MaterialChannelRole::BaseColor,
    );
    let direct_region = region_command(
        0,
        RegionId::from_bytes([31; 16]),
        &direct_source,
        &direct_domain,
        x_boundary_crop,
        PixelBounds {
            x: 0,
            y: 0,
            width: x_boundary_crop.width,
            height: x_boundary_crop.height,
        },
        SamplingMode::DirectCrop,
        RegionSampling::OneShot,
        SourceSamplingMode::Linear,
    );
    let direct_plan = plan(
        PixelSize {
            width: x_boundary_crop.width,
            height: x_boundary_crop.height,
        },
        vec![MaterialMapKind::BaseColor],
        vec![direct_source.clone()],
        vec![direct_region],
    );
    let direct_cache = Mutex::new(GpuAtlasSourceTextureCache::default());
    let direct_output = execute(
        &direct_plan,
        vec![prepared_source(&direct_source, Arc::clone(&direct_domain))],
        &direct_cache,
    );
    assert_pixels_close(
        &direct_output.base_color_rgba8,
        &cpu_base_color(
            &direct_plan,
            vec![prepared_source(&direct_source, Arc::clone(&direct_domain))],
        ),
        "direct bilinear source-tile boundary",
    );
    let page_interior = source_page_interior(edge);
    assert_footprint_residency_bounded(
        &direct_cache,
        full_source_page_layers(wide_width, wide_height, page_interior),
        full_source_page_array_bytes(wide_width, wide_height, page_interior),
        "wide direct crop must keep only its current footprint resident",
    );
    assert!(direct_output.upload_bytes > 0);
    assert!(direct_output.telemetry.iter().any(|line| {
        line.contains("requested_map=BaseColor")
            && line.contains(&format!("upload_bytes={}", direct_output.upload_bytes))
    }));

    let y_domain = color_domain(b"gpu-source-tile-parity-y", 8, edge + 64);
    let y_source_set_id = SourceSetId::from_bytes([32; 16]);
    let y_source = source_record(&y_domain, y_source_set_id, MaterialChannelRole::BaseColor);
    let y_crop = SourceCrop {
        x: 2,
        y: edge - 6,
        width: 4,
        height: 18,
    };
    let y_region = region_command(
        0,
        RegionId::from_bytes([32; 16]),
        &y_source,
        &y_domain,
        y_crop,
        PixelBounds {
            x: 0,
            y: 0,
            width: y_crop.width,
            height: y_crop.height,
        },
        SamplingMode::DirectCrop,
        RegionSampling::OneShot,
        SourceSamplingMode::Linear,
    );
    let y_plan = plan(
        PixelSize {
            width: y_crop.width,
            height: y_crop.height,
        },
        vec![MaterialMapKind::BaseColor],
        vec![y_source.clone()],
        vec![y_region],
    );
    let y_cache = Mutex::new(GpuAtlasSourceTextureCache::default());
    let y_output = execute(
        &y_plan,
        vec![prepared_source(&y_source, Arc::clone(&y_domain))],
        &y_cache,
    );
    assert_pixels_close(
        &y_output.base_color_rgba8,
        &cpu_base_color(
            &y_plan,
            vec![prepared_source(&y_source, Arc::clone(&y_domain))],
        ),
        "direct bilinear source-tile row boundary",
    );
    assert_footprint_residency_bounded(
        &y_cache,
        full_source_page_layers(8, edge + 64, page_interior),
        full_source_page_array_bytes(8, edge + 64, page_interior),
        "tall direct crop must keep only its current footprint resident",
    );

    let mode_cases = [
        (
            "loop source-tile seam",
            SamplingMode::PeriodicTile,
            RegionSampling::LoopXy,
            PixelBounds {
                x: 0,
                y: 0,
                width: 18,
                height: 8,
            },
        ),
        (
            "radial source-tile seam",
            SamplingMode::PolarRadial,
            RegionSampling::OneShot,
            PixelBounds {
                x: 0,
                y: 0,
                width: 18,
                height: 18,
            },
        ),
        (
            "three-slice source-tile caps",
            SamplingMode::ThreeSliceCap,
            RegionSampling::OneShot,
            PixelBounds {
                x: 0,
                y: 0,
                width: 28,
                height: 8,
            },
        ),
    ];
    for (label, mode, sampling, destination) in mode_cases {
        let region = region_command(
            0,
            RegionId::from_bytes([40 + mode as u8; 16]),
            &direct_source,
            &direct_domain,
            x_boundary_crop,
            destination,
            mode,
            sampling,
            SourceSamplingMode::Linear,
        );
        let mode_plan = plan(
            PixelSize {
                width: destination.width,
                height: destination.height,
            },
            vec![MaterialMapKind::BaseColor],
            vec![direct_source.clone()],
            vec![region.clone()],
        );
        let cache = Mutex::new(GpuAtlasSourceTextureCache::default());
        let gpu = execute(
            &mode_plan,
            vec![prepared_source(&direct_source, Arc::clone(&direct_domain))],
            &cache,
        );
        assert_pixels_with_tolerance(
            &gpu.base_color_rgba8,
            &cpu_base_color(
                &mode_plan,
                vec![prepared_source(&direct_source, Arc::clone(&direct_domain))],
            ),
            label,
            2,
        );
        assert_footprint_residency_bounded(
            &cache,
            full_source_page_layers(wide_width, wide_height, page_interior),
            full_source_page_array_bytes(wide_width, wide_height, page_interior),
            label,
        );
    }

    let height_domain = height_domain(b"gpu-source-tile-parity-height", wide_width, wide_height);
    let height_source_set_id = SourceSetId::from_bytes([51; 16]);
    let height_base = source_record(
        &height_domain,
        height_source_set_id,
        MaterialChannelRole::BaseColor,
    );
    let height_source = source_record(
        &height_domain,
        height_source_set_id,
        MaterialChannelRole::Height,
    );
    let height_region = region_command(
        0,
        RegionId::from_bytes([51; 16]),
        &height_base,
        &height_domain,
        x_boundary_crop,
        PixelBounds {
            x: 0,
            y: 0,
            width: x_boundary_crop.width,
            height: x_boundary_crop.height,
        },
        SamplingMode::DirectCrop,
        RegionSampling::OneShot,
        SourceSamplingMode::Nearest,
    );
    let height_plan = plan(
        PixelSize {
            width: x_boundary_crop.width,
            height: x_boundary_crop.height,
        },
        vec![MaterialMapKind::Height],
        vec![height_base.clone(), height_source.clone()],
        vec![height_region],
    );
    let height_cache = Mutex::new(GpuAtlasSourceTextureCache::default());
    let height_output = execute(
        &height_plan,
        vec![
            prepared_source(&height_base, Arc::clone(&height_domain)),
            prepared_source(&height_source, Arc::clone(&height_domain)),
        ],
        &height_cache,
    );
    let height_tile = height_output
        .map_tiles
        .get(&MaterialMapKind::Height)
        .expect("Height source-tile output");
    for y in 0..x_boundary_crop.height {
        for x in 0..x_boundary_crop.width {
            let actual = output_f32(height_tile.pixels(), x_boundary_crop.width, x, y);
            let expected = source_scalar(x_boundary_crop.x + x, x_boundary_crop.y + y).0;
            assert!(
                (actual - expected).abs() <= 0.000_01,
                "height nonzero-origin source tile sample ({x},{y}) expected {expected} got {actual}"
            );
        }
    }
    assert!(height_output.upload_bytes > 0);
    assert_footprint_residency_bounded(
        &height_cache,
        full_source_page_layers(wide_width, wide_height, page_interior),
        full_source_page_array_bytes(wide_width, wide_height, page_interior),
        "height source tile",
    );

    let missing_normal = normal_domain(
        b"gpu-source-tile-parity-missing-normal",
        wide_width,
        wide_height,
        None,
    );
    let authored_normal = TangentNormal {
        xyz: [0.6, 0.6, 0.529_150_25],
        alpha: 1.0,
    };
    let authored_normal_domain = normal_domain(
        b"gpu-source-tile-parity-authored-normal",
        wide_width,
        wide_height,
        Some(authored_normal),
    );
    let missing_set = SourceSetId::from_bytes([61; 16]);
    let authored_set = SourceSetId::from_bytes([62; 16]);
    let missing_base = source_record(&missing_normal, missing_set, MaterialChannelRole::BaseColor);
    let missing_height = source_record(&missing_normal, missing_set, MaterialChannelRole::Height);
    let authored_base = source_record(
        &authored_normal_domain,
        authored_set,
        MaterialChannelRole::BaseColor,
    );
    let authored_height = source_record(
        &authored_normal_domain,
        authored_set,
        MaterialChannelRole::Height,
    );
    let authored_normal_source = source_record(
        &authored_normal_domain,
        authored_set,
        MaterialChannelRole::Normal,
    );
    let left_region = region_command(
        0,
        RegionId::from_bytes([61; 16]),
        &missing_base,
        &missing_normal,
        x_boundary_crop,
        PixelBounds {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        },
        SamplingMode::DirectCrop,
        RegionSampling::OneShot,
        SourceSamplingMode::Nearest,
    );
    let right_region = region_command(
        1,
        RegionId::from_bytes([62; 16]),
        &authored_base,
        &authored_normal_domain,
        x_boundary_crop,
        PixelBounds {
            x: 8,
            y: 0,
            width: 8,
            height: 8,
        },
        SamplingMode::DirectCrop,
        RegionSampling::OneShot,
        SourceSamplingMode::Nearest,
    );
    let normal_plan = plan(
        PixelSize {
            width: 16,
            height: 8,
        },
        vec![MaterialMapKind::Normal],
        vec![
            missing_base.clone(),
            missing_height.clone(),
            authored_base.clone(),
            authored_height.clone(),
            authored_normal_source.clone(),
        ],
        vec![left_region, right_region],
    );
    let normal_cache = Mutex::new(GpuAtlasSourceTextureCache::default());
    let normal_output = execute(
        &normal_plan,
        vec![
            prepared_source(&missing_base, Arc::clone(&missing_normal)),
            prepared_source(&missing_height, Arc::clone(&missing_normal)),
            prepared_source(&authored_base, Arc::clone(&authored_normal_domain)),
            prepared_source(&authored_height, Arc::clone(&authored_normal_domain)),
            prepared_source(&authored_normal_source, Arc::clone(&authored_normal_domain)),
        ],
        &normal_cache,
    );
    assert_pixel_close(
        output_pixel(normal_output.interactive_tile.pixels(), 16, 0, 0),
        [128, 128, 255, 255],
        "missing Normal source must fall back to flat height-derived Normal",
        1,
    );
    assert_pixel_close(
        output_pixel(normal_output.interactive_tile.pixels(), 16, 8, 0),
        [
            signed_unit(authored_normal.xyz[0]),
            signed_unit(authored_normal.xyz[1]),
            signed_unit(authored_normal.xyz[2]),
            255,
        ],
        "authored Normal source in the same request must not be skipped",
        1,
    );
    assert!(normal_output.telemetry.iter().any(|line| {
        line.contains("requested_map=Normal")
            && line.contains("Normal<-authored-Normal|HeightFallback")
            && line.contains("upload_bytes=")
    }));
    assert!(
        normal_output.upload_bytes > 0,
        "mixed Normal source request must publish in-flight upload bytes"
    );
}

#[test]
fn gpu_source_tile_residency_uses_current_footprint_pages_under_budget() {
    let edge = caps_texture_edge();
    let page_interior = source_page_interior(edge);
    let full_source_layers = 11;
    let source_width = page_interior
        .saturating_mul(full_source_layers - 1)
        .saturating_add(37);
    let source_height = 8;
    let full_source_bytes =
        full_source_page_array_bytes(source_width, source_height, page_interior);
    let domain = color_domain_with_tile_edge(
        b"gpu-source-tile-residency-footprint",
        source_width,
        source_height,
        page_interior.min(4096).max(1),
    );
    let source_set_id = SourceSetId::from_bytes([71; 16]);
    let source = source_record(&domain, source_set_id, MaterialChannelRole::BaseColor);
    let crop = SourceCrop {
        x: page_interior.saturating_mul(7).saturating_add(4),
        y: 1,
        width: 16,
        height: 4,
    };
    let region = region_command(
        0,
        RegionId::from_bytes([71; 16]),
        &source,
        &domain,
        crop,
        PixelBounds {
            x: 0,
            y: 0,
            width: crop.width,
            height: crop.height,
        },
        SamplingMode::DirectCrop,
        RegionSampling::OneShot,
        SourceSamplingMode::Nearest,
    );
    let plan = plan(
        PixelSize {
            width: crop.width,
            height: crop.height,
        },
        vec![MaterialMapKind::BaseColor],
        vec![source.clone()],
        vec![region],
    );
    let cache = Mutex::new(GpuAtlasSourceTextureCache::default());
    let gpu = execute(
        &plan,
        vec![prepared_source(&source, Arc::clone(&domain))],
        &cache,
    );
    assert_pixels_close(
        &gpu.base_color_rgba8,
        &cpu_base_color(&plan, vec![prepared_source(&source, Arc::clone(&domain))]),
        "bounded source residency direct crop",
    );
    assert_eq!(
        full_source_page_layers(source_width, source_height, page_interior),
        full_source_layers
    );
    assert_footprint_residency_bounded(
        &cache,
        full_source_layers,
        full_source_bytes,
        "bounded source residency direct crop",
    );
}

#[test]
fn gpu_source_tile_residency_streams_multiple_live_source_groups() {
    let edge = caps_texture_edge();
    let wide_width = edge + 64;
    let wide_height = 32;
    let crop = SourceCrop {
        x: edge - 6,
        y: 7,
        width: 18,
        height: 8,
    };

    let mut sources = Vec::new();
    let mut prepared = Vec::new();
    let mut regions = Vec::new();
    for index in 0..3_u8 {
        let domain = color_domain(
            match index {
                0 => b"gpu-source-tile-live-residency-a",
                1 => b"gpu-source-tile-live-residency-b",
                _ => b"gpu-source-tile-live-residency-c",
            },
            wide_width,
            wide_height,
        );
        let source_set_id = SourceSetId::from_bytes([80 + index; 16]);
        let source = source_record(&domain, source_set_id, MaterialChannelRole::BaseColor);
        regions.push(region_command(
            u32::from(index),
            RegionId::from_bytes([80 + index; 16]),
            &source,
            &domain,
            crop,
            PixelBounds {
                x: u32::from(index) * crop.width,
                y: 0,
                width: crop.width,
                height: crop.height,
            },
            SamplingMode::DirectCrop,
            RegionSampling::OneShot,
            SourceSamplingMode::Nearest,
        ));
        prepared.push(prepared_source(&source, Arc::clone(&domain)));
        sources.push(source);
    }

    let plan = plan(
        PixelSize {
            width: crop.width * 3,
            height: crop.height,
        },
        vec![MaterialMapKind::BaseColor],
        sources,
        regions,
    );
    let cache = Mutex::new(GpuAtlasSourceTextureCache::default());
    let gpu = execute(&plan, prepared, &cache);
    for region_index in 0..3 {
        for y in 0..crop.height {
            for x in 0..crop.width {
                let actual = output_pixel(
                    &gpu.base_color_rgba8,
                    crop.width * 3,
                    region_index * crop.width + x,
                    y,
                );
                let expected = source_color(crop.x + x, crop.y + y);
                assert_pixel_close(
                    actual,
                    [
                        linear_to_srgb(expected.rgb[0]),
                        linear_to_srgb(expected.rgb[1]),
                        linear_to_srgb(expected.rgb[2]),
                        255,
                    ],
                    "multi-source streamed residency pixel",
                    2,
                );
            }
        }
    }

    let checked_out_peak = telemetry_u64(&gpu, "checked_out_source_resident_bytes_peak");
    let upload_bytes = telemetry_u64(&gpu, "upload_bytes");
    let budget = ExportMemoryBudgets::default().gpu_source_residency_bytes;
    assert!(
        checked_out_peak > 0 && checked_out_peak <= budget,
        "checked-out live source residency high-water should stay under the source budget; peak={checked_out_peak}, budget={budget}, upload_bytes={upload_bytes}, telemetry={:?}",
        gpu.telemetry
    );
    assert!(
        upload_bytes > 0,
        "streamed source groups should upload data"
    );
    assert!(
        gpu.telemetry.iter().any(|line| {
            line.contains("source_resident_bytes=")
                && line.contains("source_resident_layers=")
                && line.contains("checked_out_source_resident_bytes_peak=")
                && line.contains("checked_out_source_layers_peak=")
        }),
        "production telemetry must publish cache and checked-out residency: {:?}",
        gpu.telemetry
    );
}

#[test]
fn gpu_source_tile_residency_renders_when_complete_source_needs_too_many_layers() {
    const RESIDENT_FOOTPRINT_LAYER_LIMIT: u32 = 8;

    let edge = caps_texture_edge();
    let page_interior = source_page_interior(edge);
    let full_source_layers = RESIDENT_FOOTPRINT_LAYER_LIMIT + 4;
    let source_width = page_interior
        .saturating_mul(full_source_layers - 1)
        .saturating_add(23);
    let source_height = 1;
    let full_source_bytes =
        full_source_page_array_bytes(source_width, source_height, page_interior);
    let domain = color_domain_with_tile_edge(
        b"gpu-source-tile-residency-layer-limit",
        source_width,
        source_height,
        page_interior.min(4096).max(1),
    );
    let source_set_id = SourceSetId::from_bytes([72; 16]);
    let source = source_record(&domain, source_set_id, MaterialChannelRole::BaseColor);
    let crop = SourceCrop {
        x: page_interior.saturating_mul(5).saturating_add(3),
        y: 0,
        width: 8,
        height: 1,
    };
    let region = region_command(
        0,
        RegionId::from_bytes([72; 16]),
        &source,
        &domain,
        crop,
        PixelBounds {
            x: 0,
            y: 0,
            width: crop.width,
            height: crop.height,
        },
        SamplingMode::DirectCrop,
        RegionSampling::OneShot,
        SourceSamplingMode::Nearest,
    );
    let plan = plan(
        PixelSize {
            width: crop.width,
            height: crop.height,
        },
        vec![MaterialMapKind::BaseColor],
        vec![source.clone()],
        vec![region],
    );
    let cache = Mutex::new(GpuAtlasSourceTextureCache::default());
    let gpu = execute(
        &plan,
        vec![prepared_source(&source, Arc::clone(&domain))],
        &cache,
    );
    assert_pixels_close(
        &gpu.base_color_rgba8,
        &cpu_base_color(&plan, vec![prepared_source(&source, Arc::clone(&domain))]),
        "bounded source residency layer-limit crop",
    );
    assert!(
        full_source_layers > RESIDENT_FOOTPRINT_LAYER_LIMIT,
        "complete source page array should exceed the resident footprint layer limit"
    );
    assert_footprint_residency_bounded(
        &cache,
        full_source_layers,
        full_source_bytes,
        "bounded source residency layer-limit crop",
    );
}

fn assert_pixel_close(actual: [u8; 4], expected: [u8; 4], label: &str, tolerance: i16) {
    for (index, (actual, expected)) in actual.iter().zip(expected.iter()).enumerate() {
        let delta = i16::from(*actual) - i16::from(*expected);
        assert!(
            delta.abs() <= tolerance,
            "{label} channel {index} expected {expected} got {actual}"
        );
    }
}
