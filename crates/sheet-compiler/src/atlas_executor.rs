//! Immutable atlas render-execution boundary introduced by GPU migration Prompt 1.

use std::{
    collections::BTreeMap,
    num::NonZeroU64,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use hot_trimmer_domain::{
    CancellationToken, ContentDigest, MaterialChannelRole, RegionId, SourceSamplingMode,
    SourceSetId,
};
use hot_trimmer_image_io::{ImagePlane, LinearColor, MaskValue};
use hot_trimmer_material_synthesis::PreparedMaterialDomain;
use hot_trimmer_placement_solver::{MirrorTransform, SamplingPlan};
use hot_trimmer_render_core::PreparedExemplarChannel;
use wgpu::util::DeviceExt;

use crate::{
    AlgorithmCompiler, IntermediateAtlasArtifact, IntermediateAtlasRequest, SlotSynthesisLimits,
    SlotSynthesisRequest, SynthesizedSlotMaterial,
    compiled_atlas_plan::{CompiledAtlasPlanV1, CompiledRegionCommandV1, CompiledSourceCommandV1},
    persisted_pipeline::{SourceFramePreviewCache, semantic_rect_for_padding},
    synthesize_slot_material_with_guard,
};

#[derive(Debug, Clone)]
pub struct AtlasPreparedSource {
    pub source_set_id: SourceSetId,
    pub source_id: ContentDigest,
    pub channel_role: MaterialChannelRole,
    pub domain: Arc<PreparedMaterialDomain>,
}

#[derive(Debug)]
pub struct AtlasRenderExecutionInput<'a> {
    pub prepared_sources: Vec<AtlasPreparedSource>,
    pub source_frame_cache: Option<&'a Mutex<SourceFramePreviewCache>>,
}

#[derive(Clone, Debug)]
pub struct AtlasExecutedRegion {
    pub region_id: RegionId,
    pub sampling_plan: SamplingPlan,
    pub result: Arc<SynthesizedSlotMaterial>,
}

#[derive(Debug, Default)]
pub struct AtlasCpuRenderExecutorOutput {
    pub regions: Vec<AtlasExecutedRegion>,
    pub render_ms: u128,
    pub rendered_cache_hits: u32,
}

#[derive(Debug)]
pub struct AtlasFinalAtlasOutput {
    pub base_color_rgba8: Arc<[u8]>,
    /// Exact bounded GPU readback for interactive raw-byte publication.
    pub interactive_tile: Arc<GpuAtlasRenderedTile>,
    pub region_valid_pixel_counts: Vec<(RegionId, u64)>,
    pub render_ms: u128,
    pub source_cache_hits: u32,
    pub pipeline_cache_hits: u32,
    pub upload_bytes: u64,
    pub upload_ms: u128,
    pub command_count: u32,
    pub command_bytes: u64,
    pub dispatch_ms: u128,
    pub readback_bytes: u64,
    pub readback_ms: u128,
    pub telemetry: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GpuAtlasRenderedTile {
    pub manifest: crate::CompiledAtlasTileManifest,
    pixels: Arc<[u8]>,
}

impl GpuAtlasRenderedTile {
    #[must_use]
    pub fn pixels(&self) -> &[u8] { &self.pixels }

    #[must_use]
    pub fn payload(&self) -> Arc<[u8]> { Arc::clone(&self.pixels) }

    #[must_use]
    fn with_publication_identity(
        &self,
        identity: crate::CompiledAtlasTileIdentity,
        generation: u64,
    ) -> Self {
        let mut manifest = self.manifest.clone();
        manifest.identity = identity;
        manifest.generation = generation;
        manifest.opaque_handle.clear();
        Self {
            manifest,
            pixels: Arc::clone(&self.pixels),
        }
    }
}

#[derive(Debug)]
pub enum AtlasRenderExecutorOutput {
    CpuRegions(AtlasCpuRenderExecutorOutput),
    FinalAtlas(AtlasFinalAtlasOutput),
}

impl Default for AtlasRenderExecutorOutput {
    fn default() -> Self {
        Self::CpuRegions(AtlasCpuRenderExecutorOutput::default())
    }
}

impl AtlasRenderExecutorOutput {
    #[must_use]
    pub const fn as_cpu_regions(&self) -> Option<&AtlasCpuRenderExecutorOutput> {
        match self {
            Self::CpuRegions(output) => Some(output),
            Self::FinalAtlas(_) => None,
        }
    }

    #[must_use]
    pub const fn as_final_atlas(&self) -> Option<&AtlasFinalAtlasOutput> {
        match self {
            Self::CpuRegions(_) => None,
            Self::FinalAtlas(output) => Some(output),
        }
    }
}

#[derive(Debug)]
pub struct AtlasComposeExecutionInput<'a> {
    pub plan: &'a CompiledAtlasPlanV1,
    pub request: &'a IntermediateAtlasRequest<'a>,
}

#[derive(Debug)]
pub struct AtlasComposeExecutorOutput {
    pub artifact: IntermediateAtlasArtifact,
    pub compose_ms: u128,
}

#[derive(Debug)]
pub enum AtlasRenderExecutionError {
    Cancelled,
    Superseded,
    MissingPreparedSource {
        source_set_id: SourceSetId,
        source_id: ContentDigest,
    },
    InvalidInput(String),
    Stage14(String),
    Composition(String),
    Gpu(String),
}

impl std::fmt::Display for AtlasRenderExecutionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(formatter, "atlas render execution cancelled"),
            Self::Superseded => write!(formatter, "atlas render execution was superseded"),
            Self::MissingPreparedSource {
                source_set_id,
                source_id,
            } => write!(
                formatter,
                "missing prepared source {source_set_id}/{source_id:?}"
            ),
            Self::InvalidInput(message) => {
                write!(formatter, "atlas render input was invalid: {message}")
            }
            Self::Stage14(message) => write!(formatter, "Stage 14 CPU execution failed: {message}"),
            Self::Composition(message) => write!(formatter, "atlas composition failed: {message}"),
            Self::Gpu(message) => write!(formatter, "GPU atlas execution failed: {message}"),
        }
    }
}

impl std::error::Error for AtlasRenderExecutionError {}

pub trait AtlasRenderExecutor {
    fn execute(
        &self,
        plan: &CompiledAtlasPlanV1,
        input: &AtlasRenderExecutionInput<'_>,
        cancellation: &CancellationToken,
        is_current: &dyn Fn() -> bool,
    ) -> Result<AtlasRenderExecutorOutput, AtlasRenderExecutionError>;

    fn compose(
        &self,
        input: &AtlasComposeExecutionInput<'_>,
        cancellation: &CancellationToken,
        is_current: &dyn Fn() -> bool,
    ) -> Result<AtlasComposeExecutorOutput, AtlasRenderExecutionError>;
}

/// Prompt 1 production executor. It deliberately retains the established CPU
/// sampler while forcing every Stage 14 request through the immutable boundary.
#[derive(Debug, Default)]
pub struct CpuAtlasRenderExecutor;

static CPU_ATLAS_EXECUTOR_PLAN_CAPTURE: OnceLock<Mutex<Option<CompiledAtlasPlanV1>>> =
    OnceLock::new();
static CPU_STAGE14_EXECUTION_CALLS: AtomicU64 = AtomicU64::new(0);
static CPU_ATLAS_COMPOSITION_CALLS: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AtlasCpuExecutionCounters {
    pub stage14_calls: u64,
    pub atlas_composition_calls: u64,
}

pub fn clear_cpu_atlas_executor_plan_capture() {
    if let Ok(mut capture) = CPU_ATLAS_EXECUTOR_PLAN_CAPTURE
        .get_or_init(|| Mutex::new(None))
        .lock()
    {
        *capture = None;
    }
}

pub fn clear_atlas_cpu_execution_counters() {
    CPU_STAGE14_EXECUTION_CALLS.store(0, Ordering::SeqCst);
    CPU_ATLAS_COMPOSITION_CALLS.store(0, Ordering::SeqCst);
}

#[must_use]
pub fn atlas_cpu_execution_counters() -> AtlasCpuExecutionCounters {
    AtlasCpuExecutionCounters {
        stage14_calls: CPU_STAGE14_EXECUTION_CALLS.load(Ordering::SeqCst),
        atlas_composition_calls: CPU_ATLAS_COMPOSITION_CALLS.load(Ordering::SeqCst),
    }
}

#[must_use]
pub fn captured_cpu_atlas_executor_plan() -> Option<CompiledAtlasPlanV1> {
    CPU_ATLAS_EXECUTOR_PLAN_CAPTURE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|capture| capture.clone())
}

fn record_cpu_atlas_executor_plan(plan: &CompiledAtlasPlanV1) {
    if let Ok(mut capture) = CPU_ATLAS_EXECUTOR_PLAN_CAPTURE
        .get_or_init(|| Mutex::new(None))
        .lock()
    {
        *capture = Some(plan.clone());
    }
}

impl AtlasRenderExecutor for CpuAtlasRenderExecutor {
    fn execute(
        &self,
        plan: &CompiledAtlasPlanV1,
        input: &AtlasRenderExecutionInput<'_>,
        cancellation: &CancellationToken,
        is_current: &dyn Fn() -> bool,
    ) -> Result<AtlasRenderExecutorOutput, AtlasRenderExecutionError> {
        record_cpu_atlas_executor_plan(plan);
        plan.validate()
            .map_err(|error| AtlasRenderExecutionError::InvalidInput(error.to_string()))?;
        let started = Instant::now();
        let mut regions = Vec::with_capacity(plan.ordered_regions.len());
        let mut rendered_cache_hits = 0_u32;
        for command in &plan.ordered_regions {
            if cancellation.is_cancelled() {
                return Err(AtlasRenderExecutionError::Cancelled);
            }
            if !is_current() {
                return Err(AtlasRenderExecutionError::Superseded);
            }
            let source = input
                .prepared_sources
                .iter()
                .find(|source| {
                    source.source_set_id == command.source_set_id
                        && source.source_id == command.source_id
                        && source.channel_role == MaterialChannelRole::BaseColor
                })
                .ok_or_else(|| AtlasRenderExecutionError::MissingPreparedSource {
                    source_set_id: command.source_set_id,
                    source_id: command.source_id.clone(),
                })?;
            let allocation = command.destination_rect.0;
            let semantic = semantic_rect_for_padding(
                hot_trimmer_domain::CanonicalRect {
                    x: allocation.x,
                    y: allocation.y,
                    width: allocation.width,
                    height: allocation.height,
                },
                command.padding_px,
                command.edge_eligibility,
            );
            // Exact plan/source identities make authoritative CPU oracle output reusable.
            let use_cache = input.source_frame_cache.is_some();
            let result = if use_cache {
                input
                    .source_frame_cache
                    .and_then(|cache| cache.lock().ok())
                    .and_then(|cache| cache.get_rendered(&command.render_cache_key))
            } else {
                None
            };
            let result = if let Some(result) = result {
                rendered_cache_hits = rendered_cache_hits.saturating_add(1);
                result
            } else {
                CPU_STAGE14_EXECUTION_CALLS.fetch_add(1, Ordering::SeqCst);
                let result = Arc::new({
                    let result = synthesize_slot_material_with_guard(
                        SlotSynthesisRequest {
                            plan: &command.sampling_plan,
                            domain: source.domain.as_ref(),
                            output_dimensions: [semantic.width, semantic.height],
                            limits: SlotSynthesisLimits::default(),
                        },
                        &|| cancellation.is_cancelled() || !is_current(),
                    )
                    .map_err(|error| {
                        AtlasRenderExecutionError::Stage14(format!(
                            "region {}: {error}",
                            command.region_id
                        ))
                    })?;
                    apply_source_offset_to_result(command, source.domain.as_ref(), result)?
                });
                if use_cache
                    && let Some(cache) = input.source_frame_cache
                    && let Ok(mut cache) = cache.lock()
                {
                    cache.insert_rendered(command.render_cache_key.clone(), Arc::clone(&result));
                }
                result
            };
            regions.push(AtlasExecutedRegion {
                region_id: command.region_id,
                sampling_plan: command.sampling_plan.clone(),
                result,
            });
        }
        Ok(AtlasRenderExecutorOutput::CpuRegions(
            AtlasCpuRenderExecutorOutput {
                regions,
                render_ms: started.elapsed().as_millis(),
                rendered_cache_hits,
            },
        ))
    }

    fn compose(
        &self,
        input: &AtlasComposeExecutionInput<'_>,
        cancellation: &CancellationToken,
        is_current: &dyn Fn() -> bool,
    ) -> Result<AtlasComposeExecutorOutput, AtlasRenderExecutionError> {
        input
            .plan
            .validate()
            .map_err(|error| AtlasRenderExecutionError::InvalidInput(error.to_string()))?;
        if cancellation.is_cancelled() {
            return Err(AtlasRenderExecutionError::Cancelled);
        }
        if !is_current() {
            return Err(AtlasRenderExecutionError::Superseded);
        }
        let started = Instant::now();
        CPU_ATLAS_COMPOSITION_CALLS.fetch_add(1, Ordering::SeqCst);
        let artifact = AlgorithmCompiler::new()
            .compile_intermediate_atlas(input.request, cancellation, || {
                input.plan.document_revision
            })
            .map_err(|error| AtlasRenderExecutionError::Composition(error.to_string()))?;
        if !is_current() {
            return Err(AtlasRenderExecutionError::Superseded);
        }
        Ok(AtlasComposeExecutorOutput {
            artifact,
            compose_ms: started.elapsed().as_millis(),
        })
    }
}

fn apply_source_offset_to_result(
    command: &CompiledRegionCommandV1,
    domain: &PreparedMaterialDomain,
    mut result: SynthesizedSlotMaterial,
) -> Result<SynthesizedSlotMaterial, AtlasRenderExecutionError> {
    let offset = command.source_to_region_transform.offset;
    if offset.iter().all(|value| value.abs() <= f64::EPSILON) {
        return Ok(result);
    }
    let crop = command.source_crop.0;
    if !offset[0].is_finite() || !offset[1].is_finite() {
        return Err(AtlasRenderExecutionError::InvalidInput(format!(
            "region {} authored source offset is not finite",
            command.region_id
        )));
    }
    let shift = [
        (offset[0] * f64::from(crop.width)) as f32,
        (offset[1] * f64::from(crop.height)) as f32,
    ];
    let positions = result
        .correspondence
        .to_row_major()
        .into_iter()
        .map(|position| [position[0] + shift[0], position[1] + shift[1]])
        .collect::<Vec<_>>();
    let original_validity = result.validity.to_row_major();
    let validity = positions
        .iter()
        .zip(original_validity.iter())
        .map(|(position, original)| {
            MaskValue(if original.0 >= 0.5 && sample_offset_validity(domain, *position) {
                1.0
            } else {
                0.0
            })
        })
        .collect::<Vec<_>>();
    let tile_edge = result.correspondence.tile_edge();
    result.correspondence = offset_plane(
        command,
        "correspondence",
        ImagePlane::from_row_major(result.width, result.height, tile_edge, &positions),
    )?;
    result.validity = offset_plane(
        command,
        "validity",
        ImagePlane::from_row_major(result.width, result.height, tile_edge, &validity),
    )?;
    let linear = command.sampling_plan.sampling_policy.filter != SourceSamplingMode::Nearest;
    result.channels = result
        .channels
        .into_iter()
        .map(|channel| match channel {
            PreparedExemplarChannel::BaseColor { plane: _, alpha_mode } => {
                let Some(source) = domain.registered_channels().iter().find_map(|channel| {
                    if let PreparedExemplarChannel::BaseColor { plane, .. } = channel {
                        Some(plane)
                    } else {
                        None
                    }
                }) else {
                    return Err(AtlasRenderExecutionError::MissingPreparedSource {
                        source_set_id: command.source_set_id,
                        source_id: command.source_id.clone(),
                    });
                };
                let pixels = positions
                    .iter()
                    .map(|position| sample_offset_color(source, *position, linear))
                    .collect::<Vec<_>>();
                Ok(PreparedExemplarChannel::BaseColor {
                    plane: offset_plane(
                        command,
                        "base color",
                        ImagePlane::from_row_major(result.width, result.height, tile_edge, &pixels),
                    )?,
                    alpha_mode,
                })
            }
            other => Ok(other),
        })
        .collect::<Result<Vec<_>, AtlasRenderExecutionError>>()?;
    Ok(result)
}

fn offset_plane<T>(
    command: &CompiledRegionCommandV1,
    label: &str,
    plane: Result<ImagePlane<T>, hot_trimmer_image_io::NormalizationError>,
) -> Result<ImagePlane<T>, AtlasRenderExecutionError> {
    plane.map_err(|error| {
        AtlasRenderExecutionError::Stage14(format!(
            "region {} authored source offset {label} plane construction failed: {error}",
            command.region_id
        ))
    })
}

fn sample_offset_validity(domain: &PreparedMaterialDomain, at: [f32; 2]) -> bool {
    if at[0] < 0.0 || at[1] < 0.0 || at[0] >= domain.width as f32 || at[1] >= domain.height as f32
    {
        return false;
    }
    let pixel_x = (at[0] - 0.5).round().clamp(0.0, (domain.width - 1) as f32) as u32;
    let pixel_y = (at[1] - 0.5).round().clamp(0.0, (domain.height - 1) as f32) as u32;
    domain.validity.pixel(pixel_x, pixel_y).0 >= 0.5
}

fn offset_bounds<T>(plane: &ImagePlane<T>, at: [f32; 2]) -> (u32, u32, u32, u32, f32, f32) {
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

fn sample_offset_f32<T: Copy>(
    plane: &ImagePlane<T>,
    at: [f32; 2],
    linear: bool,
    value: impl Fn(&T) -> f32,
) -> f32 {
    let (x0, y0, x1, y1, tx, ty) = offset_bounds(plane, at);
    if !linear {
        return value(plane.pixel(if tx < 0.5 { x0 } else { x1 }, if ty < 0.5 { y0 } else { y1 }));
    }
    let a = value(plane.pixel(x0, y0)) * (1.0 - tx) + value(plane.pixel(x1, y0)) * tx;
    let b = value(plane.pixel(x0, y1)) * (1.0 - tx) + value(plane.pixel(x1, y1)) * tx;
    a * (1.0 - ty) + b * ty
}

fn sample_offset_color(
    plane: &ImagePlane<LinearColor>,
    at: [f32; 2],
    linear: bool,
) -> LinearColor {
    LinearColor {
        rgb: std::array::from_fn(|index| {
            sample_offset_f32(plane, at, linear, |color| color.rgb[index])
        }),
        alpha: sample_offset_f32(plane, at, linear, |color| color.alpha),
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct GpuSourceTextureKey {
    source_set_id: SourceSetId,
    source_id: ContentDigest,
    digest: ContentDigest,
    width: u32,
    height: u32,
    decoded_format: String,
    decoder_version: String,
    color_version: String,
    channel_role: MaterialChannelRole,
}

impl GpuSourceTextureKey {
    fn from_source(source: &CompiledSourceCommandV1) -> Self {
        Self {
            source_set_id: source.source_set_id,
            source_id: source.source_id.clone(),
            digest: source.digest.clone(),
            width: source.oriented_dimensions.width,
            height: source.oriented_dimensions.height,
            decoded_format: source.decoded_format.clone(),
            decoder_version: source.decoder_version.clone(),
            color_version: source.color_version.clone(),
            channel_role: source.channel_role,
        }
    }
}

struct GpuCachedSourceTexture {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    byte_len: u64,
    last_used: u64,
}

struct GpuAtlasPipeline {
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
}

#[derive(Default)]
pub struct GpuAtlasSourceTextureCache {
    clock: u64,
    sources: BTreeMap<GpuSourceTextureKey, Arc<GpuCachedSourceTexture>>,
    pipeline: Option<Arc<GpuAtlasPipeline>>,
    rendered_tiles: Vec<Arc<GpuAtlasRenderedTile>>,
    readback_pool: GpuAtlasReadbackPool,
}

impl GpuAtlasSourceTextureCache {
    #[must_use]
    pub fn source_count(&self) -> usize {
        self.sources.len()
    }

    fn cached_tile(
        &mut self,
        identity: &crate::CompiledAtlasTileIdentity,
        generation: u64,
    ) -> Option<Arc<GpuAtlasRenderedTile>> {
        let tile = self
            .rendered_tiles
            .iter()
            .find(|tile| tile.manifest.identity.pixel_identity() == identity.pixel_identity())?
            .with_publication_identity(identity.clone(), generation);
        Some(Arc::new(tile))
    }

    fn remember_tile(&mut self, tile: Arc<GpuAtlasRenderedTile>) {
        let pixel_identity = tile.manifest.identity.pixel_identity();
        self.rendered_tiles
            .retain(|existing| existing.manifest.identity.pixel_identity() != pixel_identity);
        self.rendered_tiles.push(Arc::clone(&tile));
        while self.rendered_tiles.len() > 8 { self.rendered_tiles.remove(0); }
    }
}

pub struct GpuAtlasRenderExecutor<'a> {
    pub service: &'a hot_trimmer_preview::GpuCapabilityService,
    pub source_texture_cache: &'a Mutex<GpuAtlasSourceTextureCache>,
}

/// Cache-owned raw bytes for a single bounded output tile.  This deliberately
/// holds only tile-sized payloads; atlas-wide coordinate and Region ID images are
/// never retained here.
#[derive(Clone, Debug)]
pub struct GpuAtlasCachedTile {
    pub manifest: crate::CompiledAtlasTileManifest,
    pixels: Arc<[u8]>,
}

impl GpuAtlasCachedTile {
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

#[derive(Clone, Debug)]
pub struct GpuAtlasTileCache {
    byte_capacity: usize,
    used_bytes: usize,
    active_generation: u64,
    next_handle: u64,
    tiles: Vec<GpuAtlasCachedTile>,
}

impl GpuAtlasTileCache {
    #[must_use]
    pub fn new(byte_capacity: usize) -> Self {
        Self {
            byte_capacity,
            used_bytes: 0,
            active_generation: 0,
            next_handle: 0,
            tiles: Vec::new(),
        }
    }

    pub fn begin_generation(&mut self, generation: u64) {
        self.active_generation = self.active_generation.max(generation);
        self.tiles.retain(|tile| tile.manifest.generation >= self.active_generation);
        self.used_bytes = self.tiles.iter().map(|tile| tile.pixels.len()).sum();
    }

    /// Rejects obsolete GPU completions before any native publication occurs.
    pub fn publish(
        &mut self,
        mut manifest: crate::CompiledAtlasTileManifest,
        pixels: Arc<[u8]>,
    ) -> Result<crate::CompiledAtlasTileManifest, AtlasRenderExecutionError> {
        if manifest.generation < self.active_generation {
            return Err(AtlasRenderExecutionError::Superseded);
        }
        let expected_bytes = usize::try_from(
            u64::from(manifest.row_stride) * u64::from(manifest.height),
        )
        .map_err(|_| {
            AtlasRenderExecutionError::InvalidInput(
                "tile row stride and height exceed addressable memory".into(),
            )
        })?;
        if pixels.len() != expected_bytes {
            return Err(AtlasRenderExecutionError::InvalidInput(format!(
                "tile payload is {} bytes, expected {} from row stride and height",
                pixels.len(), expected_bytes
            )));
        }
        if pixels.len() > self.byte_capacity {
            return Err(AtlasRenderExecutionError::Gpu(
                "tile payload exceeds the configured bounded cache".into(),
            ));
        }
        self.active_generation = self.active_generation.max(manifest.generation);
        while self.used_bytes + pixels.len() > self.byte_capacity {
            let Some(evicted) = self.tiles.first() else { break };
            self.used_bytes -= evicted.pixels.len();
            self.tiles.remove(0);
        }
        self.next_handle = self.next_handle.saturating_add(1);
        manifest.opaque_handle = format!("gpu-tile-{}", self.next_handle);
        self.used_bytes += pixels.len();
        self.tiles.push(GpuAtlasCachedTile {
            manifest: manifest.clone(),
            pixels,
        });
        Ok(manifest)
    }

    #[must_use]
    pub fn resolve(&self, handle: &str) -> Option<&GpuAtlasCachedTile> {
        self.tiles
            .iter()
            .find(|tile| tile.manifest.opaque_handle == handle)
    }

    /// Removes a caller-owned handle. Generation matching prevents an obsolete
    /// client from releasing a replacement tile with the same logical bounds.
    pub fn release(&mut self, generation: u64, handle: &str) -> bool {
        let Some(index) = self.tiles.iter().position(|tile| {
            tile.manifest.generation == generation && tile.manifest.opaque_handle == handle
        }) else {
            return false;
        };
        let removed = self.tiles.remove(index);
        self.used_bytes = self.used_bytes.saturating_sub(removed.pixels.len());
        true
    }
}

impl Default for GpuAtlasTileCache {
    fn default() -> Self {
        Self::new(512 * 1024 * 1024)
    }
}

/// Reusable, bounded staging storage for GPU readbacks.  The executor rents these
/// buffers per tile rather than allocating full-atlas staging images.
#[derive(Debug)]
pub struct GpuAtlasReadbackPool {
    maximum_buffers: usize,
    available: Vec<GpuAtlasStagingBuffer>,
}

#[derive(Debug)]
struct GpuAtlasStagingBuffer {
    byte_len: u64,
    buffer: wgpu::Buffer,
}

impl GpuAtlasReadbackPool {
    #[must_use]
    pub fn new(maximum_buffers: usize) -> Self {
        Self {
            maximum_buffers,
            available: Vec::new(),
        }
    }

    pub fn acquire_staging(&mut self, device: &wgpu::Device, byte_len: u64) -> wgpu::Buffer {
        if let Some(index) = self.available.iter().position(|buffer| buffer.byte_len >= byte_len) {
            return self.available.swap_remove(index).buffer;
        }
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hot-trimmer-base-color-readback"),
            size: byte_len,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        })
    }

    pub fn release_staging(&mut self, buffer: wgpu::Buffer, byte_len: u64) {
        if self.available.len() < self.maximum_buffers {
            self.available.push(GpuAtlasStagingBuffer { byte_len, buffer });
        }
    }
}

impl Default for GpuAtlasReadbackPool {
    fn default() -> Self { Self::new(4) }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct GpuAtlasHeader {
    output_width: u32,
    output_height: u32,
    tile_x: u32,
    tile_y: u32,
    tile_width: u32,
    tile_height: u32,
    command_count: u32,
    source_width: u32,
    source_height: u32,
    pad: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct GpuRegionCommand {
    region_index: u32,
    mode: u32,
    crop_x: u32,
    crop_y: u32,
    crop_width: u32,
    crop_height: u32,
    dst_x: u32,
    dst_y: u32,
    dst_width: u32,
    dst_height: u32,
    semantic_x: u32,
    semantic_y: u32,
    semantic_width: u32,
    semantic_height: u32,
    period_x: u32,
    period_y: u32,
    rotation: u32,
    mirror: u32,
    filter: u32,
    transform_mirror_x: u32,
    transform_mirror_y: u32,
    pad0: u32,
    slot_width: f32,
    slot_height: f32,
    pixels_per_unit: f32,
    sampling_scale: f32,
    radial_center_x: f32,
    radial_center_y: f32,
    radial_inner_radius: f32,
    radial_outer_radius: f32,
    radial_falloff: f32,
    radial_blend_width: f32,
    radial_seam_blend_width: f32,
    transform_scale_x: f32,
    transform_scale_y: f32,
    transform_offset_x: f32,
    transform_offset_y: f32,
    transform_rotation_sin: f32,
    transform_rotation_cos: f32,
}

const GPU_HEADER_BYTES: usize = 48;
const GPU_COMMAND_BYTES: usize = 156;

impl AtlasRenderExecutor for GpuAtlasRenderExecutor<'_> {
    fn execute(
        &self,
        plan: &CompiledAtlasPlanV1,
        input: &AtlasRenderExecutionInput<'_>,
        cancellation: &CancellationToken,
        is_current: &dyn Fn() -> bool,
    ) -> Result<AtlasRenderExecutorOutput, AtlasRenderExecutionError> {
        plan.validate()
            .map_err(|error| AtlasRenderExecutionError::InvalidInput(error.to_string()))?;
        if cancellation.is_cancelled() {
            return Err(AtlasRenderExecutionError::Cancelled);
        }
        if !is_current() {
            return Err(AtlasRenderExecutionError::Superseded);
        }
        let identity = plan.tile_identity(
            hot_trimmer_domain::MaterialMapKind::BaseColor,
            "stage14-base-color-wgsl-v1",
        );
        if let Some(cached) = self.source_texture_cache.lock().ok().and_then(|mut cache| {
            cache.cached_tile(&identity, plan.tile_request.generation)
        }) {
            let (nontransparent, nonzero_rgb) = rgba_payload_counts(cached.pixels());
            return Ok(AtlasRenderExecutorOutput::FinalAtlas(AtlasFinalAtlasOutput {
                base_color_rgba8: cached.payload(),
                interactive_tile: Arc::clone(&cached),
                region_valid_pixel_counts: final_atlas_metadata(plan)?,
                render_ms: 0,
                source_cache_hits: 0,
                pipeline_cache_hits: 0,
                upload_bytes: 0,
                upload_ms: 0,
                command_count: 0,
                command_bytes: 0,
                dispatch_ms: 0,
                readback_bytes: 0,
                readback_ms: 0,
                telemetry: vec![format!("executor=gpu; plan_hash={}; gpu_tile_cache=hit; dispatch_ms=0; readback_ms=0; tile_nontransparent={nontransparent}; tile_nonzero_rgb={nonzero_rgb}", plan.final_plan_hash.0)],
            }));
        }
        let tile = plan.tile_request.output_rect.0;
        let tile_width = tile.width;
        let tile_height = tile.height;
        let started = Instant::now();
        let state = self
            .service
            .initialize()
            .map_err(|error| AtlasRenderExecutionError::Gpu(error.to_string()))?;
        let caps = state.capabilities();
        if tile_width > caps.maximum_texture_dimension_2d
            || tile_height > caps.maximum_texture_dimension_2d
        {
            return Err(AtlasRenderExecutionError::Gpu(format!(
                "tile {}x{} exceeds adapter 2D texture limit {}",
                tile_width, tile_height, caps.maximum_texture_dimension_2d
            )));
        }
        if !caps
            .candidate_formats
            .iter()
            .any(|format| format.format == "Rgba8Unorm" && format.sampled && format.storage)
        {
            return Err(AtlasRenderExecutionError::Gpu(
                "Rgba8Unorm sampling/storage support is required for direct Base Color atlas writes".into(),
            ));
        }
        let device = state.device();
        let queue = state.queue();
        let pipeline_started = Instant::now();
        let (pipeline, pipeline_cache_hit) = pipeline(device, self.source_texture_cache)?;
        let pipeline_ms = pipeline_started.elapsed().as_millis();

        let output_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hot-trimmer-base-color-output-tile"),
            size: wgpu::Extent3d {
                width: tile_width,
                height: tile_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let output_view = output_texture.create_view(&wgpu::TextureViewDescriptor::default());
        if !caps.clear_texture {
            return Err(AtlasRenderExecutionError::Gpu(
                "CLEAR_TEXTURE support is required to initialize the direct Base Color atlas before source batches".into(),
            ));
        }

        let mut upload_bytes = 0_u64;
        let mut source_cache_hits = 0_u32;
        let upload_started = Instant::now();
        let mut source_groups = Vec::with_capacity(plan.ordered_sources.len());
        for source in &plan.ordered_sources {
            if cancellation.is_cancelled() {
                return Err(AtlasRenderExecutionError::Cancelled);
            }
            if !is_current() {
                return Err(AtlasRenderExecutionError::Superseded);
            }
            if source.channel_role != MaterialChannelRole::BaseColor {
                continue;
            }
            let prepared = input
                .prepared_sources
                .iter()
                .find(|prepared| {
                    prepared.source_set_id == source.source_set_id
                        && prepared.source_id == source.source_id
                        && prepared.channel_role == MaterialChannelRole::BaseColor
                })
                .ok_or_else(|| AtlasRenderExecutionError::MissingPreparedSource {
                    source_set_id: source.source_set_id,
                    source_id: source.source_id.clone(),
                })?;
            let (cached, hit) = source_texture(
                device,
                queue,
                self.source_texture_cache,
                source,
                prepared.domain.as_ref(),
            )?;
            if hit {
                source_cache_hits = source_cache_hits.saturating_add(1);
            } else {
                upload_bytes = upload_bytes.saturating_add(cached.byte_len);
            }
            let commands = plan
                .ordered_regions
                .iter()
                .filter(|region| {
                    region.source_set_id == source.source_set_id
                        && region.source_id == source.source_id
                        && region.sampling_plan.candidate.source_id == source.source_id
                })
                .map(pack_command)
                .collect::<Result<Vec<_>, _>>()?;
            if !commands.is_empty() {
                source_groups.push((source, cached, commands));
            }
        }
        let upload_ms = upload_started.elapsed().as_millis();

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("hot-trimmer-base-color-atlas-encoder"),
        });
        encoder.clear_texture(
            &output_texture,
            &wgpu::ImageSubresourceRange {
                aspect: wgpu::TextureAspect::All,
                base_mip_level: 0,
                mip_level_count: Some(1),
                base_array_layer: 0,
                array_layer_count: Some(1),
            },
        );
        let dispatch_started = Instant::now();
        let mut command_count = 0_u32;
        let mut command_bytes = 0_u64;
        for (source, cached, commands) in source_groups {
            if cancellation.is_cancelled() {
                return Err(AtlasRenderExecutionError::Cancelled);
            }
            if !is_current() {
                return Err(AtlasRenderExecutionError::Superseded);
            }
            command_count = command_count.saturating_add(commands.len() as u32);
            let header = GpuAtlasHeader {
                output_width: plan.output_size.width,
                output_height: plan.output_size.height,
                tile_x: tile.x,
                tile_y: tile.y,
                tile_width,
                tile_height,
                command_count: commands.len() as u32,
                source_width: source.oriented_dimensions.width,
                source_height: source.oriented_dimensions.height,
                pad: [0; 3],
            };
            let header_bytes = encode_header(header);
            let command_buffer_bytes = encode_commands(&commands);
            command_bytes = command_bytes.saturating_add(command_buffer_bytes.len() as u64);
            let header_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("hot-trimmer-base-color-header"),
                contents: &header_bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let commands_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("hot-trimmer-base-color-region-commands"),
                contents: &command_buffer_bytes,
                usage: wgpu::BufferUsages::STORAGE,
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("hot-trimmer-base-color-bind-group"),
                layout: &pipeline.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: header_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: commands_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&cached.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&output_view),
                    },
                ],
            });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("hot-trimmer-base-color-dispatch"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&pipeline.pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups(
                    tile_width.div_ceil(16),
                    tile_height.div_ceil(16),
                    1,
                );
            }
        }
        let dispatch_ms = dispatch_started.elapsed().as_millis();
        let padded_bytes_per_row = align_to(
            u64::from(tile_width) * 4,
            u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
        );
        let readback_bytes = padded_bytes_per_row
            .checked_mul(u64::from(tile_height))
            .ok_or_else(|| {
                AtlasRenderExecutionError::Gpu("readback buffer size overflow".into())
            })?;
        let readback_buffer = self.source_texture_cache
            .lock()
            .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?
            .readback_pool
            .acquire_staging(device, readback_bytes);
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &output_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row as u32),
                    rows_per_image: Some(tile_height),
                },
            },
            wgpu::Extent3d {
                width: tile_width,
                height: tile_height,
                depth_or_array_layers: 1,
            },
        );
        if cancellation.is_cancelled() {
            return Err(AtlasRenderExecutionError::Cancelled);
        }
        if !is_current() {
            return Err(AtlasRenderExecutionError::Superseded);
        }
        queue.submit(Some(encoder.finish()));
        let readback_started = Instant::now();
        let slice = readback_buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        device.poll(wgpu::PollType::Wait).map_err(|error| {
            AtlasRenderExecutionError::Gpu(format!("device poll failed: {error:?}"))
        })?;
        receiver
            .recv()
            .map_err(|error| {
                AtlasRenderExecutionError::Gpu(format!("readback callback failed: {error}"))
            })?
            .map_err(|error| {
                AtlasRenderExecutionError::Gpu(format!("readback map failed: {error:?}"))
            })?;
        let mapped = slice.get_mapped_range();
        let output_row_bytes = usize::try_from(u64::from(tile_width) * 4)
            .map_err(|_| AtlasRenderExecutionError::Gpu("output row size overflow".into()))?;
        let padded_row_bytes = usize::try_from(padded_bytes_per_row)
            .map_err(|_| AtlasRenderExecutionError::Gpu("padded row size overflow".into()))?;
        let mut base_color_rgba8 = vec![0; output_row_bytes * tile_height as usize];
        for y in 0..tile_height as usize {
            let src = y * padded_row_bytes;
            let dst = y * output_row_bytes;
            base_color_rgba8[dst..dst + output_row_bytes]
                .copy_from_slice(&mapped[src..src + output_row_bytes]);
        }
        drop(mapped);
        readback_buffer.unmap();
        self.source_texture_cache
            .lock()
            .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?
            .readback_pool
            .release_staging(readback_buffer, readback_bytes);
        if !is_current() {
            return Err(AtlasRenderExecutionError::Superseded);
        }
        let readback_ms = readback_started.elapsed().as_millis();
        let region_valid_pixel_counts = final_atlas_metadata(plan)?;
        let render_ms = started.elapsed().as_millis();
        let pixels: Arc<[u8]> = Arc::from(base_color_rgba8);
        let (nontransparent, nonzero_rgb) = rgba_payload_counts(&pixels);
        let interactive_tile = Arc::new(GpuAtlasRenderedTile {
            manifest: crate::CompiledAtlasTileManifest {
                identity,
                map: hot_trimmer_domain::MaterialMapKind::BaseColor,
                mip_level: plan.tile_request.mip_level,
                output_rect: plan.tile_request.output_rect,
                valid_rect: plan.tile_request.valid_rect,
                halo_px: plan.tile_request.halo_px,
                generation: plan.tile_request.generation,
                pixel_format: crate::CompiledTilePixelFormat::Rgba8UnormSrgb,
                width: tile_width,
                height: tile_height,
                row_stride: tile_width.saturating_mul(4),
                opaque_handle: String::new(),
            },
            pixels: Arc::clone(&pixels),
        });
        self.source_texture_cache
            .lock()
            .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?
            .remember_tile(Arc::clone(&interactive_tile));
        Ok(AtlasRenderExecutorOutput::FinalAtlas(
            AtlasFinalAtlasOutput {
                base_color_rgba8: pixels,
                interactive_tile,
                region_valid_pixel_counts,
                render_ms,
                source_cache_hits,
                pipeline_cache_hits: u32::from(pipeline_cache_hit),
                upload_bytes,
                upload_ms,
                command_count,
                command_bytes,
                dispatch_ms,
                readback_bytes,
                readback_ms,
                telemetry: vec![format!(
                    "executor=gpu; backend={}; plan_hash={}; source_cache_hits={source_cache_hits}; pipeline_cache_hits={}; upload_bytes={upload_bytes}; upload_ms={upload_ms}; command_count={command_count}; command_bytes={command_bytes}; pipeline_ms={pipeline_ms}; dispatch_ms={dispatch_ms}; readback_bytes={readback_bytes}; readback_ms={readback_ms}; tile_nontransparent={nontransparent}; tile_nonzero_rgb={nonzero_rgb}; composition_ms=0; render_ms={render_ms}",
                    caps.backend,
                    plan.final_plan_hash.0,
                    u32::from(pipeline_cache_hit)
                )],
            },
        ))
    }

    fn compose(
        &self,
        _input: &AtlasComposeExecutionInput<'_>,
        _cancellation: &CancellationToken,
        _is_current: &dyn Fn() -> bool,
    ) -> Result<AtlasComposeExecutorOutput, AtlasRenderExecutionError> {
        Err(AtlasRenderExecutionError::Composition(
            "GPU executor publishes a final atlas and does not run CPU atlas composition".into(),
        ))
    }
}

fn pipeline(
    device: &wgpu::Device,
    cache: &Mutex<GpuAtlasSourceTextureCache>,
) -> Result<(Arc<GpuAtlasPipeline>, bool), AtlasRenderExecutionError> {
    let mut cache = cache
        .lock()
        .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?;
    if let Some(pipeline) = &cache.pipeline {
        return Ok((Arc::clone(pipeline), true));
    }
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("hot-trimmer-base-color-bind-layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(GPU_HEADER_BYTES as u64),
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(GPU_COMMAND_BYTES as u64),
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::WriteOnly,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    view_dimension: wgpu::TextureViewDimension::D2,
                },
                count: None,
            },
        ],
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("hot-trimmer-base-color-atlas-wgsl"),
        source: wgpu::ShaderSource::Wgsl(hot_trimmer_preview::BASE_COLOR_ATLAS_WGSL.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("hot-trimmer-base-color-pipeline-layout"),
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("hot-trimmer-base-color-pipeline"),
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: Some("main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });
    let pipeline = Arc::new(GpuAtlasPipeline {
        bind_group_layout,
        pipeline,
    });
    cache.pipeline = Some(Arc::clone(&pipeline));
    Ok((pipeline, false))
}

fn source_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    cache: &Mutex<GpuAtlasSourceTextureCache>,
    source: &CompiledSourceCommandV1,
    domain: &PreparedMaterialDomain,
) -> Result<(Arc<GpuCachedSourceTexture>, bool), AtlasRenderExecutionError> {
    let key = GpuSourceTextureKey::from_source(source);
    let mut cache_guard = cache
        .lock()
        .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?;
    cache_guard.clock = cache_guard.clock.saturating_add(1);
    let clock = cache_guard.clock;
    if let Some(texture) = cache_guard.sources.get(&key) {
        return Ok((Arc::clone(texture), true));
    }
    drop(cache_guard);
    let bytes = source_rgba8(domain)?;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("hot-trimmer-base-color-source-texture"),
        size: wgpu::Extent3d {
            width: source.oriented_dimensions.width,
            height: source.oriented_dimensions.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(source.oriented_dimensions.width * 4),
            rows_per_image: Some(source.oriented_dimensions.height),
        },
        wgpu::Extent3d {
            width: source.oriented_dimensions.width,
            height: source.oriented_dimensions.height,
            depth_or_array_layers: 1,
        },
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let cached = Arc::new(GpuCachedSourceTexture {
        _texture: texture,
        view,
        byte_len: bytes.len() as u64,
        last_used: clock,
    });
    let mut cache_guard = cache
        .lock()
        .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?;
    const MAX_GPU_SOURCES: usize = 8;
    if cache_guard.sources.len() >= MAX_GPU_SOURCES
        && !cache_guard.sources.contains_key(&key)
        && let Some(oldest) = cache_guard
            .sources
            .iter()
            .min_by_key(|(_, value)| value.last_used)
            .map(|(key, _)| key.clone())
    {
        cache_guard.sources.remove(&oldest);
    }
    cache_guard.sources.insert(key, Arc::clone(&cached));
    Ok((cached, false))
}

fn source_rgba8(domain: &PreparedMaterialDomain) -> Result<Vec<u8>, AtlasRenderExecutionError> {
    let channel = domain
        .registered_channels()
        .iter()
        .find(|channel| channel.role() == MaterialChannelRole::BaseColor)
        .ok_or_else(|| {
            AtlasRenderExecutionError::Gpu("prepared source has no Base Color channel".into())
        })?;
    let PreparedExemplarChannel::BaseColor { plane, .. } = channel else {
        return Err(AtlasRenderExecutionError::Gpu(
            "prepared Base Color channel has an unexpected representation".into(),
        ));
    };
    let mut rgba =
        Vec::with_capacity((u64::from(domain.width) * u64::from(domain.height) * 4) as usize);
    for y in 0..domain.height {
        for x in 0..domain.width {
            let value = plane.pixel(x, y);
            rgba.push(linear_to_srgb(value.rgb[0]));
            rgba.push(linear_to_srgb(value.rgb[1]));
            rgba.push(linear_to_srgb(value.rgb[2]));
            rgba.push(unit(value.alpha));
        }
    }
    Ok(rgba)
}

fn rgba_payload_counts(bytes: &[u8]) -> (usize, usize) {
    let mut nontransparent = 0;
    let mut nonzero_rgb = 0;
    for pixel in bytes.chunks_exact(4) {
        if pixel[3] != 0 {
            nontransparent += 1;
        }
        if pixel[0] != 0 || pixel[1] != 0 || pixel[2] != 0 {
            nonzero_rgb += 1;
        }
    }
    (nontransparent, nonzero_rgb)
}

fn pack_command(
    command: &CompiledRegionCommandV1,
) -> Result<GpuRegionCommand, AtlasRenderExecutionError> {
    let destination = command.destination_rect.0;
    let semantic = semantic_rect_for_padding(
        hot_trimmer_domain::CanonicalRect {
            x: destination.x,
            y: destination.y,
            width: destination.width,
            height: destination.height,
        },
        command.padding_px,
        command.edge_eligibility,
    );
    let mode = match command.sampling_plan.candidate.mapping_mode {
        hot_trimmer_domain::SamplingMode::DirectCrop
        | hot_trimmer_domain::SamplingMode::UniqueContain
        | hot_trimmer_domain::SamplingMode::UniqueCover => 0,
        hot_trimmer_domain::SamplingMode::PeriodicTile => 1,
        hot_trimmer_domain::SamplingMode::RepeatX => 2,
        hot_trimmer_domain::SamplingMode::RepeatY => 3,
        hot_trimmer_domain::SamplingMode::PlanarRadial => 4,
        hot_trimmer_domain::SamplingMode::PolarRadial => 5,
        hot_trimmer_domain::SamplingMode::ExplicitStretch => 6,
        unsupported => {
            return Err(AtlasRenderExecutionError::InvalidInput(format!(
                "region {} has unsupported GPU sampling mode {unsupported:?}",
                command.region_id
            )));
        }
    };
    let crop = command.source_crop.0;
    let period = command
        .sampling_plan
        .candidate
        .period_pixels
        .unwrap_or([crop.width.max(1), crop.height.max(1)]);
    let radial = command
        .radial_parameters
        .unwrap_or(hot_trimmer_domain::RadialMappingSettings {
            center_x: 0.5,
            center_y: 0.5,
            inner_radius: 0.0,
            outer_radius: 0.5,
            falloff: 1.0,
            blend_width: 0.0,
            seam_blend_width: 0.0,
        });
    Ok(GpuRegionCommand {
        region_index: command.compact_index,
        mode,
        crop_x: crop.x,
        crop_y: crop.y,
        crop_width: crop.width,
        crop_height: crop.height,
        dst_x: destination.x,
        dst_y: destination.y,
        dst_width: destination.width,
        dst_height: destination.height,
        semantic_x: semantic.x,
        semantic_y: semantic.y,
        semantic_width: semantic.width,
        semantic_height: semantic.height,
        period_x: period[0],
        period_y: period[1],
        rotation: match command.sampling_plan.candidate.transform.rotation {
            hot_trimmer_domain::QuarterTurn::Zero => 0,
            hot_trimmer_domain::QuarterTurn::Ninety => 1,
            hot_trimmer_domain::QuarterTurn::OneEighty => 2,
            hot_trimmer_domain::QuarterTurn::TwoSeventy => 3,
        },
        mirror: match command.sampling_plan.candidate.transform.mirror {
            MirrorTransform::None => 0,
            MirrorTransform::X => 1,
            MirrorTransform::Y => 2,
        },
        filter: u32::from(
            command.sampling_plan.sampling_policy.filter != SourceSamplingMode::Nearest,
        ),
        transform_mirror_x: u32::from(command.source_to_region_transform.mirror_x),
        transform_mirror_y: u32::from(command.source_to_region_transform.mirror_y),
        pad0: 0,
        slot_width: command.sampling_plan.slot_physical_size[0] as f32,
        slot_height: command.sampling_plan.slot_physical_size[1] as f32,
        pixels_per_unit: command.sampling_plan.source_pixels_per_physical_unit as f32,
        sampling_scale: command.sampling_plan.sampling_policy.scale as f32,
        radial_center_x: radial.center_x as f32,
        radial_center_y: radial.center_y as f32,
        radial_inner_radius: radial.inner_radius as f32,
        radial_outer_radius: radial.outer_radius as f32,
        radial_falloff: radial.falloff as f32,
        radial_blend_width: radial.blend_width as f32,
        radial_seam_blend_width: radial.seam_blend_width as f32,
        transform_scale_x: command.source_to_region_transform.scale[0] as f32,
        transform_scale_y: command.source_to_region_transform.scale[1] as f32,
        transform_offset_x: command.source_to_region_transform.offset[0] as f32,
        transform_offset_y: command.source_to_region_transform.offset[1] as f32,
        transform_rotation_sin: (-command.source_to_region_transform.rotation_degrees.to_radians())
            .sin() as f32,
        transform_rotation_cos: (-command.source_to_region_transform.rotation_degrees.to_radians())
            .cos() as f32,
    })
}

fn final_atlas_metadata(
    plan: &CompiledAtlasPlanV1,
) -> Result<Vec<(RegionId, u64)>, AtlasRenderExecutionError> {
    plan.ordered_regions
        .iter()
        .map(|command| {
            Ok((command.region_id, region_valid_pixel_count(command)?))
        })
        .collect()
}

fn region_valid_pixel_count(
    command: &CompiledRegionCommandV1,
) -> Result<u64, AtlasRenderExecutionError> {
    if command.sampling_plan.candidate.mapping_mode != hot_trimmer_domain::SamplingMode::PolarRadial
    {
        let destination = command.destination_rect.0;
        return u64::from(destination.width)
            .checked_mul(u64::from(destination.height))
            .ok_or_else(|| {
                AtlasRenderExecutionError::Gpu(format!(
                    "region {} valid-pixel count overflow",
                    command.region_id
                ))
            });
    }
    let destination = command.destination_rect.0;
    let semantic = semantic_rect_for_padding(
        hot_trimmer_domain::CanonicalRect {
            x: destination.x,
            y: destination.y,
            width: destination.width,
            height: destination.height,
        },
        command.padding_px,
        command.edge_eligibility,
    );
    let radial = command
        .radial_parameters
        .unwrap_or(hot_trimmer_domain::RadialMappingSettings {
            center_x: 0.5,
            center_y: 0.5,
            inner_radius: 0.0,
            outer_radius: 0.5,
            falloff: 1.0,
            blend_width: 0.0,
            seam_blend_width: 0.0,
        });
    let transform = command.sampling_plan.candidate.transform;
    let mut count = 0_u64;
    for y in destination.y..destination.y + destination.height {
        let sem_y = y.clamp(semantic.y, semantic.y + semantic.height - 1) - semantic.y;
        let q_y = (f64::from(sem_y) + 0.5) / f64::from(semantic.height);
        for x in destination.x..destination.x + destination.width {
            let sem_x = x.clamp(semantic.x, semantic.x + semantic.width - 1) - semantic.x;
            let q_x = (f64::from(sem_x) + 0.5) / f64::from(semantic.width);
            let radial_local =
                transform_local_f64([q_x - radial.center_x, q_y - radial.center_y], transform);
            if radial_local[0].hypot(radial_local[1]) <= radial.outer_radius {
                count = count.saturating_add(1);
            }
        }
    }
    Ok(count)
}

fn transform_local_f64(
    mut point: [f64; 2],
    transform: hot_trimmer_placement_solver::CandidateTransform,
) -> [f64; 2] {
    match transform.mirror {
        MirrorTransform::X => point[0] = -point[0],
        MirrorTransform::Y => point[1] = -point[1],
        MirrorTransform::None => {}
    }
    match transform.rotation {
        hot_trimmer_domain::QuarterTurn::Zero => point,
        hot_trimmer_domain::QuarterTurn::Ninety => [point[1], -point[0]],
        hot_trimmer_domain::QuarterTurn::OneEighty => [-point[0], -point[1]],
        hot_trimmer_domain::QuarterTurn::TwoSeventy => [-point[1], point[0]],
    }
}

fn encode_header(header: GpuAtlasHeader) -> [u8; GPU_HEADER_BYTES] {
    let mut bytes = [0_u8; GPU_HEADER_BYTES];
    let values = [
        header.output_width,
        header.output_height,
        header.tile_x,
        header.tile_y,
        header.tile_width,
        header.tile_height,
        header.command_count,
        header.source_width,
        header.source_height,
        header.pad[0],
        header.pad[1],
        header.pad[2],
    ];
    for (index, value) in values.into_iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn encode_commands(commands: &[GpuRegionCommand]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(commands.len() * GPU_COMMAND_BYTES);
    for command in commands {
        for value in [
            command.region_index,
            command.mode,
            command.crop_x,
            command.crop_y,
            command.crop_width,
            command.crop_height,
            command.dst_x,
            command.dst_y,
            command.dst_width,
            command.dst_height,
            command.semantic_x,
            command.semantic_y,
            command.semantic_width,
            command.semantic_height,
            command.period_x,
            command.period_y,
            command.rotation,
            command.mirror,
            command.filter,
            command.transform_mirror_x,
            command.transform_mirror_y,
            command.pad0,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in [
            command.slot_width,
            command.slot_height,
            command.pixels_per_unit,
            command.sampling_scale,
            command.radial_center_x,
            command.radial_center_y,
            command.radial_inner_radius,
            command.radial_outer_radius,
            command.radial_falloff,
            command.radial_blend_width,
            command.radial_seam_blend_width,
            command.transform_scale_x,
            command.transform_scale_y,
            command.transform_offset_x,
            command.transform_offset_y,
            command.transform_rotation_sin,
            command.transform_rotation_cos,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    debug_assert_eq!(bytes.len(), commands.len() * GPU_COMMAND_BYTES);
    bytes
}

fn align_to(value: u64, alignment: u64) -> u64 {
    value.div_ceil(alignment) * alignment
}

fn unit(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn linear_to_srgb(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    unit(if value <= 0.003_130_8 {
        12.92 * value
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    })
}
