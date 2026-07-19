//! Immutable atlas render-execution boundary introduced by GPU migration Prompt 1.

use std::{sync::{Arc, Mutex}, time::Instant};

use hot_trimmer_domain::{CancellationToken, ContentDigest, MaterialChannelRole, RegionId, SourceSetId};
use hot_trimmer_material_synthesis::PreparedMaterialDomain;
use hot_trimmer_placement_solver::SamplingPlan;

use crate::{
    compiled_atlas_plan::CompiledAtlasPlanV1,
    persisted_pipeline::{SourceFramePreviewCache, semantic_rect_for_padding},
    synthesize_slot_material_with_guard, SlotSynthesisLimits, SlotSynthesisRequest,
    SynthesizedSlotMaterial,
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
pub struct AtlasRenderExecutorOutput {
    pub regions: Vec<AtlasExecutedRegion>,
    pub render_ms: u128,
    pub rendered_cache_hits: u32,
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
}

impl std::fmt::Display for AtlasRenderExecutionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(formatter, "atlas render execution cancelled"),
            Self::Superseded => write!(formatter, "atlas render execution was superseded"),
            Self::MissingPreparedSource {
                source_set_id,
                source_id,
            } => write!(formatter, "missing prepared source {source_set_id}/{source_id:?}"),
            Self::InvalidInput(message) => write!(formatter, "atlas render input was invalid: {message}"),
            Self::Stage14(message) => write!(formatter, "Stage 14 CPU execution failed: {message}"),
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
}

/// Prompt 1 production executor. It deliberately retains the established CPU
/// sampler while forcing every Stage 14 request through the immutable boundary.
#[derive(Debug, Default)]
pub struct CpuAtlasRenderExecutor;

impl AtlasRenderExecutor for CpuAtlasRenderExecutor {
    fn execute(
        &self,
        plan: &CompiledAtlasPlanV1,
        input: &AtlasRenderExecutionInput<'_>,
        cancellation: &CancellationToken,
        is_current: &dyn Fn() -> bool,
    ) -> Result<AtlasRenderExecutorOutput, AtlasRenderExecutionError> {
        plan.validate().map_err(|error| AtlasRenderExecutionError::InvalidInput(error.to_string()))?;
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
            let source = input.prepared_sources.iter().find(|source| {
                source.source_set_id == command.source_set_id
                    && source.source_id == command.source_id
                    && source.channel_role == MaterialChannelRole::BaseColor
            }).ok_or_else(|| AtlasRenderExecutionError::MissingPreparedSource {
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
                input.source_frame_cache.and_then(|cache| cache.lock().ok())
                    .and_then(|cache| cache.get_rendered(&command.render_cache_key))
            } else {
                None
            };
            let result = if let Some(result) = result {
                rendered_cache_hits = rendered_cache_hits.saturating_add(1);
                result
            } else {
                let result = Arc::new(synthesize_slot_material_with_guard(
                    SlotSynthesisRequest {
                        plan: &command.sampling_plan,
                        domain: source.domain.as_ref(),
                        output_dimensions: [semantic.width, semantic.height],
                        limits: SlotSynthesisLimits::default(),
                    },
                    &|| cancellation.is_cancelled() || !is_current(),
                ).map_err(|error| AtlasRenderExecutionError::Stage14(format!(
                    "region {}: {error}", command.region_id
                )))?);
                if use_cache && let Some(cache) = input.source_frame_cache
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
        Ok(AtlasRenderExecutorOutput {
            regions,
            render_ms: started.elapsed().as_millis(),
            rendered_cache_hits,
        })
    }
}
