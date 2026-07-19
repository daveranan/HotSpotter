use std::{
    collections::BTreeMap,
    io::Cursor,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use hot_trimmer_domain::{
    AlgorithmProvenance, ContentDigest, DocumentHash, EdgeEligibility, MaterialChannelRole,
    MaterialMapKind, MappingTransform, ManualRegionRole, OrientedPixelSize, PatchId, PixelBounds,
    PixelSize, RegionBehavior, QuarterTurn, RegionContinuity, RegionId, RegionSampling,
    RadialMappingSettings, SamplingMode, SamplingPolicy, SourceId, SourceSetId, StageResult,
    TemplateSlotRole, TrimSheetDocument, TrimSheetDocumentCommand,
};
use hot_trimmer_project_store::{ProjectStore, SourceChannel, SourceInput, SourceOwnership};
use hot_trimmer_placement_solver::{
    CandidateDescriptors, CandidateFamily, CandidateRoute, CandidateTransform, CropCandidate,
    EligibilityEvidence, MirrorTransform, PlacementObjectiveBreakdown, PlacementPlan,
    PlacementPlanQaView, PlacementValidationSummary, PositionStrategy, SamplingPlan,
    SliceGeometry, SourceCrop, StretchOverrideProvenance,
};

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use uuid::Uuid;

use hot_trimmer_sheet_compiler::{
    captured_cpu_atlas_executor_plan, clear_cpu_atlas_executor_plan_capture,
    compiled_atlas_plan_from_persisted, AtlasComposeExecutionInput, AtlasComposeExecutorOutput,
    AtlasRenderExecutionError,
    AtlasRenderExecutionInput, AtlasRenderExecutor, AtlasRenderExecutorOutput,
    CompiledAtlasPlanValidationError, CompiledAtlasPlanV1, CompiledAtlasPreviewProfile,
    CompiledColorSpacePolicy, CompiledNormalConvention, CompiledRegionCommandV1,
    CompiledSourceCommandV1, CompiledTileRequest, CompiledTileRequestKind, CpuAtlasRenderExecutor, IntermediateAtlasRequest,
    COMPILED_ATLAS_ALGORITHM_VERSION, COMPILED_ATLAS_PLAN_SCHEMA_VERSION,
    OutputPixelRect, SourcePixelRect, SourceFramePreviewProfile,
};

fn test_sampling_plan(
    region_id: RegionId,
    source_id: ContentDigest,
    crop: SourceCrop,
    sampling: RegionSampling,
    radial_mapping: Option<RadialMappingSettings>,
) -> SamplingPlan {
    let mapping_mode = if radial_mapping.is_some() {
        SamplingMode::PolarRadial
    } else {
        match sampling {
            RegionSampling::OneShot => SamplingMode::DirectCrop,
            RegionSampling::LoopX => SamplingMode::RepeatX,
            RegionSampling::LoopY => SamplingMode::RepeatY,
            RegionSampling::LoopXy => SamplingMode::PeriodicTile,
        }
    };
    let (family, route) = match mapping_mode {
        SamplingMode::RepeatX => (CandidateFamily::RepeatXSegment, CandidateRoute::Repeat),
        SamplingMode::RepeatY => (CandidateFamily::RepeatYSegment, CandidateRoute::Repeat),
        SamplingMode::PeriodicTile => (CandidateFamily::PanelSeamlessTile, CandidateRoute::Repeat),
        SamplingMode::PolarRadial => (CandidateFamily::PolarRadialSynthesis, CandidateRoute::PolarRadial),
        _ => (CandidateFamily::PanelDirect, CandidateRoute::Direct),
    };
    SamplingPlan {
        slot_id: region_id,
        role: TemplateSlotRole::Planar,
        variation_group: "primary".into(),
        prepared_domain_dimensions: [1_024, 1_024],
        candidate: CropCandidate {
            candidate_id: ContentDigest::sha256(region_id.to_string().as_bytes()),
            source_id,
            domain_id: ContentDigest::sha256(b"test-domain"),
            slot_id: region_id,
            crop: Some(crop),
            transform: CandidateTransform { rotation: QuarterTurn::Zero, mirror: MirrorTransform::None },
            isotropic_scale: 1.0,
            mapping_mode,
            family,
            route,
            position_strategy: PositionStrategy::DenseLowResolution,
            period_pixels: (sampling != RegionSampling::OneShot).then_some([16, 16]),
            seam_indices: Vec::new(),
            correspondence_reference: ContentDigest::sha256(b"test-domain"),
            descriptors: CandidateDescriptors {
                saliency_milli: 0,
                stationarity_milli: 0,
                feature_strength_milli: 0,
                usability_milli: 1000,
            },
            seed: 0,
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
                reasons: vec!["executor contract fixture".into()],
            },
        },
        slot_physical_size: [f64::from(crop.width), f64::from(crop.height)],
        source_pixels_per_physical_unit: 1.0,
        sampling_policy: SamplingPolicy::default(),
        radial_mapping,
        stretch_override: StretchOverrideProvenance::NotAuthorized,
        slice_geometry: SliceGeometry::None,
        maximum_seam_cost_milli: 0,
        unary_cost: 0.0,
    }
}

fn base_plan() -> CompiledAtlasPlanV1 {
    let first_source_set = SourceSetId::new();
    let second_source_set = SourceSetId::new();
    let first_source = ContentDigest::sha256(b"first source");
    let second_source = ContentDigest::sha256(b"second source");
    let _patch = PatchId::new();
    let region_a = RegionId::new();
    let region_b = RegionId::new();
    let region_c = RegionId::new();

    let sources = vec![
        CompiledSourceCommandV1 {
            source_set_id: first_source_set,
            source_id: first_source.clone(),
            digest: ContentDigest::sha256(b"first source bytes"),
            oriented_dimensions: OrientedPixelSize {
                width: 1_024,
                height: 1_024,
            },
            decoder_version: "decoder-1".into(),
            decoded_format: "rgba8".into(),
            color_version: "color-1".into(),
            channel_role: MaterialChannelRole::BaseColor,
        },
        CompiledSourceCommandV1 {
            source_set_id: second_source_set,
            source_id: second_source.clone(),
            digest: ContentDigest::sha256(b"second source bytes"),
            oriented_dimensions: OrientedPixelSize {
                width: 512,
                height: 768,
            },
            decoder_version: "decoder-2".into(),
            decoded_format: "rgba8".into(),
            color_version: "color-2".into(),
            channel_role: MaterialChannelRole::Normal,
        },
    ];

    let regions = vec![
        CompiledRegionCommandV1 {
            region_id: region_a,
            compact_index: 0,
            source_set_id: first_source_set,
            source_id: first_source.clone(),
            patch_id: None,
            source_crop: SourcePixelRect(PixelBounds {
                x: 0,
                y: 0,
                width: 256,
                height: 256,
            }),
            destination_rect: OutputPixelRect(PixelBounds {
                x: 0,
                y: 0,
                width: 256,
                height: 256,
            }),
            sampling: RegionSampling::OneShot,
            source_to_region_transform: MappingTransform {
                scale: [1.0, 1.0],
                rotation_degrees: 0.0,
                mirror_x: false,
                mirror_y: false,
                offset: [0.0, 0.0],
            },
            radial_parameters: None,
            continuity: RegionContinuity::None,
            padding_px: 4,
            edge_eligibility: EdgeEligibility::default(),
            sampling_plan: test_sampling_plan(
                region_a,
                first_source.clone(),
                SourceCrop { x: 0, y: 0, width: 256, height: 256 },
                RegionSampling::OneShot,
                None,
            ),
            render_cache_key: ContentDigest::sha256(b"region-a-render"),
        },
        CompiledRegionCommandV1 {
            region_id: region_b,
            compact_index: 1,
            source_set_id: first_source_set,
            source_id: first_source.clone(),
            patch_id: None,
            source_crop: SourcePixelRect(PixelBounds {
                x: 256,
                y: 0,
                width: 256,
                height: 256,
            }),
            destination_rect: OutputPixelRect(PixelBounds {
                x: 256,
                y: 0,
                width: 256,
                height: 256,
            }),
            sampling: RegionSampling::LoopX,
            source_to_region_transform: MappingTransform {
                scale: [1.0, 1.0],
                rotation_degrees: 0.0,
                mirror_x: false,
                mirror_y: false,
                offset: [0.0, 0.0],
            },
            radial_parameters: None,
            continuity: RegionContinuity::X,
            padding_px: 8,
            edge_eligibility: EdgeEligibility {
                left: true,
                right: true,
                top: false,
                bottom: false,
            },
            sampling_plan: test_sampling_plan(
                region_b,
                first_source.clone(),
                SourceCrop { x: 256, y: 0, width: 256, height: 256 },
                RegionSampling::LoopX,
                None,
            ),
            render_cache_key: ContentDigest::sha256(b"region-b-render"),
        },
        CompiledRegionCommandV1 {
            region_id: region_c,
            compact_index: 2,
            source_set_id: second_source_set,
            source_id: second_source.clone(),
            patch_id: None,
            source_crop: SourcePixelRect(PixelBounds {
                x: 0,
                y: 256,
                width: 256,
                height: 512,
            }),
            destination_rect: OutputPixelRect(PixelBounds {
                x: 0,
                y: 256,
                width: 256,
                height: 512,
            }),
            sampling: RegionSampling::LoopY,
            source_to_region_transform: MappingTransform {
                scale: [0.75, 1.25],
                rotation_degrees: 90.0,
                mirror_x: true,
                mirror_y: false,
                offset: [0.125, 0.25],
            },
            radial_parameters: None,
            continuity: RegionContinuity::Y,
            padding_px: 6,
            edge_eligibility: EdgeEligibility {
                left: false,
                right: false,
                top: true,
                bottom: true,
            },
            sampling_plan: test_sampling_plan(
                region_c,
                second_source.clone(),
                SourceCrop { x: 0, y: 256, width: 256, height: 512 },
                RegionSampling::LoopY,
                None,
            ),
            render_cache_key: ContentDigest::sha256(b"region-c-render"),
        },
    ];

    CompiledAtlasPlanV1 {
        schema_version: COMPILED_ATLAS_PLAN_SCHEMA_VERSION,
        algorithm_version: COMPILED_ATLAS_ALGORITHM_VERSION.into(),
        document_revision: 7,
        request_generation: Some(11),
        topology_hash: DocumentHash([0x12_u8; 32]),
        appearance_hash: DocumentHash([0x34_u8; 32]),
        output_size: PixelSize {
            width: 1024,
            height: 1024,
        },
        preview_profile: CompiledAtlasPreviewProfile::Authoritative,
        normal_convention: CompiledNormalConvention::OpenGl,
        color_space_policy: CompiledColorSpacePolicy::SrgbColorUnassociatedAlpha,
        tile_request: CompiledTileRequest {
            kind: CompiledTileRequestKind::ExactViewport,
            generation: 11,
            output_rect: OutputPixelRect(PixelBounds { x: 0, y: 0, width: 1024, height: 1024 }),
            mip_level: 0,
            halo_px: 0,
            valid_rect: OutputPixelRect(PixelBounds { x: 0, y: 0, width: 1024, height: 1024 }),
        },
        requested_maps: vec![MaterialMapKind::BaseColor, MaterialMapKind::Height],
        ordered_sources: sources,
        ordered_regions: regions,
        final_plan_hash: ContentDigest(String::new()),
    }
}

fn finalize(plan: CompiledAtlasPlanV1) -> ContentDigest {
    plan.finalize().expect("plan should finalize").final_plan_hash
}

fn compile_source_frame_document(
    store: &ProjectStore,
    document: &TrimSheetDocument,
) -> hot_trimmer_sheet_compiler::IntermediateAtlasArtifact {
    let mut summary = store.summary().expect("behavior summary");
    summary.document = Some(document.clone());
    hot_trimmer_sheet_compiler::AlgorithmCompiler::new()
        .compile_persisted_stage_14_preview(
            hot_trimmer_sheet_compiler::PersistedStage14PreviewRequest {
                project: &summary,
                revision: document.document_revision,
                draft_id: None,
                input_hash: None,
                profile: SourceFramePreviewProfile::Authoritative,
                view_intent: None,
            },
            &hot_trimmer_domain::CancellationToken::new(),
            || true,
        )
        .expect("source-frame behavioral compile")
}

#[derive(Default)]
struct CapturingAtlasRenderExecutor {
    observed_plan: Arc<Mutex<Option<CompiledAtlasPlanV1>>>,
}

impl CapturingAtlasRenderExecutor {
    fn captured_plan(&self) -> Option<CompiledAtlasPlanV1> {
        self.observed_plan
            .lock()
            .expect("executor plan capture should be lockable")
            .clone()
    }
}

fn finalize_base_plan() -> CompiledAtlasPlanV1 {
    let mut plan = base_plan();
    plan.final_plan_hash = finalize(plan.clone());
    plan
}

fn prepared_source_input() -> AtlasRenderExecutionInput<'static> {
    AtlasRenderExecutionInput {
        prepared_sources: Vec::new(),
        source_frame_cache: None,
    }
}

fn capturing_executor_output() -> AtlasRenderExecutorOutput {
    AtlasRenderExecutorOutput::default()
}

impl AtlasRenderExecutor for CapturingAtlasRenderExecutor {
    fn execute(
        &self,
        plan: &CompiledAtlasPlanV1,
        _input: &AtlasRenderExecutionInput<'_>,
        cancellation: &hot_trimmer_domain::CancellationToken,
        is_current: &dyn Fn() -> bool,
    ) -> Result<AtlasRenderExecutorOutput, AtlasRenderExecutionError> {
        *self
            .observed_plan
            .lock()
            .expect("executor plan capture should be lockable") = Some(plan.clone());
        if cancellation.is_cancelled() {
            return Err(AtlasRenderExecutionError::Cancelled);
        }
        if !is_current() {
            return Err(AtlasRenderExecutionError::Superseded);
        }
        Ok(capturing_executor_output())
    }

    fn compose(
        &self,
        _input: &AtlasComposeExecutionInput<'_>,
        cancellation: &hot_trimmer_domain::CancellationToken,
        is_current: &dyn Fn() -> bool,
    ) -> Result<AtlasComposeExecutorOutput, AtlasRenderExecutionError> {
        if cancellation.is_cancelled() {
            return Err(AtlasRenderExecutionError::Cancelled);
        }
        if !is_current() {
            return Err(AtlasRenderExecutionError::Superseded);
        }
        Err(AtlasRenderExecutionError::Composition(
            "capturing executor does not compose atlas artifacts".into(),
        ))
    }
}

fn striped_source(width: u32, height: u32) -> Vec<u8> {
    let mut image = RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            image.put_pixel(
                x,
                y,
                Rgba([
                    40_u8.saturating_add((x / 2) as u8),
                    80_u8.saturating_add((y / 3) as u8),
                    120_u8.saturating_add(((x + y) / 4) as u8),
                    255,
                ]),
            );
        }
    }
    let mut encoded = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut encoded, ImageFormat::Png)
        .expect("encode behavior fixture");
    encoded.into_inner()
}

fn telemetry_value<'a>(telemetry: &'a str, key: &str) -> Option<&'a str> {
    telemetry
        .split(';')
        .map(str::trim)
        .find_map(|entry| entry.strip_prefix(key)?.strip_prefix('='))
}

fn empty_compose_topology() -> hot_trimmer_domain::CompiledTemplateTopology {
    hot_trimmer_domain::CompiledTemplateTopology {
        identity: hot_trimmer_domain::TemplateIdentity {
            template_id: "compose-guard".into(),
            template_version: "test".into(),
            compatibility_key: "compose-guard".into(),
        },
        output_size: PixelSize {
            width: 1,
            height: 1,
        },
        slots: Vec::new(),
    }
}

fn empty_compose_placement() -> PlacementPlan {
    PlacementPlan {
        stage_result: StageResult::Executed {
            algorithm: AlgorithmProvenance {
                algorithm_id: "compose-guard".into(),
                version: "test".into(),
            },
            settings_hash: ContentDigest::sha256(b"compose-guard"),
            diagnostics: Vec::new(),
        },
        solver: AlgorithmProvenance {
            algorithm_id: "compose-guard".into(),
            version: "test".into(),
        },
        seed: 0,
        placements: Vec::new(),
        objective: PlacementObjectiveBreakdown {
            unary_cost: 0.0,
            pairwise_cost: 0.0,
            pairwise_lambda: 0.0,
            weighted_pairwise_cost: 0.0,
            total_cost: 0.0,
        },
        pairwise_decisions: Vec::new(),
        crop_reuse_heatmap: Vec::new(),
        validation: PlacementValidationSummary {
            complete_assignment: true,
            required_slots_present: true,
            isotropic_scale_only: true,
            registered_mapping_only: true,
            slot_count: 0,
        },
        qa_views: vec![PlacementPlanQaView::Validation],
    }
}

fn empty_compose_request<'a>(
    topology: &'a hot_trimmer_domain::CompiledTemplateTopology,
    placement: &'a PlacementPlan,
) -> IntermediateAtlasRequest<'a> {
    IntermediateAtlasRequest {
        topology,
        placement_plan: placement,
        slots: Vec::new(),
        revision: 7,
        algorithm_versions: BTreeMap::new(),
        diagnostics: Vec::new(),
        regions: Vec::new(),
    }
}

#[test]
fn atlas_render_executor_contract_receives_exact_compiled_plan() {
    let plan = finalize_base_plan();
    let input = prepared_source_input();
    let executor = CapturingAtlasRenderExecutor::default();
    let cancellation = hot_trimmer_domain::CancellationToken::new();

    let output = executor
        .execute(&plan, &input, &cancellation, &|| true)
        .expect("executor contract path should return a test artifact");
    let observed = executor
        .captured_plan()
        .expect("exact plan should be received by executor abstraction");

    assert_eq!(observed, plan);
    assert!(output.as_cpu_regions().expect("capturing executor returns CPU-region output").regions.is_empty());
}

#[test]
fn atlas_render_executor_contract_propagates_cancelled_token() {
    let plan = finalize_base_plan();
    let input = prepared_source_input();
    let executor = CapturingAtlasRenderExecutor::default();
    let cancellation = hot_trimmer_domain::CancellationToken::new();
    cancellation.cancel();

    let error = executor
        .execute(&plan, &input, &cancellation, &|| true)
        .expect_err("cancelled execution should return a cancellation contract error");

    assert!(matches!(error, AtlasRenderExecutionError::Cancelled));
    assert_eq!(
        executor.captured_plan(),
        Some(plan),
        "executor should still receive the submitted plan while cancelled",
    );
}

#[test]
fn compiled_atlas_plan_identity() {
    let plan = base_plan();
    let base_hash = finalize(plan.clone());

    assert_eq!(base_hash, finalize(plan.clone()));

    let mut crop_mutation = plan.clone();
    crop_mutation.ordered_regions[0].source_crop.0.width += 1;
    assert_ne!(base_hash, finalize(crop_mutation));

    let mut destination_mutation = plan.clone();
    destination_mutation.ordered_regions[0].destination_rect.0.x += 1;
    assert_ne!(base_hash, finalize(destination_mutation));

    let mut digest_mutation = plan.clone();
    digest_mutation.ordered_sources[0].digest = ContentDigest::sha256(b"other source bytes");
    assert_ne!(base_hash, finalize(digest_mutation));

    let mut mode_mutation = plan.clone();
    mode_mutation.ordered_regions[0].sampling = RegionSampling::LoopX;
    assert_ne!(base_hash, finalize(mode_mutation));

    let mut radial_mutation = plan.clone();
    let mutated_radial = RadialMappingSettings {
        center_x: 0.55,
        center_y: 0.45,
        inner_radius: 0.05,
        outer_radius: 0.48,
        falloff: 1.1,
        blend_width: 0.0,
        seam_blend_width: 0.0,
    };
    radial_mutation.ordered_regions[0].radial_parameters = Some(mutated_radial);
    radial_mutation.ordered_regions[0].sampling_plan.radial_mapping = Some(mutated_radial);
    assert_ne!(base_hash, finalize(radial_mutation));

    let mut output_size_mutation = plan.clone();
    output_size_mutation.output_size = PixelSize {
        width: 2048,
        height: 2048,
    };
    assert_ne!(base_hash, finalize(output_size_mutation));

    let mut requested_map_mutation = plan.clone();
    requested_map_mutation.requested_maps = vec![MaterialMapKind::Height, MaterialMapKind::BaseColor];
    assert_ne!(base_hash, finalize(requested_map_mutation));

    let mut decoder_version_mutation = plan.clone();
    decoder_version_mutation.ordered_sources[0].decoder_version = "decoder-1+".into();
    assert_ne!(base_hash, finalize(decoder_version_mutation));

    let mut color_version_mutation = plan.clone();
    color_version_mutation.ordered_sources[0].color_version = "color-1+".into();
    assert_ne!(base_hash, finalize(color_version_mutation));

    let mut order_mutation = plan.clone();
    order_mutation.ordered_regions.swap(0, 1);
    assert_ne!(base_hash, finalize(order_mutation));

    let mut unsupported_patch_sampling = base_plan();
    let unsupported_radial = RadialMappingSettings {
        center_x: 0.5, center_y: 0.5, inner_radius: 0.1, outer_radius: 0.4,
        falloff: 1.0, blend_width: 0.0, seam_blend_width: 0.0,
    };
    unsupported_patch_sampling.ordered_regions[0].radial_parameters = Some(unsupported_radial);
    unsupported_patch_sampling.ordered_regions[0].sampling_plan.radial_mapping = Some(unsupported_radial);
    unsupported_patch_sampling.ordered_regions[0].sampling = RegionSampling::LoopY;
    assert!(matches!(
        unsupported_patch_sampling.finalize(),
        Err(CompiledAtlasPlanValidationError::UnsupportedBindingSamplingPair { .. })
    ));
}

#[test]
fn compiled_atlas_plan_from_persisted_preserves_exact_commands() {
    let mut expected = base_plan();
    expected.ordered_regions[1].radial_parameters = None;

    let mut loop_xy = expected.ordered_regions[1].clone();
    loop_xy.region_id = RegionId::new();
    loop_xy.compact_index = 3;
    loop_xy.sampling = RegionSampling::LoopXy;
    loop_xy.sampling_plan.slot_id = loop_xy.region_id;
    loop_xy.sampling_plan.candidate.slot_id = loop_xy.region_id;
    loop_xy.sampling_plan.candidate.candidate_id = ContentDigest::sha256(loop_xy.region_id.to_string().as_bytes());
    expected.ordered_regions.push(loop_xy);

    let mut radial = expected.ordered_regions[0].clone();
    radial.region_id = RegionId::new();
    radial.compact_index = 4;
    let radial_settings = RadialMappingSettings {
        center_x: 0.5,
        center_y: 0.5,
        inner_radius: 0.05,
        outer_radius: 0.48,
        falloff: 1.0,
        blend_width: 0.02,
        seam_blend_width: 0.0,
    };
    radial.radial_parameters = Some(radial_settings);
    radial.sampling_plan.slot_id = radial.region_id;
    radial.sampling_plan.candidate.slot_id = radial.region_id;
    radial.sampling_plan.candidate.candidate_id = ContentDigest::sha256(radial.region_id.to_string().as_bytes());
    radial.sampling_plan.radial_mapping = Some(radial_settings);
    expected.ordered_regions.push(radial);

    let plan = compiled_atlas_plan_from_persisted(
        expected.document_revision,
        expected.request_generation,
        expected.topology_hash,
        expected.appearance_hash,
        expected.output_size,
        SourceFramePreviewProfile::Authoritative,
        expected.ordered_sources.clone(),
        expected.ordered_regions.clone(),
    )
    .expect("persisted commands should finalize into a plan");

    assert_eq!(plan.ordered_sources, expected.ordered_sources);
    assert_eq!(plan.ordered_regions, expected.ordered_regions);
    assert_eq!(plan.ordered_regions[0].sampling, RegionSampling::OneShot);
    assert_eq!(plan.ordered_regions[1].sampling, RegionSampling::LoopX);
    assert_eq!(plan.ordered_regions[2].sampling, RegionSampling::LoopY);
    assert_eq!(plan.ordered_regions[3].sampling, RegionSampling::LoopXy);
    assert!(plan.ordered_regions[4].radial_parameters.is_some());
    assert!(!plan.final_plan_hash.0.is_empty());
}

#[test]
fn gpu_execution_contract_production_compile_persisted_uses_cpu_executor() {
    let root = std::env::temp_dir().join(format!("hot-trimmer-plan-contract-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create behavior contract directory");
    let project_path = root.join("source-frame-contract.hottrimmer");
    let mut store = ProjectStore::create(&project_path, "Manual Region Behavior").expect("create behavior project");

    let encoded = striped_source(128, 128);
    let summary = store.summary().expect("project summary");
    let source_set_id = summary.source_sets[0].id;
    let input = SourceInput {
        id: SourceId::new(),
        ownership: SourceOwnership::OwnedCopy,
        external_path: None,
        origin_path: PathBuf::from("contract-source.png"),
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
    };
    store
        .replace_source_in_set(Uuid::from_bytes(source_set_id.to_bytes()), SourceChannel::BaseColor, &input)
        .expect("register contract source");
    store
        .create_source_frame_document()
        .expect("create source-frame document");
    store
        .execute_document_command(&TrimSheetDocumentCommand::SetOutputResolution {
            output_size: PixelSize {
                width: 128,
                height: 128,
            },
        })
        .expect("set plan contract output resolution");

    let document = store.document().expect("contract document").clone();
    let region_id = document.topology.regions[0].id;
    let document = document
        .apply_command(&TrimSheetDocumentCommand::SetRegionContent {
            region_id,
            content: hot_trimmer_domain::ContentReference::MaterialSource(
                document.primary_material.expect("primary"),
            ),
        })
        .expect("assign baseline material to the plan contract region");

    let mut direct = RegionBehavior::default();
    direct.synchronize_derived_fields();
    let direct_document = document
        .apply_command(&TrimSheetDocumentCommand::SetRegionBehavior {
            region_id,
            behavior: direct,
        })
        .expect("set direct behavior");
    let direct_artifact = compile_source_frame_document(&store, &direct_document);
    assert!(direct_artifact.telemetry.iter().any(|line| {
        line.contains("executor=cpu") && line.contains("plan_hash=")
    }), "production compile_persisted must publish the executor and immutable plan identity");
    let direct_slot = direct_artifact
        .slots
        .iter()
        .find(|slot| slot.region_id == region_id)
        .expect("direct slot");
    assert_eq!(direct_slot.requested_sampling, RegionSampling::OneShot);
    assert_eq!(direct_slot.executed_mode, SamplingMode::DirectCrop);

    let mut loop_x = RegionBehavior::default();
    loop_x.sampling = RegionSampling::LoopX;
    loop_x.period_pixels = Some([16, 16]);
    loop_x.synchronize_derived_fields();
    let loop_x_document = document
        .apply_command(&TrimSheetDocumentCommand::SetRegionBehavior {
            region_id,
            behavior: loop_x,
        })
        .expect("set loop-x behavior");
    let loop_x_artifact = compile_source_frame_document(&store, &loop_x_document);
    let loop_x_slot = loop_x_artifact
        .slots
        .iter()
        .find(|slot| slot.region_id == region_id)
        .expect("loop-x slot");
    assert_eq!(loop_x_slot.requested_sampling, RegionSampling::LoopX);
    assert_eq!(loop_x_slot.executed_mode, SamplingMode::RepeatX);

    let mut loop_y = RegionBehavior::default();
    loop_y.sampling = RegionSampling::LoopY;
    loop_y.period_pixels = Some([16, 16]);
    loop_y.synchronize_derived_fields();
    let loop_y_document = document
        .apply_command(&TrimSheetDocumentCommand::SetRegionBehavior {
            region_id,
            behavior: loop_y,
        })
        .expect("set loop-y behavior");
    let loop_y_artifact = compile_source_frame_document(&store, &loop_y_document);
    let loop_y_slot = loop_y_artifact
        .slots
        .iter()
        .find(|slot| slot.region_id == region_id)
        .expect("loop-y slot");
    assert_eq!(loop_y_slot.requested_sampling, RegionSampling::LoopY);
    assert_eq!(loop_y_slot.executed_mode, SamplingMode::RepeatY);

    let mut radial = RegionBehavior::new(ManualRegionRole::Radial);
    radial.radial = Some(RadialMappingSettings {
        center_x: 0.5,
        center_y: 0.5,
        inner_radius: 0.05,
        outer_radius: 0.48,
        falloff: 1.0,
        blend_width: 0.02,
        seam_blend_width: 0.0,
    });
    radial.synchronize_derived_fields();
    let radial_document = document
        .apply_command(&TrimSheetDocumentCommand::SetRegionBehavior {
            region_id,
            behavior: radial,
        })
        .expect("set radial behavior");
    let radial_artifact = compile_source_frame_document(&store, &radial_document);
    let radial_slot = radial_artifact
        .slots
        .iter()
        .find(|slot| slot.region_id == region_id)
        .expect("radial slot");
    assert_eq!(radial_slot.requested_sampling, RegionSampling::OneShot);
    assert_eq!(radial_slot.executed_mode, SamplingMode::PolarRadial);
    assert!(radial_slot.source_crop.is_some());
}

#[test]
fn gpu_executor_owns_base_color_composition() {
    let root = std::env::temp_dir().join(format!("hot-trimmer-compose-contract-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create composition contract directory");
    let project_path = root.join("source-frame-compose.hottrimmer");
    let mut store = ProjectStore::create(&project_path, "Executor Composition").expect("create composition project");

    let encoded = striped_source(64, 64);
    let summary = store.summary().expect("project summary");
    let source_set_id = summary.source_sets[0].id;
    let input = SourceInput {
        id: SourceId::new(),
        ownership: SourceOwnership::OwnedCopy,
        external_path: None,
        origin_path: PathBuf::from("composition-source.png"),
        sha256: ContentDigest::sha256(&encoded).0,
        width: 64,
        height: 64,
        format: "PNG".into(),
        color_type: "Rgba8".into(),
        has_alpha: true,
        exif_orientation: 1,
        has_embedded_icc_profile: false,
        encoded_bytes: encoded.len() as u64,
        owned_bytes: Some(encoded),
    };
    store
        .replace_source_in_set(Uuid::from_bytes(source_set_id.to_bytes()), SourceChannel::BaseColor, &input)
        .expect("register composition source");
    store
        .create_source_frame_document()
        .expect("create composition source-frame document");
    store
        .execute_document_command(&TrimSheetDocumentCommand::SetOutputResolution {
            output_size: PixelSize {
                width: 64,
                height: 64,
            },
        })
        .expect("set composition output resolution");

    let document = store.document().expect("composition document").clone();
    let region_id = document.topology.regions[0].id;
    let document = document
        .apply_command(&TrimSheetDocumentCommand::SetRegionContent {
            region_id,
            content: hot_trimmer_domain::ContentReference::MaterialSource(
                document.primary_material.expect("primary"),
            ),
        })
        .expect("assign composition material");
    let mut direct = RegionBehavior::default();
    direct.synchronize_derived_fields();
    let document = document
        .apply_command(&TrimSheetDocumentCommand::SetRegionBehavior {
            region_id,
            behavior: direct,
        })
        .expect("set composition direct behavior");

    clear_cpu_atlas_executor_plan_capture();
    let artifact = compile_source_frame_document(&store, &document);
    let captured_plan = captured_cpu_atlas_executor_plan()
        .expect("CPU executor should capture the compiled plan before execution");
    let telemetry = artifact
        .telemetry
        .iter()
        .find(|line| line.contains("executor=cpu") && line.contains("compose_executor=cpu"))
        .expect("production compile_persisted should publish executor-owned composition telemetry");
    assert!(
        !captured_plan.final_plan_hash.0.is_empty(),
        "the captured CompiledAtlasPlanV1 must have a real identity",
    );
    assert_eq!(
        telemetry_value(telemetry, "plan_hash"),
        Some(captured_plan.final_plan_hash.0.as_str()),
    );
    assert!(telemetry.contains("output=64x64"));
    assert!(telemetry.contains("compose_ms="));
    assert_eq!(artifact.channels[0].rgba8.len(), 64 * 64 * 4);
    assert_eq!(
        ContentDigest::sha256(&artifact.channels[0].rgba8).0,
        "e226611667fc2fa8d52355677346a7846553b4cab4484e1bcfffabf39ea5687e",
        "Base Color pixels must match the fixed CPU composition golden for this persisted document",
    );

    let compose_plan = finalize_base_plan();
    let compose_topology = empty_compose_topology();
    let compose_placement = empty_compose_placement();
    let compose_request = empty_compose_request(&compose_topology, &compose_placement);
    let compose_input = AtlasComposeExecutionInput {
        plan: &compose_plan,
        request: &compose_request,
    };
    let compose_executor = CpuAtlasRenderExecutor;
    let cancelled = hot_trimmer_domain::CancellationToken::new();
    cancelled.cancel();
    let cancelled_result = compose_executor.compose(&compose_input, &cancelled, &|| true);
    assert!(
        matches!(cancelled_result, Err(AtlasRenderExecutionError::Cancelled)),
        "cancelled composition must not publish an artifact",
    );

    let stale_result =
        compose_executor.compose(&compose_input, &hot_trimmer_domain::CancellationToken::new(), &|| false);
    assert!(
        matches!(stale_result, Err(AtlasRenderExecutionError::Superseded)),
        "stale composition must not publish an artifact",
    );
}
