use std::{
    io::Cursor,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use base64::Engine as _;
use hot_trimmer_domain::{
    AUTHORED_LAYOUT_PRESET_SCHEMA_VERSION, AuthoredLayoutPreset, AuthoredLayoutPresetRegion,
    CancellationToken, ContentDigest, GridRect, LogicalGridSpec, ManualRegionRole,
    MaterialChannelRole, MaterialMapKind, PixelSize, RegionBehavior, RegionOrientation,
    RegionSampling, StructuralProfile, TemplateSlotRole, TrimSheetDocumentCommand,
};
use hot_trimmer_project_store::{ProjectStore, SourceChannel, SourceInput, SourceOwnership};
use hot_trimmer_sheet_compiler::{
    AlgorithmCompiler, GpuAtlasRenderExecutor, GpuAtlasSourceTextureCache,
    PersistedStage14PreviewRequest, SourceFramePreviewCache, SourceFramePreviewProfile,
    SourceFramePreviewViewIntent,
};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use serde::Serialize;
use sysinfo::{ProcessesToUpdate, System};
use uuid::Uuid;

const SOURCE_WIDTH: u32 = 7_952;
const SOURCE_HEIGHT: u32 = 4_016;
const OUTPUT_EDGE: u32 = 8_192;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RunRecord {
    label: &'static str,
    completed: bool,
    elapsed_ms: u128,
    error: Option<String>,
    telemetry: Vec<String>,
    encode_ms: u128,
    ipc_preparation_ms: u128,
    ipc_payload_bytes: u64,
    ui_paint_ms: Option<u128>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BaselineRecord {
    schema_version: u16,
    build_profile: &'static str,
    commit: String,
    worktree_dirty: bool,
    os: String,
    cpu: String,
    logical_threads: usize,
    physical_threads: Option<usize>,
    ram_bytes: u64,
    gpu_vram_bytes: Option<u64>,
    peak_observed_rss_bytes: u64,
    gpu_capability: String,
    source_count: u32,
    source_format: &'static str,
    source_dimensions: [u32; 2],
    decoded_bytes: u64,
    source_generation_encode_ms: u128,
    region_count: usize,
    patch_count: usize,
    requested_maps: Vec<&'static str>,
    output_dimensions: [u32; 2],
    profile: &'static str,
    snapshot_ms: u128,
    upload_bytes: u64,
    ui_paint_status: &'static str,
    runs: Vec<RunRecord>,
}

#[test]
#[ignore = "real 7952x4016 decode and 8192x8192 GPU material-map set qualification; run explicitly in release mode"]
fn real_8k_cpu_baseline() {
    assert!(
        !cfg!(debug_assertions),
        "real-8K qualification must run with --release"
    );
    let output_directory = std::env::var_os("HOT_TRIMMER_GPU_BASELINE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/gpu-prompt-002"));
    std::fs::create_dir_all(&output_directory).expect("create baseline output directory");
    let root = std::env::temp_dir().join(format!("hot-trimmer-real-8k-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create real baseline project directory");

    let decoded_started = Instant::now();
    let mut source = RgbaImage::new(SOURCE_WIDTH, SOURCE_HEIGHT);
    for (x, y, pixel) in source.enumerate_pixels_mut() {
        *pixel = Rgba([
            ((x / 31 + y / 47) & 255) as u8,
            ((x / 17) & 255) as u8,
            ((y / 13) & 255) as u8,
            255,
        ]);
    }
    assert_eq!(source.dimensions(), (SOURCE_WIDTH, SOURCE_HEIGHT));
    let decoded_bytes = source.as_raw().len() as u64;
    let mut encoded = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(source)
        .write_to(&mut encoded, ImageFormat::Png)
        .expect("encode real source");
    let encoded = encoded.into_inner();
    let decode_generate_ms = decoded_started.elapsed().as_millis();

    let project_path = root.join("real-8k-baseline.hottrimmer");
    let mut store = ProjectStore::create(&project_path, "GPU Prompt 1 Real 8K")
        .expect("create baseline project");
    let summary = store.summary().expect("baseline summary");
    let source_set = Uuid::from_bytes(summary.source_sets[0].id.to_bytes());
    let source_id = hot_trimmer_domain::SourceId::new();
    store
        .replace_source_in_set(
            source_set,
            SourceChannel::BaseColor,
            &SourceInput {
                id: source_id,
                ownership: SourceOwnership::OwnedCopy,
                external_path: None,
                origin_path: PathBuf::from("generated-real-7952x4016.png"),
                sha256: ContentDigest::sha256(&encoded).0,
                width: SOURCE_WIDTH,
                height: SOURCE_HEIGHT,
                format: "PNG".into(),
                color_type: "Rgba8".into(),
                has_alpha: true,
                exif_orientation: 1,
                has_embedded_icc_profile: false,
                encoded_bytes: encoded.len() as u64,
                owned_bytes: Some(encoded),
            },
        )
        .expect("register real source");
    store
        .create_source_frame_document()
        .expect("create SourceFrame document");
    let seed = store.document().expect("document").topology.regions[0].clone();
    let regions = (0..8_u32)
        .flat_map(|y| (0..8_u32).map(move |x| (x, y)))
        .enumerate()
        .map(|(index, (x, y))| AuthoredLayoutPresetRegion {
            preset_region_key: format!("cell-{index:02}"),
            display_name: format!("Cell {}", index + 1),
            grid_rect: GridRect {
                x: x * 8,
                y: y * 8,
                width: 8,
                height: 8,
            },
            role: TemplateSlotRole::Planar,
            orientation: RegionOrientation::Unspecified,
            uv_fit: seed.uv_fit.clone(),
            structural_profile: StructuralProfile::Flat,
            default_behavior: RegionBehavior::default(),
        })
        .collect();
    store
        .execute_document_command(&TrimSheetDocumentCommand::ApplyAuthoredLayoutPreset {
            preset: AuthoredLayoutPreset {
                preset_id: "fixture.gpu-prompt-1-real-8k".into(),
                schema_version: AUTHORED_LAYOUT_PRESET_SCHEMA_VERSION,
                name: "GPU Prompt 1 Real 8K".into(),
                logical_grid: LogicalGridSpec::DEFAULT,
                canonical_aspect: [1, 1],
                regions,
                provenance: "ignored release baseline".into(),
            },
            instance_id: "gpu-prompt-1-real-8k".into(),
        })
        .expect("apply 64-region authored layout");
    store
        .execute_document_command(&TrimSheetDocumentCommand::SetOutputResolution {
            output_size: PixelSize {
                width: OUTPUT_EDGE,
                height: OUTPUT_EDGE,
            },
        })
        .expect("request authoritative 8192 output");
    let region_ids = store
        .document()
        .expect("document")
        .topology
        .regions
        .iter()
        .map(|region| region.id)
        .collect::<Vec<_>>();
    for (index, sampling) in [
        (1_usize, RegionSampling::LoopX),
        (2, RegionSampling::LoopY),
        (3, RegionSampling::LoopXy),
    ] {
        let mut behavior = RegionBehavior::default();
        behavior.sampling = sampling;
        behavior.period_pixels = Some([64, 64]);
        behavior.synchronize_derived_fields();
        store
            .execute_document_command(&TrimSheetDocumentCommand::SetRegionBehavior {
                region_id: region_ids[index],
                behavior,
            })
            .expect("set loop behavior");
    }
    let mut radial = RegionBehavior::new(ManualRegionRole::Radial);
    radial.synchronize_derived_fields();
    store
        .execute_document_command(&TrimSheetDocumentCommand::SetRegionBehavior {
            region_id: region_ids[4],
            behavior: radial,
        })
        .expect("set radial behavior");

    let snapshot_started = Instant::now();
    let document = store.document().expect("complete document").clone();
    let mut project = store.summary().expect("complete summary");
    project.document = Some(document.clone());
    let snapshot_ms = snapshot_started.elapsed().as_millis();
    let compiler = AlgorithmCompiler::new();
    let cache = Mutex::new(SourceFramePreviewCache::default());
    let gpu_capabilities = hot_trimmer_preview::GpuCapabilityService::default();
    let gpu_source_cache = Mutex::new(GpuAtlasSourceTextureCache::default());
    let sampling_complete = Arc::new(AtomicBool::new(false));
    let peak_rss = Arc::new(AtomicU64::new(0));
    let sampler_complete = Arc::clone(&sampling_complete);
    let sampler_peak = Arc::clone(&peak_rss);
    let sampler = std::thread::spawn(move || {
        let Ok(pid) = sysinfo::get_current_pid() else {
            return;
        };
        let mut system = System::new();
        while !sampler_complete.load(Ordering::Acquire) {
            system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
            if let Some(process) = system.process(pid) {
                sampler_peak.fetch_max(process.memory(), Ordering::AcqRel);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
        if let Some(process) = system.process(pid) {
            sampler_peak.fetch_max(process.memory(), Ordering::AcqRel);
        }
    });
    let mut runs = Vec::new();
    let requested_maps = vec![
        MaterialMapKind::BaseColor,
        MaterialMapKind::Height,
        MaterialMapKind::Normal,
        MaterialMapKind::Roughness,
        MaterialMapKind::Metallic,
        MaterialMapKind::AmbientOcclusion,
        MaterialMapKind::RegionId,
    ];
    for label in ["cold", "warm-1", "warm-2"] {
        if label == "cold" {
            *cache.lock().expect("cache") = SourceFramePreviewCache::default();
            *gpu_source_cache.lock().expect("gpu cache") = GpuAtlasSourceTextureCache::default();
        }
        let started = Instant::now();
        let gpu_executor = GpuAtlasRenderExecutor {
            service: &gpu_capabilities,
            source_texture_cache: &gpu_source_cache,
        };
        let result = compiler.compile_persisted_stage_14_preview_with_cache_and_executor(
            PersistedStage14PreviewRequest {
                project: &project,
                revision: document.document_revision,
                draft_id: Some(1),
                input_hash: Some("gpu-prompt-1-real-8k".into()),
                profile: SourceFramePreviewProfile::Authoritative,
                view_intent: Some(SourceFramePreviewViewIntent::MaterialMaps(
                    requested_maps.clone(),
                )),
            },
            &CancellationToken::new(),
            || true,
            Some(&cache),
            Some(&gpu_executor),
        );
        let elapsed_ms = started.elapsed().as_millis();
        runs.push(match result {
            Ok(artifact) => {
                for map in &requested_maps {
                    assert!(
                        artifact.rendered_tiles.contains_key(map),
                        "real-8K material-map set omitted {map:?}"
                    );
                }
                assert!(
                    artifact
                        .rendered_tiles
                        .get(&MaterialMapKind::RegionId)
                        .is_some_and(|tile| tile.manifest.pixel_format
                            == hot_trimmer_sheet_compiler::CompiledTilePixelFormat::R32Uint),
                    "real-8K material-map set must publish compact Region ID"
                );
                let encode_started = Instant::now();
                let base_color = artifact
                    .channels
                    .iter()
                    .find(|channel| channel.role == MaterialChannelRole::BaseColor)
                    .expect("real-8K artifact must contain Base Color");
                let pixels =
                    RgbaImage::from_raw(OUTPUT_EDGE, OUTPUT_EDGE, base_color.rgba8.clone())
                        .expect("Base Color dimensions must match the requested output");
                let mut png = Cursor::new(Vec::new());
                DynamicImage::ImageRgba8(pixels)
                    .write_to(&mut png, ImageFormat::Png)
                    .expect("encode real-8K compiler output");
                let encode_ms = encode_started.elapsed().as_millis();
                let png = png.into_inner();
                let ipc_started = Instant::now();
                let ipc_payload = base64::engine::general_purpose::STANDARD.encode(&png);
                let ipc_preparation_ms = ipc_started.elapsed().as_millis();
                let ipc_payload_bytes = u64::try_from(ipc_payload.len()).unwrap_or(u64::MAX);
                RunRecord {
                    label,
                    completed: true,
                    elapsed_ms,
                    error: None,
                    telemetry: artifact.telemetry,
                    encode_ms,
                    ipc_preparation_ms,
                    ipc_payload_bytes,
                    ui_paint_ms: None,
                }
            }
            Err(error) => RunRecord {
                label,
                completed: false,
                elapsed_ms,
                error: Some(error.to_string()),
                telemetry: Vec::new(),
                encode_ms: 0,
                ipc_preparation_ms: 0,
                ipc_payload_bytes: 0,
                ui_paint_ms: None,
            },
        });
    }
    let switch_started = Instant::now();
    let gpu_executor = GpuAtlasRenderExecutor {
        service: &gpu_capabilities,
        source_texture_cache: &gpu_source_cache,
    };
    let switch_result = compiler.compile_persisted_stage_14_preview_with_cache_and_executor(
        PersistedStage14PreviewRequest {
            project: &project,
            revision: document.document_revision,
            draft_id: Some(2),
            input_hash: Some("gpu-prompt-004-cached-normal-switch".into()),
            profile: SourceFramePreviewProfile::Authoritative,
            view_intent: Some(SourceFramePreviewViewIntent::MaterialMaps(vec![
                MaterialMapKind::Normal,
            ])),
        },
        &CancellationToken::new(),
        || true,
        Some(&cache),
        Some(&gpu_executor),
    );
    let switch_elapsed_ms = switch_started.elapsed().as_millis();
    runs.push(match switch_result {
        Ok(artifact) => {
            assert!(
                artifact
                    .rendered_tiles
                    .contains_key(&MaterialMapKind::Normal),
                "cached single-map switch must publish Normal"
            );
            assert!(
                artifact.telemetry.iter().any(|line| {
                    line.contains("requested_map=Normal")
                        && line.contains("executed_gpu_passes=none")
                        && line.contains("readback_ms=0")
                }),
                "cached single-map switch must avoid new GPU dispatch/readback"
            );
            RunRecord {
                label: "cached-normal-switch",
                completed: true,
                elapsed_ms: switch_elapsed_ms,
                error: None,
                telemetry: artifact.telemetry,
                encode_ms: 0,
                ipc_preparation_ms: 0,
                ipc_payload_bytes: 0,
                ui_paint_ms: None,
            }
        }
        Err(error) => RunRecord {
            label: "cached-normal-switch",
            completed: false,
            elapsed_ms: switch_elapsed_ms,
            error: Some(error.to_string()),
            telemetry: Vec::new(),
            encode_ms: 0,
            ipc_preparation_ms: 0,
            ipc_payload_bytes: 0,
            ui_paint_ms: None,
        },
    });
    sampling_complete.store(true, Ordering::Release);
    sampler.join().expect("RSS sampler thread");

    let mut system = System::new_all();
    system.refresh_all();
    let gpu_capability = match gpu_capabilities.initialize() {
        Ok(state) => state.capabilities().diagnostic_line(),
        Err(error) => format!("unsupported: {error}"),
    };
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".into());
    let worktree_dirty = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .is_some_and(|output| !output.stdout.is_empty());
    let record = BaselineRecord {
        schema_version: 1,
        build_profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        commit,
        worktree_dirty,
        os: System::long_os_version().unwrap_or_else(|| std::env::consts::OS.into()),
        cpu: system
            .cpus()
            .first()
            .map(|cpu| cpu.brand().to_owned())
            .unwrap_or_else(|| "unknown".into()),
        logical_threads: system.cpus().len(),
        physical_threads: system.physical_core_count(),
        ram_bytes: system.total_memory(),
        gpu_vram_bytes: detected_nvidia_vram_bytes(),
        peak_observed_rss_bytes: peak_rss.load(Ordering::Acquire),
        gpu_capability,
        source_count: 1,
        source_format: "PNG/RGBA8",
        source_dimensions: [SOURCE_WIDTH, SOURCE_HEIGHT],
        decoded_bytes,
        source_generation_encode_ms: decode_generate_ms,
        region_count: region_ids.len(),
        patch_count: 0,
        requested_maps: vec![
            "BaseColor",
            "Height",
            "Normal",
            "Roughness",
            "Metallic",
            "AmbientOcclusion",
            "RegionId",
        ],
        output_dimensions: [OUTPUT_EDGE, OUTPUT_EDGE],
        profile: "Authoritative",
        snapshot_ms,
        upload_bytes: 0,
        ui_paint_status: "not available in the headless release harness; capture separately in the native UI trace",
        runs,
    };
    let json = serde_json::to_string_pretty(&record).expect("serialize baseline JSON");
    std::fs::write(output_directory.join("gpu-prompt-002-real-8k.json"), &json)
        .expect("write baseline JSON");
    let statuses = record
        .runs
        .iter()
        .map(|run| {
            format!(
                "- {}: {} in {} ms{}",
                run.label,
                if run.completed { "completed" } else { "failed" },
                run.elapsed_ms,
                run.error
                    .as_ref()
                    .map(|error| format!(" ({error})"))
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let telemetry_excerpt = telemetry_markdown_excerpt(&record.runs);
    let markdown = format!(
        "# GPU Prompt 004 real-8K material-map set\n\nActual decoded source: {SOURCE_WIDTH}x{SOURCE_HEIGHT} ({decoded_bytes} bytes); generation/encode: {decode_generate_ms} ms.\n\nRequested output: {OUTPUT_EDGE}x{OUTPUT_EDGE} Base Color, Height, Normal, Roughness, Metallic, Ambient Occlusion, and Region ID through `compile_persisted`.\n\n{statuses}\n\n{telemetry_excerpt}\n\nGPU capability: `{}`\n",
        record.gpu_capability
    );
    std::fs::write(output_directory.join("gpu-prompt-002-real-8k.md"), markdown)
        .expect("write baseline Markdown");
    println!("{}", output_directory.display());
}

fn telemetry_markdown_excerpt(runs: &[RunRecord]) -> String {
    let mut lines = vec!["## Telemetry excerpt".to_string()];
    for run in runs {
        lines.push(format!("### {}", run.label));
        let mut selected = run
            .telemetry
            .iter()
            .filter(|line| {
                line.contains("gpu_pass_timing=")
                    || line.contains("executed_gpu_passes=")
                    || line.contains("readback_bytes=")
            })
            .take(12)
            .map(|line| format!("- `{line}`"))
            .collect::<Vec<_>>();
        if selected.is_empty() {
            selected.push("- `no telemetry`".into());
        }
        lines.extend(selected);
    }
    lines.join("\n")
}

fn detected_nvidia_vram_bytes() -> Option<u64> {
    let output = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()?
        .lines()
        .next()?
        .trim()
        .parse::<u64>()
        .ok()?
        .checked_mul(1_024 * 1_024)
}
