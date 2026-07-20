use std::collections::{BTreeMap, BTreeSet};

use hot_trimmer_domain::{
    AlgorithmProvenance, CompilationDiagnostic, ContentDigest, DiagnosticCode,
    MaterialBehaviorClass, QuarterTurn, RecoveryChoice, RegionId, RegionOrientation, SamplingMode,
    StageResult, TemplateSlotRole,
};
use hot_trimmer_material_synthesis::{DomainRoute, PreparedMaterialDomain, SeamAxis};
use serde::{Deserialize, Serialize};

pub const STAGE_11_ALGORITHM_ID: &str = "hot-trimmer.stage-11.crop-candidates";
pub const STAGE_11_ALGORITHM_VERSION: &str = "1.0.0";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceFootprintKind {
    SourcePixels,
    RelativeTexels,
}

/// The narrow dependency-safe view Stage 11 needs from the authoritative Stage 10 record.
pub trait SlotDemandView {
    fn slot_id(&self) -> RegionId;
    fn role(&self) -> TemplateSlotRole;
    fn orientation(&self) -> RegionOrientation;
    fn destination_pixels(&self) -> (u32, u32);
    fn required_source_footprint(&self) -> (f64, f64, SourceFootprintKind);
    fn allowed_mapping_modes(&self) -> &[SamplingMode];
    fn allowed_rotations(&self) -> &[QuarterTurn];
    fn mirror_allowed(&self) -> bool;
}

/// Domain access is abstracted so the bounded generator can be tested without manufacturing PBR planes.
pub trait MaterialDomainView {
    fn domain_id(&self) -> &ContentDigest;
    fn source_id(&self) -> &ContentDigest;
    fn dimensions(&self) -> (u32, u32);
    fn route(&self) -> DomainRoute;
    fn valid(&self, x: u32, y: u32) -> bool;
    fn seam_indices(&self, axis: SeamAxis) -> Vec<u32>;
}

impl MaterialDomainView for PreparedMaterialDomain {
    fn domain_id(&self) -> &ContentDigest {
        &self.cache_key
    }
    fn source_id(&self) -> &ContentDigest {
        &self.prepared_source_digest
    }
    fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    fn route(&self) -> DomainRoute {
        self.route
    }
    fn valid(&self, x: u32, y: u32) -> bool {
        self.validity.pixel(x, y).0 >= 0.5
    }
    fn seam_indices(&self, axis: SeamAxis) -> Vec<u32> {
        self.seams
            .iter()
            .enumerate()
            .filter_map(|(index, seam)| (seam.axis == axis).then_some(index as u32))
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MirrorTransform {
    None,
    X,
    Y,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateTransform {
    pub rotation: QuarterTurn,
    pub mirror: MirrorTransform,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PositionStrategy {
    DenseLowResolution,
    CoarseToFine,
    FeatureAware,
    Saliency,
    StationaryZone,
    PeriodAligned,
    FarthestPoint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceCrop {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateFamily {
    PanelDirect,
    PanelSeamlessTile,
    PanelQuiltedExpansion,
    PanelPatchMatchExpansion,
    PanelProceduralResynthesis,
    RepeatXSegment,
    RepeatXContiguous,
    RepeatXGraphCut,
    RepeatXQuilted,
    RepeatYSegment,
    RepeatYContiguous,
    RepeatYGraphCut,
    RepeatYQuilted,
    UniqueContain,
    UniqueCover,
    UniquePatchBase,
    UniqueSynthesisExtension,
    ThreeSliceCap,
    NineSlicePanel,
    PlanarRadialSquare,
    PlanarRadialDetail,
    PlanarRadialAnnularProfile,
    PolarRadialSynthesis,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateRoute {
    Direct,
    Repeat,
    Unique,
    Cap,
    PlanarRadial,
    PolarRadial,
    Synthesis,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateDescriptors {
    pub saliency_milli: u16,
    pub stationarity_milli: u16,
    pub feature_strength_milli: u16,
    pub usability_milli: u16,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EligibilityEvidence {
    pub mapping_permitted: bool,
    pub transform_permitted: bool,
    pub isotropic_scale: bool,
    pub exact_aspect: bool,
    pub entire_crop_usable: Option<bool>,
    pub cross_axis_preserved: Option<bool>,
    pub lattice_aligned: Option<bool>,
    pub direct_crop_applicable: bool,
    pub direct_crop_rejection: Option<String>,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CropCandidate {
    pub candidate_id: ContentDigest,
    pub source_id: ContentDigest,
    pub domain_id: ContentDigest,
    pub slot_id: RegionId,
    pub crop: Option<SourceCrop>,
    pub transform: CandidateTransform,
    /// One scalar drives both axes. There is deliberately no X/Y scale pair.
    pub isotropic_scale: f64,
    pub mapping_mode: SamplingMode,
    pub family: CandidateFamily,
    pub route: CandidateRoute,
    pub position_strategy: PositionStrategy,
    pub period_pixels: Option<[u32; 2]>,
    pub seam_indices: Vec<u32>,
    pub correspondence_reference: ContentDigest,
    pub descriptors: CandidateDescriptors,
    pub seed: u64,
    pub eligibility: EligibilityEvidence,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FeaturePosition {
    pub x: u32,
    pub y: u32,
    pub saliency_milli: u16,
    pub stationarity_milli: u16,
    pub feature_strength_milli: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CandidateEvidence {
    pub material_class: MaterialBehaviorClass,
    pub class_confidence_milli: u16,
    pub orientation_confidence_milli: u16,
    pub destructive_quarter_turn_override: bool,
    pub periods: Vec<[u32; 2]>,
    pub feature_positions: Vec<FeaturePosition>,
}

impl Default for CandidateEvidence {
    fn default() -> Self {
        Self {
            material_class: MaterialBehaviorClass::MixedUnknown,
            class_confidence_milli: 0,
            orientation_confidence_milli: 0,
            destructive_quarter_turn_override: false,
            periods: Vec::new(),
            feature_positions: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CandidateSettings {
    pub scale_ladder: Vec<f64>,
    pub minimum_scale: f64,
    pub maximum_scale: f64,
    pub maximum_upscale: f64,
    pub minimum_usable_milli: u16,
    pub dense_grid_edge: u8,
    pub max_positions_per_size: u16,
    pub max_candidates_per_slot: u32,
    pub max_work: u64,
}

impl Default for CandidateSettings {
    fn default() -> Self {
        Self {
            scale_ladder: vec![0.5, 0.63, 0.8, 1.0, 1.25, 1.6, 2.0],
            minimum_scale: 0.5,
            maximum_scale: 2.0,
            maximum_upscale: 2.0,
            minimum_usable_milli: 1000,
            dense_grid_edge: 5,
            max_positions_per_size: 48,
            max_candidates_per_slot: 512,
            max_work: 100_000_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CandidateError {
    InvalidSettings,
    MalformedInput,
    ResourceLimitExceeded,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateQaView {
    CandidateWindows,
    SourceFootprints,
    PositionStrategies,
    Eligibility,
    SourceUsage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateSet {
    pub stage_result: StageResult,
    pub candidates: Vec<CropCandidate>,
    pub qa_views: Vec<CandidateQaView>,
    pub attempted_direct_windows: u32,
    pub rejected_unusable_windows: u32,
    pub truncated_candidates: u32,
    pub recovery_choices: Vec<RecoveryChoice>,
}

#[derive(Clone, Copy)]
struct Position {
    x: u32,
    y: u32,
    strategy: PositionStrategy,
    descriptors: CandidateDescriptors,
}

pub fn generate_candidates<D: MaterialDomainView, S: SlotDemandView>(
    domain: &D,
    slot: &S,
    evidence: &CandidateEvidence,
    settings: &CandidateSettings,
    seed: u64,
) -> Result<CandidateSet, CandidateError> {
    generate_candidates_with_guard(domain, slot, evidence, settings, seed, &|| false)
}

pub fn generate_candidates_with_guard<D: MaterialDomainView, S: SlotDemandView>(
    domain: &D,
    slot: &S,
    evidence: &CandidateEvidence,
    settings: &CandidateSettings,
    seed: u64,
    cancelled: &dyn Fn() -> bool,
) -> Result<CandidateSet, CandidateError> {
    if cancelled() {
        return Err(CandidateError::Cancelled);
    }
    validate(domain, slot, evidence, settings)?;
    let (domain_w, domain_h) = domain.dimensions();
    let pixels = u64::from(domain_w) * u64::from(domain_h);
    if pixels > settings.max_work {
        return Err(CandidateError::ResourceLimitExceeded);
    }
    let unusable_integral = build_unusable_integral(domain, domain_w, domain_h, cancelled)?;
    let transforms = legal_transforms(slot, evidence);
    let direct_possible = transforms
        .iter()
        .flat_map(|transform| crop_sizes(slot, settings, transform.rotation))
        .any(|(_, w, h)| {
            w <= domain_w
                && h <= domain_h
                && any_usable_window(&unusable_integral, domain_w, domain_h, w, h)
        });
    let direct_rejection = (!direct_possible).then(||
        "no exact-aspect crop at a permitted isotropic scale fits wholly inside the usable domain".into());
    let mut out = Vec::new();
    let mut attempted = 0_u32;
    let mut rejected = 0_u32;
    for transform in &transforms {
        if cancelled() {
            return Err(CandidateError::Cancelled);
        }
        for (scale, width, height) in crop_sizes(slot, settings, transform.rotation) {
            if width > domain_w || height > domain_h {
                continue;
            }
            let positions = positions(domain_w, domain_h, width, height, evidence, settings);
            for position in positions {
                if cancelled() {
                    return Err(CandidateError::Cancelled);
                }
                attempted = attempted.saturating_add(1);
                let crop = SourceCrop {
                    x: position.x,
                    y: position.y,
                    width,
                    height,
                };
                if !rect_usable(&unusable_integral, domain_w, crop) {
                    rejected = rejected.saturating_add(1);
                    continue;
                }
                for mode in slot.allowed_mapping_modes() {
                    for family in direct_families(slot, *mode) {
                        let lattice = aligned_period(crop, evidence, *mode);
                        if periodic_mode(*mode) && !evidence.periods.is_empty() && lattice.is_none()
                        {
                            continue;
                        }
                        let (route, cross_axis) =
                            route_and_cross_axis(*mode, slot.orientation(), crop, slot);
                        push_candidate(
                            &mut out,
                            domain,
                            slot,
                            crop,
                            *transform,
                            scale,
                            *mode,
                            family,
                            route,
                            position,
                            lattice,
                            cross_axis,
                            direct_possible,
                            direct_rejection.clone(),
                            seed,
                        );
                    }
                }
            }
        }
    }
    add_synthesis_candidates(
        &mut out,
        domain,
        slot,
        evidence,
        &transforms,
        direct_possible,
        direct_rejection.clone(),
        seed,
    );
    if cancelled() {
        return Err(CandidateError::Cancelled);
    }
    out.sort_by(|a, b| candidate_sort_key(a).cmp(&candidate_sort_key(b)));
    out.dedup_by(|a, b| a.candidate_id == b.candidate_id);
    let before = out.len();
    out = truncate_preserving_families(out, settings.max_candidates_per_slot as usize);
    let truncated = u32::try_from(before.saturating_sub(out.len())).unwrap_or(u32::MAX);
    let recovery_choices = if out.is_empty() {
        vec![
            RecoveryChoice::ChooseAnotherSource,
            RecoveryChoice::LowerTexelDensity,
            RecoveryChoice::UseSynthesis,
        ]
    } else if !direct_possible {
        vec![
            RecoveryChoice::ChooseAnotherSource,
            RecoveryChoice::LowerTexelDensity,
        ]
    } else {
        Vec::new()
    };
    let diagnostics = if !direct_possible {
        vec![CompilationDiagnostic {
            code: DiagnosticCode::InsufficientInput,
            stage: Some(11),
            message: direct_rejection.unwrap_or_default(),
            context: BTreeMap::from([
                ("domain_width".into(), domain_w.to_string()),
                ("domain_height".into(), domain_h.to_string()),
            ]),
        }]
    } else {
        Vec::new()
    };
    let settings_hash =
        ContentDigest::sha256(format!("{settings:?}|{evidence:?}|{seed}").as_bytes());
    Ok(CandidateSet {
        stage_result: StageResult::Executed {
            algorithm: AlgorithmProvenance {
                algorithm_id: STAGE_11_ALGORITHM_ID.into(),
                version: STAGE_11_ALGORITHM_VERSION.into(),
            },
            settings_hash,
            diagnostics,
        },
        candidates: out,
        qa_views: vec![
            CandidateQaView::CandidateWindows,
            CandidateQaView::SourceFootprints,
            CandidateQaView::PositionStrategies,
            CandidateQaView::Eligibility,
            CandidateQaView::SourceUsage,
        ],
        attempted_direct_windows: attempted,
        rejected_unusable_windows: rejected,
        truncated_candidates: truncated,
        recovery_choices,
    })
}

fn validate<D: MaterialDomainView, S: SlotDemandView>(
    domain: &D,
    slot: &S,
    evidence: &CandidateEvidence,
    s: &CandidateSettings,
) -> Result<(), CandidateError> {
    let (w, h) = domain.dimensions();
    let (dw, dh) = slot.destination_pixels();
    let (fw, fh, _) = slot.required_source_footprint();
    if w == 0
        || h == 0
        || dw == 0
        || dh == 0
        || !fw.is_finite()
        || !fh.is_finite()
        || fw <= 0.0
        || fh <= 0.0
    {
        return Err(CandidateError::MalformedInput);
    }
    if s.scale_ladder.is_empty()
        || s.scale_ladder.iter().any(|v| !v.is_finite() || *v <= 0.0)
        || !s.minimum_scale.is_finite()
        || !s.maximum_scale.is_finite()
        || !s.maximum_upscale.is_finite()
        || s.minimum_scale <= 0.0
        || s.minimum_scale > s.maximum_scale
        || s.maximum_upscale < 1.0
        || s.maximum_scale > s.maximum_upscale
        || s.minimum_usable_milli > 1000
        || s.dense_grid_edge < 2
        || s.max_positions_per_size == 0
        || s.max_candidates_per_slot == 0
        || s.max_work == 0
        || evidence.class_confidence_milli > 1000
        || evidence.orientation_confidence_milli > 1000
    {
        return Err(CandidateError::InvalidSettings);
    }
    Ok(())
}

fn crop_sizes<S: SlotDemandView>(
    slot: &S,
    s: &CandidateSettings,
    rotation: QuarterTurn,
) -> Vec<(f64, u32, u32)> {
    let (required_w, required_h, _) = slot.required_source_footprint();
    let rotated = matches!(rotation, QuarterTurn::Ninety | QuarterTurn::TwoSeventy);
    let (source_w, source_h) = if rotated {
        (required_h, required_w)
    } else {
        (required_w, required_h)
    };
    let mut result = Vec::new();
    for scale in &s.scale_ladder {
        if *scale + f64::EPSILON < s.minimum_scale || *scale > s.maximum_scale + f64::EPSILON {
            continue;
        }
        // One scalar is applied independently to both Stage 10 footprint axes. Never coerce
        // the source footprint to destination-pixel aspect by averaging the two axes.
        let width = (source_w * scale).round().clamp(1.0, f64::from(u32::MAX)) as u32;
        let height = (source_h * scale).round().clamp(1.0, f64::from(u32::MAX)) as u32;
        result.push((*scale, width, height));
    }
    result.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    result.dedup_by(|a, b| a.1 == b.1 && a.2 == b.2);
    result
}

fn legal_transforms<S: SlotDemandView>(slot: &S, e: &CandidateEvidence) -> Vec<CandidateTransform> {
    let directional = matches!(
        e.material_class,
        MaterialBehaviorClass::StochasticDirectional
            | MaterialBehaviorClass::LayeredBanded
            | MaterialBehaviorClass::OrganicDirectional
            | MaterialBehaviorClass::PeriodicLatticeStructured
            | MaterialBehaviorClass::ManufacturedPattern
    );
    let permits_destructive = e.destructive_quarter_turn_override
        || !directional
        || e.class_confidence_milli < 350
        || e.orientation_confidence_milli < 220;
    let mut rotations: Vec<_> = slot
        .allowed_rotations()
        .iter()
        .copied()
        .filter(|r| {
            !matches!(r, QuarterTurn::Ninety | QuarterTurn::TwoSeventy) || permits_destructive
        })
        .collect();
    rotations.sort_by_key(|r| match r {
        QuarterTurn::Zero => 0,
        QuarterTurn::OneEighty => 1,
        QuarterTurn::Ninety => 2,
        QuarterTurn::TwoSeventy => 3,
    });
    rotations.dedup();
    let mirrors: &[MirrorTransform] = if slot.mirror_allowed() {
        &[
            MirrorTransform::None,
            MirrorTransform::X,
            MirrorTransform::Y,
        ]
    } else {
        &[MirrorTransform::None]
    };
    rotations
        .into_iter()
        .flat_map(|rotation| {
            mirrors.iter().map(move |mirror| CandidateTransform {
                rotation,
                mirror: *mirror,
            })
        })
        .collect()
}

fn build_unusable_integral<D: MaterialDomainView>(
    domain: &D,
    w: u32,
    h: u32,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<u64>, CandidateError> {
    let stride = w as usize + 1;
    let mut integral = vec![0_u64; stride * (h as usize + 1)];
    for y in 0..h {
        if cancelled() {
            return Err(CandidateError::Cancelled);
        }
        let mut row = 0_u64;
        for x in 0..w {
            row += u64::from(!domain.valid(x, y));
            integral[(y as usize + 1) * stride + x as usize + 1] =
                integral[y as usize * stride + x as usize + 1] + row;
        }
    }
    Ok(integral)
}

fn rect_usable(integral: &[u64], domain_w: u32, r: SourceCrop) -> bool {
    let s = domain_w as usize + 1;
    let x0 = r.x as usize;
    let y0 = r.y as usize;
    let x1 = (r.x + r.width) as usize;
    let y1 = (r.y + r.height) as usize;
    integral[y1 * s + x1] + integral[y0 * s + x0] - integral[y0 * s + x1] - integral[y1 * s + x0]
        == 0
}

fn any_usable_window(integral: &[u64], dw: u32, dh: u32, w: u32, h: u32) -> bool {
    (0..=dh - h).any(|y| {
        (0..=dw - w).any(|x| {
            rect_usable(
                integral,
                dw,
                SourceCrop {
                    x,
                    y,
                    width: w,
                    height: h,
                },
            )
        })
    })
}

fn positions(
    dw: u32,
    dh: u32,
    w: u32,
    h: u32,
    e: &CandidateEvidence,
    s: &CandidateSettings,
) -> Vec<Position> {
    let max_x = dw - w;
    let max_y = dh - h;
    let mut map = BTreeMap::<(u32, u32, PositionStrategy), CandidateDescriptors>::new();
    let default = CandidateDescriptors {
        saliency_milli: 0,
        stationarity_milli: 500,
        feature_strength_milli: 0,
        usability_milli: 1000,
    };
    let grid = u32::from(s.dense_grid_edge);
    for gy in 0..grid {
        for gx in 0..grid {
            let x = if grid == 1 {
                0
            } else {
                max_x * gx / (grid - 1)
            };
            let y = if grid == 1 {
                0
            } else {
                max_y * gy / (grid - 1)
            };
            map.insert((x, y, PositionStrategy::DenseLowResolution), default);
        }
    }
    for &(x, y) in &[
        (0, 0),
        (max_x, 0),
        (0, max_y),
        (max_x, max_y),
        (max_x / 2, max_y / 2),
    ] {
        map.insert((x, y, PositionStrategy::CoarseToFine), default);
    }
    for f in &e.feature_positions {
        let x = f.x.saturating_sub(w / 2).min(max_x);
        let y = f.y.saturating_sub(h / 2).min(max_y);
        let d = CandidateDescriptors {
            saliency_milli: f.saliency_milli,
            stationarity_milli: f.stationarity_milli,
            feature_strength_milli: f.feature_strength_milli,
            usability_milli: 1000,
        };
        map.insert((x, y, PositionStrategy::FeatureAware), d);
        map.insert(
            (
                x,
                y,
                if f.saliency_milli >= f.stationarity_milli {
                    PositionStrategy::Saliency
                } else {
                    PositionStrategy::StationaryZone
                },
            ),
            d,
        );
    }
    for p in &e.periods {
        if p[0] > 0 && p[1] > 0 {
            let mut y = 0;
            while y <= max_y {
                let mut x = 0;
                while x <= max_x {
                    map.insert((x, y, PositionStrategy::PeriodAligned), default);
                    x = x.saturating_add(p[0]);
                    if x == 0 {
                        break;
                    }
                }
                y = y.saturating_add(p[1]);
                if y == 0 {
                    break;
                }
            }
        }
    }
    let mut selected = map
        .into_iter()
        .map(|((x, y, strategy), descriptors)| Position {
            x,
            y,
            strategy,
            descriptors,
        })
        .collect::<Vec<_>>();
    selected.sort_by_key(|p| (p.strategy, p.y, p.x));
    selected.truncate(s.max_positions_per_size as usize);
    // Search a bounded probe lattice for a genuinely new center maximizing its minimum
    // squared distance to all already selected centers. Stable coordinate ties prefer Y/X.
    let probes = u32::from(s.dense_grid_edge)
        .saturating_mul(2)
        .saturating_add(1);
    let occupied = selected.iter().map(|p| (p.x, p.y)).collect::<BTreeSet<_>>();
    let mut best: Option<(u64, u32, u32)> = None;
    for py in 0..probes {
        for px in 0..probes {
            let x = if probes == 1 {
                0
            } else {
                max_x * px / (probes - 1)
            };
            let y = if probes == 1 {
                0
            } else {
                max_y * py / (probes - 1)
            };
            if occupied.contains(&(x, y)) {
                continue;
            }
            let distance = selected
                .iter()
                .map(|p| {
                    let dx = i64::from(x) - i64::from(p.x);
                    let dy = i64::from(y) - i64::from(p.y);
                    (dx * dx + dy * dy) as u64
                })
                .min()
                .unwrap_or(u64::MAX);
            if best.is_none_or(|old| {
                (distance, std::cmp::Reverse(y), std::cmp::Reverse(x))
                    > (old.0, std::cmp::Reverse(old.1), std::cmp::Reverse(old.2))
            }) {
                best = Some((distance, y, x));
            }
        }
    }
    if let Some((_, y, x)) = best {
        selected.push(Position {
            x,
            y,
            strategy: PositionStrategy::FarthestPoint,
            descriptors: default,
        });
    }
    selected
}

fn direct_families<S: SlotDemandView>(slot: &S, mode: SamplingMode) -> Vec<CandidateFamily> {
    match mode {
        SamplingMode::DirectCrop if slot.role() == TemplateSlotRole::Planar => {
            vec![CandidateFamily::PanelDirect]
        }
        SamplingMode::DirectCrop
            if slot.role() == TemplateSlotRole::RepeatingStrip
                && slot.orientation() == RegionOrientation::Vertical =>
        {
            vec![CandidateFamily::RepeatYSegment]
        }
        SamplingMode::DirectCrop if slot.role() == TemplateSlotRole::RepeatingStrip => {
            vec![CandidateFamily::RepeatXSegment]
        }
        SamplingMode::DirectCrop if slot.role() == TemplateSlotRole::TrimCap => {
            vec![CandidateFamily::ThreeSliceCap]
        }
        SamplingMode::PeriodicTile if slot.role() == TemplateSlotRole::Planar => {
            vec![CandidateFamily::PanelSeamlessTile]
        }
        SamplingMode::RepeatX => vec![
            CandidateFamily::RepeatXSegment,
            CandidateFamily::RepeatXContiguous,
            CandidateFamily::RepeatXGraphCut,
        ],
        SamplingMode::RepeatY => vec![
            CandidateFamily::RepeatYSegment,
            CandidateFamily::RepeatYContiguous,
            CandidateFamily::RepeatYGraphCut,
        ],
        SamplingMode::UniqueContain => vec![
            CandidateFamily::UniqueContain,
            CandidateFamily::UniquePatchBase,
        ],
        SamplingMode::UniqueCover => vec![CandidateFamily::UniqueCover],
        SamplingMode::ThreeSliceCap => vec![CandidateFamily::ThreeSliceCap],
        SamplingMode::NineSlicePanel => vec![CandidateFamily::NineSlicePanel],
        SamplingMode::PlanarRadial => vec![
            CandidateFamily::PlanarRadialSquare,
            CandidateFamily::PlanarRadialDetail,
            CandidateFamily::PlanarRadialAnnularProfile,
        ],
        _ => Vec::new(),
    }
}

fn periodic_mode(mode: SamplingMode) -> bool {
    matches!(
        mode,
        SamplingMode::PeriodicTile | SamplingMode::RepeatX | SamplingMode::RepeatY
    )
}
fn aligned_period(c: SourceCrop, e: &CandidateEvidence, mode: SamplingMode) -> Option<[u32; 2]> {
    e.periods.iter().copied().find(|p| {
        p[0] > 0
            && p[1] > 0
            && c.x % p[0] == 0
            && c.y % p[1] == 0
            && match mode {
                SamplingMode::RepeatX => c.width % p[0] == 0,
                SamplingMode::RepeatY => c.height % p[1] == 0,
                SamplingMode::PeriodicTile => c.width % p[0] == 0 && c.height % p[1] == 0,
                _ => true,
            }
    })
}
fn route_and_cross_axis<S: SlotDemandView>(
    mode: SamplingMode,
    o: RegionOrientation,
    _c: SourceCrop,
    _slot: &S,
) -> (CandidateRoute, Option<bool>) {
    match mode {
        SamplingMode::RepeatX => (
            CandidateRoute::Repeat,
            Some(o != RegionOrientation::Vertical),
        ),
        SamplingMode::RepeatY => (
            CandidateRoute::Repeat,
            Some(o == RegionOrientation::Vertical),
        ),
        SamplingMode::DirectCrop if _slot.role() == TemplateSlotRole::RepeatingStrip => {
            (CandidateRoute::Direct, Some(true))
        }
        SamplingMode::UniqueContain | SamplingMode::UniqueCover => (CandidateRoute::Unique, None),
        SamplingMode::ThreeSliceCap | SamplingMode::NineSlicePanel => (CandidateRoute::Cap, None),
        SamplingMode::PlanarRadial => (CandidateRoute::PlanarRadial, None),
        _ => (CandidateRoute::Direct, None),
    }
}

#[allow(clippy::too_many_arguments)]
fn push_candidate<D: MaterialDomainView, S: SlotDemandView>(
    out: &mut Vec<CropCandidate>,
    domain: &D,
    slot: &S,
    crop: SourceCrop,
    transform: CandidateTransform,
    scale: f64,
    mode: SamplingMode,
    family: CandidateFamily,
    route: CandidateRoute,
    position: Position,
    period: Option<[u32; 2]>,
    cross: Option<bool>,
    direct: bool,
    rejection: Option<String>,
    seed: u64,
) {
    let seam_indices = match mode {
        SamplingMode::RepeatX => domain.seam_indices(SeamAxis::X),
        SamplingMode::RepeatY => domain.seam_indices(SeamAxis::Y),
        SamplingMode::PeriodicTile => {
            let mut v = domain.seam_indices(SeamAxis::X);
            v.extend(domain.seam_indices(SeamAxis::Y));
            v
        }
        _ => Vec::new(),
    };
    let id = ContentDigest::sha256(
        format!(
            "{}|{:?}|{}:{}:{}:{}|{:?}|{:?}|{:?}|{:?}|{scale:.12}|{seed}",
            domain.domain_id().0,
            slot.slot_id(),
            crop.x,
            crop.y,
            crop.width,
            crop.height,
            transform,
            mode,
            family,
            position.strategy
        )
        .as_bytes(),
    );
    out.push(CropCandidate {
        candidate_id: id,
        source_id: domain.source_id().clone(),
        domain_id: domain.domain_id().clone(),
        slot_id: slot.slot_id(),
        crop: Some(crop),
        transform,
        isotropic_scale: scale,
        mapping_mode: mode,
        family,
        route,
        position_strategy: position.strategy,
        period_pixels: period,
        seam_indices,
        correspondence_reference: domain.domain_id().clone(),
        descriptors: position.descriptors,
        seed,
        eligibility: EligibilityEvidence {
            mapping_permitted: true,
            transform_permitted: true,
            isotropic_scale: true,
            exact_aspect: true,
            entire_crop_usable: Some(true),
            cross_axis_preserved: cross,
            lattice_aligned: if periodic_mode(mode) {
                period.map(|_| true)
            } else {
                None
            },
            direct_crop_applicable: direct,
            direct_crop_rejection: rejection,
            reasons: Vec::new(),
        },
    })
}

fn add_synthesis_candidates<D: MaterialDomainView, S: SlotDemandView>(
    out: &mut Vec<CropCandidate>,
    domain: &D,
    slot: &S,
    e: &CandidateEvidence,
    transforms: &[CandidateTransform],
    direct: bool,
    rejection: Option<String>,
    seed: u64,
) {
    let transform = transforms.first().copied().unwrap_or(CandidateTransform {
        rotation: QuarterTurn::Zero,
        mirror: MirrorTransform::None,
    });
    for mode in slot.allowed_mapping_modes() {
        let families: &[CandidateFamily] = match mode {
            SamplingMode::TextureSynthesis if slot.role() == TemplateSlotRole::Planar => &[
                CandidateFamily::PanelQuiltedExpansion,
                CandidateFamily::PanelPatchMatchExpansion,
                CandidateFamily::PanelProceduralResynthesis,
            ],
            SamplingMode::TextureSynthesis
                if slot.role() == TemplateSlotRole::RepeatingStrip
                    && slot.orientation() == RegionOrientation::Vertical =>
            {
                &[CandidateFamily::RepeatYQuilted]
            }
            SamplingMode::TextureSynthesis if slot.role() == TemplateSlotRole::RepeatingStrip => {
                &[CandidateFamily::RepeatXQuilted]
            }
            SamplingMode::TextureSynthesis if slot.role() == TemplateSlotRole::UniqueDetail => {
                &[CandidateFamily::UniqueSynthesisExtension]
            }
            SamplingMode::PolarRadial => &[CandidateFamily::PolarRadialSynthesis],
            _ => &[],
        };
        for family in families {
            let route = if *mode == SamplingMode::PolarRadial {
                CandidateRoute::PolarRadial
            } else {
                CandidateRoute::Synthesis
            };
            let id = ContentDigest::sha256(
                format!(
                    "{}|{:?}|{:?}|{:?}|{seed}",
                    domain.domain_id().0,
                    slot.slot_id(),
                    mode,
                    family
                )
                .as_bytes(),
            );
            out.push(CropCandidate {
                candidate_id: id,
                source_id: domain.source_id().clone(),
                domain_id: domain.domain_id().clone(),
                slot_id: slot.slot_id(),
                crop: None,
                transform,
                isotropic_scale: 1.0,
                mapping_mode: *mode,
                family: *family,
                route,
                position_strategy: PositionStrategy::CoarseToFine,
                period_pixels: e.periods.first().copied(),
                seam_indices: Vec::new(),
                correspondence_reference: domain.domain_id().clone(),
                descriptors: CandidateDescriptors {
                    saliency_milli: 0,
                    stationarity_milli: 0,
                    feature_strength_milli: 0,
                    usability_milli: 1000,
                },
                seed,
                eligibility: EligibilityEvidence {
                    mapping_permitted: true,
                    transform_permitted: true,
                    isotropic_scale: true,
                    exact_aspect: true,
                    entire_crop_usable: None,
                    cross_axis_preserved: matches!(slot.role(), TemplateSlotRole::RepeatingStrip)
                        .then_some(true),
                    lattice_aligned: None,
                    direct_crop_applicable: direct,
                    direct_crop_rejection: rejection.clone(),
                    reasons: vec![format!(
                        "typed synthesis route {:?}; direct applicability remains explicit",
                        domain.route()
                    )],
                },
            });
        }
    }
}

fn truncate_preserving_families(
    candidates: Vec<CropCandidate>,
    limit: usize,
) -> Vec<CropCandidate> {
    if candidates.len() <= limit {
        return candidates;
    }
    let mut groups = BTreeMap::<CandidateFamily, Vec<CropCandidate>>::new();
    for candidate in candidates {
        groups.entry(candidate.family).or_default().push(candidate);
    }
    let mut kept = Vec::with_capacity(limit);
    let mut depth = 0_usize;
    while kept.len() < limit {
        let mut added = false;
        for group in groups.values() {
            if let Some(candidate) = group.get(depth) {
                kept.push(candidate.clone());
                added = true;
                if kept.len() == limit {
                    break;
                }
            }
        }
        if !added {
            break;
        }
        depth += 1;
    }
    kept.sort_by(|a, b| candidate_sort_key(a).cmp(&candidate_sort_key(b)));
    kept
}

fn candidate_sort_key(c: &CropCandidate) -> (u8, u8, u32, u32, u32, u32, u8, u8, String) {
    let crop = c.crop.unwrap_or(SourceCrop {
        x: u32::MAX,
        y: u32::MAX,
        width: 0,
        height: 0,
    });
    (
        mode_rank(c.mapping_mode),
        strategy_rank(c.position_strategy),
        crop.y,
        crop.x,
        crop.height,
        crop.width,
        rotation_rank(c.transform.rotation),
        mirror_rank(c.transform.mirror),
        c.candidate_id.0.clone(),
    )
}
fn mode_rank(m: SamplingMode) -> u8 {
    match m {
        SamplingMode::DirectCrop => 0,
        SamplingMode::PeriodicTile => 1,
        SamplingMode::RepeatX => 2,
        SamplingMode::RepeatY => 3,
        SamplingMode::UniqueContain => 4,
        SamplingMode::UniqueCover => 5,
        SamplingMode::ThreeSliceCap => 6,
        SamplingMode::NineSlicePanel => 7,
        SamplingMode::PlanarRadial => 8,
        SamplingMode::PolarRadial => 9,
        SamplingMode::TextureSynthesis => 10,
        SamplingMode::ExplicitStretch => 11,
    }
}
fn strategy_rank(s: PositionStrategy) -> u8 {
    match s {
        PositionStrategy::DenseLowResolution => 0,
        PositionStrategy::CoarseToFine => 1,
        PositionStrategy::FeatureAware => 2,
        PositionStrategy::Saliency => 3,
        PositionStrategy::StationaryZone => 4,
        PositionStrategy::PeriodAligned => 5,
        PositionStrategy::FarthestPoint => 6,
    }
}
fn rotation_rank(r: QuarterTurn) -> u8 {
    match r {
        QuarterTurn::Zero => 0,
        QuarterTurn::OneEighty => 1,
        QuarterTurn::Ninety => 2,
        QuarterTurn::TwoSeventy => 3,
    }
}
fn mirror_rank(m: MirrorTransform) -> u8 {
    match m {
        MirrorTransform::None => 0,
        MirrorTransform::X => 1,
        MirrorTransform::Y => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Clone)]
    struct Domain {
        id: ContentDigest,
        w: u32,
        h: u32,
        invalid: BTreeSet<(u32, u32)>,
    }
    impl MaterialDomainView for Domain {
        fn domain_id(&self) -> &ContentDigest {
            &self.id
        }
        fn source_id(&self) -> &ContentDigest {
            &self.id
        }
        fn dimensions(&self) -> (u32, u32) {
            (self.w, self.h)
        }
        fn route(&self) -> DomainRoute {
            DomainRoute::DirectSource
        }
        fn valid(&self, x: u32, y: u32) -> bool {
            !self.invalid.contains(&(x, y))
        }
        fn seam_indices(&self, _: SeamAxis) -> Vec<u32> {
            vec![0]
        }
    }
    struct Slot {
        role: TemplateSlotRole,
        o: RegionOrientation,
        modes: Vec<SamplingMode>,
        rot: Vec<QuarterTurn>,
        mirror: bool,
        foot: (f64, f64),
        dest: (u32, u32),
    }
    impl SlotDemandView for Slot {
        fn slot_id(&self) -> RegionId {
            RegionId::from_bytes([7; 16])
        }
        fn role(&self) -> TemplateSlotRole {
            self.role
        }
        fn orientation(&self) -> RegionOrientation {
            self.o
        }
        fn destination_pixels(&self) -> (u32, u32) {
            self.dest
        }
        fn required_source_footprint(&self) -> (f64, f64, SourceFootprintKind) {
            (self.foot.0, self.foot.1, SourceFootprintKind::SourcePixels)
        }
        fn allowed_mapping_modes(&self) -> &[SamplingMode] {
            &self.modes
        }
        fn allowed_rotations(&self) -> &[QuarterTurn] {
            &self.rot
        }
        fn mirror_allowed(&self) -> bool {
            self.mirror
        }
    }
    #[test]
    fn algorithm_stage_11_candidates() {
        let domain = Domain {
            id: ContentDigest::sha256(b"domain"),
            w: 160,
            h: 96,
            invalid: BTreeSet::from([(0, 0)]),
        };
        let slot = Slot {
            role: TemplateSlotRole::RepeatingStrip,
            o: RegionOrientation::Horizontal,
            modes: vec![SamplingMode::RepeatX, SamplingMode::TextureSynthesis],
            rot: vec![
                QuarterTurn::Zero,
                QuarterTurn::Ninety,
                QuarterTurn::OneEighty,
            ],
            mirror: true,
            foot: (80.0, 16.0),
            dest: (80, 16),
        };
        let evidence = CandidateEvidence {
            material_class: MaterialBehaviorClass::OrganicDirectional,
            class_confidence_milli: 900,
            orientation_confidence_milli: 900,
            destructive_quarter_turn_override: false,
            periods: vec![[10, 8]],
            feature_positions: vec![FeaturePosition {
                x: 80,
                y: 48,
                saliency_milli: 700,
                stationarity_milli: 300,
                feature_strength_milli: 800,
            }],
        };
        let settings = CandidateSettings {
            max_candidates_per_slot: 128,
            ..CandidateSettings::default()
        };
        let a = generate_candidates(&domain, &slot, &evidence, &settings, 41).unwrap();
        let b = generate_candidates(&domain, &slot, &evidence, &settings, 41).unwrap();
        assert_eq!(a.candidates, b.candidates);
        assert!(!a.candidates.is_empty());
        assert!(
            a.candidates
                .iter()
                .all(|c| c.isotropic_scale.is_finite() && c.eligibility.isotropic_scale)
        );
        assert!(
            a.candidates
                .iter()
                .all(|c| c.transform.rotation != QuarterTurn::Ninety)
        );
        assert!(
            a.candidates
                .iter()
                .filter_map(|c| c.crop)
                .all(|r| r.width * slot.dest.1 == r.height * slot.dest.0)
        );
        assert!(
            a.candidates
                .iter()
                .filter(|c| c.mapping_mode == SamplingMode::RepeatX)
                .all(|c| c.eligibility.cross_axis_preserved == Some(true))
        );
        assert!(
            a.candidates.iter().filter(|c| c.crop.is_none()).all(|c| c
                .eligibility
                .direct_crop_rejection
                .is_none()
                == c.eligibility.direct_crop_applicable)
        );
        assert!(a.qa_views.contains(&CandidateQaView::SourceFootprints));
    }

    #[test]
    fn source_placement_acceptance() {
        let domain = Domain {
            id: ContentDigest::sha256(b"acceptance-domain"),
            w: 200,
            h: 200,
            invalid: BTreeSet::new(),
        };
        let slot = Slot {
            role: TemplateSlotRole::Planar,
            o: RegionOrientation::Unspecified,
            modes: vec![
                SamplingMode::DirectCrop,
                SamplingMode::PeriodicTile,
                SamplingMode::TextureSynthesis,
            ],
            rot: vec![QuarterTurn::Zero, QuarterTurn::Ninety],
            mirror: false,
            foot: (80.0, 20.0),
            dest: (80, 16),
        };
        let evidence = CandidateEvidence::default();
        let settings = CandidateSettings {
            scale_ladder: vec![1.0],
            minimum_scale: 1.0,
            maximum_scale: 1.0,
            max_candidates_per_slot: 5,
            ..CandidateSettings::default()
        };
        assert_eq!(
            crop_sizes(&slot, &settings, QuarterTurn::Zero),
            vec![(1.0, 80, 20)]
        );
        assert_eq!(
            crop_sizes(&slot, &settings, QuarterTurn::Ninety),
            vec![(1.0, 20, 80)]
        );
        let result = generate_candidates(&domain, &slot, &evidence, &settings, 77).unwrap();
        let families = result
            .candidates
            .iter()
            .map(|c| c.family)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            families,
            BTreeSet::from([
                CandidateFamily::PanelDirect,
                CandidateFamily::PanelSeamlessTile,
                CandidateFamily::PanelQuiltedExpansion,
                CandidateFamily::PanelPatchMatchExpansion,
                CandidateFamily::PanelProceduralResynthesis
            ])
        );
        assert!(
            result
                .candidates
                .iter()
                .filter(|c| c.mapping_mode == SamplingMode::PeriodicTile)
                .all(|c| c.eligibility.lattice_aligned.is_none())
        );
        let all_positions = positions(200, 200, 80, 20, &evidence, &CandidateSettings::default());
        let ordinary = all_positions
            .iter()
            .filter(|p| p.strategy != PositionStrategy::FarthestPoint)
            .map(|p| (p.x, p.y))
            .collect::<BTreeSet<_>>();
        let farthest = all_positions
            .iter()
            .find(|p| p.strategy == PositionStrategy::FarthestPoint)
            .unwrap();
        assert!(!ordinary.contains(&(farthest.x, farthest.y)));
    }
}
