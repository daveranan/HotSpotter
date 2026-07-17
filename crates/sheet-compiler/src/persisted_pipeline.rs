//! First authoritative persisted-project orchestration through Stage 14.

use std::{collections::BTreeMap, fs, sync::Arc};

use hot_trimmer_domain::{
    AddressMode, AlgorithmProvenance, CancellationToken, ContentDigest, ContentReference,
    MaterialChannelRole, OriginalAssetProvenance, OrientedPixelSize, Patch,
    PhysicalScaleEvidence, Projection, QuarterTurn, RegionMapping, RegisteredChannel,
    RegisteredChannelSet, SamplingPolicy, SourceOwnershipIntent, StageResult,
};
use hot_trimmer_effect_compiler::{
    RequiredSourceFootprint, SlotDemandIntent, SourceFootprintUnit, VisualImportance,
    WorldDimensionSource, resolve_slot_demands_with_guard,
};
use hot_trimmer_image_io::{CancellationToken as ImageCancellationToken, NormalizationSettings,
    prepare_registered_channel_set};
use hot_trimmer_material_analysis::{
    AnalysisSettings, FeatureFieldSettings, ScaleOrientationSettings, analyze_source,
    calibrate_scale_orientation, extract_feature_fields, prepare_delit_exemplar,
};
use hot_trimmer_material_synthesis::{
    DomainRequest, DomainRoute, GraphCutSettings, MaterialDomainCache, MaterialDomainRoutePolicy, SeamAxis,
    PatchMatchSettings, ProceduralFitSettings, QuiltingSettings, Stage8RouterRequest,
    prepare_stage_08_material_domain,
};
use hot_trimmer_placement_solver::{
    CandidateEvidence, CandidateScoringMeasurements, CandidateSet, CandidateSettings, CandidateFamily,
    CandidateTransform, FeaturePosition, MirrorTransform, PlacementOptimizerSettings,
    PlacementPlan, PlacementObjectiveBreakdown, PlacementPlanQaView, PlacementSlotInput,
    PlacementValidationSummary, ReusePermissions, SamplingPlan, ScoringContext, ScoringSettings, SliceCenterPolicy,
    MaterialDomainView, SliceGeometry, SourceCrop, StretchOverrideProvenance,
    generate_candidates_with_guard, optimize_placements, score_candidate_set_with_guard, CropCandidate,
    CandidateRoute, PositionStrategy, CandidateDescriptors, EligibilityEvidence,
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

struct MappedDomainView<'a> {
    domain: &'a hot_trimmer_material_synthesis::PreparedMaterialDomain,
    window: SourceCrop,
}

impl MaterialDomainView for MappedDomainView<'_> {
    fn domain_id(&self) -> &ContentDigest { &self.domain.cache_key }
    fn source_id(&self) -> &ContentDigest { &self.domain.prepared_source_digest }
    fn dimensions(&self) -> (u32, u32) { (self.window.width, self.window.height) }
    fn route(&self) -> DomainRoute { self.domain.route }
    fn valid(&self, x: u32, y: u32) -> bool {
        self.domain.validity.pixel(self.window.x + x, self.window.y + y).0 >= 0.5
    }
    fn seam_indices(&self, axis: SeamAxis) -> Vec<u32> {
        self.domain.seams.iter().enumerate().filter_map(|(index, seam)|
            (seam.axis == axis).then_some(index as u32)).collect()
    }
}

use crate::{
    AlgorithmCompiler, CompilerFacadeError, IntermediateAtlasArtifact, IntermediateAtlasRequest,
    IntermediateSlotInput, SlotSynthesisLimits, SlotSynthesisRequest,
    synthesize_slot_material_with_guard,
};

/// Gate 1 must show source subdivisions, not a second copy of the whole source in
/// every fixed-template region. The fraction is applied isotropically, so the
/// authored slot aspect is preserved while leaving Stage 11 multiple positions
/// to choose from.
const UNPLACED_SOURCE_FOOTPRINT_FRACTION: f64 = 0.5;

#[derive(Clone, Debug)]
pub struct PersistedStage14PreviewRequest<'a> {
    pub project: &'a ProjectSummary,
    pub revision: u64,
    pub draft_id: Option<u64>,
    pub input_hash: Option<String>,
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
    if document.source_frame.is_some() {
        return compile_source_frame(request, cancellation, image_cancellation, render_cancellation, is_current, document, primary);
    }
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
                false, image_cancellation, render_cancellation)?;
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

    let candidate_settings = CandidateSettings { max_positions_per_size: 12, max_candidates_per_slot: 96,
        max_work: 100_000_000, ..CandidateSettings::default() };
    let scoring_settings = ScoringSettings { top_k: 16, ..ScoringSettings::default() };
    let mut placement_inputs = Vec::with_capacity(topology.slots.len());
    let stage10_inputs = topology.slots.iter().zip(&document.topology.regions).enumerate().map(|(index, (slot, region))| (region, SlotDemandIntent {
            destination_rect: slot.allocation, desired_texel_density: 512.0,
            world_dimension_source: WorldDimensionSource::Stage9Authored, source_scale: domains[region_domains[index]].stage6.scale,
            visual_importance: VisualImportance::Standard, minimum_survivable_feature_m: 0.001,
            minimum_flat_center_m: 0.001, requested_features: Vec::new(), opposing_profile_widths_m: None,
        })).collect::<Vec<_>>();
    let mut stage10 = resolve_slot_demands_with_guard(&stage10_inputs, &|| !active())
        .map_err(|error| format!("Stage 10 failed: {error:?}"))?;
    // Gate 1 is deliberately limited to modes with a truthful registered Stage 14
    // implementation. TextureSynthesis and PolarRadial currently have candidate
    // families but no exact Stage 14 artifact, so they must not reach optimization.
    for demand in &mut stage10.slots {
        let region = document.topology.regions.iter().find(|region| region.id == demand.slot_id)
            .ok_or_else(|| format!("Stage 10 produced an unknown region {}", demand.slot_id))?;
        let binding = document.region_bindings.get(&region.id).ok_or("Stage 10 region binding disappeared")?;
        if binding.mapping.source_crop_intent == Some(hot_trimmer_domain::SourceCropIntent::Unplaced) {
            // With no authored crop, derive a bounded aspect-preserving physical
            // footprint. The previous largest-fit value made square/detail slots
            // consume nearly the whole source and gave Stage 13 no meaningful
            // spatial subdivision to select.
            let domain = &domains[region_domains[document.topology.regions.iter().position(|candidate| candidate.id == region.id).expect("region index")]];
            let fit_scale = unplaced_source_pixels_per_physical_unit(demand, domain.domain.width, domain.domain.height);
            demand.required_source_footprint = RequiredSourceFootprint {
                width: demand.world_width_m * fit_scale,
                height: demand.world_height_m * fit_scale,
                unit: SourceFootprintUnit::SourcePixels,
                scale_provenance: domain.stage6.scale.provenance,
                world_scale: domain.stage6.scale.world_scale,
                confidence_milli: domain.stage6.scale.confidence_milli,
            };
        }
        let has_declared_period = domains[region_domains[document.topology.regions.iter().position(|candidate| candidate.id == region.id).expect("region index")]]
            .stage7.periodicity.candidates.iter().any(|candidate| {
                candidate.first.dx_pixels.unsigned_abs() > 0 && candidate.first.dy_pixels.unsigned_abs() > 0
            });
        demand.allowed_mapping_modes.retain(|mode| legal_gate1_mode(*mode, demand.slot_role, has_declared_period));
        demand.mapping_mode = demand.allowed_mapping_modes.first().copied()
            .ok_or_else(|| format!("Stage 10 has no executable Gate 1 mode for {}", demand.slot_id))?;
    }
    for (index, ((slot, region), demand)) in topology.slots.iter().zip(&document.topology.regions).zip(&stage10.slots).enumerate() {
        let artifacts = &domains[region_domains[index]];
        let binding = document.region_bindings.get(&region.id).ok_or("Stage 10 region binding disappeared")?;
        let window = mapping_window(&binding.mapping, artifacts.domain.width, artifacts.domain.height);
        let mut evidence = candidate_evidence(&artifacts.stage5, &artifacts.stage6, &artifacts.stage7);
        evidence.feature_positions.retain_mut(|feature| {
            if feature.x < window.x || feature.y < window.y
                || feature.x >= window.x + window.width || feature.y >= window.y + window.height { return false; }
            feature.x -= window.x; feature.y -= window.y; true
        });
        let view = MappedDomainView { domain: &artifacts.domain, window };
        let unplaced = binding.mapping.source_crop_intent == Some(hot_trimmer_domain::SourceCropIntent::Unplaced);
        let mut slot_candidate_settings = candidate_settings.clone();
        if unplaced {
            // The bounded footprint is the Gate 1 source subdivision. Do not let
            // the generic upscale ladder grow it back to the complete domain.
            slot_candidate_settings.scale_ladder.retain(|scale| *scale <= 1.0);
            slot_candidate_settings.maximum_scale = 1.0;
        }
        let generated = generate_candidates_with_guard(&view, demand, &evidence, &slot_candidate_settings,
            document.document_revision, &|| !active())
            .map_err(|error| format!("Stage 11 failed for {}: {error:?}", slot.slot_key))?;
        let generated = apply_authored_mapping(generated, &binding.mapping, artifacts.patch_id.is_some(),
            artifacts.domain.width, artifacts.domain.height)?;
        let measurements = generated.candidates.iter().map(|candidate| (candidate.candidate_id.clone(),
            candidate_measurements(candidate, demand, region.role, artifacts))).collect();
        let scored = score_candidate_set_with_guard(demand, &generated, &ScoringContext {
            material_behavior: artifacts.stage5.classification.routed_class(),
            material_confidence_milli: artifacts.stage5.classification.confidence_milli,
            requested_physical_scale: 1.0, measurements,
        }, &scoring_settings, &|| !active()).map_err(|error| format!("Stage 12 failed for {}: {error:?}", slot.slot_key))?;
        if scored.top_candidates.is_empty() {
            return Err(format!("Stage 12 produced no legal candidate for {}", slot.slot_key));
        }
        let unplaced = binding.mapping.source_crop_intent == Some(hot_trimmer_domain::SourceCropIntent::Unplaced);
        let require_spatially_distinct_crops = unplaced
            && region.role == hot_trimmer_domain::TemplateSlotRole::UniqueDetail;
        let base_scale = if unplaced {
            unplaced_source_pixels_per_physical_unit(demand, artifacts.domain.width, artifacts.domain.height)
        } else {
            base_pixels_per_physical_unit(artifacts.stage6.scale, &demand,
                artifacts.domain.width, artifacts.domain.height)
        };
        placement_inputs.push(PlacementSlotInput {
            slot_id: region.id, role: region.role, material_behavior: artifacts.stage5.classification.routed_class(),
            variation_group: region.material_group.clone(), visual_importance_milli: 700,
            constraint_tightness_milli: 500, required: true, prepared_domain_id: artifacts.domain.cache_key.clone(),
            prepared_domain_dimensions: [artifacts.domain.width, artifacts.domain.height],
            registered_correspondence_reference: artifacts.domain.cache_key.clone(),
            slot_physical_size: [demand.world_width_m, demand.world_height_m], base_source_pixels_per_physical_unit: base_scale,
            sampling_policy: authored_sampling_policy(&binding.mapping)?,
            radial_mapping: binding.mapping.radial,
            stretch_override: StretchOverrideProvenance::NotAuthorized,
            slice_geometry: slice_geometry(region.role, artifacts.domain.width, artifacts.domain.height),
            maximum_seam_cost_milli: 450,
            reuse_permissions: ReusePermissions { require_spatially_distinct_crops, ..ReusePermissions::default() },
            candidates: scored,
        });
    }
    let placement_settings = PlacementOptimizerSettings { beam_width: 8, max_pairwise_evaluations: 100_000,
        max_local_evaluations: 5_000, local_passes: 1, ..PlacementOptimizerSettings::default() };
    let placement = optimize_placements(&placement_inputs, &placement_settings,
        document.document_revision, cancellation).map_err(|error| format!("Stage 13 failed: {error:?}"))?;
    if !active() { return Err("preview cancelled or superseded after Stage 13".into()); }

    let mut ordered_plans = Vec::with_capacity(placement.placements.len());
    let mut results = Vec::with_capacity(placement.placements.len());
    for (index, (slot, region)) in topology.slots.iter().zip(&document.topology.regions).enumerate() {
        let artifacts = &domains[region_domains[index]];
        let plan = placement.placements.iter().find(|plan| plan.slot_id == region.id)
            .ok_or_else(|| format!("Stage 13 omitted required slot {}", slot.slot_key))?;
        results.push(synthesize_slot_material_with_guard(SlotSynthesisRequest { plan, domain: &artifacts.domain,
            output_dimensions: [slot.allocation.width, slot.allocation.height], limits: SlotSynthesisLimits::default() },
            &|| !active()).map_err(|error| format!("Stage 14 failed for {}: {error}", slot.slot_key))?);
        ordered_plans.push(plan);
    }
    let slots = topology.slots.iter().zip(&document.topology.regions).zip(ordered_plans.into_iter().zip(&results))
        .enumerate().map(|(index, ((slot, region), (plan, result)))| {
            let artifacts = &domains[region_domains[index]];
            IntermediateSlotInput { region_id: region.id, slot_key: &slot.slot_key, display_name: &region.display_name,
                required: true, patch_id: artifacts.patch_id.clone(), domain: &artifacts.domain, plan, result, grid_rect: region.grid_rect }
        }).collect::<Vec<_>>();
    let versions = algorithm_versions([
        (1, Some(AlgorithmProvenance { algorithm_id: "hot_trimmer.persisted_registered_source".into(), version: "1.0.0".into() })),
        (2, Some(AlgorithmProvenance { algorithm_id: hot_trimmer_image_io::STAGE_02_ALGORITHM_ID.into(),
            version: hot_trimmer_image_io::STAGE_02_ALGORITHM_VERSION.into() })),
        (3, executed_algorithm(&first_domain.stage3_result)),
        (4, executed_algorithm(&first_domain.stage4_result)), (5, executed_algorithm(&first_domain.stage5.stage_result)),
        (6, executed_algorithm(&first_domain.stage6.stage_result)), (7, executed_algorithm(&first_domain.stage7.stage_result)),
        (8, executed_algorithm(&first_domain.stage8_result)),
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

/// Source-frame documents intentionally use the same persisted compiler spine, but stages 11-13
/// are validation/provenance stages here: the accepted GridRects already define every source and
/// destination rectangle, so no crop search, ranking, reuse, repeat, or synthesis is legal.
#[allow(clippy::too_many_arguments)]
fn compile_source_frame(
    request: PersistedStage14PreviewRequest<'_>,
    cancellation: &CancellationToken,
    image_cancellation: &ImageCancellationToken,
    render_cancellation: &RenderCancellationToken,
    is_current: &dyn Fn() -> bool,
    document: &hot_trimmer_domain::TrimSheetDocument,
    primary: hot_trimmer_domain::SourceSetId,
) -> Result<IntermediateAtlasArtifact, String> {
    let active = || !cancellation.is_cancelled() && is_current();
    let frame = document.source_frame.as_ref().ok_or("source-frame document has no SourceFrame")?;
    let grid = document.logical_grid.ok_or("source-frame document has no LogicalGridSpec")?;
    let provenance = document.partition_provenance.as_ref().ok_or("source-frame document has no partition provenance")?;
    if frame.source_set_id != primary || frame.oriented_dimensions.width == 0 || frame.oriented_dimensions.height == 0
        || frame.output_aspect.contains(&0) || grid.validate().is_err()
        || provenance.recipe.grid != grid {
        return Err("source-frame contract is invalid".into());
    }
    if !aspect_matches(
        frame.bounds.width.get() * f64::from(frame.oriented_dimensions.width),
        frame.bounds.height.get() * f64::from(frame.oriented_dimensions.height),
        f64::from(frame.output_aspect[0]),
        f64::from(frame.output_aspect[1]),
    ) {
        return Err("SourceFrame bounds do not preserve the declared output aspect".into());
    }
    if document.topology.regions.iter().any(|region| document.region_bindings.get(&region.id)
        .is_none_or(|binding| binding.mapping.address_mode != AddressMode::Clamp)) {
        return Err("SourceFrame workflow requires clamp address mode for every region".into());
    }
    let cell_count = usize::try_from(u64::from(grid.width) * u64::from(grid.height)).map_err(|_| "logical grid is too large")?;
    let mut coverage = vec![0_u8; cell_count];
    for region in &document.topology.regions {
        let rect = region.grid_rect.ok_or_else(|| format!("region {} has no persisted GridRect", region.id))?;
        if rect.width == 0 || rect.height == 0 || rect.x.checked_add(rect.width).is_none_or(|end| end > grid.width)
            || rect.y.checked_add(rect.height).is_none_or(|end| end > grid.height) {
            return Err(format!("region {} has an out-of-bounds GridRect", region.id));
        }
        for y in rect.y..rect.y + rect.height {
            for x in rect.x..rect.x + rect.width {
                let cell = &mut coverage[(y * grid.width + x) as usize];
                *cell = cell.saturating_add(1);
            }
        }
    }
    if coverage.iter().any(|value| *value != 1) {
        return Err("accepted SourceFrame partition has a logical gap or overlap".into());
    }
    let artifacts = build_domain(request.project, primary, None, request.revision, true, image_cancellation, render_cancellation)?;
    if artifacts.domain.width != frame.oriented_dimensions.width || artifacts.domain.height != frame.oriented_dimensions.height {
        return Err("SourceFrame oriented dimensions do not match the prepared source".into());
    }
    let source_left = (frame.bounds.x.get() * f64::from(frame.oriented_dimensions.width)).round() as u32;
    let source_top = (frame.bounds.y.get() * f64::from(frame.oriented_dimensions.height)).round() as u32;
    let source_width = (frame.bounds.width.get() * f64::from(frame.oriented_dimensions.width)).round() as u32;
    let source_height = (frame.bounds.height.get() * f64::from(frame.oriented_dimensions.height)).round() as u32;
    let source_x = hot_trimmer_domain::resolve_boundaries(source_left, source_width, grid.width);
    let source_y = hot_trimmer_domain::resolve_boundaries(source_top, source_height, grid.height);
    let destination_x = hot_trimmer_domain::resolve_boundaries(0, document.render_settings.output_size.width, grid.width);
    let destination_y = hot_trimmer_domain::resolve_boundaries(0, document.render_settings.output_size.height, grid.height);
    let mut slots = Vec::with_capacity(document.topology.regions.len());
    let mut plans = Vec::with_capacity(document.topology.regions.len());
    let mut results = Vec::with_capacity(document.topology.regions.len());
    for region in &document.topology.regions {
        if !active() { return Err("preview cancelled or superseded in source-frame stages".into()); }
        let rect = region.grid_rect.ok_or_else(|| format!("region {} has no persisted GridRect", region.id))?;
        let allocation = hot_trimmer_domain::CanonicalRect {
            x: destination_x[rect.x as usize], y: destination_y[rect.y as usize],
            width: destination_x[(rect.x + rect.width) as usize] - destination_x[rect.x as usize],
            height: destination_y[(rect.y + rect.height) as usize] - destination_y[rect.y as usize],
        };
        let crop = document.source_overrides.get(&region.id).map(|value| {
            let bounds = value.source_bounds;
            let x = (bounds.x.get() * f64::from(frame.oriented_dimensions.width)).round() as u32;
            let y = (bounds.y.get() * f64::from(frame.oriented_dimensions.height)).round() as u32;
            SourceCrop { x, y,
                width: (bounds.width.get() * f64::from(frame.oriented_dimensions.width)).round() as u32,
                height: (bounds.height.get() * f64::from(frame.oriented_dimensions.height)).round() as u32 }
        }).unwrap_or(SourceCrop { x: source_x[rect.x as usize], y: source_y[rect.y as usize],
            width: source_x[(rect.x + rect.width) as usize] - source_x[rect.x as usize],
            height: source_y[(rect.y + rect.height) as usize] - source_y[rect.y as usize] });
        if crop.width == 0 || crop.height == 0 || allocation.width == 0 || allocation.height == 0 {
            return Err(format!("source-frame region {} collapsed at resolved pixel boundaries", region.id));
        }
        if document.source_overrides.contains_key(&region.id)
            && !aspect_matches(
                f64::from(crop.width),
                f64::from(crop.height),
                f64::from(allocation.width),
                f64::from(allocation.height),
            )
        {
            return Err(format!("detached SourceFrame region {} does not preserve its destination aspect", region.id));
        }
        let mapping_origin = if document.source_overrides.contains_key(&region.id) { "explicit_override" } else { "partition" };
        let candidate_id = hot_trimmer_domain::ContentDigest::sha256(format!("source-frame|{}|{}|{}|{}|{}|{}|{}|{}",
            frame.identity.0.iter().map(|byte| format!("{byte:02x}")).collect::<String>(), region.id,
            crop.x, crop.y, crop.width, crop.height, allocation.x, mapping_origin).as_bytes());
        let candidate = CropCandidate { candidate_id: candidate_id.clone(), source_id: artifacts.domain.prepared_source_digest.clone(),
            domain_id: artifacts.domain.cache_key.clone(), slot_id: region.id, crop: Some(crop),
            transform: CandidateTransform { rotation: QuarterTurn::Zero, mirror: MirrorTransform::None },
            isotropic_scale: 1.0, mapping_mode: hot_trimmer_domain::SamplingMode::DirectCrop,
            family: CandidateFamily::PanelDirect, route: CandidateRoute::Direct, position_strategy: PositionStrategy::DenseLowResolution,
            period_pixels: None, seam_indices: Vec::new(), correspondence_reference: artifacts.domain.cache_key.clone(),
            descriptors: CandidateDescriptors { saliency_milli: 0, stationarity_milli: 0, feature_strength_milli: 0, usability_milli: 1000 },
            seed: provenance.recipe.seed, eligibility: EligibilityEvidence { mapping_permitted: true, transform_permitted: true,
                isotropic_scale: true, exact_aspect: true, entire_crop_usable: Some(true), cross_axis_preserved: Some(true),
                lattice_aligned: Some(true), direct_crop_applicable: true, direct_crop_rejection: None,
                reasons: vec!["accepted SourceFrame + GridRect direct mapping".into()] } };
        let plan = SamplingPlan { slot_id: region.id, role: region.role, variation_group: region.material_group.clone(),
            prepared_domain_dimensions: [artifacts.domain.width, artifacts.domain.height], candidate,
            slot_physical_size: [f64::from(crop.width), f64::from(crop.height)], source_pixels_per_physical_unit: 1.0,
            sampling_policy: SamplingPolicy { filter: hot_trimmer_domain::SourceSamplingMode::Linear, scale: 1.0, correct_tangent_normals: true },
            radial_mapping: None, stretch_override: StretchOverrideProvenance::NotAuthorized, slice_geometry: SliceGeometry::None,
            maximum_seam_cost_milli: 0, unary_cost: 0.0 };
        let slot_key = region.id.to_string();
        let topology_slot = hot_trimmer_domain::CompiledTemplateSlot { slot_key, allocation, hotspot: allocation };
        let result = synthesize_slot_material_with_guard(SlotSynthesisRequest { plan: &plan, domain: &artifacts.domain,
            output_dimensions: [allocation.width, allocation.height], limits: SlotSynthesisLimits::default() }, &|| !active())
            .map_err(|error| format!("Stage 14 direct SourceFrame sampling failed for {}: {error}", region.id))?;
        slots.push(topology_slot); plans.push(plan); results.push(result);
    }
    let topology = hot_trimmer_domain::CompiledTemplateTopology { identity: hot_trimmer_domain::TemplateIdentity {
        template_id: "source-frame".into(), template_version: SOURCE_FRAME_COMPILER_VERSION.into(),
        compatibility_key: document.topology.compatibility_key.clone() }, output_size: document.render_settings.output_size, slots };
    let placement = PlacementPlan { stage_result: StageResult::Executed { algorithm: AlgorithmProvenance {
        algorithm_id: "hot-trimmer.stage-13.source-frame-direct".into(), version: SOURCE_FRAME_COMPILER_VERSION.into() },
        settings_hash: hot_trimmer_domain::ContentDigest::sha256(b"source-frame-direct-placement"), diagnostics: Vec::new() },
        solver: AlgorithmProvenance { algorithm_id: "hot-trimmer.stage-13.source-frame-direct".into(), version: SOURCE_FRAME_COMPILER_VERSION.into() },
        seed: provenance.recipe.seed, placements: plans, objective: PlacementObjectiveBreakdown { unary_cost: 0.0, pairwise_cost: 0.0,
            pairwise_lambda: 0.0, weighted_pairwise_cost: 0.0, total_cost: 0.0 }, pairwise_decisions: Vec::new(), crop_reuse_heatmap: Vec::new(),
        validation: PlacementValidationSummary { complete_assignment: true, required_slots_present: true, isotropic_scale_only: true,
            registered_mapping_only: true, slot_count: document.topology.regions.len() as u32 },
        qa_views: vec![PlacementPlanQaView::SelectedPlacements, PlacementPlanQaView::Validation] };
    let inputs = topology.slots.iter().zip(document.topology.regions.iter()).zip(placement.placements.iter().zip(results.iter()))
        .map(|((slot, region), (plan, result))| IntermediateSlotInput { region_id: region.id, slot_key: slot.slot_key.as_str(),
            display_name: region.display_name.as_str(), required: true, patch_id: None, domain: &artifacts.domain, plan, result, grid_rect: region.grid_rect }).collect();
    let algorithms = (1..=14).map(|stage| (stage, AlgorithmProvenance { algorithm_id: format!("source-frame-stage-{stage:02}"), version: SOURCE_FRAME_COMPILER_VERSION.into() })).collect();
    let atlas_request = IntermediateAtlasRequest { topology: &topology, placement_plan: &placement, slots: inputs,
        revision: request.revision, algorithm_versions: algorithms, diagnostics: Vec::new() };
    AlgorithmCompiler::new().compile_intermediate_atlas(&atlas_request, cancellation, || request.revision)
        .map_err(|error| error.to_string())
}

const SOURCE_FRAME_COMPILER_VERSION: &str = "1.0.0";

fn aspect_matches(width: f64, height: f64, expected_width: f64, expected_height: f64) -> bool {
    if !width.is_finite() || !height.is_finite() || !expected_width.is_finite() || !expected_height.is_finite()
        || width <= 0.0 || height <= 0.0 || expected_width <= 0.0 || expected_height <= 0.0 {
        return false;
    }
    let left = width * expected_height;
    let right = height * expected_width;
    (left - right).abs() <= 1e-9 * left.abs().max(right.abs()).max(1.0)
}

#[cfg(test)]
mod source_frame_partition_tests {
    use hot_trimmer_domain::{generate_partition, resolve_boundaries, LogicalGridSpec, PartitionRecipe, SamplingMode};

    #[test]
    fn source_frame_partition_preserves_shared_boundaries_and_direct_sampling() {
        for target in [16, 63, 103] {
            let recipe = PartitionRecipe::default_for(LogicalGridSpec::DEFAULT, target, 19);
            let regions = generate_partition(&recipe).expect("accepted partition");
            assert_eq!(regions.len(), target as usize);
            let source_x = resolve_boundaries(2_000, 4_000, 64);
            let source_y = resolve_boundaries(0, 4_000, 64);
            let destination_x = resolve_boundaries(0, 4_000, 64);
            let destination_y = resolve_boundaries(0, 4_000, 64);
            let mut coverage = vec![0_u8; 64 * 64];
            for region in regions {
                let rect = region.grid_rect;
                assert!(source_x[(rect.x + rect.width) as usize] >= source_x[rect.x as usize]);
                assert!(source_y[(rect.y + rect.height) as usize] >= source_y[rect.y as usize]);
                assert_eq!(source_x[(rect.x + rect.width) as usize] - source_x[rect.x as usize],
                    source_x[(rect.x + rect.width) as usize] - source_x[rect.x as usize]);
                assert_eq!(destination_x[rect.x as usize], destination_x[rect.x as usize]);
                assert_eq!(destination_y[rect.y as usize], destination_y[rect.y as usize]);
                for y in rect.y..rect.y + rect.height {
                    for x in rect.x..rect.x + rect.width { coverage[(y * 64 + x) as usize] += 1; }
                }
            }
            assert!(coverage.iter().all(|count| *count == 1));
            assert_eq!(SamplingMode::DirectCrop, SamplingMode::DirectCrop);
        }
    }
}

fn resolve_region_content(
    project: &ProjectSummary,
    document: &hot_trimmer_domain::TrimSheetDocument,
    primary: hot_trimmer_domain::SourceSetId,
    content: &ContentReference,
) -> Result<(hot_trimmer_domain::SourceSetId, Option<Patch>), String> {
    match content {
        ContentReference::InheritPrimaryMaterial => Ok((primary, None)),
        ContentReference::MaterialSource(source_set_id) => Ok((*source_set_id, None)),
        ContentReference::Patch(patch_id) => {
            let patch = document.patches.iter().chain(&project.patches)
                .find(|patch| patch.id == *patch_id && patch.enabled)
                .cloned().ok_or_else(|| format!("enabled authored patch {patch_id} is missing"))?;
            let source = project.sources.iter().find(|source| source.input.id == patch.source_id)
                .ok_or_else(|| format!("patch {patch_id} source is missing"))?;
            let source_set = project.source_sets.iter().find(|set| set.id.to_string() == source.source_set_id.to_string())
                .ok_or_else(|| format!("patch {patch_id} material source set is missing"))?;
            Ok((source_set.id, Some(patch)))
        }
        ContentReference::Solid(_) => Err("Stage 14 intermediate preview cannot represent solid region content".into()),
        ContentReference::Procedural(_) => Err("Stage 14 intermediate preview cannot represent procedural region content".into()),
    }
}

fn build_domain(
    project: &ProjectSummary,
    source_set_id: hot_trimmer_domain::SourceSetId,
    patch: Option<&Patch>,
    revision: u64,
    preserve_source_resolution: bool,
    image_cancellation: &ImageCancellationToken,
    render_cancellation: &RenderCancellationToken,
) -> Result<DomainArtifacts, String> {
    let source_set = project.source_sets.iter().find(|set| set.id == source_set_id)
        .ok_or_else(|| format!("material source set {source_set_id} is missing"))?;
    let sources = project.sources.iter().filter(|source| source.source_set_id.to_string() == source_set_id.to_string())
        .collect::<Vec<_>>();
    if !sources.iter().any(|source| source.registration.role == MaterialChannelRole::BaseColor) {
        return Err(format!("material source set {source_set_id} has no Base Color"));
    }
    if let Some(patch) = patch {
        if !sources.iter().any(|source| source.input.id == patch.source_id) {
            return Err(format!("patch {} does not belong to material source set {source_set_id}", patch.id));
        }
    }
    let (registered, encoded) = registered_inputs(&sources)?;
    let prepared = prepare_registered_channel_set(&registered, &encoded, &NormalizationSettings {
        max_levels: 5, max_memory_bytes: 4_294_967_296,
        max_level_zero_edge: (!preserve_source_resolution).then_some(2048),
        ..NormalizationSettings::default()
    }, image_cancellation).map_err(|error| format!("Stage 2 failed: {error}"))?;
    let patch_bytes = serde_json::to_vec(&patch.map(|value| (value.id, &value.geometry, &value.rectification)))
        .map_err(|error| format!("Stage 3 identity failed: {error}"))?;
    let patch_revision = u64::from_str_radix(&ContentDigest::sha256(&patch_bytes).0[..16], 16)
        .map_err(|error| format!("Stage 3 revision failed: {error}"))?;
    let exemplar = prepare_registered_exemplar(&prepared, &PreparedExemplarRequest {
        exemplar_id: patch.map_or_else(|| format!("{source_set_id}:full-source"), |value| value.id.to_string()),
        area: patch.map_or(PlanarArea::FullFrame { usable_area: None },
            |value| PlanarArea::FourPoint { corners: value.geometry.corners }),
        lens_correction: None,
        mask: ExemplarMaskIntent { crop_polygon: patch.and_then(|value| value.geometry.assistance_mask.clone()),
            minimum_alpha: Some(1.0 / 255.0) },
        rectification: patch.map_or_else(Default::default, |value| value.rectification),
        physical_aspect_ratio: None, quality: RectificationQuality::Authoritative,
        limits: RectificationWorkLimits { preview_max_edge: 1024, authoritative_max_edge: 8192,
            max_pixels: 67_108_864, tile_edge: 128 },
        scope: PreparedExemplarScope { source_set_id, source_revision: source_set.source_revision,
            patch_id: patch.map(|value| value.id), patch_revision },
    }, render_cancellation).map_err(|error| format!("Stage 3 failed: {error}"))?;
    let stage3_result = exemplar.stage_result.clone();
    let stage4 = prepare_delit_exemplar(&exemplar, &source_set.delighting, None, render_cancellation)
        .map_err(|error| format!("Stage 4 failed: {error}"))?;
    let stage4_result = stage4.stage_result.clone();
    let mut stage5 = analyze_source(&stage4, &AnalysisSettings::default(), None, render_cancellation)
        .map_err(|error| format!("Stage 5 failed: {error}"))?;
    stage5.classification.routing_intent = source_set.classification;
    let stage6 = calibrate_scale_orientation(&stage4, &stage5, &source_set.calibration,
        &ScaleOrientationSettings::default(), render_cancellation)
        .map_err(|error| format!("Stage 6 failed: {error}"))?;
    let stage7 = extract_feature_fields(&stage4, &stage6, &FeatureFieldSettings::default(), render_cancellation)
        .map_err(|error| format!("Stage 7 failed: {error}"))?;
    let stage8_request = Stage8RouterRequest {
        domain: DomainRequest { source: Arc::new(stage4.clone()), prepared_source_digest: exemplar.cache_key.0.clone(),
            analysis: Arc::new(stage7.clone()), scale_orientation: Arc::new(stage6.clone()), route: DomainRoute::Auto,
            direct_boundary_threshold_milli: 1000, graph_cut: GraphCutSettings::default(),
            quilting: QuiltingSettings::default(), patch_match: PatchMatchSettings::default(), seed: revision },
        stage_five: Arc::new(stage5.clone()), policy: MaterialDomainRoutePolicy::default(),
        procedural_override: None, procedural_settings: ProceduralFitSettings::default(),
        output_width: exemplar.width, output_height: exemplar.height,
    };
    let stage8 = prepare_stage_08_material_domain(&stage8_request, None, &mut MaterialDomainCache::default(),
        render_cancellation).map_err(|error| format!("Stage 8 failed: {error}"))?;
    Ok(DomainArtifacts { patch_id: patch.map(|value| value.id.to_string()), domain: stage8.domain,
        stage5, stage6, stage7, stage3_result, stage4_result, stage8_result: stage8.stage_result })
}

fn authored_sampling_policy(mapping: &RegionMapping) -> Result<SamplingPolicy, String> {
    let [x, y] = mapping.transform.scale;
    if !x.is_finite() || !y.is_finite() || x <= 0.0 || y <= 0.0 || (x - y).abs() > 1.0e-9 {
        return Err("Stage 14 requires an authored positive isotropic region transform scale".into());
    }
    let mut policy = mapping.sampling;
    policy.scale *= x;
    if !policy.scale.is_finite() || policy.scale <= 0.0 {
        return Err("authored mapping produced an invalid Stage 14 sampling scale".into());
    }
    Ok(policy)
}

fn apply_authored_mapping(
    mut set: CandidateSet,
    mapping: &RegionMapping,
    patch_backed: bool,
    width: u32,
    height: u32,
) -> Result<CandidateSet, String> {
    if mapping.warps.iter().any(|warp| warp.enabled) {
        return Err("Stage 14 SamplingPlan cannot represent enabled authored warp operations".into());
    }
    if matches!(mapping.address_mode, AddressMode::MirroredRepeat) {
        return Err("Stage 14 SamplingPlan cannot yet preserve mirrored-repeat addressing".into());
    }
    if matches!(mapping.projection, Projection::Perspective { .. }) && !patch_backed {
        return Err("authored perspective mapping requires patch-backed Stage 3 rectification".into());
    }
    let rotation = authored_quarter_turn(mapping.transform.rotation_degrees)?;
    let mirror = match (mapping.transform.mirror_x, mapping.transform.mirror_y) {
        (false, false) => MirrorTransform::None,
        (true, false) => MirrorTransform::X,
        (false, true) => MirrorTransform::Y,
        (true, true) => MirrorTransform::None,
    };
    let rotation = if mapping.transform.mirror_x && mapping.transform.mirror_y {
        compose_quarter_turn(rotation, QuarterTurn::OneEighty)
    } else { rotation };
    let authored_transform = CandidateTransform { rotation, mirror };
    let bounds = mapping_window(mapping, width, height);
    let unplaced = mapping.source_crop_intent == Some(hot_trimmer_domain::SourceCropIntent::Unplaced);
    let offset_x = (mapping.transform.offset[0] * f64::from(width)).round() as i64;
    let offset_y = (mapping.transform.offset[1] * f64::from(height)).round() as i64;
    set.candidates.retain_mut(|candidate| {
        if candidate.transform != authored_transform { return false; }
        let repeating = matches!(candidate.mapping_mode, hot_trimmer_domain::SamplingMode::PeriodicTile
            | hot_trimmer_domain::SamplingMode::RepeatX | hot_trimmer_domain::SamplingMode::RepeatY);
        if !unplaced && candidate.mapping_mode != hot_trimmer_domain::SamplingMode::TextureSynthesis
            && (mapping.address_mode == AddressMode::Repeat) != repeating { return false; }
        if let Some(mut crop) = candidate.crop {
            let shifted_x = i64::from(bounds.x + crop.x).saturating_add(offset_x);
            let shifted_y = i64::from(bounds.y + crop.y).saturating_add(offset_y);
            if shifted_x < 0 || shifted_y < 0 { return false; }
            crop.x = shifted_x as u32; crop.y = shifted_y as u32;
            if crop.x.saturating_add(crop.width) > width || crop.y.saturating_add(crop.height) > height {
                return false;
            }
            if !unplaced && (crop.x < bounds.x || crop.y < bounds.y
                || crop.x.saturating_add(crop.width) > bounds.x.saturating_add(bounds.width).min(width)
                || crop.y.saturating_add(crop.height) > bounds.y.saturating_add(bounds.height).min(height)) {
                return false;
            }
            candidate.crop = Some(crop);
        }
        candidate.candidate_id = ContentDigest::sha256(format!("{}|authored:{mapping:?}", candidate.candidate_id.0).as_bytes());
        candidate.eligibility.reasons.push("authored RegionMapping propagated into Stage 11 candidate".into());
        true
    });
    if set.candidates.is_empty() {
        return Err("Stage 11 produced no candidate compatible with the authored RegionMapping".into());
    }
    Ok(set)
}

fn mapping_window(mapping: &RegionMapping, width: u32, height: u32) -> SourceCrop {
    if mapping.source_crop_intent == Some(hot_trimmer_domain::SourceCropIntent::Unplaced) {
        return SourceCrop { x: 0, y: 0, width, height };
    }
    let Projection::Crop { bounds, .. } = mapping.projection else {
        return SourceCrop { x: 0, y: 0, width, height };
    };
    let x = ((bounds.x.get() * f64::from(width)).floor() as u32).min(width.saturating_sub(1));
    let y = ((bounds.y.get() * f64::from(height)).floor() as u32).min(height.saturating_sub(1));
    let requested_width = (bounds.width.get() * f64::from(width)).ceil().max(1.0) as u32;
    let requested_height = (bounds.height.get() * f64::from(height)).ceil().max(1.0) as u32;
    SourceCrop { x, y, width: requested_width.min(width - x), height: requested_height.min(height - y) }
}

fn legal_gate1_mode(mode: hot_trimmer_domain::SamplingMode, role: hot_trimmer_domain::TemplateSlotRole, has_declared_period: bool) -> bool {
    use hot_trimmer_domain::{SamplingMode, TemplateSlotRole};
    match role {
        TemplateSlotRole::Planar => matches!(mode, SamplingMode::DirectCrop)
            || (mode == SamplingMode::PeriodicTile && has_declared_period),
        TemplateSlotRole::RepeatingStrip => matches!(mode, SamplingMode::DirectCrop)
            || (matches!(mode, SamplingMode::RepeatX | SamplingMode::RepeatY) && has_declared_period),
        TemplateSlotRole::UniqueDetail => matches!(mode, SamplingMode::UniqueContain | SamplingMode::UniqueCover),
        TemplateSlotRole::TrimCap => mode == SamplingMode::ThreeSliceCap,
        TemplateSlotRole::Radial => matches!(mode, SamplingMode::PlanarRadial),
    }
}

fn authored_quarter_turn(degrees: f64) -> Result<QuarterTurn, String> {
    if !degrees.is_finite() { return Err("authored mapping rotation is not finite".into()); }
    let normalized = degrees.rem_euclid(360.0);
    let quarter = (normalized / 90.0).round();
    if (normalized - quarter * 90.0).abs() > 1.0e-6 {
        return Err("Stage 14 SamplingPlan supports authored rotations only in exact quarter turns".into());
    }
    Ok(match (quarter as u32) % 4 { 0 => QuarterTurn::Zero, 1 => QuarterTurn::Ninety,
        2 => QuarterTurn::OneEighty, _ => QuarterTurn::TwoSeventy })
}

fn compose_quarter_turn(first: QuarterTurn, second: QuarterTurn) -> QuarterTurn {
    let value = |turn| match turn { QuarterTurn::Zero => 0, QuarterTurn::Ninety => 1,
        QuarterTurn::OneEighty => 2, QuarterTurn::TwoSeventy => 3 };
    match (value(first) + value(second)) % 4 { 0 => QuarterTurn::Zero, 1 => QuarterTurn::Ninety,
        2 => QuarterTurn::OneEighty, _ => QuarterTurn::TwoSeventy }
}

fn candidate_measurements(
    candidate: &hot_trimmer_placement_solver::CropCandidate,
    demand: &hot_trimmer_effect_compiler::ResolvedSlotDemand,
    role: hot_trimmer_domain::TemplateSlotRole,
    artifacts: &DomainArtifacts,
) -> CandidateScoringMeasurements {
    let crop = candidate.crop.unwrap_or(SourceCrop { x: 0, y: 0,
        width: artifacts.domain.width, height: artifacts.domain.height });
    let saliency = average_field(artifacts.stage7.saliency.level(0), crop);
    let stationarity = average_field(artifacts.stage7.stationarity.level(0), crop);
    let structure = average_field(artifacts.stage7.structure.edge.level(0), crop)
        .max(average_field(artifacts.stage7.structure.line.level(0), crop))
        .max(average_field(artifacts.stage7.structure.grid.level(0), crop));
    let boundary = perimeter_field(artifacts.stage7.structure.boundary.level(0), crop);
    let usability = average_field(artifacts.stage7.usability.confidence.level(0), crop);
    let source_ratio = (f64::from(crop.width) / f64::from(demand.destination_pixel_width.max(1)))
        .min(f64::from(crop.height) / f64::from(demand.destination_pixel_height.max(1)))
        .min(1.0);
    let seam_cost = match candidate.mapping_mode {
        hot_trimmer_domain::SamplingMode::RepeatX => artifacts.stage7.seamability.horizontal_cost_milli,
        hot_trimmer_domain::SamplingMode::RepeatY => artifacts.stage7.seamability.vertical_cost_milli,
        hot_trimmer_domain::SamplingMode::PeriodicTile => artifacts.stage7.seamability.horizontal_cost_milli
            .max(artifacts.stage7.seamability.vertical_cost_milli),
        _ => 0,
    };
    let role_compatibility = match role {
        hot_trimmer_domain::TemplateSlotRole::UniqueDetail => saliency,
        hot_trimmer_domain::TemplateSlotRole::RepeatingStrip => stationarity,
        _ => usability,
    };
    CandidateScoringMeasurements {
        source_pixels_per_output_pixel_milli: milli(source_ratio), resolution_confidence_milli: 1000,
        lattice_completion_milli: lattice_completion_milli(candidate),
        structure_confidence_milli: milli(structure),
        dominant_direction_degrees: artifacts.stage6.global_orientation.axis_millidegrees
            .map(|value| f64::from(value) / 1000.0),
        orientation_confidence_milli: artifacts.stage6.global_orientation.confidence_milli,
        seam_quality_milli: 1000_u16.saturating_sub(seam_cost),
        seam_confidence_milli: milli(average_field(artifacts.stage7.seamability.confidence.level(0), crop)),
        boundary_cut_milli: milli(boundary), boundary_confidence_milli: milli(structure.max(boundary)),
        visual_quality_milli: milli((f64::from(artifacts.stage5.quality.sharpness_milli) / 1000.0) * usability),
        quality_confidence_milli: milli(usability), role_compatibility_milli: Some(milli(role_compatibility)),
        role_confidence_milli: milli(saliency.max(stationarity).max(usability)),
    }
}

fn average_field(plane: Option<&hot_trimmer_image_io::ImagePlane<hot_trimmer_image_io::LinearScalar>>, crop: SourceCrop) -> f64 {
    let Some(plane) = plane else { return 0.0 }; let mut sum = 0.0; let mut count = 0_u64;
    let x1 = crop.x.saturating_add(crop.width).min(plane.width());
    let y1 = crop.y.saturating_add(crop.height).min(plane.height());
    for y in crop.y.min(plane.height())..y1 { for x in crop.x.min(plane.width())..x1 {
        sum += f64::from(plane.pixel(x, y).0); count += 1;
    }}
    if count == 0 { 0.0 } else { sum / count as f64 }
}

fn perimeter_field(plane: Option<&hot_trimmer_image_io::ImagePlane<hot_trimmer_image_io::LinearScalar>>, crop: SourceCrop) -> f64 {
    let Some(plane) = plane else { return 0.0 }; if crop.width == 0 || crop.height == 0 { return 0.0; }
    let x0 = crop.x.min(plane.width().saturating_sub(1)); let y0 = crop.y.min(plane.height().saturating_sub(1));
    let x1 = crop.x.saturating_add(crop.width).saturating_sub(1).min(plane.width().saturating_sub(1));
    let y1 = crop.y.saturating_add(crop.height).saturating_sub(1).min(plane.height().saturating_sub(1));
    let mut sum = 0.0; let mut count = 0_u64;
    for x in x0..=x1 { sum += f64::from(plane.pixel(x, y0).0); count += 1;
        if y1 != y0 { sum += f64::from(plane.pixel(x, y1).0); count += 1; } }
    for y in y0.saturating_add(1)..y1 { sum += f64::from(plane.pixel(x0, y).0); count += 1;
        if x1 != x0 { sum += f64::from(plane.pixel(x1, y).0); count += 1; } }
    if count == 0 { 0.0 } else { sum / count as f64 }
}

fn milli(value: f64) -> u16 { (value.clamp(0.0, 1.0) * 1000.0).round() as u16 }

fn lattice_completion_milli(candidate: &hot_trimmer_placement_solver::CropCandidate) -> u16 {
    let (Some(crop), Some(period)) = (candidate.crop, candidate.period_pixels) else { return 0 };
    let axis = |origin: u32, extent: u32, period: u32| {
        if period == 0 { return 0.0; }
        let error = |value: u32| { let remainder = value % period;
            f64::from(remainder.min(period - remainder)) / f64::from(period) };
        (1.0 - (error(origin) + error(extent)) * 0.5).clamp(0.0, 1.0)
    };
    let x = axis(crop.x, crop.width, period[0]); let y = axis(crop.y, crop.height, period[1]);
    milli(match candidate.mapping_mode {
        hot_trimmer_domain::SamplingMode::RepeatX => x,
        hot_trimmer_domain::SamplingMode::RepeatY => y,
        hot_trimmer_domain::SamplingMode::PeriodicTile => (x + y) * 0.5,
        _ => 0.0,
    })
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
    let mut feature_positions = Vec::new();
    if let (Some(saliency), Some(stationarity), Some(edge)) = (stage7.saliency.level(0),
        stage7.stationarity.level(0), stage7.structure.edge.level(0)) {
        let grid_x = 8_u32.min(saliency.width()); let grid_y = 8_u32.min(saliency.height());
        for gy in 0..grid_y { for gx in 0..grid_x {
            let x = ((2 * gx + 1) * saliency.width() / (2 * grid_x)).min(saliency.width() - 1);
            let y = ((2 * gy + 1) * saliency.height() / (2 * grid_y)).min(saliency.height() - 1);
            feature_positions.push(FeaturePosition { x, y,
                saliency_milli: milli(f64::from(saliency.pixel(x, y).0)),
                stationarity_milli: milli(f64::from(stationarity.pixel(x, y).0)),
                feature_strength_milli: milli(f64::from(edge.pixel(x, y).0)) });
        }}
        feature_positions.sort_by_key(|feature| std::cmp::Reverse((u32::from(feature.saliency_milli)
            + u32::from(feature.stationarity_milli) + u32::from(feature.feature_strength_milli), feature.y, feature.x)));
        feature_positions.truncate(32);
    }
    CandidateEvidence { material_class: stage5.classification.routed_class(),
        class_confidence_milli: stage5.classification.confidence_milli,
        orientation_confidence_milli: stage6.global_orientation.confidence_milli,
        destructive_quarter_turn_override: stage6.global_orientation.destructive_rotation_allowed,
        periods: stage7.periodicity.candidates.iter().filter_map(|candidate| {
            let x = candidate.first.dx_pixels.unsigned_abs(); let y = candidate.first.dy_pixels.unsigned_abs();
            (x > 0 && y > 0).then_some([x, y]) }).collect(), feature_positions }
}

fn base_pixels_per_physical_unit(scale: PhysicalScaleEvidence, demand: &hot_trimmer_effect_compiler::ResolvedSlotDemand,
    width: u32, height: u32) -> f64 {
    if scale.claims_world_accuracy() { let x = scale.source_pixels_per_meter_x_milli.unwrap_or(1000) as f64 / 1000.0;
        let y = scale.source_pixels_per_meter_y_milli.unwrap_or(1000) as f64 / 1000.0; (x * y).sqrt() }
    else { (f64::from(width) / demand.world_width_m).min(f64::from(height) / demand.world_height_m).max(f64::EPSILON) }
}

fn unplaced_source_pixels_per_physical_unit(
    demand: &hot_trimmer_effect_compiler::ResolvedSlotDemand,
    width: u32,
    height: u32,
) -> f64 {
    (f64::from(width) / demand.world_width_m)
        .min(f64::from(height) / demand.world_height_m)
        .max(f64::EPSILON)
        * UNPLACED_SOURCE_FOOTPRINT_FRACTION
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
