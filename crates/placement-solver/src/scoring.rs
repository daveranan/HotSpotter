use std::collections::BTreeMap;

use hot_trimmer_domain::{
    AlgorithmProvenance, CompilationDiagnostic, ContentDigest, MaterialBehaviorClass, QuarterTurn,
    RegionOrientation, StageResult, TemplateSlotRole,
};
use serde::{Deserialize, Serialize};

use crate::{
    CandidateFamily, CandidateRoute, CandidateSet, CropCandidate, SlotDemandView,
};

pub const STAGE_12_ALGORITHM_ID: &str = "hot-trimmer.stage-12.candidate-scoring";
pub const STAGE_12_ALGORITHM_VERSION: &str = "1.0.0";
pub const DEFAULT_SCORING_TOP_K: usize = 64;

/// Terms are kept in equation order so serialized explanations and QA tables are stable.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaryCostTerm {
    Scale,
    Resolution,
    Stationarity,
    Saliency,
    Structure,
    Orientation,
    Seam,
    BoundaryCut,
    Quality,
    Role,
    SynthesisComplexity,
}

const ALL_TERMS: [UnaryCostTerm; 11] = [
    UnaryCostTerm::Scale,
    UnaryCostTerm::Resolution,
    UnaryCostTerm::Stationarity,
    UnaryCostTerm::Saliency,
    UnaryCostTerm::Structure,
    UnaryCostTerm::Orientation,
    UnaryCostTerm::Seam,
    UnaryCostTerm::BoundaryCut,
    UnaryCostTerm::Quality,
    UnaryCostTerm::Role,
    UnaryCostTerm::SynthesisComplexity,
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnaryWeights {
    pub scale: f64,
    pub resolution: f64,
    pub stationarity: f64,
    pub saliency: f64,
    pub structure: f64,
    pub orientation: f64,
    pub seam: f64,
    pub boundary_cut: f64,
    pub quality: f64,
    pub role: f64,
    pub synthesis_complexity: f64,
}

impl UnaryWeights {
    /// Policy is selected only from measured behavior and authored slot role. It never sees a
    /// filename, fixture identifier, source digest, or candidate index.
    #[must_use]
    pub fn for_behavior_and_role(behavior: MaterialBehaviorClass, role: TemplateSlotRole) -> Self {
        let mut weights = Self {
            scale: 1.0,
            resolution: 1.1,
            stationarity: 0.8,
            saliency: 0.8,
            structure: 0.7,
            orientation: 0.7,
            seam: 0.8,
            boundary_cut: 0.8,
            quality: 1.4,
            role: 1.2,
            // Deliberately smaller than quality and role. Complexity breaks comparable-quality
            // ties; it cannot legalize a candidate or make a poor direct route mandatory.
            synthesis_complexity: 0.18,
        };
        match role {
            TemplateSlotRole::Planar | TemplateSlotRole::RepeatingStrip => {
                weights.stationarity = 1.45;
                weights.saliency = 1.35;
            }
            TemplateSlotRole::UniqueDetail => {
                weights.stationarity = 0.45;
                weights.saliency = 1.55;
                weights.role = 1.7;
            }
            TemplateSlotRole::TrimCap => weights.role = 1.65,
            TemplateSlotRole::Radial => weights.role = 2.0,
        }
        if matches!(behavior, MaterialBehaviorClass::PeriodicLatticeStructured
            | MaterialBehaviorClass::ManufacturedPattern | MaterialBehaviorClass::LayeredBanded)
        {
            weights.structure = 2.0;
            weights.boundary_cut = 1.8;
            weights.seam = 1.25;
        }
        if is_directional(behavior) {
            weights.orientation = 2.0;
        }
        weights
    }

    fn get(&self, term: UnaryCostTerm) -> f64 {
        match term {
            UnaryCostTerm::Scale => self.scale,
            UnaryCostTerm::Resolution => self.resolution,
            UnaryCostTerm::Stationarity => self.stationarity,
            UnaryCostTerm::Saliency => self.saliency,
            UnaryCostTerm::Structure => self.structure,
            UnaryCostTerm::Orientation => self.orientation,
            UnaryCostTerm::Seam => self.seam,
            UnaryCostTerm::BoundaryCut => self.boundary_cut,
            UnaryCostTerm::Quality => self.quality,
            UnaryCostTerm::Role => self.role,
            UnaryCostTerm::SynthesisComplexity => self.synthesis_complexity,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScoringSettings {
    pub top_k: usize,
    /// Scale error reaches one at this symmetric ratio (4x or 1/4x by default).
    pub scale_normalization_ratio: f64,
    /// Explicit experiment/tuning overrides. Keys are equation terms, not fixture identities.
    pub weight_overrides: BTreeMap<UnaryCostTerm, f64>,
}

impl Default for ScoringSettings {
    fn default() -> Self {
        Self { top_k: DEFAULT_SCORING_TOP_K, scale_normalization_ratio: 4.0,
            weight_overrides: BTreeMap::new() }
    }
}

/// Bounded measurements use milli-units: 0 means none/worst and 1000 means full/best,
/// as indicated by each field name. Missing measurements have zero confidence and therefore
/// remain visible but do not silently invent evidence.
#[derive(Clone, Debug, PartialEq)]
pub struct CandidateScoringMeasurements {
    pub source_pixels_per_output_pixel_milli: u16,
    pub resolution_confidence_milli: u16,
    pub lattice_completion_milli: u16,
    pub structure_confidence_milli: u16,
    pub dominant_direction_degrees: Option<f64>,
    pub orientation_confidence_milli: u16,
    pub seam_quality_milli: u16,
    pub seam_confidence_milli: u16,
    pub boundary_cut_milli: u16,
    pub boundary_confidence_milli: u16,
    pub visual_quality_milli: u16,
    pub quality_confidence_milli: u16,
    pub role_compatibility_milli: Option<u16>,
    pub role_confidence_milli: u16,
}

impl Default for CandidateScoringMeasurements {
    fn default() -> Self {
        Self { source_pixels_per_output_pixel_milli: 1000, resolution_confidence_milli: 0,
            lattice_completion_milli: 0, structure_confidence_milli: 0,
            dominant_direction_degrees: None, orientation_confidence_milli: 0,
            seam_quality_milli: 0, seam_confidence_milli: 0, boundary_cut_milli: 0,
            boundary_confidence_milli: 0, visual_quality_milli: 1000,
            quality_confidence_milli: 0, role_compatibility_milli: None,
            role_confidence_milli: 0 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScoringContext {
    pub material_behavior: MaterialBehaviorClass,
    pub material_confidence_milli: u16,
    pub requested_physical_scale: f64,
    pub measurements: BTreeMap<ContentDigest, CandidateScoringMeasurements>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostTermExplanation {
    pub term: UnaryCostTerm,
    /// Every term is clamped to [0, 1]. Lower is better.
    pub normalized_cost: f64,
    /// Evidence confidence is clamped to [0, 1] and multiplies the term.
    pub confidence: f64,
    pub weight: f64,
    pub weighted_cost: f64,
    pub applicable: bool,
    pub explanation: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateCostBreakdown {
    pub terms: Vec<CostTermExplanation>,
    pub total_cost: f64,
}

impl CandidateCostBreakdown {
    #[must_use]
    pub fn term(&self, term: UnaryCostTerm) -> Option<&CostTermExplanation> {
        self.terms.iter().find(|entry| entry.term == term)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoredCandidate {
    pub rank: u32,
    pub candidate: CropCandidate,
    pub breakdown: CandidateCostBreakdown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicabilityRejection {
    pub candidate_id: ContentDigest,
    pub reasons: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoringQaView { RankedCandidates, CostBreakdown, Applicability, TermContribution, RoleMaterialPolicy }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoredCandidateSet {
    pub stage_result: StageResult,
    pub top_candidates: Vec<ScoredCandidate>,
    pub applicability_rejections: Vec<ApplicabilityRejection>,
    pub legal_candidate_count: u32,
    pub truncated_candidates: u32,
    pub weights: UnaryWeights,
    pub qa_views: Vec<ScoringQaView>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScoringError { InvalidSettings, MalformedInput, Cancelled }

pub fn score_candidate_set<S: SlotDemandView>(
    slot: &S,
    candidates: &CandidateSet,
    context: &ScoringContext,
    settings: &ScoringSettings,
) -> Result<ScoredCandidateSet, ScoringError> {
    score_candidate_set_with_guard(slot, candidates, context, settings, &|| false)
}

pub fn score_candidate_set_with_guard<S: SlotDemandView>(
    slot: &S, candidates: &CandidateSet, context: &ScoringContext, settings: &ScoringSettings,
    cancelled: &dyn Fn() -> bool,
) -> Result<ScoredCandidateSet, ScoringError> {
    score_candidates_with_guard(slot, &candidates.candidates, context, settings, cancelled)
}

pub fn score_candidates<S: SlotDemandView>(
    slot: &S,
    candidates: &[CropCandidate],
    context: &ScoringContext,
    settings: &ScoringSettings,
) -> Result<ScoredCandidateSet, ScoringError> {
    score_candidates_with_guard(slot, candidates, context, settings, &|| false)
}

fn score_candidates_with_guard<S: SlotDemandView>(
    slot: &S, candidates: &[CropCandidate], context: &ScoringContext, settings: &ScoringSettings,
    cancelled: &dyn Fn() -> bool,
) -> Result<ScoredCandidateSet, ScoringError> {
    if cancelled() { return Err(ScoringError::Cancelled); }
    validate(slot, context, settings)?;
    let mut weights = UnaryWeights::for_behavior_and_role(context.material_behavior, slot.role());
    for (term, value) in &settings.weight_overrides { set_weight(&mut weights, *term, *value); }

    let mut scored = Vec::new();
    let mut rejections = Vec::new();
    for candidate in candidates {
        if cancelled() { return Err(ScoringError::Cancelled); }
        let reasons = legality_rejections(slot, candidate);
        if !reasons.is_empty() {
            rejections.push(ApplicabilityRejection { candidate_id: candidate.candidate_id.clone(), reasons });
            continue;
        }
        let measurements = context.measurements.get(&candidate.candidate_id).cloned().unwrap_or_default();
        let breakdown = breakdown(slot, candidate, context, &measurements, &weights, settings);
        scored.push(ScoredCandidate { rank: 0, candidate: candidate.clone(), breakdown });
    }
    scored.sort_by(|a, b| a.breakdown.total_cost.total_cmp(&b.breakdown.total_cost)
        .then_with(|| a.candidate.candidate_id.cmp(&b.candidate.candidate_id)));
    let legal_count = scored.len();
    scored.truncate(settings.top_k);
    for (index, candidate) in scored.iter_mut().enumerate() { candidate.rank = index as u32 + 1; }
    rejections.sort_by(|a, b| a.candidate_id.cmp(&b.candidate_id));
    let settings_hash = ContentDigest::sha256(format!("{context:?}|{settings:?}|{weights:?}").as_bytes());
    Ok(ScoredCandidateSet {
        stage_result: StageResult::Executed { algorithm: AlgorithmProvenance {
            algorithm_id: STAGE_12_ALGORITHM_ID.into(), version: STAGE_12_ALGORITHM_VERSION.into(),
        }, settings_hash, diagnostics: Vec::<CompilationDiagnostic>::new() },
        top_candidates: scored,
        applicability_rejections: rejections,
        legal_candidate_count: u32::try_from(legal_count).unwrap_or(u32::MAX),
        truncated_candidates: u32::try_from(legal_count.saturating_sub(settings.top_k)).unwrap_or(u32::MAX),
        weights,
        qa_views: vec![ScoringQaView::RankedCandidates, ScoringQaView::CostBreakdown,
            ScoringQaView::Applicability, ScoringQaView::TermContribution,
            ScoringQaView::RoleMaterialPolicy],
    })
}

fn validate<S: SlotDemandView>(slot: &S, context: &ScoringContext,
    settings: &ScoringSettings) -> Result<(), ScoringError>
{
    if settings.top_k == 0 || !settings.scale_normalization_ratio.is_finite()
        || settings.scale_normalization_ratio <= 1.0
        || !context.requested_physical_scale.is_finite() || context.requested_physical_scale <= 0.0
        || settings.weight_overrides.values().any(|v| !v.is_finite() || *v < 0.0)
        || slot.destination_pixels().0 == 0 || slot.destination_pixels().1 == 0
    { return Err(ScoringError::InvalidSettings); }
    if context.measurements.values().any(|m| m.dominant_direction_degrees.is_some_and(|v| !v.is_finite())) {
        return Err(ScoringError::MalformedInput);
    }
    Ok(())
}

fn legality_rejections<S: SlotDemandView>(slot: &S, c: &CropCandidate) -> Vec<String> {
    let mut reasons = Vec::new();
    let typed_synthesis = is_typed_synthesis(c);
    if c.slot_id != slot.slot_id() { reasons.push("candidate belongs to a different slot".into()); }
    if !c.eligibility.mapping_permitted { reasons.push("mapping mode was not permitted by Stage 11".into()); }
    if !c.eligibility.transform_permitted { reasons.push("transform was not permitted by Stage 11".into()); }
    if !c.eligibility.isotropic_scale || !c.isotropic_scale.is_finite() || c.isotropic_scale <= 0.0 {
        reasons.push("candidate does not preserve a finite positive isotropic scale".into());
    }
    if !c.eligibility.exact_aspect { reasons.push("candidate does not preserve the required aspect".into()); }
    if c.eligibility.entire_crop_usable == Some(false) {
        reasons.push("candidate has explicit unusable-source evidence".into());
    }
    if !typed_synthesis {
        if c.crop.is_none() { reasons.push("direct candidate has no source crop".into()); }
        if !c.eligibility.direct_crop_applicable {
            reasons.push("Stage 11 did not establish direct-crop applicability".into());
        }
        match c.eligibility.entire_crop_usable {
            Some(true) => {}
            Some(false) => {}
            None => reasons.push("direct candidate lacks positive whole-crop usability evidence".into()),
        }
    }
    if is_repeat_family(c.family) {
        match c.eligibility.cross_axis_preserved {
            Some(true) => {}
            Some(false) => reasons.push("strip cross-axis thickness is not preserved".into()),
            None => reasons.push("repeat candidate lacks positive cross-axis preservation evidence".into()),
        }
    } else if c.eligibility.cross_axis_preserved == Some(false) {
        reasons.push("strip cross-axis thickness is not preserved".into());
    }
    if c.eligibility.lattice_aligned == Some(false) { reasons.push("required lattice alignment failed".into()); }
    if matches!(c.mapping_mode, hot_trimmer_domain::SamplingMode::PeriodicTile
        | hot_trimmer_domain::SamplingMode::RepeatX | hot_trimmer_domain::SamplingMode::RepeatY)
        && c.period_pixels.is_none() {
        reasons.push("repeat mapping lacks the executable Stage 14 period".into());
    }
    reasons
}

fn breakdown<S: SlotDemandView>(slot: &S, c: &CropCandidate, context: &ScoringContext,
    m: &CandidateScoringMeasurements, weights: &UnaryWeights, settings: &ScoringSettings) -> CandidateCostBreakdown
{
    let behavior_confidence = unit(context.material_confidence_milli);
    let saliency = unit(c.descriptors.saliency_milli);
    let stationarity = unit(c.descriptors.stationarity_milli);
    let structured = matches!(context.material_behavior, MaterialBehaviorClass::PeriodicLatticeStructured
        | MaterialBehaviorClass::ManufacturedPattern | MaterialBehaviorClass::LayeredBanded);
    let repeatable = matches!(c.family, CandidateFamily::PanelSeamlessTile)
        || is_repeat_family(c.family);
    let unique_compatible = slot.role() == TemplateSlotRole::UniqueDetail
        && matches!(context.material_behavior, MaterialBehaviorClass::UniqueDetail
            | MaterialBehaviorClass::ManufacturedPattern | MaterialBehaviorClass::RadialDetail);
    let ratio = c.isotropic_scale / context.requested_physical_scale;
    let scale = (ratio.ln().abs() / settings.scale_normalization_ratio.ln()).clamp(0.0, 1.0);
    let resolution_ratio = f64::from(m.source_pixels_per_output_pixel_milli.min(1000)) / 1000.0;
    let resolution = (1.0 - resolution_ratio).clamp(0.0, 1.0);
    let stationarity_cost = if slot.role() == TemplateSlotRole::UniqueDetail {
        ((stationarity - 0.45).abs() / 0.55).clamp(0.0, 1.0)
    } else { 1.0 - stationarity };
    let saliency_cost = if unique_compatible {
        ((saliency - 0.70).abs() / 0.70).clamp(0.0, 1.0)
    } else { saliency };
    // Stage 11 alignment is an eligibility predicate; measured completion remains independent
    // evidence and must not be promoted to perfect merely because the crop origin is aligned.
    let lattice_completion = unit(m.lattice_completion_milli);
    let structure_cost = 1.0 - lattice_completion;
    let orientation = orientation_cost(slot.orientation(), c.transform.rotation,
        m.dominant_direction_degrees);
    let seam_cost = 1.0 - unit(m.seam_quality_milli);
    let boundary_cost = unit(m.boundary_cut_milli);
    let quality_cost = 1.0 - unit(m.visual_quality_milli);
    let derived_role = role_compatibility(slot.role(), c.family, context.material_behavior);
    let role_compatibility = m.role_compatibility_milli.map(unit).unwrap_or(derived_role);
    let synthesis_cost = synthesis_complexity(c.family);

    let values = [
        (UnaryCostTerm::Scale, scale, 1.0, true, "absolute log scale error; one at a 4x default ratio"),
        (UnaryCostTerm::Resolution, resolution, unit(m.resolution_confidence_milli), true,
            "upsampling deficit below one source pixel per output pixel"),
        (UnaryCostTerm::Stationarity, stationarity_cost, behavior_confidence, true,
            if slot.role() == TemplateSlotRole::UniqueDetail { "distance from controlled unique-detail stationarity" }
            else { "inverse measured stationarity for a generic/repeating surface" }),
        (UnaryCostTerm::Saliency, saliency_cost, behavior_confidence, true,
            if unique_compatible { "distance from controlled saliency target 0.70 for a compatible unique role" }
            else { "salient uniqueness penalty for a generic or incompatible role" }),
        (UnaryCostTerm::Structure, structure_cost, unit(m.structure_confidence_milli) * behavior_confidence,
            structured, "inverse lattice/period completion"),
        (UnaryCostTerm::Orientation, orientation, unit(m.orientation_confidence_milli) * behavior_confidence,
            is_directional(context.material_behavior) && slot.orientation() != RegionOrientation::Unspecified,
            "1 - abs(cos(delta)); source direction includes candidate rotation"),
        (UnaryCostTerm::Seam, seam_cost, unit(m.seam_confidence_milli), repeatable,
            "inverse opposite-boundary or solved-seam quality"),
        (UnaryCostTerm::BoundaryCut, boundary_cost, unit(m.boundary_confidence_milli),
            structured || m.boundary_confidence_milli > 0,
            "fraction of strong unresolved structure cut at the crop boundary"),
        (UnaryCostTerm::Quality, quality_cost, unit(m.quality_confidence_milli),
            true, "inverse bounded visual quality; absent quality evidence has zero confidence"),
        (UnaryCostTerm::Role, 1.0 - role_compatibility, m.role_compatibility_milli
            .map(|_| unit(m.role_confidence_milli)).unwrap_or(1.0), true,
            "inverse compatibility of candidate family/material behavior with authored slot role"),
        (UnaryCostTerm::SynthesisComplexity, synthesis_cost, 1.0, true,
            "bounded route complexity used only as a small comparable-quality preference"),
    ];
    let terms = ALL_TERMS.into_iter().zip(values).map(|(expected, value)| {
        debug_assert_eq!(expected, value.0);
        let (term, normalized, confidence, applicable, explanation) = value;
        let normalized_cost = normalized.clamp(0.0, 1.0);
        let confidence = confidence.clamp(0.0, 1.0);
        let weight = weights.get(term);
        CostTermExplanation { term, normalized_cost, confidence, weight,
            weighted_cost: if applicable { normalized_cost * confidence * weight } else { 0.0 },
            applicable, explanation: explanation.into() }
    }).collect::<Vec<_>>();
    let total_cost = terms.iter().map(|term| term.weighted_cost).sum();
    CandidateCostBreakdown { terms, total_cost }
}

fn unit(value: u16) -> f64 { f64::from(value.min(1000)) / 1000.0 }

fn is_directional(behavior: MaterialBehaviorClass) -> bool {
    matches!(behavior, MaterialBehaviorClass::StochasticDirectional
        | MaterialBehaviorClass::PeriodicLatticeStructured | MaterialBehaviorClass::LayeredBanded
        | MaterialBehaviorClass::OrganicDirectional | MaterialBehaviorClass::ManufacturedPattern)
}

fn is_repeat_family(family: CandidateFamily) -> bool {
    matches!(family, CandidateFamily::RepeatXSegment | CandidateFamily::RepeatXContiguous
        | CandidateFamily::RepeatXGraphCut | CandidateFamily::RepeatXQuilted
        | CandidateFamily::RepeatYSegment | CandidateFamily::RepeatYContiguous
        | CandidateFamily::RepeatYGraphCut | CandidateFamily::RepeatYQuilted)
}

fn is_typed_synthesis(candidate: &CropCandidate) -> bool {
    matches!((candidate.route, candidate.family),
        (CandidateRoute::Synthesis, CandidateFamily::PanelQuiltedExpansion
            | CandidateFamily::PanelPatchMatchExpansion | CandidateFamily::PanelProceduralResynthesis
            | CandidateFamily::RepeatXQuilted | CandidateFamily::RepeatYQuilted
            | CandidateFamily::UniqueSynthesisExtension)
        | (CandidateRoute::PolarRadial, CandidateFamily::PolarRadialSynthesis))
}

fn orientation_cost(slot: RegionOrientation, rotation: QuarterTurn, source_degrees: Option<f64>) -> f64 {
    let Some(source) = source_degrees else { return 0.0; };
    let target = match slot { RegionOrientation::Horizontal => 0.0,
        RegionOrientation::Vertical => 90.0, RegionOrientation::Unspecified => return 0.0 };
    let rotation_degrees = match rotation { QuarterTurn::Zero => 0.0, QuarterTurn::Ninety => 90.0,
        QuarterTurn::OneEighty => 180.0, QuarterTurn::TwoSeventy => 270.0 };
    let delta = (source + rotation_degrees - target).to_radians();
    (1.0 - delta.cos().abs()).clamp(0.0, 1.0)
}

fn role_compatibility(role: TemplateSlotRole, family: CandidateFamily,
    behavior: MaterialBehaviorClass) -> f64
{
    let family_match = match role {
        TemplateSlotRole::Planar => matches!(family, CandidateFamily::PanelDirect
            | CandidateFamily::PanelSeamlessTile | CandidateFamily::PanelQuiltedExpansion
            | CandidateFamily::PanelPatchMatchExpansion | CandidateFamily::PanelProceduralResynthesis),
        TemplateSlotRole::RepeatingStrip => matches!(family, CandidateFamily::RepeatXSegment
            | CandidateFamily::RepeatXContiguous | CandidateFamily::RepeatXGraphCut
            | CandidateFamily::RepeatXQuilted | CandidateFamily::RepeatYSegment
            | CandidateFamily::RepeatYContiguous | CandidateFamily::RepeatYGraphCut
            | CandidateFamily::RepeatYQuilted),
        TemplateSlotRole::UniqueDetail => matches!(family, CandidateFamily::UniqueContain
            | CandidateFamily::UniqueCover | CandidateFamily::UniquePatchBase
            | CandidateFamily::UniqueSynthesisExtension),
        TemplateSlotRole::TrimCap => matches!(family, CandidateFamily::ThreeSliceCap
            | CandidateFamily::NineSlicePanel),
        TemplateSlotRole::Radial => matches!(family, CandidateFamily::PlanarRadialSquare
            | CandidateFamily::PlanarRadialDetail | CandidateFamily::PlanarRadialAnnularProfile
            | CandidateFamily::PolarRadialSynthesis),
    };
    if !family_match { return 0.0; }
    match role {
        TemplateSlotRole::UniqueDetail if matches!(behavior, MaterialBehaviorClass::UniqueDetail
            | MaterialBehaviorClass::ManufacturedPattern) => 1.0,
        TemplateSlotRole::Radial if behavior == MaterialBehaviorClass::RadialDetail => 1.0,
        TemplateSlotRole::UniqueDetail | TemplateSlotRole::Radial => 0.65,
        _ => 1.0,
    }
}

fn synthesis_complexity(family: CandidateFamily) -> f64 {
    match family {
        CandidateFamily::PanelDirect | CandidateFamily::RepeatXSegment
        | CandidateFamily::RepeatXContiguous | CandidateFamily::RepeatYSegment
        | CandidateFamily::RepeatYContiguous | CandidateFamily::UniqueContain
        | CandidateFamily::UniqueCover | CandidateFamily::ThreeSliceCap
        | CandidateFamily::NineSlicePanel | CandidateFamily::PlanarRadialSquare
        | CandidateFamily::PlanarRadialDetail | CandidateFamily::PlanarRadialAnnularProfile => 0.0,
        CandidateFamily::PanelSeamlessTile | CandidateFamily::RepeatXGraphCut
        | CandidateFamily::RepeatYGraphCut | CandidateFamily::UniquePatchBase => 0.2,
        CandidateFamily::PanelQuiltedExpansion | CandidateFamily::RepeatXQuilted
        | CandidateFamily::RepeatYQuilted | CandidateFamily::UniqueSynthesisExtension => 0.55,
        CandidateFamily::PanelPatchMatchExpansion => 0.8,
        CandidateFamily::PanelProceduralResynthesis | CandidateFamily::PolarRadialSynthesis => 1.0,
    }
}

fn set_weight(weights: &mut UnaryWeights, term: UnaryCostTerm, value: f64) {
    match term {
        UnaryCostTerm::Scale => weights.scale = value,
        UnaryCostTerm::Resolution => weights.resolution = value,
        UnaryCostTerm::Stationarity => weights.stationarity = value,
        UnaryCostTerm::Saliency => weights.saliency = value,
        UnaryCostTerm::Structure => weights.structure = value,
        UnaryCostTerm::Orientation => weights.orientation = value,
        UnaryCostTerm::Seam => weights.seam = value,
        UnaryCostTerm::BoundaryCut => weights.boundary_cut = value,
        UnaryCostTerm::Quality => weights.quality = value,
        UnaryCostTerm::Role => weights.role = value,
        UnaryCostTerm::SynthesisComplexity => weights.synthesis_complexity = value,
    }
}

#[cfg(test)]
mod tests {
    use hot_trimmer_domain::{RegionId, SamplingMode};

    use super::*;
    use crate::{CandidateDescriptors, CandidateTransform, EligibilityEvidence, MirrorTransform,
        PositionStrategy, SourceCrop, SourceFootprintKind};

    struct Slot { id: RegionId, role: TemplateSlotRole, orientation: RegionOrientation }
    impl SlotDemandView for Slot {
        fn slot_id(&self) -> RegionId { self.id }
        fn role(&self) -> TemplateSlotRole { self.role }
        fn orientation(&self) -> RegionOrientation { self.orientation }
        fn destination_pixels(&self) -> (u32, u32) { (128, 64) }
        fn required_source_footprint(&self) -> (f64, f64, SourceFootprintKind) {
            (128.0, 64.0, SourceFootprintKind::SourcePixels)
        }
        fn allowed_mapping_modes(&self) -> &[SamplingMode] { &[SamplingMode::DirectCrop] }
        fn allowed_rotations(&self) -> &[QuarterTurn] { &[QuarterTurn::Zero] }
        fn mirror_allowed(&self) -> bool { false }
    }

    fn candidate(slot: &Slot, name: &[u8], saliency: u16, stationarity: u16,
        family: CandidateFamily, rotation: QuarterTurn) -> CropCandidate
    {
        CropCandidate {
            candidate_id: ContentDigest::sha256(name), source_id: ContentDigest::sha256(b"source"),
            domain_id: ContentDigest::sha256(b"domain"), slot_id: slot.id,
            crop: Some(SourceCrop { x: 0, y: 0, width: 128, height: 64 }),
            transform: CandidateTransform { rotation, mirror: MirrorTransform::None },
            isotropic_scale: 1.0, mapping_mode: SamplingMode::DirectCrop, family,
            route: if matches!(family, CandidateFamily::PanelQuiltedExpansion
                | CandidateFamily::PanelPatchMatchExpansion | CandidateFamily::PanelProceduralResynthesis)
                { CandidateRoute::Synthesis } else { CandidateRoute::Direct },
            position_strategy: PositionStrategy::FeatureAware, period_pixels: None,
            seam_indices: Vec::new(), correspondence_reference: ContentDigest::sha256(b"domain"),
            descriptors: CandidateDescriptors { saliency_milli: saliency,
                stationarity_milli: stationarity, feature_strength_milli: saliency,
                usability_milli: 1000 }, seed: 9,
            eligibility: EligibilityEvidence { mapping_permitted: true, transform_permitted: true,
                isotropic_scale: true, exact_aspect: true, entire_crop_usable: Some(true),
                cross_axis_preserved: None, lattice_aligned: None, direct_crop_applicable: true,
                direct_crop_rejection: None, reasons: Vec::new() },
        }
    }

    fn measured(quality: u16) -> CandidateScoringMeasurements {
        CandidateScoringMeasurements { visual_quality_milli: quality,
            quality_confidence_milli: 1000, ..CandidateScoringMeasurements::default() }
    }

    fn context(behavior: MaterialBehaviorClass, candidates: &[(&CropCandidate, CandidateScoringMeasurements)])
        -> ScoringContext
    {
        ScoringContext { material_behavior: behavior, material_confidence_milli: 1000,
            requested_physical_scale: 1.0, measurements: candidates.iter()
                .map(|(c, m)| (c.candidate_id.clone(), m.clone())).collect() }
    }

    #[test]
    fn algorithm_stage_12_scoring_explains_role_material_rankings_and_legality() {
        let generic = Slot { id: RegionId::from_bytes([12; 16]), role: TemplateSlotRole::Planar,
            orientation: RegionOrientation::Horizontal };
        let calm = candidate(&generic, b"calm", 80, 920, CandidateFamily::PanelDirect, QuarterTurn::Zero);
        let loud = candidate(&generic, b"loud", 850, 250, CandidateFamily::PanelDirect, QuarterTurn::Zero);
        let generic_result = score_candidates(&generic, &[loud.clone(), calm.clone()],
            &context(MaterialBehaviorClass::StochasticIsotropic,
                &[(&calm, measured(900)), (&loud, measured(900))]), &ScoringSettings::default()).unwrap();
        assert_eq!(generic_result.top_candidates[0].candidate.candidate_id, calm.candidate_id);
        let calm_cost = &generic_result.top_candidates[0].breakdown;
        assert_eq!(calm_cost.terms.len(), 11);
        assert!(calm_cost.term(UnaryCostTerm::Stationarity).unwrap().normalized_cost < 0.1);
        assert!(calm_cost.term(UnaryCostTerm::Saliency).unwrap().normalized_cost < 0.1);

        let structured = Slot { id: RegionId::from_bytes([13; 16]), role: TemplateSlotRole::RepeatingStrip,
            orientation: RegionOrientation::Horizontal };
        let mut aligned = candidate(&structured, b"aligned", 200, 800,
            CandidateFamily::RepeatXContiguous, QuarterTurn::Zero);
        let mut crossed = candidate(&structured, b"crossed", 200, 800,
            CandidateFamily::RepeatXContiguous, QuarterTurn::Ninety);
        aligned.route = CandidateRoute::Repeat;
        crossed.route = CandidateRoute::Repeat;
        aligned.eligibility.cross_axis_preserved = Some(true);
        crossed.eligibility.cross_axis_preserved = Some(true);
        aligned.eligibility.lattice_aligned = Some(true);
        crossed.eligibility.lattice_aligned = Some(true);
        let aligned_m = CandidateScoringMeasurements { lattice_completion_milli: 1000,
            structure_confidence_milli: 1000, dominant_direction_degrees: Some(0.0),
            orientation_confidence_milli: 1000, boundary_cut_milli: 0,
            boundary_confidence_milli: 1000, visual_quality_milli: 900,
            quality_confidence_milli: 1000, ..CandidateScoringMeasurements::default() };
        let crossed_m = CandidateScoringMeasurements { lattice_completion_milli: 0,
            boundary_cut_milli: 800,
            ..aligned_m.clone() };
        let structured_result = score_candidates(&structured, &[crossed.clone(), aligned.clone()],
            &context(MaterialBehaviorClass::PeriodicLatticeStructured,
                &[(&aligned, aligned_m), (&crossed, crossed_m)]), &ScoringSettings::default()).unwrap();
        assert_eq!(structured_result.top_candidates[0].candidate.candidate_id, aligned.candidate_id);
        let crossed_cost = structured_result.top_candidates.iter().find(|s|
            s.candidate.candidate_id == crossed.candidate_id).unwrap();
        assert!(crossed_cost.breakdown.term(UnaryCostTerm::Orientation).unwrap().weighted_cost > 1.9);
        assert!(crossed_cost.breakdown.term(UnaryCostTerm::Structure).unwrap().weighted_cost > 1.9);
        assert!(crossed_cost.breakdown.term(UnaryCostTerm::BoundaryCut).unwrap().weighted_cost > 1.4);

        let unique = Slot { id: RegionId::from_bytes([14; 16]), role: TemplateSlotRole::UniqueDetail,
            orientation: RegionOrientation::Unspecified };
        let controlled = candidate(&unique, b"controlled", 700, 450,
            CandidateFamily::UniqueContain, QuarterTurn::Zero);
        let empty = candidate(&unique, b"empty", 50, 450,
            CandidateFamily::UniqueContain, QuarterTurn::Zero);
        let unique_result = score_candidates(&unique, &[empty.clone(), controlled.clone()],
            &context(MaterialBehaviorClass::UniqueDetail,
                &[(&controlled, measured(900)), (&empty, measured(900))]), &ScoringSettings::default()).unwrap();
        assert_eq!(unique_result.top_candidates[0].candidate.candidate_id, controlled.candidate_id);
        assert_eq!(unique_result.top_candidates[0].breakdown.term(UnaryCostTerm::Saliency)
            .unwrap().normalized_cost, 0.0);

        let mut illegal_mapping = calm.clone();
        illegal_mapping.candidate_id = ContentDigest::sha256(b"illegal-mapping");
        illegal_mapping.descriptors.saliency_milli = 0;
        illegal_mapping.descriptors.stationarity_milli = 1000;
        illegal_mapping.eligibility.mapping_permitted = false;
        let mut missing_route = calm.clone();
        missing_route.candidate_id = ContentDigest::sha256(b"missing-route");
        missing_route.eligibility.direct_crop_applicable = false;
        let mut missing_usability = calm.clone();
        missing_usability.candidate_id = ContentDigest::sha256(b"missing-usability");
        missing_usability.eligibility.entire_crop_usable = None;
        let mut legal_synthesis = candidate(&generic, b"legal-synthesis", 80, 920,
            CandidateFamily::PanelPatchMatchExpansion, QuarterTurn::Zero);
        legal_synthesis.crop = None;
        legal_synthesis.eligibility.direct_crop_applicable = false;
        legal_synthesis.eligibility.entire_crop_usable = None;
        let mut unusable_synthesis = legal_synthesis.clone();
        unusable_synthesis.candidate_id = ContentDigest::sha256(b"unusable-synthesis");
        unusable_synthesis.eligibility.entire_crop_usable = Some(false);
        let gated = score_candidates(&generic, &[illegal_mapping.clone(), missing_route.clone(),
                missing_usability.clone(), unusable_synthesis.clone(), legal_synthesis.clone(), calm.clone()],
            &context(MaterialBehaviorClass::StochasticIsotropic, &[(&calm, measured(900))]),
            &ScoringSettings::default()).unwrap();
        assert_eq!(gated.top_candidates.len(), 2);
        assert!(gated.top_candidates.iter().any(|s| s.candidate.candidate_id == legal_synthesis.candidate_id));
        assert_eq!(gated.applicability_rejections.len(), 4);
        let route_rejection = gated.applicability_rejections.iter().find(|r|
            r.candidate_id == missing_route.candidate_id).unwrap();
        assert!(route_rejection.reasons.iter().any(|reason| reason.contains("direct-crop applicability")));
        let usability_rejection = gated.applicability_rejections.iter().find(|r|
            r.candidate_id == missing_usability.candidate_id).unwrap();
        assert!(usability_rejection.reasons.iter().any(|reason| reason.contains("positive whole-crop")));
        let synthesis_rejection = gated.applicability_rejections.iter().find(|r|
            r.candidate_id == unusable_synthesis.candidate_id).unwrap();
        assert!(synthesis_rejection.reasons.iter().any(|reason| reason.contains("explicit unusable")));
    }

    #[test]
    fn algorithm_stage_12_scoring_weight_changes_and_ties_are_explainable() {
        let slot = Slot { id: RegionId::from_bytes([15; 16]), role: TemplateSlotRole::Planar,
            orientation: RegionOrientation::Unspecified };
        let low_saliency = candidate(&slot, b"low-saliency", 100, 700,
            CandidateFamily::PanelDirect, QuarterTurn::Zero);
        let high_quality = candidate(&slot, b"high-quality", 800, 700,
            CandidateFamily::PanelDirect, QuarterTurn::Zero);
        let ctx = context(MaterialBehaviorClass::StochasticIsotropic,
            &[(&low_saliency, measured(600)), (&high_quality, measured(1000))]);
        let base = score_candidates(&slot, &[high_quality.clone(), low_saliency.clone()],
            &ctx, &ScoringSettings::default()).unwrap();
        assert_eq!(base.top_candidates[0].candidate.candidate_id, low_saliency.candidate_id);
        let mut changed_settings = ScoringSettings::default();
        changed_settings.weight_overrides.insert(UnaryCostTerm::Saliency, 0.0);
        let changed = score_candidates(&slot, &[high_quality.clone(), low_saliency.clone()],
            &ctx, &changed_settings).unwrap();
        assert_eq!(changed.top_candidates[0].candidate.candidate_id, high_quality.candidate_id);
        let before_saliency = base.top_candidates.iter().find(|s|
            s.candidate.candidate_id == high_quality.candidate_id).unwrap()
            .breakdown.term(UnaryCostTerm::Saliency).unwrap();
        let after_saliency = changed.top_candidates[0].breakdown.term(UnaryCostTerm::Saliency).unwrap();
        assert!(before_saliency.weighted_cost > 1.0);
        assert_eq!(after_saliency.weight, 0.0);
        assert_eq!(after_saliency.weighted_cost, 0.0);

        let tie_a = candidate(&slot, b"tie-a", 300, 700, CandidateFamily::PanelDirect, QuarterTurn::Zero);
        let tie_b = candidate(&slot, b"tie-b", 300, 700, CandidateFamily::PanelDirect, QuarterTurn::Zero);
        let tie_context = context(MaterialBehaviorClass::StochasticIsotropic,
            &[(&tie_a, measured(900)), (&tie_b, measured(900))]);
        let forward = score_candidates(&slot, &[tie_a.clone(), tie_b.clone()],
            &tie_context, &ScoringSettings::default()).unwrap();
        let reverse = score_candidates(&slot, &[tie_b, tie_a], &tie_context,
            &ScoringSettings::default()).unwrap();
        assert_eq!(forward.top_candidates, reverse.top_candidates);
        assert_eq!(forward.top_candidates[0].breakdown.total_cost,
            forward.top_candidates[1].breakdown.total_cost);
    }

    #[test]
    fn algorithm_stage_12_scoring_complexity_is_only_a_comparable_quality_preference() {
        let slot = Slot { id: RegionId::from_bytes([16; 16]), role: TemplateSlotRole::Planar,
            orientation: RegionOrientation::Unspecified };
        let direct = candidate(&slot, b"direct", 300, 700,
            CandidateFamily::PanelDirect, QuarterTurn::Zero);
        let synthesis = candidate(&slot, b"synthesis", 300, 700,
            CandidateFamily::PanelPatchMatchExpansion, QuarterTurn::Zero);
        let equal = context(MaterialBehaviorClass::StochasticIsotropic,
            &[(&direct, measured(900)), (&synthesis, measured(900))]);
        let result = score_candidates(&slot, &[synthesis.clone(), direct.clone()],
            &equal, &ScoringSettings::default()).unwrap();
        assert_eq!(result.top_candidates[0].candidate.candidate_id, direct.candidate_id);
        assert_eq!(result.top_candidates[0].breakdown.term(UnaryCostTerm::SynthesisComplexity)
            .unwrap().normalized_cost, 0.0);
        let synthesis_complexity = result.top_candidates.iter().find(|s|
            s.candidate.candidate_id == synthesis.candidate_id).unwrap()
            .breakdown.term(UnaryCostTerm::SynthesisComplexity).unwrap();
        assert!(synthesis_complexity.weight < result.weights.quality);
        assert_eq!(synthesis_complexity.normalized_cost, 0.8);

        let missing_quality_context = context(MaterialBehaviorClass::StochasticIsotropic,
            &[(&direct, CandidateScoringMeasurements::default())]);
        let missing_quality = score_candidates(&slot, &[direct.clone()], &missing_quality_context,
            &ScoringSettings::default()).unwrap();
        let quality_term = missing_quality.top_candidates[0].breakdown
            .term(UnaryCostTerm::Quality).unwrap();
        assert_eq!(quality_term.confidence, 0.0);
        assert_eq!(quality_term.weighted_cost, 0.0);

        let repeat_slot = Slot { id: RegionId::from_bytes([17; 16]),
            role: TemplateSlotRole::RepeatingStrip, orientation: RegionOrientation::Horizontal };
        let mut quilted_repeat = candidate(&repeat_slot, b"quilted-repeat", 200, 800,
            CandidateFamily::RepeatXQuilted, QuarterTurn::Zero);
        quilted_repeat.route = CandidateRoute::Synthesis;
        quilted_repeat.crop = None;
        quilted_repeat.eligibility.direct_crop_applicable = false;
        quilted_repeat.eligibility.entire_crop_usable = None;
        quilted_repeat.eligibility.cross_axis_preserved = Some(true);
        let mut missing_cross_axis = quilted_repeat.clone();
        missing_cross_axis.candidate_id = ContentDigest::sha256(b"missing-cross-axis");
        missing_cross_axis.eligibility.cross_axis_preserved = None;
        let repeat_measurement = CandidateScoringMeasurements { seam_quality_milli: 0,
            seam_confidence_milli: 1000, ..measured(900) };
        let repeat_context = context(MaterialBehaviorClass::StochasticIsotropic,
            &[(&quilted_repeat, repeat_measurement)]);
        let repeat_result = score_candidates(&repeat_slot, &[missing_cross_axis.clone(), quilted_repeat], &repeat_context,
            &ScoringSettings::default()).unwrap();
        assert_eq!(repeat_result.applicability_rejections.len(), 1);
        assert_eq!(repeat_result.applicability_rejections[0].candidate_id, missing_cross_axis.candidate_id);
        assert!(repeat_result.applicability_rejections[0].reasons.iter()
            .any(|reason| reason.contains("positive cross-axis")));
        let seam_term = repeat_result.top_candidates[0].breakdown.term(UnaryCostTerm::Seam).unwrap();
        assert!(seam_term.applicable);
        assert!(seam_term.weighted_cost > 0.0);

        let wood_measurement = CandidateScoringMeasurements { boundary_cut_milli: 1000,
            boundary_confidence_milli: 1000, ..measured(900) };
        let wood_context = context(MaterialBehaviorClass::OrganicDirectional,
            &[(&direct, wood_measurement)]);
        let wood_result = score_candidates(&slot, &[direct], &wood_context,
            &ScoringSettings::default()).unwrap();
        let boundary_term = wood_result.top_candidates[0].breakdown
            .term(UnaryCostTerm::BoundaryCut).unwrap();
        assert!(boundary_term.applicable);
        assert!(boundary_term.weighted_cost > 0.0);
    }
}
