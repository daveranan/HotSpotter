use std::collections::BTreeMap;

use hot_trimmer_domain::{
    AlgorithmProvenance, CancellationToken, CompilationDiagnostic, ContentDigest, DiagnosticCode,
    MaterialBehaviorClass, RadialMappingSettings, RecoveryChoice, RegionId, SamplingPolicy,
    StageResult, TemplateSlotRole,
};
use serde::{Deserialize, Serialize};

use crate::{CropCandidate, ScoredCandidate, ScoredCandidateSet, SourceCrop};

pub const STAGE_13_ALGORITHM_ID: &str = "hot-trimmer.stage-13.global-placement";
pub const STAGE_13_ALGORITHM_VERSION: &str = "1.0.0";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReusePermissions {
    pub stochastic_overlap: bool,
    pub manufactured_periodic_reuse: bool,
    pub repeated_salient_feature: bool,
    /// Fixed-template unplaced slots must not silently select the same source
    /// rectangle. Periodic strip reuse remains legal when explicitly identified
    /// by the candidate's period metadata.
    #[serde(default)]
    pub require_spatially_distinct_crops: bool,
}

impl Default for ReusePermissions {
    fn default() -> Self {
        Self {
            stochastic_overlap: true,
            manufactured_periodic_reuse: true,
            repeated_salient_feature: false,
            require_spatially_distinct_crops: false,
        }
    }
}

/// Owned Stage 13 input. `candidates` is exactly the stable Stage 12 top-K artifact.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacementSlotInput {
    pub slot_id: RegionId,
    pub role: TemplateSlotRole,
    pub material_behavior: MaterialBehaviorClass,
    pub variation_group: String,
    pub visual_importance_milli: u16,
    pub constraint_tightness_milli: u16,
    pub required: bool,
    pub prepared_domain_id: ContentDigest,
    pub prepared_domain_dimensions: [u32; 2],
    /// Bounded prepared-domain window selected by the authored mapping. TextureSynthesis
    /// samples this window directly instead of fabricating a full-domain crop.
    pub prepared_domain_sampling_window: SourceCrop,
    pub registered_correspondence_reference: ContentDigest,
    /// Authoritative Stage 10 physical extent in meters (or the declared relative physical unit).
    pub slot_physical_size: [f64; 2],
    /// Stage 10/6 conversion before the Stage 11 dimensionless scale-ladder multiplier.
    pub base_source_pixels_per_physical_unit: f64,
    pub sampling_policy: SamplingPolicy,
    pub radial_mapping: Option<RadialMappingSettings>,
    pub stretch_override: StretchOverrideProvenance,
    pub slice_geometry: SliceGeometry,
    /// Maximum accepted cost for every selected Stage 8 seam used by this slot.
    pub maximum_seam_cost_milli: u16,
    pub reuse_permissions: ReusePermissions,
    pub candidates: ScoredCandidateSet,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SliceCenterPolicy {
    Repeat,
    Synthesize,
    ExplicitStretch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SliceGeometry {
    None,
    Three {
        leading_cap_pixels: u32,
        trailing_cap_pixels: u32,
        center: SliceCenterPolicy,
    },
    Nine {
        left_pixels: u32,
        right_pixels: u32,
        top_pixels: u32,
        bottom_pixels: u32,
        center: SliceCenterPolicy,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum StretchOverrideProvenance {
    NotAuthorized,
    UserOverride { settings_revision: u64 },
}

/// Authoritative source-coordinate basis for Stage 14. A prepared domain is not an
/// authored crop even when its executable window spans the complete domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SamplingBasis {
    SelectedCrop,
    PreparedDomain { window: SourceCrop },
}

impl Default for SamplingBasis {
    fn default() -> Self {
        Self::SelectedCrop
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlacementOptimizerSettings {
    pub beam_width: usize,
    pub pairwise_lambda: f64,
    pub max_pairwise_evaluations: u64,
    pub max_local_evaluations: u64,
    pub local_passes: u8,
    pub salient_threshold_milli: u16,
    pub large_slot_importance_milli: u16,
    pub heatmap_grid_edge: u8,
}

impl Default for PlacementOptimizerSettings {
    fn default() -> Self {
        Self {
            beam_width: 24,
            pairwise_lambda: 1.0,
            max_pairwise_evaluations: 1_000_000,
            max_local_evaluations: 100_000,
            local_passes: 3,
            salient_threshold_milli: 650,
            large_slot_importance_milli: 600,
            heatmap_grid_edge: 16,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairwiseCostTerm {
    SourceOverlap,
    DescriptorSimilarity,
    RepeatedSalientFeature,
    IdenticalTransform,
    VariationGroup,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairwiseTermBreakdown {
    pub term: PairwiseCostTerm,
    pub normalized_cost: f64,
    pub policy_multiplier: f64,
    pub weighted_cost: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairwiseCostBreakdown {
    pub terms: Vec<PairwiseTermBreakdown>,
    pub total_cost: f64,
    pub intentional_periodic_reuse: bool,
    pub stochastic_overlap_permitted: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairwiseDecisionOutcome {
    Accepted,
    RejectedByPolicy,
    RejectedByObjective,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairwiseDecision {
    pub first_slot_id: RegionId,
    pub first_candidate_id: ContentDigest,
    pub second_slot_id: RegionId,
    pub second_candidate_id: ContentDigest,
    pub outcome: PairwiseDecisionOutcome,
    pub reason: String,
    pub breakdown: PairwiseCostBreakdown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SamplingPlan {
    pub slot_id: RegionId,
    pub role: TemplateSlotRole,
    pub variation_group: String,
    pub prepared_domain_dimensions: [u32; 2],
    pub candidate: CropCandidate,
    #[serde(default)]
    pub sampling_basis: SamplingBasis,
    pub slot_physical_size: [f64; 2],
    /// Complete physical-to-domain coefficient. This is not the Stage 11 scale-ladder value.
    pub source_pixels_per_physical_unit: f64,
    /// Authoritative raster policy. Stage 14 must consume this value for every channel.
    pub sampling_policy: SamplingPolicy,
    pub radial_mapping: Option<RadialMappingSettings>,
    /// Visible provenance for the only route allowed to use non-uniform scale.
    pub stretch_override: StretchOverrideProvenance,
    pub slice_geometry: SliceGeometry,
    pub maximum_seam_cost_milli: u16,
    pub unary_cost: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacementObjectiveBreakdown {
    pub unary_cost: f64,
    pub pairwise_cost: f64,
    pub pairwise_lambda: f64,
    pub weighted_pairwise_cost: f64,
    pub total_cost: f64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacementValidationSummary {
    pub complete_assignment: bool,
    pub required_slots_present: bool,
    pub isotropic_scale_only: bool,
    pub registered_mapping_only: bool,
    pub slot_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CropReuseHeatmapCell {
    pub source_id: ContentDigest,
    pub domain_id: ContentDigest,
    pub x: u8,
    pub y: u8,
    pub reuse_count: u16,
    pub repetition_milli: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementPlanQaView {
    SelectedPlacements,
    ObjectiveBreakdown,
    PairwiseDecisions,
    SourceUsage,
    CropReuseHeatmap,
    RepetitionHeatmap,
    Validation,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacementPlan {
    pub stage_result: StageResult,
    pub solver: AlgorithmProvenance,
    pub seed: u64,
    pub placements: Vec<SamplingPlan>,
    pub objective: PlacementObjectiveBreakdown,
    pub pairwise_decisions: Vec<PairwiseDecision>,
    pub crop_reuse_heatmap: Vec<CropReuseHeatmapCell>,
    pub validation: PlacementValidationSummary,
    pub qa_views: Vec<PlacementPlanQaView>,
}

impl PlacementPlan {
    pub fn deterministic_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub fn objective_report_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(&(&self.objective, &self.pairwise_decisions, &self.validation))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlacementError {
    InvalidSettings,
    MalformedInput(String),
    ResourceLimitExceeded,
    Cancelled,
    InsufficientAssignment {
        diagnostic: CompilationDiagnostic,
        recovery_choices: Vec<RecoveryChoice>,
    },
}

#[derive(Clone)]
struct BeamState {
    selected: Vec<Option<usize>>,
    cost: f64,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct PairKey {
    first_slot_id: RegionId,
    first_candidate: String,
    second_slot_id: RegionId,
    second_candidate: String,
}

#[derive(Clone)]
struct PairEvaluation {
    accepted: bool,
    reason: String,
    breakdown: PairwiseCostBreakdown,
}

struct SolverContext<'a> {
    slots: &'a [PlacementSlotInput],
    settings: &'a PlacementOptimizerSettings,
    cancellation: &'a CancellationToken,
    pair_cache: BTreeMap<PairKey, PairEvaluation>,
    pair_evaluations: u64,
}

pub fn optimize_placements(
    slots: &[PlacementSlotInput],
    settings: &PlacementOptimizerSettings,
    seed: u64,
    cancellation: &CancellationToken,
) -> Result<PlacementPlan, PlacementError> {
    validate_inputs(slots, settings)?;
    if cancellation.is_cancelled() {
        return Err(PlacementError::Cancelled);
    }
    let mut context = SolverContext {
        slots,
        settings,
        cancellation,
        pair_cache: BTreeMap::new(),
        pair_evaluations: 0,
    };
    let order = slot_order(slots);
    let mut beam = vec![BeamState {
        selected: vec![None; slots.len()],
        cost: 0.0,
    }];
    for slot_index in order {
        let candidate_order = candidate_order(&slots[slot_index].candidates.top_candidates, seed);
        if candidate_order.is_empty() {
            return Err(insufficient(
                slots[slot_index].slot_id,
                "Stage 12 produced no legal top-K candidate",
            ));
        }
        let mut next = Vec::new();
        for state in &beam {
            for &candidate_index in &candidate_order {
                check_cancel(cancellation)?;
                let mut added = slots[slot_index].candidates.top_candidates[candidate_index]
                    .breakdown
                    .total_cost;
                let mut permitted = true;
                for (other_slot, selected) in state.selected.iter().enumerate() {
                    if let Some(other_candidate) = selected {
                        let pair = context.pair(
                            slot_index,
                            candidate_index,
                            other_slot,
                            *other_candidate,
                        )?;
                        if !pair.accepted {
                            permitted = false;
                            break;
                        }
                        added += settings.pairwise_lambda * pair.breakdown.total_cost;
                    }
                }
                if permitted {
                    let mut selected = state.selected.clone();
                    selected[slot_index] = Some(candidate_index);
                    next.push(BeamState {
                        selected,
                        cost: state.cost + added,
                    });
                }
            }
        }
        if next.is_empty() {
            return Err(insufficient(
                slots[slot_index].slot_id,
                "pairwise reuse policy left no complete candidate for a required slot",
            ));
        }
        next.sort_by(|a, b| {
            a.cost.total_cmp(&b.cost).then_with(|| {
                assignment_tie_key(&a.selected, slots, seed).cmp(&assignment_tie_key(
                    &b.selected,
                    slots,
                    seed,
                ))
            })
        });
        next.truncate(settings.beam_width);
        beam = next;
    }
    let mut selected = beam.remove(0).selected;
    local_improvement(&mut context, &mut selected, seed)?;
    check_cancel(cancellation)?;
    build_plan(context, selected, seed)
}

fn validate_inputs(
    slots: &[PlacementSlotInput],
    settings: &PlacementOptimizerSettings,
) -> Result<(), PlacementError> {
    if settings.beam_width == 0
        || !settings.pairwise_lambda.is_finite()
        || settings.pairwise_lambda < 0.0
        || settings.max_pairwise_evaluations == 0
        || settings.max_local_evaluations == 0
        || settings.local_passes == 0
        || settings.salient_threshold_milli > 1000
        || settings.large_slot_importance_milli > 1000
        || settings.heatmap_grid_edge == 0
        || settings.heatmap_grid_edge > 64
    {
        return Err(PlacementError::InvalidSettings);
    }
    if slots.is_empty() {
        return Err(PlacementError::MalformedInput(
            "placement request has no slots".into(),
        ));
    }
    let mut ids = slots.iter().map(|slot| slot.slot_id).collect::<Vec<_>>();
    ids.sort();
    if ids.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(PlacementError::MalformedInput("duplicate slot id".into()));
    }
    let mut domain_dimensions = BTreeMap::<ContentDigest, [u32; 2]>::new();
    for slot in slots {
        if slot.visual_importance_milli > 1000
            || slot.constraint_tightness_milli > 1000
            || slot.variation_group.trim().is_empty()
            || slot.prepared_domain_dimensions[0] == 0
            || slot.prepared_domain_dimensions[1] == 0
            || slot.maximum_seam_cost_milli > 1000
            || slot
                .slot_physical_size
                .iter()
                .any(|value| !value.is_finite() || *value <= 0.0)
            || !slot.base_source_pixels_per_physical_unit.is_finite()
            || slot.base_source_pixels_per_physical_unit <= 0.0
            || !slot.sampling_policy.scale.is_finite()
            || slot.sampling_policy.scale <= 0.0
        {
            return Err(PlacementError::MalformedInput(format!(
                "slot {} has malformed placement metadata",
                slot.slot_id
            )));
        }
        if domain_dimensions
            .insert(
                slot.prepared_domain_id.clone(),
                slot.prepared_domain_dimensions,
            )
            .is_some_and(|dimensions| dimensions != slot.prepared_domain_dimensions)
        {
            return Err(PlacementError::MalformedInput(
                "one prepared domain was supplied with conflicting dimensions".into(),
            ));
        }
        if slot.required && slot.candidates.top_candidates.is_empty() {
            return Err(insufficient(
                slot.slot_id,
                "required slot has no Stage 12 candidate",
            ));
        }
        for scored in &slot.candidates.top_candidates {
            let candidate = &scored.candidate;
            if candidate.slot_id != slot.slot_id
                || !scored.breakdown.total_cost.is_finite()
                || scored.breakdown.total_cost < 0.0
                || !candidate.isotropic_scale.is_finite()
                || candidate.isotropic_scale <= 0.0
                || !candidate.eligibility.isotropic_scale
                || candidate.domain_id != slot.prepared_domain_id
                || candidate.correspondence_reference != slot.registered_correspondence_reference
                || slot.prepared_domain_sampling_window.width == 0
                || slot.prepared_domain_sampling_window.height == 0
                || slot
                    .prepared_domain_sampling_window
                    .x
                    .saturating_add(slot.prepared_domain_sampling_window.width)
                    > slot.prepared_domain_dimensions[0]
                || slot
                    .prepared_domain_sampling_window
                    .y
                    .saturating_add(slot.prepared_domain_sampling_window.height)
                    > slot.prepared_domain_dimensions[1]
                || (candidate.mapping_mode == hot_trimmer_domain::SamplingMode::ExplicitStretch
                    && !matches!(
                        slot.stretch_override,
                        StretchOverrideProvenance::UserOverride { .. }
                    ))
                || (candidate.mapping_mode == hot_trimmer_domain::SamplingMode::ThreeSliceCap
                    && !matches!(slot.slice_geometry, SliceGeometry::Three { .. }))
                || (candidate.mapping_mode == hot_trimmer_domain::SamplingMode::NineSlicePanel
                    && !matches!(slot.slice_geometry, SliceGeometry::Nine { .. }))
                || candidate.crop.is_some_and(|crop| {
                    crop.width == 0
                        || crop.height == 0
                        || crop.x.saturating_add(crop.width) > slot.prepared_domain_dimensions[0]
                        || crop.y.saturating_add(crop.height) > slot.prepared_domain_dimensions[1]
                })
            {
                return Err(PlacementError::MalformedInput(format!(
                    "slot {} contains a non-isotropic, unregistered, or foreign candidate",
                    slot.slot_id
                )));
            }
        }
    }
    Ok(())
}

fn slot_order(slots: &[PlacementSlotInput]) -> Vec<usize> {
    let mut order = (0..slots.len()).collect::<Vec<_>>();
    order.sort_by(|&a, &b| {
        slots[b]
            .visual_importance_milli
            .cmp(&slots[a].visual_importance_milli)
            .then(
                slots[b]
                    .constraint_tightness_milli
                    .cmp(&slots[a].constraint_tightness_milli),
            )
            .then(
                slots[a]
                    .candidates
                    .top_candidates
                    .len()
                    .cmp(&slots[b].candidates.top_candidates.len()),
            )
            .then(slots[a].slot_id.cmp(&slots[b].slot_id))
    });
    order
}

fn candidate_order(candidates: &[ScoredCandidate], seed: u64) -> Vec<usize> {
    let mut order = (0..candidates.len()).collect::<Vec<_>>();
    order.sort_by(|&a, &b| {
        candidates[a]
            .breakdown
            .total_cost
            .total_cmp(&candidates[b].breakdown.total_cost)
            .then(
                seed_tie(seed, &candidates[a].candidate.candidate_id)
                    .cmp(&seed_tie(seed, &candidates[b].candidate.candidate_id)),
            )
            .then(
                candidates[a]
                    .candidate
                    .candidate_id
                    .cmp(&candidates[b].candidate.candidate_id),
            )
    });
    order
}

impl SolverContext<'_> {
    fn pair(
        &mut self,
        slot_a: usize,
        candidate_a: usize,
        slot_b: usize,
        candidate_b: usize,
    ) -> Result<PairEvaluation, PlacementError> {
        let (first_slot, first_candidate, second_slot, second_candidate) =
            if self.slots[slot_a].slot_id < self.slots[slot_b].slot_id {
                (slot_a, candidate_a, slot_b, candidate_b)
            } else {
                (slot_b, candidate_b, slot_a, candidate_a)
            };
        let first = &self.slots[first_slot].candidates.top_candidates[first_candidate].candidate;
        let second = &self.slots[second_slot].candidates.top_candidates[second_candidate].candidate;
        let key = PairKey {
            first_slot_id: self.slots[first_slot].slot_id,
            first_candidate: first.candidate_id.0.clone(),
            second_slot_id: self.slots[second_slot].slot_id,
            second_candidate: second.candidate_id.0.clone(),
        };
        if let Some(value) = self.pair_cache.get(&key) {
            return Ok(value.clone());
        }
        check_cancel(self.cancellation)?;
        if self.pair_evaluations >= self.settings.max_pairwise_evaluations {
            return Err(PlacementError::ResourceLimitExceeded);
        }
        self.pair_evaluations += 1;
        let value = evaluate_pair(
            &self.slots[first_slot],
            first,
            &self.slots[second_slot],
            second,
            self.settings,
        );
        self.pair_cache.insert(key, value.clone());
        Ok(value)
    }
}

fn evaluate_pair(
    first_slot: &PlacementSlotInput,
    first: &CropCandidate,
    second_slot: &PlacementSlotInput,
    second: &CropCandidate,
    settings: &PlacementOptimizerSettings,
) -> PairEvaluation {
    let same_coordinate_domain = first.source_id == second.source_id
        && first.domain_id == second.domain_id
        && first.correspondence_reference == second.correspondence_reference;
    let overlap = if same_coordinate_domain {
        crop_overlap(first.crop, second.crop)
    } else {
        0.0
    };
    let descriptor_similarity = descriptor_similarity(first, second);
    let salient = unit(
        first
            .descriptors
            .saliency_milli
            .min(second.descriptors.saliency_milli),
    ) * unit(
        first
            .descriptors
            .feature_strength_milli
            .min(second.descriptors.feature_strength_milli),
    ) * overlap;
    let identical = if same_coordinate_domain && first.transform == second.transform {
        overlap
    } else {
        0.0
    };
    let same_variation = first_slot.variation_group == second_slot.variation_group;
    let periodic = intentional_periodic(first_slot, first, second_slot, second);
    let stochastic = stochastic_overlap(first_slot, second_slot)
        && first_slot.reuse_permissions.stochastic_overlap
        && second_slot.reuse_permissions.stochastic_overlap
        && first.transform != second.transform;
    let unique_behavior = matches!(
        first_slot.material_behavior,
        MaterialBehaviorClass::UniqueDetail
    ) || matches!(
        second_slot.material_behavior,
        MaterialBehaviorClass::UniqueDetail
    );
    let large_slots = first_slot.visual_importance_milli >= settings.large_slot_importance_milli
        && second_slot.visual_importance_milli >= settings.large_slot_importance_milli;
    let salient_duplicate = same_coordinate_domain
        && overlap > 0.10
        && first.descriptors.saliency_milli >= settings.salient_threshold_milli
        && second.descriptors.saliency_milli >= settings.salient_threshold_milli
        && (large_slots || unique_behavior);
    let salient_reuse_permitted = first_slot.reuse_permissions.repeated_salient_feature
        && second_slot.reuse_permissions.repeated_salient_feature;
    let periodic_permitted = periodic
        && first_slot.reuse_permissions.manufactured_periodic_reuse
        && second_slot.reuse_permissions.manufactured_periodic_reuse;
    let identical_crop_reuse_penalized = same_coordinate_domain
        && !periodic_permitted
        && (first_slot
            .reuse_permissions
            .require_spatially_distinct_crops
            || second_slot
                .reuse_permissions
                .require_spatially_distinct_crops)
        && first.crop.is_some()
        && first.crop == second.crop;
    let accepted = !salient_duplicate || salient_reuse_permitted || periodic_permitted;

    let overlap_multiplier = if periodic_permitted {
        0.0
    } else if identical_crop_reuse_penalized {
        6.0
    } else if stochastic {
        0.18
    } else if unique_behavior {
        2.5
    } else {
        1.0
    };
    let descriptor_multiplier = if periodic_permitted { 0.08 } else { 0.45 };
    let salient_multiplier = if salient_reuse_permitted || periodic_permitted {
        0.15
    } else if unique_behavior || large_slots {
        8.0
    } else {
        3.0
    };
    let transform_multiplier = if periodic_permitted {
        0.0
    } else if stochastic {
        0.35
    } else {
        1.25
    };
    let variation_multiplier = if same_variation && !periodic_permitted {
        0.8
    } else {
        0.0
    };
    let values = [
        (PairwiseCostTerm::SourceOverlap, overlap, overlap_multiplier),
        (
            PairwiseCostTerm::DescriptorSimilarity,
            descriptor_similarity,
            descriptor_multiplier,
        ),
        (
            PairwiseCostTerm::RepeatedSalientFeature,
            salient,
            salient_multiplier,
        ),
        (
            PairwiseCostTerm::IdenticalTransform,
            identical,
            transform_multiplier,
        ),
        (
            PairwiseCostTerm::VariationGroup,
            if same_variation {
                descriptor_similarity * (0.5 + 0.5 * overlap)
            } else {
                0.0
            },
            variation_multiplier,
        ),
    ];
    let terms = values
        .into_iter()
        .map(
            |(term, normalized_cost, policy_multiplier)| PairwiseTermBreakdown {
                term,
                normalized_cost,
                policy_multiplier,
                weighted_cost: normalized_cost * policy_multiplier,
            },
        )
        .collect::<Vec<_>>();
    let total_cost = terms.iter().map(|term| term.weighted_cost).sum();
    PairEvaluation {
        accepted,
        reason: if accepted {
            if periodic_permitted {
                "accepted as intentional manufactured-periodic cycle reuse".into()
            } else if stochastic && overlap > 0.0 {
                "accepted stochastic overlap under transform-aware reuse policy".into()
            } else {
                "accepted by material-class reuse policy".into()
            }
        } else {
            "rejected: a unique salient source feature would repeat across large visible slots"
                .into()
        },
        breakdown: PairwiseCostBreakdown {
            terms,
            total_cost,
            intentional_periodic_reuse: periodic_permitted,
            stochastic_overlap_permitted: stochastic,
        },
    }
}

fn crop_overlap(first: Option<SourceCrop>, second: Option<SourceCrop>) -> f64 {
    let (Some(a), Some(b)) = (first, second) else {
        return 0.0;
    };
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = a.x.saturating_add(a.width).min(b.x.saturating_add(b.width));
    let y1 =
        a.y.saturating_add(a.height)
            .min(b.y.saturating_add(b.height));
    if x1 <= x0 || y1 <= y0 {
        return 0.0;
    }
    let intersection = u64::from(x1 - x0) * u64::from(y1 - y0);
    let smaller =
        (u64::from(a.width) * u64::from(a.height)).min(u64::from(b.width) * u64::from(b.height));
    if smaller == 0 {
        0.0
    } else {
        intersection as f64 / smaller as f64
    }
}

fn descriptor_similarity(first: &CropCandidate, second: &CropCandidate) -> f64 {
    let a = &first.descriptors;
    let b = &second.descriptors;
    let distance = u32::from(a.saliency_milli.abs_diff(b.saliency_milli))
        + u32::from(a.stationarity_milli.abs_diff(b.stationarity_milli))
        + u32::from(a.feature_strength_milli.abs_diff(b.feature_strength_milli))
        + u32::from(a.usability_milli.abs_diff(b.usability_milli));
    (1.0 - f64::from(distance.min(4000)) / 4000.0).clamp(0.0, 1.0)
}

fn intentional_periodic(
    first_slot: &PlacementSlotInput,
    first: &CropCandidate,
    second_slot: &PlacementSlotInput,
    second: &CropCandidate,
) -> bool {
    let periodic_behavior = |behavior| {
        matches!(
            behavior,
            MaterialBehaviorClass::AlreadyTileable
                | MaterialBehaviorClass::PeriodicLatticeStructured
                | MaterialBehaviorClass::ManufacturedPattern
        )
    };
    periodic_behavior(first_slot.material_behavior)
        && periodic_behavior(second_slot.material_behavior)
        && first.source_id == second.source_id
        && first.domain_id == second.domain_id
        && first.correspondence_reference == second.correspondence_reference
        && first.period_pixels.is_some()
        && first.period_pixels == second.period_pixels
}

fn stochastic_overlap(first: &PlacementSlotInput, second: &PlacementSlotInput) -> bool {
    let stochastic = |behavior| {
        matches!(
            behavior,
            MaterialBehaviorClass::StochasticIsotropic
                | MaterialBehaviorClass::StochasticDirectional
        )
    };
    stochastic(first.material_behavior) && stochastic(second.material_behavior)
}

fn local_improvement(
    context: &mut SolverContext<'_>,
    selected: &mut [Option<usize>],
    seed: u64,
) -> Result<(), PlacementError> {
    let mut local_work = 0_u64;
    let stable_slots = slot_order(context.slots);
    for _ in 0..context.settings.local_passes {
        let baseline = assignment_cost(context, selected)?;
        let mut best: Option<(f64, Vec<Option<usize>>)> = None;
        for &slot in &stable_slots {
            for candidate in candidate_order(&context.slots[slot].candidates.top_candidates, seed) {
                if selected[slot] == Some(candidate)
                    || local_work >= context.settings.max_local_evaluations
                {
                    continue;
                }
                check_cancel(context.cancellation)?;
                local_work += 1;
                let mut proposal = selected.to_vec();
                proposal[slot] = Some(candidate);
                if let Some(cost) = legal_assignment_cost(context, &proposal)? {
                    consider_improvement(&mut best, cost, proposal, baseline, context.slots, seed);
                }
            }
        }
        // Slot-specific candidate sets cannot literally exchange records. This is the equivalent
        // pairwise swap: choose two replacements atomically, allowing an improvement that neither
        // single replacement can make on its own.
        'pairs: for first_position in 0..stable_slots.len() {
            for second_position in first_position + 1..stable_slots.len() {
                let first = stable_slots[first_position];
                let second = stable_slots[second_position];
                for first_candidate in
                    candidate_order(&context.slots[first].candidates.top_candidates, seed)
                {
                    for second_candidate in
                        candidate_order(&context.slots[second].candidates.top_candidates, seed)
                    {
                        if local_work >= context.settings.max_local_evaluations {
                            break 'pairs;
                        }
                        if selected[first] == Some(first_candidate)
                            && selected[second] == Some(second_candidate)
                        {
                            continue;
                        }
                        check_cancel(context.cancellation)?;
                        local_work += 1;
                        let mut proposal = selected.to_vec();
                        proposal[first] = Some(first_candidate);
                        proposal[second] = Some(second_candidate);
                        if let Some(cost) = legal_assignment_cost(context, &proposal)? {
                            consider_improvement(
                                &mut best,
                                cost,
                                proposal,
                                baseline,
                                context.slots,
                                seed,
                            );
                        }
                    }
                }
            }
        }
        let Some((_, improved)) = best else { break };
        selected.clone_from_slice(&improved);
        if local_work >= context.settings.max_local_evaluations {
            break;
        }
    }
    Ok(())
}

fn consider_improvement(
    best: &mut Option<(f64, Vec<Option<usize>>)>,
    cost: f64,
    proposal: Vec<Option<usize>>,
    baseline: f64,
    slots: &[PlacementSlotInput],
    seed: u64,
) {
    if cost >= baseline - 1.0e-12 {
        return;
    }
    let replace = best.as_ref().is_none_or(|(old_cost, old)| {
        cost < *old_cost - 1.0e-12
            || ((cost - *old_cost).abs() <= 1.0e-12
                && assignment_tie_key(&proposal, slots, seed)
                    < assignment_tie_key(old, slots, seed))
    });
    if replace {
        *best = Some((cost, proposal));
    }
}

fn assignment_cost(
    context: &mut SolverContext<'_>,
    selected: &[Option<usize>],
) -> Result<f64, PlacementError> {
    legal_assignment_cost(context, selected)?.ok_or_else(|| {
        PlacementError::MalformedInput("internal assignment violated pairwise policy".into())
    })
}

fn legal_assignment_cost(
    context: &mut SolverContext<'_>,
    selected: &[Option<usize>],
) -> Result<Option<f64>, PlacementError> {
    let mut total = 0.0;
    let stable_slots = slot_order(context.slots);
    for &slot in &stable_slots {
        let Some(candidate) = selected[slot] else {
            return Ok(None);
        };
        total += context.slots[slot].candidates.top_candidates[candidate]
            .breakdown
            .total_cost;
    }
    for first_position in 0..stable_slots.len() {
        for second_position in first_position + 1..stable_slots.len() {
            let first = stable_slots[first_position];
            let second = stable_slots[second_position];
            let pair = context.pair(
                first,
                selected[first].unwrap(),
                second,
                selected[second].unwrap(),
            )?;
            if !pair.accepted {
                return Ok(None);
            }
            total += context.settings.pairwise_lambda * pair.breakdown.total_cost;
        }
    }
    Ok(Some(total))
}

fn build_plan(
    mut context: SolverContext<'_>,
    selected: Vec<Option<usize>>,
    seed: u64,
) -> Result<PlacementPlan, PlacementError> {
    let total_cost = assignment_cost(&mut context, &selected)?;
    let mut unary_cost = 0.0;
    let mut pairwise_cost = 0.0;
    let stable_slots = slot_order(context.slots);
    for &slot in &stable_slots {
        let candidate = selected[slot];
        unary_cost += context.slots[slot].candidates.top_candidates[candidate.unwrap()]
            .breakdown
            .total_cost;
    }
    for first_position in 0..stable_slots.len() {
        for second_position in first_position + 1..stable_slots.len() {
            let first = stable_slots[first_position];
            let second = stable_slots[second_position];
            pairwise_cost += context
                .pair(
                    first,
                    selected[first].unwrap(),
                    second,
                    selected[second].unwrap(),
                )?
                .breakdown
                .total_cost;
        }
    }
    let selected_ids = selected
        .iter()
        .enumerate()
        .map(|(slot, candidate)| {
            (
                context.slots[slot].slot_id,
                context.slots[slot].candidates.top_candidates[candidate.unwrap()]
                    .candidate
                    .candidate_id
                    .0
                    .clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut pairwise_decisions = context
        .pair_cache
        .iter()
        .map(|(key, evaluation)| {
            let chosen = selected_ids.get(&key.first_slot_id) == Some(&key.first_candidate)
                && selected_ids.get(&key.second_slot_id) == Some(&key.second_candidate);
            PairwiseDecision {
                first_slot_id: key.first_slot_id,
                first_candidate_id: ContentDigest(key.first_candidate.clone()),
                second_slot_id: key.second_slot_id,
                second_candidate_id: ContentDigest(key.second_candidate.clone()),
                outcome: if !evaluation.accepted {
                    PairwiseDecisionOutcome::RejectedByPolicy
                } else if chosen {
                    PairwiseDecisionOutcome::Accepted
                } else {
                    PairwiseDecisionOutcome::RejectedByObjective
                },
                reason: if evaluation.accepted && !chosen {
                    "rejected by the bounded global objective in favor of a lower-cost assignment"
                        .into()
                } else {
                    evaluation.reason.clone()
                },
                breakdown: evaluation.breakdown.clone(),
            }
        })
        .collect::<Vec<_>>();
    pairwise_decisions.sort_by(|a, b| {
        a.first_slot_id
            .cmp(&b.first_slot_id)
            .then(a.first_candidate_id.cmp(&b.first_candidate_id))
            .then(a.second_slot_id.cmp(&b.second_slot_id))
            .then(a.second_candidate_id.cmp(&b.second_candidate_id))
    });
    let mut placements = selected
        .iter()
        .enumerate()
        .map(|(slot, candidate)| {
            let scored = &context.slots[slot].candidates.top_candidates[candidate.unwrap()];
            SamplingPlan {
                slot_id: context.slots[slot].slot_id,
                role: context.slots[slot].role,
                variation_group: context.slots[slot].variation_group.clone(),
                prepared_domain_dimensions: context.slots[slot].prepared_domain_dimensions,
                candidate: scored.candidate.clone(),
                sampling_basis: if scored.candidate.mapping_mode
                    == hot_trimmer_domain::SamplingMode::TextureSynthesis
                {
                    SamplingBasis::PreparedDomain {
                        window: context.slots[slot].prepared_domain_sampling_window,
                    }
                } else {
                    SamplingBasis::SelectedCrop
                },
                slot_physical_size: context.slots[slot].slot_physical_size,
                source_pixels_per_physical_unit: context.slots[slot]
                    .base_source_pixels_per_physical_unit
                    * scored.candidate.isotropic_scale,
                sampling_policy: context.slots[slot].sampling_policy,
                radial_mapping: context.slots[slot].radial_mapping,
                stretch_override: context.slots[slot].stretch_override,
                slice_geometry: match scored.candidate.mapping_mode {
                    hot_trimmer_domain::SamplingMode::ThreeSliceCap
                    | hot_trimmer_domain::SamplingMode::NineSlicePanel => {
                        context.slots[slot].slice_geometry
                    }
                    _ => SliceGeometry::None,
                },
                maximum_seam_cost_milli: context.slots[slot].maximum_seam_cost_milli,
                unary_cost: scored.breakdown.total_cost,
            }
        })
        .collect::<Vec<_>>();
    placements.sort_by_key(|placement| placement.slot_id);
    let validation = validate_complete_plan(context.slots, &placements)?;
    let crop_reuse_heatmap = heatmap(&placements, context.settings.heatmap_grid_edge);
    let settings_hash = ContentDigest::sha256(format!("{:?}|{seed}", context.settings).as_bytes());
    let solver = AlgorithmProvenance {
        algorithm_id: STAGE_13_ALGORITHM_ID.into(),
        version: STAGE_13_ALGORITHM_VERSION.into(),
    };
    Ok(PlacementPlan {
        stage_result: StageResult::Executed {
            algorithm: solver.clone(),
            settings_hash,
            diagnostics: Vec::new(),
        },
        solver,
        seed,
        placements,
        objective: PlacementObjectiveBreakdown {
            unary_cost,
            pairwise_cost,
            pairwise_lambda: context.settings.pairwise_lambda,
            weighted_pairwise_cost: context.settings.pairwise_lambda * pairwise_cost,
            total_cost,
        },
        pairwise_decisions,
        crop_reuse_heatmap,
        validation,
        qa_views: vec![
            PlacementPlanQaView::SelectedPlacements,
            PlacementPlanQaView::ObjectiveBreakdown,
            PlacementPlanQaView::PairwiseDecisions,
            PlacementPlanQaView::SourceUsage,
            PlacementPlanQaView::CropReuseHeatmap,
            PlacementPlanQaView::RepetitionHeatmap,
            PlacementPlanQaView::Validation,
        ],
    })
}

fn validate_complete_plan(
    slots: &[PlacementSlotInput],
    placements: &[SamplingPlan],
) -> Result<PlacementValidationSummary, PlacementError> {
    let complete = placements.len() == slots.len();
    let required = slots.iter().filter(|slot| slot.required).all(|slot| {
        placements
            .iter()
            .any(|placement| placement.slot_id == slot.slot_id)
    });
    let isotropic = placements.iter().all(|placement| {
        placement.candidate.eligibility.isotropic_scale
            && placement.candidate.isotropic_scale.is_finite()
            && placement.candidate.isotropic_scale > 0.0
    });
    let registered = placements.iter().all(|placement| {
        slots
            .iter()
            .find(|slot| slot.slot_id == placement.slot_id)
            .is_some_and(|slot| {
                placement.candidate.correspondence_reference
                    == slot.registered_correspondence_reference
            })
    });
    if !complete || !required || !isotropic || !registered {
        return Err(PlacementError::MalformedInput(
            "completed placement failed authoritative validation".into(),
        ));
    }
    Ok(PlacementValidationSummary {
        complete_assignment: complete,
        required_slots_present: required,
        isotropic_scale_only: isotropic,
        registered_mapping_only: registered,
        slot_count: u32::try_from(placements.len()).unwrap_or(u32::MAX),
    })
}

fn heatmap(placements: &[SamplingPlan], edge: u8) -> Vec<CropReuseHeatmapCell> {
    let mut counts = BTreeMap::<(ContentDigest, ContentDigest, u8, u8), u16>::new();
    for placement in placements {
        let Some(crop) = placement.candidate.crop else {
            continue;
        };
        let [width, height] = placement.prepared_domain_dimensions;
        let grid = u32::from(edge);
        let x0 = (u64::from(crop.x) * u64::from(grid) / u64::from(width)).min(u64::from(edge - 1));
        let y0 = (u64::from(crop.y) * u64::from(grid) / u64::from(height)).min(u64::from(edge - 1));
        let x1 = ((u64::from(crop.x.saturating_add(crop.width)) * u64::from(grid)
            + u64::from(width)
            - 1)
            / u64::from(width))
        .min(u64::from(edge));
        let y1 = ((u64::from(crop.y.saturating_add(crop.height)) * u64::from(grid)
            + u64::from(height)
            - 1)
            / u64::from(height))
        .min(u64::from(edge));
        for y in y0..y1 {
            for x in x0..x1 {
                let key = (
                    placement.candidate.source_id.clone(),
                    placement.candidate.domain_id.clone(),
                    x as u8,
                    y as u8,
                );
                let count = counts.entry(key).or_default();
                *count = count.saturating_add(1);
            }
        }
    }
    counts
        .into_iter()
        .map(
            |((source_id, domain_id, x, y), reuse_count)| CropReuseHeatmapCell {
                source_id,
                domain_id,
                x,
                y,
                reuse_count,
                repetition_milli: if reuse_count <= 1 {
                    0
                } else {
                    u16::try_from(
                        (u32::from(reuse_count - 1) * 1000 / u32::from(reuse_count)).min(1000),
                    )
                    .unwrap_or(1000)
                },
            },
        )
        .collect()
}

fn assignment_tie_key(
    selected: &[Option<usize>],
    slots: &[PlacementSlotInput],
    seed: u64,
) -> Vec<(RegionId, u64, String)> {
    let mut key = selected
        .iter()
        .enumerate()
        .filter_map(|(slot, candidate)| {
            candidate.map(|candidate| {
                let id = &slots[slot].candidates.top_candidates[candidate]
                    .candidate
                    .candidate_id;
                (slots[slot].slot_id, seed_tie(seed, id), id.0.clone())
            })
        })
        .collect::<Vec<_>>();
    key.sort();
    key
}

fn seed_tie(seed: u64, id: &ContentDigest) -> u64 {
    id.0.as_bytes()
        .iter()
        .fold(seed ^ 0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

fn check_cancel(cancellation: &CancellationToken) -> Result<(), PlacementError> {
    if cancellation.is_cancelled() {
        Err(PlacementError::Cancelled)
    } else {
        Ok(())
    }
}

fn insufficient(slot_id: RegionId, message: &str) -> PlacementError {
    PlacementError::InsufficientAssignment {
        diagnostic: CompilationDiagnostic {
            code: DiagnosticCode::InsufficientInput,
            stage: Some(13),
            message: message.into(),
            context: BTreeMap::from([("slot_id".into(), slot_id.to_string())]),
        },
        recovery_choices: vec![
            RecoveryChoice::ChooseAnotherSource,
            RecoveryChoice::UseSynthesis,
            RecoveryChoice::LowerTexelDensity,
            RecoveryChoice::AdjustSettings,
        ],
    }
}

fn unit(value: u16) -> f64 {
    f64::from(value.min(1000)) / 1000.0
}

#[cfg(test)]
mod tests {
    use hot_trimmer_domain::{QuarterTurn, SamplingMode, SourceSamplingMode};

    use super::*;
    use crate::{
        CandidateCostBreakdown, CandidateDescriptors, CandidateFamily, CandidateRoute,
        CandidateTransform, EligibilityEvidence, MirrorTransform, PositionStrategy, ScoringQaView,
        SourceCrop, UnaryWeights,
    };

    fn candidate(
        slot_id: RegionId,
        name: &[u8],
        x: u32,
        saliency: u16,
        cost: f64,
    ) -> ScoredCandidate {
        let reference = ContentDigest::sha256(b"registered-domain");
        ScoredCandidate {
            rank: 1,
            candidate: CropCandidate {
                candidate_id: ContentDigest::sha256(name),
                source_id: ContentDigest::sha256(b"shared-source"),
                domain_id: reference.clone(),
                slot_id,
                crop: Some(SourceCrop {
                    x,
                    y: 0,
                    width: 100,
                    height: 100,
                }),
                transform: CandidateTransform {
                    rotation: QuarterTurn::Zero,
                    mirror: MirrorTransform::None,
                },
                isotropic_scale: 1.0,
                mapping_mode: SamplingMode::DirectCrop,
                family: CandidateFamily::PanelDirect,
                route: CandidateRoute::Direct,
                position_strategy: PositionStrategy::FeatureAware,
                period_pixels: None,
                seam_indices: Vec::new(),
                correspondence_reference: reference,
                descriptors: CandidateDescriptors {
                    saliency_milli: saliency,
                    stationarity_milli: 700,
                    feature_strength_milli: saliency,
                    usability_milli: 1000,
                },
                seed: 13,
                eligibility: EligibilityEvidence {
                    mapping_permitted: true,
                    transform_permitted: true,
                    isotropic_scale: true,
                    exact_aspect: true,
                    entire_crop_usable: Some(true),
                    cross_axis_preserved: None,
                    lattice_aligned: None,
                    direct_crop_applicable: true,
                    direct_crop_rejection: None,
                    reasons: Vec::new(),
                },
            },
            breakdown: CandidateCostBreakdown {
                terms: Vec::new(),
                total_cost: cost,
            },
        }
    }

    fn slot(
        id: u8,
        behavior: MaterialBehaviorClass,
        candidates: Vec<ScoredCandidate>,
    ) -> PlacementSlotInput {
        PlacementSlotInput {
            slot_id: RegionId::from_bytes([id; 16]),
            role: TemplateSlotRole::Planar,
            material_behavior: behavior,
            variation_group: "large-panels".into(),
            visual_importance_milli: 900,
            constraint_tightness_milli: 700,
            required: true,
            prepared_domain_id: ContentDigest::sha256(b"registered-domain"),
            prepared_domain_dimensions: [500, 500],
            prepared_domain_sampling_window: SourceCrop {
                x: 0,
                y: 0,
                width: 500,
                height: 500,
            },
            registered_correspondence_reference: ContentDigest::sha256(b"registered-domain"),
            slot_physical_size: [2.0, 2.0],
            base_source_pixels_per_physical_unit: 50.0,
            sampling_policy: SamplingPolicy {
                filter: SourceSamplingMode::Nearest,
                scale: 1.25,
                correct_tangent_normals: false,
            },
            radial_mapping: None,
            stretch_override: StretchOverrideProvenance::NotAuthorized,
            slice_geometry: SliceGeometry::None,
            maximum_seam_cost_milli: 450,
            reuse_permissions: ReusePermissions::default(),
            candidates: ScoredCandidateSet {
                stage_result: StageResult::Executed {
                    algorithm: AlgorithmProvenance {
                        algorithm_id: "stage-12".into(),
                        version: "1".into(),
                    },
                    settings_hash: ContentDigest::sha256(b"stage-12-settings"),
                    diagnostics: Vec::new(),
                },
                top_candidates: candidates,
                applicability_rejections: Vec::new(),
                legal_candidate_count: 2,
                truncated_candidates: 0,
                weights: UnaryWeights::for_behavior_and_role(behavior, TemplateSlotRole::Planar),
                qa_views: vec![ScoringQaView::RankedCandidates],
            },
        }
    }

    #[test]
    fn algorithm_stage_13_global_placement() {
        let first_id = RegionId::from_bytes([1; 16]);
        let second_id = RegionId::from_bytes([2; 16]);
        let slots = vec![
            slot(
                1,
                MaterialBehaviorClass::UniqueDetail,
                vec![
                    candidate(first_id, b"first-cheapest-mark", 0, 900, 0.0),
                    candidate(first_id, b"first-diverse", 220, 100, 0.4),
                ],
            ),
            slot(
                2,
                MaterialBehaviorClass::UniqueDetail,
                vec![
                    candidate(second_id, b"second-cheapest-mark", 0, 900, 0.0),
                    candidate(second_id, b"second-diverse", 220, 100, 0.4),
                ],
            ),
        ];
        let settings = PlacementOptimizerSettings::default();
        let first = optimize_placements(&slots, &settings, 99, &CancellationToken::new()).unwrap();
        let second = optimize_placements(&slots, &settings, 99, &CancellationToken::new()).unwrap();
        let mut permuted_slots = slots.clone();
        permuted_slots.reverse();
        let permuted =
            optimize_placements(&permuted_slots, &settings, 99, &CancellationToken::new()).unwrap();
        assert_eq!(
            first.deterministic_bytes().unwrap(),
            second.deterministic_bytes().unwrap()
        );
        assert_eq!(
            first.deterministic_bytes().unwrap(),
            permuted.deterministic_bytes().unwrap()
        );
        assert_eq!(
            first.objective_report_bytes().unwrap(),
            second.objective_report_bytes().unwrap()
        );
        assert!(first.validation.complete_assignment);
        assert!(first.validation.isotropic_scale_only);
        assert!(first.validation.registered_mapping_only);
        assert_ne!(
            first.placements[0].candidate.crop,
            first.placements[1].candidate.crop
        );
        assert_eq!(
            first.placements[0].sampling_policy,
            slots[0].sampling_policy
        );
        assert_eq!(first.placements[0].source_pixels_per_physical_unit, 50.0);
        assert_eq!(
            first.placements[0].stretch_override,
            StretchOverrideProvenance::NotAuthorized
        );
        assert!(first.pairwise_decisions.iter().any(|decision| {
            decision.outcome == PairwiseDecisionOutcome::RejectedByPolicy
                && decision.reason.contains("unique salient")
        }));
        assert!(
            first
                .qa_views
                .contains(&PlacementPlanQaView::CropReuseHeatmap)
        );
        assert!(
            first
                .pairwise_decisions
                .iter()
                .all(|decision| { decision.first_slot_id < decision.second_slot_id })
        );

        let same_transform = optimize_placements(
            &[
                slot(
                    7,
                    MaterialBehaviorClass::StochasticIsotropic,
                    vec![candidate(
                        RegionId::from_bytes([7; 16]),
                        b"same-transform-a",
                        0,
                        300,
                        0.0,
                    )],
                ),
                slot(
                    8,
                    MaterialBehaviorClass::StochasticIsotropic,
                    vec![candidate(
                        RegionId::from_bytes([8; 16]),
                        b"same-transform-b",
                        20,
                        300,
                        0.0,
                    )],
                ),
            ],
            &settings,
            99,
            &CancellationToken::new(),
        )
        .unwrap();
        let same_transform_pair = same_transform
            .pairwise_decisions
            .iter()
            .find(|decision| decision.outcome == PairwiseDecisionOutcome::Accepted)
            .unwrap();
        assert!(!same_transform_pair.breakdown.stochastic_overlap_permitted);
        assert!(
            same_transform_pair
                .breakdown
                .terms
                .iter()
                .find(|term| term.term == PairwiseCostTerm::IdenticalTransform)
                .unwrap()
                .weighted_cost
                > 0.0
        );

        let stochastic_a = slot(
            3,
            MaterialBehaviorClass::StochasticIsotropic,
            vec![candidate(
                RegionId::from_bytes([3; 16]),
                b"stochastic-a",
                0,
                300,
                0.0,
            )],
        );
        let mut stochastic_b = slot(
            4,
            MaterialBehaviorClass::StochasticIsotropic,
            vec![candidate(
                RegionId::from_bytes([4; 16]),
                b"stochastic-b",
                20,
                300,
                0.0,
            )],
        );
        stochastic_b.candidates.top_candidates[0]
            .candidate
            .transform
            .rotation = QuarterTurn::OneEighty;
        let stochastic = optimize_placements(
            &[stochastic_a, stochastic_b],
            &settings,
            99,
            &CancellationToken::new(),
        )
        .unwrap();
        let stochastic_pair = stochastic
            .pairwise_decisions
            .iter()
            .find(|decision| decision.outcome == PairwiseDecisionOutcome::Accepted)
            .unwrap();
        assert!(stochastic_pair.breakdown.stochastic_overlap_permitted);
        assert!(!stochastic_pair.breakdown.intentional_periodic_reuse);

        let cross_domain_a = slot(
            9,
            MaterialBehaviorClass::UniqueDetail,
            vec![candidate(
                RegionId::from_bytes([9; 16]),
                b"cross-domain-a",
                0,
                900,
                0.0,
            )],
        );
        let mut cross_domain_b = slot(
            10,
            MaterialBehaviorClass::UniqueDetail,
            vec![candidate(
                RegionId::from_bytes([10; 16]),
                b"cross-domain-b",
                0,
                900,
                0.0,
            )],
        );
        let other_domain = ContentDigest::sha256(b"other-domain");
        cross_domain_b.prepared_domain_id = other_domain.clone();
        cross_domain_b.registered_correspondence_reference = other_domain.clone();
        cross_domain_b.candidates.top_candidates[0]
            .candidate
            .domain_id = other_domain.clone();
        cross_domain_b.candidates.top_candidates[0]
            .candidate
            .correspondence_reference = other_domain;
        let cross_domain = optimize_placements(
            &[cross_domain_a, cross_domain_b],
            &settings,
            99,
            &CancellationToken::new(),
        )
        .unwrap();
        let cross_domain_pair = cross_domain
            .pairwise_decisions
            .iter()
            .find(|decision| decision.outcome == PairwiseDecisionOutcome::Accepted)
            .unwrap();
        assert_eq!(
            cross_domain_pair
                .breakdown
                .terms
                .iter()
                .find(|term| term.term == PairwiseCostTerm::SourceOverlap)
                .unwrap()
                .normalized_cost,
            0.0
        );
        assert!(
            cross_domain
                .crop_reuse_heatmap
                .iter()
                .all(|cell| cell.reuse_count == 1)
        );

        let mut periodic_a = slot(
            5,
            MaterialBehaviorClass::ManufacturedPattern,
            vec![candidate(
                RegionId::from_bytes([5; 16]),
                b"periodic-a",
                0,
                900,
                0.0,
            )],
        );
        let mut periodic_b = slot(
            6,
            MaterialBehaviorClass::ManufacturedPattern,
            vec![candidate(
                RegionId::from_bytes([6; 16]),
                b"periodic-b",
                0,
                900,
                0.0,
            )],
        );
        periodic_a.candidates.top_candidates[0]
            .candidate
            .period_pixels = Some([16, 16]);
        periodic_b.candidates.top_candidates[0]
            .candidate
            .period_pixels = Some([16, 16]);
        let periodic = optimize_placements(
            &[periodic_a, periodic_b],
            &settings,
            99,
            &CancellationToken::new(),
        )
        .unwrap();
        let periodic_pair = periodic
            .pairwise_decisions
            .iter()
            .find(|decision| decision.outcome == PairwiseDecisionOutcome::Accepted)
            .unwrap();
        assert!(periodic_pair.breakdown.intentional_periodic_reuse);
        assert_eq!(
            periodic_pair
                .breakdown
                .terms
                .iter()
                .find(|term| term.term == PairwiseCostTerm::SourceOverlap)
                .unwrap()
                .weighted_cost,
            0.0
        );

        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert_eq!(
            optimize_placements(&slots, &settings, 99, &cancelled).unwrap_err(),
            PlacementError::Cancelled
        );
        let mut insufficient_slots = slots.clone();
        insufficient_slots[0].candidates.top_candidates.clear();
        assert!(matches!(
            optimize_placements(
                &insufficient_slots,
                &settings,
                99,
                &CancellationToken::new()
            ),
            Err(PlacementError::InsufficientAssignment { .. })
        ));
    }

    #[test]
    fn texture_synthesis_preserves_authored_prepared_domain_window() {
        let slot_id = RegionId::from_bytes([31; 16]);
        let mut synthesis = candidate(slot_id, b"windowed-synthesis", 0, 0, 0.0);
        synthesis.candidate.crop = None;
        synthesis.candidate.mapping_mode = SamplingMode::TextureSynthesis;
        synthesis.candidate.family = CandidateFamily::PanelQuiltedExpansion;
        synthesis.candidate.route = CandidateRoute::Synthesis;
        let mut input = slot(
            31,
            MaterialBehaviorClass::StochasticIsotropic,
            vec![synthesis],
        );
        input.prepared_domain_sampling_window = SourceCrop {
            x: 73,
            y: 41,
            width: 211,
            height: 173,
        };
        let plan = optimize_placements(
            &[input],
            &PlacementOptimizerSettings::default(),
            99,
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(
            plan.placements[0].sampling_basis,
            SamplingBasis::PreparedDomain {
                window: SourceCrop {
                    x: 73,
                    y: 41,
                    width: 211,
                    height: 173,
                },
            }
        );
    }
}
