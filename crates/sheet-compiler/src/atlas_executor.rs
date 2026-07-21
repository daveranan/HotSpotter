//! Immutable atlas render-execution boundary introduced by GPU migration Prompt 1.

use std::{
    collections::{BTreeMap, BTreeSet},
    hash::{Hash, Hasher},
    num::NonZeroU64,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use hot_trimmer_domain::{
    CancellationToken, ContentDigest, MaterialChannelRole, MaterialMapKind, PixelBounds, RegionId,
    SourceSamplingMode, SourceSetId,
};
use hot_trimmer_export::{
    ExportMemoryBudgets, ExportPixelFormat, PixelRect, TiledExportError, bounded_tile_byte_len,
    choose_bounded_tile_edge,
};
use hot_trimmer_image_io::{ImagePlane, LinearColor, MaskValue};
use hot_trimmer_material_synthesis::{DomainRoute, PreparedMaterialDomain};
use hot_trimmer_placement_solver::{
    CandidateFamily, CandidateRoute, MirrorTransform, SamplingBasis, SamplingPlan,
    SliceCenterPolicy, SliceGeometry, StretchOverrideProvenance,
};
use hot_trimmer_render_core::PreparedExemplarChannel;
use wgpu::util::DeviceExt;

use crate::{
    AlgorithmCompiler, IntermediateAtlasArtifact, IntermediateAtlasRequest, SlotSynthesisLimits,
    SlotSynthesisRequest, SynthesizedSlotMaterial,
    compiled_atlas_plan::{
        CompiledAtlasPlanV1, CompiledAtlasTileIdentity, CompiledRegionCommandV1,
        CompiledSourceCommandV1, CompiledTilePixelFormat, OutputPixelRect,
    },
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
    pub tile_timings: BTreeMap<MaterialMapKind, GpuAtlasTileTiming>,
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GpuAtlasTileTiming {
    pub render_ms: u128,
    pub readback_ms: u128,
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

    /// Rebinds cache-reused pixels to the current native publication generation.
    /// Pixel identity and payload remain unchanged; only cancellation/publication
    /// lineage and the cache-owned opaque handle are refreshed.
    #[must_use]
    pub fn for_publication_generation(&self, generation: u64) -> Self {
        let mut identity = self.manifest.identity.clone();
        identity.generation = generation;
        self.with_publication_identity(identity, generation)
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
    origin_x: u32,
    origin_y: u32,
    width: u32,
    height: u32,
    decoded_format: String,
    decoder_version: String,
    color_version: String,
    channel_role: MaterialChannelRole,
    page_interior_width: u32,
    page_interior_height: u32,
    page_halo: u32,
    page_mode: u32,
    page_table_hash: u64,
}

impl GpuSourceTextureKey {
    fn from_source(source: &CompiledSourceCommandV1) -> Self {
        Self::from_source_rect(
            source,
            PixelRect {
                x: 0,
                y: 0,
                width: source.oriented_dimensions.width,
                height: source.oriented_dimensions.height,
            },
        )
    }

    fn from_source_rect(source: &CompiledSourceCommandV1, rect: PixelRect) -> Self {
        Self::from_source_page_layout(source, &single_layer_source_page_layout(rect))
    }

    fn from_source_page_layout(
        source: &CompiledSourceCommandV1,
        layout: &GpuSourcePageLayout,
    ) -> Self {
        Self {
            source_set_id: source.source_set_id,
            source_id: source.source_id.clone(),
            digest: source.digest.clone(),
            origin_x: layout.source_rect.x,
            origin_y: layout.source_rect.y,
            width: layout.source_rect.width,
            height: layout.source_rect.height,
            decoded_format: source.decoded_format.clone(),
            decoder_version: source.decoder_version.clone(),
            color_version: source.color_version.clone(),
            channel_role: source.channel_role,
            page_interior_width: layout.source_page_interior_width,
            page_interior_height: layout.source_page_interior_height,
            page_halo: layout.source_page_halo,
            page_mode: layout.source_page_mode,
            page_table_hash: layout.source_page_table_hash,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct GpuResidentSourcePage {
    x: u32,
    y: u32,
}

#[derive(Clone, Debug)]
struct GpuSourcePageLayout {
    source_rect: PixelRect,
    source_page_width: u32,
    source_page_height: u32,
    source_page_interior_width: u32,
    source_page_interior_height: u32,
    source_page_count_x: u32,
    source_page_count_y: u32,
    source_page_halo: u32,
    source_page_mode: u32,
    source_page_table_hash: u64,
    source_page_table: Vec<GpuResidentSourcePage>,
}

fn single_layer_source_page_layout(source_rect: PixelRect) -> GpuSourcePageLayout {
    GpuSourcePageLayout {
        source_rect,
        source_page_width: source_rect.width,
        source_page_height: source_rect.height,
        source_page_interior_width: source_rect.width,
        source_page_interior_height: source_rect.height,
        source_page_count_x: 1,
        source_page_count_y: 1,
        source_page_halo: 0,
        source_page_mode: 0,
        source_page_table_hash: 0,
        source_page_table: vec![GpuResidentSourcePage { x: 0, y: 0 }],
    }
}

struct GpuCachedSourceTexture {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    _validity_texture: wgpu::Texture,
    validity_view: wgpu::TextureView,
    byte_len: u64,
    layer_count: u32,
    last_used: u64,
}

struct GpuSourceTextureReservation<'a> {
    cache: &'a Mutex<GpuAtlasSourceTextureCache>,
    byte_len: u64,
    active: bool,
}

impl<'a> GpuSourceTextureReservation<'a> {
    fn commit(
        mut self,
        key: GpuSourceTextureKey,
        cached: Arc<GpuCachedSourceTexture>,
    ) -> Result<(Arc<GpuCachedSourceTexture>, GpuSourceTextureLease<'a>), AtlasRenderExecutionError>
    {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?;
        cache.source_reserved_bytes = cache.source_reserved_bytes.saturating_sub(self.byte_len);
        let cached = if let Some(existing) = cache.sources.get(&key) {
            Arc::clone(existing)
        } else {
            cache.sources.insert(key.clone(), Arc::clone(&cached));
            cached
        };
        *cache.source_pins.entry(key.clone()).or_insert(0) += 1;
        self.active = false;
        Ok((
            cached,
            GpuSourceTextureLease {
                cache: self.cache,
                key,
                active: true,
            },
        ))
    }
}

impl Drop for GpuSourceTextureReservation<'_> {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        if let Ok(mut cache) = self.cache.lock() {
            cache.source_reserved_bytes = cache.source_reserved_bytes.saturating_sub(self.byte_len);
        }
    }
}

struct GpuSourceTextureLease<'a> {
    cache: &'a Mutex<GpuAtlasSourceTextureCache>,
    key: GpuSourceTextureKey,
    active: bool,
}

impl Drop for GpuSourceTextureLease<'_> {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        if let Ok(mut cache) = self.cache.lock()
            && let Some(count) = cache.source_pins.get_mut(&self.key)
        {
            *count = count.saturating_sub(1);
            if *count == 0 {
                cache.source_pins.remove(&self.key);
            }
        }
    }
}

struct GpuMaterialSourceGroup<'a, 'cache> {
    source: &'a CompiledSourceCommandV1,
    cached: Arc<GpuCachedSourceTexture>,
    _lease: GpuSourceTextureLease<'cache>,
    cache_hit: bool,
    commands: Vec<GpuRegionCommand>,
    source_role: MaterialChannelRole,
    source_layout: GpuSourcePageLayout,
}

struct GpuMaterialSourceGroupPlan<'a> {
    source: &'a CompiledSourceCommandV1,
    commands: Vec<GpuRegionCommand>,
    source_role: MaterialChannelRole,
    residency: GpuMaterialSourceResidency,
}

enum GpuMaterialSourceResidency {
    Full(PixelRect),
    Pages(GpuResidentSourcePagePlan),
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
    StructuralProfile,
    SemanticDetail,
    EdgeDetail,
    EdgeDetailCompositionRgba8,
    EdgeDetailCompositionR32Float,
    ScalarDisplayRgba8,
}

#[derive(Default)]
pub struct GpuAtlasSourceTextureCache {
    clock: u64,
    source_reserved_bytes: u64,
    source_pins: BTreeMap<GpuSourceTextureKey, u32>,
    sources: BTreeMap<GpuSourceTextureKey, Arc<GpuCachedSourceTexture>>,
    pipelines: BTreeMap<GpuAtlasPipelineKind, Arc<GpuAtlasPipeline>>,
    rendered_tiles: Vec<Arc<GpuAtlasRenderedTile>>,
    rendered_textures: Vec<Arc<GpuCachedRenderedTexture>>,
    readback_pool: GpuAtlasReadbackPool,
}

impl GpuAtlasSourceTextureCache {
    fn budgets() -> ExportMemoryBudgets {
        ExportMemoryBudgets::default()
    }

    #[must_use]
    pub fn source_count(&self) -> usize {
        self.sources.len()
    }

    #[must_use]
    pub fn source_layer_count(&self) -> u32 {
        self.sources
            .values()
            .map(|texture| texture.layer_count)
            .sum::<u32>()
    }

    #[must_use]
    pub fn source_resident_bytes(&self) -> u64 {
        self.sources
            .values()
            .map(|texture| texture.byte_len)
            .sum::<u64>()
    }

    #[must_use]
    pub fn source_reserved_bytes(&self) -> u64 {
        self.source_reserved_bytes
    }

    #[must_use]
    pub fn source_pinned_count(&self) -> usize {
        self.source_pins.len()
    }

    #[must_use]
    pub fn rendered_tile_bytes(&self) -> u64 {
        self.rendered_tiles
            .iter()
            .map(|tile| tile.pixels.len() as u64)
            .sum::<u64>()
    }

    #[must_use]
    pub fn rendered_texture_bytes(&self) -> u64 {
        self.rendered_textures
            .iter()
            .map(|texture| texture.byte_len)
            .sum::<u64>()
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
        let budget = Self::budgets().staging_buffers_bytes;
        while self.rendered_tiles.len() > 1
            && (self.rendered_tiles.len() > 32 || self.rendered_tile_bytes() > budget)
        {
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
        byte_len: u64,
    ) {
        let pixel_identity = identity.pixel_identity();
        self.rendered_textures
            .retain(|existing| existing.pixel_identity != pixel_identity);
        self.rendered_textures
            .push(Arc::new(GpuCachedRenderedTexture {
                pixel_identity,
                _texture: texture,
                view,
                width,
                height,
                format,
                byte_len,
            }));
        let budget = Self::budgets().gpu_output_intermediate_residency_bytes;
        while self.rendered_textures.len() > 1
            && (self.rendered_textures.len() > 16 || self.rendered_texture_bytes() > budget)
        {
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
    byte_len: u64,
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
    byte_capacity: u64,
    checked_out_bytes: u64,
    available: Vec<GpuAtlasStagingBuffer>,
}

#[derive(Debug)]
pub struct GpuAtlasStagingBuffer {
    byte_len: u64,
    buffer: wgpu::Buffer,
}

struct GpuAtlasStagingLease<'a> {
    cache: &'a Mutex<GpuAtlasSourceTextureCache>,
    staging: Option<GpuAtlasStagingBuffer>,
}

impl GpuAtlasStagingLease<'_> {
    fn buffer(&self) -> &wgpu::Buffer {
        &self
            .staging
            .as_ref()
            .expect("staging lease must own a buffer until drop")
            .buffer
    }
}

impl Drop for GpuAtlasStagingLease<'_> {
    fn drop(&mut self) {
        if let Some(staging) = self.staging.take()
            && let Ok(mut cache) = self.cache.lock()
        {
            cache.readback_pool.release_staging(staging);
        }
    }
}

fn acquire_staging_lease<'a>(
    device: &wgpu::Device,
    cache: &'a Mutex<GpuAtlasSourceTextureCache>,
    byte_len: u64,
) -> Result<GpuAtlasStagingLease<'a>, AtlasRenderExecutionError> {
    let staging = cache
        .lock()
        .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?
        .readback_pool
        .acquire_staging(device, byte_len)?;
    Ok(GpuAtlasStagingLease {
        cache,
        staging: Some(staging),
    })
}

impl GpuAtlasReadbackPool {
    #[must_use]
    pub fn new(maximum_buffers: usize) -> Self {
        Self {
            maximum_buffers,
            byte_capacity: ExportMemoryBudgets::default().staging_buffers_bytes,
            checked_out_bytes: 0,
            available: Vec::new(),
        }
    }

    fn available_bytes(&self) -> u64 {
        self.available
            .iter()
            .map(|buffer| buffer.byte_len)
            .sum::<u64>()
    }

    fn reserve_staging_bytes(&mut self, byte_len: u64) -> Result<(), AtlasRenderExecutionError> {
        if byte_len > self.byte_capacity {
            return Err(AtlasRenderExecutionError::Gpu(format!(
                "readback staging request {byte_len} exceeds the declared staging budget"
            )));
        }
        if self.checked_out_bytes.saturating_add(byte_len) > self.byte_capacity {
            return Err(AtlasRenderExecutionError::Gpu(format!(
                "readback staging request {byte_len} would exceed the declared in-flight staging budget"
            )));
        }
        self.checked_out_bytes = self.checked_out_bytes.saturating_add(byte_len);
        Ok(())
    }

    fn release_staging_bytes(&mut self, byte_len: u64) {
        self.checked_out_bytes = self.checked_out_bytes.saturating_sub(byte_len);
    }

    pub fn acquire_staging(
        &mut self,
        device: &wgpu::Device,
        byte_len: u64,
    ) -> Result<GpuAtlasStagingBuffer, AtlasRenderExecutionError> {
        if let Some(index) = self
            .available
            .iter()
            .position(|buffer| buffer.byte_len >= byte_len)
        {
            let staging = self.available.swap_remove(index);
            self.reserve_staging_bytes(staging.byte_len)?;
            return Ok(staging);
        }
        while self
            .checked_out_bytes
            .saturating_add(self.available_bytes())
            .saturating_add(byte_len)
            > self.byte_capacity
            && !self.available.is_empty()
        {
            self.available.remove(0);
        }
        self.reserve_staging_bytes(byte_len)?;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hot-trimmer-base-color-readback"),
            size: byte_len,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Ok(GpuAtlasStagingBuffer { byte_len, buffer })
    }

    pub fn release_staging(&mut self, staging: GpuAtlasStagingBuffer) {
        self.release_staging_bytes(staging.byte_len);
        while self
            .checked_out_bytes
            .saturating_add(self.available_bytes())
            .saturating_add(staging.byte_len)
            > self.byte_capacity
            && !self.available.is_empty()
        {
            self.available.remove(0);
        }
        if self.available.len() < self.maximum_buffers {
            self.available.push(staging);
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
    source_origin_x: u32,
    source_origin_y: u32,
    map_kind: u32,
    normal_convention: u32,
    source_role: u32,
    source_page_width: u32,
    source_page_height: u32,
    source_page_interior_width: u32,
    source_page_interior_height: u32,
    source_page_count_x: u32,
    source_page_count_y: u32,
    source_page_halo: u32,
    source_page_mode: u32,
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
    slice_left: u32,
    slice_right: u32,
    slice_top: u32,
    slice_bottom: u32,
    slice_center: u32,
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

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct GpuProfileHeader {
    output_width: u32,
    output_height: u32,
    tile_x: u32,
    tile_y: u32,
    tile_width: u32,
    tile_height: u32,
    command_count: u32,
    _pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct GpuProfileCommand {
    program: u32,
    lod: u32,
    supersampling: u32,
    occupancy_bits: u32,
    dst_x: u32,
    dst_y: u32,
    dst_width: u32,
    dst_height: u32,
    edge_mask: u32,
    curve_offset: u32,
    curve_count: u32,
    sdf_kind: u32,
    slot_width_m: f32,
    slot_height_m: f32,
    pixels_per_meter_x: f32,
    pixels_per_meter_y: f32,
    first_width_m: f32,
    second_width_m: f32,
    minimum_flat_center_m: f32,
    amplitude_m: f32,
    angle_radians: f32,
    inner_radius_m: f32,
    outer_radius_m: f32,
    _pad_float: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct GpuEdgeDetailCommand {
    evaluator: u32,
    source_route: u32,
    seed: u32,
    edge_mask: u32,
    dst_x: u32,
    dst_y: u32,
    dst_width: u32,
    dst_height: u32,
    semantic_x: u32,
    semantic_y: u32,
    semantic_width: u32,
    semantic_height: u32,
    declared_halo_px: u32,
    cap_major_axis: u32,
    source_stencil_halo_px: u32,
    exposed_metal_enabled: u32,
    slot_width_m: f32,
    slot_height_m: f32,
    meters_per_pixel_x: f32,
    meters_per_pixel_y: f32,
    wear_amount: f32,
    intensity: f32,
    edge_width_m: f32,
    bevel_radius_m: f32,
    edge_softness: f32,
    breakup_amount: f32,
    breakup_scale_m: f32,
    micro_detail_amount: f32,
    micro_detail_scale_m: f32,
    source_height_influence: f32,
    source_luminance_influence: f32,
    height_amplitude_m: f32,
    normal_detail_strength: f32,
    source_height_range_m: f32,
    requested_extent_m: f32,
    hue_shift_degrees: f32,
    saturation_multiplier: f32,
    value_multiplier: f32,
    roughness_offset: f32,
    metallic_offset: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct GpuDetailCommand {
    family: u32,
    lod: u32,
    mapping_mode: u32,
    channel_bits: u32,
    dst_x: u32,
    dst_y: u32,
    dst_width: u32,
    dst_height: u32,
    seed: u32,
    layer_order: i32,
    occupancy_relation: u32,
    blend: u32,
    material_id: u32,
    mirror_bits: u32,
    clipping: u32,
    asset_key: u32,
    asset_layer: u32,
    asset_scalar_layer: u32,
    asset_normal_layer: u32,
    asset_material_id_layer: u32,
    asset_mask_layer: u32,
    asset_width: u32,
    asset_height: u32,
    slot_width_m: f32,
    slot_height_m: f32,
    pixels_per_meter_x: f32,
    pixels_per_meter_y: f32,
    size_x_m: f32,
    size_y_m: f32,
    position_x_m: f32,
    position_y_m: f32,
    pivot_x: f32,
    pivot_y: f32,
    period_x_m: f32,
    period_y_m: f32,
    scatter: f32,
    jitter_x_m: f32,
    jitter_y_m: f32,
    rotation_sin: f32,
    rotation_cos: f32,
    opacity: f32,
    height_amount: f32,
    normal_amount: f32,
    scalar_amount: f32,
    color_amount: f32,
}

const GPU_PROFILE_HEADER_BYTES: usize = 32;
const GPU_PROFILE_COMMAND_BYTES: usize = 96;
const GPU_DETAIL_COMMAND_BYTES: usize = 180;
const GPU_EDGE_DETAIL_COMMAND_BYTES: usize = 160;
const SOURCE_HEIGHT_RANGE_M: f32 = 0.002;

const GPU_HEADER_BYTES: usize = 88;
const GPU_COMMAND_BYTES: usize = 176;
const GPU_SHADER_VERSION: &str = "stage14-material-map-wgsl-v16-native-edge-detail-composition";

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

#[derive(Clone, Debug)]
struct CompiledExportSchedule {
    adapter_max_texture_dimension_2d: u32,
    output_width: u32,
    output_height: u32,
    output_monolithic_bytes: u64,
    output_monolithic_budget_bytes: u64,
    source_monolithic_bytes: Vec<u64>,
    source_monolithic_budget_bytes: u64,
    output_tile_edge: u32,
    source_tile_edge: u32,
    output_tiles: Vec<CompiledExportOutputTile>,
    source_tiles: Vec<CompiledExportSourceTile>,
}

#[derive(Clone, Debug)]
struct CompiledExportOutputTile {
    identity: CompiledAtlasTileIdentity,
    output_rect: OutputPixelRect,
    valid_rect: OutputPixelRect,
    bit_depth: u8,
    color_space: &'static str,
    staging_bytes: u64,
    footprints: Vec<CompiledExportSourceFootprint>,
}

#[derive(Clone, Debug)]
struct CompiledExportSourceTile {
    source_index: usize,
    source: CompiledSourceCommandV1,
    rect: PixelRect,
    halo_rect: PixelRect,
    halo_px: u32,
    byte_len: u64,
}

#[derive(Clone, Debug)]
struct CompiledExportSourceFootprint {
    region_id: RegionId,
    source_index: usize,
    source_rect: PixelRect,
    required_source_tiles: Vec<usize>,
}

fn preflight_tiled_export_plan(
    executor: &GpuAtlasRenderExecutor<'_>,
    plan: &CompiledAtlasPlanV1,
    requested_maps: &[MaterialMapKind],
) -> Result<CompiledExportSchedule, AtlasRenderExecutionError> {
    let gpu = executor
        .service
        .initialize()
        .map_err(|error| AtlasRenderExecutionError::Gpu(error.to_string()))?;
    let schedule = schedule_compiled_export_tiles(plan, requested_maps, gpu.capabilities())
        .map_err(|error| {
            AtlasRenderExecutionError::Gpu(format!("tiled compiled-plan preflight failed: {error}"))
        })?;
    if schedule.output_tiles.is_empty() {
        return Err(AtlasRenderExecutionError::Gpu(
            "tiled compiled-plan preflight produced no output tiles".into(),
        ));
    }
    if schedule.output_tile_edge == 0 || schedule.source_tile_edge == 0 {
        return Err(AtlasRenderExecutionError::Gpu(
            "tiled compiled-plan preflight selected a zero tile edge".into(),
        ));
    }
    let total_staging_bytes = schedule
        .output_tiles
        .iter()
        .map(|tile| {
            let _identity = &tile.identity;
            let _output_rect = tile.output_rect;
            let _valid_rect = tile.valid_rect;
            let metadata_bytes =
                u64::from(tile.bit_depth).saturating_add(tile.color_space.len() as u64);
            tile.staging_bytes
                .saturating_add(metadata_bytes)
                .saturating_add(
                    tile.footprints
                        .iter()
                        .map(|footprint| {
                            let _region_id = footprint.region_id;
                            let _source_rect = footprint.source_rect;
                            u64::try_from(footprint.required_source_tiles.len())
                                .unwrap_or(u64::MAX)
                                .saturating_add(
                                    u64::try_from(footprint.source_index).unwrap_or(0).min(1),
                                )
                        })
                        .sum::<u64>(),
                )
        })
        .sum::<u64>();
    let total_source_bytes = schedule
        .source_tiles
        .iter()
        .map(|tile| {
            let _source = &tile.source;
            let _rect = tile.rect;
            tile.byte_len.saturating_add(u64::from(tile.halo_px))
        })
        .sum::<u64>();
    if total_staging_bytes == 0 || total_source_bytes == 0 {
        return Err(AtlasRenderExecutionError::Gpu(
            "tiled compiled-plan preflight produced empty residency accounting".into(),
        ));
    }
    Ok(schedule)
}

fn ensure_schedule_publishable_by_current_executor(
    plan: &CompiledAtlasPlanV1,
    schedule: &CompiledExportSchedule,
    requested_maps: &[MaterialMapKind],
) -> Result<(), AtlasRenderExecutionError> {
    let current_tile = plan.tile_request.output_rect.0;
    let current_tile_bytes = current_executor_tile_residency_bytes(plan, requested_maps)
        .map_err(|error| AtlasRenderExecutionError::Gpu(error.to_string()))?;
    let current_tile_fits_renderer = current_tile.width
        <= schedule.adapter_max_texture_dimension_2d
        && current_tile.height <= schedule.adapter_max_texture_dimension_2d
        && current_tile_bytes <= schedule.output_monolithic_budget_bytes;
    let output_fits_current_renderer = schedule.output_width
        <= schedule.adapter_max_texture_dimension_2d
        && schedule.output_height <= schedule.adapter_max_texture_dimension_2d
        && schedule.output_monolithic_bytes <= schedule.output_monolithic_budget_bytes;
    let source_fits_current_renderer = schedule.source_tiles.iter().all(|tile| {
        tile.source.oriented_dimensions.width <= schedule.adapter_max_texture_dimension_2d
            && tile.source.oriented_dimensions.height <= schedule.adapter_max_texture_dimension_2d
    }) && schedule
        .source_monolithic_bytes
        .iter()
        .copied()
        .sum::<u64>()
        <= schedule.source_monolithic_budget_bytes;

    if (output_fits_current_renderer || current_tile_fits_renderer) && source_fits_current_renderer
    {
        return Ok(());
    }

    let output_tile_limit = requested_maps.len().max(1);
    if !output_fits_current_renderer
        && !current_tile_fits_renderer
        && schedule.output_tiles.len() > output_tile_limit
    {
        return Err(AtlasRenderExecutionError::Gpu(
            "compiled export schedule requires multi-output-tile streaming; the current GPU executor cannot publish that as a single interactive tile".into(),
        ));
    }
    Ok(())
}

fn current_executor_tile_residency_bytes(
    plan: &CompiledAtlasPlanV1,
    requested_maps: &[MaterialMapKind],
) -> Result<u64, TiledExportError> {
    let tile = plan.tile_request.output_rect.0;
    let mut bytes = 0_u64;
    for map in requested_maps {
        let format =
            export_format_from_compiled(plan.tile_identity(*map, GPU_SHADER_VERSION).pixel_format);
        bytes = bytes.saturating_add(bounded_tile_byte_len(
            tile.width,
            tile.height,
            format.bytes_per_pixel(),
            plan.tile_request.halo_px,
            256,
        )?);
        if material_map_requires_display_tile(*map) {
            bytes = bytes.saturating_add(bounded_tile_byte_len(
                tile.width,
                tile.height,
                4,
                plan.tile_request.halo_px,
                256,
            )?);
        }
    }
    if requested_maps.contains(&MaterialMapKind::Normal)
        && !requested_maps.contains(&MaterialMapKind::Height)
    {
        bytes = bytes.saturating_add(bounded_tile_byte_len(
            tile.width,
            tile.height,
            4,
            plan.tile_request.halo_px,
            256,
        )?);
    }
    Ok(bytes)
}

fn material_map_requires_display_tile(map: MaterialMapKind) -> bool {
    matches!(
        map,
        MaterialMapKind::Height
            | MaterialMapKind::Roughness
            | MaterialMapKind::Metallic
            | MaterialMapKind::AmbientOcclusion
            | MaterialMapKind::RegionId
    )
}

fn schedule_compiled_export_tiles(
    plan: &CompiledAtlasPlanV1,
    requested_maps: &[MaterialMapKind],
    caps: &hot_trimmer_preview::GpuCapabilityRecord,
) -> Result<CompiledExportSchedule, TiledExportError> {
    let budgets = ExportMemoryBudgets::default();
    let concurrency = budgets.total_in_flight_tiles.max(1);
    let output_monolithic_budget = budgets
        .gpu_output_intermediate_residency_bytes
        .min(budgets.staging_buffers_bytes);
    let source_monolithic_budget = budgets
        .decoded_cpu_source_tiles_bytes
        .min(budgets.gpu_source_residency_bytes);
    let largest_output_bytes_per_pixel = requested_maps
        .iter()
        .map(|map| {
            let identity = plan.tile_identity(*map, GPU_SHADER_VERSION);
            ensure_compiled_pixel_format_supported(caps, identity.pixel_format)?;
            Ok(export_format_from_compiled(identity.pixel_format).bytes_per_pixel())
        })
        .collect::<Result<Vec<_>, TiledExportError>>()?
        .into_iter()
        .max()
        .unwrap_or(4);
    let output_monolithic_bytes = bounded_tile_byte_len(
        plan.output_size.width,
        plan.output_size.height,
        largest_output_bytes_per_pixel,
        0,
        caps.copy_bytes_per_row_alignment,
    )?;
    let output_halo = plan.tile_request.halo_px.saturating_add(1);
    let output_tile_edge = choose_bounded_tile_edge(
        plan.output_size.width.max(plan.output_size.height),
        caps.maximum_texture_dimension_2d,
        largest_output_bytes_per_pixel,
        output_halo,
        budgets
            .gpu_output_intermediate_residency_bytes
            .min(budgets.staging_buffers_bytes)
            / u64::from(concurrency),
        caps.copy_bytes_per_row_alignment,
    )?;
    let largest_source_edge = plan
        .ordered_sources
        .iter()
        .map(|source| {
            source
                .oriented_dimensions
                .width
                .max(source.oriented_dimensions.height)
        })
        .max()
        .unwrap_or(1);
    let source_tile_edge = choose_bounded_tile_edge(
        largest_source_edge,
        caps.maximum_texture_dimension_2d,
        4,
        1,
        budgets
            .decoded_cpu_source_tiles_bytes
            .min(budgets.gpu_source_residency_bytes)
            / u64::from(concurrency),
        caps.copy_bytes_per_row_alignment,
    )?;
    let source_monolithic_bytes = plan
        .ordered_sources
        .iter()
        .map(|source| {
            bounded_tile_byte_len(
                source.oriented_dimensions.width,
                source.oriented_dimensions.height,
                export_source_bytes_per_pixel(source.channel_role),
                0,
                caps.copy_bytes_per_row_alignment,
            )
        })
        .collect::<Result<Vec<_>, TiledExportError>>()?;
    let source_tiles = schedule_compiled_source_tiles(plan, source_tile_edge, 1, caps)?;
    let output_tiles = schedule_compiled_output_tiles(
        plan,
        requested_maps,
        output_tile_edge,
        output_halo,
        &source_tiles,
        caps,
    )?;
    Ok(CompiledExportSchedule {
        adapter_max_texture_dimension_2d: caps.maximum_texture_dimension_2d,
        output_width: plan.output_size.width,
        output_height: plan.output_size.height,
        output_monolithic_bytes,
        output_monolithic_budget_bytes: output_monolithic_budget,
        source_monolithic_bytes,
        source_monolithic_budget_bytes: source_monolithic_budget,
        output_tile_edge,
        source_tile_edge,
        output_tiles,
        source_tiles,
    })
}

fn schedule_compiled_source_tiles(
    plan: &CompiledAtlasPlanV1,
    tile_edge: u32,
    halo_px: u32,
    caps: &hot_trimmer_preview::GpuCapabilityRecord,
) -> Result<Vec<CompiledExportSourceTile>, TiledExportError> {
    let mut tiles = Vec::new();
    for (source_index, source) in plan.ordered_sources.iter().enumerate() {
        let width = source.oriented_dimensions.width;
        let height = source.oriented_dimensions.height;
        for y in (0..height).step_by(tile_edge as usize) {
            for x in (0..width).step_by(tile_edge as usize) {
                let rect = PixelRect {
                    x,
                    y,
                    width: tile_edge.min(width - x),
                    height: tile_edge.min(height - y),
                };
                let halo_rect = inflate_rect(rect, halo_px, width, height);
                let byte_len = bounded_tile_byte_len(
                    halo_rect.width,
                    halo_rect.height,
                    export_source_bytes_per_pixel(source.channel_role),
                    0,
                    caps.copy_bytes_per_row_alignment,
                )?;
                tiles.push(CompiledExportSourceTile {
                    source_index,
                    source: source.clone(),
                    rect,
                    halo_rect,
                    halo_px,
                    byte_len,
                });
            }
        }
    }
    Ok(tiles)
}

fn schedule_compiled_output_tiles(
    plan: &CompiledAtlasPlanV1,
    requested_maps: &[MaterialMapKind],
    tile_edge: u32,
    halo_px: u32,
    source_tiles: &[CompiledExportSourceTile],
    caps: &hot_trimmer_preview::GpuCapabilityRecord,
) -> Result<Vec<CompiledExportOutputTile>, TiledExportError> {
    let mut tiles = Vec::new();
    for map in requested_maps {
        let base_identity = plan.tile_identity(*map, GPU_SHADER_VERSION);
        let format = export_format_from_compiled(base_identity.pixel_format);
        for y in (0..plan.output_size.height).step_by(tile_edge as usize) {
            for x in (0..plan.output_size.width).step_by(tile_edge as usize) {
                let valid = PixelRect {
                    x,
                    y,
                    width: tile_edge.min(plan.output_size.width - x),
                    height: tile_edge.min(plan.output_size.height - y),
                };
                let output = inflate_rect(
                    valid,
                    halo_px,
                    plan.output_size.width,
                    plan.output_size.height,
                );
                let output_rect = compiled_output_rect(output);
                let valid_rect = compiled_output_rect(valid);
                let identity =
                    compiled_tile_identity_for(plan, *map, output_rect, valid_rect, halo_px);
                let staging_bytes = bounded_tile_byte_len(
                    output.width,
                    output.height,
                    format.bytes_per_pixel(),
                    0,
                    caps.copy_bytes_per_row_alignment,
                )?;
                let bit_depth = compiled_bit_depth(base_identity.pixel_format);
                let color_space = compiled_color_space(*map, base_identity.pixel_format);
                tiles.push(CompiledExportOutputTile {
                    identity,
                    output_rect,
                    valid_rect,
                    bit_depth,
                    color_space,
                    staging_bytes,
                    footprints: compiled_source_footprints_for_tile(plan, output, source_tiles)?,
                });
            }
        }
    }
    Ok(tiles)
}

fn compiled_tile_identity_for(
    plan: &CompiledAtlasPlanV1,
    map: MaterialMapKind,
    output_rect: OutputPixelRect,
    valid_rect: OutputPixelRect,
    halo_px: u32,
) -> CompiledAtlasTileIdentity {
    let mut tile_plan = plan.clone();
    tile_plan.requested_maps = vec![map];
    tile_plan.tile_request.output_rect = output_rect;
    tile_plan.tile_request.valid_rect = valid_rect;
    tile_plan.tile_request.halo_px = halo_px;
    tile_plan.tile_identity(map, GPU_SHADER_VERSION)
}

fn compiled_source_footprints_for_tile(
    plan: &CompiledAtlasPlanV1,
    output_rect: PixelRect,
    source_tiles: &[CompiledExportSourceTile],
) -> Result<Vec<CompiledExportSourceFootprint>, TiledExportError> {
    let mut footprints = Vec::new();
    for region in &plan.ordered_regions {
        let region_output = pixel_rect(region.destination_rect.0);
        let Some(intersection) = intersect_rect(output_rect, region_output) else {
            continue;
        };
        let source_index = plan.ordered_sources.iter().position(|source| {
            source.source_set_id == region.source_set_id && source.source_id == region.source_id
        });
        let Some(source_index) = source_index else {
            continue;
        };
        let source = &plan.ordered_sources[source_index];
        for source_rect in compiled_region_source_footprints(
            region,
            intersection,
            source.oriented_dimensions.width,
            source.oriented_dimensions.height,
        )? {
            let required_source_tiles = source_tiles
                .iter()
                .enumerate()
                .filter(|(_, tile)| {
                    tile.source_index == source_index
                        && intersect_rect(tile.halo_rect, source_rect).is_some()
                })
                .map(|(index, _)| index)
                .collect();
            footprints.push(CompiledExportSourceFootprint {
                region_id: region.region_id,
                source_index,
                source_rect,
                required_source_tiles,
            });
        }
    }
    Ok(footprints)
}

fn compiled_region_source_footprints(
    region: &CompiledRegionCommandV1,
    intersection: PixelRect,
    source_width: u32,
    source_height: u32,
) -> Result<Vec<PixelRect>, TiledExportError> {
    let command = pack_command(region).map_err(|error| {
        TiledExportError::InvalidRequest(format!(
            "compiled region {} cannot be packed for footprint planning: {error}",
            region.region_id
        ))
    })?;
    if matches!(command.mode, 4 | 5) {
        return Ok(vec![PixelRect {
            x: 0,
            y: 0,
            width: source_width,
            height: source_height,
        }]);
    }
    let points = footprint_sample_points(intersection, &command);
    let mut primary = Vec::new();
    let mut seam = Vec::new();
    for point in points {
        let position = source_position_for_command(&command, point);
        primary.push(position.primary);
        if let Some(other) = position.seam {
            seam.push(other);
        }
    }
    add_wrapped_axis_extrema(&command, &mut primary);
    add_wrapped_axis_extrema(&command, &mut seam);
    let mut rects = source_bounds_rects(&primary, source_width, source_height);
    rects.extend(source_bounds_rects(&seam, source_width, source_height));
    if rects.is_empty() {
        rects.push(pixel_rect(region.source_crop.0));
    }
    Ok(rects)
}

fn execution_source_footprint_rects_for_commands(
    commands: &[GpuRegionCommand],
    output_rect: PixelRect,
    source_width: u32,
    source_height: u32,
) -> Vec<PixelRect> {
    let mut rects = Vec::new();
    for command in commands {
        let Some(intersection) = intersect_rect(output_rect, command_output_rect(command)) else {
            continue;
        };
        rects.extend(command_source_footprint_rects(
            command,
            intersection,
            source_width,
            source_height,
        ));
    }
    rects
}

fn command_source_footprint_rects(
    command: &GpuRegionCommand,
    intersection: PixelRect,
    source_width: u32,
    source_height: u32,
) -> Vec<PixelRect> {
    let points = footprint_sample_points(intersection, command);
    let mut primary = Vec::with_capacity(points.len());
    let mut seam = Vec::new();
    for point in points {
        let position = source_position_for_command(command, point);
        primary.push(position.primary);
        if let Some(other) = position.seam {
            seam.push(other);
        }
    }
    add_wrapped_axis_extrema(command, &mut primary);
    add_wrapped_axis_extrema(command, &mut seam);
    let mut rects = if command.mode == 5 {
        source_bounds_rects_wrapped_x(command, &primary, source_width, source_height)
    } else {
        source_bounds_rects(&primary, source_width, source_height)
    };
    rects.extend(if command.mode == 5 {
        source_bounds_rects_wrapped_x(command, &seam, source_width, source_height)
    } else {
        source_bounds_rects(&seam, source_width, source_height)
    });
    if rects.is_empty() {
        rects.push(PixelRect {
            x: command.crop_x.min(source_width.saturating_sub(1)),
            y: command.crop_y.min(source_height.saturating_sub(1)),
            width: command
                .crop_width
                .min(source_width.saturating_sub(command.crop_x))
                .max(1),
            height: command
                .crop_height
                .min(source_height.saturating_sub(command.crop_y))
                .max(1),
        });
    }
    rects
}

fn command_output_rect(command: &GpuRegionCommand) -> PixelRect {
    PixelRect {
        x: command.dst_x,
        y: command.dst_y,
        width: command.dst_width,
        height: command.dst_height,
    }
}

fn union_rect(a: PixelRect, b: PixelRect) -> PixelRect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let right = a.x.saturating_add(a.width).max(b.x.saturating_add(b.width));
    let bottom =
        a.y.saturating_add(a.height)
            .max(b.y.saturating_add(b.height));
    PixelRect {
        x,
        y,
        width: right.saturating_sub(x),
        height: bottom.saturating_sub(y),
    }
}

fn ensure_compiled_pixel_format_supported(
    caps: &hot_trimmer_preview::GpuCapabilityRecord,
    format: CompiledTilePixelFormat,
) -> Result<(), TiledExportError> {
    let supported = caps.candidate_formats.iter().any(|candidate| {
        (candidate.sampled || candidate.storage)
            && match format {
                CompiledTilePixelFormat::Rgba8UnormSrgb => candidate.format == "Rgba8UnormSrgb",
                CompiledTilePixelFormat::Rgba8UnormLinear => candidate.format == "Rgba8Unorm",
                CompiledTilePixelFormat::R32Float => candidate.format == "R32Float",
                CompiledTilePixelFormat::R32Uint => candidate.format == "R32Uint",
            }
    });
    if supported {
        Ok(())
    } else {
        Err(TiledExportError::UnsupportedFeatureOrFormat(format!(
            "adapter does not support compiled tile format {format:?}"
        )))
    }
}

fn export_format_from_compiled(format: CompiledTilePixelFormat) -> ExportPixelFormat {
    match format {
        CompiledTilePixelFormat::Rgba8UnormSrgb => ExportPixelFormat::Rgba8UnormSrgb,
        CompiledTilePixelFormat::Rgba8UnormLinear => ExportPixelFormat::Rgba8UnormLinear,
        CompiledTilePixelFormat::R32Float => ExportPixelFormat::R32Float,
        CompiledTilePixelFormat::R32Uint => ExportPixelFormat::R32Uint,
    }
}

fn compiled_bit_depth(format: CompiledTilePixelFormat) -> u8 {
    match format {
        CompiledTilePixelFormat::Rgba8UnormSrgb | CompiledTilePixelFormat::Rgba8UnormLinear => 8,
        CompiledTilePixelFormat::R32Float | CompiledTilePixelFormat::R32Uint => 32,
    }
}

fn compiled_color_space(map: MaterialMapKind, format: CompiledTilePixelFormat) -> &'static str {
    match (map, format) {
        (MaterialMapKind::BaseColor, CompiledTilePixelFormat::Rgba8UnormSrgb) => "sRGB",
        _ => "linear",
    }
}

fn export_source_bytes_per_pixel(_role: MaterialChannelRole) -> u64 {
    4
}

#[derive(Clone, Copy, Debug)]
struct PlannedSourcePosition {
    primary: [f32; 2],
    seam: Option<[f32; 2]>,
}

fn footprint_sample_points(rect: PixelRect, command: &GpuRegionCommand) -> Vec<[u32; 2]> {
    let right = rect.x.saturating_add(rect.width.saturating_sub(1));
    let bottom = rect.y.saturating_add(rect.height.saturating_sub(1));
    let center_x = rect.x.saturating_add(rect.width / 2).min(right);
    let center_y = rect.y.saturating_add(rect.height / 2).min(bottom);
    let step = footprint_sample_step(command);
    let mut points = BTreeSet::<[u32; 2]>::new();
    let mut x = rect.x;
    loop {
        push_footprint_point(&mut points, x, rect.y, right, bottom);
        push_footprint_point(&mut points, x, bottom, right, bottom);
        push_footprint_point(&mut points, x, center_y, right, bottom);
        if x == right {
            break;
        }
        x = x.saturating_add(step).min(right);
    }
    let mut y = rect.y;
    loop {
        push_footprint_point(&mut points, rect.x, y, right, bottom);
        push_footprint_point(&mut points, right, y, right, bottom);
        push_footprint_point(&mut points, center_x, y, right, bottom);
        if y == bottom {
            break;
        }
        y = y.saturating_add(step).min(bottom);
    }
    push_footprint_point(&mut points, center_x, center_y, right, bottom);
    if matches!(command.mode, 4 | 5) {
        let samples = rect.width.max(rect.height).div_ceil(step).max(1);
        for index in 0..=samples {
            let t = index as f32 / samples as f32;
            let left_to_right = rect.x as f32 + (right.saturating_sub(rect.x)) as f32 * t;
            let top_to_bottom = rect.y as f32 + (bottom.saturating_sub(rect.y)) as f32 * t;
            let right_to_left = right as f32 - (right.saturating_sub(rect.x)) as f32 * t;
            push_footprint_point(
                &mut points,
                left_to_right.round() as u32,
                top_to_bottom.round() as u32,
                right,
                bottom,
            );
            push_footprint_point(
                &mut points,
                right_to_left.round() as u32,
                top_to_bottom.round() as u32,
                right,
                bottom,
            );
        }
    }
    points.into_iter().collect()
}

fn footprint_sample_step(command: &GpuRegionCommand) -> u32 {
    let repeat_step = |period: u32| period.max(1).div_ceil(2).clamp(1, 64);
    match command.mode {
        1 => repeat_step(command.period_x.min(command.period_y)),
        2 => repeat_step(command.period_x),
        3 => repeat_step(command.period_y),
        4 | 5 => 4,
        _ => 64,
    }
}

fn push_footprint_point(points: &mut BTreeSet<[u32; 2]>, x: u32, y: u32, right: u32, bottom: u32) {
    points.insert([x.min(right), y.min(bottom)]);
}

fn add_wrapped_axis_extrema(command: &GpuRegionCommand, points: &mut Vec<[f32; 2]>) {
    if points.is_empty() {
        return;
    }
    let crop_size = [
        command.crop_width.max(1) as f32,
        command.crop_height.max(1) as f32,
    ];
    let crop_origin = [
        command.crop_x as f32 + command.transform_offset_x * crop_size[0],
        command.crop_y as f32 + command.transform_offset_y * crop_size[1],
    ];
    let wraps_x = matches!(command.mode, 1 | 2);
    let wraps_y = matches!(command.mode, 1 | 3);
    if !wraps_x && !wraps_y {
        return;
    }
    let x0 = crop_origin[0];
    let wrapped_x_extent = if command.mode == 5 {
        command.crop_width.max(1)
    } else {
        command.period_x.max(1).min(command.crop_width.max(1))
    };
    let x1 = crop_origin[0] + wrapped_x_extent as f32 - 0.001;
    let y0 = crop_origin[1];
    let y1 =
        crop_origin[1] + command.period_y.max(1).min(command.crop_height.max(1)) as f32 - 0.001;
    let original = points.clone();
    for point in original {
        match (wraps_x, wraps_y) {
            (true, true) => {
                points.push([x0, y0]);
                points.push([x1, y0]);
                points.push([x0, y1]);
                points.push([x1, y1]);
            }
            (true, false) => {
                points.push([x0, point[1]]);
                points.push([x1, point[1]]);
            }
            (false, true) => {
                points.push([point[0], y0]);
                points.push([point[0], y1]);
            }
            (false, false) => {}
        }
    }
}

fn source_position_for_command(cmd: &GpuRegionCommand, pixel: [u32; 2]) -> PlannedSourcePosition {
    let semantic_max_x = cmd
        .semantic_x
        .saturating_add(cmd.semantic_width.saturating_sub(1));
    let semantic_max_y = cmd
        .semantic_y
        .saturating_add(cmd.semantic_height.saturating_sub(1));
    let sem_x = pixel[0]
        .clamp(cmd.semantic_x, semantic_max_x)
        .saturating_sub(cmd.semantic_x);
    let sem_y = pixel[1]
        .clamp(cmd.semantic_y, semantic_max_y)
        .saturating_sub(cmd.semantic_y);
    let q = [
        (sem_x as f32 + 0.5) / cmd.semantic_width.max(1) as f32,
        (sem_y as f32 + 0.5) / cmd.semantic_height.max(1) as f32,
    ];
    let crop_size = [cmd.crop_width.max(1) as f32, cmd.crop_height.max(1) as f32];
    let crop_origin = [
        cmd.crop_x as f32 + cmd.transform_offset_x * crop_size[0],
        cmd.crop_y as f32 + cmd.transform_offset_y * crop_size[1],
    ];
    let destination_size = [
        cmd.slot_width.max(0.000_001),
        cmd.slot_height.max(0.000_001),
    ];
    let local = [
        (q[0] - 0.5) * destination_size[0],
        (q[1] - 0.5) * destination_size[1],
    ];
    let source_local = transform_local_cpu(local, cmd.rotation, cmd.mirror);
    let source_size = if cmd.rotation == 1 || cmd.rotation == 3 {
        [destination_size[1], destination_size[0]]
    } else {
        destination_size
    };
    let m = [
        source_local[0] + source_size[0] * 0.5,
        source_local[1] + source_size[1] * 0.5,
    ];
    let scale = cmd.pixels_per_unit * cmd.sampling_scale;
    let mut p = [
        crop_origin[0] + crop_size[0] * 0.5 + source_local[0] * scale,
        crop_origin[1] + crop_size[1] * 0.5 + source_local[1] * scale,
    ];
    let mut seam = None;
    match cmd.mode {
        1 => {
            p[0] = crop_origin[0] + positive_mod(p[0] - crop_origin[0], cmd.period_x.max(1) as f32);
            p[1] = crop_origin[1] + positive_mod(p[1] - crop_origin[1], cmd.period_y.max(1) as f32);
        }
        2 => {
            p[1] = p[1].clamp(crop_origin[1], crop_origin[1] + crop_size[1] - 1.0);
            p[0] = crop_origin[0] + positive_mod(p[0] - crop_origin[0], cmd.period_x.max(1) as f32);
        }
        3 => {
            p[0] = p[0].clamp(crop_origin[0], crop_origin[0] + crop_size[0] - 1.0);
            p[1] = crop_origin[1] + positive_mod(p[1] - crop_origin[1], cmd.period_y.max(1) as f32);
        }
        4 => {
            let delta = [q[0] - cmd.radial_center_x, q[1] - cmd.radial_center_y];
            let radius = (delta[0] * delta[0] + delta[1] * delta[1]).sqrt();
            let span = (cmd.radial_outer_radius - cmd.radial_inner_radius).max(0.000_001);
            let mut warped_radius = cmd.radial_inner_radius
                + ((radius - cmd.radial_inner_radius) / span)
                    .clamp(0.0, 1.0)
                    .powf(cmd.radial_falloff)
                    * span;
            if radius >= cmd.radial_outer_radius {
                let inset = 1.5_f32.min((crop_size[0].min(crop_size[1]) * 0.5).max(0.5));
                let normalized_inset = inset / crop_size[0].min(crop_size[1]).max(1.0);
                warped_radius = cmd
                    .radial_inner_radius
                    .max(cmd.radial_outer_radius - span * normalized_inset);
            }
            let radial_scale = if radius > 0.000_001 {
                warped_radius / radius
            } else {
                0.0
            };
            let radial_local = transform_local_cpu(
                [
                    delta[0] * radial_scale * destination_size[0],
                    delta[1] * radial_scale * destination_size[1],
                ],
                cmd.rotation,
                cmd.mirror,
            );
            p = [
                crop_origin[0] + cmd.radial_center_x * crop_size[0] + radial_local[0] * scale,
                crop_origin[1] + cmd.radial_center_y * crop_size[1] + radial_local[1] * scale,
            ];
            p[0] = p[0].clamp(crop_origin[0] + 0.5, crop_origin[0] + crop_size[0] - 0.5);
            p[1] = p[1].clamp(crop_origin[1] + 0.5, crop_origin[1] + crop_size[1] - 0.5);
        }
        5 => {
            let radial_local = transform_local_cpu(
                [q[0] - cmd.radial_center_x, q[1] - cmd.radial_center_y],
                cmd.rotation,
                cmd.mirror,
            );
            let radius =
                (radial_local[0] * radial_local[0] + radial_local[1] * radial_local[1]).sqrt();
            let span = (cmd.radial_outer_radius - cmd.radial_inner_radius).max(0.000_001);
            if radius < cmd.radial_inner_radius {
                p = [
                    crop_origin[0] + (cmd.radial_center_x + radial_local[0]) * crop_size[0],
                    crop_origin[1] + (cmd.radial_center_y + radial_local[1]) * crop_size[1],
                ];
                return PlannedSourcePosition { primary: p, seam };
            }
            let radial_inset = 1.5_f32.min((crop_size[1] * 0.5).max(0.5));
            let outer_extension_radius = cmd
                .radial_inner_radius
                .max(cmd.radial_outer_radius - span * radial_inset / crop_size[1].max(1.0));
            let sample_radius = if radius >= cmd.radial_outer_radius {
                outer_extension_radius
            } else {
                radius.clamp(cmd.radial_inner_radius, cmd.radial_outer_radius)
            };
            let theta = radial_local[1].atan2(radial_local[0]) / std::f32::consts::TAU;
            let wrapped_theta = theta - theta.floor();
            let polar = [
                (wrapped_theta * crop_size[0]).min(crop_size[0] - 0.000_001),
                (((sample_radius - cmd.radial_inner_radius) / span)
                    .clamp(0.0, 1.0)
                    .powf(cmd.radial_falloff)
                    * crop_size[1])
                    .min(crop_size[1] - 0.000_001),
            ];
            let planar = [
                (cmd.radial_center_x + radial_local[0]) * crop_size[0],
                (cmd.radial_center_y + radial_local[1]) * crop_size[1],
            ];
            let transition = cmd.radial_blend_width.min(span);
            let t = if transition > 0.000_001 {
                ((radius - cmd.radial_inner_radius) / transition).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let blend = t * t * (3.0 - 2.0 * t);
            p = [
                crop_origin[0] + planar[0] * (1.0 - blend) + polar[0] * blend,
                crop_origin[1] + planar[1] * (1.0 - blend) + polar[1] * blend,
            ];
            let seam_distance = wrapped_theta.min(1.0 - wrapped_theta);
            if cmd.radial_seam_blend_width > 0.000_001
                && seam_distance < cmd.radial_seam_blend_width
            {
                let other_polar_x =
                    ((1.0 - wrapped_theta) * crop_size[0]).min(crop_size[0] - 0.000_001);
                seam = Some([
                    crop_origin[0] + planar[0] * (1.0 - blend) + other_polar_x * blend,
                    crop_origin[1] + planar[1] * (1.0 - blend) + polar[1] * blend,
                ]);
            }
        }
        6 => {
            p = [
                crop_origin[0] + m[0] / source_size[0] * crop_size[0],
                crop_origin[1] + m[1] / source_size[1] * crop_size[1],
            ];
        }
        7 => {
            p = [
                slice_axis_cpu(
                    m[0],
                    source_size[0],
                    crop_origin[0],
                    crop_size[0],
                    cmd.slice_left,
                    cmd.slice_right,
                    scale,
                    cmd.slice_center,
                ),
                crop_origin[1] + (m[1] - source_size[1] * 0.5) * scale + crop_size[1] * 0.5,
            ];
        }
        8 | 9 => {
            let base_pixels_per_unit = if cmd.mode == 8 {
                (crop_size[0] / source_size[0]).max(crop_size[1] / source_size[1])
            } else {
                (crop_size[0] / source_size[0]).min(crop_size[1] / source_size[1])
            };
            let fit_scale = base_pixels_per_unit * cmd.sampling_scale;
            let extent = [crop_size[0] / fit_scale, crop_size[1] / fit_scale];
            let origin = [
                (source_size[0] - extent[0]) * 0.5,
                (source_size[1] - extent[1]) * 0.5,
            ];
            p = [
                crop_origin[0] + (m[0] - origin[0]) * fit_scale,
                crop_origin[1] + (m[1] - origin[1]) * fit_scale,
            ];
        }
        10 => {}
        11 => {
            p = [
                slice_axis_cpu(
                    m[0],
                    source_size[0],
                    crop_origin[0],
                    crop_size[0],
                    cmd.slice_left,
                    cmd.slice_right,
                    scale,
                    cmd.slice_center,
                ),
                slice_axis_cpu(
                    m[1],
                    source_size[1],
                    crop_origin[1],
                    crop_size[1],
                    cmd.slice_top,
                    cmd.slice_bottom,
                    scale,
                    cmd.slice_center,
                ),
            ];
        }
        _ => {}
    }
    PlannedSourcePosition { primary: p, seam }
}

fn slice_axis_cpu(
    value: f32,
    destination: f32,
    origin: f32,
    extent: f32,
    leading: u32,
    trailing: u32,
    scale: f32,
    center: u32,
) -> f32 {
    let leading_px = leading as f32;
    let trailing_px = trailing as f32;
    let leading_world = leading_px / scale;
    let trailing_world = trailing_px / scale;
    if value < leading_world {
        return origin + value * scale;
    }
    if value >= destination - trailing_world {
        return origin + extent - trailing_px + (value - (destination - trailing_world)) * scale;
    }
    let center_pixels = (extent - leading_px - trailing_px).max(1.0);
    let offset = (value - leading_world) * scale;
    if center == 0 {
        origin + leading_px + positive_mod(offset, center_pixels)
    } else if center == 1 {
        origin + leading_px + offset
    } else {
        let destination_center = (destination - leading_world - trailing_world).max(0.000_001);
        origin + leading_px + (value - leading_world) / destination_center * center_pixels
    }
}

fn slice_center_code(center: SliceCenterPolicy) -> u32 {
    match center {
        SliceCenterPolicy::Repeat => 0,
        SliceCenterPolicy::Synthesize => 1,
        SliceCenterPolicy::ExplicitStretch => 2,
    }
}

fn source_bounds_rects(
    points: &[[f32; 2]],
    source_width: u32,
    source_height: u32,
) -> Vec<PixelRect> {
    let valid = points
        .iter()
        .filter(|point| point[0].is_finite() && point[1].is_finite())
        .collect::<Vec<_>>();
    if valid.is_empty() {
        return Vec::new();
    }
    let min_x = valid
        .iter()
        .map(|point| point[0])
        .fold(f32::INFINITY, f32::min)
        .floor() as i64
        - 8;
    let min_y = valid
        .iter()
        .map(|point| point[1])
        .fold(f32::INFINITY, f32::min)
        .floor() as i64
        - 8;
    let max_x = valid
        .iter()
        .map(|point| point[0])
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil() as i64
        + 9;
    let max_y = valid
        .iter()
        .map(|point| point[1])
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil() as i64
        + 9;
    let x = min_x.clamp(0, i64::from(source_width.saturating_sub(1))) as u32;
    let y = min_y.clamp(0, i64::from(source_height.saturating_sub(1))) as u32;
    let right = max_x.clamp(i64::from(x + 1), i64::from(source_width)) as u32;
    let bottom = max_y.clamp(i64::from(y + 1), i64::from(source_height)) as u32;
    vec![PixelRect {
        x,
        y,
        width: right.saturating_sub(x).max(1),
        height: bottom.saturating_sub(y).max(1),
    }]
}

fn source_bounds_rects_wrapped_x(
    command: &GpuRegionCommand,
    points: &[[f32; 2]],
    source_width: u32,
    source_height: u32,
) -> Vec<PixelRect> {
    let valid = points
        .iter()
        .copied()
        .filter(|point| point[0].is_finite() && point[1].is_finite())
        .collect::<Vec<_>>();
    if valid.is_empty() {
        return Vec::new();
    }
    let crop_width = command.crop_width.max(1) as f32;
    let crop_origin_x = command.crop_x as f32 + command.transform_offset_x * crop_width;
    let min_x = valid
        .iter()
        .map(|point| point[0])
        .fold(f32::INFINITY, f32::min);
    let max_x = valid
        .iter()
        .map(|point| point[0])
        .fold(f32::NEG_INFINITY, f32::max);
    if max_x - min_x <= crop_width * 0.5 {
        return source_bounds_rects(&valid, source_width, source_height);
    }
    let split_x = crop_origin_x + crop_width * 0.5;
    let mut low = Vec::new();
    let mut high = Vec::new();
    for point in valid {
        if point[0] < split_x {
            low.push(point);
        } else {
            high.push(point);
        }
    }
    let mut rects = source_bounds_rects(&low, source_width, source_height);
    rects.extend(source_bounds_rects(&high, source_width, source_height));
    rects
}

fn transform_local_cpu(local: [f32; 2], rotation: u32, mirror: u32) -> [f32; 2] {
    let mut p = local;
    if mirror == 1 {
        p[0] = -p[0];
    } else if mirror == 2 {
        p[1] = -p[1];
    }
    match rotation {
        1 => [p[1], -p[0]],
        2 => [-p[0], -p[1]],
        3 => [-p[1], p[0]],
        _ => p,
    }
}

fn positive_mod(value: f32, period: f32) -> f32 {
    ((value % period) + period) % period
}

fn inflate_rect(rect: PixelRect, halo: u32, max_width: u32, max_height: u32) -> PixelRect {
    let x = rect.x.saturating_sub(halo);
    let y = rect.y.saturating_sub(halo);
    let right = rect
        .x
        .saturating_add(rect.width)
        .saturating_add(halo)
        .min(max_width);
    let bottom = rect
        .y
        .saturating_add(rect.height)
        .saturating_add(halo)
        .min(max_height);
    PixelRect {
        x,
        y,
        width: right.saturating_sub(x),
        height: bottom.saturating_sub(y),
    }
}

fn intersect_rect(a: PixelRect, b: PixelRect) -> Option<PixelRect> {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = a.x.saturating_add(a.width).min(b.x.saturating_add(b.width));
    let bottom =
        a.y.saturating_add(a.height)
            .min(b.y.saturating_add(b.height));
    (right > x && bottom > y).then(|| PixelRect {
        x,
        y,
        width: right - x,
        height: bottom - y,
    })
}

fn pixel_rect(bounds: PixelBounds) -> PixelRect {
    PixelRect {
        x: bounds.x,
        y: bounds.y,
        width: bounds.width,
        height: bounds.height,
    }
}

fn compiled_output_rect(rect: PixelRect) -> OutputPixelRect {
    OutputPixelRect(PixelBounds {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    })
}

fn validate_gpu_material_map(map: MaterialMapKind) -> Result<(), AtlasRenderExecutionError> {
    match map {
        MaterialMapKind::BaseColor
        | MaterialMapKind::Height
        | MaterialMapKind::Normal
        | MaterialMapKind::Roughness
        | MaterialMapKind::Metallic
        | MaterialMapKind::AmbientOcclusion
        | MaterialMapKind::RegionId
        | MaterialMapKind::EdgeMask
        | MaterialMapKind::MaterialId
        | MaterialMapKind::Specular
        | MaterialMapKind::Opacity => Ok(()),
    }
}

fn validate_gpu_prepared_domain_sampling(
    plan: &CompiledAtlasPlanV1,
    input: &AtlasRenderExecutionInput<'_>,
) -> Result<(), AtlasRenderExecutionError> {
    for command in &plan.ordered_regions {
        if command.sampling_plan.candidate.mapping_mode
            != hot_trimmer_domain::SamplingMode::TextureSynthesis
        {
            continue;
        }
        let SamplingBasis::PreparedDomain { window } = command.sampling_plan.sampling_basis else {
            return Err(AtlasRenderExecutionError::InvalidInput(format!(
                "region {} synthesis is missing a prepared-domain sampling basis",
                command.region_id
            )));
        };
        let candidate = &command.sampling_plan.candidate;
        if candidate.crop.is_some() || candidate.route != CandidateRoute::Synthesis {
            return Err(AtlasRenderExecutionError::InvalidInput(format!(
                "region {} has incompatible prepared synthesis provenance",
                command.region_id
            )));
        }
        let source = input
            .prepared_sources
            .iter()
            .find(|source| {
                source.source_set_id == command.source_set_id
                    && source.source_id == command.source_id
            })
            .ok_or_else(|| {
                AtlasRenderExecutionError::InvalidInput(format!(
                    "region {} has no prepared GPU source",
                    command.region_id
                ))
            })?;
        let domain = source.domain.as_ref();
        if domain.cache_key != candidate.domain_id
            || [domain.width, domain.height] != command.sampling_plan.prepared_domain_dimensions
            || domain.validity.width() != domain.width
            || domain.validity.height() != domain.height
            || !synthesis_family_matches_domain_route(candidate.family, domain.route)
        {
            return Err(AtlasRenderExecutionError::InvalidInput(format!(
                "region {} prepared synthesis domain identity, route, dimensions, or validity is incompatible",
                command.region_id
            )));
        }
        if window.width == 0
            || window.height == 0
            || window.x.saturating_add(window.width) > domain.width
            || window.y.saturating_add(window.height) > domain.height
        {
            return Err(AtlasRenderExecutionError::InvalidInput(format!(
                "region {} prepared synthesis window exceeds its domain",
                command.region_id
            )));
        }
        let rotated = matches!(
            candidate.transform.rotation,
            hot_trimmer_domain::QuarterTurn::Ninety | hot_trimmer_domain::QuarterTurn::TwoSeventy
        );
        let slot = command.sampling_plan.slot_physical_size;
        let required = if rotated { [slot[1], slot[0]] } else { slot };
        let pixels_per_unit = command.sampling_plan.source_pixels_per_physical_unit
            * command.sampling_plan.sampling_policy.scale;
        if required[0] * pixels_per_unit > f64::from(window.width) + 1.0e-9
            || required[1] * pixels_per_unit > f64::from(window.height) + 1.0e-9
        {
            return Err(AtlasRenderExecutionError::InvalidInput(format!(
                "region {} prepared synthesis window lacks required physical coverage",
                command.region_id
            )));
        }
    }
    Ok(())
}

fn validate_gpu_synthesized_slice_centers(
    plan: &CompiledAtlasPlanV1,
    input: &AtlasRenderExecutionInput<'_>,
) -> Result<(), AtlasRenderExecutionError> {
    for command in &plan.ordered_regions {
        let synthesized_center = matches!(
            command.sampling_plan.slice_geometry,
            SliceGeometry::Three {
                center: SliceCenterPolicy::Synthesize,
                ..
            } | SliceGeometry::Nine {
                center: SliceCenterPolicy::Synthesize,
                ..
            }
        );
        if !synthesized_center {
            continue;
        }
        let source = input
            .prepared_sources
            .iter()
            .find(|source| {
                source.source_set_id == command.source_set_id
                    && source.source_id == command.source_id
            })
            .ok_or_else(|| {
                AtlasRenderExecutionError::InvalidInput(format!(
                    "region {} synthesized slice center has no prepared GPU source",
                    command.region_id
                ))
            })?;
        let candidate = &command.sampling_plan.candidate;
        if candidate.domain_id != source.domain.cache_key
            || candidate.source_id != source.domain.prepared_source_digest
            || command.sampling_plan.prepared_domain_dimensions
                != [source.domain.width, source.domain.height]
            || candidate.correspondence_reference != source.domain.cache_key
        {
            return Err(AtlasRenderExecutionError::InvalidInput(format!(
                "region {} synthesized slice center prepared-domain identity is incompatible",
                command.region_id
            )));
        }
        if !matches!(
            source.domain.route,
            DomainRoute::TextureQuilting
                | DomainRoute::PatchMatch
                | DomainRoute::StatisticalSynthesis
                | DomainRoute::ProceduralReconstruction
                | DomainRoute::LearnedProvider
        ) {
            return Err(AtlasRenderExecutionError::InvalidInput(format!(
                "region {} synthesized slice center requires a synthesis-capable prepared domain",
                command.region_id
            )));
        }
        let plan = &command.sampling_plan;
        let crop = command.source_crop.0;
        let rotated = matches!(
            plan.candidate.transform.rotation,
            hot_trimmer_domain::QuarterTurn::Ninety | hot_trimmer_domain::QuarterTurn::TwoSeventy
        );
        let size = if rotated {
            [plan.slot_physical_size[1], plan.slot_physical_size[0]]
        } else {
            plan.slot_physical_size
        };
        let scale = plan.source_pixels_per_physical_unit * plan.sampling_policy.scale;
        let enough = match plan.slice_geometry {
            SliceGeometry::Three {
                leading_cap_pixels,
                trailing_cap_pixels,
                ..
            } => {
                let cap_pixels = leading_cap_pixels.saturating_add(trailing_cap_pixels);
                let requested = size[0] - f64::from(cap_pixels) / scale;
                requested >= 0.0
                    && requested * scale
                        <= f64::from(crop.width.saturating_sub(cap_pixels)) + 1.0e-9
            }
            SliceGeometry::Nine {
                left_pixels,
                right_pixels,
                top_pixels,
                bottom_pixels,
                ..
            } => {
                let horizontal = left_pixels.saturating_add(right_pixels);
                let vertical = top_pixels.saturating_add(bottom_pixels);
                let requested_x = size[0] - f64::from(horizontal) / scale;
                let requested_y = size[1] - f64::from(vertical) / scale;
                requested_x >= 0.0
                    && requested_y >= 0.0
                    && requested_x * scale
                        <= f64::from(crop.width.saturating_sub(horizontal)) + 1.0e-9
                    && requested_y * scale
                        <= f64::from(crop.height.saturating_sub(vertical)) + 1.0e-9
            }
            SliceGeometry::None => false,
        };
        if !enough {
            return Err(AtlasRenderExecutionError::InvalidInput(format!(
                "region {} synthesized slice center exceeds prepared center coverage",
                command.region_id
            )));
        }
    }
    Ok(())
}

fn synthesis_family_matches_domain_route(family: CandidateFamily, route: DomainRoute) -> bool {
    match family {
        CandidateFamily::PanelQuiltedExpansion
        | CandidateFamily::RepeatXQuilted
        | CandidateFamily::RepeatYQuilted => route == DomainRoute::TextureQuilting,
        CandidateFamily::PanelPatchMatchExpansion => route == DomainRoute::PatchMatch,
        CandidateFamily::PanelProceduralResynthesis => matches!(
            route,
            DomainRoute::StatisticalSynthesis
                | DomainRoute::ProceduralReconstruction
                | DomainRoute::LearnedProvider
        ),
        CandidateFamily::UniqueSynthesisExtension => matches!(
            route,
            DomainRoute::TextureQuilting
                | DomainRoute::PatchMatch
                | DomainRoute::StatisticalSynthesis
                | DomainRoute::ProceduralReconstruction
                | DomainRoute::LearnedProvider
        ),
        _ => false,
    }
}

fn logical_passes_for_map(plan: &CompiledAtlasPlanV1, map: MaterialMapKind) -> &'static str {
    match map {
        MaterialMapKind::BaseColor => "registered-source,edge-detail-core,edge-detail-transition,edge-detail-fade,linear-compose,publish",
        MaterialMapKind::Height => {
            "registered-source,material-height,stage15-structural-height,stage16-detail-height,edge-detail-height,final-physical-height,range-convert,publish"
        }
        MaterialMapKind::Normal => {
            "registered-source,material-height,stage15-structural-height,stage16-detail-height,edge-detail-height,final-physical-height,physical-scharr,rnm,normal-convention,publish"
        }
        MaterialMapKind::Roughness => {
            "registered-source,edge-detail-combined-mask,bounded-roughness,publish"
        }
        MaterialMapKind::AmbientOcclusion => "hotspot-profile,structural-height,ao,publish",
        MaterialMapKind::Metallic => "registered-source,edge-detail-combined-mask,explicit-metallic,publish",
        MaterialMapKind::RegionId => "compact-region-id,publish",
        MaterialMapKind::EdgeMask => {
            if plan.ordered_regions.iter().any(|region| region.edge_detail.is_some()) {
                "stage15-sdf,stage15-semantics,edge-detail-masks,publish"
            } else { "registered-source,publish" }
        }
        MaterialMapKind::Specular | MaterialMapKind::Opacity | MaterialMapKind::MaterialId => {
            "unavailable"
        }
    }
}

fn edge_detail_plan_identity(plan: &CompiledAtlasPlanV1) -> String {
    plan.ordered_regions.iter().filter_map(|region| region.edge_detail.as_ref())
        .map(|edge| edge.cache_identity.0.as_str())
        .collect::<Vec<_>>().join("|")
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
        validate_gpu_prepared_domain_sampling(plan, input)?;
        validate_gpu_synthesized_slice_centers(plan, input)?;
        if cancellation.is_cancelled() {
            return Err(AtlasRenderExecutionError::Cancelled);
        }
        if !is_current() {
            return Err(AtlasRenderExecutionError::Superseded);
        }
        let requested_maps = requested_material_maps(plan)?;
        let export_schedule = preflight_tiled_export_plan(self, plan, &requested_maps)?;
        ensure_schedule_publishable_by_current_executor(plan, &export_schedule, &requested_maps)?;
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
            let mut tile_timings = BTreeMap::<MaterialMapKind, GpuAtlasTileTiming>::new();
            let mut consumed_maps = Vec::<MaterialMapKind>::new();

            if requested_maps.contains(&MaterialMapKind::Normal) {
                let cached_normal = self.source_texture_cache.lock().ok().and_then(|mut cache| {
                    let identity = plan.tile_identity(MaterialMapKind::Normal, GPU_SHADER_VERSION);
                    cache.cached_tile(&identity, plan.tile_request.generation)
                });
                if let Some(cached_normal) = cached_normal {
                    let cached_height =
                        self.source_texture_cache.lock().ok().and_then(|mut cache| {
                            let identity =
                                plan.tile_identity(MaterialMapKind::Height, GPU_SHADER_VERSION);
                            cache.cached_tile(&identity, plan.tile_request.generation)
                        });
                    let cached_height_display =
                        self.source_texture_cache.lock().ok().and_then(|mut cache| {
                            let identity = display_tile_identity(plan, MaterialMapKind::Height);
                            cache.cached_tile(&identity, plan.tile_request.generation)
                        });
                    map_tiles.insert(MaterialMapKind::Normal, Arc::clone(&cached_normal));
                    display_tiles.insert(MaterialMapKind::Normal, cached_normal);
                    tile_timings.insert(MaterialMapKind::Normal, GpuAtlasTileTiming::default());
                    consumed_maps.push(MaterialMapKind::Normal);
                    if requested_maps.contains(&MaterialMapKind::Height)
                        && let Some(cached_height) = cached_height
                    {
                        intermediate_tiles
                            .insert("final-height".into(), Arc::clone(&cached_height));
                        intermediate_tiles
                            .insert("normal.final-height".into(), Arc::clone(&cached_height));
                        map_tiles.insert(MaterialMapKind::Height, cached_height);
                        tile_timings.insert(MaterialMapKind::Height, GpuAtlasTileTiming::default());
                        if let Some(cached_height_display) = cached_height_display {
                            display_tiles.insert(MaterialMapKind::Height, cached_height_display);
                        }
                        consumed_maps.push(MaterialMapKind::Height);
                    }
                    let cached_has_authored_normal = plan
                        .ordered_sources
                        .iter()
                        .any(|source| source.channel_role == MaterialChannelRole::Normal);
                    telemetry.push(format!(
                        "executor=gpu; requested_map=Normal; logical_passes={}; executed_gpu_passes=none; final_tile_cache=hit; dependency={}; intermediate_cache={}; gpu_tile_cache=hit; dispatch_ms=0; readback_ms=0",
                        logical_passes_for_map(plan, MaterialMapKind::Normal),
                        if cached_has_authored_normal {
                            "Normal<-authored-Normal"
                        } else {
                            "Normal<-Height"
                        },
                        if !requested_maps.contains(&MaterialMapKind::Height)
                            && cached_has_authored_normal
                        {
                            "final-height:not-used"
                        } else if map_tiles.contains_key(&MaterialMapKind::Height) {
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
                    for (map, timing) in &output.tile_timings {
                        if requested_maps.contains(map) {
                            tile_timings.insert(*map, *timing);
                        }
                    }
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
                let render_map_timing = output.tile_timings.get(render_map).copied();
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
                    tile_timings.insert(*render_map, render_map_timing.unwrap_or_default());
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
                    map_tiles: map_tiles.clone(),
                    display_tiles,
                    intermediate_tiles,
                    base_color_rgba8,
                    interactive_tile,
                    tile_timings,
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
        let logical_passes = logical_passes_for_map(plan, requested_map);
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
            let (nontransparent, nonzero_rgb) = payload_counts(
                interactive_tile.pixels(),
                interactive_tile.manifest.pixel_format,
            );
            let mut map_tiles = BTreeMap::new();
            map_tiles.insert(requested_map, Arc::clone(&cached));
            let mut display_tiles = BTreeMap::new();
            display_tiles.insert(requested_map, Arc::clone(&interactive_tile));
            return Ok(AtlasRenderExecutorOutput::FinalAtlas(
                AtlasFinalAtlasOutput {
                    map_tiles: map_tiles.clone(),
                    display_tiles,
                    intermediate_tiles: BTreeMap::new(),
                    base_color_rgba8: interactive_tile.payload(),
                    interactive_tile,
                    tile_timings: tile_timings_for(&map_tiles, 0, 0),
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
        if requested_map == MaterialMapKind::MaterialId {
            return execute_detail_material_id_gpu_tile(
                self,
                plan,
                input,
                cancellation,
                is_current,
            );
        }
        if requested_map == MaterialMapKind::Normal {
            return Ok(AtlasRenderExecutorOutput::FinalAtlas(
                execute_height_normal_gpu(self, plan, input, false, cancellation, is_current)?,
            ));
        }
        if requested_map == MaterialMapKind::EdgeMask
            && plan
                .ordered_regions
                .iter()
                .any(|region| region.edge_detail.is_some())
        {
            return execute_edge_detail_gpu_tile(self, plan, input, cancellation, is_current);
        }
        if matches!(
            requested_map,
            MaterialMapKind::Height
                | MaterialMapKind::Roughness
                | MaterialMapKind::Metallic
                | MaterialMapKind::AmbientOcclusion
                | MaterialMapKind::Specular
                | MaterialMapKind::Opacity
                | MaterialMapKind::EdgeMask
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
        let profile_stats =
            execute_or_load_profile_fields(self, plan, device, queue, cancellation, is_current)?;
        let detail_stats = execute_or_load_detail_fields(
            self,
            plan,
            Some(input),
            Some(requested_map),
            device,
            queue,
            cancellation,
            is_current,
        )?;
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
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
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
        let mut upload_ms = 0_u128;
        let mut checked_out_source_resident_bytes_peak = 0_u64;
        let mut checked_out_source_layers_peak = 0_u32;

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("hot-trimmer-base-color-clear-encoder"),
        });
        let timing: Option<GpuPassTimingRecorder> = None;
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
        submit_encoder_and_wait(device, queue, encoder)?;
        let dispatch_started = Instant::now();
        let mut command_count = 0_u32;
        let mut command_bytes = 0_u64;
        for source in &plan.ordered_sources {
            if cancellation.is_cancelled() {
                return Err(AtlasRenderExecutionError::Cancelled);
            }
            if !is_current() {
                return Err(AtlasRenderExecutionError::Superseded);
            }
            let Some(source_role) = source_channel_role_for_source(plan, source, requested_map)
            else {
                continue;
            };
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
            for group_plan in material_source_group_plans_for_tile(
                source,
                commands,
                source_role,
                tile,
                caps.maximum_texture_dimension_2d,
                device.limits().max_texture_array_layers,
            )? {
                let group_upload_started = Instant::now();
                let group = load_material_source_group(
                    device,
                    queue,
                    self.source_texture_cache,
                    prepared.domain.as_ref(),
                    group_plan,
                )?;
                upload_ms = upload_ms.saturating_add(group_upload_started.elapsed().as_millis());
                if group.cache_hit {
                    source_cache_hits = source_cache_hits.saturating_add(1);
                } else {
                    upload_bytes = upload_bytes.saturating_add(group.cached.byte_len);
                }
                let (live_source_bytes, live_source_layers) = self
                    .source_texture_cache
                    .lock()
                    .map(|cache| (cache.source_resident_bytes(), cache.source_layer_count()))
                    .unwrap_or((group.cached.byte_len, group.cached.layer_count));
                checked_out_source_resident_bytes_peak =
                    checked_out_source_resident_bytes_peak.max(live_source_bytes);
                checked_out_source_layers_peak =
                    checked_out_source_layers_peak.max(live_source_layers);
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("hot-trimmer-base-color-material-encoder"),
                });
                encode_material_source_dispatch(
                    device,
                    &mut encoder,
                    None,
                    "material-publish",
                    pipeline.as_ref(),
                    plan,
                    requested_map,
                    &output_view,
                    group.source,
                    group.cached.as_ref(),
                    &group.commands,
                    group.source_role,
                    group.source_layout,
                    tile,
                    &mut command_count,
                    &mut command_bytes,
                )?;
                submit_encoder_and_wait(device, queue, encoder)?;
            }
        }
        let dispatch_ms = dispatch_started.elapsed().as_millis();
        let edge_composition = compose_edge_detail_map(
            self, plan, input, device, queue, requested_map, &output_view,
            wgpu::TextureFormat::Rgba8Unorm, false, cancellation, is_current,
        )?;
        let published_texture = edge_composition.as_ref().map_or(&output_texture, |value| &value.0);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("hot-trimmer-base-color-readback-encoder"),
        });
        let padded_bytes_per_row = align_to(
            u64::from(tile_width) * 4,
            u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
        );
        let readback_bytes = padded_bytes_per_row
            .checked_mul(u64::from(tile_height))
            .ok_or_else(|| {
                AtlasRenderExecutionError::Gpu("readback buffer size overflow".into())
            })?;
        let output_row_bytes = usize::try_from(u64::from(tile_width) * 4)
            .map_err(|_| AtlasRenderExecutionError::Gpu("output row size overflow".into()))?;
        let padded_row_bytes = usize::try_from(padded_bytes_per_row)
            .map_err(|_| AtlasRenderExecutionError::Gpu("padded row size overflow".into()))?;
        let readback_staging =
            acquire_staging_lease(device, self.source_texture_cache, readback_bytes)?;
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: published_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: readback_staging.buffer(),
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
        let readback_pending = PendingGpuReadback {
            staging: readback_staging,
            output_row_bytes,
            padded_row_bytes,
            height: tile_height,
        };
        let (pixels, readback_ms) = finish_readback(device, readback_pending)?;
        if !is_current() {
            return Err(AtlasRenderExecutionError::Superseded);
        }
        let region_valid_pixel_counts = final_atlas_metadata(plan)?;
        let render_ms = started.elapsed().as_millis();
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
        let (source_resident_bytes, source_resident_layers) = self
            .source_texture_cache
            .lock()
            .map(|cache| (cache.source_resident_bytes(), cache.source_layer_count()))
            .unwrap_or((0, 0));
        let executed_gpu_passes = if edge_composition.is_some() {
            "material-publish,edge-detail-composition-v2"
        } else {
            "material-publish"
        };
        let intermediate_cache = if edge_composition.is_some() { "authoritative-edge-detail" } else { "not-used" };
        let mut telemetry = vec![format!(
            "executor=gpu; backend={}; plan_hash={}; requested_map={requested_map:?}; logical_passes={logical_passes}; executed_gpu_passes={executed_gpu_passes}; final_tile_cache=miss; intermediate_cache={intermediate_cache}; source_cache_hits={source_cache_hits}; source_resident_bytes={source_resident_bytes}; source_resident_layers={source_resident_layers}; checked_out_source_resident_bytes_peak={checked_out_source_resident_bytes_peak}; checked_out_source_layers_peak={checked_out_source_layers_peak}; pipeline_cache_hits={}; upload_bytes={upload_bytes}; upload_ms={upload_ms}; command_count={command_count}; command_bytes={command_bytes}; pipeline_ms={pipeline_ms}; dispatch_ms={dispatch_ms}; readback_bytes={readback_bytes}; readback_ms={readback_ms}; tile_nontransparent={nontransparent}; tile_nonzero_rgb={nonzero_rgb}; composition_ms={}; render_ms={render_ms}",
            caps.backend,
            plan.final_plan_hash.0,
            u32::from(pipeline_cache_hit),
            edge_composition.as_ref().map_or(0, |value| value.2.dispatch_ms),
        )];
        if let Some((_, _, edge_stats)) = &edge_composition {
            telemetry.push(format!("requested_map_dependencies={requested_map:?}<-RegisteredSource|Stage15Sdf|Stage15Semantics|EdgeDetailCore|EdgeDetailTransition|EdgeDetailFade; edge_detail_mask_identity={}; edge_detail_dispatches={}", edge_detail_plan_identity(plan), edge_stats.dispatches));
        }
        telemetry.push(format!(
            "profile_pass_dispatches={}; profile_cache_hit={}; profile_commands={}; profile_command_bytes={}; profile_resident_bytes={}; profile_required_halo={}; profile_dispatch_ms={}",
            profile_stats.dispatches,
            profile_stats.cache_hit,
            profile_stats.command_count,
            profile_stats.command_bytes,
            profile_stats.resident_bytes,
            profile_stats.required_halo_px,
            profile_stats.dispatch_ms,
        ));
        telemetry.push(format!(
            "detail_pass_dispatches={}; detail_cache_hit={}; detail_commands={}; detail_command_bytes={}; detail_resident_bytes={}; detail_asset_resident_bytes={}; detail_asset_upload_bytes={}; detail_required_halo={}; detail_dispatch_ms={}",
            detail_stats.dispatches,
            detail_stats.cache_hit,
            detail_stats.command_count,
            detail_stats.command_bytes,
            detail_stats.resident_bytes,
            detail_stats.asset_resident_bytes,
            detail_stats.asset_upload_bytes,
            detail_stats.required_halo_px,
            detail_stats.dispatch_ms,
        ));
        if let Some(timing) = timing {
            telemetry.extend(timing.finish(device)?);
        }
        Ok(AtlasRenderExecutorOutput::FinalAtlas(
            AtlasFinalAtlasOutput {
                map_tiles: map_tiles.clone(),
                display_tiles,
                intermediate_tiles: BTreeMap::new(),
                base_color_rgba8: Arc::clone(&pixels),
                interactive_tile,
                tile_timings: tile_timings_for(&map_tiles, dispatch_ms, readback_ms),
                region_valid_pixel_counts,
                render_ms,
                source_cache_hits,
                pipeline_cache_hits: u32::from(pipeline_cache_hit),
                upload_bytes,
                upload_ms,
                command_count: command_count.saturating_add(detail_stats.command_count),
                command_bytes: command_bytes.saturating_add(detail_stats.command_bytes),
                dispatch_ms: dispatch_ms.saturating_add(detail_stats.dispatch_ms),
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
            texture_array_layout_entry(2),
            storage_texture_layout_entry(3, wgpu::TextureFormat::Rgba8Unorm),
            source_page_table_layout_entry(4),
            texture_array_layout_entry(5),
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
            texture_array_layout_entry(2),
            storage_texture_layout_entry(3, wgpu::TextureFormat::R32Float),
            source_page_table_layout_entry(4),
            texture_array_layout_entry(5),
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
        GpuAtlasPipelineKind::StructuralProfile => vec![
            uniform_layout_entry(0, GPU_PROFILE_HEADER_BYTES as u64),
            readonly_storage_layout_entry(1, GPU_PROFILE_COMMAND_BYTES as u64),
            storage_texture_layout_entry(2, wgpu::TextureFormat::R32Float),
            storage_texture_layout_entry(3, wgpu::TextureFormat::R32Float),
            storage_texture_layout_entry(4, wgpu::TextureFormat::R32Float),
            storage_texture_layout_entry(5, wgpu::TextureFormat::R32Float),
            storage_texture_layout_entry(6, wgpu::TextureFormat::R32Float),
            readonly_storage_layout_entry(7, 8),
        ],
        GpuAtlasPipelineKind::SemanticDetail => vec![
            uniform_layout_entry(0, GPU_PROFILE_HEADER_BYTES as u64),
            readonly_storage_layout_entry(1, GPU_DETAIL_COMMAND_BYTES as u64),
            storage_texture_layout_entry(2, wgpu::TextureFormat::R32Float),
            storage_texture_layout_entry(3, wgpu::TextureFormat::R32Float),
            storage_texture_layout_entry(4, wgpu::TextureFormat::Rgba8Unorm),
            storage_texture_layout_entry(5, wgpu::TextureFormat::R32Float),
            storage_texture_layout_entry(6, wgpu::TextureFormat::Rgba8Unorm),
            storage_texture_layout_entry(7, wgpu::TextureFormat::R32Uint),
            storage_texture_layout_entry(8, wgpu::TextureFormat::R32Uint),
            texture_array_layout_entry(9),
            texture_array_layout_entry(10),
            texture_array_layout_entry(11),
            uint_texture_array_layout_entry(12),
            texture_array_layout_entry(13),
            texture_layout_entry(14),
        ],
        GpuAtlasPipelineKind::EdgeDetail => vec![
            uniform_layout_entry(0, GPU_PROFILE_HEADER_BYTES as u64),
            readonly_storage_layout_entry(1, GPU_EDGE_DETAIL_COMMAND_BYTES as u64),
            texture_layout_entry(2),
            texture_layout_entry(3),
            texture_layout_entry(4),
            texture_layout_entry(5),
            storage_texture_layout_entry(6, wgpu::TextureFormat::R32Float),
            storage_texture_layout_entry(7, wgpu::TextureFormat::R32Float),
            storage_texture_layout_entry(8, wgpu::TextureFormat::R32Float),
            storage_texture_layout_entry(9, wgpu::TextureFormat::R32Float),
            storage_texture_layout_entry(10, wgpu::TextureFormat::R32Float),
        ],
        GpuAtlasPipelineKind::EdgeDetailCompositionRgba8
        | GpuAtlasPipelineKind::EdgeDetailCompositionR32Float => vec![
            uniform_layout_entry(0, 32),
            readonly_storage_layout_entry(1, GPU_EDGE_DETAIL_COMMAND_BYTES as u64),
            texture_layout_entry(2), texture_layout_entry(3), texture_layout_entry(4),
            texture_layout_entry(5), texture_layout_entry(6), texture_layout_entry(7),
            texture_layout_entry(8), texture_layout_entry(9),
            storage_texture_layout_entry(10, if kind == GpuAtlasPipelineKind::EdgeDetailCompositionRgba8 {
                wgpu::TextureFormat::Rgba8Unorm
            } else { wgpu::TextureFormat::R32Float }),
        ],
        GpuAtlasPipelineKind::ScalarDisplayRgba8 => vec![
            uniform_layout_entry(0, 32),
            texture_layout_entry(1),
            texture_layout_entry(2),
            storage_texture_layout_entry(3, wgpu::TextureFormat::Rgba8Unorm),
        ],
    };
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(match kind {
            GpuAtlasPipelineKind::MaterialRgba8 => "hot-trimmer-material-rgba8-bind-layout",
            GpuAtlasPipelineKind::MaterialR32Float => "hot-trimmer-material-r32float-bind-layout",
            GpuAtlasPipelineKind::FillR32Float => "hot-trimmer-fill-r32float-bind-layout",
            GpuAtlasPipelineKind::NormalFromHeight => "hot-trimmer-normal-from-height-bind-layout",
            GpuAtlasPipelineKind::RegionIdR32Uint => "hot-trimmer-region-id-bind-layout",
            GpuAtlasPipelineKind::RegionIdDisplayRgba8 => {
                "hot-trimmer-region-id-display-bind-layout"
            }
            GpuAtlasPipelineKind::StructuralProfile => "hot-trimmer-structural-profile-bind-layout",
            GpuAtlasPipelineKind::SemanticDetail => "hot-trimmer-semantic-detail-bind-layout",
            GpuAtlasPipelineKind::EdgeDetail => "hot-trimmer-edge-detail-bind-layout",
            GpuAtlasPipelineKind::EdgeDetailCompositionRgba8 => "hot-trimmer-edge-detail-composition-rgba8-bind-layout",
            GpuAtlasPipelineKind::EdgeDetailCompositionR32Float => "hot-trimmer-edge-detail-composition-r32float-bind-layout",
            GpuAtlasPipelineKind::ScalarDisplayRgba8 => "hot-trimmer-scalar-display-bind-layout",
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
        GpuAtlasPipelineKind::StructuralProfile => {
            hot_trimmer_preview::STRUCTURAL_PROFILE_ATLAS_WGSL.into()
        }
        GpuAtlasPipelineKind::SemanticDetail => {
            hot_trimmer_preview::SEMANTIC_DETAIL_ATLAS_WGSL.into()
        }
        GpuAtlasPipelineKind::EdgeDetail => hot_trimmer_preview::EDGE_DETAIL_ATLAS_WGSL.into(),
        GpuAtlasPipelineKind::EdgeDetailCompositionRgba8 => hot_trimmer_preview::EDGE_DETAIL_COMPOSITION_ATLAS_WGSL.into(),
        GpuAtlasPipelineKind::EdgeDetailCompositionR32Float => hot_trimmer_preview::EDGE_DETAIL_COMPOSITION_ATLAS_WGSL
            .replace("texture_storage_2d<rgba8unorm, write>", "texture_storage_2d<r32float, write>").into(),
        GpuAtlasPipelineKind::ScalarDisplayRgba8 => hot_trimmer_preview::SCALAR_DISPLAY_ATLAS_WGSL.into(),
    };
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(match kind {
            GpuAtlasPipelineKind::MaterialRgba8 => "hot-trimmer-material-rgba8-wgsl",
            GpuAtlasPipelineKind::MaterialR32Float => "hot-trimmer-material-r32float-wgsl",
            GpuAtlasPipelineKind::FillR32Float => "hot-trimmer-fill-r32float-wgsl",
            GpuAtlasPipelineKind::NormalFromHeight => "hot-trimmer-normal-from-height-wgsl",
            GpuAtlasPipelineKind::RegionIdR32Uint => "hot-trimmer-region-id-wgsl",
            GpuAtlasPipelineKind::RegionIdDisplayRgba8 => "hot-trimmer-region-id-display-wgsl",
            GpuAtlasPipelineKind::StructuralProfile => "hot-trimmer-structural-profile-wgsl",
            GpuAtlasPipelineKind::SemanticDetail => "hot-trimmer-semantic-detail-wgsl",
            GpuAtlasPipelineKind::EdgeDetail => "hot-trimmer-edge-detail-wgsl",
            GpuAtlasPipelineKind::EdgeDetailCompositionRgba8 => "hot-trimmer-edge-detail-composition-rgba8-wgsl",
            GpuAtlasPipelineKind::EdgeDetailCompositionR32Float => "hot-trimmer-edge-detail-composition-r32float-wgsl",
            GpuAtlasPipelineKind::ScalarDisplayRgba8 => "hot-trimmer-scalar-display-wgsl",
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
            GpuAtlasPipelineKind::StructuralProfile => "hot-trimmer-structural-profile-pipeline",
            GpuAtlasPipelineKind::SemanticDetail => "hot-trimmer-semantic-detail-pipeline",
            GpuAtlasPipelineKind::EdgeDetail => "hot-trimmer-edge-detail-pipeline",
            GpuAtlasPipelineKind::EdgeDetailCompositionRgba8 => "hot-trimmer-edge-detail-composition-rgba8-pipeline",
            GpuAtlasPipelineKind::EdgeDetailCompositionR32Float => "hot-trimmer-edge-detail-composition-r32float-pipeline",
            GpuAtlasPipelineKind::ScalarDisplayRgba8 => "hot-trimmer-scalar-display-pipeline",
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

fn uniform_layout_entry(binding: u32, minimum_size: u64) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: NonZeroU64::new(minimum_size),
        },
        count: None,
    }
}

fn readonly_storage_layout_entry(binding: u32, minimum_size: u64) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: NonZeroU64::new(minimum_size),
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

fn source_page_table_layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: NonZeroU64::new(16),
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

fn texture_array_layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2Array,
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

fn uint_texture_array_layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Uint,
            view_dimension: wgpu::TextureViewDimension::D2Array,
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

struct PendingGpuReadback<'a> {
    staging: GpuAtlasStagingLease<'a>,
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
                let delta_ns =
                    end.saturating_sub(start) as f64 * f64::from(self.timestamp_period_ns);
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
    checked_out_source_resident_bytes_peak: u64,
    checked_out_source_layers_peak: u32,
    command_count: u32,
    command_bytes: u64,
    dispatch_ms: u128,
}

#[derive(Default)]
struct GpuProfileDispatchStats {
    cache_hit: bool,
    dispatches: u32,
    command_count: u32,
    command_bytes: u64,
    resident_bytes: u64,
    required_halo_px: u32,
    dispatch_ms: u128,
}

#[derive(Default)]
struct GpuDetailDispatchStats {
    cache_hit: bool,
    dispatches: u32,
    command_count: u32,
    command_bytes: u64,
    resident_bytes: u64,
    asset_resident_bytes: u64,
    asset_upload_bytes: u64,
    required_halo_px: u32,
    dispatch_ms: u128,
}

#[derive(Default)]
struct GpuEdgeDetailDispatchStats {
    cache_hit: bool,
    dispatches: u32,
    command_count: u32,
    command_bytes: u64,
    resident_bytes: u64,
    source_cache_hits: u32,
    source_upload_bytes: u64,
    required_halo_px: u32,
    dispatch_ms: u128,
}

const PROFILE_FIELD_IDENTITIES: [(MaterialMapKind, &str); 5] = [
    (MaterialMapKind::Height, "stage15-profile-height-v1"),
    (MaterialMapKind::Roughness, "stage15-profile-sdf-v2"),
    (
        MaterialMapKind::AmbientOcclusion,
        "stage15-profile-semantics-v2",
    ),
    (MaterialMapKind::Metallic, "stage15-profile-derivative-x-v1"),
    (MaterialMapKind::Opacity, "stage15-profile-derivative-y-v1"),
];

const EDGE_DETAIL_FIELD_IDENTITIES: [(MaterialMapKind, &str); 5] = [
    (MaterialMapKind::BaseColor, "edge-detail-core-r32float-v2"),
    (MaterialMapKind::Roughness, "edge-detail-transition-r32float-v2"),
    (MaterialMapKind::AmbientOcclusion, "edge-detail-fade-r32float-v2"),
    (MaterialMapKind::EdgeMask, "edge-detail-combined-r32float-v2"),
    (MaterialMapKind::Height, "edge-detail-height-r32float-v2"),
];

#[derive(Clone, Copy)]
struct DetailFieldSpec {
    map: MaterialMapKind,
    shader: &'static str,
    field_index: usize,
    format: wgpu::TextureFormat,
    pixel_format: crate::CompiledTilePixelFormat,
}

const DETAIL_FIELD_MASK: usize = 0;
const DETAIL_FIELD_HEIGHT: usize = 1;
const DETAIL_FIELD_NORMAL_INPUT: usize = 2;
const DETAIL_FIELD_SCALAR: usize = 3;
const DETAIL_FIELD_COLOR: usize = 4;
const DETAIL_FIELD_MATERIAL_ID: usize = 5;
const DETAIL_FIELD_MATERIAL_ID_VALID: usize = 6;
const DETAIL_REQUEST_ALL_FIELDS: u32 = 99;

const DETAIL_FIELD_SPECS: [DetailFieldSpec; 10] = [
    DetailFieldSpec {
        map: MaterialMapKind::BaseColor,
        shader: "stage16-detail-base-color-rgba-v2",
        field_index: DETAIL_FIELD_COLOR,
        format: wgpu::TextureFormat::Rgba8Unorm,
        pixel_format: crate::CompiledTilePixelFormat::Rgba8UnormSrgb,
    },
    DetailFieldSpec {
        map: MaterialMapKind::EdgeMask,
        shader: "stage16-detail-mask-v1",
        field_index: DETAIL_FIELD_MASK,
        format: wgpu::TextureFormat::R32Float,
        pixel_format: crate::CompiledTilePixelFormat::R32Float,
    },
    DetailFieldSpec {
        map: MaterialMapKind::Height,
        shader: "stage16-detail-height-v1",
        field_index: DETAIL_FIELD_HEIGHT,
        format: wgpu::TextureFormat::R32Float,
        pixel_format: crate::CompiledTilePixelFormat::R32Float,
    },
    DetailFieldSpec {
        map: MaterialMapKind::Normal,
        shader: "stage16-detail-normal-rgba-v2",
        field_index: DETAIL_FIELD_NORMAL_INPUT,
        format: wgpu::TextureFormat::Rgba8Unorm,
        pixel_format: crate::CompiledTilePixelFormat::Rgba8UnormLinear,
    },
    DetailFieldSpec {
        map: MaterialMapKind::Roughness,
        shader: "stage16-detail-roughness-scalar-v1",
        field_index: DETAIL_FIELD_SCALAR,
        format: wgpu::TextureFormat::R32Float,
        pixel_format: crate::CompiledTilePixelFormat::R32Float,
    },
    DetailFieldSpec {
        map: MaterialMapKind::Metallic,
        shader: "stage16-detail-metallic-scalar-v1",
        field_index: DETAIL_FIELD_SCALAR,
        format: wgpu::TextureFormat::R32Float,
        pixel_format: crate::CompiledTilePixelFormat::R32Float,
    },
    DetailFieldSpec {
        map: MaterialMapKind::AmbientOcclusion,
        shader: "stage16-detail-ao-scalar-v1",
        field_index: DETAIL_FIELD_SCALAR,
        format: wgpu::TextureFormat::R32Float,
        pixel_format: crate::CompiledTilePixelFormat::R32Float,
    },
    DetailFieldSpec {
        map: MaterialMapKind::Specular,
        shader: "stage16-detail-specular-scalar-v1",
        field_index: DETAIL_FIELD_SCALAR,
        format: wgpu::TextureFormat::R32Float,
        pixel_format: crate::CompiledTilePixelFormat::R32Float,
    },
    DetailFieldSpec {
        map: MaterialMapKind::Opacity,
        shader: "stage16-detail-opacity-scalar-v1",
        field_index: DETAIL_FIELD_SCALAR,
        format: wgpu::TextureFormat::R32Float,
        pixel_format: crate::CompiledTilePixelFormat::R32Float,
    },
    DetailFieldSpec {
        map: MaterialMapKind::MaterialId,
        shader: "stage16-detail-material-id-r32uint-v2",
        field_index: DETAIL_FIELD_MATERIAL_ID,
        format: wgpu::TextureFormat::R32Uint,
        pixel_format: crate::CompiledTilePixelFormat::R32Uint,
    },
];

struct DetailFieldIdentity {
    field_index: usize,
    identity: crate::CompiledAtlasTileIdentity,
    format: wgpu::TextureFormat,
}

fn execute_or_load_profile_fields(
    executor: &GpuAtlasRenderExecutor<'_>,
    plan: &CompiledAtlasPlanV1,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    cancellation: &CancellationToken,
    is_current: &dyn Fn() -> bool,
) -> Result<GpuProfileDispatchStats, AtlasRenderExecutionError> {
    if cancellation.is_cancelled() {
        return Err(AtlasRenderExecutionError::Cancelled);
    }
    if !is_current() {
        return Err(AtlasRenderExecutionError::Superseded);
    }
    let tile = plan.tile_request.output_rect.0;
    let bytes_per_field = u64::from(tile.width)
        .saturating_mul(u64::from(tile.height))
        .saturating_mul(4);
    let resident_bytes = bytes_per_field.saturating_mul(5);
    if resident_bytes
        > GpuAtlasSourceTextureCache::budgets().gpu_output_intermediate_residency_bytes
    {
        return Err(AtlasRenderExecutionError::Gpu(format!(
            "Stage 15 profile fields require {resident_bytes} bytes, exceeding the bounded GPU tile residency budget"
        )));
    }
    let identities = PROFILE_FIELD_IDENTITIES.map(|(map, shader)| plan.tile_identity(map, shader));
    let cached_count = executor
        .source_texture_cache
        .lock()
        .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?
        .rendered_textures
        .iter()
        .filter(|texture| {
            identities
                .iter()
                .any(|identity| texture.pixel_identity == identity.pixel_identity())
                && texture.format == wgpu::TextureFormat::R32Float
                && texture.width == tile.width
                && texture.height == tile.height
        })
        .count();
    let required_halo_px = plan
        .ordered_regions
        .iter()
        .map(|region| region.compiled_profile.required_halo_px)
        .max()
        .unwrap_or(0);
    if cached_count == identities.len() {
        return Ok(GpuProfileDispatchStats {
            cache_hit: true,
            resident_bytes,
            required_halo_px,
            ..GpuProfileDispatchStats::default()
        });
    }

    let started = Instant::now();
    let (profile_pipeline, _) = pipeline(
        device,
        executor.source_texture_cache,
        GpuAtlasPipelineKind::StructuralProfile,
    )?;
    let mut commands = Vec::with_capacity(plan.ordered_regions.len());
    let mut curve_points = Vec::<[f32; 2]>::new();
    for region in &plan.ordered_regions {
        let profile = &region.compiled_profile;
        let curve_offset = curve_points.len() as u32;
        curve_points.extend(
            profile
                .custom_curve
                .iter()
                .map(|point| [point.position as f32, point.height as f32]),
        );
        let edges = region.edge_eligibility;
        let edge_mask = u32::from(edges.left)
            | (u32::from(edges.right) << 1)
            | (u32::from(edges.top) << 2)
            | (u32::from(edges.bottom) << 3);
        let occupancy_bits = u32::from(profile.occupancy.signed_distance)
            | (u32::from(profile.occupancy.inside_outside) << 1)
            | (u32::from(profile.occupancy.flat_center) << 2)
            | (u32::from(profile.occupancy.raised) << 3)
            | (u32::from(profile.occupancy.recessed) << 4)
            | (u32::from(profile.occupancy.cap) << 5)
            | (u32::from(profile.occupancy.groove) << 6)
            | (u32::from(profile.occupancy.profile_exclusion) << 7);
        let dst = region.destination_rect.0;
        commands.push(GpuProfileCommand {
            program: profile_program_code(profile.program),
            lod: profile_lod_code(profile.lod),
            supersampling: u32::from(profile.supersampling),
            occupancy_bits,
            dst_x: dst.x,
            dst_y: dst.y,
            dst_width: dst.width,
            dst_height: dst.height,
            edge_mask,
            curve_offset,
            curve_count: profile.custom_curve.len() as u32,
            sdf_kind: profile_sdf_code(profile.sdf),
            slot_width_m: profile.slot_size_m[0] as f32,
            slot_height_m: profile.slot_size_m[1] as f32,
            pixels_per_meter_x: profile.pixels_per_meter[0] as f32,
            pixels_per_meter_y: profile.pixels_per_meter[1] as f32,
            first_width_m: profile.first_width_m as f32,
            second_width_m: profile.second_width_m as f32,
            minimum_flat_center_m: profile.minimum_flat_center_m as f32,
            amplitude_m: profile.amplitude_m as f32,
            angle_radians: (profile.angle_degrees as f32).to_radians(),
            inner_radius_m: profile.inner_radius_m as f32,
            outer_radius_m: profile.outer_radius_m as f32,
            _pad_float: 0.0,
        });
    }
    if commands.is_empty() {
        return Ok(GpuProfileDispatchStats {
            required_halo_px,
            ..Default::default()
        });
    }
    if curve_points.is_empty() {
        curve_points.push([0.0, 0.0]);
    }
    let header = GpuProfileHeader {
        output_width: plan.output_size.width,
        output_height: plan.output_size.height,
        tile_x: tile.x,
        tile_y: tile.y,
        tile_width: tile.width,
        tile_height: tile.height,
        command_count: commands.len() as u32,
        _pad: 0,
    };
    let header_bytes = encode_profile_header(header);
    let command_bytes = encode_profile_commands(&commands);
    let curve_bytes = encode_profile_curves(&curve_points);
    let header_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("hot-trimmer-profile-header"),
        contents: &header_bytes,
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let command_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("hot-trimmer-profile-commands"),
        contents: &command_bytes,
        usage: wgpu::BufferUsages::STORAGE,
    });
    let curve_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("hot-trimmer-profile-curves"),
        contents: &curve_bytes,
        usage: wgpu::BufferUsages::STORAGE,
    });
    let mut fields = Vec::with_capacity(5);
    for label in ["height", "sdf", "semantics", "derivative-x", "derivative-y"] {
        fields.push(create_working_texture(
            device,
            match label {
                "height" => "hot-trimmer-profile-height",
                "sdf" => "hot-trimmer-profile-sdf",
                "semantics" => "hot-trimmer-profile-semantics",
                "derivative-x" => "hot-trimmer-profile-derivative-x",
                _ => "hot-trimmer-profile-derivative-y",
            },
            tile.width,
            tile.height,
            wgpu::TextureFormat::R32Float,
            wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        ));
    }
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("hot-trimmer-profile-bind-group"),
        layout: &profile_pipeline.bind_group_layout,
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
                resource: wgpu::BindingResource::TextureView(&fields[0].1),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&fields[1].1),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&fields[2].1),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::TextureView(&fields[3].1),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(&fields[4].1),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: curve_buffer.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("hot-trimmer-profile-encoder"),
    });
    for (texture, _) in &fields {
        encoder.clear_texture(texture, &wgpu::ImageSubresourceRange::default());
    }
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("hot-trimmer-profile-dispatch"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&profile_pipeline.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(tile.width.div_ceil(16), tile.height.div_ceil(16), 1);
    }
    submit_encoder_and_wait(device, queue, encoder)?;
    if cancellation.is_cancelled() {
        return Err(AtlasRenderExecutionError::Cancelled);
    }
    if !is_current() {
        return Err(AtlasRenderExecutionError::Superseded);
    }
    let mut cache = executor
        .source_texture_cache
        .lock()
        .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?;
    for ((texture, view), identity) in fields.into_iter().zip(identities) {
        cache.remember_rendered_texture(
            identity,
            texture,
            view,
            tile.width,
            tile.height,
            wgpu::TextureFormat::R32Float,
            bytes_per_field,
        );
    }
    Ok(GpuProfileDispatchStats {
        cache_hit: false,
        dispatches: 1,
        command_count: commands.len() as u32,
        command_bytes: command_bytes.len() as u64,
        resident_bytes,
        required_halo_px,
        dispatch_ms: started.elapsed().as_millis(),
    })
}

fn edge_detail_identity(
    plan: &CompiledAtlasPlanV1,
    map: MaterialMapKind,
    shader: &str,
) -> crate::CompiledAtlasTileIdentity {
    let mut identity = plan.tile_identity(map, shader);
    identity.pixel_format = crate::CompiledTilePixelFormat::R32Float;
    identity
}

fn edge_detail_evaluator_code(
    evaluator: hot_trimmer_effect_compiler::EdgeDetailRoleEvaluator,
) -> u32 {
    use hot_trimmer_effect_compiler::EdgeDetailRoleEvaluator;
    match evaluator {
        EdgeDetailRoleEvaluator::RectangularPanel => 0,
        EdgeDetailRoleEvaluator::HorizontalStrip => 1,
        EdgeDetailRoleEvaluator::VerticalStrip => 2,
        EdgeDetailRoleEvaluator::RadialOuter => 3,
        EdgeDetailRoleEvaluator::RadialInnerOuter => 4,
        EdgeDetailRoleEvaluator::TrimCap => 5,
        EdgeDetailRoleEvaluator::Unique => 6,
    }
}

fn edge_detail_source_route_code(
    route: hot_trimmer_effect_compiler::EdgeDetailSourceModulationRoute,
) -> u32 {
    use hot_trimmer_effect_compiler::EdgeDetailSourceModulationRoute;
    match route {
        EdgeDetailSourceModulationRoute::None => 0,
        EdgeDetailSourceModulationRoute::RegisteredHeight => 1,
        EdgeDetailSourceModulationRoute::HighPassedLinearLuminance => 2,
    }
}

fn pack_edge_detail_command(region: &CompiledRegionCommandV1) -> Option<GpuEdgeDetailCommand> {
    let edge = region.edge_detail.as_ref()?;
    let destination = region.destination_rect.0;
    let semantic = semantic_rect_for_padding(
        hot_trimmer_domain::CanonicalRect {
            x: destination.x, y: destination.y,
            width: destination.width, height: destination.height,
        },
        region.padding_px,
        region.edge_eligibility,
    );
    let eligible = region.edge_eligibility;
    let edge_mask = u32::from(eligible.left)
        | (u32::from(eligible.right) << 1)
        | (u32::from(eligible.top) << 2)
        | (u32::from(eligible.bottom) << 3);
    Some(GpuEdgeDetailCommand {
        evaluator: edge_detail_evaluator_code(edge.evaluator),
        source_route: edge_detail_source_route_code(edge.source_modulation_route),
        seed: edge.seed, edge_mask,
        dst_x: destination.x, dst_y: destination.y,
        dst_width: destination.width, dst_height: destination.height,
        semantic_x: semantic.x, semantic_y: semantic.y,
        semantic_width: semantic.width, semantic_height: semantic.height,
        declared_halo_px: u32::from(
            edge.source_modulation_route
                != hot_trimmer_effect_compiler::EdgeDetailSourceModulationRoute::None,
        ),
        cap_major_axis: u32::from(matches!(
            edge.manual_role,
            hot_trimmer_domain::ManualRegionRole::VerticalStrip
        )),
        source_stencil_halo_px: u32::from(
            edge.source_modulation_route
                != hot_trimmer_effect_compiler::EdgeDetailSourceModulationRoute::None,
        ),
        exposed_metal_enabled: u32::from(edge.exposed_metal_enabled),
        slot_width_m: edge.slot_size_m[0] as f32,
        slot_height_m: edge.slot_size_m[1] as f32,
        meters_per_pixel_x: edge.meters_per_pixel[0] as f32,
        meters_per_pixel_y: edge.meters_per_pixel[1] as f32,
        wear_amount: edge.wear_amount, intensity: edge.intensity,
        edge_width_m: edge.edge_width_m, bevel_radius_m: edge.bevel_radius_m,
        edge_softness: edge.edge_softness, breakup_amount: edge.breakup_amount,
        breakup_scale_m: edge.breakup_scale_m,
        micro_detail_amount: edge.micro_detail_amount,
        micro_detail_scale_m: edge.micro_detail_scale_m,
        source_height_influence: edge.source_height_influence,
        source_luminance_influence: edge.source_luminance_influence,
        height_amplitude_m: edge.height_amplitude_m,
        normal_detail_strength: edge.normal_detail_strength,
        source_height_range_m: SOURCE_HEIGHT_RANGE_M,
        requested_extent_m: edge.requested_physical_extent_m as f32,
        hue_shift_degrees: edge.hue_shift_degrees,
        saturation_multiplier: edge.saturation_multiplier,
        value_multiplier: edge.value_multiplier,
        roughness_offset: edge.roughness_offset,
        metallic_offset: edge.metallic_offset,
    })
}

fn encode_edge_detail_commands(commands: &[GpuEdgeDetailCommand]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(commands.len() * GPU_EDGE_DETAIL_COMMAND_BYTES);
    for command in commands {
        for value in [
            command.evaluator, command.source_route, command.seed, command.edge_mask,
            command.dst_x, command.dst_y, command.dst_width, command.dst_height,
            command.semantic_x, command.semantic_y, command.semantic_width, command.semantic_height,
            command.declared_halo_px, command.cap_major_axis,
            command.source_stencil_halo_px, command.exposed_metal_enabled,
        ] { bytes.extend_from_slice(&value.to_le_bytes()); }
        for value in [
            command.slot_width_m, command.slot_height_m, command.meters_per_pixel_x,
            command.meters_per_pixel_y, command.wear_amount, command.intensity,
            command.edge_width_m, command.bevel_radius_m, command.edge_softness,
            command.breakup_amount, command.breakup_scale_m, command.micro_detail_amount,
            command.micro_detail_scale_m, command.source_height_influence,
            command.source_luminance_influence, command.height_amplitude_m,
            command.normal_detail_strength, command.source_height_range_m,
            command.requested_extent_m, command.hue_shift_degrees, command.saturation_multiplier,
            command.value_multiplier, command.roughness_offset, command.metallic_offset,
        ] { bytes.extend_from_slice(&value.to_le_bytes()); }
    }
    debug_assert_eq!(bytes.len(), commands.len() * GPU_EDGE_DETAIL_COMMAND_BYTES);
    bytes
}

fn execute_or_load_edge_detail_fields(
    executor: &GpuAtlasRenderExecutor<'_>,
    plan: &CompiledAtlasPlanV1,
    input: Option<&AtlasRenderExecutionInput<'_>>,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    cancellation: &CancellationToken,
    is_current: &dyn Fn() -> bool,
) -> Result<GpuEdgeDetailDispatchStats, AtlasRenderExecutionError> {
    if cancellation.is_cancelled() { return Err(AtlasRenderExecutionError::Cancelled); }
    if !is_current() { return Err(AtlasRenderExecutionError::Superseded); }
    let commands = plan.ordered_regions.iter().filter_map(pack_edge_detail_command).collect::<Vec<_>>();
    let required_halo_px = commands
        .iter()
        .map(|command| command.declared_halo_px.max(command.source_stencil_halo_px))
        .max()
        .unwrap_or(0);
    if commands.is_empty() {
        return Ok(GpuEdgeDetailDispatchStats { required_halo_px, ..Default::default() });
    }
    let tile = plan.tile_request.output_rect.0;
    let is_bounded_tile = tile.x != 0
        || tile.y != 0
        || tile.width != plan.output_size.width
        || tile.height != plan.output_size.height;
    if is_bounded_tile && plan.tile_request.halo_px < required_halo_px {
        return Err(AtlasRenderExecutionError::InvalidInput(format!(
            "Edge Detail requires tile halo {required_halo_px}, but the request declares {}",
            plan.tile_request.halo_px
        )));
    }
    let bytes_per_field = u64::from(tile.width).saturating_mul(u64::from(tile.height)).saturating_mul(4);
    let resident_bytes = bytes_per_field.saturating_mul(7);
    if resident_bytes > GpuAtlasSourceTextureCache::budgets().gpu_output_intermediate_residency_bytes {
        return Err(AtlasRenderExecutionError::Gpu(format!(
            "Edge Detail fields require {resident_bytes} bytes, exceeding the bounded GPU tile residency budget"
        )));
    }
    let identities = EDGE_DETAIL_FIELD_IDENTITIES.map(|(map, shader)| edge_detail_identity(plan, map, shader));
    let cached_count = executor.source_texture_cache.lock()
        .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?
        .rendered_textures.iter().filter(|texture| identities.iter().any(|identity| {
            texture.pixel_identity == identity.pixel_identity()
                && texture.format == wgpu::TextureFormat::R32Float
                && texture.width == tile.width && texture.height == tile.height
        })).count();
    if cached_count == identities.len() {
        return Ok(GpuEdgeDetailDispatchStats {
            cache_hit: true, resident_bytes: bytes_per_field * 5,
            required_halo_px, ..Default::default()
        });
    }
    execute_or_load_profile_fields(executor, plan, device, queue, cancellation, is_current)?;
    let sdf_identity = plan.tile_identity(MaterialMapKind::Roughness, "stage15-profile-sdf-v2");
    let semantics_identity = plan.tile_identity(MaterialMapKind::AmbientOcclusion, "stage15-profile-semantics-v2");
    let (sdf, semantics) = {
        let mut cache = executor.source_texture_cache.lock()
            .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?;
        (
            cache.cached_rendered_texture(&sdf_identity)
                .ok_or_else(|| AtlasRenderExecutionError::Gpu("Stage 15 SDF field is missing".into()))?,
            cache.cached_rendered_texture(&semantics_identity)
                .ok_or_else(|| AtlasRenderExecutionError::Gpu("Stage 15 semantics field is missing".into()))?,
        )
    };
    let started = Instant::now();
    let (height_source_texture, height_source_view) = create_working_texture(
        device, "hot-trimmer-edge-source-height", tile.width, tile.height,
        wgpu::TextureFormat::R32Float,
        wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let (color_source_texture, color_source_view) = create_working_texture(
        device, "hot-trimmer-edge-source-color", tile.width, tile.height,
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let fill = pipeline(device, executor.source_texture_cache, GpuAtlasPipelineKind::FillR32Float)?.0;
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("hot-trimmer-edge-source-clear"),
    });
    dispatch_fill_r32float_with_pipeline(
        device, &mut encoder, None, &fill, &height_source_view, tile.width, tile.height,
    )?;
    encoder.clear_texture(&color_source_texture, &wgpu::ImageSubresourceRange::default());
    submit_encoder_and_wait(device, queue, encoder)?;
    let mut source_cache_hits = 0;
    let mut source_upload_bytes = 0;
    if let Some(input) = input {
        let mut modulation_plan = plan.clone();
        for region in &mut modulation_plan.ordered_regions { region.edge_wear = None; }
        if commands.iter().any(|command| command.source_route == 1) {
            let material = pipeline(device, executor.source_texture_cache, GpuAtlasPipelineKind::MaterialR32Float)?.0;
            let stats = dispatch_material_map_to_view(
                device, queue, executor.source_texture_cache, "edge-detail-source-height",
                &material, &modulation_plan, input, MaterialMapKind::Height, &height_source_view,
                device.limits().max_texture_dimension_2d, cancellation, is_current,
            )?;
            source_cache_hits += stats.source_cache_hits;
            source_upload_bytes += stats.upload_bytes;
        }
        if commands.iter().any(|command| command.source_route == 2) {
            let material = pipeline(device, executor.source_texture_cache, GpuAtlasPipelineKind::MaterialRgba8)?.0;
            let stats = dispatch_material_map_to_view(
                device, queue, executor.source_texture_cache, "edge-detail-source-linear-color",
                &material, &modulation_plan, input, MaterialMapKind::BaseColor, &color_source_view,
                device.limits().max_texture_dimension_2d, cancellation, is_current,
            )?;
            source_cache_hits += stats.source_cache_hits;
            source_upload_bytes += stats.upload_bytes;
        }
    }
    let (edge_pipeline, _) = pipeline(device, executor.source_texture_cache, GpuAtlasPipelineKind::EdgeDetail)?;
    let header = GpuProfileHeader {
        output_width: plan.output_size.width, output_height: plan.output_size.height,
        tile_x: tile.x, tile_y: tile.y, tile_width: tile.width, tile_height: tile.height,
        command_count: commands.len() as u32, _pad: plan.tile_request.halo_px,
    };
    let header_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("hot-trimmer-edge-detail-header"),
        contents: &encode_profile_header(header), usage: wgpu::BufferUsages::UNIFORM,
    });
    let command_bytes = encode_edge_detail_commands(&commands);
    let command_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("hot-trimmer-edge-detail-commands"),
        contents: &command_bytes, usage: wgpu::BufferUsages::STORAGE,
    });
    let mut fields = Vec::with_capacity(5);
    for label in ["core", "transition", "fade", "combined", "height"] {
        fields.push(create_working_texture(
            device,
            match label {
                "core" => "hot-trimmer-edge-core",
                "transition" => "hot-trimmer-edge-transition",
                "fade" => "hot-trimmer-edge-fade",
                "combined" => "hot-trimmer-edge-combined",
                _ => "hot-trimmer-edge-height",
            },
            tile.width, tile.height, wgpu::TextureFormat::R32Float,
            wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        ));
    }
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("hot-trimmer-edge-detail-bind-group"),
        layout: &edge_pipeline.bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: header_buffer.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: command_buffer.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&sdf.view) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&semantics.view) },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&height_source_view) },
            wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&color_source_view) },
            wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&fields[0].1) },
            wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(&fields[1].1) },
            wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::TextureView(&fields[2].1) },
            wgpu::BindGroupEntry { binding: 9, resource: wgpu::BindingResource::TextureView(&fields[3].1) },
            wgpu::BindGroupEntry { binding: 10, resource: wgpu::BindingResource::TextureView(&fields[4].1) },
        ],
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("hot-trimmer-edge-detail-encoder"),
    });
    for (texture, _) in &fields {
        encoder.clear_texture(texture, &wgpu::ImageSubresourceRange::default());
    }
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("hot-trimmer-edge-detail-dispatch"), timestamp_writes: None,
        });
        pass.set_pipeline(&edge_pipeline.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(tile.width.div_ceil(16), tile.height.div_ceil(16), 1);
    }
    submit_encoder_and_wait(device, queue, encoder)?;
    drop(height_source_texture);
    drop(color_source_texture);
    let mut cache = executor.source_texture_cache.lock()
        .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?;
    for ((texture, view), identity) in fields.into_iter().zip(identities) {
        cache.remember_rendered_texture(
            identity, texture, view, tile.width, tile.height,
            wgpu::TextureFormat::R32Float, bytes_per_field,
        );
    }
    Ok(GpuEdgeDetailDispatchStats {
        cache_hit: false, dispatches: 1, command_count: commands.len() as u32,
        command_bytes: command_bytes.len() as u64, resident_bytes: bytes_per_field * 5,
        source_cache_hits, source_upload_bytes, required_halo_px,
        dispatch_ms: started.elapsed().as_millis(),
    })
}

/// Publishes all ED-3 channel responses from the same cached ED-2 field set.
/// Returning `None` is the disabled route: it dispatches no Edge Detail work
/// and leaves the base publication byte-identical.
fn compose_edge_detail_map(
    executor: &GpuAtlasRenderExecutor<'_>,
    plan: &CompiledAtlasPlanV1,
    input: &AtlasRenderExecutionInput<'_>,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    requested_map: MaterialMapKind,
    base_view: &wgpu::TextureView,
    output_format: wgpu::TextureFormat,
    base_height_is_physical: bool,
    cancellation: &CancellationToken,
    is_current: &dyn Fn() -> bool,
) -> Result<Option<(wgpu::Texture, wgpu::TextureView, GpuEdgeDetailDispatchStats)>, AtlasRenderExecutionError> {
    let commands = plan.ordered_regions.iter().filter_map(pack_edge_detail_command).collect::<Vec<_>>();
    if commands.is_empty() {
        return Ok(None);
    }
    let stats = execute_or_load_edge_detail_fields(
        executor, plan, Some(input), device, queue, cancellation, is_current,
    )?;
    if matches!(requested_map, MaterialMapKind::Height | MaterialMapKind::Normal)
        && !base_height_is_physical
    {
        execute_or_load_detail_fields(
            executor, plan, Some(input), Some(MaterialMapKind::Height), device, queue,
            cancellation, is_current,
        )?;
    }
    let tile = plan.tile_request.output_rect.0;
    let stage15_identity = plan.tile_identity(MaterialMapKind::Height, "stage15-profile-height-v1");
    let mut stage16_identity = plan.tile_identity(MaterialMapKind::Height, "stage16-detail-height-v1");
    stage16_identity.pixel_format = crate::CompiledTilePixelFormat::R32Float;
    let edge_identities = EDGE_DETAIL_FIELD_IDENTITIES.map(|(map, shader)| edge_detail_identity(plan, map, shader));
    let (stage15, stage16, edge_fields) = {
        let mut cache = executor.source_texture_cache.lock()
            .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?;
        let stage15 = cache.cached_rendered_texture(&stage15_identity)
            .ok_or_else(|| AtlasRenderExecutionError::Gpu("Stage 15 physical Height is missing".into()))?;
        let stage16 = cache.cached_rendered_texture(&stage16_identity);
        let mut edge_fields = Vec::with_capacity(5);
        for identity in &edge_identities {
            edge_fields.push(cache.cached_rendered_texture(identity)
                .ok_or_else(|| AtlasRenderExecutionError::Gpu("authoritative Edge Detail field is missing".into()))?);
        }
        (stage15, stage16, edge_fields)
    };
    let (zero_texture, zero_view) = create_working_texture(
        device, "hot-trimmer-zero-stage16-height", tile.width, tile.height,
        wgpu::TextureFormat::R32Float,
        wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    );
    if stage16.is_none() {
        let mut clear = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("hot-trimmer-zero-stage16-height") });
        clear.clear_texture(&zero_texture, &wgpu::ImageSubresourceRange::default());
        submit_encoder_and_wait(device, queue, clear)?;
    }
    let kind = if output_format == wgpu::TextureFormat::Rgba8Unorm {
        GpuAtlasPipelineKind::EdgeDetailCompositionRgba8
    } else {
        GpuAtlasPipelineKind::EdgeDetailCompositionR32Float
    };
    let composition = pipeline(device, executor.source_texture_cache, kind)?.0;
    let (output_texture, output_view) = create_working_texture(
        device, "hot-trimmer-edge-detail-composed-map", tile.width, tile.height,
        output_format,
        wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
    );
    let mut header_bytes = Vec::with_capacity(32);
    for value in [
        tile.width, tile.height, gpu_map_code(requested_map), commands.len() as u32,
        tile.x, tile.y, u32::from(base_height_is_physical), 0,
    ] {
        header_bytes.extend_from_slice(&value.to_le_bytes());
    }
    let header_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("hot-trimmer-edge-detail-composition-header"), contents: &header_bytes,
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let command_bytes = encode_edge_detail_commands(&commands);
    let command_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("hot-trimmer-edge-detail-composition-commands"), contents: &command_bytes,
        usage: wgpu::BufferUsages::STORAGE,
    });
    let stage16_view = stage16.as_ref().map_or(&zero_view, |field| &field.view);
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("hot-trimmer-edge-detail-composition-bind-group"),
        layout: &composition.bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: header_buffer.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: command_buffer.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(base_view) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&stage15.view) },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(stage16_view) },
            wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&edge_fields[0].view) },
            wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&edge_fields[1].view) },
            wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(&edge_fields[2].view) },
            wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::TextureView(&edge_fields[3].view) },
            wgpu::BindGroupEntry { binding: 9, resource: wgpu::BindingResource::TextureView(&edge_fields[4].view) },
            wgpu::BindGroupEntry { binding: 10, resource: wgpu::BindingResource::TextureView(&output_view) },
        ],
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("hot-trimmer-edge-detail-composition") });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("hot-trimmer-edge-detail-composition"), timestamp_writes: None });
        pass.set_pipeline(&composition.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(tile.width.div_ceil(16), tile.height.div_ceil(16), 1);
    }
    submit_encoder_and_wait(device, queue, encoder)?;
    Ok(Some((output_texture, output_view, stats)))
}

fn scalar_display_from_composed_map(
    executor: &GpuAtlasRenderExecutor<'_>,
    plan: &CompiledAtlasPlanV1,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    requested_map: MaterialMapKind,
    source_view: &wgpu::TextureView,
    width: u32,
    height: u32,
) -> Result<(wgpu::Texture, wgpu::TextureView), AtlasRenderExecutionError> {
    let pipeline = pipeline(
        device, executor.source_texture_cache, GpuAtlasPipelineKind::ScalarDisplayRgba8,
    )?.0;
    let (texture, view) = create_working_texture(
        device, "hot-trimmer-composed-scalar-display", width, height,
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
    );
    let allocation_identity = plan.tile_identity(
        MaterialMapKind::AmbientOcclusion, "stage15-profile-semantics-v2",
    );
    let allocation = executor.source_texture_cache.lock()
        .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?
        .cached_rendered_texture(&allocation_identity)
        .ok_or_else(|| AtlasRenderExecutionError::Gpu("Stage 15 allocation semantics are missing".into()))?;
    let mut header = Vec::with_capacity(32);
    for value in [width, height, gpu_map_code(requested_map), 0] {
        header.extend_from_slice(&value.to_le_bytes());
    }
    for value in [SOURCE_HEIGHT_RANGE_M, 0.0, 0.0, 0.0] {
        header.extend_from_slice(&value.to_le_bytes());
    }
    let header = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("hot-trimmer-composed-scalar-display-header"),
        contents: &header,
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("hot-trimmer-composed-scalar-display-bind-group"),
        layout: &pipeline.bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: header.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(source_view) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&allocation.view) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&view) },
        ],
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("hot-trimmer-composed-scalar-display"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("hot-trimmer-composed-scalar-display"), timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(width.div_ceil(16), height.div_ceil(16), 1);
    }
    submit_encoder_and_wait(device, queue, encoder)?;
    Ok((texture, view))
}


fn execute_or_load_detail_fields(
    executor: &GpuAtlasRenderExecutor<'_>,
    plan: &CompiledAtlasPlanV1,
    input: Option<&AtlasRenderExecutionInput<'_>>,
    requested_map: Option<MaterialMapKind>,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    cancellation: &CancellationToken,
    is_current: &dyn Fn() -> bool,
) -> Result<GpuDetailDispatchStats, AtlasRenderExecutionError> {
    if cancellation.is_cancelled() {
        return Err(AtlasRenderExecutionError::Cancelled);
    }
    if !is_current() {
        return Err(AtlasRenderExecutionError::Superseded);
    }
    execute_or_load_profile_fields(executor, plan, device, queue, cancellation, is_current)?;
    let command_count = detail_gpu_command_count(plan, requested_map);
    let required_halo_px = plan
        .ordered_regions
        .iter()
        .flat_map(|region| &region.compiled_details.details)
        .map(|detail| detail.required_halo_px)
        .max()
        .unwrap_or(0);
    if command_count == 0 {
        return Ok(GpuDetailDispatchStats {
            required_halo_px,
            ..Default::default()
        });
    }
    let tile = plan.tile_request.output_rect.0;
    let bytes_per_field = u64::from(tile.width)
        .saturating_mul(u64::from(tile.height))
        .saturating_mul(4);
    let requested_field = detail_requested_field(requested_map);
    let field_resident_bytes = if requested_field == DETAIL_REQUEST_ALL_FIELDS {
        bytes_per_field.saturating_mul(7)
    } else {
        if requested_field == DETAIL_FIELD_MATERIAL_ID as u32 {
            bytes_per_field.saturating_mul(2).saturating_add(5 * 4)
        } else {
            bytes_per_field.saturating_add(6 * 4)
        }
    };
    if field_resident_bytes
        > GpuAtlasSourceTextureCache::budgets().gpu_output_intermediate_residency_bytes
    {
        return Err(AtlasRenderExecutionError::Gpu(format!(
            "Stage 16 detail fields require {field_resident_bytes} bytes, exceeding the bounded GPU tile residency budget"
        )));
    }
    let fields_to_cache = detail_fields_for_request(plan, requested_map);
    if requested_map.is_some() && fields_to_cache.is_empty() {
        return Err(AtlasRenderExecutionError::InvalidInput(format!(
            "Stage 16 has no executable detail field for requested map {:?}",
            requested_map.expect("checked")
        )));
    }
    let field_indices = fields_to_cache
        .iter()
        .map(|field| field.field_index)
        .collect::<BTreeSet<_>>();
    let cached_count = executor
        .source_texture_cache
        .lock()
        .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?
        .rendered_textures
        .iter()
        .filter(|texture| {
            fields_to_cache.iter().any(|field| {
                texture.pixel_identity == field.identity.pixel_identity()
                    && texture.format == field.format
                    && texture.width == tile.width
                    && texture.height == tile.height
            })
        })
        .count();
    if cached_count == field_indices.len() {
        return Ok(GpuDetailDispatchStats {
            cache_hit: true,
            resident_bytes: bytes_per_field.saturating_mul(field_indices.len() as u64),
            required_halo_px,
            ..Default::default()
        });
    }
    let started = Instant::now();
    let (detail_pipeline, _) = pipeline(
        device,
        executor.source_texture_cache,
        GpuAtlasPipelineKind::SemanticDetail,
    )?;
    let mut commands = Vec::with_capacity(command_count);
    for region in &plan.ordered_regions {
        for detail in &region.compiled_details.details {
            if !detail_contributes_to_requested_map(detail, requested_map) {
                continue;
            }
            if detail_emits_procedural_default(detail) {
                commands.push(pack_detail_command(
                    region,
                    detail,
                    None,
                    None,
                    requested_map,
                ));
            }
            for operation in &detail.reusable_atlas_operations {
                commands.push(pack_detail_command(
                    region,
                    detail,
                    Some(operation),
                    None,
                    requested_map,
                ));
            }
            for stroke in &detail.strokes {
                if stroke.operation.scope
                    != hot_trimmer_effect_compiler::StampScope::MaterialReusableAtlas
                {
                    continue;
                }
                for sample in &stroke.physical_samples_m {
                    commands.push(pack_detail_command(
                        region,
                        detail,
                        Some(&stroke.operation),
                        Some(*sample),
                        requested_map,
                    ));
                }
            }
        }
    }
    commands.sort_by_key(|command| command.layer_order);
    let asset_textures = detail_asset_textures(
        device,
        queue,
        executor.source_texture_cache,
        input,
        plan,
        &mut commands,
        requested_map,
    )?;
    let occupancy_identity = plan.tile_identity(
        MaterialMapKind::AmbientOcclusion,
        "stage15-profile-semantics-v2",
    );
    let occupancy_texture = executor
        .source_texture_cache
        .lock()
        .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?
        .cached_rendered_texture(&occupancy_identity)
        .ok_or_else(|| {
            AtlasRenderExecutionError::Gpu("Stage 15 occupancy field is missing".into())
        })?;
    let header = GpuProfileHeader {
        output_width: plan.output_size.width,
        output_height: plan.output_size.height,
        tile_x: tile.x,
        tile_y: tile.y,
        tile_width: tile.width,
        tile_height: tile.height,
        command_count: commands.len() as u32,
        _pad: requested_field,
    };
    let header_bytes = encode_profile_header(header);
    let command_bytes = encode_detail_commands(&commands);
    let header_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("hot-trimmer-detail-header"),
        contents: &header_bytes,
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let command_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("hot-trimmer-detail-commands"),
        contents: &command_bytes,
        usage: wgpu::BufferUsages::STORAGE,
    });
    let mut fields = Vec::with_capacity(7);
    let mut allocated_resident_bytes = 0_u64;
    for (index, label, format) in [
        (DETAIL_FIELD_MASK, "mask", wgpu::TextureFormat::R32Float),
        (DETAIL_FIELD_HEIGHT, "height", wgpu::TextureFormat::R32Float),
        (
            DETAIL_FIELD_NORMAL_INPUT,
            "normal-input",
            // The semantic-detail shader publishes an RGBA tangent-space
            // vector (including the authored alpha/validity component) at
            // binding 4. Keep the working texture format in lock-step with
            // that WGSL storage binding; an R32Float view is rejected by
            // wgpu when the bind group is created.
            wgpu::TextureFormat::Rgba8Unorm,
        ),
        (DETAIL_FIELD_SCALAR, "scalar", wgpu::TextureFormat::R32Float),
        (DETAIL_FIELD_COLOR, "color", wgpu::TextureFormat::Rgba8Unorm),
        (
            DETAIL_FIELD_MATERIAL_ID,
            "material-id",
            wgpu::TextureFormat::R32Uint,
        ),
        (
            DETAIL_FIELD_MATERIAL_ID_VALID,
            "material-id-valid",
            wgpu::TextureFormat::R32Uint,
        ),
    ] {
        let is_requested = requested_field == DETAIL_REQUEST_ALL_FIELDS
            || requested_field == index as u32
            || (requested_field == DETAIL_FIELD_MATERIAL_ID as u32
                && index == DETAIL_FIELD_MATERIAL_ID_VALID);
        let field_width = if is_requested { tile.width } else { 1 };
        let field_height = if is_requested { tile.height } else { 1 };
        allocated_resident_bytes = allocated_resident_bytes.saturating_add(
            u64::from(field_width)
                .saturating_mul(u64::from(field_height))
                .saturating_mul(4),
        );
        fields.push(create_working_texture(
            device,
            match label {
                "mask" => "hot-trimmer-detail-mask",
                "height" => "hot-trimmer-detail-height",
                "normal-input" => "hot-trimmer-detail-normal",
                "scalar" => "hot-trimmer-detail-scalar",
                "color" => "hot-trimmer-detail-color",
                "material-id" => "hot-trimmer-detail-material-id",
                _ => "hot-trimmer-detail-material-id-valid",
            },
            field_width,
            field_height,
            format,
            wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        ));
    }
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("hot-trimmer-detail-bind-group"),
        layout: &detail_pipeline.bind_group_layout,
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
                resource: wgpu::BindingResource::TextureView(&fields[0].1),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&fields[1].1),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&fields[2].1),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::TextureView(&fields[3].1),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(&fields[4].1),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::TextureView(&fields[5].1),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: wgpu::BindingResource::TextureView(&fields[6].1),
            },
            wgpu::BindGroupEntry {
                binding: 9,
                resource: wgpu::BindingResource::TextureView(asset_textures.color.view()),
            },
            wgpu::BindGroupEntry {
                binding: 10,
                resource: wgpu::BindingResource::TextureView(asset_textures.scalar.view()),
            },
            wgpu::BindGroupEntry {
                binding: 11,
                resource: wgpu::BindingResource::TextureView(asset_textures.normal.view()),
            },
            wgpu::BindGroupEntry {
                binding: 12,
                resource: wgpu::BindingResource::TextureView(asset_textures.material_id.view()),
            },
            wgpu::BindGroupEntry {
                binding: 13,
                resource: wgpu::BindingResource::TextureView(asset_textures.mask.view()),
            },
            wgpu::BindGroupEntry {
                binding: 14,
                resource: wgpu::BindingResource::TextureView(&occupancy_texture.view),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("hot-trimmer-detail-encoder"),
    });
    for (texture, _) in &fields {
        encoder.clear_texture(texture, &wgpu::ImageSubresourceRange::default());
    }
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("hot-trimmer-detail-dispatch"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&detail_pipeline.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(tile.width.div_ceil(16), tile.height.div_ceil(16), 1);
    }
    submit_encoder_and_wait(device, queue, encoder)?;
    if cancellation.is_cancelled() {
        return Err(AtlasRenderExecutionError::Cancelled);
    }
    if !is_current() {
        return Err(AtlasRenderExecutionError::Superseded);
    }
    let mut cache = executor
        .source_texture_cache
        .lock()
        .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?;
    let mut fields = fields.into_iter().map(Some).collect::<Vec<_>>();
    for field in fields_to_cache {
        if let Some((texture, view)) = fields.get_mut(field.field_index).and_then(Option::take) {
            cache.remember_rendered_texture(
                field.identity,
                texture,
                view,
                tile.width,
                tile.height,
                field.format,
                bytes_per_field,
            );
        }
    }
    Ok(GpuDetailDispatchStats {
        cache_hit: false,
        dispatches: 1,
        command_count: commands.len() as u32,
        command_bytes: command_bytes.len() as u64,
        resident_bytes: allocated_resident_bytes.saturating_add(asset_textures.resident_bytes),
        asset_resident_bytes: asset_textures.resident_bytes,
        asset_upload_bytes: asset_textures.upload_bytes,
        required_halo_px,
        dispatch_ms: started.elapsed().as_millis(),
    })
}

fn execute_edge_detail_gpu_tile(
    executor: &GpuAtlasRenderExecutor<'_>,
    plan: &CompiledAtlasPlanV1,
    input: &AtlasRenderExecutionInput<'_>,
    cancellation: &CancellationToken,
    is_current: &dyn Fn() -> bool,
) -> Result<AtlasRenderExecutorOutput, AtlasRenderExecutionError> {
    let started = Instant::now();
    let state = executor.service.initialize()
        .map_err(|error| AtlasRenderExecutionError::Gpu(error.to_string()))?;
    validate_tile_size(plan, state.capabilities())?;
    require_format(state.capabilities(), "R32Float", true, true)?;
    require_format(state.capabilities(), "Rgba8Unorm", true, true)?;
    let device = state.device();
    let queue = state.queue();
    let stats = execute_or_load_edge_detail_fields(
        executor, plan, Some(input), device, queue, cancellation, is_current,
    )?;
    let tile = plan.tile_request.output_rect.0;
    let mut cached_fields = Vec::with_capacity(5);
    {
        let mut cache = executor.source_texture_cache.lock()
            .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?;
        for (map, shader) in EDGE_DETAIL_FIELD_IDENTITIES {
            let identity = edge_detail_identity(plan, map, shader);
            cached_fields.push(cache.cached_rendered_texture(&identity).ok_or_else(|| {
                AtlasRenderExecutionError::Gpu(format!("Edge Detail output {shader} is missing"))
            })?);
        }
    }
    let (display_texture, _) = scalar_display_from_composed_map(
        executor,
        plan,
        device,
        queue,
        MaterialMapKind::EdgeMask,
        &cached_fields[3].view,
        tile.width,
        tile.height,
    )?;
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("hot-trimmer-edge-detail-readback"),
    });
    let mut pending = Vec::with_capacity(5);
    for field in &cached_fields {
        pending.push(schedule_readback(
            device, executor.source_texture_cache, &mut encoder, &field._texture,
            tile.width, tile.height, 4,
        )?);
    }
    let display_pending = schedule_readback(
        device,
        executor.source_texture_cache,
        &mut encoder,
        &display_texture,
        tile.width,
        tile.height,
        4,
    )?;
    queue.submit(Some(encoder.finish()));
    let mut payloads = Vec::with_capacity(5);
    let mut readback_ms = 0;
    let mut readback_bytes = 0;
    for readback in pending {
        let (pixels, elapsed) = finish_readback(device, readback)?;
        readback_ms += elapsed;
        readback_bytes += pixels.len() as u64;
        payloads.push(pixels);
    }
    let (display_pixels, elapsed) = finish_readback(device, display_pending)?;
    readback_ms += elapsed;
    readback_bytes += display_pixels.len() as u64;
    let combined_pixels = Arc::clone(&payloads[3]);
    let combined_tile = remember_rendered_tile(
        executor.source_texture_cache, plan, MaterialMapKind::EdgeMask, combined_pixels,
    )?;
    let mut intermediate_tiles = BTreeMap::new();
    for (index, label) in ["edge-detail.core", "edge-detail.transition", "edge-detail.fade", "edge-detail.combined", "edge-detail.height"].into_iter().enumerate() {
        let (map, shader) = EDGE_DETAIL_FIELD_IDENTITIES[index];
        let tile = remember_rendered_tile_with_identity(
            executor.source_texture_cache, plan, map,
            edge_detail_identity(plan, map, shader), Arc::clone(&payloads[index]),
        )?;
        intermediate_tiles.insert(label.into(), tile);
    }
    let mut map_tiles = BTreeMap::new();
    map_tiles.insert(MaterialMapKind::EdgeMask, Arc::clone(&combined_tile));
    let display_tile = remember_rendered_tile_with_identity(
        executor.source_texture_cache,
        plan,
        MaterialMapKind::EdgeMask,
        display_tile_identity(plan, MaterialMapKind::EdgeMask),
        Arc::clone(&display_pixels),
    )?;
    let mut display_tiles = BTreeMap::new();
    display_tiles.insert(MaterialMapKind::EdgeMask, Arc::clone(&display_tile));
    let smooth_values = combined_tile.pixels().chunks_exact(4).filter(|pixel| {
        let value = f32::from_le_bytes((*pixel).try_into().unwrap_or([0; 4]));
        value > 0.0 && value < 1.0
    }).count();
    let edge_regions = plan
        .ordered_regions
        .iter()
        .filter_map(|region| region.edge_detail.as_ref().map(|edge| (region, edge)))
        .collect::<Vec<_>>();
    let eligible_regions = plan
        .ordered_regions
        .iter()
        .filter(|region| {
            let edge = region.edge_eligibility;
            edge.left || edge.right || edge.top || edge.bottom
        })
        .count();
    let scope = if edge_regions.len() == eligible_regions {
        "global"
    } else {
        "region"
    };
    let region_ids = edge_regions
        .iter()
        .map(|(region, _)| format!("{:?}", region.region_id))
        .collect::<Vec<_>>()
        .join("|");
    let source_routes = edge_regions
        .iter()
        .map(|(_, edge)| format!("{:?}", edge.source_modulation_route))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join("|");
    let source_identities = edge_regions
        .iter()
        .filter_map(|(_, edge)| edge.source_modulation_identity.as_ref())
        .map(|identity| identity.0.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join("|");
    let lod_fallbacks = edge_regions
        .iter()
        .filter_map(|(region, edge)| edge.lod_fallback.as_ref().map(|fallback| (region, fallback)))
        .map(|(region, fallback)| format!(
            "{:?}:{}:mpp={:.9}:edge={:.9}->{:.9}:breakup={:.9}->{:.9}:micro={:.9}->{:.9}",
            region.region_id,
            fallback.policy,
            fallback.maximum_meters_per_pixel,
            fallback.authored_edge_width_m,
            fallback.effective_edge_width_m,
            fallback.authored_breakup_scale_m,
            fallback.effective_breakup_scale_m,
            fallback.authored_micro_detail_scale_m,
            fallback.effective_micro_detail_scale_m,
        ))
        .collect::<Vec<_>>()
        .join("|");
    let sdf_identity = plan.tile_identity(MaterialMapKind::Roughness, "stage15-profile-sdf-v2");
    let semantics_identity = plan.tile_identity(
        MaterialMapKind::AmbientOcclusion,
        "stage15-profile-semantics-v2",
    );
    let output_identities = EDGE_DETAIL_FIELD_IDENTITIES
        .iter()
        .map(|(map, shader)| {
            let identity = edge_detail_identity(plan, *map, shader);
            format!("{shader}:{:?}:{:?}", identity.pixel_format, identity.output_rect.0)
        })
        .collect::<Vec<_>>()
        .join("|");
    let telemetry = vec![
        format!(
            "executor=gpu; backend={}; plan_hash={}; requested_map=EdgeMask; logical_passes={}; executed_gpu_passes=stage15-profile,edge-detail-mvp-v2,scalar-display-rgba8; shader_identity=edge-detail-mvp-v2; mask_format=R32Float; display_format=Rgba8UnormLinear; height_format=R32Float; final_tile_cache=miss; edge_detail_cache={}; dispatches={}; command_count={}; command_bytes={}; resident_bytes={}; required_halo_px={}; source_cache_hits={}; source_upload_bytes={}; dispatch_ms={}; readback_ms={}; smooth_intermediate_pixels={smooth_values}; render_ms={}",
            state.capabilities().backend, plan.final_plan_hash.0,
            logical_passes_for_map(plan, MaterialMapKind::EdgeMask),
            if stats.cache_hit { "CacheHit" } else { "CacheMiss" },
            stats.dispatches, stats.command_count, stats.command_bytes,
            stats.resident_bytes, stats.required_halo_px, stats.source_cache_hits,
            stats.source_upload_bytes, stats.dispatch_ms, readback_ms,
            started.elapsed().as_millis(),
        ),
        format!(
            "requested_map_dependencies=EdgeMask<-Stage15Sdf|Stage15Semantics|EdgeDetailCombined; edge_detail_mask_identity={}; scope={scope}; region_ids={region_ids}; stage15_sdf_identity={}:{}; stage15_semantics_identity={}:{}; selected_source_routes={source_routes}; selected_source_identities={source_identities}; lod_fallbacks={lod_fallbacks}; output_identities={output_identities}; edge_detail_outputs=core|transition|fade|combined|signed-physical-height; declared_halo_px={}; resident_bytes={}; dispatch_ms={}; cache_result={}",
            edge_detail_plan_identity(plan),
            sdf_identity.structural_plan_id.0,
            sdf_identity.shader_version,
            semantics_identity.structural_plan_id.0,
            semantics_identity.shader_version,
            stats.required_halo_px,
            stats.resident_bytes,
            stats.dispatch_ms,
            if stats.cache_hit { "CacheHit" } else { "CacheMiss" },
        ),
    ];
    Ok(AtlasRenderExecutorOutput::FinalAtlas(AtlasFinalAtlasOutput {
        map_tiles: map_tiles.clone(),
        display_tiles,
        intermediate_tiles,
        base_color_rgba8: Arc::clone(&display_pixels),
        interactive_tile: display_tile,
        tile_timings: tile_timings_for(&map_tiles, stats.dispatch_ms, readback_ms),
        region_valid_pixel_counts: final_atlas_metadata(plan)?,
        render_ms: started.elapsed().as_millis(),
        source_cache_hits: stats.source_cache_hits,
        pipeline_cache_hits: u32::from(stats.cache_hit),
        upload_bytes: stats.source_upload_bytes,
        upload_ms: 0,
        command_count: stats.command_count,
        command_bytes: stats.command_bytes,
        dispatch_ms: stats.dispatch_ms,
        readback_bytes,
        readback_ms,
        telemetry,
    }))
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
        let (nontransparent, nonzero_rgb) = payload_counts(
            interactive_tile.pixels(),
            interactive_tile.manifest.pixel_format,
        );
        let mut map_tiles = BTreeMap::new();
        map_tiles.insert(requested_map, Arc::clone(&cached));
        let mut display_tiles = BTreeMap::new();
        display_tiles.insert(requested_map, Arc::clone(&interactive_tile));
        return Ok(AtlasRenderExecutorOutput::FinalAtlas(
            AtlasFinalAtlasOutput {
                map_tiles: map_tiles.clone(),
                display_tiles,
                intermediate_tiles: BTreeMap::new(),
                base_color_rgba8: interactive_tile.payload(),
                interactive_tile,
                tile_timings: tile_timings_for(&map_tiles, 0, 0),
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
                    logical_passes_for_map(plan, requested_map)
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
    let profile_stats =
        execute_or_load_profile_fields(executor, plan, device, queue, cancellation, is_current)?;
    let detail_stats = execute_or_load_detail_fields(
        executor,
        plan,
        Some(input),
        Some(requested_map),
        device,
        queue,
        cancellation,
        is_current,
    )?;
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
    let timing = GpuPassTimingRecorder::new(device, queue, state.capabilities(), 8);
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
            None,
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
    submit_encoder_and_wait(device, queue, encoder)?;
    let stats = dispatch_material_map_to_view(
        device,
        queue,
        executor.source_texture_cache,
        "material-r32float-publish",
        &material_pipeline,
        plan,
        input,
        requested_map,
        &output_view,
        state.capabilities().maximum_texture_dimension_2d,
        cancellation,
        is_current,
    )?;
    let display_stats = dispatch_material_map_to_view(
        device,
        queue,
        executor.source_texture_cache,
        "material-rgba8-display-publish",
        &display_pipeline,
        plan,
        input,
        requested_map,
        &display_view,
        state.capabilities().maximum_texture_dimension_2d,
        cancellation,
        is_current,
    )?;
    let edge_composition = compose_edge_detail_map(
        executor, plan, input, device, queue, requested_map, &output_view,
        wgpu::TextureFormat::R32Float, false, cancellation, is_current,
    )?;
    let edge_composition_executed = edge_composition.is_some();
    let edge_composition_ms = edge_composition.as_ref().map_or(0, |value| value.2.dispatch_ms);
    let published_texture = edge_composition.as_ref().map_or(&output_texture, |value| &value.0);
    let published_view = edge_composition.as_ref().map_or(&output_view, |value| &value.1);
    let composed_display = if edge_composition.is_some() {
        Some(scalar_display_from_composed_map(
            executor, plan, device, queue, requested_map, published_view, tile.width, tile.height,
        )?)
    } else {
        None
    };
    let published_display_texture = composed_display.as_ref().map_or(&display_texture, |value| &value.0);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("hot-trimmer-r32float-material-readback-encoder"),
    });
    let pending = schedule_readback(
        device,
        executor.source_texture_cache,
        &mut encoder,
        published_texture,
        tile.width,
        tile.height,
        4,
    )?;
    let display_pending = schedule_readback(
        device,
        executor.source_texture_cache,
        &mut encoder,
        published_display_texture,
        tile.width,
        tile.height,
        4,
    )?;
    queue.submit(Some(encoder.finish()));
    if requested_map == MaterialMapKind::Height {
        let (cache_texture, cache_view) = match edge_composition {
            Some((texture, view, _)) => (texture, view),
            None => (output_texture, output_view),
        };
        executor
            .source_texture_cache
            .lock()
            .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?
            .remember_rendered_texture(
                identity.clone(),
                cache_texture,
                cache_view,
                tile.width,
                tile.height,
                wgpu::TextureFormat::R32Float,
                u64::from(tile.width)
                    .saturating_mul(u64::from(tile.height))
                    .saturating_mul(4),
            );
    }
    let (pixels, readback_ms) = finish_readback(device, pending)?;
    let (display_pixels, display_readback_ms) = finish_readback(device, display_pending)?;
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
    let (source_resident_bytes, source_resident_layers) = executor
        .source_texture_cache
        .lock()
        .map(|cache| (cache.source_resident_bytes(), cache.source_layer_count()))
        .unwrap_or((0, 0));
    let checked_out_source_resident_bytes_peak = stats
        .checked_out_source_resident_bytes_peak
        .max(display_stats.checked_out_source_resident_bytes_peak);
    let checked_out_source_layers_peak = stats
        .checked_out_source_layers_peak
        .max(display_stats.checked_out_source_layers_peak);
    let executed_gpu_passes = if edge_composition_executed {
        "material-r32float-publish,edge-detail-composition-v2,material-rgba8-display-publish"
    } else {
        "material-r32float-publish,material-rgba8-display-publish"
    };
    let intermediate_cache = if edge_composition_executed { "authoritative-edge-detail" } else { "not-used" };
    let mut telemetry = vec![format!(
        "executor=gpu; backend={}; plan_hash={}; requested_map={requested_map:?}; logical_passes={}; executed_gpu_passes={executed_gpu_passes}; pixel_format=R32Float; display_pixel_format=Rgba8UnormLinear; final_tile_cache=miss; intermediate_cache={intermediate_cache}; source_cache_hits={}; source_resident_bytes={source_resident_bytes}; source_resident_layers={source_resident_layers}; checked_out_source_resident_bytes_peak={checked_out_source_resident_bytes_peak}; checked_out_source_layers_peak={checked_out_source_layers_peak}; pipeline_cache_hits={}; upload_bytes={}; upload_ms={}; command_count={}; command_bytes={}; pipeline_ms={pipeline_ms}; dispatch_ms={}; readback_bytes={}; readback_ms={}; tile_nontransparent={nontransparent}; tile_nonzero_rgb={nonzero_rgb}; composition_ms={edge_composition_ms}; render_ms={}",
        state.capabilities().backend,
        plan.final_plan_hash.0,
        logical_passes_for_map(plan, requested_map),
        stats
            .source_cache_hits
            .saturating_add(display_stats.source_cache_hits),
        u32::from(pipeline_cache_hit) + u32::from(display_pipeline_cache_hit),
        stats
            .upload_bytes
            .saturating_add(display_stats.upload_bytes),
        stats.upload_ms.saturating_add(display_stats.upload_ms),
        stats
            .command_count
            .saturating_add(display_stats.command_count),
        stats
            .command_bytes
            .saturating_add(display_stats.command_bytes),
        stats.dispatch_ms.saturating_add(display_stats.dispatch_ms),
        readback_bytes.saturating_add(display_readback_bytes),
        readback_ms.saturating_add(display_readback_ms),
        started.elapsed().as_millis()
    )];
    if !edge_detail_plan_identity(plan).is_empty() {
        let dependencies = match requested_map {
            MaterialMapKind::Height => "RegisteredSource|Stage15Height|Stage16DetailHeight|EdgeDetailHeight",
            MaterialMapKind::Roughness | MaterialMapKind::Metallic => "RegisteredSource|Stage15Sdf|Stage15Semantics|EdgeDetailCombined",
            _ => "RegisteredSource",
        };
        telemetry.push(format!("requested_map_dependencies={requested_map:?}<-{dependencies}; edge_detail_mask_identity={}", edge_detail_plan_identity(plan)));
    }
    telemetry.push(format!(
        "profile_pass_dispatches={}; profile_cache_hit={}; profile_commands={}; profile_command_bytes={}; profile_resident_bytes={}; profile_required_halo={}; profile_dispatch_ms={}",
        profile_stats.dispatches,
        profile_stats.cache_hit,
        profile_stats.command_count,
        profile_stats.command_bytes,
        profile_stats.resident_bytes,
        profile_stats.required_halo_px,
        profile_stats.dispatch_ms,
    ));
    telemetry.push(format!(
        "detail_pass_dispatches={}; detail_cache_hit={}; detail_commands={}; detail_command_bytes={}; detail_resident_bytes={}; detail_required_halo={}; detail_dispatch_ms={}",
        detail_stats.dispatches,
        detail_stats.cache_hit,
        detail_stats.command_count,
        detail_stats.command_bytes,
        detail_stats.resident_bytes,
        detail_stats.required_halo_px,
        detail_stats.dispatch_ms,
    ));
    if let Some(timing) = timing {
        telemetry.extend(timing.finish(device)?);
    }
    let mut map_tiles = BTreeMap::new();
    map_tiles.insert(requested_map, Arc::clone(&rendered_tile));
    let mut display_tiles = BTreeMap::new();
    display_tiles.insert(requested_map, Arc::clone(&display_tile));
    Ok(AtlasRenderExecutorOutput::FinalAtlas(
        AtlasFinalAtlasOutput {
            map_tiles: map_tiles.clone(),
            display_tiles,
            intermediate_tiles: BTreeMap::new(),
            base_color_rgba8: Arc::clone(&display_pixels),
            interactive_tile: display_tile,
            tile_timings: tile_timings_for(
                &map_tiles,
                stats.dispatch_ms.saturating_add(display_stats.dispatch_ms),
                readback_ms.saturating_add(display_readback_ms),
            ),
            region_valid_pixel_counts: final_atlas_metadata(plan)?,
            render_ms: started.elapsed().as_millis(),
            source_cache_hits: stats
                .source_cache_hits
                .saturating_add(display_stats.source_cache_hits),
            pipeline_cache_hits: u32::from(pipeline_cache_hit)
                + u32::from(display_pipeline_cache_hit),
            upload_bytes: stats
                .upload_bytes
                .saturating_add(display_stats.upload_bytes),
            upload_ms: stats.upload_ms.saturating_add(display_stats.upload_ms),
            command_count: stats
                .command_count
                .saturating_add(display_stats.command_count)
                .saturating_add(detail_stats.command_count),
            command_bytes: stats
                .command_bytes
                .saturating_add(display_stats.command_bytes)
                .saturating_add(detail_stats.command_bytes),
            dispatch_ms: stats
                .dispatch_ms
                .saturating_add(display_stats.dispatch_ms)
                .saturating_add(detail_stats.dispatch_ms),
            readback_bytes: readback_bytes.saturating_add(display_readback_bytes),
            readback_ms: readback_ms.saturating_add(display_readback_ms),
            telemetry,
        },
    ))
}

fn execute_detail_material_id_gpu_tile(
    executor: &GpuAtlasRenderExecutor<'_>,
    plan: &CompiledAtlasPlanV1,
    input: &AtlasRenderExecutionInput<'_>,
    cancellation: &CancellationToken,
    is_current: &dyn Fn() -> bool,
) -> Result<AtlasRenderExecutorOutput, AtlasRenderExecutionError> {
    let requested_map = MaterialMapKind::MaterialId;
    let identity = plan.tile_identity(requested_map, GPU_SHADER_VERSION);
    if let Some(cached) = executor
        .source_texture_cache
        .lock()
        .ok()
        .and_then(|mut cache| cache.cached_tile(&identity, plan.tile_request.generation))
    {
        let (nontransparent, nonzero_rgb) =
            payload_counts(cached.pixels(), cached.manifest.pixel_format);
        let mut map_tiles = BTreeMap::new();
        map_tiles.insert(requested_map, Arc::clone(&cached));
        let mut display_tiles = BTreeMap::new();
        display_tiles.insert(requested_map, Arc::clone(&cached));
        return Ok(AtlasRenderExecutorOutput::FinalAtlas(
            AtlasFinalAtlasOutput {
                map_tiles: map_tiles.clone(),
                display_tiles,
                intermediate_tiles: BTreeMap::new(),
                base_color_rgba8: cached.payload(),
                interactive_tile: cached,
                tile_timings: tile_timings_for(&map_tiles, 0, 0),
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
                    "executor=gpu; plan_hash={}; requested_map=MaterialId; logical_passes={}; executed_gpu_passes=none; final_tile_cache=hit; gpu_tile_cache=hit; dispatch_ms=0; readback_ms=0; tile_nontransparent={nontransparent}; tile_nonzero_rgb={nonzero_rgb}",
                    plan.final_plan_hash.0,
                    logical_passes_for_map(plan, requested_map)
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
    require_format(state.capabilities(), "R32Uint", false, true)?;
    require_format(state.capabilities(), "R32Uint", true, false)?;
    let device = state.device();
    let queue = state.queue();
    let detail_stats = execute_or_load_detail_fields(
        executor,
        plan,
        Some(input),
        Some(requested_map),
        device,
        queue,
        cancellation,
        is_current,
    )?;
    let mut detail_identity = plan.tile_identity(
        MaterialMapKind::MaterialId,
        "stage16-detail-material-id-r32uint-v2",
    );
    detail_identity.pixel_format = crate::CompiledTilePixelFormat::R32Uint;
    let cached_texture = executor
        .source_texture_cache
        .lock()
        .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?
        .cached_rendered_texture(&detail_identity)
        .ok_or_else(|| {
            AtlasRenderExecutionError::Gpu("Stage 16 Material ID detail field is missing".into())
        })?;
    let tile = plan.tile_request.output_rect.0;
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("hot-trimmer-detail-material-id-readback"),
    });
    let pending = schedule_readback(
        device,
        executor.source_texture_cache,
        &mut encoder,
        &cached_texture._texture,
        tile.width,
        tile.height,
        4,
    )?;
    queue.submit(Some(encoder.finish()));
    let (pixels, readback_ms) = finish_readback(device, pending)?;
    let readback_bytes = u64::try_from(pixels.len())
        .map_err(|_| AtlasRenderExecutionError::Gpu("readback payload overflow".into()))?;
    let rendered_tile = Arc::new(GpuAtlasRenderedTile {
        manifest: crate::CompiledAtlasTileManifest {
            identity,
            map: requested_map,
            mip_level: plan.tile_request.mip_level,
            output_rect: plan.tile_request.output_rect,
            valid_rect: plan.tile_request.valid_rect,
            halo_px: plan.tile_request.halo_px,
            generation: plan.tile_request.generation,
            pixel_format: crate::CompiledTilePixelFormat::R32Uint,
            width: tile.width,
            height: tile.height,
            row_stride: tile.width.saturating_mul(4),
            opaque_handle: String::new(),
        },
        pixels: Arc::clone(&pixels),
    });
    executor
        .source_texture_cache
        .lock()
        .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?
        .remember_tile(Arc::clone(&rendered_tile));
    let mut map_tiles = BTreeMap::new();
    map_tiles.insert(requested_map, Arc::clone(&rendered_tile));
    let mut display_tiles = BTreeMap::new();
    display_tiles.insert(requested_map, Arc::clone(&rendered_tile));
    let (nontransparent, nonzero_rgb) =
        payload_counts(rendered_tile.pixels(), rendered_tile.manifest.pixel_format);
    Ok(AtlasRenderExecutorOutput::FinalAtlas(
        AtlasFinalAtlasOutput {
            map_tiles: map_tiles.clone(),
            display_tiles,
            intermediate_tiles: BTreeMap::new(),
            base_color_rgba8: rendered_tile.payload(),
            interactive_tile: rendered_tile,
            tile_timings: tile_timings_for(&map_tiles, detail_stats.dispatch_ms, readback_ms),
            region_valid_pixel_counts: final_atlas_metadata(plan)?,
            render_ms: started.elapsed().as_millis(),
            source_cache_hits: 0,
            pipeline_cache_hits: 0,
            upload_bytes: 0,
            upload_ms: 0,
            command_count: detail_stats.command_count,
            command_bytes: detail_stats.command_bytes,
            dispatch_ms: detail_stats.dispatch_ms,
            readback_bytes,
            readback_ms,
            telemetry: vec![format!(
                "executor=gpu; backend={}; plan_hash={}; requested_map=MaterialId; logical_passes={}; executed_gpu_passes=semantic-detail-publish; pixel_format=R32Uint; final_tile_cache=miss; detail_cache_hit={}; detail_commands={}; detail_command_bytes={}; detail_resident_bytes={}; detail_asset_resident_bytes={}; detail_asset_upload_bytes={}; detail_dispatch_ms={}; readback_bytes={readback_bytes}; readback_ms={readback_ms}; tile_nontransparent={nontransparent}; tile_nonzero_rgb={nonzero_rgb}; render_ms={}",
                state.capabilities().backend,
                plan.final_plan_hash.0,
                logical_passes_for_map(plan, requested_map),
                detail_stats.cache_hit,
                detail_stats.command_count,
                detail_stats.command_bytes,
                detail_stats.resident_bytes,
                detail_stats.asset_resident_bytes,
                detail_stats.asset_upload_bytes,
                detail_stats.dispatch_ms,
                started.elapsed().as_millis()
            )],
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
        let (nontransparent, nonzero_rgb) = payload_counts(
            interactive_tile.pixels(),
            interactive_tile.manifest.pixel_format,
        );
        let mut map_tiles = BTreeMap::new();
        map_tiles.insert(requested_map, Arc::clone(&cached));
        let mut display_tiles = BTreeMap::new();
        display_tiles.insert(requested_map, Arc::clone(&interactive_tile));
        return Ok(AtlasRenderExecutorOutput::FinalAtlas(
            AtlasFinalAtlasOutput {
                map_tiles: map_tiles.clone(),
                display_tiles,
                intermediate_tiles: BTreeMap::new(),
                base_color_rgba8: interactive_tile.payload(),
                interactive_tile,
                tile_timings: tile_timings_for(&map_tiles, 0, 0),
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
                    logical_passes_for_map(plan, requested_map)
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
        source_origin_x: 0,
        source_origin_y: 0,
        map_kind: gpu_map_code(requested_map),
        normal_convention: 0,
        source_role: 0,
        source_page_width: 0,
        source_page_height: 0,
        source_page_interior_width: 0,
        source_page_interior_height: 0,
        source_page_count_x: 1,
        source_page_count_y: 1,
        source_page_halo: 0,
        source_page_mode: 0,
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
    let mut timing = GpuPassTimingRecorder::new(device, queue, state.capabilities(), 4);
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
    let (pixels, readback_ms) = finish_readback(device, pending)?;
    let (display_pixels, display_readback_ms) = finish_readback(device, display_pending)?;
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
        logical_passes_for_map(plan, requested_map),
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
            map_tiles: map_tiles.clone(),
            display_tiles,
            intermediate_tiles: BTreeMap::new(),
            base_color_rgba8: Arc::clone(&display_pixels),
            interactive_tile: display_tile,
            tile_timings: tile_timings_for(
                &map_tiles,
                dispatch_ms,
                readback_ms.saturating_add(display_readback_ms),
            ),
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
    let profile_stats =
        execute_or_load_profile_fields(executor, plan, device, queue, cancellation, is_current)?;
    let detail_stats = execute_or_load_detail_fields(
        executor,
        plan,
        Some(input),
        Some(MaterialMapKind::Normal),
        device,
        queue,
        cancellation,
        is_current,
    )?;
    let pipeline_started = Instant::now();
    let has_authored_normal = plan
        .ordered_sources
        .iter()
        .any(|source| source.channel_role == MaterialChannelRole::Normal);
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
        wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
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
    let mut timing = GpuPassTimingRecorder::new(device, queue, state.capabilities(), 8);
    if cached_height_texture.is_none() {
        dispatch_fill_r32float_with_pipeline(
            device,
            &mut encoder,
            None,
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
    submit_encoder_and_wait(device, queue, encoder)?;
    let height_stats = if cached_height_texture.is_some() {
        GpuMaterialDispatchStats::default()
    } else {
        dispatch_material_map_to_view(
            device,
            queue,
            executor.source_texture_cache,
            "height-r32float",
            &height_pipeline,
            plan,
            input,
            MaterialMapKind::Height,
            &height_view,
            state.capabilities().maximum_texture_dimension_2d,
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
            "height-rgba8-display",
            height_display_pipeline,
            plan,
            input,
            MaterialMapKind::Height,
            height_display_view,
            state.capabilities().maximum_texture_dimension_2d,
            cancellation,
            is_current,
        )?)
    } else {
        None
    };
    let authored_normal_stats =
        if let Some((authored_normal_pipeline, _)) = &authored_normal_pipeline {
            Some(dispatch_material_map_to_view(
                device,
                queue,
                executor.source_texture_cache,
                "authored-normal-sample",
                authored_normal_pipeline,
                plan,
                input,
                MaterialMapKind::Normal,
                &authored_normal_view,
                state.capabilities().maximum_texture_dimension_2d,
                cancellation,
                is_current,
            )?)
        } else {
            None
        };
    let height_composition = if cached_height_texture.is_none() {
        compose_edge_detail_map(
            executor, plan, input, device, queue, MaterialMapKind::Height, &height_view,
            wgpu::TextureFormat::R32Float, false, cancellation, is_current,
        )?
    } else {
        None
    };
    let normal_height_composition = if let Some(cached_height) = &cached_height_texture {
        compose_edge_detail_map(
            executor, plan, input, device, queue, MaterialMapKind::Normal,
            &cached_height.view, wgpu::TextureFormat::R32Float, true,
            cancellation, is_current,
        )?
    } else {
        compose_edge_detail_map(
            executor, plan, input, device, queue, MaterialMapKind::Normal, &height_view,
            wgpu::TextureFormat::R32Float, false, cancellation, is_current,
        )?
    };
    let height_composition_ms = height_composition.as_ref().map_or(0, |value| value.2.dispatch_ms)
        .saturating_add(normal_height_composition.as_ref().map_or(0, |value| value.2.dispatch_ms));
    let live_final_height_view = cached_height_texture.as_ref().map_or_else(
        || height_composition.as_ref().map_or(&height_view, |value| &value.1),
        |cached| &cached.view,
    );
    let live_normal_height_view = normal_height_composition.as_ref().map_or(live_final_height_view, |value| &value.1);
    let composed_height_display = if publish_height && height_composition.is_some() {
        Some(scalar_display_from_composed_map(
            executor, plan, device, queue, MaterialMapKind::Height, live_final_height_view,
            tile.width, tile.height,
        )?)
    } else {
        None
    };
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("hot-trimmer-height-normal-final-encoder"),
    });
    let (command_count, command_buffer_bytes_len, normal_dispatch_ms) = {
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
            source_origin_x: 0,
            source_origin_y: 0,
            map_kind: gpu_map_code(MaterialMapKind::Normal),
            normal_convention: match plan.normal_convention {
                crate::CompiledNormalConvention::OpenGl => 0,
                crate::CompiledNormalConvention::DirectX => 1,
            },
            source_role: gpu_channel_role_code(if authored_normal_stats.is_some() {
                MaterialChannelRole::Normal
            } else {
                MaterialChannelRole::Height
            }),
            source_page_width: 0,
            source_page_height: 0,
            source_page_interior_width: 0,
            source_page_interior_height: 0,
            source_page_count_x: 1,
            source_page_count_y: 1,
            source_page_halo: 0,
            source_page_mode: 0,
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
                    resource: wgpu::BindingResource::TextureView(live_normal_height_view),
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
        (
            commands.len() as u32,
            command_buffer_bytes.len() as u64,
            normal_dispatch_started.elapsed().as_millis(),
        )
    };
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
                height_composition.as_ref().map_or(&height_texture, |value| &value.0),
                tile.width,
                tile.height,
                4,
            )?)
        }
    } else {
        None
    };
    let height_display_pending = if cached_height_display.is_none() {
        let display_texture = composed_height_display.as_ref().map(|value| &value.0)
            .or_else(|| height_display.as_ref().map(|value| &value.0));
        if let Some(display_texture) = display_texture {
            Some(schedule_readback(
                device,
                executor.source_texture_cache,
                &mut encoder,
                display_texture,
                tile.width,
                tile.height,
                4,
            )?)
        } else {
            None
        }
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
        let (cache_texture, cache_view) = match height_composition {
            Some((texture, view, _)) => (texture, view),
            None => (height_texture, height_view),
        };
        executor
            .source_texture_cache
            .lock()
            .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?
            .remember_rendered_texture(
                plan.tile_identity(MaterialMapKind::Height, GPU_SHADER_VERSION),
                cache_texture,
                cache_view,
                tile.width,
                tile.height,
                wgpu::TextureFormat::R32Float,
                u64::from(tile.width)
                    .saturating_mul(u64::from(tile.height))
                    .saturating_mul(4),
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
            finish_readback(device, height_display_pending)?;
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
        let (height_pixels, height_readback_ms) = finish_readback(device, height_pending)?;
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
    let (normal_pixels, normal_readback_ms) = finish_readback(device, normal_pending)?;
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
    let (source_resident_bytes, source_resident_layers) = executor
        .source_texture_cache
        .lock()
        .map(|cache| (cache.source_resident_bytes(), cache.source_layer_count()))
        .unwrap_or((0, 0));
    let checked_out_source_resident_bytes_peak = height_stats
        .checked_out_source_resident_bytes_peak
        .max(
            height_display_stats
                .as_ref()
                .map_or(0, |stats| stats.checked_out_source_resident_bytes_peak),
        )
        .max(
            authored_normal_stats
                .as_ref()
                .map_or(0, |stats| stats.checked_out_source_resident_bytes_peak),
        );
    let checked_out_source_layers_peak = height_stats
        .checked_out_source_layers_peak
        .max(
            height_display_stats
                .as_ref()
                .map_or(0, |stats| stats.checked_out_source_layers_peak),
        )
        .max(
            authored_normal_stats
                .as_ref()
                .map_or(0, |stats| stats.checked_out_source_layers_peak),
        );
    let mut telemetry = vec![format!(
        "executor=gpu; backend={}; plan_hash={}; requested_map=Normal; logical_passes={}; executed_gpu_passes={}; dependency={}; intermediate_cache={}; normal_publish={}; source_cache_hits={}; source_resident_bytes={source_resident_bytes}; source_resident_layers={source_resident_layers}; checked_out_source_resident_bytes_peak={checked_out_source_resident_bytes_peak}; checked_out_source_layers_peak={checked_out_source_layers_peak}; pipeline_cache_hits={}; upload_bytes={}; upload_ms={}; command_count={}; command_bytes={}; pipeline_ms={pipeline_ms}; dispatch_ms={}; readback_bytes={readback_bytes}; readback_ms={readback_ms}; render_ms={}",
        state.capabilities().backend,
        plan.final_plan_hash.0,
        logical_passes_for_map(plan, MaterialMapKind::Normal),
        if has_authored_normal {
            if cached_height_texture.is_some() {
                "height-r32float-gpu-resource-cache,authored-normal-sample"
            } else {
                "height-r32float,authored-normal-sample"
            }
        } else if cached_height_texture.is_some() {
            "height-r32float-gpu-resource-cache,normal-from-final-height"
        } else {
            "height-r32float,normal-from-final-height"
        },
        if has_authored_normal {
            "Normal<-authored-Normal|HeightFallback"
        } else {
            "Normal<-Height"
        },
        if cached_height_texture.is_some() {
            "final-height:persistent-gpu-resource-hit"
        } else {
            "final-height:live-gpu-hit"
        },
        if has_authored_normal {
            "authored-normal-pass-through-with-height-fallback"
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
        height_stats
            .upload_bytes
            .saturating_add(
                height_display_stats
                    .as_ref()
                    .map_or(0, |stats| stats.upload_bytes),
            )
            .saturating_add(
                authored_normal_stats
                    .as_ref()
                    .map_or(0, |stats| stats.upload_bytes)
            ),
        height_stats
            .upload_ms
            .saturating_add(
                height_display_stats
                    .as_ref()
                    .map_or(0, |stats| stats.upload_ms),
            )
            .saturating_add(
                authored_normal_stats
                    .as_ref()
                    .map_or(0, |stats| stats.upload_ms)
            ),
        height_stats
            .command_count
            .saturating_add(command_count)
            .saturating_add(
                height_display_stats
                    .as_ref()
                    .map_or(0, |stats| stats.command_count),
            )
            .saturating_add(
                authored_normal_stats
                    .as_ref()
                    .map_or(0, |stats| stats.command_count)
            ),
        height_stats
            .command_bytes
            .saturating_add(command_buffer_bytes_len)
            .saturating_add(
                height_display_stats
                    .as_ref()
                    .map_or(0, |stats| stats.command_bytes),
            )
            .saturating_add(
                authored_normal_stats
                    .as_ref()
                    .map_or(0, |stats| stats.command_bytes)
            ),
        height_stats
            .dispatch_ms
            .saturating_add(normal_dispatch_ms)
            .saturating_add(
                height_display_stats
                    .as_ref()
                    .map_or(0, |stats| stats.dispatch_ms),
            )
            .saturating_add(
                authored_normal_stats
                    .as_ref()
                    .map_or(0, |stats| stats.dispatch_ms)
            ),
        started.elapsed().as_millis()
    )];
    telemetry.push(format!(
        "profile_pass_dispatches={}; profile_cache_hit={}; profile_commands={}; profile_command_bytes={}; profile_resident_bytes={}; profile_required_halo={}; profile_dispatch_ms={}",
        profile_stats.dispatches,
        profile_stats.cache_hit,
        profile_stats.command_count,
        profile_stats.command_bytes,
        profile_stats.resident_bytes,
        profile_stats.required_halo_px,
        profile_stats.dispatch_ms,
    ));
    telemetry.push(format!(
        "detail_pass_dispatches={}; detail_cache_hit={}; detail_commands={}; detail_command_bytes={}; detail_resident_bytes={}; detail_required_halo={}; detail_dispatch_ms={}",
        detail_stats.dispatches,
        detail_stats.cache_hit,
        detail_stats.command_count,
        detail_stats.command_bytes,
        detail_stats.resident_bytes,
        detail_stats.required_halo_px,
        detail_stats.dispatch_ms,
    ));
    if !edge_detail_plan_identity(plan).is_empty() {
        telemetry.push(format!("requested_map_dependencies=Normal<-FinalPhysicalHeight(MaterialHeight|Stage15Height|Stage16DetailHeight|EdgeDetailHeight)|ImportedTangentNormal; edge_detail_mask_identity={}; normal_composition=RNM; gradient=physical-Scharr; edge_detail_composition_ms={height_composition_ms}", edge_detail_plan_identity(plan)));
    }
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
        map_tiles: map_tiles.clone(),
        display_tiles,
        intermediate_tiles,
        base_color_rgba8: interactive_tile.payload(),
        interactive_tile,
        tile_timings: tile_timings_for(
            &map_tiles,
            height_stats
                .dispatch_ms
                .saturating_add(normal_dispatch_ms)
                .saturating_add(
                    height_display_stats
                        .as_ref()
                        .map_or(0, |stats| stats.dispatch_ms),
                )
                .saturating_add(
                    authored_normal_stats
                        .as_ref()
                        .map_or(0, |stats| stats.dispatch_ms),
                ),
            readback_ms,
        ),
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
        upload_bytes: height_stats
            .upload_bytes
            .saturating_add(
                height_display_stats
                    .as_ref()
                    .map_or(0, |stats| stats.upload_bytes),
            )
            .saturating_add(
                authored_normal_stats
                    .as_ref()
                    .map_or(0, |stats| stats.upload_bytes),
            ),
        upload_ms: height_stats
            .upload_ms
            .saturating_add(
                height_display_stats
                    .as_ref()
                    .map_or(0, |stats| stats.upload_ms),
            )
            .saturating_add(
                authored_normal_stats
                    .as_ref()
                    .map_or(0, |stats| stats.upload_ms),
            ),
        command_count: height_stats
            .command_count
            .saturating_add(command_count)
            .saturating_add(detail_stats.command_count)
            .saturating_add(
                height_display_stats
                    .as_ref()
                    .map_or(0, |stats| stats.command_count),
            )
            .saturating_add(
                authored_normal_stats
                    .as_ref()
                    .map_or(0, |stats| stats.command_count),
            ),
        command_bytes: height_stats
            .command_bytes
            .saturating_add(command_buffer_bytes_len)
            .saturating_add(detail_stats.command_bytes)
            .saturating_add(
                height_display_stats
                    .as_ref()
                    .map_or(0, |stats| stats.command_bytes),
            )
            .saturating_add(
                authored_normal_stats
                    .as_ref()
                    .map_or(0, |stats| stats.command_bytes),
            ),
        dispatch_ms: height_stats
            .dispatch_ms
            .saturating_add(normal_dispatch_ms)
            .saturating_add(detail_stats.dispatch_ms)
            .saturating_add(
                height_display_stats
                    .as_ref()
                    .map_or(0, |stats| stats.dispatch_ms),
            )
            .saturating_add(
                authored_normal_stats
                    .as_ref()
                    .map_or(0, |stats| stats.dispatch_ms),
            ),
        readback_bytes,
        readback_ms,
        telemetry,
    })
}

fn dispatch_material_map_to_view(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    cache: &Mutex<GpuAtlasSourceTextureCache>,
    pass_label: &'static str,
    pipeline: &GpuAtlasPipeline,
    plan: &CompiledAtlasPlanV1,
    input: &AtlasRenderExecutionInput<'_>,
    requested_map: MaterialMapKind,
    output_view: &wgpu::TextureView,
    max_texture_dimension_2d: u32,
    cancellation: &CancellationToken,
    is_current: &dyn Fn() -> bool,
) -> Result<GpuMaterialDispatchStats, AtlasRenderExecutionError> {
    let mut upload_bytes = 0_u64;
    let mut source_cache_hits = 0_u32;
    let mut upload_ms = 0_u128;
    let mut checked_out_source_resident_bytes_peak = 0_u64;
    let mut checked_out_source_layers_peak = 0_u32;
    let mut command_count = 0_u32;
    let mut command_bytes = 0_u64;
    let tile = plan.tile_request.output_rect.0;
    let dispatch_started = Instant::now();
    for source in &plan.ordered_sources {
        if cancellation.is_cancelled() {
            return Err(AtlasRenderExecutionError::Cancelled);
        }
        if !is_current() {
            return Err(AtlasRenderExecutionError::Superseded);
        }
        let Some(source_role) = source_channel_role_for_source(plan, source, requested_map) else {
            continue;
        };
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
        if commands.is_empty() {
            continue;
        }
        for group_plan in material_source_group_plans_for_tile(
            source,
            commands,
            source_role,
            tile,
            max_texture_dimension_2d,
            device.limits().max_texture_array_layers,
        )? {
            if cancellation.is_cancelled() {
                return Err(AtlasRenderExecutionError::Cancelled);
            }
            if !is_current() {
                return Err(AtlasRenderExecutionError::Superseded);
            }
            let group_upload_started = Instant::now();
            let group = load_material_source_group(
                device,
                queue,
                cache,
                prepared.domain.as_ref(),
                group_plan,
            )?;
            upload_ms = upload_ms.saturating_add(group_upload_started.elapsed().as_millis());
            if group.cache_hit {
                source_cache_hits = source_cache_hits.saturating_add(1);
            } else {
                upload_bytes = upload_bytes.saturating_add(group.cached.byte_len);
            }
            let (live_source_bytes, live_source_layers) = cache
                .lock()
                .map(|cache| (cache.source_resident_bytes(), cache.source_layer_count()))
                .unwrap_or((group.cached.byte_len, group.cached.layer_count));
            checked_out_source_resident_bytes_peak =
                checked_out_source_resident_bytes_peak.max(live_source_bytes);
            checked_out_source_layers_peak = checked_out_source_layers_peak.max(live_source_layers);
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some(pass_label),
            });
            encode_material_source_dispatch(
                device,
                &mut encoder,
                None,
                pass_label,
                pipeline,
                plan,
                requested_map,
                output_view,
                group.source,
                group.cached.as_ref(),
                &group.commands,
                group.source_role,
                group.source_layout,
                tile,
                &mut command_count,
                &mut command_bytes,
            )?;
            submit_encoder_and_wait(device, queue, encoder)?;
        }
    }
    Ok(GpuMaterialDispatchStats {
        source_cache_hits,
        upload_bytes,
        upload_ms,
        checked_out_source_resident_bytes_peak,
        checked_out_source_layers_peak,
        command_count,
        command_bytes,
        dispatch_ms: dispatch_started.elapsed().as_millis(),
    })
}

#[allow(clippy::too_many_arguments)]
fn encode_material_source_dispatch(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    timing: Option<&mut GpuPassTimingRecorder>,
    pass_label: &'static str,
    pipeline: &GpuAtlasPipeline,
    plan: &CompiledAtlasPlanV1,
    requested_map: MaterialMapKind,
    output_view: &wgpu::TextureView,
    source: &CompiledSourceCommandV1,
    cached: &GpuCachedSourceTexture,
    commands: &[GpuRegionCommand],
    source_role: MaterialChannelRole,
    source_layout: GpuSourcePageLayout,
    tile: PixelBounds,
    command_count: &mut u32,
    command_bytes: &mut u64,
) -> Result<(), AtlasRenderExecutionError> {
    *command_count = command_count.saturating_add(commands.len() as u32);
    let source_rect = source_layout.source_rect;
    let header = GpuAtlasHeader {
        output_width: plan.output_size.width,
        output_height: plan.output_size.height,
        tile_x: tile.x,
        tile_y: tile.y,
        tile_width: tile.width,
        tile_height: tile.height,
        command_count: commands.len() as u32,
        source_width: source_rect.width,
        source_height: source_rect.height,
        source_origin_x: source_rect.x,
        source_origin_y: source_rect.y,
        map_kind: gpu_map_code(requested_map),
        normal_convention: match plan.normal_convention {
            crate::CompiledNormalConvention::OpenGl => 0,
            crate::CompiledNormalConvention::DirectX => 1,
        },
        source_role: gpu_channel_role_code(source_role),
        source_page_width: source_layout.source_page_width,
        source_page_height: source_layout.source_page_height,
        source_page_interior_width: source_layout.source_page_interior_width,
        source_page_interior_height: source_layout.source_page_interior_height,
        source_page_count_x: source_layout.source_page_count_x,
        source_page_count_y: source_layout.source_page_count_y,
        source_page_halo: source_layout.source_page_halo,
        source_page_mode: source_layout.source_page_mode,
    };
    let command_buffer_bytes = encode_commands(commands);
    *command_bytes = command_bytes.saturating_add(command_buffer_bytes.len() as u64);
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
    let source_page_table_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("hot-trimmer-material-source-page-table"),
        contents: &encode_source_page_table(&source_layout),
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
            wgpu::BindGroupEntry {
                binding: 4,
                resource: source_page_table_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::TextureView(&cached.validity_view),
            },
        ],
    });
    let timestamp_writes = timing.and_then(|recorder| {
        recorder.timestamp_writes(format!("{pass_label}:{:?}", source.source_id))
    });
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("hot-trimmer-material-dispatch"),
        timestamp_writes,
    });
    pass.set_pipeline(&pipeline.pipeline);
    pass.set_bind_group(0, &bind_group, &[]);
    pass.dispatch_workgroups(tile.width.div_ceil(16), tile.height.div_ceil(16), 1);
    Ok(())
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
    let timestamp_writes = timing.and_then(|recorder| recorder.timestamp_writes("fill-r32float"));
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("hot-trimmer-fill-r32float-dispatch"),
        timestamp_writes,
    });
    pass.set_pipeline(&pipeline.pipeline);
    pass.set_bind_group(0, &bind_group, &[]);
    pass.dispatch_workgroups(width.div_ceil(16), height.div_ceil(16), 1);
    Ok(())
}

#[allow(dead_code)]
fn command_intersects_source_tile(command: GpuRegionCommand, source_tile: PixelRect) -> bool {
    let crop = PixelRect {
        x: command.crop_x,
        y: command.crop_y,
        width: command.crop_width,
        height: command.crop_height,
    };
    intersect_rect(crop, source_tile).is_some()
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

fn schedule_readback<'a>(
    device: &wgpu::Device,
    cache: &'a Mutex<GpuAtlasSourceTextureCache>,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
    bytes_per_pixel: u32,
) -> Result<PendingGpuReadback<'a>, AtlasRenderExecutionError> {
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
    let readback_staging = acquire_staging_lease(device, cache, readback_bytes)?;
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: readback_staging.buffer(),
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
        staging: readback_staging,
        output_row_bytes: usize::try_from(output_row_bytes)
            .map_err(|_| AtlasRenderExecutionError::Gpu("output row size overflow".into()))?,
        padded_row_bytes: usize::try_from(padded_bytes_per_row)
            .map_err(|_| AtlasRenderExecutionError::Gpu("padded row size overflow".into()))?,
        height,
    })
}

fn finish_readback(
    device: &wgpu::Device,
    pending: PendingGpuReadback<'_>,
) -> Result<(Arc<[u8]>, u128), AtlasRenderExecutionError> {
    let readback_started = Instant::now();
    let pixels = {
        let slice = pending.staging.buffer().slice(..);
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
        Ok::<Arc<[u8]>, AtlasRenderExecutionError>(Arc::from(pixels))
    }?;
    pending.staging.buffer().unmap();
    Ok((pixels, readback_started.elapsed().as_millis()))
}

fn submit_encoder_and_wait(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: wgpu::CommandEncoder,
) -> Result<(), AtlasRenderExecutionError> {
    queue.submit(Some(encoder.finish()));
    device
        .poll(wgpu::PollType::Wait)
        .map(|_| ())
        .map_err(|error| AtlasRenderExecutionError::Gpu(format!("device poll failed: {error:?}")))
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

fn tile_timings_for(
    map_tiles: &BTreeMap<MaterialMapKind, Arc<GpuAtlasRenderedTile>>,
    render_ms: u128,
    readback_ms: u128,
) -> BTreeMap<MaterialMapKind, GpuAtlasTileTiming> {
    map_tiles
        .keys()
        .map(|map| {
            (
                *map,
                GpuAtlasTileTiming {
                    render_ms,
                    readback_ms,
                },
            )
        })
        .collect()
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

#[derive(Clone, Debug)]
struct GpuResidentSourcePagePlan {
    source_rect: PixelRect,
    interior_width: u32,
    interior_height: u32,
    pages: Vec<GpuResidentSourcePage>,
    byte_len: u64,
}

impl GpuResidentSourcePagePlan {
    fn layer_count(&self) -> u32 {
        self.pages.len() as u32
    }
}

#[allow(clippy::too_many_arguments)]
fn material_source_group_plans_for_tile<'a>(
    source: &'a CompiledSourceCommandV1,
    commands: Vec<GpuRegionCommand>,
    source_role: MaterialChannelRole,
    tile: PixelBounds,
    max_texture_dimension_2d: u32,
    max_texture_array_layers: u32,
) -> Result<Vec<GpuMaterialSourceGroupPlan<'a>>, AtlasRenderExecutionError> {
    let tile_rect = pixel_rect(tile);
    let active_commands = commands
        .into_iter()
        .filter(|command| intersect_rect(tile_rect, command_output_rect(command)).is_some())
        .collect::<Vec<_>>();
    if active_commands.is_empty() {
        return Ok(Vec::new());
    }

    let budget = GpuAtlasSourceTextureCache::budgets().gpu_source_residency_bytes;
    let bytes_per_pixel = export_source_bytes_per_pixel(source.channel_role);
    let full_source_bytes = bounded_tile_byte_len(
        source.oriented_dimensions.width,
        source.oriented_dimensions.height,
        bytes_per_pixel,
        0,
        wgpu::COPY_BYTES_PER_ROW_ALIGNMENT,
    )
    .map_err(|error| AtlasRenderExecutionError::Gpu(error.to_string()))?;
    let full_source_fits = source.oriented_dimensions.width <= max_texture_dimension_2d
        && source.oriented_dimensions.height <= max_texture_dimension_2d
        && full_source_bytes <= budget;
    if full_source_fits {
        return Ok(vec![GpuMaterialSourceGroupPlan {
            source,
            commands: active_commands,
            source_role,
            residency: GpuMaterialSourceResidency::Full(PixelRect {
                x: 0,
                y: 0,
                width: source.oriented_dimensions.width,
                height: source.oriented_dimensions.height,
            }),
        }]);
    }

    let all_commands_plan = resident_source_page_plan_for_commands(
        &active_commands,
        tile_rect,
        source.oriented_dimensions.width,
        source.oriented_dimensions.height,
        max_texture_dimension_2d,
        max_texture_array_layers,
        bytes_per_pixel,
        budget,
    );
    let command_batches = if let Ok(plan) = all_commands_plan {
        vec![(active_commands, plan)]
    } else {
        let mut batches = Vec::with_capacity(active_commands.len());
        for command in active_commands {
            let plan = resident_source_page_plan_for_commands(
                std::slice::from_ref(&command),
                tile_rect,
                source.oriented_dimensions.width,
                source.oriented_dimensions.height,
                max_texture_dimension_2d,
                max_texture_array_layers,
                bytes_per_pixel,
                budget,
            )?;
            batches.push((vec![command], plan));
        }
        batches
    };

    let mut groups = Vec::with_capacity(command_batches.len());
    for (commands, plan) in command_batches {
        groups.push(GpuMaterialSourceGroupPlan {
            source,
            commands,
            source_role,
            residency: GpuMaterialSourceResidency::Pages(plan),
        });
    }
    Ok(groups)
}

fn load_material_source_group<'a, 'cache>(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    cache: &'cache Mutex<GpuAtlasSourceTextureCache>,
    domain: &PreparedMaterialDomain,
    plan: GpuMaterialSourceGroupPlan<'a>,
) -> Result<GpuMaterialSourceGroup<'a, 'cache>, AtlasRenderExecutionError> {
    let (cached, lease, source_layout, cache_hit) = match plan.residency {
        GpuMaterialSourceResidency::Full(source_rect) => {
            let (cached, lease, cache_hit) =
                source_texture(device, queue, cache, plan.source, domain)?;
            (
                cached,
                lease,
                single_layer_source_page_layout(source_rect),
                cache_hit,
            )
        }
        GpuMaterialSourceResidency::Pages(page_plan) => {
            let (cached, lease, layout, cache_hit) = source_texture_page_array(
                device,
                queue,
                cache,
                plan.source,
                domain,
                page_plan.source_rect,
                page_plan.interior_width,
                page_plan.interior_height,
                page_plan.pages,
                1,
            )?;
            (cached, lease, layout, cache_hit)
        }
    };
    Ok(GpuMaterialSourceGroup {
        source: plan.source,
        cached,
        _lease: lease,
        cache_hit,
        commands: plan.commands,
        source_role: plan.source_role,
        source_layout,
    })
}

#[allow(clippy::too_many_arguments)]
fn resident_source_page_plan_for_commands(
    commands: &[GpuRegionCommand],
    tile_rect: PixelRect,
    source_width: u32,
    source_height: u32,
    max_texture_dimension_2d: u32,
    max_texture_array_layers: u32,
    bytes_per_pixel: u64,
    budget: u64,
) -> Result<GpuResidentSourcePagePlan, AtlasRenderExecutionError> {
    let footprints = execution_source_footprint_rects_for_commands(
        commands,
        tile_rect,
        source_width,
        source_height,
    );
    if footprints.is_empty() {
        return Err(AtlasRenderExecutionError::Gpu(
            "source command batch has no resident source footprint".into(),
        ));
    }
    resident_source_page_plan(
        &footprints,
        source_width,
        source_height,
        max_texture_dimension_2d,
        max_texture_array_layers,
        bytes_per_pixel,
        budget,
        1,
    )
}

#[allow(clippy::too_many_arguments)]
fn resident_source_page_plan(
    footprints: &[PixelRect],
    source_width: u32,
    source_height: u32,
    max_texture_dimension_2d: u32,
    max_texture_array_layers: u32,
    bytes_per_pixel: u64,
    budget: u64,
    halo: u32,
) -> Result<GpuResidentSourcePagePlan, AtlasRenderExecutionError> {
    if max_texture_array_layers == 0 {
        return Err(AtlasRenderExecutionError::Gpu(
            "adapter reports zero texture array layers for source residency".into(),
        ));
    }
    let max_interior = max_texture_dimension_2d
        .checked_sub(halo.saturating_mul(2))
        .ok_or_else(|| {
            AtlasRenderExecutionError::Gpu(
                "adapter maximum texture size cannot contain source page halo".into(),
            )
        })?
        .max(1);
    let Some(footprint_bounds) = union_rects(footprints) else {
        return Err(AtlasRenderExecutionError::Gpu(
            "source command batch has no resident source footprint".into(),
        ));
    };
    let widest_footprint = footprints.iter().map(|rect| rect.width).max().unwrap_or(1);
    let tallest_footprint = footprints.iter().map(|rect| rect.height).max().unwrap_or(1);
    let width_candidates =
        resident_interior_candidates(widest_footprint, max_interior, max_texture_array_layers);
    let height_candidates =
        resident_interior_candidates(tallest_footprint, max_interior, max_texture_array_layers);
    let mut best = None;
    for interior_width in width_candidates {
        for &interior_height in &height_candidates {
            let source_rect = align_resident_rect_to_page_grid(
                footprint_bounds,
                interior_width,
                interior_height,
                source_width,
                source_height,
            );
            let pages =
                required_resident_pages(footprints, source_rect, interior_width, interior_height);
            let layer_count = pages.len() as u32;
            if layer_count == 0 || layer_count > max_texture_array_layers {
                continue;
            }
            let Some(page_width) = interior_width.checked_add(halo.saturating_mul(2)) else {
                continue;
            };
            let Some(page_height) = interior_height.checked_add(halo.saturating_mul(2)) else {
                continue;
            };
            if page_width > max_texture_dimension_2d || page_height > max_texture_dimension_2d {
                continue;
            }
            let Some(byte_len) = u64::from(page_width)
                .checked_mul(u64::from(page_height))
                .and_then(|bytes| bytes.checked_mul(bytes_per_pixel))
                .and_then(|bytes| bytes.checked_mul(u64::from(layer_count)))
            else {
                continue;
            };
            if byte_len > budget {
                continue;
            }
            let candidate = GpuResidentSourcePagePlan {
                source_rect,
                interior_width,
                interior_height,
                pages,
                byte_len,
            };
            best = Some(match best {
                Some(existing)
                    if resident_plan_sort_key(&existing) <= resident_plan_sort_key(&candidate) =>
                {
                    existing
                }
                _ => candidate,
            });
        }
    }
    best.ok_or_else(|| {
        AtlasRenderExecutionError::Gpu(format!(
            "source footprint {}x{} at {},{} cannot fit resident source pages within {} array layers and {} bytes",
            footprint_bounds.width,
            footprint_bounds.height,
            footprint_bounds.x,
            footprint_bounds.y,
            max_texture_array_layers,
            budget
        ))
    })
}

fn resident_plan_sort_key(plan: &GpuResidentSourcePagePlan) -> (u32, u64) {
    (plan.layer_count(), plan.byte_len)
}

fn union_rects(rects: &[PixelRect]) -> Option<PixelRect> {
    rects
        .iter()
        .copied()
        .reduce(|existing, rect| union_rect(existing, rect))
}

fn required_resident_pages(
    rects: &[PixelRect],
    source_rect: PixelRect,
    interior_width: u32,
    interior_height: u32,
) -> Vec<GpuResidentSourcePage> {
    let mut pages = BTreeSet::new();
    let interior_width = interior_width.max(1);
    let interior_height = interior_height.max(1);
    let max_page_x = source_rect.width.div_ceil(interior_width).saturating_sub(1);
    let max_page_y = source_rect
        .height
        .div_ceil(interior_height)
        .saturating_sub(1);
    for rect in rects {
        let Some(intersection) = intersect_rect(*rect, source_rect) else {
            continue;
        };
        let local_x0 = intersection.x.saturating_sub(source_rect.x);
        let local_y0 = intersection.y.saturating_sub(source_rect.y);
        let local_x1 = intersection
            .x
            .saturating_add(intersection.width.saturating_sub(1))
            .saturating_sub(source_rect.x);
        let local_y1 = intersection
            .y
            .saturating_add(intersection.height.saturating_sub(1))
            .saturating_sub(source_rect.y);
        let page_x0 = (local_x0 / interior_width).min(max_page_x);
        let page_y0 = (local_y0 / interior_height).min(max_page_y);
        let page_x1 = (local_x1 / interior_width).min(max_page_x);
        let page_y1 = (local_y1 / interior_height).min(max_page_y);
        for page_y in page_y0..=page_y1 {
            for page_x in page_x0..=page_x1 {
                pages.insert(GpuResidentSourcePage {
                    x: page_x,
                    y: page_y,
                });
            }
        }
    }
    let mut pages = pages.into_iter().collect::<Vec<_>>();
    pages.sort_by_key(|page| (page.y, page.x));
    pages
}

fn source_page_table_hash(pages: &[GpuResidentSourcePage]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    pages.hash(&mut hasher);
    hasher.finish()
}

fn encode_source_page_table(layout: &GpuSourcePageLayout) -> Vec<u8> {
    let pages = if layout.source_page_table.is_empty() {
        vec![GpuResidentSourcePage { x: 0, y: 0 }]
    } else {
        layout.source_page_table.clone()
    };
    let mut bytes = Vec::with_capacity(pages.len() * 16);
    for (layer, page) in pages.iter().enumerate() {
        bytes.extend_from_slice(&page.x.to_le_bytes());
        bytes.extend_from_slice(&page.y.to_le_bytes());
        bytes.extend_from_slice(&(layer as u32).to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
    }
    bytes
}

fn resident_interior_candidates(length: u32, max_interior: u32, max_layers: u32) -> Vec<u32> {
    let length = length.max(1);
    let max_interior = max_interior.max(1);
    let max_counts = max_layers.max(1).min(length);
    let mut candidates = BTreeSet::new();
    candidates.insert(length.min(max_interior));
    candidates.insert(1);
    for count in 1..=max_counts.min(64) {
        candidates.insert(length.div_ceil(count).min(max_interior).max(1));
    }
    let mut count = 1_u32;
    while count <= max_counts {
        candidates.insert(length.div_ceil(count).min(max_interior).max(1));
        count = count.saturating_mul(2);
        if count == 0 {
            break;
        }
    }
    candidates.into_iter().rev().collect()
}

fn align_resident_rect_to_page_grid(
    rect: PixelRect,
    interior_width: u32,
    interior_height: u32,
    source_width: u32,
    source_height: u32,
) -> PixelRect {
    let aligned_width = rect.width.div_ceil(interior_width) * interior_width;
    let aligned_height = rect.height.div_ceil(interior_height) * interior_height;
    let right = rect.x.saturating_add(aligned_width).min(source_width);
    let bottom = rect.y.saturating_add(aligned_height).min(source_height);
    PixelRect {
        x: rect.x,
        y: rect.y,
        width: right.saturating_sub(rect.x).max(rect.width),
        height: bottom.saturating_sub(rect.y).max(rect.height),
    }
}

fn source_texture<'cache>(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    cache: &'cache Mutex<GpuAtlasSourceTextureCache>,
    source: &CompiledSourceCommandV1,
    domain: &PreparedMaterialDomain,
) -> Result<
    (
        Arc<GpuCachedSourceTexture>,
        GpuSourceTextureLease<'cache>,
        bool,
    ),
    AtlasRenderExecutionError,
> {
    let key = GpuSourceTextureKey::from_source(source);
    let mut cache_guard = cache
        .lock()
        .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?;
    cache_guard.clock = cache_guard.clock.saturating_add(1);
    let clock = cache_guard.clock;
    if let Some(texture) = cache_guard.sources.get(&key) {
        let texture = Arc::clone(texture);
        *cache_guard.source_pins.entry(key.clone()).or_insert(0) += 1;
        drop(cache_guard);
        return Ok((
            texture,
            GpuSourceTextureLease {
                cache,
                key,
                active: true,
            },
            true,
        ));
    }
    drop(cache_guard);
    let payload = source_texture_payload(domain, source.channel_role)?;
    let validity_byte_len = u64::from(source.oriented_dimensions.width)
        .checked_mul(u64::from(source.oriented_dimensions.height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| AtlasRenderExecutionError::Gpu("validity texture bytes overflow".into()))?;
    let byte_len = (payload.bytes.len() as u64)
        .checked_add(validity_byte_len)
        .ok_or_else(|| AtlasRenderExecutionError::Gpu("source texture bytes overflow".into()))?;
    let reservation = reserve_source_texture_cache_space(
        cache,
        key.clone(),
        byte_len,
        source.oriented_dimensions.width,
        source.oriented_dimensions.height,
    )?;
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
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("hot-trimmer-base-color-source-texture-array-view"),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        array_layer_count: Some(1),
        ..Default::default()
    });
    let validity_layout = single_layer_source_page_layout(PixelRect {
        x: 0,
        y: 0,
        width: source.oriented_dimensions.width,
        height: source.oriented_dimensions.height,
    });
    let (validity_texture, validity_view) =
        create_validity_texture_array(device, queue, domain, &validity_layout)?;
    let cached = Arc::new(GpuCachedSourceTexture {
        _texture: texture,
        view,
        _validity_texture: validity_texture,
        validity_view,
        byte_len,
        layer_count: 1,
        last_used: clock,
    });
    let (cached, lease) = reservation.commit(key, cached)?;
    Ok((cached, lease, false))
}

fn reserve_source_texture_cache_space<'a>(
    cache: &'a Mutex<GpuAtlasSourceTextureCache>,
    key: GpuSourceTextureKey,
    byte_len: u64,
    width: u32,
    height: u32,
) -> Result<GpuSourceTextureReservation<'a>, AtlasRenderExecutionError> {
    let budget = GpuAtlasSourceTextureCache::budgets().gpu_source_residency_bytes;
    if byte_len > budget {
        return Err(AtlasRenderExecutionError::Gpu(format!(
            "source texture {width}x{height} exceeds the declared GPU source residency budget",
        )));
    }
    let mut cache_guard = cache
        .lock()
        .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?;
    while !cache_guard.sources.is_empty()
        && cache_guard
            .source_resident_bytes()
            .saturating_add(cache_guard.source_reserved_bytes)
            .saturating_add(byte_len)
            > budget
    {
        let Some(oldest) = cache_guard
            .sources
            .iter()
            .filter(|(key, _)| !cache_guard.source_pins.contains_key(*key))
            .min_by_key(|(_, value)| value.last_used)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        cache_guard.sources.remove(&oldest);
    }
    const MAX_GPU_SOURCES: usize = 8;
    while cache_guard.sources.len() >= MAX_GPU_SOURCES && !cache_guard.sources.contains_key(&key) {
        let Some(oldest) = cache_guard
            .sources
            .iter()
            .filter(|(key, _)| !cache_guard.source_pins.contains_key(*key))
            .min_by_key(|(_, value)| value.last_used)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        cache_guard.sources.remove(&oldest);
    }
    if cache_guard
        .source_resident_bytes()
        .saturating_add(cache_guard.source_reserved_bytes)
        .saturating_add(byte_len)
        > budget
    {
        return Err(AtlasRenderExecutionError::Gpu(format!(
            "source texture {width}x{height} cannot reserve GPU source residency budget",
        )));
    }
    cache_guard.source_reserved_bytes = cache_guard.source_reserved_bytes.saturating_add(byte_len);
    drop(cache_guard);
    Ok(GpuSourceTextureReservation {
        cache,
        byte_len,
        active: true,
    })
}

#[allow(dead_code)]
fn source_texture_tile<'cache>(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    cache: &'cache Mutex<GpuAtlasSourceTextureCache>,
    source: &CompiledSourceCommandV1,
    domain: &PreparedMaterialDomain,
    rect: PixelRect,
) -> Result<
    (
        Arc<GpuCachedSourceTexture>,
        GpuSourceTextureLease<'cache>,
        bool,
    ),
    AtlasRenderExecutionError,
> {
    let key = GpuSourceTextureKey::from_source_rect(source, rect);
    let mut cache_guard = cache
        .lock()
        .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?;
    cache_guard.clock = cache_guard.clock.saturating_add(1);
    let clock = cache_guard.clock;
    if let Some(texture) = cache_guard.sources.get(&key) {
        let texture = Arc::clone(texture);
        *cache_guard.source_pins.entry(key.clone()).or_insert(0) += 1;
        drop(cache_guard);
        return Ok((
            texture,
            GpuSourceTextureLease {
                cache,
                key,
                active: true,
            },
            true,
        ));
    }
    drop(cache_guard);
    let payload = source_texture_payload_rect(domain, source.channel_role, rect)?;
    let validity_byte_len = u64::from(rect.width)
        .checked_mul(u64::from(rect.height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| AtlasRenderExecutionError::Gpu("validity texture bytes overflow".into()))?;
    let byte_len = (payload.bytes.len() as u64)
        .checked_add(validity_byte_len)
        .ok_or_else(|| AtlasRenderExecutionError::Gpu("source tile bytes overflow".into()))?;
    let reservation =
        reserve_source_texture_cache_space(cache, key.clone(), byte_len, rect.width, rect.height)?;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("hot-trimmer-source-tile-texture"),
        size: wgpu::Extent3d {
            width: rect.width,
            height: rect.height,
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
            bytes_per_row: Some(rect.width * payload.bytes_per_pixel),
            rows_per_image: Some(rect.height),
        },
        wgpu::Extent3d {
            width: rect.width,
            height: rect.height,
            depth_or_array_layers: 1,
        },
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("hot-trimmer-source-tile-texture-array-view"),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        array_layer_count: Some(1),
        ..Default::default()
    });
    let validity_layout = single_layer_source_page_layout(rect);
    let (validity_texture, validity_view) =
        create_validity_texture_array(device, queue, domain, &validity_layout)?;
    let cached = Arc::new(GpuCachedSourceTexture {
        _texture: texture,
        view,
        _validity_texture: validity_texture,
        validity_view,
        byte_len,
        layer_count: 1,
        last_used: clock,
    });
    let (cached, lease) = reservation.commit(key, cached)?;
    Ok((cached, lease, false))
}

#[allow(clippy::too_many_arguments)]
fn source_texture_page_array<'cache>(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    cache: &'cache Mutex<GpuAtlasSourceTextureCache>,
    source: &CompiledSourceCommandV1,
    domain: &PreparedMaterialDomain,
    source_rect: PixelRect,
    interior_width: u32,
    interior_height: u32,
    pages: Vec<GpuResidentSourcePage>,
    halo: u32,
) -> Result<
    (
        Arc<GpuCachedSourceTexture>,
        GpuSourceTextureLease<'cache>,
        GpuSourcePageLayout,
        bool,
    ),
    AtlasRenderExecutionError,
> {
    if interior_width == 0 || interior_height == 0 {
        return Err(AtlasRenderExecutionError::Gpu(
            "source page array interior dimensions must be non-zero".into(),
        ));
    }
    if source_rect.x.saturating_add(source_rect.width) > domain.width
        || source_rect.y.saturating_add(source_rect.height) > domain.height
        || source_rect.width == 0
        || source_rect.height == 0
    {
        return Err(AtlasRenderExecutionError::Gpu(
            "source page array exceeds prepared source bounds".into(),
        ));
    }
    let page_width = interior_width
        .checked_add(halo.saturating_mul(2))
        .ok_or_else(|| AtlasRenderExecutionError::Gpu("source page width overflow".into()))?;
    let page_height = interior_height
        .checked_add(halo.saturating_mul(2))
        .ok_or_else(|| AtlasRenderExecutionError::Gpu("source page height overflow".into()))?;
    let count_x = source_rect.width.div_ceil(interior_width);
    let count_y = source_rect.height.div_ceil(interior_height);
    let dense_layer_count = count_x
        .checked_mul(count_y)
        .ok_or_else(|| AtlasRenderExecutionError::Gpu("source page layer count overflow".into()))?;
    let pages = if pages.is_empty() {
        (0..count_y)
            .flat_map(|page_y| {
                (0..count_x).map(move |page_x| GpuResidentSourcePage {
                    x: page_x,
                    y: page_y,
                })
            })
            .collect::<Vec<_>>()
    } else {
        pages
    };
    let layer_count = u32::try_from(pages.len())
        .map_err(|_| AtlasRenderExecutionError::Gpu("source page layer count overflow".into()))?;
    let limits = device.limits();
    if page_width > limits.max_texture_dimension_2d || page_height > limits.max_texture_dimension_2d
    {
        return Err(AtlasRenderExecutionError::Gpu(format!(
            "source page array page {}x{} exceeds adapter 2D texture limit {}",
            page_width, page_height, limits.max_texture_dimension_2d
        )));
    }
    if layer_count > limits.max_texture_array_layers {
        return Err(AtlasRenderExecutionError::Gpu(format!(
            "source page array layer count {} exceeds adapter texture array layer limit {}",
            layer_count, limits.max_texture_array_layers
        )));
    }
    let layout = GpuSourcePageLayout {
        source_rect,
        source_page_width: page_width,
        source_page_height: page_height,
        source_page_interior_width: interior_width,
        source_page_interior_height: interior_height,
        source_page_count_x: count_x,
        source_page_count_y: count_y,
        source_page_halo: halo,
        source_page_mode: if layer_count == dense_layer_count {
            1
        } else {
            2
        },
        source_page_table_hash: source_page_table_hash(&pages),
        source_page_table: pages,
    };
    let key = GpuSourceTextureKey::from_source_page_layout(source, &layout);
    let mut cache_guard = cache
        .lock()
        .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?;
    cache_guard.clock = cache_guard.clock.saturating_add(1);
    let clock = cache_guard.clock;
    if let Some(texture) = cache_guard.sources.get(&key) {
        let texture = Arc::clone(texture);
        *cache_guard.source_pins.entry(key.clone()).or_insert(0) += 1;
        drop(cache_guard);
        return Ok((
            texture,
            GpuSourceTextureLease {
                cache,
                key,
                active: true,
            },
            layout,
            true,
        ));
    }
    drop(cache_guard);

    let first_payload = source_texture_payload_clamped_rect(
        domain,
        source.channel_role,
        source_rect,
        PixelRect {
            x: source_rect
                .x
                .saturating_add(layout.source_page_table[0].x.saturating_mul(interior_width))
                .saturating_sub(halo)
                .max(source_rect.x),
            y: source_rect
                .y
                .saturating_add(
                    layout.source_page_table[0]
                        .y
                        .saturating_mul(interior_height),
                )
                .saturating_sub(halo)
                .max(source_rect.y),
            width: page_width,
            height: page_height,
        },
    )?;
    let layer_bytes = first_payload.bytes.len() as u64;
    let source_byte_len = layer_bytes
        .checked_mul(u64::from(layer_count))
        .ok_or_else(|| AtlasRenderExecutionError::Gpu("source page array bytes overflow".into()))?;
    let validity_byte_len = u64::from(page_width)
        .checked_mul(u64::from(page_height))
        .and_then(|pixels| pixels.checked_mul(u64::from(layer_count)))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| {
            AtlasRenderExecutionError::Gpu("validity page array bytes overflow".into())
        })?;
    let byte_len = source_byte_len
        .checked_add(validity_byte_len)
        .ok_or_else(|| AtlasRenderExecutionError::Gpu("source page array bytes overflow".into()))?;
    let budget = GpuAtlasSourceTextureCache::budgets().gpu_source_residency_bytes;
    if byte_len > budget {
        return Err(AtlasRenderExecutionError::Gpu(format!(
            "source page array {}x{}x{} exceeds the declared GPU source residency budget",
            page_width, page_height, layer_count
        )));
    }
    let reservation = reserve_source_texture_cache_space(
        cache,
        key.clone(),
        byte_len,
        page_width,
        page_height.saturating_mul(layer_count),
    )?;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("hot-trimmer-source-page-array-texture"),
        size: wgpu::Extent3d {
            width: page_width,
            height: page_height,
            depth_or_array_layers: layer_count,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: first_payload.format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (layer_index, page) in layout.source_page_table.iter().enumerate() {
        let layer = u32::try_from(layer_index).map_err(|_| {
            AtlasRenderExecutionError::Gpu("source page layer count overflow".into())
        })?;
        let interior_origin_x = source_rect
            .x
            .saturating_add(page.x.saturating_mul(interior_width));
        let interior_origin_y = source_rect
            .y
            .saturating_add(page.y.saturating_mul(interior_height));
        let page_origin = PixelRect {
            x: interior_origin_x.saturating_sub(halo).max(source_rect.x),
            y: interior_origin_y.saturating_sub(halo).max(source_rect.y),
            width: page_width,
            height: page_height,
        };
        let payload = if layer == 0 {
            GpuSourceTexturePayload {
                bytes: first_payload.bytes.clone(),
                format: first_payload.format,
                bytes_per_pixel: first_payload.bytes_per_pixel,
            }
        } else {
            source_texture_payload_clamped_rect(
                domain,
                source.channel_role,
                source_rect,
                page_origin,
            )?
        };
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: layer,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &payload.bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(page_width * payload.bytes_per_pixel),
                rows_per_image: Some(page_height),
            },
            wgpu::Extent3d {
                width: page_width,
                height: page_height,
                depth_or_array_layers: 1,
            },
        );
    }
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("hot-trimmer-source-page-array-view"),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        array_layer_count: Some(layer_count),
        ..Default::default()
    });
    let (validity_texture, validity_view) =
        create_validity_texture_array(device, queue, domain, &layout)?;
    let cached = Arc::new(GpuCachedSourceTexture {
        _texture: texture,
        view,
        _validity_texture: validity_texture,
        validity_view,
        byte_len,
        layer_count,
        last_used: clock,
    });
    let (cached, lease) = reservation.commit(key, cached)?;
    Ok((cached, lease, layout, false))
}

struct GpuSourceTexturePayload {
    bytes: Vec<u8>,
    format: wgpu::TextureFormat,
    bytes_per_pixel: u32,
}

fn create_validity_texture_array(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    domain: &PreparedMaterialDomain,
    layout: &GpuSourcePageLayout,
) -> Result<(wgpu::Texture, wgpu::TextureView), AtlasRenderExecutionError> {
    let layer_count = u32::try_from(layout.source_page_table.len())
        .map_err(|_| AtlasRenderExecutionError::Gpu("validity layer count overflow".into()))?;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("hot-trimmer-source-validity-array"),
        size: wgpu::Extent3d {
            width: layout.source_page_width,
            height: layout.source_page_height,
            depth_or_array_layers: layer_count,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (layer_index, page) in layout.source_page_table.iter().enumerate() {
        let interior_x = layout
            .source_rect
            .x
            .saturating_add(page.x.saturating_mul(layout.source_page_interior_width));
        let interior_y = layout
            .source_rect
            .y
            .saturating_add(page.y.saturating_mul(layout.source_page_interior_height));
        let page_x = interior_x
            .saturating_sub(layout.source_page_halo)
            .max(layout.source_rect.x);
        let page_y = interior_y
            .saturating_sub(layout.source_page_halo)
            .max(layout.source_rect.y);
        let mut bytes = Vec::with_capacity(
            layout.source_page_width as usize * layout.source_page_height as usize * 4,
        );
        let max_x = layout
            .source_rect
            .x
            .saturating_add(layout.source_rect.width)
            .saturating_sub(1);
        let max_y = layout
            .source_rect
            .y
            .saturating_add(layout.source_rect.height)
            .saturating_sub(1);
        for y in 0..layout.source_page_height {
            for x in 0..layout.source_page_width {
                let source_x = page_x.saturating_add(x).min(max_x);
                let source_y = page_y.saturating_add(y).min(max_y);
                bytes.extend_from_slice(&domain.validity.pixel(source_x, source_y).0.to_le_bytes());
            }
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: layer_index as u32,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(layout.source_page_width * 4),
                rows_per_image: Some(layout.source_page_height),
            },
            wgpu::Extent3d {
                width: layout.source_page_width,
                height: layout.source_page_height,
                depth_or_array_layers: 1,
            },
        );
    }
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("hot-trimmer-source-validity-array-view"),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        array_layer_count: Some(layer_count),
        ..Default::default()
    });
    Ok((texture, view))
}

fn source_texture_payload(
    domain: &PreparedMaterialDomain,
    role: MaterialChannelRole,
) -> Result<GpuSourceTexturePayload, AtlasRenderExecutionError> {
    source_texture_payload_rect(
        domain,
        role,
        PixelRect {
            x: 0,
            y: 0,
            width: domain.width,
            height: domain.height,
        },
    )
}

fn source_texture_payload_rect(
    domain: &PreparedMaterialDomain,
    role: MaterialChannelRole,
    rect: PixelRect,
) -> Result<GpuSourceTexturePayload, AtlasRenderExecutionError> {
    if rect.x.saturating_add(rect.width) > domain.width
        || rect.y.saturating_add(rect.height) > domain.height
    {
        return Err(AtlasRenderExecutionError::Gpu(
            "source texture tile exceeds prepared source bounds".into(),
        ));
    }
    source_texture_payload_mapped(domain, role, rect.width, rect.height, |local_x, local_y| {
        (rect.x + local_x, rect.y + local_y)
    })
}

fn source_texture_payload_clamped_rect(
    domain: &PreparedMaterialDomain,
    role: MaterialChannelRole,
    source_rect: PixelRect,
    rect: PixelRect,
) -> Result<GpuSourceTexturePayload, AtlasRenderExecutionError> {
    if source_rect.x.saturating_add(source_rect.width) > domain.width
        || source_rect.y.saturating_add(source_rect.height) > domain.height
        || source_rect.width == 0
        || source_rect.height == 0
    {
        return Err(AtlasRenderExecutionError::Gpu(
            "source page array exceeds prepared source bounds".into(),
        ));
    }
    let max_x = source_rect.x + source_rect.width - 1;
    let max_y = source_rect.y + source_rect.height - 1;
    source_texture_payload_mapped(domain, role, rect.width, rect.height, |local_x, local_y| {
        (
            rect.x.saturating_add(local_x).clamp(source_rect.x, max_x),
            rect.y.saturating_add(local_y).clamp(source_rect.y, max_y),
        )
    })
}

fn source_texture_payload_mapped(
    domain: &PreparedMaterialDomain,
    role: MaterialChannelRole,
    width: u32,
    height: u32,
    mut source_xy: impl FnMut(u32, u32) -> (u32, u32),
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
            let mut rgba = Vec::with_capacity((u64::from(width) * u64::from(height) * 4) as usize);
            for local_y in 0..height {
                for local_x in 0..width {
                    let (x, y) = source_xy(local_x, local_y);
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
            let mut r32 = Vec::with_capacity((u64::from(width) * u64::from(height) * 4) as usize);
            for local_y in 0..height {
                for local_x in 0..width {
                    let (x, y) = source_xy(local_x, local_y);
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
            let mut r32 = Vec::with_capacity((u64::from(width) * u64::from(height) * 4) as usize);
            for local_y in 0..height {
                for local_x in 0..width {
                    let (x, y) = source_xy(local_x, local_y);
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
            let mut rgba = Vec::with_capacity((u64::from(width) * u64::from(height) * 4) as usize);
            for local_y in 0..height {
                for local_x in 0..width {
                    let (x, y) = source_xy(local_x, local_y);
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
            let mut r32 = Vec::with_capacity((u64::from(width) * u64::from(height) * 4) as usize);
            for local_y in 0..height {
                for local_x in 0..width {
                    let (x, y) = source_xy(local_x, local_y);
                    r32.extend_from_slice(&plane.pixel(x, y).0.to_le_bytes());
                }
            }
            Ok(GpuSourceTexturePayload {
                bytes: r32,
                format: wgpu::TextureFormat::R32Uint,
                bytes_per_pixel: 4,
            })
        }
    }
}

fn source_channel_role_for_source(
    plan: &CompiledAtlasPlanV1,
    source: &CompiledSourceCommandV1,
    map: hot_trimmer_domain::MaterialMapKind,
) -> Option<MaterialChannelRole> {
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
        Some(preferred)
    } else if map == MaterialMapKind::Normal {
        None
    } else {
        Some(MaterialChannelRole::BaseColor)
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
        MaterialMapKind::EdgeMask => 8,
        MaterialMapKind::Specular
        | MaterialMapKind::Opacity
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
    validate_gpu_slice_contract(command)?;
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
        hot_trimmer_domain::SamplingMode::DirectCrop => 0,
        hot_trimmer_domain::SamplingMode::PeriodicTile => 1,
        hot_trimmer_domain::SamplingMode::RepeatX => 2,
        hot_trimmer_domain::SamplingMode::RepeatY => 3,
        hot_trimmer_domain::SamplingMode::PlanarRadial => 4,
        hot_trimmer_domain::SamplingMode::PolarRadial => 5,
        hot_trimmer_domain::SamplingMode::ExplicitStretch => 6,
        hot_trimmer_domain::SamplingMode::ThreeSliceCap => 7,
        hot_trimmer_domain::SamplingMode::UniqueContain => 8,
        hot_trimmer_domain::SamplingMode::UniqueCover => 9,
        hot_trimmer_domain::SamplingMode::TextureSynthesis => 10,
        hot_trimmer_domain::SamplingMode::NineSlicePanel => 11,
    };
    let crop = command.source_crop.0;
    let period = command
        .sampling_plan
        .candidate
        .period_pixels
        .unwrap_or([crop.width.max(1), crop.height.max(1)]);
    let (slice_left, slice_right, slice_top, slice_bottom, slice_center) =
        match command.sampling_plan.slice_geometry {
            SliceGeometry::None => (0, 0, 0, 0, 0),
            SliceGeometry::Three {
                leading_cap_pixels,
                trailing_cap_pixels,
                center,
            } => (
                leading_cap_pixels,
                trailing_cap_pixels,
                0,
                0,
                slice_center_code(center),
            ),
            SliceGeometry::Nine {
                left_pixels,
                right_pixels,
                top_pixels,
                bottom_pixels,
                center,
            } => (
                left_pixels,
                right_pixels,
                top_pixels,
                bottom_pixels,
                slice_center_code(center),
            ),
        };
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
        slice_left,
        slice_right,
        slice_top,
        slice_bottom,
        slice_center,
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

fn validate_gpu_slice_contract(
    command: &CompiledRegionCommandV1,
) -> Result<(), AtlasRenderExecutionError> {
    let plan = &command.sampling_plan;
    let mode = plan.candidate.mapping_mode;
    let crop = command.source_crop.0;
    let authorized_stretch = matches!(
        plan.stretch_override,
        StretchOverrideProvenance::UserOverride { .. }
    );
    if mode == hot_trimmer_domain::SamplingMode::ExplicitStretch && !authorized_stretch {
        return Err(AtlasRenderExecutionError::InvalidInput(format!(
            "region {} has unauthorized ExplicitStretch",
            command.region_id
        )));
    }
    let geometry_valid = match (mode, plan.slice_geometry) {
        (
            hot_trimmer_domain::SamplingMode::ThreeSliceCap,
            SliceGeometry::Three {
                leading_cap_pixels,
                trailing_cap_pixels,
                center,
            },
        ) => {
            leading_cap_pixels > 0
                && trailing_cap_pixels > 0
                && leading_cap_pixels
                    .checked_add(trailing_cap_pixels)
                    .is_some_and(|sum| sum < crop.width)
                && center != SliceCenterPolicy::ExplicitStretch
        }
        (
            hot_trimmer_domain::SamplingMode::NineSlicePanel,
            SliceGeometry::Nine {
                left_pixels,
                right_pixels,
                top_pixels,
                bottom_pixels,
                center,
            },
        ) => {
            left_pixels > 0
                && right_pixels > 0
                && top_pixels > 0
                && bottom_pixels > 0
                && left_pixels
                    .checked_add(right_pixels)
                    .is_some_and(|sum| sum < crop.width)
                && top_pixels
                    .checked_add(bottom_pixels)
                    .is_some_and(|sum| sum < crop.height)
                && (center != SliceCenterPolicy::ExplicitStretch || authorized_stretch)
        }
        (
            hot_trimmer_domain::SamplingMode::ThreeSliceCap
            | hot_trimmer_domain::SamplingMode::NineSlicePanel,
            _,
        ) => false,
        (_, SliceGeometry::None) => true,
        _ => false,
    };
    let rotated = matches!(
        plan.candidate.transform.rotation,
        hot_trimmer_domain::QuarterTurn::Ninety | hot_trimmer_domain::QuarterTurn::TwoSeventy
    );
    let size = if rotated {
        [plan.slot_physical_size[1], plan.slot_physical_size[0]]
    } else {
        plan.slot_physical_size
    };
    let scale = plan.source_pixels_per_physical_unit * plan.sampling_policy.scale;
    let destination_valid = match plan.slice_geometry {
        SliceGeometry::Three {
            leading_cap_pixels,
            trailing_cap_pixels,
            ..
        } => size[0] > f64::from(leading_cap_pixels.saturating_add(trailing_cap_pixels)) / scale,
        SliceGeometry::Nine {
            left_pixels,
            right_pixels,
            top_pixels,
            bottom_pixels,
            ..
        } => {
            size[0] > f64::from(left_pixels.saturating_add(right_pixels)) / scale
                && size[1] > f64::from(top_pixels.saturating_add(bottom_pixels)) / scale
        }
        SliceGeometry::None => true,
    };
    if !geometry_valid || !destination_valid {
        return Err(AtlasRenderExecutionError::InvalidInput(format!(
            "region {} has an illegal GPU slice mode, geometry, center policy, or destination",
            command.region_id
        )));
    }
    Ok(())
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
        header.source_origin_x,
        header.source_origin_y,
        header.map_kind,
        header.normal_convention,
        header.source_role,
        header.source_page_width,
        header.source_page_height,
        header.source_page_interior_width,
        header.source_page_interior_height,
        header.source_page_count_x,
        header.source_page_count_y,
        header.source_page_halo,
        header.source_page_mode,
    ];
    for (index, value) in values.into_iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn profile_program_code(program: hot_trimmer_effect_compiler::ProfileProgram) -> u32 {
    use hot_trimmer_effect_compiler::ProfileProgram::*;
    match program {
        Flat => 0,
        ConvexBevel => 1,
        ConcaveGroove => 2,
        RoundedBevel => 3,
        DoubleBevel => 4,
        RaisedLip => 5,
        RecessedSeam => 6,
        PanelFrame => 7,
        FullyRoundedStrip => 8,
        MergedOpposingBevel => 9,
        RadialDisc => 10,
        Annulus => 11,
        CustomCurve => 12,
    }
}

fn profile_lod_code(lod: hot_trimmer_effect_compiler::ProfileLod) -> u32 {
    use hot_trimmer_effect_compiler::ProfileLod::*;
    match lod {
        FullHeight => 0,
        SimplifiedHeight => 1,
        NormalOnly => 2,
        RoughnessOnly => 3,
        Disabled => 4,
    }
}

fn profile_sdf_code(sdf: hot_trimmer_effect_compiler::ProfileSdf) -> u32 {
    match sdf {
        hot_trimmer_effect_compiler::ProfileSdf::Rectangle => 0,
        hot_trimmer_effect_compiler::ProfileSdf::Disc => 1,
        hot_trimmer_effect_compiler::ProfileSdf::Annulus => 2,
    }
}

fn detail_family_code(family: hot_trimmer_effect_compiler::DetailFamily) -> u32 {
    use hot_trimmer_effect_compiler::DetailFamily;
    match family {
        DetailFamily::RepeatingStrip => 0,
        DetailFamily::UniqueDetail => 1,
        DetailFamily::RadialDetail => 2,
        DetailFamily::TrimCap => 3,
        DetailFamily::BoltGroup => 4,
        DetailFamily::Vent => 5,
        DetailFamily::PanelStamp => 6,
        DetailFamily::Groove => 7,
        DetailFamily::Decal => 8,
        DetailFamily::ProceduralMotif => 9,
        DetailFamily::UserPatch => 10,
    }
}

fn detail_lod_code(lod: hot_trimmer_effect_compiler::DetailLod) -> u32 {
    use hot_trimmer_effect_compiler::DetailLod;
    match lod {
        DetailLod::Full => 0,
        DetailLod::SimplifiedHeightNormal => 1,
        DetailLod::NormalOnly => 2,
        DetailLod::RoughnessColor => 3,
        DetailLod::Disabled => 4,
    }
}

fn detail_mapping_code(mode: hot_trimmer_effect_compiler::DetailMappingMode) -> u32 {
    match mode {
        hot_trimmer_effect_compiler::DetailMappingMode::Planar => 0,
        hot_trimmer_effect_compiler::DetailMappingMode::PolarAuthored => 1,
    }
}

fn occupancy_relation_code(relation: hot_trimmer_effect_compiler::OccupancyRelation) -> u32 {
    use hot_trimmer_effect_compiler::OccupancyRelation;
    match relation {
        OccupancyRelation::AboveProfile => 0,
        OccupancyRelation::BelowProfile => 1,
        OccupancyRelation::AvoidRaised => 2,
        OccupancyRelation::OnlyFlatCenter => 3,
        OccupancyRelation::Ignore => 4,
    }
}

fn detail_channel_bits(channels: &[hot_trimmer_effect_compiler::DetailChannelContribution]) -> u32 {
    channels.iter().fold(0_u32, |bits, channel| {
        bits | match channel.channel {
            MaterialChannelRole::Height => 1,
            MaterialChannelRole::Normal => 2,
            MaterialChannelRole::Roughness
            | MaterialChannelRole::Metallic
            | MaterialChannelRole::AmbientOcclusion
            | MaterialChannelRole::Specular
            | MaterialChannelRole::Opacity => 4,
            MaterialChannelRole::BaseColor => 8,
            MaterialChannelRole::MaterialId | MaterialChannelRole::RegionId => 16,
            MaterialChannelRole::EdgeMask => 32,
        }
    })
}

fn detail_gpu_command_count(
    plan: &CompiledAtlasPlanV1,
    requested_map: Option<MaterialMapKind>,
) -> usize {
    plan.ordered_regions
        .iter()
        .flat_map(|region| &region.compiled_details.details)
        .filter(|detail| detail_contributes_to_requested_map(detail, requested_map))
        .map(|detail| {
            let procedural = usize::from(detail_emits_procedural_default(detail));
            let operations = detail.reusable_atlas_operations.len();
            let strokes = detail
                .strokes
                .iter()
                .filter(|stroke| {
                    stroke.operation.scope
                        == hot_trimmer_effect_compiler::StampScope::MaterialReusableAtlas
                })
                .map(|stroke| stroke.physical_samples_m.len())
                .sum::<usize>();
            procedural + operations + strokes
        })
        .sum()
}

fn detail_emits_procedural_default(detail: &hot_trimmer_effect_compiler::CompiledDetail) -> bool {
    // A definition with immutable source assets must be evaluated through the
    // asset-backed path. Emitting the procedural fallback here would silently
    // bypass persisted asset resolution (and later force a dummy texture path).
    detail.definition.required_sources.is_empty()
        && detail.reusable_atlas_operations.is_empty()
        && detail.strokes.is_empty()
        && detail.asset_specific_deferred_operations.is_empty()
        && detail_allows_procedural_default(detail)
}

fn detail_fields_for_request(
    plan: &CompiledAtlasPlanV1,
    requested_map: Option<MaterialMapKind>,
) -> Vec<DetailFieldIdentity> {
    let mut fields = DETAIL_FIELD_SPECS
        .iter()
        .filter(|spec| {
            requested_map.map_or(
                matches!(
                    spec.map,
                    MaterialMapKind::BaseColor
                        | MaterialMapKind::Normal
                        | MaterialMapKind::EdgeMask
                        | MaterialMapKind::Height
                        | MaterialMapKind::Roughness
                        | MaterialMapKind::Metallic
                        | MaterialMapKind::AmbientOcclusion
                        | MaterialMapKind::Specular
                        | MaterialMapKind::Opacity
                        | MaterialMapKind::MaterialId
                ),
                |requested| spec.map == requested,
            )
        })
        .map(|spec| {
            let mut identity = plan.tile_identity(spec.map, spec.shader);
            identity.pixel_format = spec.pixel_format;
            DetailFieldIdentity {
                field_index: spec.field_index,
                identity,
                format: spec.format,
            }
        })
        .collect::<Vec<_>>();
    if requested_map.is_none_or(|map| map == MaterialMapKind::MaterialId) {
        let mut identity = plan.tile_identity(
            MaterialMapKind::MaterialId,
            "stage16-detail-material-id-valid-r32uint-v1",
        );
        identity.pixel_format = crate::CompiledTilePixelFormat::R32Uint;
        fields.push(DetailFieldIdentity {
            field_index: DETAIL_FIELD_MATERIAL_ID_VALID,
            identity,
            format: wgpu::TextureFormat::R32Uint,
        });
    }
    fields
}

fn detail_requested_field(requested_map: Option<MaterialMapKind>) -> u32 {
    let Some(requested) = requested_map else {
        return DETAIL_REQUEST_ALL_FIELDS;
    };
    DETAIL_FIELD_SPECS
        .iter()
        .find(|spec| spec.map == requested)
        .map_or(DETAIL_REQUEST_ALL_FIELDS, |spec| spec.field_index as u32)
}

fn detail_allows_procedural_default(detail: &hot_trimmer_effect_compiler::CompiledDetail) -> bool {
    use hot_trimmer_effect_compiler::DetailFamily;
    matches!(
        detail.resolved_family,
        DetailFamily::RepeatingStrip
            | DetailFamily::RadialDetail
            | DetailFamily::TrimCap
            | DetailFamily::BoltGroup
            | DetailFamily::Vent
            | DetailFamily::Groove
            | DetailFamily::ProceduralMotif
    )
}

enum DetailAssetTexture<'cache> {
    Cached {
        cached: Arc<GpuCachedSourceTexture>,
        _lease: GpuSourceTextureLease<'cache>,
    },
    Transient {
        _texture: wgpu::Texture,
        view: wgpu::TextureView,
    },
}

impl DetailAssetTexture<'_> {
    fn view(&self) -> &wgpu::TextureView {
        match self {
            Self::Cached { cached, .. } => &cached.view,
            Self::Transient { view, .. } => view,
        }
    }
}

struct DetailAssetTextures<'cache> {
    color: DetailAssetTexture<'cache>,
    scalar: DetailAssetTexture<'cache>,
    normal: DetailAssetTexture<'cache>,
    material_id: DetailAssetTexture<'cache>,
    mask: DetailAssetTexture<'cache>,
    resident_bytes: u64,
    upload_bytes: u64,
}

#[derive(Clone, Copy, Debug)]
enum DetailAssetLayerTarget {
    Color,
    Scalar,
    Normal,
    MaterialId,
    Mask,
}

fn detail_asset_textures<'cache>(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    cache: &'cache Mutex<GpuAtlasSourceTextureCache>,
    input: Option<&AtlasRenderExecutionInput<'_>>,
    plan: &CompiledAtlasPlanV1,
    commands: &mut [GpuDetailCommand],
    requested_map: Option<MaterialMapKind>,
) -> Result<DetailAssetTextures<'cache>, AtlasRenderExecutionError> {
    let needed = commands
        .iter()
        .filter(|command| command.asset_key != 0)
        .map(|command| command.asset_key)
        .collect::<BTreeSet<_>>();
    if needed.is_empty() {
        return Ok(create_dummy_detail_asset_textures(device, queue));
    }
    let input = input.ok_or_else(|| {
        AtlasRenderExecutionError::InvalidInput(
            "Stage 16 asset-backed detail execution requires production prepared-source input"
                .into(),
        )
    })?;
    let assets_by_key = plan
        .ordered_regions
        .iter()
        .flat_map(|region| &region.compiled_details.details)
        .flat_map(|detail| {
            detail
                .definition
                .required_sources
                .iter()
                .chain(
                    detail
                        .reusable_atlas_operations
                        .iter()
                        .map(|operation| &operation.asset),
                )
                .filter(|asset| needed.contains(&stable_u32_from_digest(&asset.digest)))
        })
        .fold(BTreeMap::new(), |mut assets, asset| {
            assets
                .entry(stable_u32_from_digest(&asset.digest))
                .or_insert(asset);
            assets
        });
    if assets_by_key.is_empty() {
        return Err(AtlasRenderExecutionError::MissingPreparedSource {
            source_set_id: SourceSetId::new(),
            source_id: ContentDigest("missing-stage16-stamp-asset".into()),
        });
    }
    let mut resident_bytes = 0_u64;
    let mut upload_bytes = 0_u64;
    let (color, allocation, uploaded) = detail_asset_texture_array_for_role(
        device,
        queue,
        cache,
        input,
        &assets_by_key,
        commands,
        plan.normal_convention,
        MaterialChannelRole::BaseColor,
        DetailAssetLayerTarget::Color,
        wgpu::TextureFormat::Rgba8Unorm,
        [255, 255, 255, 255],
    )?;
    resident_bytes = resident_bytes.saturating_add(allocation);
    upload_bytes = upload_bytes.saturating_add(uploaded);
    let scalar_role = requested_map
        .and_then(material_channel_role_for_map)
        .filter(|role| {
            matches!(
                role,
                MaterialChannelRole::Height
                    | MaterialChannelRole::Roughness
                    | MaterialChannelRole::Metallic
                    | MaterialChannelRole::AmbientOcclusion
                    | MaterialChannelRole::Specular
                    | MaterialChannelRole::Opacity
            )
        })
        .unwrap_or(MaterialChannelRole::Height);
    let (scalar, allocation, uploaded) = detail_asset_texture_array_for_role(
        device,
        queue,
        cache,
        input,
        &assets_by_key,
        commands,
        plan.normal_convention,
        scalar_role,
        DetailAssetLayerTarget::Scalar,
        wgpu::TextureFormat::R32Float,
        1.0_f32.to_le_bytes(),
    )?;
    resident_bytes = resident_bytes.saturating_add(allocation);
    upload_bytes = upload_bytes.saturating_add(uploaded);
    let (normal, allocation, uploaded) = detail_asset_texture_array_for_role(
        device,
        queue,
        cache,
        input,
        &assets_by_key,
        commands,
        plan.normal_convention,
        MaterialChannelRole::Normal,
        DetailAssetLayerTarget::Normal,
        wgpu::TextureFormat::Rgba8Unorm,
        [128, 128, 255, 255],
    )?;
    resident_bytes = resident_bytes.saturating_add(allocation);
    upload_bytes = upload_bytes.saturating_add(uploaded);
    let (material_id, allocation, uploaded) = detail_asset_texture_array_for_role(
        device,
        queue,
        cache,
        input,
        &assets_by_key,
        commands,
        plan.normal_convention,
        MaterialChannelRole::MaterialId,
        DetailAssetLayerTarget::MaterialId,
        wgpu::TextureFormat::R32Uint,
        0_u32.to_le_bytes(),
    )?;
    resident_bytes = resident_bytes.saturating_add(allocation);
    upload_bytes = upload_bytes.saturating_add(uploaded);
    let mask_role = requested_map
        .and_then(material_channel_role_for_map)
        .filter(|role| {
            matches!(
                role,
                MaterialChannelRole::EdgeMask | MaterialChannelRole::Opacity
            )
        })
        .unwrap_or(MaterialChannelRole::EdgeMask);
    let (mask, allocation, uploaded) = detail_asset_texture_array_for_role(
        device,
        queue,
        cache,
        input,
        &assets_by_key,
        commands,
        plan.normal_convention,
        mask_role,
        DetailAssetLayerTarget::Mask,
        wgpu::TextureFormat::R32Float,
        1.0_f32.to_le_bytes(),
    )?;
    resident_bytes = resident_bytes.saturating_add(allocation);
    upload_bytes = upload_bytes.saturating_add(uploaded);
    Ok(DetailAssetTextures {
        color,
        scalar,
        normal,
        material_id,
        mask,
        resident_bytes,
        upload_bytes,
    })
}

#[allow(clippy::too_many_arguments)]
fn detail_asset_texture_array_for_role<'cache>(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    cache: &'cache Mutex<GpuAtlasSourceTextureCache>,
    input: &AtlasRenderExecutionInput<'_>,
    assets_by_key: &BTreeMap<u32, &hot_trimmer_effect_compiler::StampAssetRef>,
    commands: &mut [GpuDetailCommand],
    normal_convention: crate::CompiledNormalConvention,
    role: MaterialChannelRole,
    target: DetailAssetLayerTarget,
    format: wgpu::TextureFormat,
    dummy_pixel: [u8; 4],
) -> Result<(DetailAssetTexture<'cache>, u64, u64), AtlasRenderExecutionError> {
    let needed = commands
        .iter()
        .filter(|command| command.asset_key != 0 && detail_command_needs_asset_role(command, role))
        .map(|command| command.asset_key)
        .collect::<BTreeSet<_>>();
    if needed.is_empty() {
        let (texture, view) = create_dummy_detail_asset_texture(
            device,
            queue,
            format,
            dummy_pixel,
            "hot-trimmer-detail-dummy-role-asset",
        );
        return Ok((
            DetailAssetTexture::Transient {
                _texture: texture,
                view,
            },
            0,
            0,
        ));
    }
    let mut prepared_assets = Vec::with_capacity(needed.len());
    let mut array_width = 1_u32;
    let mut array_height = 1_u32;
    for asset_key in &needed {
        let asset = assets_by_key.get(asset_key).ok_or_else(|| {
            AtlasRenderExecutionError::MissingPreparedSource {
                source_set_id: SourceSetId::new(),
                source_id: ContentDigest("missing-stage16-stamp-asset".into()),
            }
        })?;
        let prepared = input
            .prepared_sources
            .iter()
            .find(|prepared| {
                prepared.channel_role == role
                    && (prepared.source_id == asset.digest
                        || prepared.domain.prepared_source_digest == asset.digest
                        || prepared.domain.cache_key == asset.digest)
            })
            .ok_or_else(|| AtlasRenderExecutionError::MissingPreparedSource {
                source_set_id: SourceSetId::new(),
                source_id: asset.digest.clone(),
            })?;
        array_width = array_width.max(prepared.domain.width);
        array_height = array_height.max(prepared.domain.height);
        prepared_assets.push((*asset_key, prepared));
    }
    let layer_count = u32::try_from(prepared_assets.len())
        .map_err(|_| AtlasRenderExecutionError::Gpu("too many Stage 16 stamp assets".into()))?;
    for (layer, (asset_key, prepared)) in prepared_assets.iter().enumerate() {
        for command in commands
            .iter_mut()
            .filter(|command| command.asset_key == *asset_key)
        {
            match target {
                DetailAssetLayerTarget::Color => command.asset_layer = layer as u32,
                DetailAssetLayerTarget::Scalar => command.asset_scalar_layer = layer as u32,
                DetailAssetLayerTarget::Normal => command.asset_normal_layer = layer as u32,
                DetailAssetLayerTarget::MaterialId => {
                    command.asset_material_id_layer = layer as u32;
                }
                DetailAssetLayerTarget::Mask => command.asset_mask_layer = layer as u32,
            }
            command.asset_width = prepared.domain.width;
            command.asset_height = prepared.domain.height;
        }
    }
    let key = detail_asset_array_cache_key(
        &prepared_assets,
        role,
        target,
        format,
        normal_convention,
        array_width,
        array_height,
    );
    let mut cache_guard = cache
        .lock()
        .map_err(|_| AtlasRenderExecutionError::Gpu("GPU atlas cache is unavailable".into()))?;
    cache_guard.clock = cache_guard.clock.saturating_add(1);
    let clock = cache_guard.clock;
    if let Some(cached) = cache_guard.sources.get(&key).cloned() {
        *cache_guard.source_pins.entry(key.clone()).or_insert(0) += 1;
        drop(cache_guard);
        let resident_bytes = cached.byte_len;
        return Ok((
            DetailAssetTexture::Cached {
                cached,
                _lease: GpuSourceTextureLease {
                    cache,
                    key,
                    active: true,
                },
            },
            resident_bytes,
            0,
        ));
    }
    drop(cache_guard);
    let array_bytes = u64::from(array_width)
        .saturating_mul(u64::from(array_height))
        .saturating_mul(u64::from(layer_count))
        .saturating_mul(4);
    // GpuCachedSourceTexture owns a validity texture for the material-source
    // path. Detail arrays do not consume it, but account for the required 1x1
    // cache-owned allocation exactly.
    let resident_bytes = array_bytes.saturating_add(4);
    if resident_bytes > GpuAtlasSourceTextureCache::budgets().gpu_source_residency_bytes {
        return Err(AtlasRenderExecutionError::Gpu(format!(
            "Stage 16 {role:?} asset array requires {resident_bytes} bytes, exceeding the GPU source residency budget"
        )));
    }
    let reservation = reserve_source_texture_cache_space(
        cache,
        key.clone(),
        resident_bytes,
        array_width,
        array_height,
    )?;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("hot-trimmer-detail-asset-role-array"),
        size: wgpu::Extent3d {
            width: array_width,
            height: array_height,
            depth_or_array_layers: layer_count,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut uploaded_bytes = 0_u64;
    for (layer, (_, prepared)) in prepared_assets.iter().enumerate() {
        let payload =
            detail_asset_texture_payload(prepared.domain.as_ref(), role, normal_convention)?;
        if payload.format != format {
            return Err(AtlasRenderExecutionError::Gpu(format!(
                "Stage 16 stamp asset {role:?} uploaded as {:?}, expected {format:?}",
                payload.format
            )));
        }
        uploaded_bytes = uploaded_bytes.saturating_add(payload.bytes.len() as u64);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: layer as u32,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &payload.bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(prepared.domain.width * payload.bytes_per_pixel),
                rows_per_image: Some(prepared.domain.height),
            },
            wgpu::Extent3d {
                width: prepared.domain.width,
                height: prepared.domain.height,
                depth_or_array_layers: 1,
            },
        );
    }
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("hot-trimmer-detail-asset-role-array-view"),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        array_layer_count: Some(layer_count),
        ..Default::default()
    });
    let validity_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("hot-trimmer-detail-asset-cache-validity"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Uint,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let validity_view = validity_texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("hot-trimmer-detail-asset-cache-validity-view"),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        array_layer_count: Some(1),
        ..Default::default()
    });
    let cached = Arc::new(GpuCachedSourceTexture {
        _texture: texture,
        view,
        _validity_texture: validity_texture,
        validity_view,
        byte_len: resident_bytes,
        layer_count,
        last_used: clock,
    });
    let (cached, lease) = reservation.commit(key, cached)?;
    let resident_bytes = cached.byte_len;
    Ok((
        DetailAssetTexture::Cached {
            cached,
            _lease: lease,
        },
        resident_bytes,
        uploaded_bytes,
    ))
}

fn detail_asset_array_cache_key(
    prepared_assets: &[(u32, &AtlasPreparedSource)],
    role: MaterialChannelRole,
    target: DetailAssetLayerTarget,
    format: wgpu::TextureFormat,
    normal_convention: crate::CompiledNormalConvention,
    width: u32,
    height: u32,
) -> GpuSourceTextureKey {
    let mut fingerprint = format!(
        "stage16-detail-asset-array-v1|role={role:?}|target={target:?}|format={format:?}|normal={normal_convention:?}|size={width}x{height}|layers={}|",
        prepared_assets.len()
    );
    for (asset_key, prepared) in prepared_assets {
        fingerprint.push_str(&format!(
            "{asset_key}:{}:{}:{}:{}x{};",
            prepared.source_set_id,
            prepared.source_id.0,
            prepared.domain.prepared_source_digest.0,
            prepared.domain.width,
            prepared.domain.height
        ));
    }
    let digest = ContentDigest::sha256(fingerprint.as_bytes());
    GpuSourceTextureKey {
        source_set_id: prepared_assets[0].1.source_set_id,
        source_id: digest.clone(),
        digest,
        origin_x: 0,
        origin_y: 0,
        width,
        height,
        decoded_format: format!("{format:?}"),
        decoder_version: "stage16-detail-asset-array-v1".into(),
        color_version: format!("registered-{normal_convention:?}"),
        channel_role: role,
        page_interior_width: width,
        page_interior_height: height,
        page_halo: 0,
        page_mode: 2,
        page_table_hash: prepared_assets.len() as u64,
    }
}

fn detail_asset_texture_payload(
    domain: &PreparedMaterialDomain,
    role: MaterialChannelRole,
    requested_convention: crate::CompiledNormalConvention,
) -> Result<GpuSourceTexturePayload, AtlasRenderExecutionError> {
    if role != MaterialChannelRole::Normal {
        return source_texture_payload(domain, role);
    }
    let channel = domain
        .registered_channels()
        .iter()
        .find(|channel| channel.role() == MaterialChannelRole::Normal)
        .ok_or_else(|| {
            AtlasRenderExecutionError::Gpu("prepared source has no Normal channel".into())
        })?;
    let PreparedExemplarChannel::Normal {
        plane,
        canonical_convention,
        ..
    } = channel
    else {
        return Err(AtlasRenderExecutionError::Gpu(
            "registered Normal channel has the wrong payload type".into(),
        ));
    };
    let requested_is_open_gl = requested_convention == crate::CompiledNormalConvention::OpenGl;
    let canonical_is_open_gl =
        *canonical_convention == hot_trimmer_domain::NormalConvention::OpenGl;
    let flip_y = requested_is_open_gl != canonical_is_open_gl;
    let mut rgba =
        Vec::with_capacity((u64::from(domain.width) * u64::from(domain.height) * 4) as usize);
    for y in 0..domain.height {
        for x in 0..domain.width {
            let value = plane.pixel(x, y);
            rgba.extend_from_slice(&[
                signed_unit(value.xyz[0]),
                signed_unit(if flip_y { -value.xyz[1] } else { value.xyz[1] }),
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

fn create_dummy_detail_asset_textures(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> DetailAssetTextures<'static> {
    let (color_texture, color_view) = create_dummy_detail_asset_texture(
        device,
        queue,
        wgpu::TextureFormat::Rgba8Unorm,
        [255, 255, 255, 255],
        "hot-trimmer-detail-dummy-color-asset",
    );
    let (scalar_texture, scalar_view) = create_dummy_detail_asset_texture(
        device,
        queue,
        wgpu::TextureFormat::R32Float,
        1.0_f32.to_le_bytes(),
        "hot-trimmer-detail-dummy-scalar-asset",
    );
    let (normal_texture, normal_view) = create_dummy_detail_asset_texture(
        device,
        queue,
        wgpu::TextureFormat::Rgba8Unorm,
        [128, 128, 255, 255],
        "hot-trimmer-detail-dummy-normal-asset",
    );
    let (material_id_texture, material_id_view) = create_dummy_detail_asset_texture(
        device,
        queue,
        wgpu::TextureFormat::R32Uint,
        0_u32.to_le_bytes(),
        "hot-trimmer-detail-dummy-material-id-asset",
    );
    let (mask_texture, mask_view) = create_dummy_detail_asset_texture(
        device,
        queue,
        wgpu::TextureFormat::R32Float,
        1.0_f32.to_le_bytes(),
        "hot-trimmer-detail-dummy-mask-asset",
    );
    DetailAssetTextures {
        color: DetailAssetTexture::Transient {
            _texture: color_texture,
            view: color_view,
        },
        scalar: DetailAssetTexture::Transient {
            _texture: scalar_texture,
            view: scalar_view,
        },
        normal: DetailAssetTexture::Transient {
            _texture: normal_texture,
            view: normal_view,
        },
        material_id: DetailAssetTexture::Transient {
            _texture: material_id_texture,
            view: material_id_view,
        },
        mask: DetailAssetTexture::Transient {
            _texture: mask_texture,
            view: mask_view,
        },
        resident_bytes: 0,
        upload_bytes: 0,
    }
}

fn create_dummy_detail_asset_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
    pixel: [u8; 4],
    label: &'static str,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
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
        &pixel,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("hot-trimmer-detail-dummy-role-asset-view"),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        array_layer_count: Some(1),
        ..Default::default()
    });
    (texture, view)
}

fn detail_contributes_to_requested_map(
    detail: &hot_trimmer_effect_compiler::CompiledDetail,
    requested_map: Option<MaterialMapKind>,
) -> bool {
    let Some(map) = requested_map else {
        return true;
    };
    detail
        .definition
        .channels
        .iter()
        .chain(
            detail
                .reusable_atlas_operations
                .iter()
                .flat_map(|operation| &operation.channels),
        )
        .chain(
            detail
                .strokes
                .iter()
                .filter(|stroke| {
                    stroke.operation.scope
                        == hot_trimmer_effect_compiler::StampScope::MaterialReusableAtlas
                })
                .flat_map(|stroke| &stroke.operation.channels),
        )
        .any(|channel| detail_channel_matches_map(channel.channel, map))
}

fn detail_channel_matches_map(channel: MaterialChannelRole, map: MaterialMapKind) -> bool {
    matches!(
        (channel, map),
        (MaterialChannelRole::Height, MaterialMapKind::Height)
            | (MaterialChannelRole::Normal, MaterialMapKind::Normal)
            | (MaterialChannelRole::Roughness, MaterialMapKind::Roughness)
            | (MaterialChannelRole::Metallic, MaterialMapKind::Metallic)
            | (
                MaterialChannelRole::AmbientOcclusion,
                MaterialMapKind::AmbientOcclusion
            )
            | (MaterialChannelRole::Specular, MaterialMapKind::Specular)
            | (MaterialChannelRole::Opacity, MaterialMapKind::Opacity)
            | (MaterialChannelRole::BaseColor, MaterialMapKind::BaseColor)
            | (MaterialChannelRole::MaterialId, MaterialMapKind::MaterialId)
            | (MaterialChannelRole::RegionId, MaterialMapKind::RegionId)
            | (MaterialChannelRole::EdgeMask, MaterialMapKind::EdgeMask)
    )
}

fn material_channel_role_for_map(map: MaterialMapKind) -> Option<MaterialChannelRole> {
    Some(match map {
        MaterialMapKind::BaseColor => MaterialChannelRole::BaseColor,
        MaterialMapKind::Height => MaterialChannelRole::Height,
        MaterialMapKind::Normal => MaterialChannelRole::Normal,
        MaterialMapKind::Roughness => MaterialChannelRole::Roughness,
        MaterialMapKind::Metallic => MaterialChannelRole::Metallic,
        MaterialMapKind::AmbientOcclusion => MaterialChannelRole::AmbientOcclusion,
        MaterialMapKind::Specular => MaterialChannelRole::Specular,
        MaterialMapKind::Opacity => MaterialChannelRole::Opacity,
        MaterialMapKind::EdgeMask => MaterialChannelRole::EdgeMask,
        MaterialMapKind::MaterialId => MaterialChannelRole::MaterialId,
        MaterialMapKind::RegionId => return None,
    })
}

fn detail_command_needs_asset_role(command: &GpuDetailCommand, role: MaterialChannelRole) -> bool {
    if command.asset_key == 0 {
        return false;
    }
    match role {
        MaterialChannelRole::BaseColor => (command.channel_bits & 8) != 0,
        MaterialChannelRole::Height => (command.channel_bits & 1) != 0,
        MaterialChannelRole::Normal => (command.channel_bits & 2) != 0,
        MaterialChannelRole::Roughness
        | MaterialChannelRole::Metallic
        | MaterialChannelRole::AmbientOcclusion
        | MaterialChannelRole::Specular
        | MaterialChannelRole::Opacity => (command.channel_bits & 4) != 0,
        MaterialChannelRole::MaterialId => (command.channel_bits & 16) != 0,
        MaterialChannelRole::EdgeMask => (command.channel_bits & 32) != 0,
        MaterialChannelRole::RegionId => false,
    }
}

fn pack_detail_command(
    region: &CompiledRegionCommandV1,
    detail: &hot_trimmer_effect_compiler::CompiledDetail,
    operation: Option<&hot_trimmer_effect_compiler::StampOperation>,
    stroke_sample_m: Option<[f64; 2]>,
    requested_map: Option<MaterialMapKind>,
) -> GpuDetailCommand {
    let dst = region.destination_rect.0;
    let period = operation
        .map(|operation| operation.spacing_m)
        .filter(|period| period.iter().all(|value| *value > 0.0))
        .or(detail.repeat_period_m)
        .unwrap_or(detail.physical_size_m);
    let channels = detail.definition.channels.iter().chain(
        operation
            .into_iter()
            .flat_map(|operation| &operation.channels),
    );
    let mut height_amount = 0.0_f32;
    let mut normal_amount = 0.0_f32;
    let mut scalar_amount = 0.0_f32;
    let mut color_amount = 0.0_f32;
    let mut material_id = 0_u32;
    let mut channel_bits = 0_u32;
    for channel in channels {
        if !requested_map.is_none_or(|map| detail_channel_matches_map(channel.channel, map)) {
            continue;
        }
        channel_bits |= detail_channel_bits(std::slice::from_ref(channel));
        match channel.channel {
            MaterialChannelRole::Height => height_amount += channel.amount as f32,
            MaterialChannelRole::Normal => normal_amount += channel.amount as f32,
            MaterialChannelRole::Roughness
            | MaterialChannelRole::Metallic
            | MaterialChannelRole::AmbientOcclusion
            | MaterialChannelRole::Specular
            | MaterialChannelRole::Opacity => scalar_amount += channel.amount as f32,
            MaterialChannelRole::BaseColor => color_amount += channel.amount as f32,
            MaterialChannelRole::MaterialId | MaterialChannelRole::RegionId => {
                material_id = channel.material_id.unwrap_or(0);
            }
            MaterialChannelRole::EdgeMask => {}
        }
    }
    let rotation = if detail.definition.orientation
        == hot_trimmer_effect_compiler::DetailOrientation::ExplicitDegrees
    {
        detail.definition.explicit_rotation_degrees
    } else {
        operation.map_or(0.0, |op| op.rotation_degrees)
    }
    .to_radians();
    let position = stroke_sample_m.unwrap_or_else(|| {
        operation
            .map(|operation| operation.physical_position_m)
            .unwrap_or([0.0, 0.0])
    });
    let size = operation
        .map(|operation| operation.physical_size_m)
        .unwrap_or(detail.physical_size_m);
    let mirror = operation.map_or([false, false], |operation| operation.mirror);
    let asset_key = operation
        .map(|operation| stable_u32_from_digest(&operation.asset.digest))
        .or_else(|| {
            detail
                .definition
                .required_sources
                .first()
                .map(|asset| stable_u32_from_digest(&asset.digest))
        })
        .unwrap_or(0);
    GpuDetailCommand {
        family: detail_family_code(detail.resolved_family),
        lod: detail_lod_code(detail.lod),
        mapping_mode: detail_mapping_code(detail.definition.mapping_mode),
        channel_bits,
        dst_x: dst.x,
        dst_y: dst.y,
        dst_width: dst.width,
        dst_height: dst.height,
        seed: operation.map_or(detail.definition.seed, |op| op.seed) as u32,
        layer_order: operation.map_or(0, |op| op.layer_order),
        occupancy_relation: operation.map_or(4, |op| occupancy_relation_code(op.occupancy)),
        blend: operation.map_or(0, |op| stamp_blend_code(op.blend)),
        material_id,
        mirror_bits: u32::from(mirror[0]) | (u32::from(mirror[1]) << 1),
        clipping: operation.map_or(detail_fit_code(detail.definition.fit_policy), |op| {
            detail_fit_code(op.clipping)
        }),
        asset_key,
        asset_layer: u32::MAX,
        asset_scalar_layer: u32::MAX,
        asset_normal_layer: u32::MAX,
        asset_material_id_layer: u32::MAX,
        asset_mask_layer: u32::MAX,
        asset_width: 1,
        asset_height: 1,
        slot_width_m: detail.slot_size_m[0] as f32,
        slot_height_m: detail.slot_size_m[1] as f32,
        pixels_per_meter_x: detail.pixels_per_meter[0] as f32,
        pixels_per_meter_y: detail.pixels_per_meter[1] as f32,
        size_x_m: size[0] as f32,
        size_y_m: size[1] as f32,
        position_x_m: position[0] as f32,
        position_y_m: position[1] as f32,
        pivot_x: operation.map_or(0.5, |op| op.pivot[0]) as f32,
        pivot_y: operation.map_or(0.5, |op| op.pivot[1]) as f32,
        period_x_m: period[0] as f32,
        period_y_m: period[1] as f32,
        scatter: operation.map_or(0.0, |op| op.scatter) as f32,
        jitter_x_m: operation.map_or(0.0, |op| op.jitter_m[0]) as f32,
        jitter_y_m: operation.map_or(0.0, |op| op.jitter_m[1]) as f32,
        rotation_sin: (rotation as f32).sin(),
        rotation_cos: (rotation as f32).cos(),
        opacity: operation.map_or(1.0, |op| op.opacity) as f32,
        height_amount,
        normal_amount,
        scalar_amount,
        color_amount,
    }
}

fn stable_u32_from_digest(digest: &ContentDigest) -> u32 {
    let bytes = digest.0.as_bytes();
    let mut hash = 0x811c_9dc5_u32;
    for byte in bytes {
        hash = (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193);
    }
    hash
}

fn stamp_blend_code(blend: hot_trimmer_effect_compiler::StampBlendPolicy) -> u32 {
    match blend {
        hot_trimmer_effect_compiler::StampBlendPolicy::Replace => 0,
        hot_trimmer_effect_compiler::StampBlendPolicy::Add => 1,
        hot_trimmer_effect_compiler::StampBlendPolicy::Multiply => 2,
        hot_trimmer_effect_compiler::StampBlendPolicy::Max => 3,
    }
}

fn detail_fit_code(fit: hot_trimmer_effect_compiler::DetailFitPolicy) -> u32 {
    match fit {
        hot_trimmer_effect_compiler::DetailFitPolicy::Contain => 0,
        hot_trimmer_effect_compiler::DetailFitPolicy::Cover => 1,
        hot_trimmer_effect_compiler::DetailFitPolicy::Repeat => 2,
        hot_trimmer_effect_compiler::DetailFitPolicy::FailIfOversized => 3,
    }
}

fn encode_profile_header(header: GpuProfileHeader) -> [u8; GPU_PROFILE_HEADER_BYTES] {
    let mut bytes = [0_u8; GPU_PROFILE_HEADER_BYTES];
    for (index, value) in [
        header.output_width,
        header.output_height,
        header.tile_x,
        header.tile_y,
        header.tile_width,
        header.tile_height,
        header.command_count,
        header._pad,
    ]
    .into_iter()
    .enumerate()
    {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn encode_detail_commands(commands: &[GpuDetailCommand]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(commands.len() * GPU_DETAIL_COMMAND_BYTES);
    for command in commands {
        for value in [
            command.family,
            command.lod,
            command.mapping_mode,
            command.channel_bits,
            command.dst_x,
            command.dst_y,
            command.dst_width,
            command.dst_height,
            command.seed,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&command.layer_order.to_le_bytes());
        for value in [
            command.occupancy_relation,
            command.blend,
            command.material_id,
            command.mirror_bits,
            command.clipping,
            command.asset_key,
            command.asset_layer,
            command.asset_scalar_layer,
            command.asset_normal_layer,
            command.asset_material_id_layer,
            command.asset_mask_layer,
            command.asset_width,
            command.asset_height,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in [
            command.slot_width_m,
            command.slot_height_m,
            command.pixels_per_meter_x,
            command.pixels_per_meter_y,
            command.size_x_m,
            command.size_y_m,
            command.position_x_m,
            command.position_y_m,
            command.pivot_x,
            command.pivot_y,
            command.period_x_m,
            command.period_y_m,
            command.scatter,
            command.jitter_x_m,
            command.jitter_y_m,
            command.rotation_sin,
            command.rotation_cos,
            command.opacity,
            command.height_amount,
            command.normal_amount,
            command.scalar_amount,
            command.color_amount,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    debug_assert_eq!(bytes.len(), commands.len() * GPU_DETAIL_COMMAND_BYTES);
    bytes
}

fn encode_profile_commands(commands: &[GpuProfileCommand]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(commands.len() * GPU_PROFILE_COMMAND_BYTES);
    for command in commands {
        for value in [
            command.program,
            command.lod,
            command.supersampling,
            command.occupancy_bits,
            command.dst_x,
            command.dst_y,
            command.dst_width,
            command.dst_height,
            command.edge_mask,
            command.curve_offset,
            command.curve_count,
            command.sdf_kind,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in [
            command.slot_width_m,
            command.slot_height_m,
            command.pixels_per_meter_x,
            command.pixels_per_meter_y,
            command.first_width_m,
            command.second_width_m,
            command.minimum_flat_center_m,
            command.amplitude_m,
            command.angle_radians,
            command.inner_radius_m,
            command.outer_radius_m,
            command._pad_float,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    debug_assert_eq!(bytes.len(), commands.len() * GPU_PROFILE_COMMAND_BYTES);
    bytes
}

fn encode_profile_curves(curves: &[[f32; 2]]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(curves.len() * 8);
    for point in curves {
        bytes.extend_from_slice(&point[0].to_le_bytes());
        bytes.extend_from_slice(&point[1].to_le_bytes());
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
            command.slice_left,
            command.slice_right,
            command.slice_top,
            command.slice_bottom,
            command.slice_center,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        COMPILED_ATLAS_ALGORITHM_VERSION, COMPILED_ATLAS_PLAN_SCHEMA_VERSION,
        CompiledAtlasPreviewProfile, CompiledColorSpacePolicy, CompiledNormalConvention,
        CompiledTileRequest, CompiledTileRequestKind, SourcePixelRect,
    };
    use hot_trimmer_domain::{
        DocumentHash, EdgeEligibility, ManualRegionRole, MappingTransform, PixelSize, QuarterTurn,
        RadialMappingSettings, RegionContinuity, RegionSampling, SamplingMode, SamplingPolicy,
        StructuralProfile, TemplateSlotRole,
    };
    use hot_trimmer_image_io::{CategoryId, LinearScalar, NormalAlphaPolicy, TangentNormal};
    use hot_trimmer_placement_solver::{
        CandidateDescriptors, CandidateFamily, CandidateRoute, CandidateTransform, CropCandidate,
        EligibilityEvidence, PositionStrategy, SliceGeometry, SourceCrop,
        StretchOverrideProvenance,
    };
    use hot_trimmer_preview::{
        GPU_CAPABILITY_CONTRACT_VERSION, GpuCapabilityRecord, PINNED_WGPU_VERSION,
        TextureFormatCapability,
    };

    fn gpu_tiled_export_caps(max_texture_dimension_2d: u32) -> GpuCapabilityRecord {
        GpuCapabilityRecord {
            contract_version: GPU_CAPABILITY_CONTRACT_VERSION,
            service_generation: 1,
            wgpu_version: PINNED_WGPU_VERSION,
            adapter_name: "bounded export fixture".into(),
            vendor: 0,
            device: 0,
            backend: "test".into(),
            driver: "test".into(),
            driver_info: "test".into(),
            maximum_texture_dimension_2d: max_texture_dimension_2d,
            maximum_sampled_textures_per_stage: 16,
            maximum_storage_textures_per_stage: 8,
            timestamp_queries: false,
            clear_texture: true,
            copy_bytes_per_row_alignment: 256,
            uniform_buffer_offset_alignment: 256,
            storage_buffer_offset_alignment: 256,
            recommended_tile_size: 2048,
            candidate_formats: vec![
                TextureFormatCapability {
                    format: "Rgba8UnormSrgb".into(),
                    sampled: true,
                    storage: true,
                },
                TextureFormatCapability {
                    format: "Rgba8Unorm".into(),
                    sampled: true,
                    storage: true,
                },
                TextureFormatCapability {
                    format: "R32Float".into(),
                    sampled: true,
                    storage: true,
                },
                TextureFormatCapability {
                    format: "R32Uint".into(),
                    sampled: true,
                    storage: true,
                },
            ],
        }
    }

    fn gpu_tiled_export_sampling_plan(
        region_id: RegionId,
        source_id: ContentDigest,
        crop: SourceCrop,
    ) -> SamplingPlan {
        SamplingPlan {
            slot_id: region_id,
            role: TemplateSlotRole::Planar,
            variation_group: "gpu-tiled-export".into(),
            prepared_domain_dimensions: [crop.width, crop.height],
            candidate: CropCandidate {
                candidate_id: ContentDigest::sha256(b"gpu-tiled-export-candidate"),
                source_id: source_id.clone(),
                domain_id: ContentDigest::sha256(b"gpu-tiled-export-domain"),
                slot_id: region_id,
                crop: Some(crop),
                transform: CandidateTransform {
                    rotation: QuarterTurn::Zero,
                    mirror: MirrorTransform::None,
                },
                isotropic_scale: 1.0,
                mapping_mode: SamplingMode::DirectCrop,
                family: CandidateFamily::PanelDirect,
                route: CandidateRoute::Direct,
                position_strategy: PositionStrategy::DenseLowResolution,
                period_pixels: None,
                seam_indices: Vec::new(),
                correspondence_reference: ContentDigest::sha256(b"gpu-tiled-export-domain"),
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
                    reasons: vec!["gpu tiled export scheduler fixture".into()],
                },
            },
            sampling_basis: hot_trimmer_placement_solver::SamplingBasis::SelectedCrop,
            slot_physical_size: [f64::from(crop.width), f64::from(crop.height)],
            source_pixels_per_physical_unit: 1.0,
            sampling_policy: SamplingPolicy::default(),
            radial_mapping: None,
            stretch_override: StretchOverrideProvenance::NotAuthorized,
            slice_geometry: SliceGeometry::None,
            maximum_seam_cost_milli: 0,
            unary_cost: 0.0,
        }
    }

    fn gpu_tiled_export_plan() -> CompiledAtlasPlanV1 {
        let source_set_id = SourceSetId::new();
        let source_id = ContentDigest::sha256(b"gpu-tiled-export-source");
        let region_id = RegionId::new();
        let source_crop = PixelBounds {
            x: 0,
            y: 0,
            width: 8192,
            height: 8192,
        };
        let sampling_plan = gpu_tiled_export_sampling_plan(
            region_id,
            source_id.clone(),
            SourceCrop {
                x: source_crop.x,
                y: source_crop.y,
                width: source_crop.width,
                height: source_crop.height,
            },
        );
        CompiledAtlasPlanV1 {
            schema_version: COMPILED_ATLAS_PLAN_SCHEMA_VERSION,
            algorithm_version: COMPILED_ATLAS_ALGORITHM_VERSION.into(),
            document_revision: 42,
            request_generation: Some(99),
            topology_hash: DocumentHash([0x11; 32]),
            appearance_hash: DocumentHash([0x22; 32]),
            output_size: PixelSize {
                width: 8192,
                height: 8192,
            },
            preview_profile: CompiledAtlasPreviewProfile::Authoritative,
            normal_convention: CompiledNormalConvention::OpenGl,
            color_space_policy: CompiledColorSpacePolicy::SrgbColorUnassociatedAlpha,
            tile_request: CompiledTileRequest {
                kind: CompiledTileRequestKind::ExactViewport,
                generation: 99,
                output_rect: OutputPixelRect(PixelBounds {
                    x: 0,
                    y: 0,
                    width: 8192,
                    height: 8192,
                }),
                mip_level: 0,
                halo_px: 0,
                valid_rect: OutputPixelRect(PixelBounds {
                    x: 0,
                    y: 0,
                    width: 8192,
                    height: 8192,
                }),
            },
            requested_maps: vec![MaterialMapKind::BaseColor, MaterialMapKind::Height],
            ordered_sources: vec![CompiledSourceCommandV1 {
                source_set_id,
                source_id: source_id.clone(),
                digest: ContentDigest::sha256(b"gpu-tiled-export-source-bytes"),
                oriented_dimensions: hot_trimmer_domain::OrientedPixelSize {
                    width: 8192,
                    height: 8192,
                },
                decoder_version: "decoder-fixture".into(),
                decoded_format: "rgba8".into(),
                color_version: "color-fixture".into(),
                channel_role: MaterialChannelRole::BaseColor,
            }],
            ordered_regions: vec![CompiledRegionCommandV1 {
                region_id,
                compact_index: 0,
                region_role: ManualRegionRole::Panel,
                source_set_id,
                source_id: source_id.clone(),
                patch_id: None,
                source_crop: SourcePixelRect(source_crop),
                destination_rect: OutputPixelRect(source_crop),
                sampling: RegionSampling::OneShot,
                source_to_region_transform: MappingTransform::default(),
                radial_parameters: None,
                structural_profile: StructuralProfile::Bevel,
                compiled_profile: crate::compile_profile_for_region(
                    StructuralProfile::Bevel,
                    &sampling_plan,
                    source_crop,
                    &ContentDigest::sha256(b"gpu-tiled-export-profile"),
                )
                .expect("profile fixture should compile"),
                compiled_details: hot_trimmer_effect_compiler::empty_compiled_detail_set(),
                continuity: RegionContinuity::None,
                padding_px: 0,
                edge_eligibility: EdgeEligibility::default(),
                edge_detail: None,
                edge_wear: None,
                sampling_plan,
                render_cache_key: ContentDigest::sha256(b"gpu-tiled-export-region-render"),
            }],
            final_plan_hash: ContentDigest(String::new()),
        }
        .finalize()
        .expect("fixture plan should validate")
    }

    fn gpu_tiled_export_resized_plan(edge: u32) -> CompiledAtlasPlanV1 {
        let mut plan = gpu_tiled_export_plan();
        let bounds = PixelBounds {
            x: 0,
            y: 0,
            width: edge,
            height: edge,
        };
        plan.output_size = PixelSize {
            width: edge,
            height: edge,
        };
        plan.tile_request.output_rect = OutputPixelRect(bounds);
        plan.tile_request.valid_rect = OutputPixelRect(bounds);
        plan.ordered_sources[0].oriented_dimensions.width = edge;
        plan.ordered_sources[0].oriented_dimensions.height = edge;
        let region = &mut plan.ordered_regions[0];
        region.source_crop = SourcePixelRect(bounds);
        region.destination_rect = OutputPixelRect(bounds);
        region.sampling_plan.prepared_domain_dimensions = [edge, edge];
        region.sampling_plan.candidate.crop = Some(SourceCrop {
            x: 0,
            y: 0,
            width: edge,
            height: edge,
        });
        region.sampling_plan.slot_physical_size = [f64::from(edge), f64::from(edge)];
        region.compiled_profile = crate::compile_profile_for_region(
            region.structural_profile,
            &region.sampling_plan,
            bounds,
            &ContentDigest::sha256(b"gpu-tiled-export-resized-profile"),
        )
        .expect("resized profile fixture should compile");
        plan.final_plan_hash = ContentDigest(String::new());
        plan.finalize()
            .expect("resized fixture plan should validate")
    }

    fn read_cached_profile_field(
        handle: &hot_trimmer_preview::GpuDeviceState,
        cache: &Mutex<GpuAtlasSourceTextureCache>,
        plan: &CompiledAtlasPlanV1,
        map: MaterialMapKind,
        shader: &str,
    ) -> Arc<[u8]> {
        let identity = if shader.starts_with("edge-detail-") {
            edge_detail_identity(plan, map, shader)
        } else {
            plan.tile_identity(map, shader)
        };
        let texture = cache
            .lock()
            .unwrap()
            .cached_rendered_texture(&identity)
            .expect("profile field must remain GPU resident");
        let tile = plan.tile_request.output_rect.0;
        let mut encoder = handle
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("stage15-profile-field-readback"),
            });
        let pending = schedule_readback(
            handle.device(),
            cache,
            &mut encoder,
            &texture._texture,
            tile.width,
            tile.height,
            4,
        )
        .expect("bounded profile readback should schedule");
        handle.queue().submit(Some(encoder.finish()));
        finish_readback(handle.device(), pending)
            .expect("profile field readback")
            .0
    }

    fn r32_pixel(bytes: &[u8], width: u32, x: u32, y: u32) -> f32 {
        let offset = ((y * width + x) * 4) as usize;
        f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    fn u32_pixel(bytes: &[u8], width: u32, x: u32, y: u32) -> u32 {
        let offset = ((y * width + x) * 4) as usize;
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    fn run_single_profile_fixture(
        gpu: &hot_trimmer_preview::GpuCapabilityService,
        handle: &hot_trimmer_preview::GpuDeviceState,
        requested: &hot_trimmer_effect_compiler::RequestedProfile,
        upstream: &[u8],
    ) -> (
        hot_trimmer_effect_compiler::CompiledProfile,
        Arc<[u8]>,
        Arc<[u8]>,
        Arc<[u8]>,
        Arc<[u8]>,
        Arc<[u8]>,
    ) {
        let mut plan = gpu_tiled_export_resized_plan(64);
        plan.ordered_regions[0].compiled_profile = hot_trimmer_effect_compiler::compile_profile(
            hot_trimmer_effect_compiler::ProfileCompileRequest {
                requested,
                slot_size_m: [64.0, 64.0],
                destination_pixels: [64, 64],
                capacity: &hot_trimmer_effect_compiler::conservative_profile_capacity([64.0, 64.0]),
                upstream_identity: &ContentDigest::sha256(upstream),
            },
        )
        .expect("single profile fixture should compile");
        let profile = plan.ordered_regions[0].compiled_profile.clone();
        plan.final_plan_hash = ContentDigest(String::new());
        plan = plan
            .finalize()
            .expect("single profile fixture plan should validate");
        let cache = Mutex::new(GpuAtlasSourceTextureCache::default());
        let executor = GpuAtlasRenderExecutor {
            service: gpu,
            source_texture_cache: &cache,
        };
        let cancellation = CancellationToken::new();
        execute_or_load_profile_fields(
            &executor,
            &plan,
            handle.device(),
            handle.queue(),
            &cancellation,
            &|| true,
        )
        .expect("single profile fixture should execute on GPU");
        (
            profile,
            read_cached_profile_field(
                handle,
                &cache,
                &plan,
                MaterialMapKind::Height,
                "stage15-profile-height-v1",
            ),
            read_cached_profile_field(
                handle,
                &cache,
                &plan,
                MaterialMapKind::Roughness,
                "stage15-profile-sdf-v2",
            ),
            read_cached_profile_field(
                handle,
                &cache,
                &plan,
                MaterialMapKind::AmbientOcclusion,
                "stage15-profile-semantics-v2",
            ),
            read_cached_profile_field(
                handle,
                &cache,
                &plan,
                MaterialMapKind::Metallic,
                "stage15-profile-derivative-x-v1",
            ),
            read_cached_profile_field(
                handle,
                &cache,
                &plan,
                MaterialMapKind::Opacity,
                "stage15-profile-derivative-y-v1",
            ),
        )
    }

    fn stage16_asset(label: &str) -> hot_trimmer_effect_compiler::StampAssetRef {
        hot_trimmer_effect_compiler::StampAssetRef {
            asset_id: format!("asset-{label}"),
            version: "1.0.0".into(),
            digest: ContentDigest::sha256(label.as_bytes()),
            kind: "RegisteredStampChannels".into(),
        }
    }

    fn stage16_prepared_asset(
        label: &str,
        index: usize,
    ) -> (hot_trimmer_domain::SourceSetId, Arc<PreparedMaterialDomain>) {
        let (width, height) = if index.is_multiple_of(2) {
            (3, 2)
        } else {
            (5, 4)
        };
        let pixels = usize::try_from(width * height).unwrap();
        let color = ImagePlane::from_row_major(
            width,
            height,
            2,
            &vec![
                LinearColor {
                    rgb: [0.0, 1.0, 0.5],
                    alpha: 0.25,
                };
                pixels
            ],
        )
        .unwrap();
        let scalar =
            ImagePlane::from_row_major(width, height, 2, &vec![LinearScalar(0.75); pixels])
                .unwrap();
        let normal = ImagePlane::from_row_major(
            width,
            height,
            2,
            &vec![
                TangentNormal {
                    xyz: [0.2, 0.3, 0.9],
                    alpha: 0.75,
                };
                pixels
            ],
        )
        .unwrap();
        let material_id =
            ImagePlane::from_row_major(width, height, 2, &vec![CategoryId(0); pixels]).unwrap();
        let mask =
            ImagePlane::from_row_major(width, height, 2, &vec![MaskValue(1.0); pixels]).unwrap();
        let domain = PreparedMaterialDomain::from_registered_channels(
            ContentDigest::sha256(format!("stage16-domain-{label}").as_bytes()),
            ContentDigest::sha256(format!("stage16-prepared-{label}").as_bytes()),
            vec![
                PreparedExemplarChannel::BaseColor {
                    plane: color,
                    alpha_mode: hot_trimmer_image_io::ResolvedAlphaMode::Straight,
                },
                PreparedExemplarChannel::Scalar {
                    role: MaterialChannelRole::Height,
                    plane: scalar.clone(),
                },
                PreparedExemplarChannel::Scalar {
                    role: MaterialChannelRole::Roughness,
                    plane: scalar,
                },
                PreparedExemplarChannel::Normal {
                    plane: normal,
                    source_convention: hot_trimmer_domain::NormalConvention::DirectX,
                    canonical_convention: hot_trimmer_domain::NormalConvention::DirectX,
                    alpha_policy: NormalAlphaPolicy::Preserve,
                },
                PreparedExemplarChannel::MaterialId { plane: material_id },
                PreparedExemplarChannel::Mask {
                    role: MaterialChannelRole::EdgeMask,
                    plane: mask,
                },
            ],
        )
        .unwrap();
        (
            hot_trimmer_domain::SourceSetId::from_bytes([index as u8 + 1; 16]),
            Arc::new(domain),
        )
    }

    fn stage16_detail(
        family: hot_trimmer_effect_compiler::DetailFamily,
        name: &str,
        size: [f64; 2],
        period: Option<[f64; 2]>,
    ) -> hot_trimmer_effect_compiler::DetailDefinition {
        hot_trimmer_effect_compiler::DetailDefinition {
            name: name.into(),
            family,
            physical_size: size,
            scale_space: hot_trimmer_effect_compiler::EffectScaleSpace::World,
            compatible_roles: vec![
                TemplateSlotRole::Planar,
                TemplateSlotRole::RepeatingStrip,
                TemplateSlotRole::UniqueDetail,
                TemplateSlotRole::Radial,
                TemplateSlotRole::TrimCap,
            ],
            orientation: hot_trimmer_effect_compiler::DetailOrientation::Slot,
            explicit_rotation_degrees: 0.0,
            aspect_limits: [0.05, 20.0],
            minimum_pixels: [1, 1],
            repeat_period_m: period,
            fit_policy: hot_trimmer_effect_compiler::DetailFitPolicy::FailIfOversized,
            mapping_mode: if family == hot_trimmer_effect_compiler::DetailFamily::RadialDetail {
                hot_trimmer_effect_compiler::DetailMappingMode::Planar
            } else {
                hot_trimmer_effect_compiler::DetailMappingMode::Planar
            },
            channels: vec![
                hot_trimmer_effect_compiler::DetailChannelContribution {
                    channel: MaterialChannelRole::Height,
                    amount: 0.25,
                    blend: hot_trimmer_effect_compiler::StampBlendPolicy::Add,
                    material_id: None,
                    metallic_explicit: false,
                },
                hot_trimmer_effect_compiler::DetailChannelContribution {
                    channel: MaterialChannelRole::MaterialId,
                    amount: 1.0,
                    blend: hot_trimmer_effect_compiler::StampBlendPolicy::Replace,
                    material_id: Some(0),
                    metallic_explicit: false,
                },
                hot_trimmer_effect_compiler::DetailChannelContribution {
                    channel: MaterialChannelRole::BaseColor,
                    amount: 1.0,
                    blend: hot_trimmer_effect_compiler::StampBlendPolicy::Replace,
                    material_id: None,
                    metallic_explicit: false,
                },
                hot_trimmer_effect_compiler::DetailChannelContribution {
                    channel: MaterialChannelRole::Normal,
                    amount: 1.0,
                    blend: hot_trimmer_effect_compiler::StampBlendPolicy::Replace,
                    material_id: None,
                    metallic_explicit: false,
                },
                hot_trimmer_effect_compiler::DetailChannelContribution {
                    channel: MaterialChannelRole::EdgeMask,
                    amount: 1.0,
                    blend: hot_trimmer_effect_compiler::StampBlendPolicy::Replace,
                    material_id: None,
                    metallic_explicit: false,
                },
            ],
            fallback: hot_trimmer_effect_compiler::DetailFallback::Disabled,
            provenance: "stage16-test-provenance".into(),
            seed: 0x1600 + family as u64,
            required_sources: vec![stage16_asset(name)],
            required_halo_px: 2,
            dependencies: vec!["stage15-profile-occupancy".into()],
        }
    }

    fn stage16_operation(
        name: &str,
        scope: hot_trimmer_effect_compiler::StampScope,
    ) -> hot_trimmer_effect_compiler::StampOperation {
        hot_trimmer_effect_compiler::StampOperation {
            asset: stage16_asset(name),
            scope,
            target_region: name.into(),
            physical_position_m: [0.0, 0.0],
            physical_size_m: [12.0, 12.0],
            pivot: [0.5, 0.5],
            rotation_degrees: 15.0,
            mirror: [false, false],
            opacity: 1.0,
            blend: hot_trimmer_effect_compiler::StampBlendPolicy::Add,
            clipping: hot_trimmer_effect_compiler::DetailFitPolicy::FailIfOversized,
            seed: 0x16,
            spacing_m: [8.0, 8.0],
            scatter: 0.25,
            jitter_m: [0.1, 0.1],
            layer_order: 3,
            occupancy: hot_trimmer_effect_compiler::OccupancyRelation::AboveProfile,
            channels: vec![hot_trimmer_effect_compiler::DetailChannelContribution {
                channel: MaterialChannelRole::Height,
                amount: 0.25,
                blend: hot_trimmer_effect_compiler::StampBlendPolicy::Add,
                material_id: None,
                metallic_explicit: false,
            }],
        }
    }

    fn compile_stage16_fixture(
        definitions: &[hot_trimmer_effect_compiler::DetailDefinition],
        operations: &[hot_trimmer_effect_compiler::StampOperation],
        pixels: [u32; 2],
    ) -> hot_trimmer_effect_compiler::CompiledDetailSet {
        hot_trimmer_effect_compiler::compile_details(
            hot_trimmer_effect_compiler::DetailCompileRequest {
                definitions,
                operations,
                strokes: &[],
                slot_role: TemplateSlotRole::Planar,
                slot_size_m: [64.0, 64.0],
                destination_pixels: pixels,
                capacity: &hot_trimmer_effect_compiler::conservative_profile_capacity([64.0, 64.0]),
                upstream_identity: &ContentDigest::sha256(b"stage16-detail-fixture"),
            },
        )
        .expect("Stage 16 fixture should compile")
    }

    fn read_cached_detail_field(
        handle: &hot_trimmer_preview::GpuDeviceState,
        cache: &Mutex<GpuAtlasSourceTextureCache>,
        plan: &CompiledAtlasPlanV1,
        map: MaterialMapKind,
        shader: &str,
    ) -> Arc<[u8]> {
        let mut identity = plan.tile_identity(map, shader);
        identity.pixel_format = match map {
            MaterialMapKind::BaseColor => crate::CompiledTilePixelFormat::Rgba8UnormSrgb,
            MaterialMapKind::Normal => crate::CompiledTilePixelFormat::Rgba8UnormLinear,
            MaterialMapKind::MaterialId => crate::CompiledTilePixelFormat::R32Uint,
            _ => crate::CompiledTilePixelFormat::R32Float,
        };
        let texture = cache
            .lock()
            .unwrap()
            .cached_rendered_texture(&identity)
            .expect("detail field must remain GPU resident");
        let tile = plan.tile_request.output_rect.0;
        let mut encoder = handle
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("stage16-detail-field-readback"),
            });
        let pending = schedule_readback(
            handle.device(),
            cache,
            &mut encoder,
            &texture._texture,
            tile.width,
            tile.height,
            4,
        )
        .expect("bounded detail readback should schedule");
        handle.queue().submit(Some(encoder.finish()));
        finish_readback(handle.device(), pending)
            .expect("detail field readback")
            .0
    }

    fn assert_close(actual: f32, expected: f32, tolerance: f32, label: &str) {
        assert!(
            (actual - expected).abs() <= tolerance,
            "{label}: actual={actual} expected={expected} tolerance={tolerance}"
        );
    }

    fn gpu_tiled_export_three_source_plan() -> CompiledAtlasPlanV1 {
        let mut plan = gpu_tiled_export_resized_plan(8192);
        let template = plan.ordered_sources[0].clone();
        for index in 1..3 {
            let mut source = template.clone();
            source.source_set_id = SourceSetId::new();
            source.source_id =
                ContentDigest::sha256(format!("gpu-tiled-export-source-{index}").as_bytes());
            source.digest =
                ContentDigest::sha256(format!("gpu-tiled-export-source-bytes-{index}").as_bytes());
            plan.ordered_sources.push(source);
        }
        plan.final_plan_hash = ContentDigest(String::new());
        plan.finalize()
            .expect("multi-source fixture plan should validate")
    }

    fn gpu_tiled_export_loop_x_plan() -> CompiledAtlasPlanV1 {
        let mut plan = gpu_tiled_export_resized_plan(257);
        let region = &mut plan.ordered_regions[0];
        region.sampling = RegionSampling::LoopX;
        region.continuity = RegionContinuity::X;
        region.sampling_plan.candidate.mapping_mode = SamplingMode::RepeatX;
        region.sampling_plan.candidate.family = CandidateFamily::RepeatXSegment;
        region.sampling_plan.candidate.route = CandidateRoute::Repeat;
        region.sampling_plan.candidate.period_pixels = Some([128, 128]);
        plan.final_plan_hash = ContentDigest(String::new());
        plan.finalize().expect("loop fixture plan should validate")
    }

    fn gpu_tiled_export_polar_radial_plan() -> CompiledAtlasPlanV1 {
        let mut plan = gpu_tiled_export_resized_plan(1024);
        let radial = RadialMappingSettings {
            center_x: 0.5,
            center_y: 0.5,
            inner_radius: 0.1,
            outer_radius: 0.5,
            falloff: 1.0,
            blend_width: 0.05,
            seam_blend_width: 0.1,
        };
        let region = &mut plan.ordered_regions[0];
        region.region_role = ManualRegionRole::Radial;
        region.radial_parameters = Some(radial);
        region.sampling_plan.radial_mapping = Some(radial);
        region.sampling_plan.candidate.mapping_mode = SamplingMode::PolarRadial;
        region.sampling_plan.candidate.family = CandidateFamily::PolarRadialSynthesis;
        region.sampling_plan.candidate.route = CandidateRoute::PolarRadial;
        plan.final_plan_hash = ContentDigest(String::new());
        plan.finalize()
            .expect("polar radial fixture plan should validate")
    }

    fn gpu_tiled_export_planar_radial_plan() -> CompiledAtlasPlanV1 {
        let mut plan = gpu_tiled_export_polar_radial_plan();
        let region = &mut plan.ordered_regions[0];
        region.sampling_plan.candidate.mapping_mode = SamplingMode::PlanarRadial;
        region.sampling_plan.candidate.family = CandidateFamily::PlanarRadialSquare;
        region.sampling_plan.candidate.route = CandidateRoute::PlanarRadial;
        plan.final_plan_hash = ContentDigest(String::new());
        plan.finalize()
            .expect("planar radial fixture plan should validate")
    }

    fn gpu_tiled_export_offset_polar_radial_plan() -> CompiledAtlasPlanV1 {
        let mut plan = gpu_tiled_export_polar_radial_plan();
        let region = &mut plan.ordered_regions[0];
        region.source_crop = SourcePixelRect(PixelBounds {
            x: 128,
            y: 96,
            width: 512,
            height: 512,
        });
        region.sampling_plan.candidate.crop = Some(SourceCrop {
            x: 128,
            y: 96,
            width: 512,
            height: 512,
        });
        region.source_to_region_transform.offset = [0.6, 0.5];
        plan.final_plan_hash = ContentDigest(String::new());
        plan.finalize()
            .expect("offset polar radial fixture plan should validate")
    }

    #[test]
    fn gpu_tiled_export_readback_pool_counts_checked_out_staging() {
        let mut pool = GpuAtlasReadbackPool::new(4);
        pool.byte_capacity = 12;

        pool.reserve_staging_bytes(5)
            .expect("first pending readback should fit");
        pool.reserve_staging_bytes(4)
            .expect("second pending readback should fit");
        let error = pool
            .reserve_staging_bytes(4)
            .expect_err("third pending readback must honor aggregate checked-out budget");
        assert!(
            error.to_string().contains("in-flight staging budget"),
            "{error}"
        );

        pool.release_staging_bytes(5);
        pool.reserve_staging_bytes(3)
            .expect("released staging budget should become available");
    }

    #[test]
    fn gpu_tiled_export_normal_never_falls_back_to_base_color() {
        let mut plan = gpu_tiled_export_plan();
        let missing_normal_source = plan.ordered_sources[0].clone();
        let mut unrelated_authored_normal = missing_normal_source.clone();
        unrelated_authored_normal.source_set_id = SourceSetId::new();
        unrelated_authored_normal.source_id =
            ContentDigest::sha256(b"gpu-tiled-export-unrelated-normal-source");
        unrelated_authored_normal.channel_role = MaterialChannelRole::Normal;
        plan.ordered_sources.push(unrelated_authored_normal);

        assert_eq!(
            source_channel_role_for_source(&plan, &missing_normal_source, MaterialMapKind::Normal),
            None,
            "a mixed-source authored Normal pass must skip sources without an actual Normal channel"
        );
        assert_eq!(
            source_channel_role_for_source(
                &plan,
                &missing_normal_source,
                MaterialMapKind::Roughness
            ),
            Some(MaterialChannelRole::BaseColor),
            "non-Normal maps may still use Base Color only as an explicit default signal"
        );

        let mut same_source_authored_normal = missing_normal_source.clone();
        same_source_authored_normal.channel_role = MaterialChannelRole::Normal;
        plan.ordered_sources.push(same_source_authored_normal);
        assert_eq!(
            source_channel_role_for_source(&plan, &missing_normal_source, MaterialMapKind::Normal),
            Some(MaterialChannelRole::Normal)
        );
    }

    #[test]
    fn gpu_tiled_export_schedule_uses_compiled_plan_and_blocks_monolithic_executor() {
        let plan = gpu_tiled_export_plan();
        let requested_maps = requested_material_maps(&plan).expect("requested maps");
        let schedule =
            schedule_compiled_export_tiles(&plan, &requested_maps, &gpu_tiled_export_caps(4096))
                .expect("compiled export schedule");

        assert_eq!(requested_maps, plan.requested_maps);
        assert!(schedule.output_tiles.len() > requested_maps.len());
        assert!(schedule.source_tiles.iter().any(|tile| {
            tile.rect.width < tile.source.oriented_dimensions.width
                && tile.rect.height < tile.source.oriented_dimensions.height
        }));

        let base_color = schedule
            .output_tiles
            .iter()
            .find(|tile| tile.identity.map == MaterialMapKind::BaseColor)
            .expect("base color tile");
        assert_eq!(
            base_color.identity.pixel_format,
            CompiledTilePixelFormat::Rgba8UnormSrgb
        );
        assert_eq!(base_color.bit_depth, 8);
        assert_eq!(base_color.color_space, "sRGB");
        assert_eq!(base_color.identity.output_rect, base_color.output_rect);
        assert_eq!(base_color.identity.valid_rect, base_color.valid_rect);

        let height = schedule
            .output_tiles
            .iter()
            .find(|tile| tile.identity.map == MaterialMapKind::Height)
            .expect("height tile");
        assert_eq!(
            height.identity.pixel_format,
            CompiledTilePixelFormat::R32Float
        );
        assert_eq!(height.bit_depth, 32);
        assert_eq!(height.color_space, "linear");

        assert!(schedule.output_tiles.iter().any(|tile| {
            tile.footprints.iter().any(|footprint| {
                footprint.source_rect.width < plan.output_size.width
                    && footprint.source_rect.height < plan.output_size.height
                    && !footprint.required_source_tiles.is_empty()
            })
        }));

        let error =
            ensure_schedule_publishable_by_current_executor(&plan, &schedule, &requested_maps)
                .expect_err("current executor must not silently publish a tiled export schedule");
        assert!(error.to_string().contains("multi-output-tile streaming"));

        let large_plan = gpu_tiled_export_resized_plan(24_576);
        let large_maps = requested_material_maps(&large_plan).expect("large requested maps");
        let large_schedule = schedule_compiled_export_tiles(
            &large_plan,
            &large_maps,
            &gpu_tiled_export_caps(32_768),
        )
        .expect("large compiled export schedule");
        assert!(large_schedule.output_tiles.len() > large_maps.len());
        let error = ensure_schedule_publishable_by_current_executor(
            &large_plan,
            &large_schedule,
            &large_maps,
        )
        .expect_err("budget-forbidden 24K schedule must not reach monolithic execution");
        assert!(error.to_string().contains("multi-output-tile streaming"));

        let mut large_viewport_plan = large_plan.clone();
        let tile = PixelBounds {
            x: 0,
            y: 0,
            width: 4096,
            height: 4096,
        };
        let source_bounds = PixelBounds {
            x: 0,
            y: 0,
            width: 4096,
            height: 4096,
        };
        large_viewport_plan.tile_request.output_rect = OutputPixelRect(tile);
        large_viewport_plan.tile_request.valid_rect = OutputPixelRect(tile);
        large_viewport_plan.ordered_sources[0]
            .oriented_dimensions
            .width = source_bounds.width;
        large_viewport_plan.ordered_sources[0]
            .oriented_dimensions
            .height = source_bounds.height;
        let region = &mut large_viewport_plan.ordered_regions[0];
        region.source_crop = SourcePixelRect(source_bounds);
        region.sampling_plan.prepared_domain_dimensions =
            [source_bounds.width, source_bounds.height];
        region.sampling_plan.candidate.crop = Some(SourceCrop {
            x: source_bounds.x,
            y: source_bounds.y,
            width: source_bounds.width,
            height: source_bounds.height,
        });
        region.sampling_plan.slot_physical_size = [
            f64::from(source_bounds.width),
            f64::from(source_bounds.height),
        ];
        large_viewport_plan.final_plan_hash = ContentDigest(String::new());
        large_viewport_plan = large_viewport_plan
            .finalize()
            .expect("large exact viewport fixture plan");
        let large_viewport_maps =
            requested_material_maps(&large_viewport_plan).expect("large viewport requested maps");
        let large_viewport_schedule = schedule_compiled_export_tiles(
            &large_viewport_plan,
            &large_viewport_maps,
            &gpu_tiled_export_caps(32_768),
        )
        .expect("large viewport schedule");
        ensure_schedule_publishable_by_current_executor(
            &large_viewport_plan,
            &large_viewport_schedule,
            &large_viewport_maps,
        )
        .expect("bounded exact viewport tile should not be rejected by full-atlas output budget");

        let mut all_map_viewport_plan = large_viewport_plan.clone();
        all_map_viewport_plan.requested_maps = vec![
            MaterialMapKind::BaseColor,
            MaterialMapKind::Height,
            MaterialMapKind::Normal,
            MaterialMapKind::Roughness,
            MaterialMapKind::Metallic,
            MaterialMapKind::AmbientOcclusion,
            MaterialMapKind::RegionId,
        ];
        all_map_viewport_plan.final_plan_hash = ContentDigest(String::new());
        all_map_viewport_plan = all_map_viewport_plan
            .finalize()
            .expect("all-map viewport fixture plan");
        let all_map_viewport_maps =
            requested_material_maps(&all_map_viewport_plan).expect("all-map requested maps");
        let all_map_viewport_schedule = schedule_compiled_export_tiles(
            &all_map_viewport_plan,
            &all_map_viewport_maps,
            &gpu_tiled_export_caps(32_768),
        )
        .expect("all-map viewport schedule");
        assert!(
            current_executor_tile_residency_bytes(&all_map_viewport_plan, &all_map_viewport_maps)
                .expect("current tile residency")
                > all_map_viewport_schedule.output_monolithic_budget_bytes
        );
        let error = ensure_schedule_publishable_by_current_executor(
            &all_map_viewport_plan,
            &all_map_viewport_schedule,
            &all_map_viewport_maps,
        )
        .expect_err("multi-map tile residency must be admitted as one concurrent working set");
        assert!(error.to_string().contains("multi-output-tile streaming"));

        let multi_source_plan = gpu_tiled_export_three_source_plan();
        let multi_source_maps =
            requested_material_maps(&multi_source_plan).expect("multi-source requested maps");
        let multi_source_schedule = schedule_compiled_export_tiles(
            &multi_source_plan,
            &multi_source_maps,
            &gpu_tiled_export_caps(32_768),
        )
        .expect("multi-source compiled export schedule");
        assert!(
            multi_source_schedule
                .source_monolithic_bytes
                .iter()
                .all(|bytes| *bytes <= multi_source_schedule.source_monolithic_budget_bytes)
        );
        assert!(
            multi_source_schedule
                .source_monolithic_bytes
                .iter()
                .copied()
                .sum::<u64>()
                > multi_source_schedule.source_monolithic_budget_bytes
        );
        assert!(multi_source_schedule.source_tiles.iter().any(|tile| {
            tile.rect.width < tile.source.oriented_dimensions.width
                || tile.rect.height < tile.source.oriented_dimensions.height
        }));
        ensure_schedule_publishable_by_current_executor(
            &multi_source_plan,
            &multi_source_schedule,
            &multi_source_maps,
        )
        .expect("source-tile upload schedules should be admitted by the current GPU executor");
    }

    #[test]
    fn gpu_tiled_export_loop_and_radial_footprints_cover_wrapped_extrema() {
        let loop_plan = gpu_tiled_export_loop_x_plan();
        let loop_region = &loop_plan.ordered_regions[0];
        let loop_footprints = compiled_region_source_footprints(
            loop_region,
            PixelRect {
                x: 0,
                y: 0,
                width: 257,
                height: 257,
            },
            257,
            257,
        )
        .expect("loop footprint");
        assert!(loop_footprints.iter().any(|rect| rect.width >= 128));

        for radial_plan in [
            gpu_tiled_export_planar_radial_plan(),
            gpu_tiled_export_polar_radial_plan(),
            gpu_tiled_export_offset_polar_radial_plan(),
        ] {
            let radial_region = &radial_plan.ordered_regions[0];
            let radial_footprints = compiled_region_source_footprints(
                radial_region,
                PixelRect {
                    x: 511,
                    y: 511,
                    width: 2,
                    height: 2,
                },
                1024,
                1024,
            )
            .expect("radial footprint");
            assert_eq!(
                radial_footprints,
                vec![PixelRect {
                    x: 0,
                    y: 0,
                    width: 1024,
                    height: 1024
                }]
            );
        }

        let radial_plan = gpu_tiled_export_polar_radial_plan();
        let radial_footprints = compiled_region_source_footprints(
            &radial_plan.ordered_regions[0],
            PixelRect {
                x: 0,
                y: 0,
                width: 1024,
                height: 1024,
            },
            1024,
            1024,
        )
        .expect("radial footprint");
        assert!(
            radial_footprints
                .iter()
                .any(|rect| rect.width >= 1024 && rect.height >= 1024)
        );
    }

    #[test]
    fn gpu_execution_radial_footprint_is_bounded_to_output_subtile() {
        let radial_plan = gpu_tiled_export_polar_radial_plan();
        let mut command =
            pack_command(&radial_plan.ordered_regions[0]).expect("radial command should pack");
        command.crop_x = 0;
        command.crop_y = 0;
        command.crop_width = 4096;
        command.crop_height = 4096;
        command.dst_x = 0;
        command.dst_y = 0;
        command.dst_width = 4096;
        command.dst_height = 4096;
        command.semantic_x = 0;
        command.semantic_y = 0;
        command.semantic_width = 4096;
        command.semantic_height = 4096;
        command.slot_width = 4096.0;
        command.slot_height = 4096.0;

        let rects = command_source_footprint_rects(
            &command,
            PixelRect {
                x: 3968,
                y: 2016,
                width: 64,
                height: 64,
            },
            4096,
            4096,
        );

        assert!(
            !rects.is_empty(),
            "radial subtile should produce at least one resident source footprint"
        );
        assert!(
            rects
                .iter()
                .all(|rect| rect.width < command.crop_width && rect.height < command.crop_height),
            "radial execution footprint must stay bounded to the output subtile instead of the whole crop: {rects:?}"
        );
        let plan = resident_source_page_plan_for_commands(
            std::slice::from_ref(&command),
            PixelRect {
                x: 3968,
                y: 2016,
                width: 64,
                height: 64,
            },
            4096,
            4096,
            512,
            256,
            4,
            ExportMemoryBudgets::default().gpu_source_residency_bytes,
        )
        .expect("sparse radial resident plan");
        let dense_page_count = plan
            .source_rect
            .width
            .div_ceil(plan.interior_width)
            .saturating_mul(plan.source_rect.height.div_ceil(plan.interior_height));
        assert!(
            plan.layer_count() < dense_page_count,
            "radial seam resident plan must preserve sparse pages instead of densifying the bounding rect; layers={}, dense_page_count={}, plan={plan:?}",
            plan.layer_count(),
            dense_page_count
        );
    }

    #[test]
    fn resident_source_page_plan_rejects_byte_over_budget_footprint() {
        let error = resident_source_page_plan(
            &[PixelRect {
                x: 0,
                y: 0,
                width: 32,
                height: 32,
            }],
            32,
            32,
            64,
            16,
            1 << 30,
            ExportMemoryBudgets::default().gpu_source_residency_bytes,
            1,
        )
        .expect_err("resident page plan must reject a byte-over-budget footprint");
        assert!(
            error
                .to_string()
                .contains("cannot fit resident source pages"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn resident_source_pages_are_row_major_for_dense_shader_layers() {
        let pages = required_resident_pages(
            &[PixelRect {
                x: 0,
                y: 0,
                width: 20,
                height: 20,
            }],
            PixelRect {
                x: 0,
                y: 0,
                width: 20,
                height: 20,
            },
            10,
            10,
        );
        assert_eq!(
            pages,
            vec![
                GpuResidentSourcePage { x: 0, y: 0 },
                GpuResidentSourcePage { x: 1, y: 0 },
                GpuResidentSourcePage { x: 0, y: 1 },
                GpuResidentSourcePage { x: 1, y: 1 },
            ]
        );
    }

    #[test]
    fn source_texture_reservations_count_against_budget_until_dropped() {
        let cache = Mutex::new(GpuAtlasSourceTextureCache::default());
        let budget = ExportMemoryBudgets::default().gpu_source_residency_bytes;
        let key = GpuSourceTextureKey {
            source_set_id: SourceSetId::from_bytes([1; 16]),
            source_id: ContentDigest::sha256(b"reservation-source"),
            digest: ContentDigest::sha256(b"reservation-digest"),
            origin_x: 0,
            origin_y: 0,
            width: 1,
            height: 1,
            decoded_format: "rgba8".into(),
            decoder_version: "test".into(),
            color_version: "test".into(),
            channel_role: MaterialChannelRole::BaseColor,
            page_interior_width: 1,
            page_interior_height: 1,
            page_halo: 0,
            page_mode: 0,
            page_table_hash: 0,
        };
        let reservation = reserve_source_texture_cache_space(
            &cache,
            key.clone(),
            budget,
            1,
            u32::try_from(budget).unwrap_or(u32::MAX),
        )
        .expect("first reservation should consume the source budget");
        assert_eq!(cache.lock().unwrap().source_reserved_bytes(), budget);
        let second = match reserve_source_texture_cache_space(&cache, key, 1, 1, 1) {
            Ok(_) => panic!("second reservation must see in-flight reserved bytes"),
            Err(error) => error,
        };
        assert!(
            second.to_string().contains("cannot reserve"),
            "unexpected error: {second}"
        );
        drop(reservation);
        assert_eq!(cache.lock().unwrap().source_reserved_bytes(), 0);
    }

    #[test]
    fn source_texture_pinned_lease_blocks_eviction_until_drop() {
        let gpu = hot_trimmer_preview::GpuCapabilityService::default();
        let handle = gpu
            .initialize()
            .expect("GPU service should initialize for source cache pin test");
        let device = handle.device();
        let cache = Mutex::new(GpuAtlasSourceTextureCache::default());
        let budget = ExportMemoryBudgets::default().gpu_source_residency_bytes;
        let key_a = test_source_texture_key(b"pinned-source-a");
        let mut key_b = test_source_texture_key(b"pinned-source-b");
        key_b.digest = ContentDigest::sha256(b"pinned-digest-b");

        let reservation_a = reserve_source_texture_cache_space(
            &cache,
            key_a.clone(),
            budget,
            1,
            u32::try_from(budget).unwrap_or(u32::MAX),
        )
        .expect("source A should reserve the whole source budget");
        let (_cached_a, lease_a) = reservation_a
            .commit(
                key_a,
                test_cached_source_texture(device, budget, "pinned-source-a"),
            )
            .expect("source A reservation should commit into a pinned lease");

        {
            let guard = cache.lock().unwrap();
            assert_eq!(guard.source_reserved_bytes(), 0);
            assert_eq!(guard.source_resident_bytes(), budget);
            assert_eq!(guard.source_pinned_count(), 1);
        }

        let blocked = match reserve_source_texture_cache_space(&cache, key_b.clone(), 1, 1, 1) {
            Ok(_) => panic!("source B must not evict checked-out source A"),
            Err(error) => error,
        };
        assert!(
            blocked.to_string().contains("cannot reserve"),
            "unexpected error: {blocked}"
        );
        {
            let guard = cache.lock().unwrap();
            assert_eq!(
                guard
                    .source_resident_bytes()
                    .saturating_add(guard.source_reserved_bytes()),
                budget
            );
            assert_eq!(guard.source_pinned_count(), 1);
        }

        drop(lease_a);
        let reservation_b = reserve_source_texture_cache_space(&cache, key_b, 1, 1, 1)
            .expect("source B should reserve after source A's lease drops");
        {
            let guard = cache.lock().unwrap();
            assert_eq!(guard.source_reserved_bytes(), 1);
            assert_eq!(guard.source_resident_bytes(), 0);
            assert_eq!(guard.source_pinned_count(), 0);
        }
        drop(reservation_b);
        assert_eq!(cache.lock().unwrap().source_reserved_bytes(), 0);
    }

    #[test]
    fn same_key_commits_share_canonical_texture_before_third_reservation() {
        let gpu = hot_trimmer_preview::GpuCapabilityService::default();
        let handle = gpu
            .initialize()
            .expect("GPU service should initialize for source cache canonicalization test");
        let device = handle.device();
        let cache = Mutex::new(GpuAtlasSourceTextureCache::default());
        let budget = ExportMemoryBudgets::default().gpu_source_residency_bytes;
        let texture_bytes = budget / 2;
        let key = test_source_texture_key(b"same-key-source");
        let key_c = test_source_texture_key(b"third-source");

        let reservation_a = reserve_source_texture_cache_space(
            &cache,
            key.clone(),
            texture_bytes,
            1,
            u32::try_from(texture_bytes).unwrap_or(u32::MAX),
        )
        .expect("first same-key reservation should fit");
        let reservation_b = reserve_source_texture_cache_space(
            &cache,
            key.clone(),
            texture_bytes,
            1,
            u32::try_from(texture_bytes).unwrap_or(u32::MAX),
        )
        .expect("second same-key reservation should fit while both uploads are in flight");

        let (cached_a, lease_a) = reservation_a
            .commit(
                key.clone(),
                test_cached_source_texture(device, texture_bytes, "same-key-source-a"),
            )
            .expect("first same-key commit should publish the source texture");
        let duplicate = test_cached_source_texture(device, texture_bytes, "same-key-source-b");
        let duplicate_weak = Arc::downgrade(&duplicate);
        let (cached_b, lease_b) = reservation_b
            .commit(key.clone(), duplicate)
            .expect("second same-key commit should lease the canonical source texture");

        assert!(
            Arc::ptr_eq(&cached_a, &cached_b),
            "same-key commits must share the canonical cached texture"
        );
        assert!(
            duplicate_weak.upgrade().is_none(),
            "duplicate same-key upload must be dropped instead of becoming unaccounted live residency"
        );
        {
            let guard = cache.lock().unwrap();
            assert_eq!(guard.source_reserved_bytes(), 0);
            assert_eq!(guard.source_resident_bytes(), texture_bytes);
            assert_eq!(guard.source_pins.get(&key).copied(), Some(2));
        }

        let reservation_c = reserve_source_texture_cache_space(
            &cache,
            key_c,
            texture_bytes,
            1,
            u32::try_from(texture_bytes).unwrap_or(u32::MAX),
        )
        .expect("third reservation should fit because duplicate same-key texture was dropped");
        {
            let guard = cache.lock().unwrap();
            assert_eq!(
                guard
                    .source_resident_bytes()
                    .saturating_add(guard.source_reserved_bytes()),
                budget
            );
            assert_eq!(guard.source_pins.get(&key).copied(), Some(2));
        }

        drop(reservation_c);
        drop(lease_a);
        assert_eq!(
            cache.lock().unwrap().source_pins.get(&key).copied(),
            Some(1)
        );
        drop(lease_b);
        assert_eq!(cache.lock().unwrap().source_pinned_count(), 0);
    }

    fn test_source_texture_key(name: &[u8]) -> GpuSourceTextureKey {
        GpuSourceTextureKey {
            source_set_id: SourceSetId::from_bytes([1; 16]),
            source_id: ContentDigest::sha256(name),
            digest: ContentDigest::sha256(b"pinned-digest"),
            origin_x: 0,
            origin_y: 0,
            width: 1,
            height: 1,
            decoded_format: "rgba8".into(),
            decoder_version: "test".into(),
            color_version: "test".into(),
            channel_role: MaterialChannelRole::BaseColor,
            page_interior_width: 1,
            page_interior_height: 1,
            page_halo: 0,
            page_mode: 0,
            page_table_hash: 0,
        }
    }

    fn test_cached_source_texture(
        device: &wgpu::Device,
        byte_len: u64,
        label: &'static str,
    ) -> Arc<GpuCachedSourceTexture> {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("pinned-source-view"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            array_layer_count: Some(1),
            ..Default::default()
        });
        let validity_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pinned-source-validity"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let validity_view = validity_texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("pinned-source-validity-view"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            array_layer_count: Some(1),
            ..Default::default()
        });
        Arc::new(GpuCachedSourceTexture {
            _texture: texture,
            view,
            _validity_texture: validity_texture,
            validity_view,
            byte_len,
            layer_count: 1,
            last_used: 0,
        })
    }

    #[test]
    fn algorithm_stage_15_gpu_profiles_match_oracle_cache_and_cancellation() {
        let gpu = hot_trimmer_preview::GpuCapabilityService::default();
        let handle = gpu
            .initialize()
            .expect("GPU service should initialize for Stage 15 profile parity");
        let cache = Mutex::new(GpuAtlasSourceTextureCache::default());
        let executor = GpuAtlasRenderExecutor {
            service: &gpu,
            source_texture_cache: &cache,
        };
        let plan = gpu_tiled_export_resized_plan(64);
        let cancellation = CancellationToken::new();
        let stats = execute_or_load_profile_fields(
            &executor,
            &plan,
            handle.device(),
            handle.queue(),
            &cancellation,
            &|| true,
        )
        .expect("compiled profile GPU pass should execute");
        assert_eq!(stats.dispatches, 1);
        assert_eq!(stats.command_count, 1);
        assert!(!stats.cache_hit);

        let bytes = read_cached_profile_field(
            &handle,
            &cache,
            &plan,
            MaterialMapKind::Height,
            "stage15-profile-height-v1",
        );
        let pixel = |x: u32, y: u32| {
            let offset = ((y * 64 + x) * 4) as usize;
            f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
        };
        let profile = &plan.ordered_regions[0].compiled_profile;
        let oracle = |distance_m: f32| {
            let width = profile.first_width_m as f32;
            let t = (distance_m / width).clamp(0.0, 1.0);
            profile.amplitude_m as f32 * (2.0 * t - t * t)
        };
        assert!((pixel(0, 32) - oracle(0.5)).abs() <= 2.0e-5);
        assert!((pixel(32, 32) - oracle(31.5)).abs() <= 2.0e-5);

        let cached = execute_or_load_profile_fields(
            &executor,
            &plan,
            handle.device(),
            handle.queue(),
            &cancellation,
            &|| true,
        )
        .expect("identical plan should hit GPU field cache");
        assert!(cached.cache_hit);
        assert_eq!(cached.dispatches, 0);

        let cancelled_cache = Mutex::new(GpuAtlasSourceTextureCache::default());
        let cancelled_executor = GpuAtlasRenderExecutor {
            service: &gpu,
            source_texture_cache: &cancelled_cache,
        };
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert!(matches!(
            execute_or_load_profile_fields(
                &cancelled_executor,
                &plan,
                handle.device(),
                handle.queue(),
                &cancelled,
                &|| true,
            ),
            Err(AtlasRenderExecutionError::Cancelled)
        ));
        assert_eq!(cancelled_cache.lock().unwrap().rendered_textures.len(), 0);
    }

    #[test]
    fn algorithm_stage_15_gpu_profiles_cover_programs_resolutions_and_fallbacks() {
        use hot_trimmer_effect_compiler::{
            CustomProfilePoint, ProfileCompileRequest, ProfileFallback, ProfileLegalityPolicy,
            ProfileLength, ProfileLod, ProfileLodPolicy, ProfileProgram, RequestedProfile,
            compile_profile, conservative_profile_capacity,
        };
        let programs = [
            ProfileProgram::Flat,
            ProfileProgram::ConvexBevel,
            ProfileProgram::ConcaveGroove,
            ProfileProgram::RoundedBevel,
            ProfileProgram::DoubleBevel,
            ProfileProgram::RaisedLip,
            ProfileProgram::RecessedSeam,
            ProfileProgram::PanelFrame,
            ProfileProgram::FullyRoundedStrip,
            ProfileProgram::MergedOpposingBevel,
            ProfileProgram::RadialDisc,
            ProfileProgram::Annulus,
            ProfileProgram::CustomCurve,
        ];
        for edge in [1024, 2048, 4096, 8192] {
            for program in programs {
                let mut requested =
                    RequestedProfile::from_structural_intent(StructuralProfile::Bevel, 0x15);
                requested.program = program;
                if program == ProfileProgram::Annulus {
                    requested.inner_radius = ProfileLength::RelativeMinor(0.10);
                    requested.outer_radius = ProfileLength::RelativeMinor(0.40);
                }
                requested.custom_curve = if program == ProfileProgram::CustomCurve {
                    vec![
                        CustomProfilePoint {
                            position: 0.0,
                            height: 0.0,
                        },
                        CustomProfilePoint {
                            position: 0.5,
                            height: 1.0,
                        },
                        CustomProfilePoint {
                            position: 1.0,
                            height: 0.0,
                        },
                    ]
                } else {
                    Vec::new()
                };
                let compiled = compile_profile(ProfileCompileRequest {
                    requested: &requested,
                    slot_size_m: [4.0, 0.25],
                    destination_pixels: [edge, edge / 16],
                    capacity: &conservative_profile_capacity([4.0, 0.25]),
                    upstream_identity: &ContentDigest::sha256(b"stage15-resolution-fixture"),
                })
                .expect("broad, strip, radial, cap, and curve fixtures compile");
                assert_eq!(compiled.first_width_m, 0.03125);
                assert_eq!(compiled.amplitude_m, 0.015625);
                assert_eq!(compiled.seed, 0x15);
                assert!(matches!(compiled.supersampling, 1 | 2 | 4 | 8));
            }
        }

        let gpu = hot_trimmer_preview::GpuCapabilityService::default();
        let handle = gpu
            .initialize()
            .expect("GPU service should initialize for complete Stage 15 profile coverage");
        for program in programs {
            let mut requested =
                RequestedProfile::from_structural_intent(StructuralProfile::Bevel, 0x515);
            requested.program = program;
            if program == ProfileProgram::Annulus {
                requested.inner_radius = ProfileLength::RelativeMinor(0.10);
                requested.outer_radius = ProfileLength::RelativeMinor(0.40);
            }
            requested.custom_curve = if program == ProfileProgram::CustomCurve {
                vec![
                    CustomProfilePoint {
                        position: 0.0,
                        height: 0.0,
                    },
                    CustomProfilePoint {
                        position: 0.5,
                        height: 1.0,
                    },
                    CustomProfilePoint {
                        position: 1.0,
                        height: 0.0,
                    },
                ]
            } else {
                Vec::new()
            };
            let mut plan = gpu_tiled_export_resized_plan(64);
            plan.ordered_regions[0].compiled_profile = compile_profile(ProfileCompileRequest {
                requested: &requested,
                slot_size_m: [64.0, 64.0],
                destination_pixels: [64, 64],
                capacity: &conservative_profile_capacity([64.0, 64.0]),
                upstream_identity: &ContentDigest::sha256(b"stage15-gpu-program-fixture"),
            })
            .expect("GPU profile fixture should compile");
            plan.final_plan_hash = ContentDigest(String::new());
            plan = plan
                .finalize()
                .expect("profile fixture plan should validate");
            let cache = Mutex::new(GpuAtlasSourceTextureCache::default());
            let executor = GpuAtlasRenderExecutor {
                service: &gpu,
                source_texture_cache: &cache,
            };
            let cancellation = CancellationToken::new();
            let stats = execute_or_load_profile_fields(
                &executor,
                &plan,
                handle.device(),
                handle.queue(),
                &cancellation,
                &|| true,
            )
            .expect("each Stage 15 profile program should execute on GPU");
            assert_eq!(stats.command_count, 1, "{program:?}");
            let height_bytes = read_cached_profile_field(
                &handle,
                &cache,
                &plan,
                MaterialMapKind::Height,
                "stage15-profile-height-v1",
            );
            let semantics_bytes = read_cached_profile_field(
                &handle,
                &cache,
                &plan,
                MaterialMapKind::AmbientOcclusion,
                "stage15-profile-semantics-v2",
            );
            let edge_height = r32_pixel(&height_bytes, 64, 0, 32);
            let center_semantics = r32_pixel(&semantics_bytes, 64, 32, 32);
            assert!(edge_height.is_finite(), "{program:?}");
            assert!(center_semantics.is_finite(), "{program:?}");
            if program == ProfileProgram::Flat {
                assert_eq!(edge_height, 0.0);
                assert_eq!(center_semantics, 0.0);
            }
        }

        let smooth = |value: f32| {
            let x = value.clamp(0.0, 1.0);
            x * x * (3.0 - 2.0 * x)
        };
        let radial_probe = |x: u32, y: u32| {
            let px = x as f32 + 0.5 - 32.0;
            let py = y as f32 + 0.5 - 32.0;
            let radius = (px * px + py * py).sqrt();
            (radius, px / radius, py / radius)
        };

        let mut radial = RequestedProfile::from_structural_intent(StructuralProfile::Bevel, 0x616);
        radial.program = ProfileProgram::RadialDisc;
        radial.first_width = ProfileLength::Meters(6.0);
        radial.outer_radius = ProfileLength::Meters(20.0);
        radial.maximum_supersampling = 1;
        let (profile, height, sdf, semantics, dx, _) =
            run_single_profile_fixture(&gpu, &handle, &radial, b"stage15-radial-oracle");
        let (radius, radial_x, _) = radial_probe(51, 32);
        let distance = profile.outer_radius_m as f32 - radius;
        let t = distance / profile.first_width_m as f32;
        let ramp = smooth(t);
        let ramp_derivative = 6.0 * t.clamp(0.0, 1.0) * (1.0 - t.clamp(0.0, 1.0));
        assert_close(
            r32_pixel(&height, 64, 51, 32),
            profile.amplitude_m as f32 * ramp,
            2.0e-5,
            "radial height",
        );
        assert_close(r32_pixel(&sdf, 64, 51, 32), distance, 2.0e-5, "radial sdf");
        assert_close(
            r32_pixel(&semantics, 64, 51, 32),
            69.0,
            0.0,
            "radial semantics",
        );
        assert_close(
            r32_pixel(&dx, 64, 51, 32),
            -radial_x * profile.amplitude_m as f32 * ramp_derivative / profile.first_width_m as f32,
            2.0e-5,
            "radial derivative-x",
        );

        let mut annulus = RequestedProfile::from_structural_intent(StructuralProfile::Bevel, 0x617);
        annulus.program = ProfileProgram::Annulus;
        annulus.first_width = ProfileLength::Meters(4.0);
        annulus.second_width = ProfileLength::Meters(2.0);
        annulus.inner_radius = ProfileLength::Meters(16.0);
        annulus.outer_radius = ProfileLength::Meters(24.0);
        annulus.maximum_supersampling = 1;
        let (profile, height, sdf, semantics, _, _) =
            run_single_profile_fixture(&gpu, &handle, &annulus, b"stage15-annulus-oracle");
        let (outer_radius, _, _) = radial_probe(55, 32);
        let outer_distance = profile.outer_radius_m as f32 - outer_radius;
        assert_close(
            r32_pixel(&height, 64, 55, 32),
            profile.amplitude_m as f32 * smooth(outer_distance / profile.first_width_m as f32),
            2.0e-5,
            "annulus outer height uses first width",
        );
        let (inner_radius, _, _) = radial_probe(48, 32);
        let inner_distance = inner_radius - profile.inner_radius_m as f32;
        assert_close(
            r32_pixel(&height, 64, 48, 32),
            profile.amplitude_m as f32 * smooth(inner_distance / profile.second_width_m as f32),
            2.0e-5,
            "annulus inner height uses second width",
        );
        assert_close(
            r32_pixel(&sdf, 64, 48, 32),
            inner_distance,
            2.0e-5,
            "annulus inner sdf",
        );
        assert_close(
            r32_pixel(&semantics, 64, 48, 32),
            69.0,
            0.0,
            "annulus semantics",
        );

        let mut cap = RequestedProfile::from_structural_intent(StructuralProfile::Bevel, 0x618);
        cap.program = ProfileProgram::FullyRoundedStrip;
        cap.maximum_supersampling = 1;
        let (profile, height, _, semantics, _, _) =
            run_single_profile_fixture(&gpu, &handle, &cap, b"stage15-cap-oracle");
        let across = 0.5 / 32.0_f32;
        let rounded = (1.0 - (1.0 - across) * (1.0 - across)).sqrt();
        assert_close(
            r32_pixel(&height, 64, 0, 32),
            profile.amplitude_m as f32 * rounded,
            2.0e-5,
            "cap rounded height",
        );
        assert_close(r32_pixel(&semantics, 64, 0, 32), 85.0, 0.0, "cap semantics");

        let mut groove = RequestedProfile::from_structural_intent(StructuralProfile::Bevel, 0x619);
        groove.program = ProfileProgram::ConcaveGroove;
        groove.maximum_supersampling = 1;
        let (profile, height, _, semantics, _, _) =
            run_single_profile_fixture(&gpu, &handle, &groove, b"stage15-groove-oracle");
        assert_close(
            r32_pixel(&height, 64, 0, 32),
            -profile.amplitude_m as f32 * (1.0 - 0.5 / profile.first_width_m as f32),
            2.0e-5,
            "groove recessed height",
        );
        assert_close(
            r32_pixel(&semantics, 64, 0, 32),
            105.0,
            0.0,
            "groove semantics",
        );

        let mut custom = RequestedProfile::from_structural_intent(StructuralProfile::Bevel, 0x620);
        custom.program = ProfileProgram::CustomCurve;
        custom.maximum_supersampling = 1;
        custom.custom_curve = vec![
            CustomProfilePoint {
                position: 0.0,
                height: 0.0,
            },
            CustomProfilePoint {
                position: 0.5,
                height: 1.0,
            },
            CustomProfilePoint {
                position: 1.0,
                height: 0.0,
            },
        ];
        let (profile, height, _, _, dx, _) =
            run_single_profile_fixture(&gpu, &handle, &custom, b"stage15-custom-oracle");
        assert_close(
            r32_pixel(&height, 64, 3, 32),
            profile.amplitude_m as f32 * 0.875,
            2.0e-5,
            "custom curve height",
        );
        assert_close(
            r32_pixel(&dx, 64, 3, 32),
            profile.amplitude_m as f32 * 2.0 / profile.first_width_m as f32,
            2.0e-5,
            "custom curve derivative",
        );

        let mut normal_only =
            RequestedProfile::from_structural_intent(StructuralProfile::Bevel, 0x621);
        normal_only.lod_policy = ProfileLodPolicy::Force(ProfileLod::NormalOnly);
        normal_only.maximum_supersampling = 1;
        let (profile, height, _, _, dx, _) =
            run_single_profile_fixture(&gpu, &handle, &normal_only, b"stage15-normal-only-oracle");
        assert_close(
            r32_pixel(&height, 64, 0, 32),
            0.0,
            0.0,
            "normal-only height",
        );
        assert_close(
            r32_pixel(&dx, 64, 0, 32),
            profile.amplitude_m as f32 * (2.0 - 2.0 * (0.5 / profile.first_width_m as f32))
                / profile.first_width_m as f32,
            2.0e-5,
            "normal-only derivative remains",
        );

        let mut roughness_only =
            RequestedProfile::from_structural_intent(StructuralProfile::Bevel, 0x622);
        roughness_only.lod_policy = ProfileLodPolicy::Force(ProfileLod::RoughnessOnly);
        roughness_only.maximum_supersampling = 1;
        let (_, height, _, _, dx, _) =
            run_single_profile_fixture(&gpu, &handle, &roughness_only, b"stage15-roughness-oracle");
        assert_close(
            r32_pixel(&height, 64, 0, 32),
            0.0,
            0.0,
            "roughness-only height",
        );
        assert_close(
            r32_pixel(&dx, 64, 0, 32),
            0.0,
            0.0,
            "roughness-only derivative",
        );

        let mut disabled =
            RequestedProfile::from_structural_intent(StructuralProfile::Bevel, 0x623);
        disabled.lod_policy = ProfileLodPolicy::Force(ProfileLod::Disabled);
        disabled.maximum_supersampling = 1;
        let (_, height, sdf, semantics, dx, _) =
            run_single_profile_fixture(&gpu, &handle, &disabled, b"stage15-disabled-oracle");
        assert_close(r32_pixel(&height, 64, 0, 32), 0.0, 0.0, "disabled height");
        assert_close(r32_pixel(&sdf, 64, 0, 32), 0.0, 0.0, "disabled sdf");
        assert_close(
            r32_pixel(&semantics, 64, 0, 32),
            0.0,
            0.0,
            "disabled semantics",
        );
        assert_close(r32_pixel(&dx, 64, 0, 32), 0.0, 0.0, "disabled derivative");

        let mut asymmetric = RequestedProfile::from_structural_intent(StructuralProfile::Bevel, 9);
        asymmetric.program = ProfileProgram::DoubleBevel;
        asymmetric.first_width = ProfileLength::Meters(12.0);
        asymmetric.second_width = ProfileLength::Meters(2.0);
        let mut plan = gpu_tiled_export_resized_plan(64);
        plan.ordered_regions[0].compiled_profile = compile_profile(ProfileCompileRequest {
            requested: &asymmetric,
            slot_size_m: [64.0, 64.0],
            destination_pixels: [64, 64],
            capacity: &conservative_profile_capacity([64.0, 64.0]),
            upstream_identity: &ContentDigest::sha256(b"stage15-gpu-asymmetric-opposing"),
        })
        .expect("asymmetric opposing profile should compile");
        plan.final_plan_hash = ContentDigest(String::new());
        plan = plan
            .finalize()
            .expect("asymmetric profile fixture should validate");
        let cache = Mutex::new(GpuAtlasSourceTextureCache::default());
        let executor = GpuAtlasRenderExecutor {
            service: &gpu,
            source_texture_cache: &cache,
        };
        let cancellation = CancellationToken::new();
        execute_or_load_profile_fields(
            &executor,
            &plan,
            handle.device(),
            handle.queue(),
            &cancellation,
            &|| true,
        )
        .expect("asymmetric opposing profile should execute on GPU");
        let height_bytes = read_cached_profile_field(
            &handle,
            &cache,
            &plan,
            MaterialMapKind::Height,
            "stage15-profile-height-v1",
        );
        let left = r32_pixel(&height_bytes, 64, 0, 32);
        let right = r32_pixel(&height_bytes, 64, 63, 32);
        assert!(
            right > left * 2.0,
            "second opposing width should produce a distinct right-edge profile: left={left} right={right}"
        );

        let mut opposing = RequestedProfile::from_structural_intent(StructuralProfile::Bevel, 7);
        opposing.program = ProfileProgram::DoubleBevel;
        opposing.first_width = ProfileLength::Meters(0.18);
        opposing.second_width = ProfileLength::Meters(0.18);
        opposing.minimum_flat_center = ProfileLength::Meters(0.05);
        opposing.legality_policy = ProfileLegalityPolicy::FullyRounded;
        let rounded = compile_profile(ProfileCompileRequest {
            requested: &opposing,
            slot_size_m: [2.0, 0.25],
            destination_pixels: [8192, 1024],
            capacity: &conservative_profile_capacity([2.0, 0.25]),
            upstream_identity: &ContentDigest::sha256(b"stage15-opposing-fixture"),
        })
        .expect("opposing fallback should compile");
        assert_eq!(rounded.fallback, ProfileFallback::FullyRounded);
        assert_eq!(rounded.program, ProfileProgram::FullyRoundedStrip);
        assert!(rounded.first_width_m + rounded.second_width_m <= 0.25);

        opposing.first_width = ProfileLength::Meters(0.00001);
        opposing.second_width = ProfileLength::Meters(0.00001);
        opposing.minimum_flat_center = ProfileLength::Meters(0.001);
        opposing.legality_policy = ProfileLegalityPolicy::Clamp;
        opposing.lod_policy = ProfileLodPolicy::Auto;
        let subpixel = compile_profile(ProfileCompileRequest {
            requested: &opposing,
            slot_size_m: [2.0, 0.25],
            destination_pixels: [1024, 128],
            capacity: &conservative_profile_capacity([2.0, 0.25]),
            upstream_identity: &ContentDigest::sha256(b"stage15-subpixel-fixture"),
        })
        .expect("sub-pixel fixture should compile without widening");
        assert_eq!(subpixel.first_width_m, 0.00001);
        assert!(matches!(
            subpixel.lod,
            ProfileLod::NormalOnly | ProfileLod::RoughnessOnly | ProfileLod::Disabled
        ));
    }

    #[test]
    fn algorithm_stage_16_gpu_details() {
        use hot_trimmer_effect_compiler::{DetailFallback, DetailFamily, DetailLod, StampScope};

        let families = [
            DetailFamily::RepeatingStrip,
            DetailFamily::UniqueDetail,
            DetailFamily::RadialDetail,
            DetailFamily::TrimCap,
            DetailFamily::BoltGroup,
            DetailFamily::Vent,
            DetailFamily::PanelStamp,
            DetailFamily::Groove,
            DetailFamily::Decal,
            DetailFamily::ProceduralMotif,
            DetailFamily::UserPatch,
        ];
        let definitions = families
            .iter()
            .enumerate()
            .map(|(index, family)| {
                stage16_detail(
                    *family,
                    &format!("detail-{index}"),
                    [10.0 + index as f64, 6.0],
                    Some([8.0 + index as f64, 8.0]),
                )
            })
            .collect::<Vec<_>>();
        let reusable = stage16_operation("detail-0", StampScope::MaterialReusableAtlas);
        let mut second_reusable = stage16_operation("detail-0", StampScope::MaterialReusableAtlas);
        second_reusable.physical_position_m = [8.0, 0.0];
        second_reusable.seed = 0x17;
        let deferred = stage16_operation("detail-1", StampScope::AssetSpecificDeferred);
        let compiled_1k = compile_stage16_fixture(
            &definitions,
            &[reusable.clone(), second_reusable.clone(), deferred.clone()],
            [1024, 1024],
        );
        let compiled_8k = compile_stage16_fixture(
            &definitions,
            &[reusable, second_reusable, deferred.clone()],
            [8192, 8192],
        );
        assert_eq!(compiled_1k.details.len(), families.len());
        assert_eq!(compiled_8k.details.len(), families.len());
        assert_eq!(
            compiled_1k.details[0].physical_size_m,
            compiled_8k.details[0].physical_size_m
        );
        assert_eq!(
            compiled_1k.details[0].repeat_period_m,
            compiled_8k.details[0].repeat_period_m
        );
        assert_ne!(
            compiled_1k.details[0].pixels_per_meter,
            compiled_8k.details[0].pixels_per_meter
        );
        assert_ne!(
            detail_family_code(DetailFamily::BoltGroup),
            detail_family_code(DetailFamily::RepeatingStrip)
        );
        assert_eq!(compiled_1k.details[0].reusable_atlas_operations.len(), 2);
        assert_eq!(
            compiled_1k.details[1]
                .asset_specific_deferred_operations
                .len(),
            1
        );
        assert!(compiled_1k.details.iter().all(|detail| matches!(
            detail.lod,
            DetailLod::Full
                | DetailLod::SimplifiedHeightNormal
                | DetailLod::NormalOnly
                | DetailLod::RoughnessColor
        )));

        let serialized = serde_json::to_string(&compiled_1k.details[0]).unwrap();
        let reopened: hot_trimmer_effect_compiler::CompiledDetail =
            serde_json::from_str(&serialized).unwrap();
        assert_eq!(
            reopened.physical_size_m,
            compiled_1k.details[0].physical_size_m
        );
        assert_eq!(
            reopened.definition.seed,
            compiled_1k.details[0].definition.seed
        );
        assert_eq!(
            reopened.cache_identity,
            compiled_1k.details[0].cache_identity
        );

        let mut oversized = stage16_detail(DetailFamily::Decal, "oversized", [80.0, 80.0], None);
        oversized.fallback = DetailFallback::None;
        assert!(matches!(
            hot_trimmer_effect_compiler::compile_details(
                hot_trimmer_effect_compiler::DetailCompileRequest {
                    definitions: &[oversized],
                    operations: &[],
                    strokes: &[],
                    slot_role: TemplateSlotRole::Planar,
                    slot_size_m: [64.0, 64.0],
                    destination_pixels: [1024, 1024],
                    capacity: &hot_trimmer_effect_compiler::conservative_profile_capacity([
                        64.0, 64.0
                    ]),
                    upstream_identity: &ContentDigest::sha256(b"stage16-oversized"),
                }
            ),
            Err(hot_trimmer_effect_compiler::DetailCompileError::Incompatible)
        ));
        assert!(matches!(
            hot_trimmer_effect_compiler::compile_details(
                hot_trimmer_effect_compiler::DetailCompileRequest {
                    definitions: &[],
                    operations: &[stage16_operation(
                        "missing",
                        StampScope::MaterialReusableAtlas
                    )],
                    strokes: &[],
                    slot_role: TemplateSlotRole::Planar,
                    slot_size_m: [64.0, 64.0],
                    destination_pixels: [1024, 1024],
                    capacity: &hot_trimmer_effect_compiler::conservative_profile_capacity([
                        64.0, 64.0
                    ]),
                    upstream_identity: &ContentDigest::sha256(b"stage16-orphan"),
                }
            ),
            Err(hot_trimmer_effect_compiler::DetailCompileError::OrphanOperation)
        ));
        let mut pixel_detail =
            stage16_detail(DetailFamily::Decal, "pixel-detail", [4.0, 4.0], None);
        pixel_detail.scale_space = hot_trimmer_effect_compiler::EffectScaleSpace::Pixels;
        pixel_detail.minimum_pixels = [8, 8];
        let pixel_low = compile_stage16_fixture(&[pixel_detail.clone()], &[], [64, 64]);
        let pixel_high = compile_stage16_fixture(&[pixel_detail], &[], [1024, 1024]);
        assert_eq!(pixel_low.details[0].physical_size_m, [4.0, 4.0]);
        assert_eq!(pixel_low.details[0].lod, DetailLod::Disabled);
        assert_eq!(pixel_high.details[0].lod, DetailLod::Disabled);
        let empty = hot_trimmer_effect_compiler::compile_details(
            hot_trimmer_effect_compiler::DetailCompileRequest {
                definitions: &[],
                operations: &[],
                strokes: &[],
                slot_role: TemplateSlotRole::Planar,
                slot_size_m: [64.0, 64.0],
                destination_pixels: [1024, 1024],
                capacity: &hot_trimmer_effect_compiler::conservative_profile_capacity([64.0, 64.0]),
                upstream_identity: &ContentDigest::sha256(b"stage16-empty"),
            },
        )
        .unwrap();
        assert!(matches!(
            empty.stage_result,
            hot_trimmer_domain::StageResult::SkippedBecauseUnused { .. }
        ));

        let gpu = hot_trimmer_preview::GpuCapabilityService::default();
        let handle = gpu
            .initialize()
            .expect("GPU service should initialize for Stage 16 detail coverage");
        let mut plan = gpu_tiled_export_resized_plan(64);
        plan.ordered_regions[0].compiled_details = compile_stage16_fixture(
            &definitions,
            &[
                stage16_operation("detail-0", StampScope::MaterialReusableAtlas),
                stage16_operation("detail-1", StampScope::MaterialReusableAtlas),
                deferred,
            ],
            [64, 64],
        );
        plan.final_plan_hash = ContentDigest(String::new());
        plan = plan
            .finalize()
            .expect("Stage 16 detail plan should validate");
        let mut prepared_sources = Vec::new();
        for (index, name) in (0..definitions.len())
            .map(|index| format!("detail-{index}"))
            .enumerate()
        {
            let (source_set_id, domain) = stage16_prepared_asset(&name, index);
            for channel in domain.registered_channels() {
                prepared_sources.push(AtlasPreparedSource {
                    source_set_id,
                    source_id: stage16_asset(&name).digest,
                    channel_role: channel.role(),
                    domain: Arc::clone(&domain),
                });
            }
        }
        let execution_input = AtlasRenderExecutionInput {
            prepared_sources,
            source_frame_cache: None,
        };
        let cache = Mutex::new(GpuAtlasSourceTextureCache::default());
        let executor = GpuAtlasRenderExecutor {
            service: &gpu,
            source_texture_cache: &cache,
        };
        let cancellation = CancellationToken::new();
        let material_id_cache = Mutex::new(GpuAtlasSourceTextureCache::default());
        let material_id_executor = GpuAtlasRenderExecutor {
            service: &gpu,
            source_texture_cache: &material_id_cache,
        };
        let material_id_only = execute_or_load_detail_fields(
            &material_id_executor,
            &plan,
            Some(&execution_input),
            Some(MaterialMapKind::MaterialId),
            handle.device(),
            handle.queue(),
            &cancellation,
            &|| true,
        )
        .expect("MaterialId-only Stage 16 execution should allocate ID and validity tiles");
        assert_eq!(material_id_only.dispatches, 1);
        assert_eq!(material_id_only.asset_resident_bytes, 5 * 4 * 2 * 4 + 4);
        assert_eq!(material_id_only.asset_upload_bytes, (3 * 2 + 5 * 4) * 4);
        assert_eq!(
            material_id_only.resident_bytes,
            2 * 64 * 64 * 4 + 5 * 4 + material_id_only.asset_resident_bytes
        );
        let material_id_only_value = read_cached_detail_field(
            &handle,
            &material_id_cache,
            &plan,
            MaterialMapKind::MaterialId,
            "stage16-detail-material-id-r32uint-v2",
        );
        let material_id_only_valid = read_cached_detail_field(
            &handle,
            &material_id_cache,
            &plan,
            MaterialMapKind::MaterialId,
            "stage16-detail-material-id-valid-r32uint-v1",
        );
        assert_eq!(u32_pixel(&material_id_only_value, 64, 32, 32), 0);
        assert_eq!(u32_pixel(&material_id_only_valid, 64, 32, 32), 1);
        assert_eq!(material_id_cache.lock().unwrap().source_count(), 1);
        material_id_cache.lock().unwrap().rendered_textures.clear();
        let material_id_reloaded = execute_or_load_detail_fields(
            &material_id_executor,
            &plan,
            Some(&execution_input),
            Some(MaterialMapKind::MaterialId),
            handle.device(),
            handle.queue(),
            &cancellation,
            &|| true,
        )
        .expect("MaterialId map reevaluation should reuse the digest-keyed stamp array");
        assert_eq!(material_id_reloaded.dispatches, 1);
        assert_eq!(material_id_reloaded.asset_upload_bytes, 0);
        assert_eq!(material_id_cache.lock().unwrap().source_count(), 1);

        let stats = execute_or_load_detail_fields(
            &executor,
            &plan,
            Some(&execution_input),
            None,
            handle.device(),
            handle.queue(),
            &cancellation,
            &|| true,
        )
        .expect("Stage 16 details should execute on GPU");
        assert_eq!(stats.dispatches, 1);
        assert_eq!(stats.command_count as usize, 2);
        assert!(stats.required_halo_px >= 2);
        assert_eq!(stats.asset_resident_bytes, (5 * 4 * 2 * 4 + 4) * 5);
        assert_eq!(stats.asset_upload_bytes, (3 * 2 + 5 * 4) * 4 * 5);
        assert_eq!(
            stats.resident_bytes,
            7 * 64 * 64 * 4 + stats.asset_resident_bytes
        );
        let color = read_cached_detail_field(
            &handle,
            &cache,
            &plan,
            MaterialMapKind::BaseColor,
            "stage16-detail-base-color-rgba-v2",
        );
        let normal = read_cached_detail_field(
            &handle,
            &cache,
            &plan,
            MaterialMapKind::Normal,
            "stage16-detail-normal-rgba-v2",
        );
        let mask = read_cached_detail_field(
            &handle,
            &cache,
            &plan,
            MaterialMapKind::EdgeMask,
            "stage16-detail-mask-v1",
        );
        let height = read_cached_detail_field(
            &handle,
            &cache,
            &plan,
            MaterialMapKind::Height,
            "stage16-detail-height-v1",
        );
        let id = read_cached_detail_field(
            &handle,
            &cache,
            &plan,
            MaterialMapKind::MaterialId,
            "stage16-detail-material-id-r32uint-v2",
        );
        let id_valid = read_cached_detail_field(
            &handle,
            &cache,
            &plan,
            MaterialMapKind::MaterialId,
            "stage16-detail-material-id-valid-r32uint-v1",
        );
        let color_offset = color
            .chunks_exact(4)
            .position(|pixel| pixel[3] != 0)
            .expect("asset-backed Base Color must publish RGBA pixels")
            * 4;
        assert_eq!(&color[color_offset..color_offset + 4], &[0, 255, 188, 64]);
        let normal_offset = normal
            .chunks_exact(4)
            .position(|pixel| pixel[3] != 0)
            .expect("asset-backed Normal must publish vector pixels")
            * 4;
        assert!(
            normal[normal_offset + 1] < 128,
            "DirectX -> OpenGL must flip normal Y"
        );
        assert!(r32_pixel(&mask, 64, 0, 32).is_finite());
        assert!(r32_pixel(&height, 64, 32, 32).is_finite());
        assert!(r32_pixel(&height, 64, 8, 8) >= 0.0);
        assert_eq!(u32_pixel(&id, 64, 32, 32), 0);
        assert_eq!(u32_pixel(&id_valid, 64, 32, 32), 1);
        let cached = execute_or_load_detail_fields(
            &executor,
            &plan,
            Some(&execution_input),
            None,
            handle.device(),
            handle.queue(),
            &cancellation,
            &|| true,
        )
        .expect("identical Stage 16 detail plan should hit cache");
        assert!(cached.cache_hit);
        assert_eq!(cached.dispatches, 0);
        let pruned = execute_or_load_detail_fields(
            &executor,
            &plan,
            Some(&execution_input),
            Some(MaterialMapKind::Metallic),
            handle.device(),
            handle.queue(),
            &cancellation,
            &|| true,
        )
        .expect("unrequested absent metallic detail contribution should prune");
        assert_eq!(pruned.command_count, 0);
        assert_eq!(pruned.dispatches, 0);

        let mut empty_plan = gpu_tiled_export_resized_plan(64);
        empty_plan.ordered_regions[0].compiled_details =
            hot_trimmer_effect_compiler::empty_compiled_detail_set();
        empty_plan.final_plan_hash = ContentDigest(String::new());
        empty_plan = empty_plan
            .finalize()
            .expect("empty detail plan should validate");
        let empty_stats = execute_or_load_detail_fields(
            &executor,
            &empty_plan,
            Some(&execution_input),
            None,
            handle.device(),
            handle.queue(),
            &cancellation,
            &|| true,
        )
        .expect("empty details should dispatch no GPU work");
        assert_eq!(empty_stats.command_count, 0);
        assert_eq!(empty_stats.dispatches, 0);
    }


    #[test]
    fn native_edge_detail_gpu_fixture_covers_roles_masks_height_and_cache() {
        use hot_trimmer_effect_compiler::{
            CompiledEdgeDetailCommand, EdgeDetailRoleEvaluator,
            EdgeDetailSourceModulationRoute, EDGE_DETAIL_ALGORITHM_ID,
            EDGE_DETAIL_ALGORITHM_VERSION,
        };
        let gpu = hot_trimmer_preview::GpuCapabilityService::default();
        let handle = gpu.initialize().expect("GPU service should initialize for Edge Detail");
        let cancellation = CancellationToken::new();
        let rectangular_plan = |width: u32, height: u32, slot_size_m: [f64; 2]| {
            let mut plan = gpu_tiled_export_resized_plan(width.max(height));
            let bounds = PixelBounds { x: 0, y: 0, width, height };
            plan.output_size = PixelSize { width, height };
            plan.tile_request.output_rect = OutputPixelRect(bounds);
            plan.tile_request.valid_rect = OutputPixelRect(bounds);
            plan.ordered_sources[0].oriented_dimensions.width = width;
            plan.ordered_sources[0].oriented_dimensions.height = height;
            let region = &mut plan.ordered_regions[0];
            region.source_crop = SourcePixelRect(bounds);
            region.destination_rect = OutputPixelRect(bounds);
            region.sampling_plan.prepared_domain_dimensions = [width, height];
            region.sampling_plan.candidate.crop = Some(SourceCrop {
                x: 0, y: 0, width, height,
            });
            region.sampling_plan.slot_physical_size = slot_size_m;
            region.compiled_profile = crate::compile_profile_for_region(
                region.structural_profile,
                &region.sampling_plan,
                bounds,
                &ContentDigest::sha256(b"edge-detail-profile"),
            ).expect("Edge Detail profile fixture");
            plan
        };
        for evaluator in [
            EdgeDetailRoleEvaluator::RectangularPanel,
            EdgeDetailRoleEvaluator::HorizontalStrip,
            EdgeDetailRoleEvaluator::VerticalStrip,
            EdgeDetailRoleEvaluator::RadialOuter,
            EdgeDetailRoleEvaluator::RadialInnerOuter,
            EdgeDetailRoleEvaluator::TrimCap,
        ] {
            let mut plan = match evaluator {
                EdgeDetailRoleEvaluator::HorizontalStrip | EdgeDetailRoleEvaluator::TrimCap => {
                    rectangular_plan(256, 16, [256.0, 16.0])
                }
                EdgeDetailRoleEvaluator::VerticalStrip => {
                    rectangular_plan(16, 256, [16.0, 256.0])
                }
                _ => rectangular_plan(64, 64, [64.0, 64.0]),
            };
            plan.requested_maps = vec![MaterialMapKind::EdgeMask];
            if evaluator == EdgeDetailRoleEvaluator::HorizontalStrip {
                plan.ordered_regions[0].sampling_plan.role = TemplateSlotRole::RepeatingStrip;
                plan.ordered_regions[0].region_role = ManualRegionRole::HorizontalStrip;
            } else if evaluator == EdgeDetailRoleEvaluator::VerticalStrip {
                plan.ordered_regions[0].sampling_plan.role = TemplateSlotRole::RepeatingStrip;
                plan.ordered_regions[0].region_role = ManualRegionRole::VerticalStrip;
            } else if evaluator == EdgeDetailRoleEvaluator::TrimCap {
                plan.ordered_regions[0].sampling_plan.role = TemplateSlotRole::TrimCap;
                plan.ordered_regions[0].region_role = ManualRegionRole::HorizontalStrip;
            } else if matches!(
                evaluator,
                EdgeDetailRoleEvaluator::RadialOuter
                    | EdgeDetailRoleEvaluator::RadialInnerOuter
            ) {
                use hot_trimmer_effect_compiler::{
                    ProfileCompileRequest, ProfileLength, ProfileProgram, RequestedProfile,
                    compile_profile, conservative_profile_capacity,
                };
                let region = &mut plan.ordered_regions[0];
                region.sampling_plan.role = TemplateSlotRole::Radial;
                region.region_role = ManualRegionRole::Radial;
                region.structural_profile = if evaluator == EdgeDetailRoleEvaluator::RadialOuter {
                    StructuralProfile::RadialDisc
                } else {
                    StructuralProfile::Annulus
                };
                let mut requested = RequestedProfile::from_structural_intent(
                    region.structural_profile,
                    0xED02,
                );
                requested.program = if evaluator == EdgeDetailRoleEvaluator::RadialOuter {
                    ProfileProgram::RadialDisc
                } else {
                    ProfileProgram::Annulus
                };
                requested.first_width = ProfileLength::Meters(8.0);
                requested.second_width = ProfileLength::Meters(5.0);
                requested.inner_radius = ProfileLength::Meters(16.0);
                requested.outer_radius = ProfileLength::Meters(28.0);
                requested.maximum_supersampling = 1;
                region.compiled_profile = compile_profile(ProfileCompileRequest {
                    requested: &requested,
                    slot_size_m: [64.0, 64.0],
                    destination_pixels: [64, 64],
                    capacity: &conservative_profile_capacity([64.0, 64.0]),
                    upstream_identity: &ContentDigest::sha256(b"edge-detail-radial-profile"),
                }).expect("radial Edge Detail profile");
            }
            let region = &plan.ordered_regions[0];
            let edge = CompiledEdgeDetailCommand {
                schema_version: 1,
                region_id: region.region_id,
                role: region.sampling_plan.role,
                manual_role: region.region_role,
                structural_profile: region.structural_profile,
                slot_size_m: region.sampling_plan.slot_physical_size,
                meters_per_pixel: [
                    region.sampling_plan.slot_physical_size[0]
                        / f64::from(region.destination_rect.0.width),
                    region.sampling_plan.slot_physical_size[1]
                        / f64::from(region.destination_rect.0.height),
                ],
                edge_eligibility: region.edge_eligibility,
                evaluator,
                source_modulation_route: EdgeDetailSourceModulationRoute::None,
                source_modulation_identity: None,
                requested_physical_extent_m: 14.0,
                seed: 0xED02,
                intent_identity: ContentDigest::sha256(b"edge-detail-intent"),
                stage15_plan_identity: region.compiled_profile.cache_identity.clone(),
                requested_maps: vec![MaterialMapKind::EdgeMask, MaterialMapKind::Height],
                resolution_profile: "gpu-fixture".into(),
                lod_fallback: None,
                wear_amount: 0.8,
                intensity: 0.85,
                edge_width_m: 8.0,
                bevel_radius_m: 5.0,
                edge_softness: 0.3,
                breakup_amount: 0.7,
                breakup_scale_m: 16.0,
                micro_detail_amount: 0.25,
                micro_detail_scale_m: 2.0,
                source_height_influence: 0.0,
                source_luminance_influence: 0.0,
                height_amplitude_m: -0.35,
                normal_detail_strength: 1.0,
                hue_shift_degrees: 0.0,
                saturation_multiplier: 0.55,
                value_multiplier: 1.12,
                roughness_offset: 0.18,
                exposed_metal_enabled: false,
                metallic_offset: 0.0,
                algorithm_id: EDGE_DETAIL_ALGORITHM_ID.into(),
                algorithm_version: EDGE_DETAIL_ALGORITHM_VERSION.into(),
                cache_identity: ContentDigest::sha256(format!("{evaluator:?}").as_bytes()),
            };
            plan.ordered_regions[0].edge_detail = Some(edge);
            plan.final_plan_hash = ContentDigest(String::new());
            plan = plan.finalize().expect("valid Edge Detail fixture plan");
            let cache = Mutex::new(GpuAtlasSourceTextureCache::default());
            let executor = GpuAtlasRenderExecutor { service: &gpu, source_texture_cache: &cache };
            let stats = execute_or_load_edge_detail_fields(
                &executor, &plan, None, handle.device(), handle.queue(),
                &cancellation, &|| true,
            ).expect("Edge Detail role should execute");
            assert_eq!(stats.dispatches, 1);
            let read = |index: usize| {
                let (map, shader) = EDGE_DETAIL_FIELD_IDENTITIES[index];
                read_cached_profile_field(&handle, &cache, &plan, map, shader)
            };
            let fields = [read(0), read(1), read(2), read(3), read(4)];
            let fixture_width = plan.tile_request.output_rect.0.width;
            let fixture_height = plan.tile_request.output_rect.0.height;
            for field in &fields {
                assert_eq!(field.len(), (fixture_width * fixture_height * 4) as usize);
            }
            let values = |bytes: &[u8]| bytes.chunks_exact(4)
                .map(|pixel| f32::from_le_bytes(pixel.try_into().unwrap()))
                .collect::<Vec<_>>();
            let combined = values(&fields[3]);
            assert!(combined.iter().any(|value| *value > 0.0 && *value < 1.0));
            for field in &fields[0..4] {
                assert!(values(field).iter().any(|value| *value > 0.0));
            }
            let height_bits = values(&fields[4]).into_iter()
                .filter(|value| value.abs() > 1.0e-6)
                .map(f32::to_bits).collect::<BTreeSet<_>>();
            assert!(height_bits.len() > 3, "rounded Height profile");
            if evaluator == EdgeDetailRoleEvaluator::HorizontalStrip {
                let along = (1..255).map(|x| {
                    (r32_pixel(&fields[3], 256, x, 1)
                        - r32_pixel(&fields[3], 256, x - 1, 1)).abs()
                }).sum::<f32>() / 254.0;
                let across = (1..15).map(|y| {
                    (r32_pixel(&fields[3], 256, 128, y)
                        - r32_pixel(&fields[3], 256, 128, y - 1)).abs()
                }).sum::<f32>() / 14.0;
                assert!(along < across, "horizontal strip must correlate along X");
            }
            if evaluator == EdgeDetailRoleEvaluator::VerticalStrip {
                let along = (1..255).map(|y| {
                    (r32_pixel(&fields[3], 16, 1, y)
                        - r32_pixel(&fields[3], 16, 1, y - 1)).abs()
                }).sum::<f32>() / 254.0;
                let across = (1..15).map(|x| {
                    (r32_pixel(&fields[3], 16, x, 128)
                        - r32_pixel(&fields[3], 16, x - 1, 128)).abs()
                }).sum::<f32>() / 14.0;
                assert!(along < across, "vertical strip must correlate along Y");
            }
            if matches!(
                evaluator,
                EdgeDetailRoleEvaluator::RadialOuter
                    | EdgeDetailRoleEvaluator::RadialInnerOuter
            ) {
                let above = r32_pixel(&fields[3], 64, 4, 31);
                let below = r32_pixel(&fields[3], 64, 4, 32);
                assert!(above > 0.0 || below > 0.0, "radial seam probe must be active");
                assert!((above - below).abs() < 0.08, "radial angular seam continuity");
            }
            let cached = execute_or_load_edge_detail_fields(
                &executor, &plan, None, handle.device(), handle.queue(),
                &cancellation, &|| true,
            ).expect("repeated request should be cached");
            assert!(cached.cache_hit);
            assert_eq!(cached.dispatches, 0);
        }

        let mut multi = rectangular_plan(96, 64, [0.096, 0.064]);
        multi.requested_maps = vec![MaterialMapKind::EdgeMask];
        let base = multi.ordered_regions.remove(0);
        multi.ordered_regions = (0..3)
            .map(|index| {
                let mut region = base.clone();
                let region_id = RegionId::new();
                let rect = PixelBounds { x: index * 32, y: 0, width: 32, height: 64 };
                region.region_id = region_id;
                region.compact_index = index;
                region.source_crop = SourcePixelRect(rect);
                region.destination_rect = OutputPixelRect(rect);
                region.padding_px = if index == 2 { 3 } else { 0 };
                region.sampling_plan.slot_id = region_id;
                region.sampling_plan.candidate.slot_id = region_id;
                region.sampling_plan.candidate.crop = Some(SourceCrop {
                    x: rect.x, y: rect.y, width: rect.width, height: rect.height,
                });
                region.sampling_plan.slot_physical_size = [0.032, 0.064];
                region.compiled_profile = crate::compile_profile_for_region(
                    region.structural_profile,
                    &region.sampling_plan,
                    rect,
                    &ContentDigest::sha256(format!("edge-adjacent-{index}").as_bytes()),
                ).expect("adjacent profile");
                region.render_cache_key =
                    ContentDigest::sha256(format!("edge-adjacent-render-{index}").as_bytes());
                region
            })
            .collect();
        let mut intent = hot_trimmer_domain::EdgeDetailIntentV1::default();
        intent.source_height_influence = 0.0;
        intent.source_luminance_influence = 0.0;
        let inputs = multi.ordered_regions.iter().map(|region| {
            hot_trimmer_effect_compiler::EdgeDetailRegionInput {
                region_id: region.region_id,
                role: region.sampling_plan.role,
                manual_role: region.region_role,
                structural_profile: region.structural_profile,
                slot_size_m: region.sampling_plan.slot_physical_size,
                destination_pixels: [
                    region.destination_rect.0.width,
                    region.destination_rect.0.height,
                ],
                edge_eligibility: region.edge_eligibility,
                stage15_plan_identity: region.compiled_profile.cache_identity.clone(),
                source_height_identity: None,
                source_luminance_identity: None,
            }
        }).collect::<Vec<_>>();
        let compiled = hot_trimmer_effect_compiler::compile_edge_detail_plan(
            &hot_trimmer_effect_compiler::EdgeDetailCompileRequest {
                intent: &intent,
                regions: &inputs,
                requested_maps: &[MaterialMapKind::EdgeMask, MaterialMapKind::Height],
                resolution_profile: "global-adjacency-padding",
            },
        ).expect("global Edge Detail compile");
        for region in &mut multi.ordered_regions {
            region.edge_detail = compiled.commands.iter()
                .find(|command| command.region_id == region.region_id).cloned();
        }
        multi.final_plan_hash = ContentDigest(String::new());
        multi = multi.finalize().expect("multi-region Edge Detail fixture");
        let cache = Mutex::new(GpuAtlasSourceTextureCache::default());
        let executor = GpuAtlasRenderExecutor { service: &gpu, source_texture_cache: &cache };
        let global_stats = execute_or_load_edge_detail_fields(
            &executor, &multi, None, handle.device(), handle.queue(),
            &cancellation, &|| true,
        ).expect("global adjacency fixture");
        assert_eq!(global_stats.command_count, 3);
        let combined = read_cached_profile_field(
            &handle, &cache, &multi, MaterialMapKind::EdgeMask,
            "edge-detail-combined-r32float-v2",
        );
        for x in [1, 16, 31, 32, 47, 63, 67, 80, 92] {
            let nonzero = (0..64).any(|y| r32_pixel(&combined, 96, x, y) > 0.0);
            assert!(nonzero, "global output must reach eligible region column {x}");
        }
        for x in [64, 65, 66, 93, 94, 95] {
            for y in 0..64 {
                assert_eq!(
                    r32_pixel(&combined, 96, x, y),
                    0.0,
                    "padding pixel ({x},{y}) must remain zero",
                );
            }
        }


        let attach_source_edge = |
            plan: &mut CompiledAtlasPlanV1,
            route: hot_trimmer_effect_compiler::EdgeDetailSourceModulationRoute,
            source_identity: Option<ContentDigest>,
        | {
            let region = &plan.ordered_regions[0];
            let mut source_intent = hot_trimmer_domain::EdgeDetailIntentV1::default();
            source_intent.source_height_influence =
                if route == hot_trimmer_effect_compiler::EdgeDetailSourceModulationRoute::RegisteredHeight {
                    0.65
                } else {
                    0.0
                };
            source_intent.source_luminance_influence =
                if route == hot_trimmer_effect_compiler::EdgeDetailSourceModulationRoute::HighPassedLinearLuminance {
                    0.5
                } else {
                    0.0
                };
            let source_input = hot_trimmer_effect_compiler::EdgeDetailRegionInput {
                region_id: region.region_id,
                role: region.sampling_plan.role,
                manual_role: region.region_role,
                structural_profile: region.structural_profile,
                slot_size_m: region.sampling_plan.slot_physical_size,
                destination_pixels: [
                    region.destination_rect.0.width,
                    region.destination_rect.0.height,
                ],
                edge_eligibility: region.edge_eligibility,
                stage15_plan_identity: region.compiled_profile.cache_identity.clone(),
                source_height_identity: if route
                    == hot_trimmer_effect_compiler::EdgeDetailSourceModulationRoute::RegisteredHeight
                {
                    source_identity.clone()
                } else {
                    None
                },
                source_luminance_identity: if route
                    == hot_trimmer_effect_compiler::EdgeDetailSourceModulationRoute::HighPassedLinearLuminance
                {
                    source_identity
                } else {
                    None
                },
            };
            let compiled = hot_trimmer_effect_compiler::compile_edge_detail_plan(
                &hot_trimmer_effect_compiler::EdgeDetailCompileRequest {
                    intent: &source_intent,
                    regions: &[source_input],
                    requested_maps: &[MaterialMapKind::EdgeMask, MaterialMapKind::Height],
                    resolution_profile: "source-modulation-fixture",
                },
            ).expect("source-aware Edge Detail compile");
            plan.ordered_regions[0].edge_detail = compiled.commands.into_iter().next();
            plan.final_plan_hash = ContentDigest(String::new());
            *plan = plan.clone().finalize().expect("source-aware Edge Detail plan");
        };
        let render_combined = |
            plan: &CompiledAtlasPlanV1,
            input: Option<&AtlasRenderExecutionInput<'_>>,
        | {
            let cache = Mutex::new(GpuAtlasSourceTextureCache::default());
            let executor = GpuAtlasRenderExecutor { service: &gpu, source_texture_cache: &cache };
            let stats = execute_or_load_edge_detail_fields(
                &executor, plan, input, handle.device(), handle.queue(),
                &cancellation, &|| true,
            ).expect("source-aware Edge Detail execution");
            let bytes = read_cached_profile_field(
                &handle, &cache, plan, MaterialMapKind::EdgeMask,
                "edge-detail-combined-r32float-v2",
            );
            (bytes, stats)
        };

        let mut no_source = rectangular_plan(64, 64, [0.064, 0.064]);
        no_source.requested_maps = vec![MaterialMapKind::EdgeMask];
        attach_source_edge(
            &mut no_source,
            hot_trimmer_effect_compiler::EdgeDetailSourceModulationRoute::None,
            None,
        );
        let (no_source_bytes, _) = render_combined(&no_source, None);
        let (no_source_repeat, _) = render_combined(&no_source, None);
        assert_eq!(no_source_bytes, no_source_repeat, "same seed is byte deterministic");
        let publication_cache = Mutex::new(GpuAtlasSourceTextureCache::default());
        let publication_executor = GpuAtlasRenderExecutor {
            service: &gpu,
            source_texture_cache: &publication_cache,
        };
        let empty_input = AtlasRenderExecutionInput {
            prepared_sources: Vec::new(),
            source_frame_cache: None,
        };
        execute_edge_detail_gpu_tile(
            &publication_executor,
            &no_source,
            &empty_input,
            &cancellation,
            &|| true,
        ).expect("initial Edge Detail publication");
        let repeated_publication = execute_edge_detail_gpu_tile(
            &publication_executor,
            &no_source,
            &empty_input,
            &cancellation,
            &|| true,
        ).expect("repeated Edge Detail publication");
        let repeated_publication = repeated_publication
            .as_final_atlas()
            .expect("GPU final atlas");
        assert!(
            repeated_publication.telemetry.iter()
                .any(|line| line.contains("edge_detail_cache=CacheHit")),
            "identical publication must truthfully report CacheHit",
        );
        assert_eq!(
            repeated_publication.interactive_tile.manifest.generation,
            no_source.tile_request.generation,
        );

        let gray_pixels = vec![
            LinearColor { rgb: [0.5, 0.5, 0.5], alpha: 1.0 };
            64 * 64
        ];
        let gray_plane = ImagePlane::from_row_major(64, 64, 64, &gray_pixels).unwrap();
        let gray_domain = Arc::new(PreparedMaterialDomain::from_registered_channels(
            ContentDigest::sha256(b"constant-gray-domain"),
            ContentDigest::sha256(b"constant-gray-prepared"),
            vec![PreparedExemplarChannel::BaseColor {
                plane: gray_plane,
                alpha_mode: hot_trimmer_image_io::ResolvedAlphaMode::Straight,
            }],
        ).unwrap());
        let mut gray_plan = rectangular_plan(64, 64, [0.064, 0.064]);
        gray_plan.requested_maps = vec![MaterialMapKind::EdgeMask];
        let gray_identity = gray_plan.ordered_sources[0].digest.clone();
        attach_source_edge(
            &mut gray_plan,
            hot_trimmer_effect_compiler::EdgeDetailSourceModulationRoute::HighPassedLinearLuminance,
            Some(gray_identity),
        );
        let gray_input = AtlasRenderExecutionInput {
            prepared_sources: vec![AtlasPreparedSource {
                source_set_id: gray_plan.ordered_sources[0].source_set_id,
                source_id: gray_plan.ordered_sources[0].source_id.clone(),
                channel_role: MaterialChannelRole::BaseColor,
                domain: gray_domain,
            }],
            source_frame_cache: None,
        };
        let (gray_bytes, gray_stats) = render_combined(&gray_plan, Some(&gray_input));
        assert_eq!(
            no_source_bytes, gray_bytes,
            "constant-gray high-pass must not alter Edge Detail or bleed at bounds",
        );
        assert!(gray_stats.source_upload_bytes > 0 || gray_stats.source_cache_hits > 0);

        let height_values = (0..64 * 64)
            .map(|index| LinearScalar(((index % 64) as f32 / 63.0).clamp(0.0, 1.0)))
            .collect::<Vec<_>>();
        let height_plane = ImagePlane::from_row_major(64, 64, 64, &height_values).unwrap();
        let height_domain = Arc::new(PreparedMaterialDomain::from_registered_channels(
            ContentDigest::sha256(b"controlled-height-domain"),
            ContentDigest::sha256(b"controlled-height-prepared"),
            vec![PreparedExemplarChannel::Scalar {
                role: MaterialChannelRole::Height,
                plane: height_plane,
            }],
        ).unwrap());
        let mut height_plan = rectangular_plan(64, 64, [0.064, 0.064]);
        height_plan.requested_maps = vec![MaterialMapKind::EdgeMask];
        let mut height_source = height_plan.ordered_sources[0].clone();
        height_source.channel_role = MaterialChannelRole::Height;
        height_source.decoded_format = "r32float".into();
        height_source.digest = ContentDigest::sha256(b"controlled-height-source");
        height_plan.ordered_sources.push(height_source.clone());
        attach_source_edge(
            &mut height_plan,
            hot_trimmer_effect_compiler::EdgeDetailSourceModulationRoute::RegisteredHeight,
            Some(height_source.digest.clone()),
        );
        let height_input = AtlasRenderExecutionInput {
            prepared_sources: vec![AtlasPreparedSource {
                source_set_id: height_source.source_set_id,
                source_id: height_source.source_id,
                channel_role: MaterialChannelRole::Height,
                domain: height_domain,
            }],
            source_frame_cache: None,
        };
        let (height_source_bytes, height_source_stats) =
            render_combined(&height_plan, Some(&height_input));
        assert_ne!(
            no_source_bytes, height_source_bytes,
            "registered source Height must modulate breakup",
        );
        assert!(height_source_stats.source_upload_bytes > 0
            || height_source_stats.source_cache_hits > 0);


        let resolution_plan = |edge: u32, tile_rect: PixelBounds, seed: u32| {
            let mut plan = rectangular_plan(edge, edge, [1.0, 1.0]);
            plan.requested_maps = vec![MaterialMapKind::EdgeMask];
            plan.tile_request.output_rect = OutputPixelRect(tile_rect);
            plan.tile_request.valid_rect = OutputPixelRect(tile_rect);
            plan.tile_request.halo_px = 8;
            let region = &plan.ordered_regions[0];
            let mut resolution_intent = hot_trimmer_domain::EdgeDetailIntentV1::default();
            resolution_intent.source_height_influence = 0.0;
            resolution_intent.source_luminance_influence = 0.0;
            resolution_intent.micro_detail_scale_m = 0.008;
            resolution_intent.seed = seed;
            let input = hot_trimmer_effect_compiler::EdgeDetailRegionInput {
                region_id: region.region_id,
                role: region.sampling_plan.role,
                manual_role: region.region_role,
                structural_profile: region.structural_profile,
                slot_size_m: region.sampling_plan.slot_physical_size,
                destination_pixels: [edge, edge],
                edge_eligibility: region.edge_eligibility,
                stage15_plan_identity: region.compiled_profile.cache_identity.clone(),
                source_height_identity: None,
                source_luminance_identity: None,
            };
            let compiled = hot_trimmer_effect_compiler::compile_edge_detail_plan(
                &hot_trimmer_effect_compiler::EdgeDetailCompileRequest {
                    intent: &resolution_intent,
                    regions: &[input],
                    requested_maps: &[MaterialMapKind::EdgeMask],
                    resolution_profile: "bounded-resolution-cross-section",
                },
            ).expect("resolution Edge Detail compile");
            plan.ordered_regions[0].edge_detail = compiled.commands.into_iter().next();
            plan.final_plan_hash = ContentDigest(String::new());
            plan.finalize().expect("bounded resolution plan")
        };
        let support_width_m = |bytes: &[u8], width: u32, height: u32, mpp: f32| {
            let maximum_x = (0..width)
                .filter(|x| (0..height).any(|y| r32_pixel(bytes, width, *x, y) > 0.0))
                .max()
                .unwrap_or(0);
            (maximum_x as f32 + 1.0) * mpp
        };
        let mut measured_widths = Vec::new();
        for edge in [512, 1024, 2048, 4096] {
            let tile = PixelBounds {
                x: 0,
                y: edge / 2 - 16,
                width: 48,
                height: 32,
            };
            let plan = resolution_plan(edge, tile, 201516);
            let (bytes, _) = render_combined(&plan, None);
            let measured = support_width_m(&bytes, tile.width, tile.height, 1.0 / edge as f32);
            assert!(
                measured <= plan.ordered_regions[0].edge_detail.as_ref().unwrap()
                    .requested_physical_extent_m as f32 + 2.0 / edge as f32,
                "{edge} output exceeded compiled physical extent: {measured}",
            );
            measured_widths.push((edge, measured));
        }
        let reference_width = measured_widths[3].1;
        for (edge, measured) in measured_widths {
            let declared_tolerance_m = 2.5 / edge as f32 + 0.0015;
            assert!(
                (measured - reference_width).abs() <= declared_tolerance_m,
                "{edge} physical width {measured} differs from 4096 width {reference_width} beyond {declared_tolerance_m}",
            );
        }

        let tile_a = PixelBounds { x: 0, y: 496, width: 48, height: 32 };
        let tile_b = PixelBounds { x: 4, y: 496, width: 48, height: 32 };
        let plan_a = resolution_plan(1024, tile_a, 201516);
        let plan_b = resolution_plan(1024, tile_b, 201516);
        let (bytes_a, _) = render_combined(&plan_a, None);
        let (bytes_b, _) = render_combined(&plan_b, None);
        for y in 0..32 {
            for x in 4..48 {
                assert_eq!(
                    r32_pixel(&bytes_a, 48, x, y).to_bits(),
                    r32_pixel(&bytes_b, 48, x - 4, y).to_bits(),
                    "tile-origin overlap changed at ({x},{y})",
                );
            }
        }

        let other_seed_plan = resolution_plan(1024, tile_a, 201517);
        let (other_seed_bytes, _) = render_combined(&other_seed_plan, None);
        assert_ne!(bytes_a, other_seed_bytes, "different seed must change breakup");
        let extent_a = support_width_m(&bytes_a, 48, 32, 1.0 / 1024.0);
        let extent_b = support_width_m(&other_seed_bytes, 48, 32, 1.0 / 1024.0);
        assert!(
            (extent_a - extent_b).abs() <= 2.0 / 1024.0,
            "seed changed physical extent: {extent_a} vs {extent_b}",
        );

        let disabled = gpu_tiled_export_resized_plan(64);
        let cache = Mutex::new(GpuAtlasSourceTextureCache::default());
        let executor = GpuAtlasRenderExecutor { service: &gpu, source_texture_cache: &cache };
        let empty = execute_or_load_edge_detail_fields(
            &executor, &disabled, None, handle.device(), handle.queue(),
            &cancellation, &|| true,
        ).expect("disabled intent should be zero-work");
        assert_eq!(empty.dispatches, 0);
        assert!(cache.lock().unwrap().rendered_textures.is_empty());
    }

}
