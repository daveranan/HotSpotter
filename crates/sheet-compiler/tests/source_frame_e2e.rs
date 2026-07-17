use std::{io::Cursor, path::PathBuf};

use hot_trimmer_domain::{
    generate_partition, resolve_boundaries, ContentDigest, LogicalGridSpec, MaterialMapContent,
    MaterialMapKind, MaterialSourceSet, OrientedPixelSize, PartitionRecipe, PixelSize,
    SamplingMode, SourceFrame, SourceId, TrimSheetDocument, TrimSheetDocumentCommand, NormalizedBounds, NormalizedScalar,
};
use hot_trimmer_placement_solver::MirrorTransform;
use hot_trimmer_project_store::{ProjectStore, SourceChannel, SourceInput, SourceOwnership};
use hot_trimmer_domain::CancellationToken;
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use uuid::Uuid;

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

fn fixture_project(target: u32) -> (ProjectStore, Vec<u8>, TrimSheetDocument) {
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
        hot_trimmer_domain::LayoutId::new(), frame, recipe, PixelSize { width: 64, height: 64 },
        vec![material], Vec::new(),
    ).expect("create source-frame document");
    let mut summary = summary;
    summary.document = Some(document.clone());
    (store, raw, document)
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
                source_image.save(directory.join("source-frame-fixture-8000x4000.png")).expect("write source fixture");
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
            hot_trimmer_sheet_compiler::PersistedStage14PreviewRequest { project: &moved_summary, revision: moved.document_revision, draft_id: None, input_hash: None },
            &CancellationToken::new(), || true,
        ).expect("moved compile");
    assert_eq!(detached.source_overrides[&region_id].source_bounds, moved.source_frame.as_ref().unwrap().region_bounds(moved.logical_grid.unwrap(), moved.topology.regions[0].grid_rect.unwrap()));
    let reset = detached.apply_command(&TrimSheetDocumentCommand::ResetSourceCell { region_id }).expect("reset cell");
    assert!(reset.source_overrides.is_empty());
    let reset_summary = { let mut summary = summary.clone(); summary.document = Some(reset.clone()); summary };
    let restored = hot_trimmer_sheet_compiler::AlgorithmCompiler::new()
        .compile_persisted_stage_14_preview(
            hot_trimmer_sheet_compiler::PersistedStage14PreviewRequest { project: &reset_summary, revision: reset.document_revision, draft_id: None, input_hash: None },
            &CancellationToken::new(), || true,
        ).expect("reset compile");
    assert_eq!(moved_artifact.slots[0].grid_rect, restored.slots[0].grid_rect);
    assert_eq!(moved_artifact.slots[0].source_crop, restored.slots[0].source_crop);
    assert_eq!(moved_artifact.channels, restored.channels);
}
