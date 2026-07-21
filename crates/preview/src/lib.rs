#![doc = "Interactive `wgpu` compositing and PBR preview boundary."]

use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicU64, Ordering},
};

use serde::Serialize;
use thiserror::Error;

pub const PREVIEW_EXPORT_TOLERANCE_POLICY_VERSION: u16 = 1;
pub const GPU_CAPABILITY_CONTRACT_VERSION: u16 = 1;
pub const PINNED_WGPU_VERSION: &str = "26.0.1";

static NEXT_SERVICE_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextureFormatCapability {
    pub format: String,
    pub sampled: bool,
    pub storage: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuCapabilityRecord {
    pub contract_version: u16,
    pub service_generation: u64,
    pub wgpu_version: &'static str,
    pub adapter_name: String,
    pub vendor: u32,
    pub device: u32,
    pub backend: String,
    pub driver: String,
    pub driver_info: String,
    pub maximum_texture_dimension_2d: u32,
    pub maximum_sampled_textures_per_stage: u32,
    pub maximum_storage_textures_per_stage: u32,
    pub timestamp_queries: bool,
    pub clear_texture: bool,
    pub copy_bytes_per_row_alignment: u32,
    pub uniform_buffer_offset_alignment: u32,
    pub storage_buffer_offset_alignment: u32,
    pub recommended_tile_size: u32,
    pub candidate_formats: Vec<TextureFormatCapability>,
}

impl GpuCapabilityRecord {
    #[must_use]
    pub fn diagnostic_line(&self) -> String {
        format!(
            "gpu_capability_generation={}; adapter={}; backend={}; driver={}; driver_info={}; max_texture_2d={}; tile_recommendation={}; timestamp_queries={}; clear_texture={}; row_alignment={}; formats={}",
            self.service_generation,
            self.adapter_name,
            self.backend,
            self.driver,
            self.driver_info,
            self.maximum_texture_dimension_2d,
            self.recommended_tile_size,
            self.timestamp_queries,
            self.clear_texture,
            self.copy_bytes_per_row_alignment,
            self.candidate_formats
                .iter()
                .map(|format| format!(
                    "{}:sampled={},storage={}",
                    format.format, format.sampled, format.storage
                ))
                .collect::<Vec<_>>()
                .join("|")
        )
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum GpuCapabilityError {
    #[error("no supported GPU adapter is available: {reason}")]
    NoSupportedAdapter { reason: String },
    #[error("GPU device initialization failed: {reason}")]
    DeviceInitialization { reason: String },
}

pub struct GpuDeviceState {
    #[allow(dead_code)]
    instance: wgpu::Instance,
    #[allow(dead_code)]
    adapter: wgpu::Adapter,
    #[allow(dead_code)]
    device: wgpu::Device,
    #[allow(dead_code)]
    queue: wgpu::Queue,
    capabilities: GpuCapabilityRecord,
}

impl GpuDeviceState {
    #[must_use]
    pub const fn capabilities(&self) -> &GpuCapabilityRecord {
        &self.capabilities
    }

    #[must_use]
    pub const fn device(&self) -> &wgpu::Device {
        &self.device
    }

    #[must_use]
    pub const fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
}

pub const BASE_COLOR_ATLAS_WGSL: &str = include_str!("gpu_base_color.wgsl");
pub const FILL_R32_FLOAT_ATLAS_WGSL: &str = include_str!("gpu_fill_r32float.wgsl");
pub const NORMAL_FROM_HEIGHT_ATLAS_WGSL: &str = include_str!("gpu_normal_from_height.wgsl");
pub const REGION_ID_ATLAS_WGSL: &str = include_str!("gpu_region_id.wgsl");
pub const REGION_ID_DISPLAY_ATLAS_WGSL: &str = include_str!("gpu_region_id_display.wgsl");
pub const STRUCTURAL_PROFILE_ATLAS_WGSL: &str = include_str!("gpu_structural_profile.wgsl");
pub const SEMANTIC_DETAIL_ATLAS_WGSL: &str = include_str!("gpu_semantic_detail.wgsl");
pub const EDGE_DETAIL_ATLAS_WGSL: &str = include_str!("gpu_edge_detail.wgsl");
pub const EDGE_DETAIL_COMPOSITION_ATLAS_WGSL: &str = include_str!("gpu_edge_detail_composition.wgsl");
pub const SCALAR_DISPLAY_ATLAS_WGSL: &str = include_str!("gpu_scalar_display.wgsl");

/// Application-owned, one-time GPU initialization boundary. Prompt 1 only
/// reports capabilities; no pixel executor consumes this state yet.
pub struct GpuCapabilityService {
    generation: u64,
    state: OnceLock<Result<Arc<GpuDeviceState>, GpuCapabilityError>>,
}

impl Default for GpuCapabilityService {
    fn default() -> Self {
        Self {
            generation: NEXT_SERVICE_GENERATION.fetch_add(1, Ordering::Relaxed),
            state: OnceLock::new(),
        }
    }
}

impl GpuCapabilityService {
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Initializes one adapter/device/queue and reuses that exact state on
    /// every subsequent request.
    pub fn initialize(&self) -> Result<Arc<GpuDeviceState>, GpuCapabilityError> {
        self.state
            .get_or_init(|| initialize_device(self.generation))
            .clone()
    }
}

fn initialize_device(generation: u64) -> Result<Arc<GpuDeviceState>, GpuCapabilityError> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .map_err(|error| GpuCapabilityError::NoSupportedAdapter {
        reason: error.to_string(),
    })?;
    let features = adapter.features();
    let limits = adapter.limits();
    let requested_features =
        features & (wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::CLEAR_TEXTURE);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("hot-trimmer-application-gpu"),
        required_features: requested_features,
        required_limits: limits.clone(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::Off,
    }))
    .map_err(|error| GpuCapabilityError::DeviceInitialization {
        reason: error.to_string(),
    })?;
    let info = adapter.get_info();
    let candidate_formats = [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Rgba8UnormSrgb,
        wgpu::TextureFormat::R16Float,
        wgpu::TextureFormat::R32Float,
        wgpu::TextureFormat::R32Uint,
    ]
    .into_iter()
    .map(|format| {
        let capabilities = adapter.get_texture_format_features(format);
        TextureFormatCapability {
            format: format!("{format:?}"),
            sampled: capabilities
                .allowed_usages
                .contains(wgpu::TextureUsages::TEXTURE_BINDING),
            storage: capabilities
                .allowed_usages
                .contains(wgpu::TextureUsages::STORAGE_BINDING),
        }
    })
    .collect();
    let recommended_tile_size = recommended_tile_size(limits.max_texture_dimension_2d);
    Ok(Arc::new(GpuDeviceState {
        instance,
        adapter,
        device,
        queue,
        capabilities: GpuCapabilityRecord {
            contract_version: GPU_CAPABILITY_CONTRACT_VERSION,
            service_generation: generation,
            wgpu_version: PINNED_WGPU_VERSION,
            adapter_name: info.name,
            vendor: info.vendor,
            device: info.device,
            backend: format!("{:?}", info.backend),
            driver: info.driver,
            driver_info: info.driver_info,
            maximum_texture_dimension_2d: limits.max_texture_dimension_2d,
            maximum_sampled_textures_per_stage: limits.max_sampled_textures_per_shader_stage,
            maximum_storage_textures_per_stage: limits.max_storage_textures_per_shader_stage,
            timestamp_queries: requested_features.contains(wgpu::Features::TIMESTAMP_QUERY),
            clear_texture: requested_features.contains(wgpu::Features::CLEAR_TEXTURE),
            copy_bytes_per_row_alignment: wgpu::COPY_BYTES_PER_ROW_ALIGNMENT,
            uniform_buffer_offset_alignment: limits.min_uniform_buffer_offset_alignment,
            storage_buffer_offset_alignment: limits.min_storage_buffer_offset_alignment,
            recommended_tile_size,
            candidate_formats,
        },
    }))
}

fn recommended_tile_size(maximum_texture_dimension_2d: u32) -> u32 {
    // Keep four RGBA8 working tiles under a conservative 64 MiB transient
    // policy while respecting the adapter's actual dimension limit.
    const TRANSIENT_BYTES: u64 = 64 * 1024 * 1024;
    const WORKING_TEXTURES: u64 = 4;
    const BYTES_PER_PIXEL: u64 = 4;
    let policy_edge = ((TRANSIENT_BYTES / WORKING_TEXTURES / BYTES_PER_PIXEL) as f64).sqrt() as u32;
    [2_048, 1_024, 512, 256]
        .into_iter()
        .find(|candidate| *candidate <= maximum_texture_dimension_2d && *candidate <= policy_edge)
        .unwrap_or(maximum_texture_dimension_2d.max(1))
}

#[cfg(test)]
mod tests {
    use super::GpuCapabilityService;

    #[test]
    fn repeated_requests_reuse_one_service_generation_and_result() {
        let service = GpuCapabilityService::default();
        let first = service.initialize();
        let second = service.initialize();
        assert_eq!(first.is_ok(), second.is_ok());
        match (first, second) {
            (Ok(first), Ok(second)) => {
                assert!(std::sync::Arc::ptr_eq(&first, &second));
                assert_eq!(
                    first.capabilities().service_generation,
                    service.generation()
                );
            }
            (Err(first), Err(second)) => assert_eq!(first, second),
            _ => unreachable!("OnceLock returns one stable initialization result"),
        }
    }
}
