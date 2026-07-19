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
    CancellationToken, ContentDigest, MaterialChannelRole, MaterialMapKind, RegionId,
    SourceSamplingMode, SourceSetId,
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

#[derive(Clone, Debug)]
pub struct AtlasFinalAtlasOutput {
    /// All requested material-map tiles keyed by their authored map kind.
    pub map_tiles: BTreeMap<MaterialMapKind, Arc<GpuAtlasRenderedTile>>,
    /// GPU-produced RGBA display publications keyed by authored map kind.
    pub display_tiles: BTreeMap<MaterialMapKind, Arc<GpuAtlasRenderedTile>>,
    /// Dependency outputs produced while satisfying the requested set. These are
    /// not necessarily publication channels.
    pub intermediate_tiles: BTreeMap<String, Arc<GpuAtlasRenderedTile>>,
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
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    #[must_use]
    pub fn payload(&self) -> Arc<[u8]> {
        Arc::clone(&self.pixels)
    }

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
            MaskValue(
                if original.0 >= 0.5 && sample_offset_validity(domain, *position) {
                    1.0
                } else {
                    0.0
                },
            )
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
            PreparedExemplarChannel::BaseColor {
                plane: _,
                alpha_mode,
            } => {
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
    if at[0] < 0.0 || at[1] < 0.0 || at[0] >= domain.width as f32 || at[1] >= domain.height as f32 {
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
        return value(plane.pixel(
            if tx < 0.5 { x0 } else { x1 },
            if ty < 0.5 { y0 } else { y1 },
        ));
    }
    let a = value(plane.pixel(x0, y0)) * (1.0 - tx) + value(plane.pixel(x1, y0)) * tx;
    let b = value(plane.pixel(x0, y1)) * (1.0 - tx) + value(plane.pixel(x1, y1)) * tx;
    a * (1.0 - ty) + b * ty
}

fn sample_offset_color(plane: &ImagePlane<LinearColor>, at: [f32; 2], linear: bool) -> LinearColor {
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum GpuAtlasPipelineKind {
    MaterialRgba8,
    MaterialR32Float,
    FillR32Float,
    NormalFromHeight,
    RegionIdR32Uint,
    RegionIdDisplayRgba8,
}

#[derive(Default)]
pub struct GpuAtlasSourceTextureCache {
    clock: u64,
    sources: BTreeMap<GpuSourceTextureKey, Arc<GpuCachedSourceTexture>>,
    pipelines: BTreeMap<GpuAtlasPipelineKind, Arc<GpuAtlasPipeline>>,
    rendered_tiles: Vec<Arc<GpuAtlasRenderedTile>>,
    rendered_textures: Vec<Arc<GpuCachedRenderedTexture>>,
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
        while self.rendered_tiles.len() > 32 {
            self.rendered_tiles.remove(0);
        }
    }

    fn cached_rendered_texture(
        &mut self,
        identity: &crate::CompiledAtlasTileIdentity,
    ) -> Option<Arc<GpuCachedRenderedTexture>> {
        let pixel_identity = identity.pixel_identity();
        self.rendered_textures
            .iter()
            .find(|texture| texture.pixel_identity == pixel_identity)
            .cloned()
    }

    fn remember_rendered_texture(
        &mut self,
        identity: crate::CompiledAtlasTileIdentity,
        texture: wgpu::Texture,
        view: wgpu::TextureView,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) {
        let pixel_identity = identity.pixel_identity();
        self.rendered_textures
            .retain(|existing| existing.pixel_identity != pixel_identity);
        self.rendered_textures.push(Arc::new(GpuCachedRenderedTexture {
            pixel_identity,
            _texture: texture,
            view,
            width,
            height,
            format,
        }));
        while self.rendered_textures.len() > 16 {
            self.rendered_textures.remove(0);
        }
    }
}

pub struct GpuCachedRenderedTexture {
    pixel_identity: crate::CompiledAtlasPixelIdentity,
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
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
        self.tiles
            .retain(|tile| tile.manifest.generation >= self.active_generation);
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
                pixels.len(),
                expected_bytes
            )));
        }
        if pixels.len() > self.byte_capacity {
            return Err(AtlasRenderExecutionError::Gpu(
                "tile payload exceeds the configured bounded cache".into(),
            ));
        }
        self.active_generation = self.active_generation.max(manifest.generation);
        while self.used_bytes + pixels.len() > self.byte_capacity {
            let Some(evicted) = self.tiles.first() else {
                break;
            };
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
        if let Some(index) = self
            .available
            .iter()
            .position(|buffer| buffer.byte_len >= byte_len)
        {
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
            self.available
                .push(GpuAtlasStagingBuffer { byte_len, buffer });
        }
    }
}

impl Default for GpuAtlasReadbackPool {
    fn default() -> Self {
        Self::new(4)
    }
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
    map_kind: u32,
    normal_convention: u32,
    source_role: u32,
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
    structural_profile: u32,
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
const GPU_SHADER_VERSION: &str = "stage14-material-map-wgsl-v8-radial-interior-extension";

fn requested_material_maps(
    plan: &CompiledAtlasPlanV1,
) -> Result<Vec<MaterialMapKind>, AtlasRenderExecutionError> {
    let maps = if plan.requested_maps.is_empty() {
        vec![MaterialMapKind::BaseColor]
    } else {
        plan.requested_maps.clone()
    };
    let mut unique_maps = Vec::with_capacity(maps.len());
    for map in &maps {
        validate_gpu_material_map(*map)?;
        if !unique_maps.contains(map) {
            unique_maps.push(*map);
        }
    }
    Ok(unique_maps)
}

fn validate_gpu_material_map(map: MaterialMapKind) -> Result<(), AtlasRenderExecutionError> {
    match map {
        MaterialMapKind::BaseColor
        | MaterialMapKind::Height
        | MaterialMapKind::Normal
        | MaterialMapKind::Roughness
        | MaterialMapKind::Metallic
        | MaterialMapKind::AmbientOcclusion
        | MaterialMapKind::RegionId => Ok(()),
        MaterialMapKind::Specular
        | MaterialMapKind::Opacity
        | MaterialMapKind::EdgeMask
        | MaterialMapKind::MaterialId => Err(AtlasRenderExecutionError::InvalidInput(format!(
            "GPU material map {map:?} is unavailable in the current material contract"
        ))),
    }
}

fn logical_passes_for_map(map: MaterialMapKind) -> &'static str {
    match map {
        MaterialMapKind::BaseColor => "registered-source,publish",
        MaterialMapKind::Height => {
            "registered-source,hotspot-profile,structural-height,material-height,final-height,publish"
        }
        MaterialMapKind::Normal => {
            "registered-source,hotspot-profile,structural-height,material-height,final-height,normal,publish"
        }
        MaterialMapKind::Roughness => {
            "registered-source,hotspot-profile,structural-height,roughness,publish"
        }
        MaterialMapKind::AmbientOcclusion => "hotspot-profile,structural-height,ao,publish",
        MaterialMapKind::Metallic => "registered-source,metallic,publish",
        MaterialMapKind::RegionId => "compact-region-id,publish",
        MaterialMapKind::Specular
        | MaterialMapKind::Opacity
        | MaterialMapKind::EdgeMask
        | MaterialMapKind::MaterialId => "unavailable",
    }
}

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
        let requested_maps = requested_material_maps(plan)?;
        if requested_maps.len() > 1 {
            let mut first_output = None::<AtlasFinalAtlasOutput>;
            let mut map_tiles = BTreeMap::<MaterialMapKind, Arc<GpuAtlasRenderedTile>>::new();
            let mut display_tiles = BTreeMap::<MaterialMapKind, Arc<GpuAtlasRenderedTile>>::new();
            let mut intermediate_tiles = BTreeMap::<String, Arc<GpuAtlasRenderedTile>>::new();
            let mut telemetry = vec![format!(
                "executor=gpu; plan_hash={}; requested_maps={requested_maps:?}; executed_gpu_passes=map-set; dependency_cache=enabled",
                plan.final_plan_hash.0
            )];
            let mut render_ms = 0;
            let mut source_cache_hits = 0;
            let mut pipeline_cache_hits = 0;
            let mut upload_bytes = 0;
            let mut upload_ms = 0;
            let mut command_count = 0;
            let mut command_bytes = 0;
            let mut dispatch_ms = 0;
            let mut readback_bytes = 0;
            let mut readback_ms = 0;
            let mut consumed_maps = Vec::<MaterialMapKind>::new();

            if requested_maps.contains(&MaterialMapKind::Normal) {
                let cached_normal = self.source_texture_cache.lock().ok().and_then(|mut cache| {
                    let identity = plan.tile_identity(MaterialMapKind::Normal, GPU_SHADER_VERSION);
                    cache.cached_tile(&identity, plan.tile_request.generation)
                });
                if let Some(cached_normal) = cached_normal {
                    let cached_height =
                        self.source_texture_cache
                            .lock()
                            .ok()
                            .and_then(|mut cache| {
                                let identity =
                                    plan.tile_identity(MaterialMapKind::Height, GPU_SHADER_VERSION);
                                cache.cached_tile(&identity, plan.tile_request.generation)
                            });
                    let cached_height_display =
                        self.source_texture_cache
                            .lock()
                            .ok()
                            .and_then(|mut cache| {
                                let identity = display_tile_identity(plan, MaterialMapKind::Height);
                                cache.cached_tile(&identity, plan.tile_request.generation)
                            });
                    map_tiles.insert(MaterialMapKind::Normal, Arc::clone(&cached_normal));
                    display_tiles.insert(MaterialMapKind::Normal, cached_normal);
                    consumed_maps.push(MaterialMapKind::Normal);
                    if requested_maps.contains(&MaterialMapKind::Height)
                        && let Some(cached_height) = cached_height
                    {
                        intermediate_tiles
                            .insert("final-height".into(), Arc::clone(&cached_height));
                        intermediate_tiles
                            .insert("normal.final-height".into(), Arc::clone(&cached_height));
                        map_tiles.insert(MaterialMapKind::Height, cached_height);
                        if let Some(cached_height_display) = cached_height_display {
                            display_tiles.insert(MaterialMapKind::Height, cached_height_display);
                        }
                        consumed_maps.push(MaterialMapKind::Height);
                    }
                    telemetry.push(format!(
                        "executor=gpu; requested_map=Normal; logical_passes={}; executed_gpu_passes=none; final_tile_cache=hit; dependency=Normal<-Height; intermediate_cache={}; gpu_tile_cache=hit; dispatch_ms=0; readback_ms=0",
                        logical_passes_for_map(MaterialMapKind::Normal),
                        if map_tiles.contains_key(&MaterialMapKind::Height) {
                            "final-height:persistent-cache-hit"
                        } else {
                            "final-height:not-requested"
                        }
                    ));
                } else {
                    let output = execute_height_normal_gpu(
                        self,
                        plan,
                        input,
                        requested_maps.contains(&MaterialMapKind::Height),
                        cancellation,
                        is_current,
                    )?;
                    first_output = Some(output.clone());
                    render_ms += output.render_ms;
                    source_cache_hits += output.source_cache_hits;
                    pipeline_cache_hits += output.pipeline_cache_hits;
                    upload_bytes += output.upload_bytes;
                    upload_ms += output.upload_ms;
                    command_count += output.command_count;
                    command_bytes += output.command_bytes;
                    dispatch_ms += output.dispatch_ms;
                    readback_bytes += output.readback_bytes;
                    readback_ms += output.readback_ms;
                    for (map, tile) in output.map_tiles {
                        if requested_maps.contains(&map) {
                            map_tiles.insert(map, tile);
                        }
                    }
                    for (map, tile) in output.display_tiles {
                        if requested_maps.contains(&map) {
                            display_tiles.insert(map, tile);
                        }
                    }
                    intermediate_tiles.extend(output.intermediate_tiles);
                    telemetry.extend(output.telemetry);
                    consumed_maps.push(MaterialMapKind::Normal);
                    consumed_maps.push(MaterialMapKind::Height);
                }
            }

            for render_map in &requested_maps {
                if consumed_maps.contains(render_map) {
                    continue;
                }
                let mut single_plan = plan.clone();
                single_plan.requested_maps = vec![*render_map];
                single_plan.final_plan_hash = ContentDigest(String::new());
                single_plan = single_plan
                    .finalize()
                    .map_err(|error| AtlasRenderExecutionError::InvalidInput(error.to_string()))?;
                let output = match self.execute(&single_plan, input, cancellation, is_current)? {
                    AtlasRenderExecutorOutput::FinalAtlas(output) => output,
                    AtlasRenderExecutorOutput::CpuRegions(_) => {
                        return Err(AtlasRenderExecutionError::Gpu(
                            "GPU map-set execution returned CPU regions".into(),
                        ));
                    }
                };
                if first_output.is_none() {
                    first_output = Some(output.clone());
                }
                render_ms += output.render_ms;
                source_cache_hits += output.source_cache_hits;
                pipeline_cache_hits += output.pipeline_cache_hits;
                upload_bytes += output.upload_bytes;
                upload_ms += output.upload_ms;
                command_count += output.command_count;
                command_bytes += output.command_bytes;
                dispatch_ms += output.dispatch_ms;
                readback_bytes += output.readback_bytes;
                readback_ms += output.readback_ms;
                let tile = output
                    .map_tiles
                    .get(render_map)
                    .cloned()
                    .unwrap_or_else(|| Arc::clone(&output.interactive_tile));
                let display_tile = output
                    .display_tiles
                    .get(render_map)
                    .cloned()
                    .unwrap_or_else(|| Arc::clone(&output.interactive_tile));
                let adjusted = Arc::new(tile.with_publication_identity(
                    plan.tile_identity(*render_map, GPU_SHADER_VERSION),
                    plan.tile_request.generation,
                ));
                let display_adjusted = Arc::new(display_tile.with_publication_identity(
                    display_tile_identity(plan, *render_map),
                    plan.tile_request.generation,
                ));
                self.source_texture_cache
                    .lock()
                    .map_err(|_| {
                        AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into())
                    })?
                    .remember_tile(Arc::clone(&adjusted));
                self.source_texture_cache
                    .lock()
                    .map_err(|_| {
                        AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into())
                    })?
                    .remember_tile(Arc::clone(&display_adjusted));
                if *render_map == MaterialMapKind::Height {
                    intermediate_tiles.insert("final-height".into(), Arc::clone(&adjusted));
                }
                if requested_maps.contains(render_map) {
                    map_tiles.insert(*render_map, adjusted);
                    display_tiles.insert(*render_map, display_adjusted);
                }
                telemetry.extend(output.telemetry);
            }

            let region_valid_pixel_counts = if let Some(output) = first_output {
                output.region_valid_pixel_counts
            } else {
                final_atlas_metadata(plan)?
            };
            let interactive_tile = requested_maps
                .iter()
                .find_map(|map| display_tiles.get(map).or_else(|| map_tiles.get(map)))
                .cloned()
                .ok_or_else(|| {
                    AtlasRenderExecutionError::Gpu("GPU map-set produced no tiles".into())
                })?;
            let base_color_rgba8 = map_tiles
                .get(&MaterialMapKind::BaseColor)
                .or_else(|| display_tiles.get(&MaterialMapKind::BaseColor))
                .unwrap_or(&interactive_tile)
                .payload();
            return Ok(AtlasRenderExecutorOutput::FinalAtlas(
                AtlasFinalAtlasOutput {
                    map_tiles,
                    display_tiles,
                    intermediate_tiles,
                    base_color_rgba8,
                    interactive_tile,
                    region_valid_pixel_counts,
                    render_ms,
                    source_cache_hits,
                    pipeline_cache_hits,
                    upload_bytes,
                    upload_ms,
                    command_count,
                    command_bytes,
                    dispatch_ms,
                    readback_bytes,
                    readback_ms,
                    telemetry,
                },
            ));
        }
        let requested_map = requested_maps[0];
        let logical_passes = logical_passes_for_map(requested_map);
        let identity = plan.tile_identity(requested_map, GPU_SHADER_VERSION);
        if let Some(cached) = self
            .source_texture_cache
            .lock()
            .ok()
            .and_then(|mut cache| cache.cached_tile(&identity, plan.tile_request.generation))
        {
            let interactive_tile = if matches!(
                requested_map,
                MaterialMapKind::Height
                    | MaterialMapKind::Roughness
                    | MaterialMapKind::Metallic
                    | MaterialMapKind::AmbientOcclusion
            ) {
                let display_identity = display_tile_identity(plan, requested_map);
                self.source_texture_cache
                    .lock()
                    .ok()
                    .and_then(|mut cache| {
                        cache.cached_tile(&display_identity, plan.tile_request.generation)
                    })
                    .unwrap_or_else(|| Arc::clone(&cached))
            } else {
                Arc::clone(&cached)
            };
            let (nontransparent, nonzero_rgb) =
                payload_counts(interactive_tile.pixels(), interactive_tile.manifest.pixel_format);
            let mut map_tiles = BTreeMap::new();
            map_tiles.insert(requested_map, Arc::clone(&cached));
            let mut display_tiles = BTreeMap::new();
            display_tiles.insert(requested_map, Arc::clone(&interactive_tile));
            return Ok(AtlasRenderExecutorOutput::FinalAtlas(
                AtlasFinalAtlasOutput {
                    map_tiles,
                    display_tiles,
                    intermediate_tiles: BTreeMap::new(),
                    base_color_rgba8: interactive_tile.payload(),
                    interactive_tile,
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
                    telemetry: vec![format!(
                        "executor=gpu; plan_hash={}; requested_map={requested_map:?}; logical_passes={logical_passes}; executed_gpu_passes=none; final_tile_cache=hit; intermediate_cache=not-available; gpu_tile_cache=hit; dispatch_ms=0; readback_ms=0; tile_nontransparent={nontransparent}; tile_nonzero_rgb={nonzero_rgb}",
                        plan.final_plan_hash.0
                    )],
                },
            ));
        }
        if requested_map == MaterialMapKind::RegionId {
            return execute_region_id_gpu_tile(self, plan, cancellation, is_current);
        }
        if requested_map == MaterialMapKind::Normal {
            return Ok(AtlasRenderExecutorOutput::FinalAtlas(
                execute_height_normal_gpu(self, plan, input, false, cancellation, is_current)?,
            ));
        }
        if matches!(
            requested_map,
            MaterialMapKind::Height
                | MaterialMapKind::Roughness
                | MaterialMapKind::Metallic
                | MaterialMapKind::AmbientOcclusion
        ) {
            return execute_r32float_material_tile(
                self,
                plan,
                input,
                requested_map,
                cancellation,
                is_current,
            );
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
        let (pipeline, pipeline_cache_hit) = pipeline(
            device,
            self.source_texture_cache,
            GpuAtlasPipelineKind::MaterialRgba8,
        )?;
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
            let source_role = source_channel_role_for_source(plan, source, requested_map);
            if source.channel_role != source_role {
                continue;
            }
            let prepared = input
                .prepared_sources
                .iter()
                .find(|prepared| {
                    prepared.source_set_id == source.source_set_id
                        && prepared.source_id == source.source_id
                        && prepared.channel_role == source_role
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
        let mut timing =
            GpuPassTimingRecorder::new(device, queue, caps, (plan.ordered_sources.len() as u32).saturating_add(1));
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
                map_kind: gpu_map_code(requested_map),
                normal_convention: match plan.normal_convention {
                    crate::CompiledNormalConvention::OpenGl => 0,
                    crate::CompiledNormalConvention::DirectX => 1,
                },
                source_role: gpu_channel_role_code(source.channel_role),
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
                let timestamp_writes = timing.as_mut().and_then(|recorder| {
                    recorder.timestamp_writes(format!("material-publish:{:?}", source.source_id))
                });
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("hot-trimmer-base-color-dispatch"),
                    timestamp_writes,
                });
                pass.set_pipeline(&pipeline.pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups(tile_width.div_ceil(16), tile_height.div_ceil(16), 1);
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
        let readback_buffer = self
            .source_texture_cache
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
        if let Some(timing) = &timing {
            timing.resolve(&mut encoder);
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
        let mut tile_rgba8 = vec![0; output_row_bytes * tile_height as usize];
        for y in 0..tile_height as usize {
            let src = y * padded_row_bytes;
            let dst = y * output_row_bytes;
            tile_rgba8[dst..dst + output_row_bytes]
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
        let pixels: Arc<[u8]> = Arc::from(tile_rgba8);
        let (nontransparent, nonzero_rgb) = rgba_payload_counts(&pixels);
        let pixel_format = identity.pixel_format;
        let interactive_tile = Arc::new(GpuAtlasRenderedTile {
            manifest: crate::CompiledAtlasTileManifest {
                identity,
                map: requested_map,
                mip_level: plan.tile_request.mip_level,
                output_rect: plan.tile_request.output_rect,
                valid_rect: plan.tile_request.valid_rect,
                halo_px: plan.tile_request.halo_px,
                generation: plan.tile_request.generation,
                pixel_format,
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
        let mut map_tiles = BTreeMap::new();
        map_tiles.insert(requested_map, Arc::clone(&interactive_tile));
        let mut display_tiles = BTreeMap::new();
        display_tiles.insert(requested_map, Arc::clone(&interactive_tile));
        let mut telemetry = vec![format!(
            "executor=gpu; backend={}; plan_hash={}; requested_map={requested_map:?}; logical_passes={logical_passes}; executed_gpu_passes=material-publish; final_tile_cache=miss; intermediate_cache=not-available; source_cache_hits={source_cache_hits}; pipeline_cache_hits={}; upload_bytes={upload_bytes}; upload_ms={upload_ms}; command_count={command_count}; command_bytes={command_bytes}; pipeline_ms={pipeline_ms}; dispatch_ms={dispatch_ms}; readback_bytes={readback_bytes}; readback_ms={readback_ms}; tile_nontransparent={nontransparent}; tile_nonzero_rgb={nonzero_rgb}; composition_ms=0; render_ms={render_ms}",
            caps.backend,
            plan.final_plan_hash.0,
            u32::from(pipeline_cache_hit)
        )];
        if let Some(timing) = timing {
            telemetry.extend(timing.finish(device)?);
        }
        Ok(AtlasRenderExecutorOutput::FinalAtlas(
            AtlasFinalAtlasOutput {
                map_tiles,
                display_tiles,
                intermediate_tiles: BTreeMap::new(),
                base_color_rgba8: Arc::clone(&pixels),
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
                telemetry,
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
    kind: GpuAtlasPipelineKind,
) -> Result<(Arc<GpuAtlasPipeline>, bool), AtlasRenderExecutionError> {
    let mut cache = cache
        .lock()
        .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?;
    if let Some(pipeline) = cache.pipelines.get(&kind) {
        return Ok((Arc::clone(pipeline), true));
    }
    let entries = match kind {
        GpuAtlasPipelineKind::MaterialRgba8 => vec![
            header_layout_entry(0),
            command_layout_entry(1),
            texture_layout_entry(2),
            storage_texture_layout_entry(3, wgpu::TextureFormat::Rgba8Unorm),
        ],
        GpuAtlasPipelineKind::NormalFromHeight => vec![
            header_layout_entry(0),
            command_layout_entry(1),
            texture_layout_entry(2),
            texture_layout_entry(3),
            storage_texture_layout_entry(4, wgpu::TextureFormat::Rgba8Unorm),
        ],
        GpuAtlasPipelineKind::MaterialR32Float => vec![
            header_layout_entry(0),
            command_layout_entry(1),
            texture_layout_entry(2),
            storage_texture_layout_entry(3, wgpu::TextureFormat::R32Float),
        ],
        GpuAtlasPipelineKind::FillR32Float => vec![storage_texture_layout_entry(
            0,
            wgpu::TextureFormat::R32Float,
        )],
        GpuAtlasPipelineKind::RegionIdR32Uint => vec![
            header_layout_entry(0),
            command_layout_entry(1),
            storage_texture_layout_entry(2, wgpu::TextureFormat::R32Uint),
        ],
        GpuAtlasPipelineKind::RegionIdDisplayRgba8 => vec![
            uint_texture_layout_entry(0),
            uint_storage_layout_entry(1),
            storage_texture_layout_entry(2, wgpu::TextureFormat::Rgba8Unorm),
        ],
    };
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(match kind {
            GpuAtlasPipelineKind::MaterialRgba8 => "hot-trimmer-material-rgba8-bind-layout",
            GpuAtlasPipelineKind::MaterialR32Float => "hot-trimmer-material-r32float-bind-layout",
            GpuAtlasPipelineKind::FillR32Float => "hot-trimmer-fill-r32float-bind-layout",
            GpuAtlasPipelineKind::NormalFromHeight => "hot-trimmer-normal-from-height-bind-layout",
            GpuAtlasPipelineKind::RegionIdR32Uint => "hot-trimmer-region-id-bind-layout",
            GpuAtlasPipelineKind::RegionIdDisplayRgba8 => "hot-trimmer-region-id-display-bind-layout",
        }),
        entries: &entries,
    });
    let shader_source = match kind {
        GpuAtlasPipelineKind::MaterialRgba8 => hot_trimmer_preview::BASE_COLOR_ATLAS_WGSL.into(),
        GpuAtlasPipelineKind::MaterialR32Float => hot_trimmer_preview::BASE_COLOR_ATLAS_WGSL
            .replace(
                "var out_tex: texture_storage_2d<rgba8unorm, write>;",
                "var out_tex: texture_storage_2d<r32float, write>;",
            )
            .into(),
        GpuAtlasPipelineKind::FillR32Float => hot_trimmer_preview::FILL_R32_FLOAT_ATLAS_WGSL.into(),
        GpuAtlasPipelineKind::NormalFromHeight => {
            hot_trimmer_preview::NORMAL_FROM_HEIGHT_ATLAS_WGSL.into()
        }
        GpuAtlasPipelineKind::RegionIdR32Uint => hot_trimmer_preview::REGION_ID_ATLAS_WGSL.into(),
        GpuAtlasPipelineKind::RegionIdDisplayRgba8 => {
            hot_trimmer_preview::REGION_ID_DISPLAY_ATLAS_WGSL.into()
        }
    };
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(match kind {
            GpuAtlasPipelineKind::MaterialRgba8 => "hot-trimmer-material-rgba8-wgsl",
            GpuAtlasPipelineKind::MaterialR32Float => "hot-trimmer-material-r32float-wgsl",
            GpuAtlasPipelineKind::FillR32Float => "hot-trimmer-fill-r32float-wgsl",
            GpuAtlasPipelineKind::NormalFromHeight => "hot-trimmer-normal-from-height-wgsl",
            GpuAtlasPipelineKind::RegionIdR32Uint => "hot-trimmer-region-id-wgsl",
            GpuAtlasPipelineKind::RegionIdDisplayRgba8 => "hot-trimmer-region-id-display-wgsl",
        }),
        source: wgpu::ShaderSource::Wgsl(shader_source),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("hot-trimmer-atlas-pipeline-layout"),
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(match kind {
            GpuAtlasPipelineKind::MaterialRgba8 => "hot-trimmer-material-rgba8-pipeline",
            GpuAtlasPipelineKind::MaterialR32Float => "hot-trimmer-material-r32float-pipeline",
            GpuAtlasPipelineKind::FillR32Float => "hot-trimmer-fill-r32float-pipeline",
            GpuAtlasPipelineKind::NormalFromHeight => "hot-trimmer-normal-from-height-pipeline",
            GpuAtlasPipelineKind::RegionIdR32Uint => "hot-trimmer-region-id-pipeline",
            GpuAtlasPipelineKind::RegionIdDisplayRgba8 => "hot-trimmer-region-id-display-pipeline",
        }),
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
    cache.pipelines.insert(kind, Arc::clone(&pipeline));
    Ok((pipeline, false))
}

fn header_layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: NonZeroU64::new(GPU_HEADER_BYTES as u64),
        },
        count: None,
    }
}

fn command_layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: NonZeroU64::new(GPU_COMMAND_BYTES as u64),
        },
        count: None,
    }
}

fn uint_storage_layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: NonZeroU64::new(std::mem::size_of::<u32>() as u64),
        },
        count: None,
    }
}

fn texture_layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn uint_texture_layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Uint,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn storage_texture_layout_entry(
    binding: u32,
    format: wgpu::TextureFormat,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::StorageTexture {
            access: wgpu::StorageTextureAccess::WriteOnly,
            format,
            view_dimension: wgpu::TextureViewDimension::D2,
        },
        count: None,
    }
}

struct PendingGpuReadback {
    buffer: wgpu::Buffer,
    byte_len: u64,
    output_row_bytes: usize,
    padded_row_bytes: usize,
    height: u32,
}

struct GpuPassTimingRecorder {
    query_set: wgpu::QuerySet,
    resolve_buffer: wgpu::Buffer,
    readback_buffer: wgpu::Buffer,
    labels: Vec<(String, u32, u32)>,
    next_query: u32,
    query_capacity: u32,
    timestamp_period_ns: f32,
}

impl GpuPassTimingRecorder {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        capabilities: &hot_trimmer_preview::GpuCapabilityRecord,
        pass_capacity: u32,
    ) -> Option<Self> {
        if !capabilities.timestamp_queries || pass_capacity == 0 {
            return None;
        }
        let query_capacity = pass_capacity.saturating_mul(2);
        let byte_len = u64::from(query_capacity).saturating_mul(u64::from(wgpu::QUERY_SIZE));
        let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("hot-trimmer-atlas-pass-timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: query_capacity,
        });
        let resolve_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hot-trimmer-atlas-pass-timestamp-resolve"),
            size: byte_len,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hot-trimmer-atlas-pass-timestamp-readback"),
            size: byte_len,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Some(Self {
            query_set,
            resolve_buffer,
            readback_buffer,
            labels: Vec::new(),
            next_query: 0,
            query_capacity,
            timestamp_period_ns: queue.get_timestamp_period(),
        })
    }

    fn timestamp_writes<'a>(
        &'a mut self,
        label: impl Into<String>,
    ) -> Option<wgpu::ComputePassTimestampWrites<'a>> {
        let start = self.next_query;
        let end = start.checked_add(1)?;
        if end >= self.query_capacity {
            return None;
        }
        self.next_query = end + 1;
        self.labels.push((label.into(), start, end));
        Some(wgpu::ComputePassTimestampWrites {
            query_set: &self.query_set,
            beginning_of_pass_write_index: Some(start),
            end_of_pass_write_index: Some(end),
        })
    }

    fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        if self.next_query == 0 {
            return;
        }
        let byte_len = u64::from(self.next_query).saturating_mul(u64::from(wgpu::QUERY_SIZE));
        encoder.resolve_query_set(&self.query_set, 0..self.next_query, &self.resolve_buffer, 0);
        encoder.copy_buffer_to_buffer(&self.resolve_buffer, 0, &self.readback_buffer, 0, byte_len);
    }

    fn finish(self, device: &wgpu::Device) -> Result<Vec<String>, AtlasRenderExecutionError> {
        if self.next_query == 0 {
            return Ok(Vec::new());
        }
        let byte_len = u64::from(self.next_query).saturating_mul(u64::from(wgpu::QUERY_SIZE));
        let slice = self.readback_buffer.slice(0..byte_len);
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
                AtlasRenderExecutionError::Gpu(format!("timestamp callback failed: {error}"))
            })?
            .map_err(|error| {
                AtlasRenderExecutionError::Gpu(format!("timestamp map failed: {error:?}"))
            })?;
        let mapped = slice.get_mapped_range();
        let timestamps = mapped
            .chunks_exact(wgpu::QUERY_SIZE as usize)
            .map(|chunk| {
                let mut bytes = [0_u8; 8];
                bytes.copy_from_slice(&chunk[..8]);
                u64::from_le_bytes(bytes)
            })
            .collect::<Vec<_>>();
        let telemetry = self
            .labels
            .into_iter()
            .filter_map(|(label, start, end)| {
                let start = *timestamps.get(start as usize)?;
                let end = *timestamps.get(end as usize)?;
                let delta_ns = end.saturating_sub(start) as f64 * f64::from(self.timestamp_period_ns);
                Some(format!(
                    "executor=gpu; gpu_pass_timing={label}; gpu_ms={:.3}; timestamp_bytes={}",
                    delta_ns / 1_000_000.0,
                    u64::from(wgpu::QUERY_SIZE) * 2,
                ))
            })
            .collect();
        drop(mapped);
        self.readback_buffer.unmap();
        Ok(telemetry)
    }
}

#[derive(Default)]
struct GpuMaterialDispatchStats {
    source_cache_hits: u32,
    upload_bytes: u64,
    upload_ms: u128,
    command_count: u32,
    command_bytes: u64,
    dispatch_ms: u128,
}

fn execute_r32float_material_tile(
    executor: &GpuAtlasRenderExecutor<'_>,
    plan: &CompiledAtlasPlanV1,
    input: &AtlasRenderExecutionInput<'_>,
    requested_map: MaterialMapKind,
    cancellation: &CancellationToken,
    is_current: &dyn Fn() -> bool,
) -> Result<AtlasRenderExecutorOutput, AtlasRenderExecutionError> {
    let identity = plan.tile_identity(requested_map, GPU_SHADER_VERSION);
    let display_identity = display_tile_identity(plan, requested_map);
    if let Some(cached) = executor
        .source_texture_cache
        .lock()
        .ok()
        .and_then(|mut cache| cache.cached_tile(&identity, plan.tile_request.generation))
    {
        let interactive_tile = executor
            .source_texture_cache
            .lock()
            .ok()
            .and_then(|mut cache| {
                cache.cached_tile(&display_identity, plan.tile_request.generation)
            })
            .unwrap_or_else(|| Arc::clone(&cached));
        let (nontransparent, nonzero_rgb) =
            payload_counts(interactive_tile.pixels(), interactive_tile.manifest.pixel_format);
        let mut map_tiles = BTreeMap::new();
        map_tiles.insert(requested_map, Arc::clone(&cached));
        let mut display_tiles = BTreeMap::new();
        display_tiles.insert(requested_map, Arc::clone(&interactive_tile));
        return Ok(AtlasRenderExecutorOutput::FinalAtlas(
            AtlasFinalAtlasOutput {
                map_tiles,
                display_tiles,
                intermediate_tiles: BTreeMap::new(),
                base_color_rgba8: interactive_tile.payload(),
                interactive_tile,
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
                telemetry: vec![format!(
                    "executor=gpu; plan_hash={}; requested_map={requested_map:?}; logical_passes={}; executed_gpu_passes=none; final_tile_cache=hit; intermediate_cache=not-available; gpu_tile_cache=hit; dispatch_ms=0; readback_ms=0; tile_nontransparent={nontransparent}; tile_nonzero_rgb={nonzero_rgb}",
                    plan.final_plan_hash.0,
                    logical_passes_for_map(requested_map)
                )],
            },
        ));
    }
    let started = Instant::now();
    let state = executor
        .service
        .initialize()
        .map_err(|error| AtlasRenderExecutionError::Gpu(error.to_string()))?;
    validate_tile_size(plan, state.capabilities())?;
    require_format(state.capabilities(), "R32Float", true, true)?;
    require_format(state.capabilities(), "Rgba8Unorm", false, true)?;
    let device = state.device();
    let queue = state.queue();
    let pipeline_started = Instant::now();
    let (material_pipeline, pipeline_cache_hit) = pipeline(
        device,
        executor.source_texture_cache,
        GpuAtlasPipelineKind::MaterialR32Float,
    )?;
    let (display_pipeline, display_pipeline_cache_hit) = pipeline(
        device,
        executor.source_texture_cache,
        GpuAtlasPipelineKind::MaterialRgba8,
    )?;
    let pipeline_ms = pipeline_started.elapsed().as_millis();
    let tile = plan.tile_request.output_rect.0;
    let (output_texture, output_view) = create_working_texture(
        device,
        "hot-trimmer-r32float-material-output",
        tile.width,
        tile.height,
        wgpu::TextureFormat::R32Float,
        wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
    );
    let (display_texture, display_view) = create_working_texture(
        device,
        "hot-trimmer-rgba8-material-display-output",
        tile.width,
        tile.height,
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
    );
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("hot-trimmer-r32float-material-encoder"),
    });
    let mut timing =
        GpuPassTimingRecorder::new(device, queue, state.capabilities(), 8);
    if requested_map == MaterialMapKind::Height {
        let fill_pipeline = pipeline(
            device,
            executor.source_texture_cache,
            GpuAtlasPipelineKind::FillR32Float,
        )?
        .0;
        dispatch_fill_r32float_with_pipeline(
            device,
            &mut encoder,
            timing.as_mut(),
            &fill_pipeline,
            &output_view,
            tile.width,
            tile.height,
        )?;
    } else if state.capabilities().clear_texture {
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
    } else {
        return Err(AtlasRenderExecutionError::Gpu(
            "CLEAR_TEXTURE support is required to initialize scalar atlas output".into(),
        ));
    }
    if !state.capabilities().clear_texture {
        return Err(AtlasRenderExecutionError::Gpu(
            "CLEAR_TEXTURE support is required to initialize scalar display atlas output".into(),
        ));
    }
    encoder.clear_texture(
        &display_texture,
        &wgpu::ImageSubresourceRange {
            aspect: wgpu::TextureAspect::All,
            base_mip_level: 0,
            mip_level_count: Some(1),
            base_array_layer: 0,
            array_layer_count: Some(1),
        },
    );
    let stats = dispatch_material_map_to_view(
        device,
        queue,
        executor.source_texture_cache,
        &mut encoder,
        timing.as_mut(),
        "material-r32float-publish",
        &material_pipeline,
        plan,
        input,
        requested_map,
        &output_view,
        cancellation,
        is_current,
    )?;
    let display_stats = dispatch_material_map_to_view(
        device,
        queue,
        executor.source_texture_cache,
        &mut encoder,
        timing.as_mut(),
        "material-rgba8-display-publish",
        &display_pipeline,
        plan,
        input,
        requested_map,
        &display_view,
        cancellation,
        is_current,
    )?;
    let pending = schedule_readback(
        device,
        executor.source_texture_cache,
        &mut encoder,
        &output_texture,
        tile.width,
        tile.height,
        4,
    )?;
    let display_pending = schedule_readback(
        device,
        executor.source_texture_cache,
        &mut encoder,
        &display_texture,
        tile.width,
        tile.height,
        4,
    )?;
    if let Some(timing) = &timing {
        timing.resolve(&mut encoder);
    }
    queue.submit(Some(encoder.finish()));
    if requested_map == MaterialMapKind::Height {
        executor
            .source_texture_cache
            .lock()
            .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?
            .remember_rendered_texture(
                identity.clone(),
                output_texture,
                output_view,
                tile.width,
                tile.height,
                wgpu::TextureFormat::R32Float,
            );
    }
    let (pixels, readback_ms) = finish_readback(device, executor.source_texture_cache, pending)?;
    let (display_pixels, display_readback_ms) =
        finish_readback(device, executor.source_texture_cache, display_pending)?;
    let readback_bytes = u64::try_from(pixels.len())
        .map_err(|_| AtlasRenderExecutionError::Gpu("readback payload overflow".into()))?;
    let display_readback_bytes = u64::try_from(display_pixels.len())
        .map_err(|_| AtlasRenderExecutionError::Gpu("display readback payload overflow".into()))?;
    let rendered_tile = remember_rendered_tile(
        executor.source_texture_cache,
        plan,
        requested_map,
        Arc::clone(&pixels),
    )?;
    let display_tile = remember_rendered_tile_with_identity(
        executor.source_texture_cache,
        plan,
        requested_map,
        display_identity,
        Arc::clone(&display_pixels),
    )?;
    let (nontransparent, nonzero_rgb) =
        payload_counts(display_tile.pixels(), display_tile.manifest.pixel_format);
    let mut telemetry = vec![format!(
        "executor=gpu; backend={}; plan_hash={}; requested_map={requested_map:?}; logical_passes={}; executed_gpu_passes=material-r32float-publish,material-rgba8-display-publish; pixel_format=R32Float; display_pixel_format=Rgba8UnormLinear; final_tile_cache=miss; intermediate_cache=not-available; source_cache_hits={}; pipeline_cache_hits={}; upload_bytes={}; upload_ms={}; command_count={}; command_bytes={}; pipeline_ms={pipeline_ms}; dispatch_ms={}; readback_bytes={}; readback_ms={}; tile_nontransparent={nontransparent}; tile_nonzero_rgb={nonzero_rgb}; composition_ms=0; render_ms={}",
        state.capabilities().backend,
        plan.final_plan_hash.0,
        logical_passes_for_map(requested_map),
        stats.source_cache_hits.saturating_add(display_stats.source_cache_hits),
        u32::from(pipeline_cache_hit) + u32::from(display_pipeline_cache_hit),
        stats.upload_bytes.saturating_add(display_stats.upload_bytes),
        stats.upload_ms.saturating_add(display_stats.upload_ms),
        stats.command_count.saturating_add(display_stats.command_count),
        stats.command_bytes.saturating_add(display_stats.command_bytes),
        stats.dispatch_ms.saturating_add(display_stats.dispatch_ms),
        readback_bytes.saturating_add(display_readback_bytes),
        readback_ms.saturating_add(display_readback_ms),
        started.elapsed().as_millis()
    )];
    if let Some(timing) = timing {
        telemetry.extend(timing.finish(device)?);
    }
    let mut map_tiles = BTreeMap::new();
    map_tiles.insert(requested_map, Arc::clone(&rendered_tile));
    let mut display_tiles = BTreeMap::new();
    display_tiles.insert(requested_map, Arc::clone(&display_tile));
    Ok(AtlasRenderExecutorOutput::FinalAtlas(
        AtlasFinalAtlasOutput {
            map_tiles,
            display_tiles,
            intermediate_tiles: BTreeMap::new(),
            base_color_rgba8: Arc::clone(&display_pixels),
            interactive_tile: display_tile,
            region_valid_pixel_counts: final_atlas_metadata(plan)?,
            render_ms: started.elapsed().as_millis(),
            source_cache_hits: stats
                .source_cache_hits
                .saturating_add(display_stats.source_cache_hits),
            pipeline_cache_hits: u32::from(pipeline_cache_hit)
                + u32::from(display_pipeline_cache_hit),
            upload_bytes: stats.upload_bytes.saturating_add(display_stats.upload_bytes),
            upload_ms: stats.upload_ms.saturating_add(display_stats.upload_ms),
            command_count: stats.command_count.saturating_add(display_stats.command_count),
            command_bytes: stats.command_bytes.saturating_add(display_stats.command_bytes),
            dispatch_ms: stats.dispatch_ms.saturating_add(display_stats.dispatch_ms),
            readback_bytes: readback_bytes.saturating_add(display_readback_bytes),
            readback_ms: readback_ms.saturating_add(display_readback_ms),
            telemetry,
        },
    ))
}

fn execute_region_id_gpu_tile(
    executor: &GpuAtlasRenderExecutor<'_>,
    plan: &CompiledAtlasPlanV1,
    cancellation: &CancellationToken,
    is_current: &dyn Fn() -> bool,
) -> Result<AtlasRenderExecutorOutput, AtlasRenderExecutionError> {
    let requested_map = MaterialMapKind::RegionId;
    let identity = plan.tile_identity(requested_map, GPU_SHADER_VERSION);
    let display_identity = display_tile_identity(plan, requested_map);
    if let Some(cached) = executor
        .source_texture_cache
        .lock()
        .ok()
        .and_then(|mut cache| cache.cached_tile(&identity, plan.tile_request.generation))
    {
        let interactive_tile = executor
            .source_texture_cache
            .lock()
            .ok()
            .and_then(|mut cache| {
                cache.cached_tile(&display_identity, plan.tile_request.generation)
            })
            .unwrap_or_else(|| Arc::clone(&cached));
        let (nontransparent, nonzero_rgb) =
            payload_counts(interactive_tile.pixels(), interactive_tile.manifest.pixel_format);
        let mut map_tiles = BTreeMap::new();
        map_tiles.insert(requested_map, Arc::clone(&cached));
        let mut display_tiles = BTreeMap::new();
        display_tiles.insert(requested_map, Arc::clone(&interactive_tile));
        return Ok(AtlasRenderExecutorOutput::FinalAtlas(
            AtlasFinalAtlasOutput {
                map_tiles,
                display_tiles,
                intermediate_tiles: BTreeMap::new(),
                base_color_rgba8: interactive_tile.payload(),
                interactive_tile,
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
                telemetry: vec![format!(
                    "executor=gpu; plan_hash={}; requested_map=RegionId; logical_passes={}; executed_gpu_passes=none; final_tile_cache=hit; gpu_tile_cache=hit; dispatch_ms=0; readback_ms=0; tile_nontransparent={nontransparent}; tile_nonzero_rgb={nonzero_rgb}",
                    plan.final_plan_hash.0,
                    logical_passes_for_map(requested_map)
                )],
            },
        ));
    }
    if cancellation.is_cancelled() {
        return Err(AtlasRenderExecutionError::Cancelled);
    }
    if !is_current() {
        return Err(AtlasRenderExecutionError::Superseded);
    }
    let started = Instant::now();
    let state = executor
        .service
        .initialize()
        .map_err(|error| AtlasRenderExecutionError::Gpu(error.to_string()))?;
    validate_tile_size(plan, state.capabilities())?;
    require_format(state.capabilities(), "R32Uint", false, true)?;
    require_format(state.capabilities(), "R32Uint", true, false)?;
    require_format(state.capabilities(), "Rgba8Unorm", false, true)?;
    let device = state.device();
    let queue = state.queue();
    let pipeline_started = Instant::now();
    let (region_pipeline, pipeline_cache_hit) = pipeline(
        device,
        executor.source_texture_cache,
        GpuAtlasPipelineKind::RegionIdR32Uint,
    )?;
    let (display_pipeline, display_pipeline_cache_hit) = pipeline(
        device,
        executor.source_texture_cache,
        GpuAtlasPipelineKind::RegionIdDisplayRgba8,
    )?;
    let pipeline_ms = pipeline_started.elapsed().as_millis();
    let tile = plan.tile_request.output_rect.0;
    let (output_texture, output_view) = create_working_texture(
        device,
        "hot-trimmer-region-id-output",
        tile.width,
        tile.height,
        wgpu::TextureFormat::R32Uint,
        wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
    );
    let (display_texture, display_view) = create_working_texture(
        device,
        "hot-trimmer-region-id-display-output",
        tile.width,
        tile.height,
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
    );
    let header = GpuAtlasHeader {
        output_width: plan.output_size.width,
        output_height: plan.output_size.height,
        tile_x: tile.x,
        tile_y: tile.y,
        tile_width: tile.width,
        tile_height: tile.height,
        command_count: plan.ordered_regions.len() as u32,
        source_width: 0,
        source_height: 0,
        map_kind: gpu_map_code(requested_map),
        normal_convention: 0,
        source_role: 0,
    };
    let header_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("hot-trimmer-region-id-header"),
        contents: &encode_header(header),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let commands = plan
        .ordered_regions
        .iter()
        .map(pack_command)
        .collect::<Result<Vec<_>, _>>()?;
    let command_buffer_bytes = nonempty_command_bytes(&commands);
    let command_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("hot-trimmer-region-id-commands"),
        contents: &command_buffer_bytes,
        usage: wgpu::BufferUsages::STORAGE,
    });
    // The R32Uint tile stores the compiled compact index, which is stable but is
    // not required to match command-vector order. Build the shader lookup by
    // that index so future sparse/reordered plans cannot misclassify pixels.
    let classification_count = plan
        .ordered_regions
        .iter()
        .map(|region| region.compact_index as usize)
        .max()
        .map_or(1, |maximum| maximum.saturating_add(1));
    let mut display_colors = vec![0_u32; classification_count];
    for region in &plan.ordered_regions {
        let [red, green, blue, alpha] = region
            .region_classification()
            .display_rgba8(region.compact_index);
        display_colors[region.compact_index as usize] = u32::from(red)
            | (u32::from(green) << 8)
            | (u32::from(blue) << 16)
            | (u32::from(alpha) << 24);
    }
    let classification_bytes = display_colors
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect::<Vec<_>>();
    let classification_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("hot-trimmer-region-classification-lookup"),
        contents: &classification_bytes,
        usage: wgpu::BufferUsages::STORAGE,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("hot-trimmer-region-id-bind-group"),
        layout: &region_pipeline.bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: header_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: command_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&output_view),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("hot-trimmer-region-id-encoder"),
    });
    let mut timing =
        GpuPassTimingRecorder::new(device, queue, state.capabilities(), 4);
    let dispatch_started = Instant::now();
    {
        let timestamp_writes = timing
            .as_mut()
            .and_then(|recorder| recorder.timestamp_writes("compact-region-id-r32uint"));
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("hot-trimmer-region-id-dispatch"),
            timestamp_writes,
        });
        pass.set_pipeline(&region_pipeline.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(tile.width.div_ceil(16), tile.height.div_ceil(16), 1);
    }
    let display_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("hot-trimmer-region-id-display-bind-group"),
        layout: &display_pipeline.bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&output_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: classification_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&display_view),
            },
        ],
    });
    {
        let timestamp_writes = timing
            .as_mut()
            .and_then(|recorder| recorder.timestamp_writes("compact-region-id-rgba8-display"));
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("hot-trimmer-region-id-display-dispatch"),
            timestamp_writes,
        });
        pass.set_pipeline(&display_pipeline.pipeline);
        pass.set_bind_group(0, &display_bind_group, &[]);
        pass.dispatch_workgroups(tile.width.div_ceil(16), tile.height.div_ceil(16), 1);
    }
    let dispatch_ms = dispatch_started.elapsed().as_millis();
    let pending = schedule_readback(
        device,
        executor.source_texture_cache,
        &mut encoder,
        &output_texture,
        tile.width,
        tile.height,
        4,
    )?;
    let display_pending = schedule_readback(
        device,
        executor.source_texture_cache,
        &mut encoder,
        &display_texture,
        tile.width,
        tile.height,
        4,
    )?;
    if let Some(timing) = &timing {
        timing.resolve(&mut encoder);
    }
    queue.submit(Some(encoder.finish()));
    let (pixels, readback_ms) = finish_readback(device, executor.source_texture_cache, pending)?;
    let (display_pixels, display_readback_ms) =
        finish_readback(device, executor.source_texture_cache, display_pending)?;
    let readback_bytes = u64::try_from(pixels.len())
        .map_err(|_| AtlasRenderExecutionError::Gpu("readback payload overflow".into()))?;
    let display_readback_bytes = u64::try_from(display_pixels.len())
        .map_err(|_| AtlasRenderExecutionError::Gpu("display readback payload overflow".into()))?;
    let rendered_tile = remember_rendered_tile(
        executor.source_texture_cache,
        plan,
        requested_map,
        Arc::clone(&pixels),
    )?;
    let display_tile = remember_rendered_tile_with_identity(
        executor.source_texture_cache,
        plan,
        requested_map,
        display_identity,
        Arc::clone(&display_pixels),
    )?;
    let (nontransparent, nonzero_rgb) =
        payload_counts(display_tile.pixels(), display_tile.manifest.pixel_format);
    let mut telemetry = vec![format!(
        "executor=gpu; backend={}; plan_hash={}; requested_map=RegionId; logical_passes={}; executed_gpu_passes=compact-region-id-r32uint,compact-region-id-rgba8-display; pixel_format=R32Uint; display_pixel_format=Rgba8UnormLinear; final_tile_cache=miss; pipeline_cache_hits={}; pipeline_ms={pipeline_ms}; command_count={}; command_bytes={}; dispatch_ms={dispatch_ms}; readback_bytes={}; readback_ms={}; tile_nontransparent={nontransparent}; tile_nonzero_rgb={nonzero_rgb}; render_ms={}",
        state.capabilities().backend,
        plan.final_plan_hash.0,
        logical_passes_for_map(requested_map),
        u32::from(pipeline_cache_hit) + u32::from(display_pipeline_cache_hit),
        plan.ordered_regions.len(),
        command_buffer_bytes.len(),
        readback_bytes.saturating_add(display_readback_bytes),
        readback_ms.saturating_add(display_readback_ms),
        started.elapsed().as_millis()
    )];
    if let Some(timing) = timing {
        telemetry.extend(timing.finish(device)?);
    }
    let mut map_tiles = BTreeMap::new();
    map_tiles.insert(requested_map, Arc::clone(&rendered_tile));
    let mut display_tiles = BTreeMap::new();
    display_tiles.insert(requested_map, Arc::clone(&display_tile));
    Ok(AtlasRenderExecutorOutput::FinalAtlas(
        AtlasFinalAtlasOutput {
            map_tiles,
            display_tiles,
            intermediate_tiles: BTreeMap::new(),
            base_color_rgba8: Arc::clone(&display_pixels),
            interactive_tile: display_tile,
            region_valid_pixel_counts: final_atlas_metadata(plan)?,
            render_ms: started.elapsed().as_millis(),
            source_cache_hits: 0,
            pipeline_cache_hits: u32::from(pipeline_cache_hit)
                + u32::from(display_pipeline_cache_hit),
            upload_bytes: 0,
            upload_ms: 0,
            command_count: plan.ordered_regions.len() as u32,
            command_bytes: command_buffer_bytes.len() as u64,
            dispatch_ms,
            readback_bytes: readback_bytes.saturating_add(display_readback_bytes),
            readback_ms: readback_ms.saturating_add(display_readback_ms),
            telemetry,
        },
    ))
}

fn execute_height_normal_gpu(
    executor: &GpuAtlasRenderExecutor<'_>,
    plan: &CompiledAtlasPlanV1,
    input: &AtlasRenderExecutionInput<'_>,
    publish_height: bool,
    cancellation: &CancellationToken,
    is_current: &dyn Fn() -> bool,
) -> Result<AtlasFinalAtlasOutput, AtlasRenderExecutionError> {
    if cancellation.is_cancelled() {
        return Err(AtlasRenderExecutionError::Cancelled);
    }
    if !is_current() {
        return Err(AtlasRenderExecutionError::Superseded);
    }
    let started = Instant::now();
    let state = executor
        .service
        .initialize()
        .map_err(|error| AtlasRenderExecutionError::Gpu(error.to_string()))?;
    validate_tile_size(plan, state.capabilities())?;
    require_format(state.capabilities(), "R32Float", true, true)?;
    require_format(state.capabilities(), "Rgba8Unorm", false, true)?;
    let device = state.device();
    let queue = state.queue();
    let pipeline_started = Instant::now();
    let (fill_pipeline, fill_cache_hit) = pipeline(
        device,
        executor.source_texture_cache,
        GpuAtlasPipelineKind::FillR32Float,
    )?;
    let (height_pipeline, height_pipeline_cache_hit) = pipeline(
        device,
        executor.source_texture_cache,
        GpuAtlasPipelineKind::MaterialR32Float,
    )?;
    let (normal_pipeline, normal_pipeline_cache_hit) = pipeline(
        device,
        executor.source_texture_cache,
        GpuAtlasPipelineKind::NormalFromHeight,
    )?;
    let has_authored_normal = plan
        .ordered_sources
        .iter()
        .any(|source| source.channel_role == MaterialChannelRole::Normal);
    let authored_normal_pipeline = if has_authored_normal {
        Some(pipeline(
            device,
            executor.source_texture_cache,
            GpuAtlasPipelineKind::MaterialRgba8,
        )?)
    } else {
        None
    };
    let height_display_pipeline = if publish_height {
        Some(pipeline(
            device,
            executor.source_texture_cache,
            GpuAtlasPipelineKind::MaterialRgba8,
        )?)
    } else {
        None
    };
    let pipeline_ms = pipeline_started.elapsed().as_millis();
    let tile = plan.tile_request.output_rect.0;
    let (height_texture, height_view) = create_working_texture(
        device,
        "hot-trimmer-final-height-r32float",
        tile.width,
        tile.height,
        wgpu::TextureFormat::R32Float,
        wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
    );
    let (normal_texture, normal_view) = create_working_texture(
        device,
        "hot-trimmer-normal-from-height-output",
        tile.width,
        tile.height,
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
    );
    let (authored_normal_texture, authored_normal_view) = create_working_texture(
        device,
        "hot-trimmer-authored-normal-output",
        tile.width,
        tile.height,
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let height_display = publish_height.then(|| {
        create_working_texture(
            device,
            "hot-trimmer-height-rgba8-display-output",
            tile.width,
            tile.height,
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        )
    });
    let cached_height = executor
        .source_texture_cache
        .lock()
        .ok()
        .and_then(|mut cache| {
            let identity = plan.tile_identity(MaterialMapKind::Height, GPU_SHADER_VERSION);
            cache.cached_tile(&identity, plan.tile_request.generation)
        })
        .filter(|tile| {
            tile.manifest.pixel_format == crate::CompiledTilePixelFormat::R32Float
                && tile.manifest.width == tile.manifest.output_rect.0.width
                && tile.manifest.height == tile.manifest.output_rect.0.height
        });
    let cached_height_texture = executor
        .source_texture_cache
        .lock()
        .ok()
        .and_then(|mut cache| {
            let identity = plan.tile_identity(MaterialMapKind::Height, GPU_SHADER_VERSION);
            cache.cached_rendered_texture(&identity)
        })
        .filter(|texture| {
            texture.format == wgpu::TextureFormat::R32Float
                && texture.width == tile.width
                && texture.height == tile.height
        });
    let cached_height_display = publish_height
        .then(|| {
            executor
                .source_texture_cache
                .lock()
                .ok()
                .and_then(|mut cache| {
                    let identity = display_tile_identity(plan, MaterialMapKind::Height);
                    cache.cached_tile(&identity, plan.tile_request.generation)
                })
                .filter(|tile| {
                    tile.manifest.pixel_format == crate::CompiledTilePixelFormat::Rgba8UnormLinear
                })
        })
        .flatten();
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("hot-trimmer-height-normal-encoder"),
    });
    let mut timing =
        GpuPassTimingRecorder::new(device, queue, state.capabilities(), 8);
    if cached_height_texture.is_none() {
        dispatch_fill_r32float_with_pipeline(
            device,
            &mut encoder,
            timing.as_mut(),
            &fill_pipeline,
            &height_view,
            tile.width,
            tile.height,
        )?;
    }
    if !state.capabilities().clear_texture {
        return Err(AtlasRenderExecutionError::Gpu(
            "CLEAR_TEXTURE support is required to initialize Normal output".into(),
        ));
    }
    encoder.clear_texture(
        &normal_texture,
        &wgpu::ImageSubresourceRange {
            aspect: wgpu::TextureAspect::All,
            base_mip_level: 0,
            mip_level_count: Some(1),
            base_array_layer: 0,
            array_layer_count: Some(1),
        },
    );
    encoder.clear_texture(
        &authored_normal_texture,
        &wgpu::ImageSubresourceRange {
            aspect: wgpu::TextureAspect::All,
            base_mip_level: 0,
            mip_level_count: Some(1),
            base_array_layer: 0,
            array_layer_count: Some(1),
        },
    );
    if let Some((height_display_texture, _)) = &height_display {
        encoder.clear_texture(
            height_display_texture,
            &wgpu::ImageSubresourceRange {
                aspect: wgpu::TextureAspect::All,
                base_mip_level: 0,
                mip_level_count: Some(1),
                base_array_layer: 0,
                array_layer_count: Some(1),
            },
        );
    }
    let height_stats = if cached_height_texture.is_some() {
        GpuMaterialDispatchStats::default()
    } else {
        dispatch_material_map_to_view(
            device,
            queue,
            executor.source_texture_cache,
            &mut encoder,
            timing.as_mut(),
            "height-r32float",
            &height_pipeline,
            plan,
            input,
            MaterialMapKind::Height,
            &height_view,
            cancellation,
            is_current,
        )?
    };
    let height_display_stats = if cached_height_display.is_none()
        && let (Some((height_display_pipeline, _)), Some((_, height_display_view))) =
            (&height_display_pipeline, &height_display)
    {
        Some(dispatch_material_map_to_view(
            device,
            queue,
            executor.source_texture_cache,
            &mut encoder,
            timing.as_mut(),
            "height-rgba8-display",
            height_display_pipeline,
            plan,
            input,
            MaterialMapKind::Height,
            height_display_view,
            cancellation,
            is_current,
        )?)
    } else {
        None
    };
    let authored_normal_stats = if let Some((authored_normal_pipeline, _)) =
        &authored_normal_pipeline
    {
        Some(dispatch_material_map_to_view(
            device,
            queue,
            executor.source_texture_cache,
            &mut encoder,
            timing.as_mut(),
            "authored-normal-sample",
            authored_normal_pipeline,
            plan,
            input,
            MaterialMapKind::Normal,
            &authored_normal_view,
            cancellation,
            is_current,
        )?)
    } else {
        None
    };
    let commands = plan
        .ordered_regions
        .iter()
        .map(pack_command)
        .collect::<Result<Vec<_>, _>>()?;
    let header = GpuAtlasHeader {
        output_width: plan.output_size.width,
        output_height: plan.output_size.height,
        tile_x: tile.x,
        tile_y: tile.y,
        tile_width: tile.width,
        tile_height: tile.height,
        command_count: commands.len() as u32,
        source_width: 0,
        source_height: 0,
        map_kind: gpu_map_code(MaterialMapKind::Normal),
        normal_convention: match plan.normal_convention {
            crate::CompiledNormalConvention::OpenGl => 0,
            crate::CompiledNormalConvention::DirectX => 1,
        },
        source_role: gpu_channel_role_code(if has_authored_normal {
            MaterialChannelRole::Normal
        } else {
            MaterialChannelRole::Height
        }),
    };
    let header_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("hot-trimmer-normal-from-height-header"),
        contents: &encode_header(header),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let command_buffer_bytes = nonempty_command_bytes(&commands);
    let command_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("hot-trimmer-normal-from-height-commands"),
        contents: &command_buffer_bytes,
        usage: wgpu::BufferUsages::STORAGE,
    });
    let normal_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("hot-trimmer-normal-from-height-bind-group"),
        layout: &normal_pipeline.bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: header_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: command_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(
                    cached_height_texture
                        .as_ref()
                        .map_or(&height_view, |cached| &cached.view),
                ),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&authored_normal_view),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&normal_view),
            },
        ],
    });
    let normal_dispatch_started = Instant::now();
    {
        let timestamp_writes = timing
            .as_mut()
            .and_then(|recorder| recorder.timestamp_writes("normal-from-final-height"));
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("hot-trimmer-normal-from-height-dispatch"),
            timestamp_writes,
        });
        pass.set_pipeline(&normal_pipeline.pipeline);
        pass.set_bind_group(0, &normal_bind_group, &[]);
        pass.dispatch_workgroups(tile.width.div_ceil(16), tile.height.div_ceil(16), 1);
    }
    let normal_dispatch_ms = normal_dispatch_started.elapsed().as_millis();
    let height_pending = if publish_height && cached_height.is_none() {
        if let Some(cached_height_texture) = &cached_height_texture {
            Some(schedule_readback(
                device,
                executor.source_texture_cache,
                &mut encoder,
                &cached_height_texture._texture,
                tile.width,
                tile.height,
                4,
            )?)
        } else {
            Some(schedule_readback(
                device,
                executor.source_texture_cache,
                &mut encoder,
                &height_texture,
                tile.width,
                tile.height,
                4,
            )?)
        }
    } else {
        None
    };
    let height_display_pending =
        if cached_height_display.is_none() && let Some((height_display_texture, _)) = &height_display {
        Some(schedule_readback(
            device,
            executor.source_texture_cache,
            &mut encoder,
            height_display_texture,
            tile.width,
            tile.height,
            4,
        )?)
    } else {
        None
    };
    let normal_pending = schedule_readback(
        device,
        executor.source_texture_cache,
        &mut encoder,
        &normal_texture,
        tile.width,
        tile.height,
        4,
    )?;
    if let Some(timing) = &timing {
        timing.resolve(&mut encoder);
    }
    queue.submit(Some(encoder.finish()));
    if cached_height_texture.is_none() {
        executor
            .source_texture_cache
            .lock()
            .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?
            .remember_rendered_texture(
                plan.tile_identity(MaterialMapKind::Height, GPU_SHADER_VERSION),
                height_texture,
                height_view,
                tile.width,
                tile.height,
                wgpu::TextureFormat::R32Float,
            );
    }
    let mut readback_bytes = 0_u64;
    let mut readback_ms = 0_u128;
    let mut map_tiles = BTreeMap::new();
    let mut display_tiles = BTreeMap::new();
    let mut intermediate_tiles = BTreeMap::new();
    if let Some(cached_height_display) = cached_height_display {
        display_tiles.insert(MaterialMapKind::Height, cached_height_display);
    } else if let Some(height_display_pending) = height_display_pending {
        let (height_display_pixels, height_display_readback_ms) =
            finish_readback(device, executor.source_texture_cache, height_display_pending)?;
        readback_bytes = readback_bytes.saturating_add(height_display_pixels.len() as u64);
        readback_ms = readback_ms.saturating_add(height_display_readback_ms);
        let height_display_tile = remember_rendered_tile_with_identity(
            executor.source_texture_cache,
            plan,
            MaterialMapKind::Height,
            display_tile_identity(plan, MaterialMapKind::Height),
            Arc::clone(&height_display_pixels),
        )?;
        display_tiles.insert(MaterialMapKind::Height, height_display_tile);
    }
    if let Some(cached_height) = cached_height {
        intermediate_tiles.insert("final-height".into(), Arc::clone(&cached_height));
        intermediate_tiles.insert("normal.final-height".into(), Arc::clone(&cached_height));
        map_tiles.insert(MaterialMapKind::Height, cached_height);
    } else if let Some(height_pending) = height_pending {
        let (height_pixels, height_readback_ms) =
            finish_readback(device, executor.source_texture_cache, height_pending)?;
        readback_bytes = readback_bytes.saturating_add(height_pixels.len() as u64);
        readback_ms = readback_ms.saturating_add(height_readback_ms);
        let height_tile = remember_rendered_tile(
            executor.source_texture_cache,
            plan,
            MaterialMapKind::Height,
            Arc::clone(&height_pixels),
        )?;
        intermediate_tiles.insert("final-height".into(), Arc::clone(&height_tile));
        intermediate_tiles.insert("normal.final-height".into(), Arc::clone(&height_tile));
        map_tiles.insert(MaterialMapKind::Height, height_tile);
    }
    let (normal_pixels, normal_readback_ms) =
        finish_readback(device, executor.source_texture_cache, normal_pending)?;
    readback_bytes = readback_bytes.saturating_add(normal_pixels.len() as u64);
    readback_ms = readback_ms.saturating_add(normal_readback_ms);
    let normal_tile = remember_rendered_tile(
        executor.source_texture_cache,
        plan,
        MaterialMapKind::Normal,
        Arc::clone(&normal_pixels),
    )?;
    map_tiles.insert(MaterialMapKind::Normal, Arc::clone(&normal_tile));
    display_tiles.insert(MaterialMapKind::Normal, Arc::clone(&normal_tile));
    let mut telemetry = vec![format!(
        "executor=gpu; backend={}; plan_hash={}; requested_map=Normal; logical_passes={}; executed_gpu_passes={},{}normal-from-final-height; dependency={}; intermediate_cache={}; normal_publish={}; source_cache_hits={}; pipeline_cache_hits={}; upload_bytes={}; upload_ms={}; command_count={}; command_bytes={}; pipeline_ms={pipeline_ms}; dispatch_ms={}; readback_bytes={readback_bytes}; readback_ms={readback_ms}; render_ms={}",
        state.capabilities().backend,
        plan.final_plan_hash.0,
        logical_passes_for_map(MaterialMapKind::Normal),
        if cached_height_texture.is_some() {
            "height-r32float-gpu-resource-cache"
        } else {
            "height-r32float"
        },
        if has_authored_normal {
            "authored-normal-sample,"
        } else {
            ""
        },
        if has_authored_normal {
            "Normal<-Height+authored-Normal"
        } else {
            "Normal<-Height"
        },
        if cached_height_texture.is_some() {
            "final-height:persistent-gpu-resource-hit"
        } else {
            "final-height:live-gpu-hit"
        },
        if has_authored_normal {
            "authored-plus-r32float-gpu-final-height"
        } else {
            "from-r32float-gpu-final-height"
        },
        height_stats.source_cache_hits.saturating_add(
            authored_normal_stats
                .as_ref()
                .map_or(0, |stats| stats.source_cache_hits),
        ),
        u32::from(fill_cache_hit)
            + u32::from(height_pipeline_cache_hit)
            + u32::from(normal_pipeline_cache_hit)
            + height_display_pipeline
                .as_ref()
                .map_or(0, |(_, hit)| u32::from(*hit))
            + authored_normal_pipeline
                .as_ref()
                .map_or(0, |(_, hit)| u32::from(*hit)),
        height_stats.upload_bytes.saturating_add(
            height_display_stats
                .as_ref()
                .map_or(0, |stats| stats.upload_bytes),
        ).saturating_add(authored_normal_stats.as_ref().map_or(0, |stats| stats.upload_bytes)),
        height_stats.upload_ms.saturating_add(
            height_display_stats
                .as_ref()
                .map_or(0, |stats| stats.upload_ms),
        ).saturating_add(authored_normal_stats.as_ref().map_or(0, |stats| stats.upload_ms)),
        height_stats
            .command_count
            .saturating_add(commands.len() as u32)
            .saturating_add(
                height_display_stats
                    .as_ref()
                    .map_or(0, |stats| stats.command_count),
            )
            .saturating_add(authored_normal_stats.as_ref().map_or(0, |stats| stats.command_count)),
        height_stats
            .command_bytes
            .saturating_add(command_buffer_bytes.len() as u64)
            .saturating_add(
                height_display_stats
                    .as_ref()
                    .map_or(0, |stats| stats.command_bytes),
            )
            .saturating_add(authored_normal_stats.as_ref().map_or(0, |stats| stats.command_bytes)),
        height_stats
            .dispatch_ms
            .saturating_add(normal_dispatch_ms)
            .saturating_add(
                height_display_stats
                    .as_ref()
                    .map_or(0, |stats| stats.dispatch_ms),
            )
            .saturating_add(authored_normal_stats.as_ref().map_or(0, |stats| stats.dispatch_ms)),
        started.elapsed().as_millis()
    )];
    if publish_height {
        telemetry.push(
            "executor=gpu; dependency=Normal<-Height; final_height_publication=R32Float; intermediate_cache=final-height:live-gpu-readback"
                .into(),
        );
    }
    if let Some(timing) = timing {
        telemetry.extend(timing.finish(device)?);
    }
    let interactive_tile = if publish_height {
        display_tiles
            .get(&MaterialMapKind::Height)
            .or_else(|| map_tiles.get(&MaterialMapKind::Height))
            .cloned()
            .unwrap_or_else(|| Arc::clone(&normal_tile))
    } else {
        Arc::clone(&normal_tile)
    };
    Ok(AtlasFinalAtlasOutput {
        map_tiles,
        display_tiles,
        intermediate_tiles,
        base_color_rgba8: interactive_tile.payload(),
        interactive_tile,
        region_valid_pixel_counts: final_atlas_metadata(plan)?,
        render_ms: started.elapsed().as_millis(),
        source_cache_hits: height_stats.source_cache_hits.saturating_add(
            authored_normal_stats
                .as_ref()
                .map_or(0, |stats| stats.source_cache_hits),
        ),
        pipeline_cache_hits: u32::from(fill_cache_hit)
            + u32::from(height_pipeline_cache_hit)
            + u32::from(normal_pipeline_cache_hit)
            + height_display_pipeline
                .as_ref()
                .map_or(0, |(_, hit)| u32::from(*hit))
            + authored_normal_pipeline
                .as_ref()
                .map_or(0, |(_, hit)| u32::from(*hit)),
        upload_bytes: height_stats.upload_bytes.saturating_add(
            height_display_stats
                .as_ref()
                .map_or(0, |stats| stats.upload_bytes),
        ).saturating_add(authored_normal_stats.as_ref().map_or(0, |stats| stats.upload_bytes)),
        upload_ms: height_stats.upload_ms.saturating_add(
            height_display_stats
                .as_ref()
                .map_or(0, |stats| stats.upload_ms),
        ).saturating_add(authored_normal_stats.as_ref().map_or(0, |stats| stats.upload_ms)),
        command_count: height_stats
            .command_count
            .saturating_add(commands.len() as u32)
            .saturating_add(
                height_display_stats
                    .as_ref()
                    .map_or(0, |stats| stats.command_count),
            )
            .saturating_add(authored_normal_stats.as_ref().map_or(0, |stats| stats.command_count)),
        command_bytes: height_stats
            .command_bytes
            .saturating_add(command_buffer_bytes.len() as u64)
            .saturating_add(
                height_display_stats
                    .as_ref()
                    .map_or(0, |stats| stats.command_bytes),
            )
            .saturating_add(authored_normal_stats.as_ref().map_or(0, |stats| stats.command_bytes)),
        dispatch_ms: height_stats
            .dispatch_ms
            .saturating_add(normal_dispatch_ms)
            .saturating_add(
                height_display_stats
                    .as_ref()
                    .map_or(0, |stats| stats.dispatch_ms),
            )
            .saturating_add(authored_normal_stats.as_ref().map_or(0, |stats| stats.dispatch_ms)),
        readback_bytes,
        readback_ms,
        telemetry,
    })
}

fn dispatch_material_map_to_view(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    cache: &Mutex<GpuAtlasSourceTextureCache>,
    encoder: &mut wgpu::CommandEncoder,
    timing: Option<&mut GpuPassTimingRecorder>,
    pass_label: &'static str,
    pipeline: &GpuAtlasPipeline,
    plan: &CompiledAtlasPlanV1,
    input: &AtlasRenderExecutionInput<'_>,
    requested_map: MaterialMapKind,
    output_view: &wgpu::TextureView,
    cancellation: &CancellationToken,
    is_current: &dyn Fn() -> bool,
) -> Result<GpuMaterialDispatchStats, AtlasRenderExecutionError> {
    let upload_started = Instant::now();
    let mut upload_bytes = 0_u64;
    let mut source_cache_hits = 0_u32;
    let mut source_groups = Vec::with_capacity(plan.ordered_sources.len());
    for source in &plan.ordered_sources {
        if cancellation.is_cancelled() {
            return Err(AtlasRenderExecutionError::Cancelled);
        }
        if !is_current() {
            return Err(AtlasRenderExecutionError::Superseded);
        }
        let source_role = source_channel_role_for_source(plan, source, requested_map);
        if source.channel_role != source_role {
            continue;
        }
        let prepared = input
            .prepared_sources
            .iter()
            .find(|prepared| {
                prepared.source_set_id == source.source_set_id
                    && prepared.source_id == source.source_id
                    && prepared.channel_role == source_role
            })
            .ok_or_else(|| AtlasRenderExecutionError::MissingPreparedSource {
                source_set_id: source.source_set_id,
                source_id: source.source_id.clone(),
            })?;
        let (cached, hit) = source_texture(device, queue, cache, source, prepared.domain.as_ref())?;
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
    let dispatch_started = Instant::now();
    let mut command_count = 0_u32;
    let mut command_bytes = 0_u64;
    let tile = plan.tile_request.output_rect.0;
    let mut timing = timing;
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
            tile_width: tile.width,
            tile_height: tile.height,
            command_count: commands.len() as u32,
            source_width: source.oriented_dimensions.width,
            source_height: source.oriented_dimensions.height,
            map_kind: gpu_map_code(requested_map),
            normal_convention: match plan.normal_convention {
                crate::CompiledNormalConvention::OpenGl => 0,
                crate::CompiledNormalConvention::DirectX => 1,
            },
            source_role: gpu_channel_role_code(source.channel_role),
        };
        let command_buffer_bytes = encode_commands(&commands);
        command_bytes = command_bytes.saturating_add(command_buffer_bytes.len() as u64);
        let header_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("hot-trimmer-material-header"),
            contents: &encode_header(header),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let commands_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("hot-trimmer-material-region-commands"),
            contents: &command_buffer_bytes,
            usage: wgpu::BufferUsages::STORAGE,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hot-trimmer-material-bind-group"),
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
                    resource: wgpu::BindingResource::TextureView(output_view),
                },
            ],
        });
        {
            let timestamp_writes = timing.as_deref_mut().and_then(|recorder| {
                recorder.timestamp_writes(format!("{pass_label}:{:?}", source.source_id))
            });
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("hot-trimmer-material-dispatch"),
                timestamp_writes,
            });
            pass.set_pipeline(&pipeline.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(tile.width.div_ceil(16), tile.height.div_ceil(16), 1);
        }
    }
    Ok(GpuMaterialDispatchStats {
        source_cache_hits,
        upload_bytes,
        upload_ms,
        command_count,
        command_bytes,
        dispatch_ms: dispatch_started.elapsed().as_millis(),
    })
}

fn dispatch_fill_r32float_with_pipeline(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    timing: Option<&mut GpuPassTimingRecorder>,
    pipeline: &GpuAtlasPipeline,
    output_view: &wgpu::TextureView,
    width: u32,
    height: u32,
) -> Result<(), AtlasRenderExecutionError> {
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("hot-trimmer-fill-r32float-bind-group"),
        layout: &pipeline.bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(output_view),
        }],
    });
    let timestamp_writes =
        timing.and_then(|recorder| recorder.timestamp_writes("fill-r32float"));
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("hot-trimmer-fill-r32float-dispatch"),
        timestamp_writes,
    });
    pass.set_pipeline(&pipeline.pipeline);
    pass.set_bind_group(0, &bind_group, &[]);
    pass.dispatch_workgroups(width.div_ceil(16), height.div_ceil(16), 1);
    Ok(())
}

fn create_working_texture(
    device: &wgpu::Device,
    label: &'static str,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn schedule_readback(
    device: &wgpu::Device,
    cache: &Mutex<GpuAtlasSourceTextureCache>,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
    bytes_per_pixel: u32,
) -> Result<PendingGpuReadback, AtlasRenderExecutionError> {
    let output_row_bytes = u64::from(width)
        .checked_mul(u64::from(bytes_per_pixel))
        .ok_or_else(|| AtlasRenderExecutionError::Gpu("readback row size overflow".into()))?;
    let padded_bytes_per_row = align_to(
        output_row_bytes,
        u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
    );
    let readback_bytes = padded_bytes_per_row
        .checked_mul(u64::from(height))
        .ok_or_else(|| AtlasRenderExecutionError::Gpu("readback buffer size overflow".into()))?;
    let readback_buffer = cache
        .lock()
        .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?
        .readback_pool
        .acquire_staging(device, readback_bytes);
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback_buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row as u32),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    Ok(PendingGpuReadback {
        buffer: readback_buffer,
        byte_len: readback_bytes,
        output_row_bytes: usize::try_from(output_row_bytes)
            .map_err(|_| AtlasRenderExecutionError::Gpu("output row size overflow".into()))?,
        padded_row_bytes: usize::try_from(padded_bytes_per_row)
            .map_err(|_| AtlasRenderExecutionError::Gpu("padded row size overflow".into()))?,
        height,
    })
}

fn finish_readback(
    device: &wgpu::Device,
    cache: &Mutex<GpuAtlasSourceTextureCache>,
    pending: PendingGpuReadback,
) -> Result<(Arc<[u8]>, u128), AtlasRenderExecutionError> {
    let readback_started = Instant::now();
    {
        let slice = pending.buffer.slice(..);
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
        let mut pixels = vec![0; pending.output_row_bytes * pending.height as usize];
        for y in 0..pending.height as usize {
            let src = y * pending.padded_row_bytes;
            let dst = y * pending.output_row_bytes;
            pixels[dst..dst + pending.output_row_bytes]
                .copy_from_slice(&mapped[src..src + pending.output_row_bytes]);
        }
        drop(mapped);
        pending.buffer.unmap();
        cache
            .lock()
            .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?
            .readback_pool
            .release_staging(pending.buffer, pending.byte_len);
        Ok((Arc::from(pixels), readback_started.elapsed().as_millis()))
    }
}

fn remember_rendered_tile(
    cache: &Mutex<GpuAtlasSourceTextureCache>,
    plan: &CompiledAtlasPlanV1,
    map: MaterialMapKind,
    pixels: Arc<[u8]>,
) -> Result<Arc<GpuAtlasRenderedTile>, AtlasRenderExecutionError> {
    remember_rendered_tile_with_identity(
        cache,
        plan,
        map,
        plan.tile_identity(map, GPU_SHADER_VERSION),
        pixels,
    )
}

fn remember_rendered_tile_with_identity(
    cache: &Mutex<GpuAtlasSourceTextureCache>,
    plan: &CompiledAtlasPlanV1,
    map: MaterialMapKind,
    identity: crate::CompiledAtlasTileIdentity,
    pixels: Arc<[u8]>,
) -> Result<Arc<GpuAtlasRenderedTile>, AtlasRenderExecutionError> {
    let tile = plan.tile_request.output_rect.0;
    let rendered_tile = Arc::new(GpuAtlasRenderedTile {
        manifest: crate::CompiledAtlasTileManifest {
            pixel_format: identity.pixel_format,
            identity,
            map,
            mip_level: plan.tile_request.mip_level,
            output_rect: plan.tile_request.output_rect,
            valid_rect: plan.tile_request.valid_rect,
            halo_px: plan.tile_request.halo_px,
            generation: plan.tile_request.generation,
            width: tile.width,
            height: tile.height,
            row_stride: tile.width.saturating_mul(4),
            opaque_handle: String::new(),
        },
        pixels,
    });
    cache
        .lock()
        .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?
        .remember_tile(Arc::clone(&rendered_tile));
    Ok(rendered_tile)
}

fn display_tile_identity(
    plan: &CompiledAtlasPlanV1,
    map: MaterialMapKind,
) -> crate::CompiledAtlasTileIdentity {
    let mut identity = plan.tile_identity(map, GPU_SHADER_VERSION);
    identity.pixel_format = match map {
        MaterialMapKind::BaseColor => crate::CompiledTilePixelFormat::Rgba8UnormSrgb,
        _ => crate::CompiledTilePixelFormat::Rgba8UnormLinear,
    };
    identity
}

fn nonempty_command_bytes(commands: &[GpuRegionCommand]) -> Vec<u8> {
    if commands.is_empty() {
        vec![0; GPU_COMMAND_BYTES]
    } else {
        encode_commands(commands)
    }
}

fn validate_tile_size(
    plan: &CompiledAtlasPlanV1,
    caps: &hot_trimmer_preview::GpuCapabilityRecord,
) -> Result<(), AtlasRenderExecutionError> {
    let tile = plan.tile_request.output_rect.0;
    if tile.width > caps.maximum_texture_dimension_2d
        || tile.height > caps.maximum_texture_dimension_2d
    {
        return Err(AtlasRenderExecutionError::Gpu(format!(
            "tile {}x{} exceeds adapter 2D texture limit {}",
            tile.width, tile.height, caps.maximum_texture_dimension_2d
        )));
    }
    Ok(())
}

fn require_format(
    caps: &hot_trimmer_preview::GpuCapabilityRecord,
    name: &str,
    sampled: bool,
    storage: bool,
) -> Result<(), AtlasRenderExecutionError> {
    if caps.candidate_formats.iter().any(|format| {
        format.format == name && (!sampled || format.sampled) && (!storage || format.storage)
    }) {
        Ok(())
    } else {
        Err(AtlasRenderExecutionError::Gpu(format!(
            "{name} support is required with sampled={sampled} storage={storage}"
        )))
    }
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
    let payload = source_texture_payload(domain, source.channel_role)?;
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
        format: payload.format,
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
        &payload.bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(source.oriented_dimensions.width * payload.bytes_per_pixel),
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
        byte_len: payload.bytes.len() as u64,
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

struct GpuSourceTexturePayload {
    bytes: Vec<u8>,
    format: wgpu::TextureFormat,
    bytes_per_pixel: u32,
}

fn source_texture_payload(
    domain: &PreparedMaterialDomain,
    role: MaterialChannelRole,
) -> Result<GpuSourceTexturePayload, AtlasRenderExecutionError> {
    let channel = domain
        .registered_channels()
        .iter()
        .find(|channel| channel.role() == role)
        .ok_or_else(|| {
            AtlasRenderExecutionError::Gpu(format!("prepared source has no {role:?} channel"))
        })?;
    match channel {
        PreparedExemplarChannel::BaseColor { plane, .. } => {
            let mut rgba = Vec::with_capacity(
                (u64::from(domain.width) * u64::from(domain.height) * 4) as usize,
            );
            for y in 0..domain.height {
                for x in 0..domain.width {
                    let value = plane.pixel(x, y);
                    rgba.push(linear_to_srgb(value.rgb[0]));
                    rgba.push(linear_to_srgb(value.rgb[1]));
                    rgba.push(linear_to_srgb(value.rgb[2]));
                    rgba.push(unit(value.alpha));
                }
            }
            Ok(GpuSourceTexturePayload {
                bytes: rgba,
                format: wgpu::TextureFormat::Rgba8Unorm,
                bytes_per_pixel: 4,
            })
        }
        PreparedExemplarChannel::Scalar { plane, .. } => {
            let mut r32 = Vec::with_capacity(
                (u64::from(domain.width) * u64::from(domain.height) * 4) as usize,
            );
            for y in 0..domain.height {
                for x in 0..domain.width {
                    r32.extend_from_slice(&plane.pixel(x, y).0.clamp(0.0, 1.0).to_le_bytes());
                }
            }
            Ok(GpuSourceTexturePayload {
                bytes: r32,
                format: wgpu::TextureFormat::R32Float,
                bytes_per_pixel: 4,
            })
        }
        PreparedExemplarChannel::Mask { plane, .. } => {
            let mut r32 = Vec::with_capacity(
                (u64::from(domain.width) * u64::from(domain.height) * 4) as usize,
            );
            for y in 0..domain.height {
                for x in 0..domain.width {
                    r32.extend_from_slice(&plane.pixel(x, y).0.clamp(0.0, 1.0).to_le_bytes());
                }
            }
            Ok(GpuSourceTexturePayload {
                bytes: r32,
                format: wgpu::TextureFormat::R32Float,
                bytes_per_pixel: 4,
            })
        }
        PreparedExemplarChannel::Normal { plane, .. } => {
            let mut rgba = Vec::with_capacity(
                (u64::from(domain.width) * u64::from(domain.height) * 4) as usize,
            );
            for y in 0..domain.height {
                for x in 0..domain.width {
                    let value = plane.pixel(x, y);
                    rgba.extend_from_slice(&[
                        signed_unit(value.xyz[0]),
                        signed_unit(value.xyz[1]),
                        signed_unit(value.xyz[2]),
                        unit(value.alpha),
                    ]);
                }
            }
            Ok(GpuSourceTexturePayload {
                bytes: rgba,
                format: wgpu::TextureFormat::Rgba8Unorm,
                bytes_per_pixel: 4,
            })
        }
        PreparedExemplarChannel::MaterialId { plane } => {
            let mut rgba = Vec::with_capacity(
                (u64::from(domain.width) * u64::from(domain.height) * 4) as usize,
            );
            for y in 0..domain.height {
                for x in 0..domain.width {
                    let bytes = plane.pixel(x, y).0.to_le_bytes();
                    rgba.extend_from_slice(&[bytes[0], bytes[1], bytes[2], 255]);
                }
            }
            Ok(GpuSourceTexturePayload {
                bytes: rgba,
                format: wgpu::TextureFormat::Rgba8Unorm,
                bytes_per_pixel: 4,
            })
        }
    }
}

fn source_channel_role_for_source(
    plan: &CompiledAtlasPlanV1,
    source: &CompiledSourceCommandV1,
    map: hot_trimmer_domain::MaterialMapKind,
) -> MaterialChannelRole {
    use hot_trimmer_domain::MaterialMapKind;
    let preferred = match map {
        MaterialMapKind::BaseColor => MaterialChannelRole::BaseColor,
        MaterialMapKind::Height => MaterialChannelRole::Height,
        MaterialMapKind::Normal => MaterialChannelRole::Normal,
        MaterialMapKind::Roughness => MaterialChannelRole::Roughness,
        MaterialMapKind::Metallic => MaterialChannelRole::Metallic,
        MaterialMapKind::AmbientOcclusion => MaterialChannelRole::AmbientOcclusion,
        MaterialMapKind::Specular => MaterialChannelRole::Specular,
        MaterialMapKind::Opacity => MaterialChannelRole::Opacity,
        MaterialMapKind::EdgeMask => MaterialChannelRole::EdgeMask,
        MaterialMapKind::RegionId => MaterialChannelRole::RegionId,
        MaterialMapKind::MaterialId => MaterialChannelRole::MaterialId,
    };
    if plan.ordered_sources.iter().any(|candidate| {
        candidate.source_set_id == source.source_set_id
            && candidate.source_id == source.source_id
            && candidate.channel_role == preferred
    }) {
        preferred
    } else {
        MaterialChannelRole::BaseColor
    }
}

fn gpu_map_code(map: hot_trimmer_domain::MaterialMapKind) -> u32 {
    use hot_trimmer_domain::MaterialMapKind;
    match map {
        MaterialMapKind::BaseColor => 0,
        MaterialMapKind::Height => 1,
        MaterialMapKind::Normal => 2,
        MaterialMapKind::Roughness => 3,
        MaterialMapKind::AmbientOcclusion => 4,
        MaterialMapKind::Metallic => 5,
        MaterialMapKind::RegionId => 6,
        MaterialMapKind::Specular
        | MaterialMapKind::Opacity
        | MaterialMapKind::EdgeMask
        | MaterialMapKind::MaterialId => 0,
    }
}

fn gpu_channel_role_code(role: MaterialChannelRole) -> u32 {
    match role {
        MaterialChannelRole::BaseColor => 0,
        MaterialChannelRole::Height => 1,
        MaterialChannelRole::Normal => 2,
        MaterialChannelRole::Roughness => 3,
        MaterialChannelRole::AmbientOcclusion => 4,
        MaterialChannelRole::Metallic => 5,
        MaterialChannelRole::Specular => 6,
        MaterialChannelRole::Opacity => 7,
        MaterialChannelRole::EdgeMask => 8,
        MaterialChannelRole::RegionId => 9,
        MaterialChannelRole::MaterialId => 10,
    }
}

fn structural_profile_code(profile: hot_trimmer_domain::StructuralProfile) -> u32 {
    match profile {
        hot_trimmer_domain::StructuralProfile::Flat => 0,
        hot_trimmer_domain::StructuralProfile::Bevel => 1,
        hot_trimmer_domain::StructuralProfile::Groove => 2,
        hot_trimmer_domain::StructuralProfile::RoundedBevel => 3,
        hot_trimmer_domain::StructuralProfile::PanelFrame => 4,
        hot_trimmer_domain::StructuralProfile::RadialDisc => 5,
        hot_trimmer_domain::StructuralProfile::Annulus => 6,
    }
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

fn payload_counts(bytes: &[u8], format: crate::CompiledTilePixelFormat) -> (usize, usize) {
    match format {
        crate::CompiledTilePixelFormat::Rgba8UnormSrgb
        | crate::CompiledTilePixelFormat::Rgba8UnormLinear => rgba_payload_counts(bytes),
        crate::CompiledTilePixelFormat::R32Float => {
            let mut valid = 0;
            let mut nonzero = 0;
            for pixel in bytes.chunks_exact(4) {
                let value = f32::from_le_bytes(pixel.try_into().unwrap_or([0; 4]));
                if value >= 0.0 {
                    valid += 1;
                }
                if value != 0.0 {
                    nonzero += 1;
                }
            }
            (valid, nonzero)
        }
        crate::CompiledTilePixelFormat::R32Uint => {
            let mut valid = 0;
            let mut nonzero = 0;
            for pixel in bytes.chunks_exact(4) {
                let value = u32::from_le_bytes(pixel.try_into().unwrap_or([0; 4]));
                if value != u32::MAX {
                    valid += 1;
                }
                if value != 0 && value != u32::MAX {
                    nonzero += 1;
                }
            }
            (valid, nonzero)
        }
    }
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
        structural_profile: structural_profile_code(command.structural_profile),
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
        transform_rotation_sin: (-command
            .source_to_region_transform
            .rotation_degrees
            .to_radians())
        .sin() as f32,
        transform_rotation_cos: (-command
            .source_to_region_transform
            .rotation_degrees
            .to_radians())
        .cos() as f32,
    })
}

fn final_atlas_metadata(
    plan: &CompiledAtlasPlanV1,
) -> Result<Vec<(RegionId, u64)>, AtlasRenderExecutionError> {
    plan.ordered_regions
        .iter()
        .map(|command| Ok((command.region_id, region_valid_pixel_count(command)?)))
        .collect()
}

fn region_valid_pixel_count(
    command: &CompiledRegionCommandV1,
) -> Result<u64, AtlasRenderExecutionError> {
    let destination = command.destination_rect.0;
    u64::from(destination.width)
        .checked_mul(u64::from(destination.height))
        .ok_or_else(|| {
            AtlasRenderExecutionError::Gpu(format!(
                "region {} valid-pixel count overflow",
                command.region_id
            ))
        })
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
        header.map_kind,
        header.normal_convention,
        header.source_role,
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
            command.structural_profile,
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

fn signed_unit(value: f32) -> u8 {
    ((value.clamp(-1.0, 1.0) * 0.5 + 0.5) * 255.0).round() as u8
}

fn linear_to_srgb(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    unit(if value <= 0.003_130_8 {
        12.92 * value
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    })
}
