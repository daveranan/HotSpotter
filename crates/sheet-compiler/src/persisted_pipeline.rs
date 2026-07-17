//! First authoritative persisted-project orchestration through Stage 14.

use std::{collections::BTreeMap, fs, sync::Arc};

use hot_trimmer_domain::{
    AddressMode, AlgorithmProvenance, CancellationToken, ContentDigest, ContentReference,
    MaterialChannelRole, OriginalAssetProvenance, OrientedPixelSize, Patch,
    PhysicalScaleEvidence, Projection, QuarterTurn, RegionMapping, RegisteredChannel,
    RegisteredChannelSet, SamplingPolicy, SourceOwnershipIntent, SourceSamplingMode, StageResult,
};
use hot_trimmer_effect_compiler::{
    SlotDemandIntent, VisualImportance, WorldDimensionSource, resolve_slot_demands,
};
use hot_trimmer_image_io::{CancellationToken as ImageCancellationToken, NormalizationSettings,
    prepare_registered_channel_set};
use hot_trimmer_material_analysis::{
    AnalysisSettings, FeatureFieldSettings, ScaleOrientationSettings, analyze_source,
    calibrate_scale_orientation, extract_feature_fields, prepare_delit_exemplar,
};
use hot_trimmer_material_synthesis::{
    DomainRequest, DomainRoute, GraphCutSettings, MaterialDomainCache, MaterialDomainRoutePolicy,
    PatchMatchSettings, ProceduralFitSettings, QuiltingSettings, Stage8RouterRequest,
    prepare_stage_08_material_domain,
};
use hot_trimmer_placement_solver::{
    CandidateEvidence, CandidateScoringMeasurements, CandidateSet, CandidateSettings,
    CandidateTransform, FeaturePosition, MirrorTransform, PlacementOptimizerSettings,
    PlacementSlotInput, ReusePermissions, ScoringContext, ScoringSettings, SliceCenterPolicy,
    SliceGeometry, SourceCrop, StretchOverrideProvenance, generate_candidates,
    optimize_placements, score_candidate_set,
};
use hot_trimmer_project_store::{ProjectSummary, SourceOwnership, StoredSource};
use hot_trimmer_render_core::{
    ExemplarMaskIntent, PlanarArea, PreparedExemplarRequest, PreparedExemplarScope,
    RectificationQuality, RectificationWorkLimits, RenderCancellationToken,
    prepare_registered_exemplar,
};

#[derive(Clone)]
struct DomainArtifacts {
    patch_id: Option<String>,
    domain: hot_trimmer_material_synthesis::PreparedMaterialDomain,
    stage5: hot_trimmer_material_analysis::SourceAnalysisReport,
    stage6: hot_trimmer_material_analysis::ScaleOrientationReport,
    stage7: hot_trimmer_material_analysis::FeatureFieldReport,
    stage3_result: StageResult,
    stage4_result: StageResult,
    stage8_result: StageResult,
}

use crate::{
    AlgorithmCompiler, CompilerFacadeError, IntermediateAtlasArtifact, IntermediateAtlasRequest,
    IntermediateSlotInput, SlotSynthesisLimits, SlotSynthesisRequest,
    synthesize_slot_material_with_guard,
};

#[derive(Clone, Copy, Debug)]
pub struct PersistedStage14PreviewRequest<'a> {
    pub project: &'a ProjectSummary,
    pub revision: u64,
}

impl AlgorithmCompiler {
    pub fn compile_persisted_stage_14_preview(
        &self,
        request: PersistedStage14PreviewRequest<'_>,
        cancellation: &CancellationToken,
        is_current: impl Fn() -> bool + Sync,
    ) -> Result<IntermediateAtlasArtifact, CompilerFacadeError> {
        let image_cancellation = ImageCancellationToken::new();
        let render_cancellation = RenderCancellationToken::new();
        let monitoring_complete = std::sync::atomic::AtomicBool::new(false);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                while !monitoring_complete.load(std::sync::atomic::Ordering::Acquire) {
                    if cancellation.is_cancelled() || !is_current() {
                        cancellation.cancel();
                        image_cancellation.cancel();
                        render_cancellation.cancel();
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            });
            let result = compile_persisted(request, cancellation, &image_cancellation,
                &render_cancellation, &is_current).map_err(CompilerFacadeError::Pipeline);
            monitoring_complete.store(true, std::sync::atomic::Ordering::Release);
            result
        })
    }
}

#[allow(clippy::too_many_lines)]
fn compile_persisted(
    request: PersistedStage14PreviewRequest<'_>,
    cancellation: &CancellationToken,
    image_cancellation: &ImageCancellationToken,
    render_cancellation: &RenderCancellationToken,
    is_current: &dyn Fn() -> bool,
) -> Result<IntermediateAtlasArtifact, String> {
    let active = || !cancellation.is_cancelled() && is_current();
    if !active() { return Err("preview cancelled or superseded before Stage 1".into()); }
    let document = request.project.document.as_ref().ok_or("persisted project has no trim-sheet document")?;
    if document.document_revision != request.revision { return Err("preview revision is already stale".into()); }
    let primary = document.primary_material.ok_or("persisted document has no primary material")?;
    let mut domains = Vec::<DomainArtifacts>::new();
    let mut domain_keys = BTreeMap::<String, usize>::new();
    let mut region_domains = Vec::with_capacity(document.topology.regions.len());
    for region in &document.topology.regions {
        let binding = document.region_bindings.get(&region.id)
            .ok_or_else(|| format!("region {} has no persisted content binding", region.id))?;
        let (source_set_id, patch) = resolve_region_content(request.project, document, primary, &binding.content)?;
        let key = format!("{}|{}", source_set_id, patch.as_ref().map_or_else(|| "full-source".into(), |value| value.id.to_string()));
        let index = if let Some(index) = domain_keys.get(&key).copied() { index } else {
            let artifacts = build_domain(request.project, source_set_id, patch.as_ref(), request.revision,
                image_cancellation, render_cancellation)?;
            let index = domains.len();
            domains.push(artifacts);
            domain_keys.insert(key, index);
            index
        };
        region_domains.push(index);
    }
    let first_domain = domains.first().ok_or("Stage 9 topology contains no regions")?;
    if !active() { return Err("preview cancelled or superseded after Stage 8".into()); }

    // Stage 9 is the exact persisted template snapshot compiled once for this output.
    let snapshot = document.topology.snapshot.template.as_ref().ok_or("Stage 9 requires a persisted template snapshot")?;
    let definition: hot_trimmer_domain::TemplateDefinition = serde_json::from_str(&snapshot.snapshot_json)
        .map_err(|error| format!("Stage 9 snapshot failed: {error}"))?;
    let topology = definition.compile_for_output(document.render_settings.output_size)
        .map_err(|error| format!("Stage 9 failed: {error}"))?;
    if topology.slots.len() != document.topology.regions.len() { return Err("Stage 9 slot order drifted from persisted regions".into()); }

    let evidence = candidate_evidence(&stage5, &stage6, &stage7);
    let candidate_settings = CandidateSettings { max_positions_per_size: 12, max_candidates_per_slot: 96,
        max_work: 100_000_000, ..CandidateSettings::default() };
    let scoring_settings = ScoringSettings { top_k: 16, ..ScoringSettings::default() };
    let mut placement_inputs = Vec::with_capacity(topology.slots.len());
    let stage10_inputs = topology.slots.iter().zip(&document.topology.regions).map(|(slot, region)| (region, SlotDemandIntent {
            destination_rect: slot.allocation, desired_texel_density: 512.0,
            world_dimension_source: WorldDimensionSource::Stage9Authored, source_scale: stage6.scale,
            visual_importance: VisualImportance::Standard, minimum_survivable_feature_m: 0.001,
            minimum_flat_center_m: 0.001, requested_features: Vec::new(), opposing_profile_widths_m: None,
        })).collect::<Vec<_>>();
    let stage10 = resolve_slot_demands(&stage10_inputs).map_err(|error| format!("Stage 10 failed: {error:?}"))?;
    for ((slot, region), demand) in topology.slots.iter().zip(&document.topology.regions).zip(&stage10.slots) {
        let generated = generate_candidates(&domain, demand, &evidence, &candidate_settings, document.document_revision)
            .map_err(|error| format!("Stage 11 failed for {}: {error:?}", slot.slot_key))?;
        let measurements = generated.candidates.iter().map(|candidate| (candidate.candidate_id.clone(),
            CandidateScoringMeasurements { visual_quality_milli: stage5.quality.sharpness_milli,
                quality_confidence_milli: 1000, ..CandidateScoringMeasurements::default() })).collect();
        let scored = score_candidate_set(demand, &generated, &ScoringContext {
            material_behavior: stage5.classification.routed_class(),
            material_confidence_milli: stage5.classification.confidence_milli,
            requested_physical_scale: 1.0, measurements,
        }, &scoring_settings).map_err(|error| format!("Stage 12 failed for {}: {error:?}", slot.slot_key))?;
        if scored.top_candidates.is_empty() { return Err(format!("Stage 12 produced no legal candidate for {}", slot.slot_key)); }
        let base_scale = base_pixels_per_physical_unit(stage6.scale, &demand, domain.width, domain.height);
        placement_inputs.push(PlacementSlotInput {
            slot_id: region.id, role: region.role, material_behavior: stage5.classification.routed_class(),
            variation_group: region.material_group.clone(), visual_importance_milli: 700,
            constraint_tightness_milli: 500, required: true, prepared_domain_id: domain.cache_key.clone(),
            prepared_domain_dimensions: [domain.width, domain.height], registered_correspondence_reference: domain.cache_key.clone(),
            slot_physical_size: [demand.world_width_m, demand.world_height_m], base_source_pixels_per_physical_unit: base_scale,
            sampling_policy: SamplingPolicy { filter: SourceSamplingMode::Linear, scale: 1.0, correct_tangent_normals: true },
            stretch_override: StretchOverrideProvenance::NotAuthorized, slice_geometry: slice_geometry(region.role, domain.width, domain.height),
            maximum_seam_cost_milli: 450, reuse_permissions: ReusePermissions::default(), candidates: scored,
        });
    }
    let placement_settings = PlacementOptimizerSettings { beam_width: 8, max_pairwise_evaluations: 100_000,
        max_local_evaluations: 5_000, local_passes: 1, ..PlacementOptimizerSettings::default() };
    let placement = optimize_placements(&placement_inputs, &placement_settings,
        document.document_revision, cancellation).map_err(|error| format!("Stage 13 failed: {error:?}"))?;
    if !active() { return Err("preview cancelled or superseded after Stage 13".into()); }

    let mut ordered_plans = Vec::with_capacity(placement.placements.len());
    let mut results = Vec::with_capacity(placement.placements.len());
    for (slot, region) in topology.slots.iter().zip(&document.topology.regions) {
        let plan = placement.placements.iter().find(|plan| plan.slot_id == region.id)
            .ok_or_else(|| format!("Stage 13 omitted required slot {}", slot.slot_key))?;
        results.push(synthesize_slot_material_with_guard(SlotSynthesisRequest { plan, domain: &domain,
            output_dimensions: [slot.allocation.width, slot.allocation.height], limits: SlotSynthesisLimits::default() },
            &|| !active()).map_err(|error| format!("Stage 14 failed for {}: {error}", slot.slot_key))?);
        ordered_plans.push(plan);
    }
    let patch_id = patch.id.to_string();
    let slots = topology.slots.iter().zip(&document.topology.regions).zip(ordered_plans.into_iter().zip(&results))
        .map(|((slot, region), (plan, result))| IntermediateSlotInput { slot_key: &slot.slot_key,
            display_name: &region.display_name, required: true, patch_id: Some(patch_id.clone()),
            domain: &domain, plan, result }).collect::<Vec<_>>();
    let versions = algorithm_versions([
        (1, Some(AlgorithmProvenance { algorithm_id: "hot_trimmer.persisted_registered_source".into(), version: "1.0.0".into() })),
        (2, Some(AlgorithmProvenance { algorithm_id: hot_trimmer_image_io::STAGE_02_ALGORITHM_ID.into(),
            version: hot_trimmer_image_io::STAGE_02_ALGORITHM_VERSION.into() })),
        (3, executed_algorithm(&exemplar.stage_result)),
        (4, executed_algorithm(&stage4.stage_result)), (5, executed_algorithm(&stage5.stage_result)),
        (6, executed_algorithm(&stage6.stage_result)), (7, executed_algorithm(&stage7.stage_result)),
        (8, executed_algorithm(&stage8_result)),
        (9, Some(AlgorithmProvenance { algorithm_id: "hot_trimmer.fixed_template_topology".into(), version: snapshot.identity.template_version.clone() })),
        (10, executed_algorithm(&stage10.stage_result)),
        (11, Some(AlgorithmProvenance { algorithm_id: hot_trimmer_placement_solver::STAGE_11_ALGORITHM_ID.into(), version: hot_trimmer_placement_solver::STAGE_11_ALGORITHM_VERSION.into() })),
        (12, Some(AlgorithmProvenance { algorithm_id: hot_trimmer_placement_solver::STAGE_12_ALGORITHM_ID.into(), version: hot_trimmer_placement_solver::STAGE_12_ALGORITHM_VERSION.into() })),
        (13, executed_algorithm(&placement.stage_result)),
        (14, results.first().and_then(|result| executed_algorithm(&result.stage_result))),
    ]);
    AlgorithmCompiler::new().compile_intermediate_atlas(&IntermediateAtlasRequest { topology: &topology,
        placement_plan: &placement, slots, revision: request.revision, algorithm_versions: versions, diagnostics: Vec::new() },
        cancellation, || if active() { request.revision } else { 0 }).map_err(|error| error.to_string())
}

fn registered_inputs(sources: &[&StoredSource]) -> Result<(RegisteredChannelSet, BTreeMap<hot_trimmer_domain::SourceId, Vec<u8>>), String> {
    let first = sources.first().ok_or("primary source set is empty")?;
    let size = OrientedPixelSize { width: first.input.width, height: first.input.height };
    let mut channels = Vec::new(); let mut encoded = BTreeMap::new();
    for source in sources {
        let bytes = match source.input.ownership { SourceOwnership::OwnedCopy => source.input.owned_bytes.clone()
            .ok_or("owned source bytes are missing")?, SourceOwnership::VerifiedExternalReference => fs::read(source.input.external_path.as_ref()
                .ok_or("external source path is missing")?).map_err(|error| error.to_string())? };
        channels.push(RegisteredChannel { source_id: source.input.id, registration: source.registration.clone(),
            oriented_size: size, orientation: source.input.exif_orientation,
            original: OriginalAssetProvenance { original_path: source.input.origin_path.display().to_string(),
                immutable_digest: ContentDigest(source.input.sha256.clone()), encoded_bytes: source.input.encoded_bytes },
            ownership: match source.input.ownership { SourceOwnership::OwnedCopy => SourceOwnershipIntent::OwnedCopy,
                SourceOwnership::VerifiedExternalReference => SourceOwnershipIntent::VerifiedExternalReference } });
        encoded.insert(source.input.id, bytes);
    }
    Ok((RegisteredChannelSet { oriented_size: size, orientation: first.input.exif_orientation, channels }, encoded))
}

fn candidate_evidence(stage5: &hot_trimmer_material_analysis::SourceAnalysisReport,
    stage6: &hot_trimmer_material_analysis::ScaleOrientationReport,
    stage7: &hot_trimmer_material_analysis::FeatureFieldReport) -> CandidateEvidence {
    CandidateEvidence { material_class: stage5.classification.routed_class(),
        class_confidence_milli: stage5.classification.confidence_milli,
        orientation_confidence_milli: stage6.global_orientation.confidence_milli,
        destructive_quarter_turn_override: stage6.global_orientation.destructive_rotation_allowed,
        periods: stage7.periodicity.candidates.iter().filter_map(|candidate| {
            let x = candidate.first.dx_pixels.unsigned_abs(); let y = candidate.first.dy_pixels.unsigned_abs();
            (x > 0 && y > 0).then_some([x, y]) }).collect(), feature_positions: Vec::new() }
}

fn base_pixels_per_physical_unit(scale: PhysicalScaleEvidence, demand: &hot_trimmer_effect_compiler::ResolvedSlotDemand,
    width: u32, height: u32) -> f64 {
    if scale.claims_world_accuracy() { let x = scale.source_pixels_per_meter_x_milli.unwrap_or(1000) as f64 / 1000.0;
        let y = scale.source_pixels_per_meter_y_milli.unwrap_or(1000) as f64 / 1000.0; (x * y).sqrt() }
    else { (f64::from(width) / demand.world_width_m).min(f64::from(height) / demand.world_height_m).max(f64::EPSILON) }
}

fn slice_geometry(role: hot_trimmer_domain::TemplateSlotRole, width: u32, height: u32) -> SliceGeometry {
    match role { hot_trimmer_domain::TemplateSlotRole::TrimCap if width >= 3 => SliceGeometry::Three {
        leading_cap_pixels: 1, trailing_cap_pixels: 1, center: SliceCenterPolicy::Repeat }, _ => { let _ = height; SliceGeometry::None } }
}

fn executed_algorithm(result: &StageResult) -> Option<AlgorithmProvenance> {
    match result { StageResult::Executed { algorithm, .. } => Some(algorithm.clone()), _ => None }
}

fn algorithm_versions<const N: usize>(results: [(u8, Option<AlgorithmProvenance>); N]) -> BTreeMap<u8, AlgorithmProvenance> {
    results.into_iter().filter_map(|(stage, algorithm)| algorithm.map(|algorithm| (stage, algorithm))).collect()
}
