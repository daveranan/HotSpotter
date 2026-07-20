use std::{io::Cursor, path::PathBuf};

use hot_trimmer_domain::{
    generate_partition, resolve_boundaries, ContentDigest, LogicalGridSpec, MaterialMapContent,
    MaterialMapKind, MaterialSourceSet, OrientedPixelSize, PartitionRecipe, PixelBounds, PixelSize,
    SamplingMode, SourceFrame, SourceId, TrimSheetDocument, TrimSheetDocumentCommand, NormalizedBounds,
    NormalizedScalar, source_frame_region_id,
    ManualRegionRole, QuarterTurn, RegionBehavior, RegionContinuity, RegionSampling,
};
use hot_trimmer_placement_solver::MirrorTransform;
use hot_trimmer_project_store::{ProjectStore, SourceChannel, SourceInput, SourceOwnership};
use hot_trimmer_domain::CancellationToken;
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use uuid::Uuid;
use std::sync::Mutex;

fn striped_source(width: u32, height: u32) -> (Vec<u8>, Vec<u8>) {
    let mut image = RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            image.put_pixel(x, y, Rgba([
                48_u8.saturating_add(((x / 3 + y / 7) * 17) as u8),
                32_u8.saturating_add(((x / 5) * 23) as u8),
                64_u8.saturating_add(((y / 4) * 31) as u8),
                255,
            ]));
        }
    }
    let raw = image.as_raw().clone();
    let mut encoded = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image).write_to(&mut encoded, ImageFormat::Png).expect("encode striped fixture");
    (encoded.into_inner(), raw)
}

fn numbered_source() -> (Vec<u8>, Vec<u8>) {
    let width = 128_u32;
    let height = 64_u32;
    let mut image = RgbaImage::new(width, height);
    let glyphs: [[u8; 15]; 10] = [
        [1, 1, 1, 1, 0, 1, 1, 0, 1, 1, 0, 1, 1, 1, 1],
        [0, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1, 0, 1, 1, 1],
        [1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 0, 0, 1, 1, 1],
        [1, 1, 1, 0, 0, 1, 1, 1, 1, 0, 0, 1, 1, 1, 1],
        [1, 0, 1, 1, 0, 1, 1, 1, 1, 0, 0, 1, 0, 0, 1],
        [1, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 1],
        [1, 1, 1, 1, 0, 0, 1, 1, 1, 1, 0, 1, 1, 1, 1],
        [1, 1, 1, 0, 0, 1, 0, 1, 0, 1, 0, 0, 1, 0, 0],
        [1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1],
        [1, 1, 1, 1, 0, 1, 1, 1, 1, 0, 0, 1, 1, 1, 1],
    ];
    for y in 0..height {
        for x in 0..width {
            let cell_x = x / 16;
            let cell_y = y / 16;
            let cell = cell_y * 8 + cell_x;
            let color = [
                32_u8.saturating_add((cell * 29) as u8),
                32_u8.saturating_add((cell * 47) as u8),
                32_u8.saturating_add((cell * 71) as u8),
                255,
            ];
            let local_x = (x % 16) / 3;
            let local_y = (y % 16) / 3;
            let digit = (cell % 10) as usize;
            let marked = local_x < 3 && local_y < 5 && glyphs[digit][(local_y * 3 + local_x) as usize] == 1;
            image.put_pixel(x, y, if marked { Rgba([255, 255, 255, 255]) } else { Rgba(color) });
        }
    }
    let raw = image.as_raw().clone();
    let mut encoded = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image).write_to(&mut encoded, ImageFormat::Png).expect("encode fixture");
    (encoded.into_inner(), raw)
}

fn compile_behavior_document(
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
                profile: hot_trimmer_sheet_compiler::SourceFramePreviewProfile::Authoritative,
                view_intent: None,
            },
            &CancellationToken::new(),
            || true,
        )
        .expect("manual behavior compile")
}

#[test]
fn source_frame_exact_viewport_material_map_rehashes_after_tile_request_change() {
    let root = std::env::temp_dir().join(format!("hot-trimmer-source-frame-viewport-hash-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create viewport hash fixture directory");
    let project_path = root.join("viewport-hash.hottrimmer");
    let mut store = ProjectStore::create(&project_path, "Viewport Hash").expect("create viewport hash project");
    let (encoded, _) = striped_source(64, 64);
    let initial = store.summary().expect("initial summary");
    let source_set_id = Uuid::from_bytes(initial.source_sets[0].id.to_bytes());
    let input = SourceInput {
        id: SourceId::new(), ownership: SourceOwnership::OwnedCopy, external_path: None,
        origin_path: PathBuf::from("viewport-hash.png"), sha256: ContentDigest::sha256(&encoded).0,
        width: 64, height: 64, format: "PNG".into(), color_type: "Rgba8".into(), has_alpha: true,
        exif_orientation: 1, has_embedded_icc_profile: false, encoded_bytes: encoded.len() as u64,
        owned_bytes: Some(encoded),
    };
    store.replace_source_in_set(source_set_id, SourceChannel::BaseColor, &input).expect("register viewport hash source");
    store.create_source_frame_document().expect("create viewport hash document");
    store.execute_document_command(&TrimSheetDocumentCommand::SetOutputResolution { output_size: PixelSize { width: 64, height: 64 } }).expect("set viewport hash output");
    let summary = store.summary().expect("viewport hash summary");
    let revision = summary.document.as_ref().expect("viewport hash document").document_revision;
    let artifact = hot_trimmer_sheet_compiler::AlgorithmCompiler::new()
        .compile_persisted_stage_14_preview(
            hot_trimmer_sheet_compiler::PersistedStage14PreviewRequest {
                project: &summary,
                revision,
                draft_id: Some(17),
                input_hash: Some("viewport-hash-regression".into()),
                profile: hot_trimmer_sheet_compiler::SourceFramePreviewProfile::Authoritative,
                view_intent: Some(hot_trimmer_sheet_compiler::SourceFramePreviewViewIntent::ExactViewportMaterialMaps {
                    rect: hot_trimmer_sheet_compiler::OutputPixelRect(PixelBounds { x: 8, y: 8, width: 24, height: 24 }),
                    maps: vec![MaterialMapKind::BaseColor],
                }),
            },
            &CancellationToken::new(),
            || true,
        )
        .expect("exact viewport material-map compile should rehash after tile mutation");
    assert!(artifact.telemetry.iter().any(|entry| entry.contains("maps=BaseColor")));
    drop(store);
    std::fs::remove_dir_all(root).expect("remove viewport hash fixture directory");
}

#[test]
fn source_frame_preview_retries_after_companion_map_revision_refresh() {
    let root = std::env::temp_dir().join(format!("hot-trimmer-source-frame-refresh-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create source frame refresh fixture directory");
    let project_path = root.join("source-frame-refresh.hottrimmer");
    let mut store = ProjectStore::create(&project_path, "Source Frame Refresh").expect("create source frame refresh project");
    let (base_encoded, _) = striped_source(64, 64);
    let initial = store.summary().expect("initial source frame refresh summary");
    let source_set_id = Uuid::from_bytes(initial.source_sets[0].id.to_bytes());
    let base_input = SourceInput {
        id: SourceId::new(), ownership: SourceOwnership::OwnedCopy, external_path: None,
        origin_path: PathBuf::from("source-frame-refresh-base.png"), sha256: ContentDigest::sha256(&base_encoded).0,
        width: 64, height: 64, format: "PNG".into(), color_type: "Rgba8".into(), has_alpha: true,
        exif_orientation: 1, has_embedded_icc_profile: false, encoded_bytes: base_encoded.len() as u64,
        owned_bytes: Some(base_encoded),
    };
    store.replace_source_in_set(source_set_id, SourceChannel::BaseColor, &base_input).expect("register base source");
    store.create_source_frame_document().expect("create source frame document");
    store.execute_document_command(&TrimSheetDocumentCommand::SetOutputResolution { output_size: PixelSize { width: 64, height: 64 } }).expect("set source frame refresh output");
    let stale_summary = store.summary().expect("stale source frame refresh summary");
    let stale_revision = stale_summary.document.as_ref().expect("stale source frame refresh document").document_revision;
    let (rough_encoded, _) = striped_source(64, 64);
    let rough_input = SourceInput {
        id: SourceId::new(), ownership: SourceOwnership::OwnedCopy, external_path: None,
        origin_path: PathBuf::from("source-frame-refresh-rough.png"), sha256: ContentDigest::sha256(&rough_encoded).0,
        width: 64, height: 64, format: "PNG".into(), color_type: "Rgba8".into(), has_alpha: true,
        exif_orientation: 1, has_embedded_icc_profile: false, encoded_bytes: rough_encoded.len() as u64,
        owned_bytes: Some(rough_encoded),
    };
    store.replace_source_in_set(source_set_id, SourceChannel::Roughness, &rough_input).expect("register companion source");
    let _ = store.refresh_document_assets().expect("refresh document assets");
    let refreshed_summary = store.summary().expect("refreshed source frame summary");
    let refreshed_revision = refreshed_summary.document.as_ref().expect("refreshed source frame document").document_revision;
    assert!(refreshed_revision >= stale_revision);
    let artifact = hot_trimmer_sheet_compiler::AlgorithmCompiler::new()
        .compile_persisted_stage_14_preview(
            hot_trimmer_sheet_compiler::PersistedStage14PreviewRequest {
                project: &refreshed_summary,
                revision: refreshed_revision,
                draft_id: Some(24),
                input_hash: Some("source-frame-refresh-regression-refreshed".into()),
                profile: hot_trimmer_sheet_compiler::SourceFramePreviewProfile::Authoritative,
                view_intent: Some(hot_trimmer_sheet_compiler::SourceFramePreviewViewIntent::ExactViewportMaterialMaps {
                    rect: hot_trimmer_sheet_compiler::OutputPixelRect(PixelBounds { x: 0, y: 0, width: 32, height: 32 }),
                    maps: vec![MaterialMapKind::BaseColor],
                }),
            },
            &CancellationToken::new(),
            || true,
        )
        .expect("source frame preview should succeed after asset refresh");
    assert!(artifact.telemetry.iter().any(|entry| entry.contains("maps=BaseColor")));
    drop(store);
    std::fs::remove_dir_all(root).expect("remove source frame refresh fixture directory");
}

fn selected_region_pixels(
    artifact: &hot_trimmer_sheet_compiler::IntermediateAtlasArtifact,
    region_id: hot_trimmer_domain::RegionId,
) -> Vec<u8> {
    let slot = artifact.slots.iter().find(|slot| slot.region_id == region_id).expect("selected slot");
    let base = artifact.channels.iter().find(|channel| channel.role == hot_trimmer_domain::MaterialChannelRole::BaseColor).expect("Base Color");
    let width = artifact.topology.output_size.width;
    let mut pixels = Vec::new();
    for y in slot.allocation.y..slot.allocation.y + slot.allocation.height {
        for x in slot.allocation.x..slot.allocation.x + slot.allocation.width {
            let index = ((y * width + x) * 4) as usize;
            pixels.extend_from_slice(&base.rgba8[index..index + 4]);
        }
    }
    pixels
}

#[test]
fn manual_region_behavior_compile_persisted_executes_modes_edges_radial_and_persistence() {
    let root = std::env::temp_dir().join(format!("hot-trimmer-manual-region-behavior-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create behavior fixture directory");
    let project_path = root.join("manual-region-behavior.hottrimmer");
    let mut store = ProjectStore::create(&project_path, "Manual Region Behavior").expect("create behavior project");
    let (encoded, _) = numbered_source();
    let initial = store.summary().expect("initial summary");
    let source_set_id = Uuid::from_bytes(initial.source_sets[0].id.to_bytes());
    let input = SourceInput {
        id: SourceId::new(), ownership: SourceOwnership::OwnedCopy, external_path: None,
        origin_path: PathBuf::from("manual-numbered.png"), sha256: ContentDigest::sha256(&encoded).0,
        width: 128, height: 64, format: "PNG".into(), color_type: "Rgba8".into(), has_alpha: true,
        exif_orientation: 1, has_embedded_icc_profile: false, encoded_bytes: encoded.len() as u64,
        owned_bytes: Some(encoded),
    };
    store.replace_source_in_set(source_set_id, SourceChannel::BaseColor, &input).expect("register behavior source");
    store.create_source_frame_document().expect("create authored behavior document");
    store.execute_document_command(&TrimSheetDocumentCommand::SetOutputResolution { output_size: PixelSize { width: 64, height: 64 } }).expect("small behavior output");
    let baseline_document = store.document().expect("baseline document").clone();
    let baseline = compile_behavior_document(&store, &baseline_document);
    assert!(baseline.slots.iter().all(|slot| slot.requested_sampling == RegionSampling::OneShot
        && slot.executed_mode == SamplingMode::DirectCrop && slot.period_pixels.is_none()), "defaults never repeat");

    let region_id = baseline_document.topology.regions[0].id;
    let whole_source = baseline_document.apply_command(&TrimSheetDocumentCommand::SetRegionContent {
        region_id,
        content: hot_trimmer_domain::ContentReference::MaterialSource(baseline_document.primary_material.expect("primary")),
    }).expect("assign whole numbered source");
    let mut artifacts = Vec::new();
    for sampling in [RegionSampling::OneShot, RegionSampling::LoopX, RegionSampling::LoopY, RegionSampling::LoopXy] {
        let mut behavior = RegionBehavior::default();
        behavior.sampling = sampling;
        behavior.period_pixels = (sampling != RegionSampling::OneShot).then_some([17, 11]);
        behavior.synchronize_derived_fields();
        let document = whole_source.apply_command(&TrimSheetDocumentCommand::SetRegionBehavior { region_id, behavior: behavior.clone() }).expect("author supported behavior");
        let artifact = compile_behavior_document(&store, &document);
        let slot = artifact.slots.iter().find(|slot| slot.region_id == region_id).expect("behavior slot");
        let expected = match sampling { RegionSampling::OneShot => SamplingMode::DirectCrop, RegionSampling::LoopX => SamplingMode::RepeatX, RegionSampling::LoopY => SamplingMode::RepeatY, RegionSampling::LoopXy => SamplingMode::PeriodicTile };
        assert_eq!(slot.requested_sampling, sampling);
        assert_eq!(slot.executed_mode, expected);
        assert_eq!(slot.mapping_mode, expected);
        artifacts.push((sampling, selected_region_pixels(&artifact, region_id), artifact));
    }
    for left in 0..artifacts.len() {
        for right in left + 1..artifacts.len() {
            assert_ne!(artifacts[left].1, artifacts[right].1, "each explicit sampling mode produces known different numbered pixels");
        }
    }

    for (continuity, expected) in [
        (RegionContinuity::X, [false, false, true, true]),
        (RegionContinuity::Y, [true, true, false, false]),
        (RegionContinuity::Xy, [false, false, false, false]),
    ] {
        let mut behavior = RegionBehavior::default();
        behavior.continuity = continuity;
        behavior.synchronize_derived_fields();
        let document = whole_source.apply_command(&TrimSheetDocumentCommand::SetRegionBehavior { region_id, behavior }).expect("author continuity");
        let artifact = compile_behavior_document(&store, &document);
        let edges = artifact.slots.iter().find(|slot| slot.region_id == region_id).expect("edge slot").edge_eligibility;
        assert_eq!([edges.left, edges.right, edges.top, edges.bottom], expected);
    }

    let mut radial = RegionBehavior::new(ManualRegionRole::Radial);
    radial.radial = Some(hot_trimmer_domain::RadialMappingSettings { center_x: 0.35, center_y: 0.6, inner_radius: 0.08, outer_radius: 0.42, falloff: 1.0, blend_width: 0.04, seam_blend_width: 0.03 });
    radial.synchronize_derived_fields();
    let mut moved_center = radial.clone();
    moved_center.radial.as_mut().unwrap().center_x = 0.62;
    moved_center.radial.as_mut().unwrap().center_y = 0.28;
    let mut changed_radii = radial.clone();
    changed_radii.radial.as_mut().unwrap().inner_radius = 0.2;
    changed_radii.radial.as_mut().unwrap().outer_radius = 0.7;
    let mut changed_falloff = radial.clone();
    changed_falloff.radial.as_mut().unwrap().falloff = 2.25;
    let mut changed_seam = radial.clone();
    changed_seam.orientation = QuarterTurn::Ninety;

    let mut radial_artifacts = Vec::new();
    for (label, behavior) in [
        ("base", radial.clone()),
        ("center", moved_center),
        ("radii", changed_radii),
        ("falloff", changed_falloff),
        ("seam", changed_seam),
    ] {
        let document = whole_source.apply_command(&TrimSheetDocumentCommand::SetRegionBehavior { region_id, behavior }).expect("classify radial");
        let artifact = compile_behavior_document(&store, &document);
        let slot = artifact.slots.iter().find(|slot| slot.region_id == region_id).expect("radial slot");
        assert_eq!(slot.executed_mode, SamplingMode::PolarRadial);
        assert_eq!(slot.valid_pixel_count, u64::from(slot.allocation.width) * u64::from(slot.allocation.height), "{label} radial left invalid pixels in its rectangular allocation");
        for before in &baseline.slots {
            let after = artifact.slots.iter().find(|candidate| candidate.region_id == before.region_id).expect("same RegionId after radial edit");
            if before.region_id == region_id {
                assert_ne!(before.stage_14_result_id, after.stage_14_result_id);
            } else {
                assert_eq!(before.stage_14_result_id, after.stage_14_result_id, "{label} radial edit changed another RegionId");
                assert_eq!(selected_region_pixels(&baseline, before.region_id), selected_region_pixels(&artifact, before.region_id), "{label} radial edit changed another region's pixels");
            }
        }
        radial_artifacts.push((label, selected_region_pixels(&artifact, region_id), artifact));
    }
    for left in 0..radial_artifacts.len() {
        for right in left + 1..radial_artifacts.len() {
            assert_ne!(radial_artifacts[left].1, radial_artifacts[right].1,
                "radial {} and {} controls must produce different numbered pixels", radial_artifacts[left].0, radial_artifacts[right].0);
        }
    }

    let ordered_radial_document = whole_source.apply_command(&TrimSheetDocumentCommand::SetRegionBehavior {
        region_id, behavior: radial.clone(),
    }).expect("ordered radial document");
    let ordered_radial_artifact = compile_behavior_document(&store, &ordered_radial_document);
    let mut shuffled_radial_document = ordered_radial_document.clone();
    let mut reversed_regions = shuffled_radial_document.topology.regions.clone();
    reversed_regions.reverse();
    shuffled_radial_document.topology = hot_trimmer_domain::AcceptedTopology::new(
        shuffled_radial_document.topology.kind,
        shuffled_radial_document.topology.snapshot.clone(),
        shuffled_radial_document.topology.compatibility_key.clone(),
        reversed_regions,
    ).expect("rebuild topology in reverse iteration order");
    let shuffled_radial_artifact = compile_behavior_document(&store, &shuffled_radial_document);
    for region in &ordered_radial_document.topology.regions {
        assert_eq!(
            selected_region_pixels(&ordered_radial_artifact, region.id),
            selected_region_pixels(&shuffled_radial_artifact, region.id),
            "reversing region iteration changed pixels assigned to RegionId {}", region.id,
        );
    }

    let mut unsupported = RegionBehavior::new(ManualRegionRole::Unique);
    unsupported.sampling = RegionSampling::LoopX;
    unsupported.synchronize_derived_fields();
    assert!(whole_source.apply_command(&TrimSheetDocumentCommand::SetRegionBehavior { region_id, behavior: unsupported }).is_err(), "unsupported role/sampling cannot reach Stage 14");

    store.execute_document_command(&TrimSheetDocumentCommand::SetRegionContent {
        region_id,
        content: hot_trimmer_domain::ContentReference::MaterialSource(store.document().unwrap().primary_material.unwrap()),
    }).expect("persist whole source");
    store.execute_document_command(&TrimSheetDocumentCommand::SetRegionBehavior { region_id, behavior: radial.clone() }).expect("persist radial behavior");
    store.undo_document_command().expect("undo behavior");
    store.redo_document_command().expect("redo behavior");
    drop(store);
    let reopened = ProjectStore::open(&project_path).expect("reopen behavior project");
    let reopened_document = reopened.document().expect("reopened document");
    assert_eq!(reopened_document.region_bindings[&region_id].mapping.behavior, radial);
    let mut save_as = reopened_document.authored_layout_preset.clone().expect("embedded preset");
    save_as.preset_id = "user.manual-region-behavior".into();
    save_as.regions[0].default_behavior = radial.clone();
    let reopened_save_as: hot_trimmer_domain::AuthoredLayoutPreset = serde_json::from_slice(&serde_json::to_vec(&save_as).unwrap()).unwrap();
    assert_eq!(reopened_save_as.regions[0].default_behavior, radial, "preset Save As preserves every behavior field");
}

#[test]
fn manual_region_behavior_resized_patch_invalidates_only_its_rendered_domain() {
    use hot_trimmer_domain::{
        ContentReference, NormalizedPoint, Patch, PatchCommand, PatchGeometry, PatchId,
        PatchProperties, RectificationSettings,
    };

    let root = std::env::temp_dir().join(format!("hot-trimmer-patch-domain-cache-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create patch cache fixture directory");
    let project_path = root.join("patch-domain-cache.hottrimmer");
    let mut store = ProjectStore::create(&project_path, "Patch Domain Cache").expect("create patch cache project");
    let (encoded, _) = numbered_source();
    let source_set_id = Uuid::from_bytes(store.summary().unwrap().source_sets[0].id.to_bytes());
    let source_id = SourceId::new();
    store.replace_source_in_set(source_set_id, SourceChannel::BaseColor, &SourceInput {
        id: source_id, ownership: SourceOwnership::OwnedCopy, external_path: None,
        origin_path: PathBuf::from("numbered-patch-cache.png"), sha256: ContentDigest::sha256(&encoded).0,
        width: 128, height: 64, format: "PNG".into(), color_type: "Rgba8".into(), has_alpha: true,
        exif_orientation: 1, has_embedded_icc_profile: false, encoded_bytes: encoded.len() as u64,
        owned_bytes: Some(encoded),
    }).expect("register patch cache source");
    store.create_source_frame_document().expect("create patch cache document");
    store.execute_document_command(&TrimSheetDocumentCommand::SetOutputResolution {
        output_size: PixelSize { width: 64, height: 64 },
    }).expect("set patch cache output");

    let square = |left: f64, top: f64| PatchGeometry {
        corners: [
            NormalizedPoint::new(left, top).unwrap(),
            NormalizedPoint::new(left + 0.25, top).unwrap(),
            NormalizedPoint::new(left + 0.25, top + 0.25).unwrap(),
            NormalizedPoint::new(left, top + 0.25).unwrap(),
        ],
        assistance_mask: None,
    };
    let patch_id = PatchId::new();
    store.execute_patch_command(&PatchCommand::Create {
        patch: Patch {
            id: patch_id, source_id, name: "Radial Cache Patch".into(), enabled: true,
            geometry: square(0.0, 0.0), properties: PatchProperties::default(),
            rectification: RectificationSettings::default(),
        },
        index: None,
    }, None).expect("create radial cache patch");
    store.refresh_document_assets().expect("refresh radial cache patch");
    let region_id = store.document().unwrap().topology.regions[0].id;
    store.execute_document_command(&TrimSheetDocumentCommand::SetRegionContent {
        region_id, content: ContentReference::Patch(patch_id),
    }).expect("assign radial cache patch");
    let mut radial = RegionBehavior::new(ManualRegionRole::Radial);
    radial.synchronize_derived_fields();
    store.execute_document_command(&TrimSheetDocumentCommand::SetRegionBehavior { region_id, behavior: radial })
        .expect("classify radial cache patch");

    let compiler = hot_trimmer_sheet_compiler::AlgorithmCompiler::new();
    let cache = Mutex::new(hot_trimmer_sheet_compiler::SourceFramePreviewCache::default());
    let compile = |store: &ProjectStore, input_hash: &str| {
        let summary = store.summary().expect("patch cache summary");
        let revision = summary.document.as_ref().unwrap().document_revision;
        compiler.compile_persisted_stage_14_preview_with_cache(
            hot_trimmer_sheet_compiler::PersistedStage14PreviewRequest {
                project: &summary, revision, draft_id: None, input_hash: Some(input_hash.into()),
                profile: hot_trimmer_sheet_compiler::SourceFramePreviewProfile::Draft512,
                view_intent: None,
            },
            &CancellationToken::new(), || true, Some(&cache),
        ).expect("compile radial patch cache fixture")
    };
    let before = compile(&store, "before-resize");
    store.execute_patch_command(&PatchCommand::ReplaceGeometry {
        patch_id, geometry: square(0.7, 0.7),
    }, Some(1)).expect("move radial cache patch");
    store.refresh_document_assets().expect("refresh moved radial cache patch");
    let after = compile(&store, "after-resize");

    assert_ne!(selected_region_pixels(&before, region_id), selected_region_pixels(&after, region_id),
        "the same PatchId with different authored geometry must not reuse old radial pixels");
    assert!(after.telemetry.iter().any(|line| line.contains("render_cache_hits=23")),
        "only the resized patch region should miss the rendered-region cache");
    drop(store);
    std::fs::remove_dir_all(root).expect("remove patch cache fixture directory");
}

fn fixture_project(target: u32) -> (ProjectStore, Vec<u8>, TrimSheetDocument) {
    fixture_project_with_output(target, PixelSize { width: 64, height: 64 })
}

fn fixture_project_with_output(target: u32, output: PixelSize) -> (ProjectStore, Vec<u8>, TrimSheetDocument) {
    let root = std::env::temp_dir().join(format!("hot-trimmer-source-frame-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create fixture directory");
    let project_path = root.join("source-frame.hottrimmer");
    let mut store = ProjectStore::create(&project_path, "Source Frame E2E").expect("create project");
    let (encoded, raw) = numbered_source();
    let summary = store.summary().expect("empty summary");
    let source_set_id = Uuid::from_bytes(summary.source_sets[0].id.to_bytes());
    let input = SourceInput {
        id: SourceId::new(), ownership: SourceOwnership::OwnedCopy, external_path: None,
        origin_path: PathBuf::from("numbered-8000x4000.png"), sha256: ContentDigest::sha256(&encoded).0,
        width: 128, height: 64, format: "PNG".into(), color_type: "Rgba8".into(), has_alpha: true,
        exif_orientation: 1, has_embedded_icc_profile: false, encoded_bytes: encoded.len() as u64,
        owned_bytes: Some(encoded),
    };
    store.replace_source_in_set(source_set_id, SourceChannel::BaseColor, &input).expect("register fixture");
    let summary = store.summary().expect("registered summary");
    let source_set = &summary.source_sets[0];
    let material = MaterialSourceSet {
        id: source_set.id, name: source_set.name.clone(), maps: vec![MaterialMapContent {
            kind: MaterialMapKind::BaseColor, sha256: input.sha256.clone(),
        }],
    };
    let frame = SourceFrame::centered_largest(source_set.id, OrientedPixelSize { width: 128, height: 64 }, [1, 1], source_set.source_revision);
    let recipe = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, target, 19);
    let document = TrimSheetDocument::from_source_frame(
        hot_trimmer_domain::LayoutId::new(), frame, recipe, output,
        vec![material], Vec::new(),
    ).expect("create source-frame document");
    let mut summary = summary;
    summary.document = Some(document.clone());
    (store, raw, document)
}

#[test]
fn gpu_migration_cpu_baseline_contract() {
    let (store, _source, document) = fixture_project(16);
    let provenance = document.partition_provenance.as_ref().expect("source-frame provenance");
    let direct_region = document.topology.regions[0].id;
    let loop_region = document.topology.regions[1].id;
    let radial_region = document.topology.regions[2].id;
    assert_eq!(
        direct_region,
        source_frame_region_id(&provenance.recipe, document.topology.regions[0].grid_rect.expect("direct grid"), 0),
        "direct region id changed"
    );
    assert_eq!(
        loop_region,
        source_frame_region_id(&provenance.recipe, document.topology.regions[1].grid_rect.expect("loop grid"), 1),
        "loop region id changed"
    );
    assert_eq!(
        radial_region,
        source_frame_region_id(&provenance.recipe, document.topology.regions[2].grid_rect.expect("radial grid"), 2),
        "radial region id changed"
    );

    let mut loop_behavior = RegionBehavior::default();
    loop_behavior.sampling = RegionSampling::LoopX;
    loop_behavior.period_pixels = Some([1, 1]);
    loop_behavior.synchronize_derived_fields();
    let mut radial_behavior = RegionBehavior::new(ManualRegionRole::Radial);
    radial_behavior.synchronize_derived_fields();

    let mut modified = document
        .apply_command(&TrimSheetDocumentCommand::SetRegionBehavior {
            region_id: loop_region,
            behavior: loop_behavior,
        })
        .expect("author loop sampling");
    modified = modified
        .apply_command(&TrimSheetDocumentCommand::SetRegionBehavior {
            region_id: radial_region,
            behavior: radial_behavior,
        })
        .expect("author radial role");

    let compiler = hot_trimmer_sheet_compiler::AlgorithmCompiler::new();
    let cache = Mutex::new(hot_trimmer_sheet_compiler::SourceFramePreviewCache::default());
    let compile = |store: &ProjectStore, document: &TrimSheetDocument, input_hash: &str| {
        let mut summary = store.summary().expect("artifact summary");
        summary.document = Some(document.clone());
        compiler
            .compile_persisted_stage_14_preview_with_cache(
                hot_trimmer_sheet_compiler::PersistedStage14PreviewRequest {
                    project: &summary,
                    revision: document.document_revision,
                    draft_id: None,
                    input_hash: Some(input_hash.to_owned()),
                    profile: hot_trimmer_sheet_compiler::SourceFramePreviewProfile::Authoritative,
                    view_intent: None,
                },
                &CancellationToken::new(),
                || true,
                Some(&cache),
            )
            .expect("compile source-frame artifact")
    };

    let baseline_artifact = compile(&store, &modified, "gpu-migration-cpu-baseline");
    let warm_artifact = compile(&store, &modified, "gpu-migration-cpu-baseline");

    let baseline_base = baseline_artifact
        .channels
        .iter()
        .find(|channel| channel.role == hot_trimmer_domain::MaterialChannelRole::BaseColor)
        .expect("baseline base color");
    let warm_base = warm_artifact
        .channels
        .iter()
        .find(|channel| channel.role == hot_trimmer_domain::MaterialChannelRole::BaseColor)
        .expect("warm base color");
    let baseline_digest = ContentDigest::sha256(&baseline_base.rgba8);
    assert_eq!(baseline_digest.0, "caea75196f8746fa0303e11547c6fbebb85c40d1121b086bd3efdd9357c26ee6");
    assert_eq!(baseline_base.rgba8, warm_base.rgba8);

    let contract_rows = [
        (direct_region, SamplingMode::DirectCrop, RegionSampling::OneShot),
        (loop_region, SamplingMode::RepeatX, RegionSampling::LoopX),
        (radial_region, SamplingMode::PolarRadial, RegionSampling::OneShot),
    ];
    let mut warm_expectations = Vec::with_capacity(contract_rows.len());
    for (region_id, expected_mode, expected_sampling) in contract_rows {
        let slot = baseline_artifact
            .slots
            .iter()
            .find(|slot| slot.region_id == region_id)
            .expect("contract slot");
        assert_eq!(slot.requested_sampling, expected_sampling);
        assert_eq!(slot.mapping_mode, expected_mode);
        assert_eq!(slot.executed_mode, expected_mode);
        assert!(!slot.sampling_plan_id.0.is_empty(), "sampling-plan id is empty");
        assert!(!slot.stage_14_result_id.0.is_empty(), "stage14 id is empty");
        warm_expectations.push((
            region_id,
            slot.source_crop.clone(),
            slot.atlas_destination,
            slot.sampling_plan_id.clone(),
            slot.stage_14_result_id.clone(),
        ));
    }
    for (region_id, expected_crop, expected_destination, expected_sampling_plan_id, expected_stage_14_id) in warm_expectations {
        let warm_slot = warm_artifact
            .slots
            .iter()
            .find(|slot| slot.region_id == region_id)
            .expect("warm slot");
        assert_eq!(expected_sampling_plan_id, warm_slot.sampling_plan_id, "sampling-plan identity changed while warm");
        assert_eq!(expected_stage_14_id, warm_slot.stage_14_result_id, "stage14 identity changed while warm");
        assert_eq!(region_id, warm_slot.region_id, "region id changed while warm");
        assert_eq!(expected_crop, warm_slot.source_crop, "crop changed while warm");
        assert_eq!(expected_destination, warm_slot.atlas_destination, "destination changed while warm");
    }
}

#[test]
fn source_frame_persisted_pipeline_is_pixel_exact_for_accepted_partitions() {
    for target in [16_u32, 63, 103] {
        let (store, source, document) = fixture_project(target);
        let summary = {
            let mut summary = store.summary().expect("summary");
            summary.document = Some(document.clone());
            summary
        };
        let artifact = hot_trimmer_sheet_compiler::AlgorithmCompiler::new()
            .compile_persisted_stage_14_preview(
                hot_trimmer_sheet_compiler::PersistedStage14PreviewRequest {
                    project: &summary, revision: document.document_revision, draft_id: None, input_hash: None,
                    profile: Default::default(),
                    view_intent: None,
                },
                &CancellationToken::new(), || true,
            )
            .unwrap_or_else(|error| panic!("compile {target}: {error}"));
        assert_eq!(artifact.slots.len(), target as usize);
        let frame = document.source_frame.as_ref().expect("frame");
        let grid = document.logical_grid.expect("grid");
        let source_x = resolve_boundaries(32, 64, grid.width);
        let source_y = resolve_boundaries(0, 64, grid.height);
        let expected_regions = generate_partition(&document.partition_provenance.as_ref().expect("provenance").recipe).expect("partition");
        assert_eq!(expected_regions.len(), target as usize);
        let mut coverage = vec![0_u8; 64 * 64];
        for slot in &artifact.slots {
            assert_eq!(slot.mapping_mode, SamplingMode::DirectCrop);
            let rect = slot.grid_rect.expect("published GridRect");
            let crop = slot.source_crop.expect("published direct crop");
            assert_eq!(crop.x, source_x[rect.x as usize]);
            assert_eq!(crop.y, source_y[rect.y as usize]);
            assert_eq!(crop.width, source_x[(rect.x + rect.width) as usize] - crop.x);
            assert_eq!(crop.height, source_y[(rect.y + rect.height) as usize] - crop.y);
            assert_eq!(slot.isotropic_scale, 1.0);
            assert_eq!(slot.source_transform.rotation, hot_trimmer_domain::QuarterTurn::Zero);
            assert_eq!(slot.source_transform.mirror, MirrorTransform::None);
            for y in 0..crop.height { for x in 0..crop.width { coverage[((crop.y + y - 0) * 64 + crop.x + x - 32) as usize] += 1; } }
        }
        assert!(coverage.iter().all(|count| *count == 1), "SourceFrame union is not exact for {target}");
        let base = artifact.channels.iter().find(|channel| channel.role == hot_trimmer_domain::MaterialChannelRole::BaseColor).expect("Base Color atlas");
        for y in 0..64_u32 {
            for x in 0..64_u32 {
                let atlas_index = ((y * 64 + x) * 4) as usize;
                let slot = artifact.slots.iter().find(|slot| {
                    let rect = slot.allocation;
                    x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
                }).expect("atlas slot");
                let crop = slot.source_crop.expect("crop");
                let local_x = x - slot.allocation.x;
                let local_y = y - slot.allocation.y;
                let source_index = (((crop.y + local_y) * 128 + crop.x + local_x) * 4) as usize;
                assert_eq!(&base.rgba8[atlas_index..atlas_index + 4], &source[source_index..source_index + 4], "pixel mismatch at {x},{y} for {target}");
            }
        }
        if target == 63 {
            if let Some(golden) = std::env::var_os("SOURCE_FRAME_GOLDEN").map(PathBuf::from) {
            let image = RgbaImage::from_raw(64, 64, base.rgba8.clone()).expect("atlas dimensions");
            image.save(&golden).expect("write compiler-produced PNG golden");
            }
            if let Some(directory) = std::env::var_os("SOURCE_FRAME_VISUAL_DIR").map(PathBuf::from) {
                let source_image = RgbaImage::from_raw(128, 64, source.clone()).expect("source dimensions");
                source_image.save(directory.join("source-frame-coordinate-fixture-128x64.png")).expect("write source fixture");
                let scale = 4_u32;
                let mut grid_image = RgbaImage::new(128 * scale, 64 * scale);
                for y in 0..64_u32 { for x in 0..128_u32 {
                    let pixel = *source_image.get_pixel(x, y);
                    for oy in 0..scale { for ox in 0..scale { grid_image.put_pixel(x * scale + ox, y * scale + oy, pixel); } }
                } }
                let draw_rect = |image: &mut RgbaImage, x: u32, y: u32, width: u32, height: u32, color: Rgba<u8>| {
                    for px in x..x + width { image.put_pixel(px, y, color); image.put_pixel(px, y + height - 1, color); }
                    for py in y..y + height { image.put_pixel(x, py, color); image.put_pixel(x + width - 1, py, color); }
                };
                draw_rect(&mut grid_image, 32 * scale, 0, 64 * scale, 64 * scale, Rgba([255, 220, 0, 255]));
                for slot in &artifact.slots {
                    let crop = slot.source_crop.expect("crop");
                    draw_rect(&mut grid_image, crop.x * scale, crop.y * scale, crop.width * scale, crop.height * scale, Rgba([255, 64, 64, 255]));
                }
                grid_image.save(directory.join("source-frame-grid-63.png")).expect("write source-frame grid capture");
            }
            assert_eq!(frame.oriented_dimensions, OrientedPixelSize { width: 128, height: 64 });
        }
    }
}

#[test]
fn source_frame_authoring_uses_one_compiled_record_for_move_detach_and_reset() {
    let (store, _source, document) = fixture_project(16);
    let summary = { let mut summary = store.summary().expect("summary"); summary.document = Some(document.clone()); summary };
    let region_id = document.topology.regions[0].id;
    let moved = document.apply_command(&TrimSheetDocumentCommand::SetSourceFrame {
        bounds: NormalizedBounds {
            x: NormalizedScalar::new(0.30).unwrap(), y: NormalizedScalar::new(0.0).unwrap(),
            width: NormalizedScalar::new(0.50).unwrap(), height: NormalizedScalar::new(1.0).unwrap(),
        },
    }).expect("move frame");
    assert_eq!(moved.topology.regions[0].grid_rect, document.topology.regions[0].grid_rect);
    let detached = moved.apply_command(&TrimSheetDocumentCommand::DetachSourceCell { region_id }).expect("detach cell");
    assert_eq!(detached.source_overrides.len(), 1);
    let moved_summary = { let mut summary = summary.clone(); summary.document = Some(moved.clone()); summary };
    let moved_artifact = hot_trimmer_sheet_compiler::AlgorithmCompiler::new()
        .compile_persisted_stage_14_preview(
        hot_trimmer_sheet_compiler::PersistedStage14PreviewRequest { project: &moved_summary, revision: moved.document_revision, draft_id: None, input_hash: None, profile: Default::default(), view_intent: None },
            &CancellationToken::new(), || true,
        ).expect("moved compile");
    assert_eq!(detached.source_overrides[&region_id].source_bounds, moved.source_frame.as_ref().unwrap().region_bounds(moved.logical_grid.unwrap(), moved.topology.regions[0].grid_rect.unwrap()));
    let reset = detached.apply_command(&TrimSheetDocumentCommand::ResetSourceCell { region_id }).expect("reset cell");
    assert!(reset.source_overrides.is_empty());
    let reset_summary = { let mut summary = summary.clone(); summary.document = Some(reset.clone()); summary };
    let restored = hot_trimmer_sheet_compiler::AlgorithmCompiler::new()
        .compile_persisted_stage_14_preview(
        hot_trimmer_sheet_compiler::PersistedStage14PreviewRequest { project: &reset_summary, revision: reset.document_revision, draft_id: None, input_hash: None, profile: Default::default(), view_intent: None },
            &CancellationToken::new(), || true,
        ).expect("reset compile");
    assert_eq!(moved_artifact.slots[0].grid_rect, restored.slots[0].grid_rect);
    assert_eq!(moved_artifact.slots[0].source_crop, restored.slots[0].source_crop);
    assert_eq!(moved_artifact.channels, restored.channels);
}

#[test]
fn source_frame_preview_profile_keeps_direct_coordinates_and_reuses_warm_decode() {
    let (store, _source, document) = fixture_project(63);
    let mut summary = store.summary().expect("summary");
    summary.document = Some(document.clone());
    let compiler = hot_trimmer_sheet_compiler::AlgorithmCompiler::new();
    let cache = Mutex::new(hot_trimmer_sheet_compiler::SourceFramePreviewCache::default());
    let request = |profile| hot_trimmer_sheet_compiler::PersistedStage14PreviewRequest {
        project: &summary, revision: document.document_revision, draft_id: None, input_hash: None, profile,
        view_intent: None,
    };
    let draft = compiler.compile_persisted_stage_14_preview_with_cache(
        request(hot_trimmer_sheet_compiler::SourceFramePreviewProfile::Draft512), &CancellationToken::new(), || true, Some(&cache),
    ).expect("cold 512 draft");
    let refinement = compiler.compile_persisted_stage_14_preview_with_cache(
        request(hot_trimmer_sheet_compiler::SourceFramePreviewProfile::Refinement1024), &CancellationToken::new(), || true, Some(&cache),
    ).expect("warm 1024 refinement");
    assert_eq!((draft.topology.output_size.width, draft.topology.output_size.height), (512, 512));
    assert_eq!((refinement.topology.output_size.width, refinement.topology.output_size.height), (1024, 1024));
    assert_eq!(draft.channels.len(), 1, "Base Color is the only allocated preview map");
    assert_eq!(refinement.channels.len(), 1, "Base Color is the only allocated preview map");
    for (draft_slot, refined_slot) in draft.slots.iter().zip(&refinement.slots) {
        assert_eq!(draft_slot.region_id, refined_slot.region_id);
        assert_eq!(draft_slot.grid_rect, refined_slot.grid_rect);
        assert_eq!(draft_slot.source_crop, refined_slot.source_crop);
        assert_eq!(draft_slot.source_id, refined_slot.source_id);
        assert_eq!(draft_slot.domain_id, refined_slot.domain_id);
    }
    assert_eq!(cache.lock().expect("cache").entry_count(), 1, "one cold decode and zero warm decodes");
    let repeated = compiler.compile_persisted_stage_14_preview_with_cache(
        request(hot_trimmer_sheet_compiler::SourceFramePreviewProfile::Refinement1024), &CancellationToken::new(), || true, Some(&cache),
    ).expect("composed warm refinement");
    assert!(repeated.telemetry.iter().any(|entry| entry.contains("composed_cache=hit")));
}

#[test]
fn source_frame_profiles_publish_atomic_full_square_artifacts() {
    let (store, _source, document) = fixture_project_with_output(63, PixelSize { width: 2048, height: 2048 });
    let mut summary = store.summary().expect("summary");
    summary.document = Some(document.clone());
    let compiler = hot_trimmer_sheet_compiler::AlgorithmCompiler::new();
    let cache = Mutex::new(hot_trimmer_sheet_compiler::SourceFramePreviewCache::default());
    let request = |profile| hot_trimmer_sheet_compiler::PersistedStage14PreviewRequest {
        project: &summary, revision: document.document_revision, draft_id: None, input_hash: None, profile,
        view_intent: None,
    };
    let draft = compiler.compile_persisted_stage_14_preview_with_cache(
        request(hot_trimmer_sheet_compiler::SourceFramePreviewProfile::Draft512), &CancellationToken::new(), || true, Some(&cache),
    ).expect("512 draft");
    let refinement = compiler.compile_persisted_stage_14_preview_with_cache(
        request(hot_trimmer_sheet_compiler::SourceFramePreviewProfile::Refinement1024), &CancellationToken::new(), || true, Some(&cache),
    ).expect("1024 refinement");
    let authoritative = compiler.compile_persisted_stage_14_preview_with_cache(
        request(hot_trimmer_sheet_compiler::SourceFramePreviewProfile::Authoritative), &CancellationToken::new(), || true, Some(&cache),
    ).expect("2048 authoritative");

    let profiles = [(&draft, 512_u32), (&refinement, 1024_u32), (&authoritative, 2048_u32)];
    for (artifact, size) in profiles {
        assert_eq!((artifact.topology.output_size.width, artifact.topology.output_size.height), (size, size));
        assert_eq!(artifact.validity.len(), (size * size) as usize, "validity covers the published profile");
        assert_eq!(artifact.correspondence.len(), (size * size) as usize, "correspondence covers the published profile");
        assert!(artifact.validity.iter().all(|value| *value > 0), "profile has no transparent/invalid quadrant");
        let base = artifact.channels.iter().find(|channel| channel.role == hot_trimmer_domain::MaterialChannelRole::BaseColor)
            .expect("Base Color atlas");
        assert_eq!(base.rgba8.len(), (size * size * 4) as usize, "pixels match profile dimensions");
        assert!(base.rgba8.chunks_exact(4).all(|pixel| pixel[3] > 0), "Base Color alpha covers the profile");
        let mut coverage = vec![0_u8; (size * size) as usize];
        for slot in &artifact.slots {
            for y in slot.allocation.y..slot.allocation.y + slot.allocation.height {
                for x in slot.allocation.x..slot.allocation.x + slot.allocation.width {
                    coverage[(y * size + x) as usize] += 1;
                }
            }
        }
        assert!(coverage.iter().all(|value| *value == 1), "profile allocations do not cover the entire output");
    }
    for (draft_region, refinement_region) in draft.regions.iter().zip(&refinement.regions) {
        assert_eq!(draft_region.region_id, refinement_region.region_id);
        assert_eq!(draft_region.source_crop, refinement_region.source_crop);
        assert_eq!(draft_region.source_bounds, refinement_region.source_bounds);
    }
    for (refinement_region, authoritative_region) in refinement.regions.iter().zip(&authoritative.regions) {
        assert_eq!(refinement_region.region_id, authoritative_region.region_id);
        assert_eq!(refinement_region.source_crop, authoritative_region.source_crop);
        assert_eq!(refinement_region.source_bounds, authoritative_region.source_bounds);
    }
    assert_ne!(draft.channels[0].rgba8.len(), refinement.channels[0].rgba8.len());
    assert_ne!(refinement.channels[0].rgba8.len(), authoritative.channels[0].rgba8.len());
    let cache = cache.lock().expect("cache");
    assert!(cache.rendered_region_count() <= 128);
    assert!(cache.composed_atlas_count() <= 2);
}

#[test]
fn manual_base_color_product_owns_padding_persists_large_source_coordinates_and_rejects_stale_publication() {
    use hot_trimmer_domain::{
        AuthoredLayoutPreset, AuthoredLayoutPresetRegion, ContentReference, GridRect,
        NormalizedPoint, Patch, PatchCommand, PatchGeometry, PatchId, PatchProperties,
        RectificationSettings, RegionOrientation, StructuralProfile, TemplateSlotRole,
        AUTHORED_LAYOUT_PRESET_SCHEMA_VERSION,
    };
    use std::collections::{BTreeMap, BTreeSet};

    // Large-dimension coordinate arithmetic only. This is deliberately not a
    // decoded-pixel or performance qualification; the ignored release harness owns that proof.
    for (width, height) in [(7_952, 4_016), (8_000, 8_000), (16_384, 8_192), (24_576, 12_288)] {
        let frame = SourceFrame::centered_largest(
            hot_trimmer_domain::SourceSetId::new(),
            OrientedPixelSize { width, height },
            [1, 1],
            7,
        );
        assert_eq!(frame.oriented_dimensions, OrientedPixelSize { width, height });
        let x = resolve_boundaries(0, width, 64);
        let y = resolve_boundaries(0, height, 64);
        assert_eq!((*x.first().unwrap(), *x.last().unwrap()), (0, width));
        assert_eq!((*y.first().unwrap(), *y.last().unwrap()), (0, height));
        assert!(x.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(y.windows(2).all(|pair| pair[0] < pair[1]));
    }

    let root = std::env::temp_dir().join(format!("hot-trimmer-manual-base-color-product-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create product fixture directory");
    let project_path = root.join("manual-base-color-product.hottrimmer");
    let mut store = ProjectStore::create(&project_path, "Manual Base Color Product").expect("create product project");
    let initial = store.summary().expect("initial product summary");
    let source_a_set = Uuid::from_bytes(initial.source_sets[0].id.to_bytes());
    let source_b_set = Uuid::new_v4();
    let (encoded_a, _) = numbered_source();
    let (encoded_b, _) = striped_source(96, 128);
    let source_a = SourceId::new();
    let source_b = SourceId::new();
    for (source_set_id, source_id, encoded, width, height, name) in [
        (source_a_set, source_a, encoded_a, 128, 64, "coordinate-fixture-128x64.png"),
        (source_b_set, source_b, encoded_b, 96, 128, "coordinate-fixture-96x128.png"),
    ] {
        let input = SourceInput {
            id: source_id, ownership: SourceOwnership::OwnedCopy, external_path: None,
            origin_path: PathBuf::from(name), sha256: ContentDigest::sha256(&encoded).0,
            width, height, format: "PNG".into(), color_type: "Rgba8".into(), has_alpha: true,
            exif_orientation: 1, has_embedded_icc_profile: false, encoded_bytes: encoded.len() as u64,
            owned_bytes: Some(encoded),
        };
        store.replace_source_in_set(source_set_id, SourceChannel::BaseColor, &input).expect("register product source");
    }
    store.create_source_frame_document().expect("create authored product document");
    let seed = store.document().expect("seed document").topology.regions[0].clone();
    let grid = LogicalGridSpec::DEFAULT;
    let mut authored_regions = Vec::new();
    for y in 0..8_u32 {
        for x in 0..8_u32 {
            let index = y * 8 + x;
            authored_regions.push(AuthoredLayoutPresetRegion {
                preset_region_key: format!("cell-{index:02}"),
                display_name: format!("Authored Cell {}", index + 1),
                grid_rect: GridRect { x: x * 8, y: y * 8, width: 8, height: 8 },
                role: TemplateSlotRole::Planar,
                orientation: RegionOrientation::Unspecified,
                uv_fit: seed.uv_fit.clone(),
                structural_profile: StructuralProfile::Flat,
                default_behavior: RegionBehavior::default(),
            });
        }
    }
    let preset = AuthoredLayoutPreset {
        preset_id: "fixture.manual-base-color-product".into(),
        schema_version: AUTHORED_LAYOUT_PRESET_SCHEMA_VERSION,
        name: "Manual Base Color Product Fixture".into(),
        logical_grid: grid,
        canonical_aspect: [1, 1],
        regions: authored_regions,
        provenance: "manual_base_color_product fixture".into(),
    };
    store.execute_document_command(&TrimSheetDocumentCommand::ApplyAuthoredLayoutPreset {
        preset,
        instance_id: "stable-product-instance".into(),
    }).expect("apply 64-region authored preset");

    let patch_points = |index: u32| {
        let column = index % 5;
        let row = (index / 5) % 4;
        let left = f64::from(20 + column * 180) / 1_000.0;
        let top = f64::from(30 + row * 220) / 1_000.0;
        let right = f64::from(160 + column * 180) / 1_000.0;
        let bottom = f64::from(190 + row * 220) / 1_000.0;
        [
            NormalizedPoint::new(left, top).unwrap(),
            NormalizedPoint::new(right, top).unwrap(),
            NormalizedPoint::new(right, bottom).unwrap(),
            NormalizedPoint::new(left, bottom).unwrap(),
        ]
    };
    let mut patch_ids = Vec::new();
    for index in 0..20_u32 {
        let patch_id = PatchId::new();
        patch_ids.push(patch_id);
        store.execute_patch_command(&PatchCommand::Create {
            patch: Patch {
                id: patch_id,
                source_id: if index % 2 == 0 { source_a } else { source_b },
                name: format!("Product Patch {}", index + 1),
                enabled: true,
                geometry: PatchGeometry { corners: patch_points(index), assistance_mask: None },
                properties: PatchProperties::default(),
                rectification: RectificationSettings::default(),
            },
            index: None,
        }, None).expect("create product patch");
    }
    store.refresh_document_assets().expect("refresh product patches");
    store.execute_document_command(&TrimSheetDocumentCommand::SetOutputResolution {
        output_size: PixelSize { width: 2_048, height: 2_048 },
    }).expect("set authoritative output independently of source size");
    store.execute_document_command(&TrimSheetDocumentCommand::SetAtlasPadding { padding_px: 16 })
        .expect("persist owning-edge padding");

    let region_ids = store.document().unwrap().topology.regions.iter().map(|region| region.id).collect::<Vec<_>>();
    for (region_id, patch_id) in region_ids.iter().copied().zip(patch_ids.iter().copied()) {
        store.execute_document_command(&TrimSheetDocumentCommand::SetRegionContent {
            region_id,
            content: ContentReference::Patch(patch_id),
        }).expect("bind authored patch");
    }
    let source_b_id = hot_trimmer_domain::SourceSetId::from_bytes(*source_b_set.as_bytes());
    for region_id in region_ids.iter().copied().skip(20).take(4) {
        store.execute_document_command(&TrimSheetDocumentCommand::SetRegionContent {
            region_id,
            content: ContentReference::MaterialSource(source_b_id),
        }).expect("bind whole secondary source");
    }
    for (index, sampling) in [(20_usize, RegionSampling::LoopX), (21, RegionSampling::LoopY), (22, RegionSampling::LoopXy)] {
        let mut behavior = RegionBehavior::default();
        behavior.sampling = sampling;
        behavior.period_pixels = Some([16, 16]);
        behavior.synchronize_derived_fields();
        store.execute_document_command(&TrimSheetDocumentCommand::SetRegionBehavior { region_id: region_ids[index], behavior })
            .expect("author explicit loop");
    }
    let mut radial = RegionBehavior::new(ManualRegionRole::Radial);
    radial.synchronize_derived_fields();
    store.execute_document_command(&TrimSheetDocumentCommand::SetRegionBehavior { region_id: region_ids[23], behavior: radial })
        .expect("author radial region");

    let document = store.document().expect("complete product document").clone();
    let stable_ids = document.topology.regions.iter().map(|region| region.id).collect::<Vec<_>>();
    let mut summary = store.summary().expect("complete product summary");
    summary.document = Some(document.clone());
    let compiler = hot_trimmer_sheet_compiler::AlgorithmCompiler::new();
    let cache = Mutex::new(hot_trimmer_sheet_compiler::SourceFramePreviewCache::default());
    let draft = compiler.compile_persisted_stage_14_preview_with_cache(
        hot_trimmer_sheet_compiler::PersistedStage14PreviewRequest {
            project: &summary, revision: document.document_revision, draft_id: Some(3), input_hash: Some("C".into()),
            profile: hot_trimmer_sheet_compiler::SourceFramePreviewProfile::Draft512,
            view_intent: None,
        },
        &CancellationToken::new(), || true, Some(&cache),
    ).expect("publish complete 512 product draft");
    assert_eq!(draft.slots.len(), 64);
    assert_eq!(draft.channels.len(), 1, "Base Color preview does not encode unrelated channels");
    assert!(draft.telemetry.iter().any(|line| line.contains("preview_padding_px=4")));
    assert!(draft.validity.iter().all(|value| *value > 0), "no transparent/black disappearance");
    assert_eq!(draft.region_ownership.len(), 512 * 512);

    let mut logical_coverage = vec![0_u8; (grid.width * grid.height) as usize];
    let mut crops = BTreeSet::new();
    let patch_set = patch_ids.iter().copied().collect::<BTreeSet<_>>();
    for slot in &draft.slots {
        let rect = slot.grid_rect.expect("manual GridRect");
        for y in rect.y..rect.y + rect.height { for x in rect.x..rect.x + rect.width {
            logical_coverage[(y * grid.width + x) as usize] += 1;
        }}
        let binding = &document.region_bindings[&slot.region_id];
        if matches!(binding.content, ContentReference::InheritPrimaryMaterial) {
            let crop = slot.source_crop.expect("inherited direct crop");
            assert!(crops.insert((crop.x, crop.y, crop.width, crop.height)), "default crops are unique");
        }
        if let ContentReference::Patch(id) = binding.content { assert!(patch_set.contains(&id)); }
        assert_eq!(slot.padded_rect, slot.atlas_destination);
        assert!(slot.semantic_rect.x >= slot.padded_rect.x && slot.semantic_rect.y >= slot.padded_rect.y);
        for y in slot.padded_rect.y..slot.padded_rect.y + slot.padded_rect.height {
            for x in slot.padded_rect.x..slot.padded_rect.x + slot.padded_rect.width {
                let index = (y * 512 + x) as usize;
                assert_eq!(draft.region_ownership[index], slot.region_id, "padding ownership crossed RegionId");
            }
        }
    }
    assert!(logical_coverage.iter().all(|count| *count == 1), "semantic topology is complete and non-overlapping");
    let base = &draft.channels[0].rgba8;
    for slot in &draft.slots {
        let semantic = slot.semantic_rect;
        for y in slot.padded_rect.y..slot.padded_rect.y + slot.padded_rect.height {
            for x in slot.padded_rect.x..slot.padded_rect.x + slot.padded_rect.width {
                let owner_x = x.clamp(semantic.x, semantic.x + semantic.width - 1);
                let owner_y = y.clamp(semantic.y, semantic.y + semantic.height - 1);
                let at = ((y * 512 + x) * 4) as usize;
                let owner = ((owner_y * 512 + owner_x) * 4) as usize;
                if x < semantic.x || x >= semantic.x + semantic.width || y < semantic.y || y >= semantic.y + semantic.height {
                    assert_eq!(&base[at..at + 4], &base[owner..owner + 4], "padding is nearest owning-edge dilation");
                }
            }
        }
    }

    // Warm composition is exact and bounded; direct topology edits never evict source domains.
    let warm = compiler.compile_persisted_stage_14_preview_with_cache(
        hot_trimmer_sheet_compiler::PersistedStage14PreviewRequest {
            project: &summary, revision: document.document_revision, draft_id: Some(4), input_hash: Some("C".into()),
            profile: hot_trimmer_sheet_compiler::SourceFramePreviewProfile::Draft512,
            view_intent: None,
        }, &CancellationToken::new(), || true, Some(&cache),
    ).expect("warm complete draft");
    assert!(warm.telemetry.iter().any(|line| line.contains("composed_cache=hit")));
    let refinement = compiler.compile_persisted_stage_14_preview_with_cache(
        hot_trimmer_sheet_compiler::PersistedStage14PreviewRequest {
            project: &summary, revision: document.document_revision, draft_id: Some(5), input_hash: Some("C-refinement".into()),
            profile: hot_trimmer_sheet_compiler::SourceFramePreviewProfile::Refinement1024,
            view_intent: None,
        }, &CancellationToken::new(), || true, Some(&cache),
    ).expect("publish complete 1024 refinement");
    assert_eq!(refinement.topology.output_size, PixelSize { width: 1_024, height: 1_024 });
    assert!(refinement.telemetry.iter().any(|line| line.contains("preview_padding_px=8")));
    assert!(refinement.validity.iter().all(|value| *value > 0));
    assert_eq!(refinement.region_ownership.len(), 1_024 * 1_024);
    let cache = cache.lock().expect("bounded product cache");
    assert!(cache.entry_count() <= 32);
    assert!(cache.rendered_region_count() <= 128);
    assert!(cache.composed_atlas_count() <= 2);
    drop(cache);

    // Rapid A -> B -> C: stale guards reject A/B before publication; only C is returned.
    let stale = compiler.compile_persisted_stage_14_preview(
        hot_trimmer_sheet_compiler::PersistedStage14PreviewRequest {
            project: &summary, revision: document.document_revision, draft_id: Some(1), input_hash: Some("A".into()),
            profile: hot_trimmer_sheet_compiler::SourceFramePreviewProfile::Draft512,
            view_intent: None,
        }, &CancellationToken::new(), || false,
    );
    assert!(stale.is_err());
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert!(compiler.compile_persisted_stage_14_preview(
        hot_trimmer_sheet_compiler::PersistedStage14PreviewRequest {
            project: &summary, revision: document.document_revision, draft_id: Some(2), input_hash: Some("B".into()),
            profile: hot_trimmer_sheet_compiler::SourceFramePreviewProfile::Draft512,
            view_intent: None,
        }, &cancelled, || true,
    ).is_err());
    assert_eq!(draft.revision, document.document_revision, "only current C artifact may publish");

    drop(store);
    let reopened = ProjectStore::open(&project_path).expect("reopen product fixture");
    let reopened_document = reopened.document().expect("reopened product document");
    assert_eq!(reopened_document.topology, document.topology, "topology reopens exactly");
    assert_eq!(reopened_document.logical_grid, document.logical_grid, "logical grid reopens exactly");
    assert_eq!(reopened_document.authored_layout_preset, document.authored_layout_preset, "embedded preset reopens exactly");
    assert_eq!(reopened_document.authored_layout_instance_id, document.authored_layout_instance_id, "preset instance reopens exactly");
    assert_eq!(reopened_document.materials, document.materials, "multi-source registrations reopen exactly");
    assert_eq!(reopened_document.patches, document.patches, "patches reopen exactly");
    assert_eq!(reopened_document.region_bindings, document.region_bindings, "bindings and behavior reopen exactly");
    assert_eq!(reopened_document.source_frame, document.source_frame, "SourceFrame ownership reopens exactly");
    assert_eq!(reopened_document.render_settings, document.render_settings, "output and padding reopen exactly");
    assert_eq!(reopened_document.topology.regions.iter().map(|region| region.id).collect::<Vec<_>>(), stable_ids);
    assert_eq!(reopened_document.render_settings.atlas_padding_px, 16);
    assert_eq!(reopened_document.patches.len(), 20);
    assert_eq!(reopened_document.source_frame.as_ref().unwrap().source_set_id,
        hot_trimmer_domain::SourceSetId::from_bytes(*source_a_set.as_bytes()));
    let binding_kinds = reopened_document.region_bindings.values().fold(BTreeMap::new(), |mut counts, binding| {
        *counts.entry(match binding.content { ContentReference::InheritPrimaryMaterial => "inherit", ContentReference::MaterialSource(_) => "source", ContentReference::Patch(_) => "patch", _ => "other" }).or_insert(0_u32) += 1;
        counts
    });
    assert_eq!(binding_kinds.get("patch"), Some(&20));
    assert_eq!(binding_kinds.get("source"), Some(&4));
}
