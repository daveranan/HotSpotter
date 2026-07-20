#![doc = "Snapshot-based, validated, atomic export boundary."]

use std::{
    collections::{BTreeMap, VecDeque},
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
};

use hot_trimmer_domain::{
    CancellationToken, CanonicalRect, RadialMappingSettings, RadialParameters, RevisionAuthority,
    TemplateDefinition,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const EXPORT_PRESET_VERSION: u16 = 1;
pub const HOTTRIM_MANIFEST_FILE_NAME: &str = "manifest.hottrim.json";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapRecord {
    pub role: String,
    pub relative_path: String,
    pub dimensions: [u32; 2],
    pub bit_depth: u8,
    pub color_space: String,
    pub checksum: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UvFitKind {
    Rectangular,
    Radial,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FitAxis {
    Automatic,
    None,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UvFit {
    pub kind: UvFitKind,
    pub fit_axis: FitAxis,
    pub keep_proportion: bool,
    pub allowed_rotations: Vec<u16>,
    pub mirror_allowed: bool,
    pub classification_tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HottrimSlot {
    pub slot_id: String,
    pub region_id: String,
    pub name: String,
    pub allocation_rect: PixelRect,
    pub pixel_hotspot_rect: PixelRect,
    pub normalized_hotspot_rect: NormalizedRect,
    pub role: String,
    pub uv_fit: UvFit,
    pub world_size_meters: [f64; 2],
    pub variation_group: String,
    pub enabled: bool,
    pub region_id_color: [u8; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radial_parameters: Option<RadialParameters>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behavior_role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampling: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat_period_pixels: Option<[u32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orientation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radial_mapping: Option<RadialMappingSettings>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HottrimManifest {
    pub schema_version: u16,
    pub project_id: String,
    pub material_id: String,
    pub material_name: String,
    pub material_revision: u64,
    pub template_id: String,
    pub template_version: String,
    pub compatibility_key: String,
    pub template_snapshot_hash: String,
    pub output_size: [u32; 2],
    pub normal_orientation: String,
    pub maps: BTreeMap<String, MapRecord>,
    pub slots: Vec<HottrimSlot>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ManifestExportInput {
    pub project_id: String,
    pub material_id: String,
    pub material_name: String,
    pub material_revision: u64,
    pub output_size: [u32; 2],
    pub normal_orientation: String,
    pub maps: BTreeMap<String, MapRecord>,
}

#[derive(Debug, Error)]
pub enum ManifestExportError {
    #[error("manifest output size must be nonzero")]
    InvalidOutputSize,
    #[error("template snapshot failed: {0}")]
    Template(#[from] hot_trimmer_domain::TemplateRegistryError),
    #[error("manifest serialization failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("manifest package write failed: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportPixelFormat {
    Rgba8UnormSrgb,
    Rgba8UnormLinear,
    R32Float,
    R32Uint,
}

impl ExportPixelFormat {
    #[must_use]
    pub const fn bytes_per_pixel(self) -> u64 {
        4
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportMemoryBudgets {
    pub decoded_cpu_source_tiles_bytes: u64,
    pub gpu_source_residency_bytes: u64,
    pub gpu_output_intermediate_residency_bytes: u64,
    pub staging_buffers_bytes: u64,
    pub total_in_flight_tiles: u32,
}

impl Default for ExportMemoryBudgets {
    fn default() -> Self {
        Self {
            decoded_cpu_source_tiles_bytes: 512 * 1024 * 1024,
            gpu_source_residency_bytes: 1024 * 1024 * 1024,
            gpu_output_intermediate_residency_bytes: 1024 * 1024 * 1024,
            staging_buffers_bytes: 256 * 1024 * 1024,
            total_in_flight_tiles: 2,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResidencyReport {
    pub decoded_cpu_source_pinned_bytes: u64,
    pub decoded_cpu_source_evictable_bytes: u64,
    pub gpu_source_pinned_bytes: u64,
    pub gpu_source_evictable_bytes: u64,
    pub gpu_output_intermediate_pinned_bytes: u64,
    pub gpu_output_intermediate_evictable_bytes: u64,
    pub staging_pinned_bytes: u64,
    pub staging_evictable_bytes: u64,
    pub declared_budgets: ExportMemoryBudgets,
}

impl ExportResidencyReport {
    #[must_use]
    pub const fn total_pinned_bytes(&self) -> u64 {
        self.decoded_cpu_source_pinned_bytes
            + self.gpu_source_pinned_bytes
            + self.gpu_output_intermediate_pinned_bytes
            + self.staging_pinned_bytes
    }

    #[must_use]
    pub const fn total_evictable_bytes(&self) -> u64 {
        self.decoded_cpu_source_evictable_bytes
            + self.gpu_source_evictable_bytes
            + self.gpu_output_intermediate_evictable_bytes
            + self.staging_evictable_bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportAdapterLimits {
    pub max_texture_dimension_2d: u32,
    pub min_bytes_per_row_alignment: u32,
    pub supported_formats: Vec<ExportPixelFormat>,
}

impl Default for ExportAdapterLimits {
    fn default() -> Self {
        Self {
            max_texture_dimension_2d: 8192,
            min_bytes_per_row_alignment: 256,
            supported_formats: vec![
                ExportPixelFormat::Rgba8UnormSrgb,
                ExportPixelFormat::Rgba8UnormLinear,
                ExportPixelFormat::R32Float,
                ExportPixelFormat::R32Uint,
            ],
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportDiagnosticKind {
    GpuValidation,
    UnsupportedFeatureOrFormat,
    OutOfMemory,
    DeviceLost,
    Readback,
    Encoder,
    Filesystem,
    Cancellation,
    StaleRevision,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportDiagnostic {
    pub kind: ExportDiagnosticKind,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportProgress {
    pub map: String,
    pub mip_level: u32,
    pub completed_tiles: u32,
    pub total_tiles: u32,
    pub render_ms: u128,
    pub readback_ms: u128,
    pub encode_ms: u128,
    pub bytes_written: u64,
    pub estimated_remaining_tiles: u32,
}

#[derive(Debug, Error)]
pub enum TiledExportError {
    #[error("export request is invalid: {0}")]
    InvalidRequest(String),
    #[error("GPU validation failed: {0}")]
    GpuValidation(String),
    #[error("unsupported GPU feature or format: {0}")]
    UnsupportedFeatureOrFormat(String),
    #[error("export memory budget was exceeded: {0}")]
    OutOfMemory(String),
    #[error("GPU device was lost: {0}")]
    DeviceLost(String),
    #[error("GPU readback failed: {0}")]
    Readback(String),
    #[error("streaming encoder failed: {0}")]
    Encoder(String),
    #[error("filesystem export failed: {0}")]
    Filesystem(#[from] io::Error),
    #[error("export was cancelled")]
    Cancelled,
    #[error("export revision is stale")]
    StaleRevision,
}

impl TiledExportError {
    #[must_use]
    pub fn diagnostic_kind(&self) -> ExportDiagnosticKind {
        match self {
            Self::InvalidRequest(_) | Self::GpuValidation(_) => ExportDiagnosticKind::GpuValidation,
            Self::UnsupportedFeatureOrFormat(_) => ExportDiagnosticKind::UnsupportedFeatureOrFormat,
            Self::OutOfMemory(_) => ExportDiagnosticKind::OutOfMemory,
            Self::DeviceLost(_) => ExportDiagnosticKind::DeviceLost,
            Self::Readback(_) => ExportDiagnosticKind::Readback,
            Self::Encoder(_) => ExportDiagnosticKind::Encoder,
            Self::Filesystem(_) => ExportDiagnosticKind::Filesystem,
            Self::Cancelled => ExportDiagnosticKind::Cancellation,
            Self::StaleRevision => ExportDiagnosticKind::StaleRevision,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CacheEntry {
    byte_len: u64,
    pinned: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedTileCache<K> {
    capacity_bytes: u64,
    entries: BTreeMap<K, CacheEntry>,
    lru: VecDeque<K>,
    pinned_bytes: u64,
    evictable_bytes: u64,
}

impl<K: Clone + Ord> BoundedTileCache<K> {
    #[must_use]
    pub fn new(capacity_bytes: u64) -> Self {
        Self {
            capacity_bytes,
            entries: BTreeMap::new(),
            lru: VecDeque::new(),
            pinned_bytes: 0,
            evictable_bytes: 0,
        }
    }

    #[must_use]
    pub const fn pinned_bytes(&self) -> u64 {
        self.pinned_bytes
    }

    #[must_use]
    pub const fn evictable_bytes(&self) -> u64 {
        self.evictable_bytes
    }

    #[must_use]
    pub fn contains_key(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    pub fn touch(&mut self, key: &K) {
        if self.entries.contains_key(key) {
            self.lru.retain(|candidate| candidate != key);
            self.lru.push_back(key.clone());
        }
    }

    pub fn insert(
        &mut self,
        key: K,
        byte_len: u64,
        pinned: bool,
    ) -> Result<Vec<K>, TiledExportError> {
        if pinned && byte_len > self.capacity_bytes {
            return Err(TiledExportError::OutOfMemory(
                "single pinned tile exceeds its declared cache budget".into(),
            ));
        }
        let previous = self.entries.get(&key);
        let previous_pinned_bytes = previous
            .filter(|entry| entry.pinned)
            .map_or(0, |entry| entry.byte_len);
        let pinned_after = self
            .pinned_bytes
            .saturating_sub(previous_pinned_bytes)
            .saturating_add(if pinned { byte_len } else { 0 });
        if pinned_after > self.capacity_bytes {
            return Err(TiledExportError::OutOfMemory(
                "pinned tiles exhaust the declared cache budget".into(),
            ));
        }
        if let Some(previous) = self.entries.remove(&key) {
            self.subtract_entry(&previous);
        }
        self.lru.retain(|candidate| candidate != &key);
        let entry = CacheEntry { byte_len, pinned };
        self.add_entry(&entry);
        self.entries.insert(key.clone(), entry);
        self.lru.push_back(key);
        self.evict_to_budget()
    }

    fn add_entry(&mut self, entry: &CacheEntry) {
        if entry.pinned {
            self.pinned_bytes = self.pinned_bytes.saturating_add(entry.byte_len);
        } else {
            self.evictable_bytes = self.evictable_bytes.saturating_add(entry.byte_len);
        }
    }

    fn subtract_entry(&mut self, entry: &CacheEntry) {
        if entry.pinned {
            self.pinned_bytes = self.pinned_bytes.saturating_sub(entry.byte_len);
        } else {
            self.evictable_bytes = self.evictable_bytes.saturating_sub(entry.byte_len);
        }
    }

    fn evict_to_budget(&mut self) -> Result<Vec<K>, TiledExportError> {
        let mut evicted = Vec::new();
        while self.pinned_bytes.saturating_add(self.evictable_bytes) > self.capacity_bytes {
            let Some(position) = self
                .lru
                .iter()
                .position(|key| self.entries.get(key).is_some_and(|entry| !entry.pinned))
            else {
                return Err(TiledExportError::OutOfMemory(
                    "pinned tiles exhaust the declared cache budget".into(),
                ));
            };
            let key = self.lru.remove(position).expect("LRU position exists");
            if let Some(entry) = self.entries.remove(&key) {
                self.subtract_entry(&entry);
                evicted.push(key);
            }
        }
        Ok(evicted)
    }
}

#[must_use]
pub fn supported_gpu_minimums() -> ExportAdapterLimits {
    ExportAdapterLimits::default()
}

pub fn choose_bounded_tile_edge(
    requested_edge: u32,
    adapter_max: u32,
    bytes_per_pixel: u64,
    halo_px: u32,
    per_tile_budget: u64,
    row_alignment: u32,
) -> Result<u32, TiledExportError> {
    choose_tile_edge(
        requested_edge,
        adapter_max,
        bytes_per_pixel,
        halo_px,
        per_tile_budget,
        row_alignment,
    )
}

pub fn bounded_tile_byte_len(
    width: u32,
    height: u32,
    bytes_per_pixel: u64,
    halo_px: u32,
    row_alignment: u32,
) -> Result<u64, TiledExportError> {
    tile_byte_len(width, height, bytes_per_pixel, halo_px, row_alignment)
}

#[derive(Clone, Debug)]
pub struct StreamingExportOptions {
    pub final_path: PathBuf,
    pub expected_revision: u64,
    pub revisions: RevisionAuthority,
    pub expected_outputs: Vec<String>,
    pub fail_after_bytes: Option<u64>,
}

#[derive(Debug)]
pub struct StreamingExport {
    final_path: PathBuf,
    temporary_path: PathBuf,
    file: File,
    expected_revision: u64,
    revisions: RevisionAuthority,
    expected_outputs: Vec<String>,
    completed_outputs: BTreeMap<String, u64>,
    bytes_written: u64,
    fail_after_bytes: Option<u64>,
    finalized: bool,
}

impl StreamingExport {
    pub fn begin(options: StreamingExportOptions) -> Result<Self, TiledExportError> {
        if options.expected_outputs.is_empty() {
            return Err(TiledExportError::InvalidRequest(
                "streaming export must declare expected outputs before writing".into(),
            ));
        }
        let temporary_path = sibling_temporary_export_path(&options.final_path);
        let _ = fs::remove_file(&temporary_path);
        let file = File::create(&temporary_path)?;
        Ok(Self {
            final_path: options.final_path,
            temporary_path,
            file,
            expected_revision: options.expected_revision,
            revisions: options.revisions,
            expected_outputs: options.expected_outputs,
            completed_outputs: BTreeMap::new(),
            bytes_written: 0,
            fail_after_bytes: options.fail_after_bytes,
            finalized: false,
        })
    }

    #[must_use]
    pub const fn bytes_written(&self) -> u64 {
        self.bytes_written
    }

    pub fn encode_block(
        &mut self,
        block: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<ExportProgress, TiledExportError> {
        if cancellation.is_cancelled() {
            return Err(TiledExportError::Cancelled);
        }
        let block_len = u64::try_from(block.len()).map_err(|_| {
            TiledExportError::Encoder("encoder block length does not fit in u64".into())
        })?;
        if self
            .fail_after_bytes
            .is_some_and(|limit| self.bytes_written.saturating_add(block_len) > limit)
        {
            return Err(TiledExportError::Filesystem(io::Error::new(
                io::ErrorKind::WriteZero,
                "simulated streaming export write failure",
            )));
        }
        self.file.write_all(block)?;
        self.bytes_written = self.bytes_written.saturating_add(block_len);
        if cancellation.is_cancelled() {
            return Err(TiledExportError::Cancelled);
        }
        Ok(ExportProgress {
            map: String::new(),
            mip_level: 0,
            completed_tiles: 0,
            total_tiles: 0,
            render_ms: 0,
            readback_ms: 0,
            encode_ms: 0,
            bytes_written: self.bytes_written,
            estimated_remaining_tiles: 0,
        })
    }

    pub fn mark_output_complete(
        &mut self,
        output_id: impl Into<String>,
        encoded_bytes: u64,
        cancellation: &CancellationToken,
    ) -> Result<(), TiledExportError> {
        if cancellation.is_cancelled() {
            return Err(TiledExportError::Cancelled);
        }
        let output_id = output_id.into();
        if !self.expected_outputs.contains(&output_id) {
            return Err(TiledExportError::Encoder(format!(
                "completed unexpected export output {output_id}"
            )));
        }
        if encoded_bytes == 0 {
            return Err(TiledExportError::Encoder(format!(
                "completed export output {output_id} reported zero encoded bytes"
            )));
        }
        if self.completed_outputs.contains_key(&output_id) {
            return Err(TiledExportError::Encoder(format!(
                "export output {output_id} was completed more than once"
            )));
        }
        let completed_before = self.completed_outputs.values().copied().sum::<u64>();
        if completed_before.saturating_add(encoded_bytes) > self.bytes_written {
            return Err(TiledExportError::Encoder(format!(
                "completed export output {output_id} exceeds bytes written"
            )));
        }
        self.completed_outputs.insert(output_id, encoded_bytes);
        Ok(())
    }

    pub fn finalize(mut self, cancellation: &CancellationToken) -> Result<(), TiledExportError> {
        if cancellation.is_cancelled() {
            return Err(TiledExportError::Cancelled);
        }
        if self.revisions.current() != self.expected_revision {
            return Err(TiledExportError::StaleRevision);
        }
        if let Some(missing) = self
            .expected_outputs
            .iter()
            .find(|output| !self.completed_outputs.contains_key(*output))
        {
            return Err(TiledExportError::Encoder(format!(
                "export output {missing} was not completed"
            )));
        }
        let completed_bytes = self.completed_outputs.values().copied().sum::<u64>();
        if completed_bytes == 0 || completed_bytes != self.bytes_written {
            return Err(TiledExportError::Encoder(
                "completed export bytes do not match encoded stream progress".into(),
            ));
        }
        self.file.flush()?;
        self.file.sync_all()?;
        if cancellation.is_cancelled() {
            return Err(TiledExportError::Cancelled);
        }
        if self.revisions.current() != self.expected_revision {
            return Err(TiledExportError::StaleRevision);
        }
        fs::rename(&self.temporary_path, &self.final_path)?;
        self.finalized = true;
        Ok(())
    }
}

impl Drop for StreamingExport {
    fn drop(&mut self) {
        if !self.finalized {
            let _ = fs::remove_file(&self.temporary_path);
        }
    }
}

fn choose_tile_edge(
    requested_edge: u32,
    adapter_max: u32,
    bytes_per_pixel: u64,
    halo_px: u32,
    per_tile_budget: u64,
    row_alignment: u32,
) -> Result<u32, TiledExportError> {
    if per_tile_budget == 0 {
        return Err(TiledExportError::OutOfMemory(
            "per-tile budget is zero".into(),
        ));
    }
    let max_interior = adapter_max
        .checked_sub(halo_px.saturating_mul(2))
        .ok_or_else(|| {
            TiledExportError::UnsupportedFeatureOrFormat(
                "adapter maximum texture size cannot contain the requested tile halo".into(),
            )
        })?;
    if max_interior == 0 {
        return Err(TiledExportError::UnsupportedFeatureOrFormat(
            "adapter maximum texture size leaves no room for a tile interior after halo".into(),
        ));
    }
    let mut edge = previous_power_of_two(requested_edge.min(max_interior).min(8192).max(1));
    while edge > 1 {
        let byte_len = tile_byte_len(edge, edge, bytes_per_pixel, halo_px, row_alignment)?;
        if byte_len <= per_tile_budget {
            return Ok(edge);
        }
        edge /= 2;
    }
    let byte_len = tile_byte_len(1, 1, bytes_per_pixel, halo_px, row_alignment)?;
    if byte_len <= per_tile_budget {
        Ok(1)
    } else {
        Err(TiledExportError::OutOfMemory(
            "declared budgets cannot hold a one-pixel tile with halo".into(),
        ))
    }
}

fn previous_power_of_two(value: u32) -> u32 {
    let mut edge = 1_u32;
    while edge <= value / 2 {
        edge *= 2;
    }
    edge
}

fn tile_byte_len(
    width: u32,
    height: u32,
    bytes_per_pixel: u64,
    halo_px: u32,
    row_alignment: u32,
) -> Result<u64, TiledExportError> {
    let expanded_width = width
        .checked_add(halo_px.saturating_mul(2))
        .ok_or_else(|| TiledExportError::OutOfMemory("tile width overflows".into()))?;
    let expanded_height = height
        .checked_add(halo_px.saturating_mul(2))
        .ok_or_else(|| TiledExportError::OutOfMemory("tile height overflows".into()))?;
    let row_bytes = u64::from(expanded_width)
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| TiledExportError::OutOfMemory("tile row bytes overflow".into()))?;
    let aligned_row = align_to(row_bytes, u64::from(row_alignment));
    aligned_row
        .checked_mul(u64::from(expanded_height))
        .ok_or_else(|| TiledExportError::OutOfMemory("tile bytes overflow".into()))
}

fn align_to(value: u64, alignment: u64) -> u64 {
    value.div_ceil(alignment) * alignment
}

fn sibling_temporary_export_path(final_path: &Path) -> PathBuf {
    let file_name = final_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("export");
    final_path.with_file_name(format!(".{file_name}.tmp"))
}

/// Builds manifest semantics from template metadata and never from an ID map.
pub fn manifest_from_template(
    template: &TemplateDefinition,
    input: ManifestExportInput,
) -> Result<HottrimManifest, ManifestExportError> {
    if input.output_size.contains(&0) {
        return Err(ManifestExportError::InvalidOutputSize);
    }
    let snapshot = template.snapshot()?;
    let slots = template
        .slots
        .iter()
        .map(|slot| slot_manifest(slot, template.canonical_width, template.canonical_height))
        .collect();
    Ok(HottrimManifest {
        schema_version: 1,
        project_id: input.project_id,
        material_id: input.material_id,
        material_name: input.material_name,
        material_revision: input.material_revision,
        template_id: snapshot.identity.template_id,
        template_version: snapshot.identity.template_version,
        compatibility_key: snapshot.identity.compatibility_key,
        template_snapshot_hash: snapshot.snapshot_hash,
        output_size: input.output_size,
        normal_orientation: input.normal_orientation,
        maps: input.maps,
        slots,
    })
}

/// Serializes with a terminal newline so identical revisions are byte-identical.
pub fn manifest_json(manifest: &HottrimManifest) -> Result<String, ManifestExportError> {
    Ok(format!("{}\n", serde_json::to_string_pretty(manifest)?))
}

pub fn write_package_manifest(
    package_directory: &Path,
    manifest: &HottrimManifest,
) -> Result<(), ManifestExportError> {
    fs::write(
        package_directory.join(HOTTRIM_MANIFEST_FILE_NAME),
        manifest_json(manifest)?,
    )?;
    Ok(())
}

fn slot_manifest(slot: &hot_trimmer_domain::TemplateSlot, width: u32, height: u32) -> HottrimSlot {
    let allocation_rect = PixelRect {
        x: slot.allocation.x,
        y: slot.allocation.y,
        width: slot.allocation.width,
        height: slot.allocation.height,
    };
    let radial = slot.radial_parameters;
    let is_radial = radial.is_some();
    let (kind, fit_axis, allowed_rotations, mirror_allowed, classification_tags) = if is_radial {
        (
            UvFitKind::Radial,
            FitAxis::None,
            vec![0],
            false,
            vec!["HOTSPOT".to_owned(), "Radial".to_owned()],
        )
    } else {
        (
            UvFitKind::Rectangular,
            FitAxis::Automatic,
            vec![0, 90, 180, 270],
            true,
            vec!["HOTSPOT".to_owned()],
        )
    };
    HottrimSlot {
        slot_id: slot.slot_key.clone(),
        region_id: format!("{}:{}", slot.compatibility_key, slot.slot_key),
        name: slot.slot_key.clone(),
        allocation_rect,
        pixel_hotspot_rect: allocation_rect,
        normalized_hotspot_rect: normalized_rect(slot.allocation, width, height),
        role: if is_radial { "radial" } else { "rectangular" }.to_owned(),
        uv_fit: UvFit {
            kind,
            fit_axis,
            keep_proportion: true,
            allowed_rotations,
            mirror_allowed,
            classification_tags,
        },
        world_size_meters: [slot.world_placement.width, slot.world_placement.height],
        variation_group: slot.variation_group.clone(),
        enabled: true,
        region_id_color: slot.id_color.0,
        radial_parameters: radial,
        behavior_role: None,
        sampling: None,
        repeat_period_pixels: None,
        orientation: None,
        radial_mapping: None,
    }
}

fn normalized_rect(rect: CanonicalRect, width: u32, height: u32) -> NormalizedRect {
    NormalizedRect {
        x: f64::from(rect.x) / f64::from(width),
        y: f64::from(rect.y) / f64::from(height),
        width: f64::from(rect.width) / f64::from(width),
        height: f64::from(rect.height) / f64::from(height),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hot_trimmer_domain::TemplateRegistry;
    use std::time::{SystemTime, UNIX_EPOCH};

    const GENERIC_ARCHITECTURE: &str =
        include_str!("../../../assets/templates/generic_architecture/1.0.0/template.json");

    fn input() -> ManifestExportInput {
        ManifestExportInput {
            project_id: "project-fixture".to_owned(),
            material_id: "material-fixture".to_owned(),
            material_name: "Fixture Concrete".to_owned(),
            material_revision: 7,
            output_size: [2048, 2048],
            normal_orientation: "OpenGL".to_owned(),
            maps: BTreeMap::new(),
        }
    }
    #[test]
    fn generic_architecture_manifest_is_deterministic_and_carries_fit_semantics() {
        let registry =
            TemplateRegistry::from_json(GENERIC_ARCHITECTURE).expect("template registry");
        let template = registry
            .get("ht.generic_architecture", "1.0.0")
            .expect("generic architecture template");
        let first = manifest_from_template(template, input()).expect("manifest");
        assert_eq!(
            manifest_json(&first).expect("json"),
            manifest_json(&manifest_from_template(template, input()).expect("manifest"))
                .expect("json")
        );
        let rectangular = first
            .slots
            .iter()
            .find(|slot| slot.slot_id == "wall_primary")
            .expect("rectangular fixture");
        assert_eq!(rectangular.uv_fit.kind, UvFitKind::Rectangular);
        assert_eq!(
            rectangular.region_id_color,
            template
                .slots
                .iter()
                .find(|slot| slot.slot_key == "wall_primary")
                .unwrap()
                .id_color
                .0,
        );
        let radial = first
            .slots
            .iter()
            .find(|slot| slot.slot_id == "radial_fixture_a")
            .expect("radial fixture");
        assert_eq!(radial.uv_fit.kind, UvFitKind::Radial);
        assert_eq!(
            radial.region_id_color,
            template
                .slots
                .iter()
                .find(|slot| slot.slot_key == "radial_fixture_a")
                .unwrap()
                .id_color
                .0,
        );
        assert!(radial.radial_parameters.is_some());
    }

    #[test]
    fn gpu_tiled_export_lru_eviction_is_bounded_and_deterministic() {
        let mut cache = BoundedTileCache::new(10);
        assert!(cache.insert("a", 4, false).expect("insert a").is_empty());
        assert!(cache.insert("b", 4, false).expect("insert b").is_empty());
        cache.touch(&"a");
        let evicted = cache.insert("c", 4, false).expect("insert c");
        assert_eq!(evicted, vec!["b"]);
        assert!(cache.contains_key(&"a"));
        assert!(cache.contains_key(&"c"));
        assert_eq!(cache.evictable_bytes(), 8);

        assert!(cache.insert("pin", 6, true).expect("pin").contains(&"a"));
        assert_eq!(cache.pinned_bytes(), 6);
        assert_eq!(cache.evictable_bytes(), 4);
        assert!(matches!(
            cache.insert("too-much-pinned", 5, true),
            Err(TiledExportError::OutOfMemory(_))
        ));
        assert!(!cache.contains_key(&"too-much-pinned"));
        assert_eq!(cache.pinned_bytes(), 6);
        assert_eq!(cache.evictable_bytes(), 4);
    }

    #[test]
    fn gpu_tiled_export_tile_helpers_reserve_halo_and_staging_budget() {
        let edge = choose_bounded_tile_edge(8192, 8192, 4, 3, 8 * 1024 * 1024, 256)
            .expect("bounded tile edge");
        assert!(edge < 8192);
        let bytes = bounded_tile_byte_len(edge, edge, 4, 3, 256).expect("tile bytes");
        assert!(bytes <= 8 * 1024 * 1024);

        let error = choose_bounded_tile_edge(16, 4, 4, 3, 8 * 1024 * 1024, 256)
            .expect_err("halo cannot fit in adapter max");
        assert_eq!(
            error.diagnostic_kind(),
            ExportDiagnosticKind::UnsupportedFeatureOrFormat
        );
    }

    #[test]
    fn gpu_tiled_export_streaming_finalizes_atomically_and_preserves_existing_on_failure() {
        let root = unique_export_test_dir();
        fs::create_dir_all(&root).expect("test dir");
        let final_path = root.join("material.bin");
        fs::write(&final_path, b"existing-output").expect("existing output");
        let revisions = RevisionAuthority::new(7);

        let mut cancelled = StreamingExport::begin(StreamingExportOptions {
            final_path: final_path.clone(),
            expected_revision: 7,
            revisions: revisions.clone(),
            expected_outputs: vec!["Base Color".into()],
            fail_after_bytes: None,
        })
        .expect("cancelled stream");
        cancelled
            .encode_block(b"new bytes", &CancellationToken::new())
            .expect("write block");
        cancelled
            .mark_output_complete("Base Color", 9, &CancellationToken::new())
            .expect("complete output");
        let token = CancellationToken::new();
        token.cancel();
        assert!(matches!(
            cancelled.finalize(&token),
            Err(TiledExportError::Cancelled)
        ));
        assert_eq!(
            fs::read(&final_path).expect("existing after cancel"),
            b"existing-output"
        );

        let mut failed = StreamingExport::begin(StreamingExportOptions {
            final_path: final_path.clone(),
            expected_revision: 7,
            revisions: revisions.clone(),
            expected_outputs: vec!["Base Color".into()],
            fail_after_bytes: Some(4),
        })
        .expect("failed stream");
        let error = failed
            .encode_block(b"too many bytes", &CancellationToken::new())
            .expect_err("simulated write failure");
        assert_eq!(error.diagnostic_kind(), ExportDiagnosticKind::Filesystem);
        drop(failed);
        assert_eq!(
            fs::read(&final_path).expect("existing after write failure"),
            b"existing-output"
        );

        let mut zero = StreamingExport::begin(StreamingExportOptions {
            final_path: final_path.clone(),
            expected_revision: 7,
            revisions: revisions.clone(),
            expected_outputs: vec!["Base Color".into()],
            fail_after_bytes: None,
        })
        .expect("zero stream");
        zero.encode_block(b"bytes", &CancellationToken::new())
            .expect("write zero fixture");
        assert!(matches!(
            zero.mark_output_complete("Base Color", 0, &CancellationToken::new()),
            Err(TiledExportError::Encoder(_))
        ));
        assert!(matches!(
            zero.mark_output_complete("Base Color", 6, &CancellationToken::new()),
            Err(TiledExportError::Encoder(_))
        ));
        assert_eq!(
            fs::read(&final_path).expect("existing after zero/inflated completion"),
            b"existing-output"
        );

        let mut underreported = StreamingExport::begin(StreamingExportOptions {
            final_path: final_path.clone(),
            expected_revision: 7,
            revisions: revisions.clone(),
            expected_outputs: vec!["Base Color".into()],
            fail_after_bytes: None,
        })
        .expect("underreported stream");
        underreported
            .encode_block(b"under", &CancellationToken::new())
            .expect("write underreported fixture");
        underreported
            .mark_output_complete("Base Color", 4, &CancellationToken::new())
            .expect("complete with underreported bytes");
        assert!(matches!(
            underreported.mark_output_complete("Base Color", 1, &CancellationToken::new()),
            Err(TiledExportError::Encoder(_))
        ));
        assert!(matches!(
            underreported.finalize(&CancellationToken::new()),
            Err(TiledExportError::Encoder(_))
        ));
        assert_eq!(
            fs::read(&final_path).expect("existing after underreported completion"),
            b"existing-output"
        );

        let mut stale = StreamingExport::begin(StreamingExportOptions {
            final_path: final_path.clone(),
            expected_revision: 7,
            revisions: revisions.clone(),
            expected_outputs: vec!["Base Color".into()],
            fail_after_bytes: None,
        })
        .expect("stale stream");
        stale
            .encode_block(b"stale", &CancellationToken::new())
            .expect("write stale");
        stale
            .mark_output_complete("Base Color", 5, &CancellationToken::new())
            .expect("complete stale");
        revisions.supersede_with(8);
        assert!(matches!(
            stale.finalize(&CancellationToken::new()),
            Err(TiledExportError::StaleRevision)
        ));
        assert_eq!(
            fs::read(&final_path).expect("existing after stale"),
            b"existing-output"
        );

        let revisions = RevisionAuthority::new(9);
        let incomplete = StreamingExport::begin(StreamingExportOptions {
            final_path: final_path.clone(),
            expected_revision: 9,
            revisions: revisions.clone(),
            expected_outputs: vec!["Base Color".into(), "Normal".into()],
            fail_after_bytes: None,
        })
        .expect("incomplete stream");
        assert!(matches!(
            incomplete.finalize(&CancellationToken::new()),
            Err(TiledExportError::Encoder(_))
        ));
        assert_eq!(
            fs::read(&final_path).expect("existing after incomplete"),
            b"existing-output"
        );

        let mut successful = StreamingExport::begin(StreamingExportOptions {
            final_path: final_path.clone(),
            expected_revision: 9,
            revisions,
            expected_outputs: vec!["Base Color".into()],
            fail_after_bytes: None,
        })
        .expect("successful stream");
        successful
            .encode_block(b"final", &CancellationToken::new())
            .expect("write final");
        successful
            .mark_output_complete("Base Color", 5, &CancellationToken::new())
            .expect("complete final");
        successful
            .finalize(&CancellationToken::new())
            .expect("atomic finalize");
        assert_eq!(fs::read(&final_path).expect("final bytes"), b"final");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn gpu_tiled_export_reports_distinct_diagnostic_kinds() {
        assert_eq!(
            TiledExportError::DeviceLost("lost".into()).diagnostic_kind(),
            ExportDiagnosticKind::DeviceLost
        );
        assert_eq!(
            TiledExportError::Readback("map failed".into()).diagnostic_kind(),
            ExportDiagnosticKind::Readback
        );
        assert_eq!(
            TiledExportError::Encoder("png stream failed".into()).diagnostic_kind(),
            ExportDiagnosticKind::Encoder
        );
    }

    fn unique_export_test_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("hot-trimmer-gpu-tiled-export-{nanos}"))
    }
}
