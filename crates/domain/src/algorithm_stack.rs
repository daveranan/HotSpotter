//! Stable, material-agnostic contracts for the twenty-stage compiler.

use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const ALGORITHM_STACK_CONTRACT_VERSION: u16 = 1;
pub const MATERIAL_CORPUS_MANIFEST_JSON: &str =
    include_str!("../../../fixtures/algorithm-stack/material-corpus.json");
pub const ALGORITHM_TRACEABILITY_JSON: &str =
    include_str!("../../../fixtures/algorithm-stack/stage-traceability.json");

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialCorpusManifest {
    pub schema_version: u16,
    pub behavior_classes: Vec<CorpusBehaviorClass>,
    pub source_conditions: Vec<CorpusSourceCondition>,
    pub registered_map_combinations: Vec<RegisteredMapCombination>,
    pub semantic_slot_roles: Vec<CorpusSlotRole>,
    pub synthetic_fixtures: Vec<SyntheticFixtureSpec>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorpusBehaviorClass {
    pub id: String,
    pub expected_properties: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorpusSourceCondition {
    pub id: String,
    pub expected_properties: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredMapCombination {
    pub id: String,
    pub channels: Vec<RegisteredChannelRole>,
    pub expected_properties: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegisteredChannelRole {
    BaseColor,
    Normal,
    Height,
    Roughness,
    Metallic,
    AmbientOcclusion,
    Specular,
    Opacity,
    EdgeMask,
    MaterialId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorpusSlotRole {
    pub id: String,
    pub expected_properties: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyntheticFixtureSpec {
    pub id: String,
    pub generator: SyntheticGenerator,
    pub seed: u64,
    pub width: u32,
    pub height: u32,
    pub provenance: FixtureProvenance,
    pub expected_properties: BTreeMap<String, i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyntheticGenerator {
    Structure,
    Orientation,
    Periodicity,
    Saliency,
    Registration,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixtureProvenance {
    pub generator_id: String,
    pub generator_version: u16,
    pub generated_not_captured: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntheticFixture {
    pub spec: SyntheticFixtureSpec,
    /// Registered integer planes in stable channel-name order.
    pub planes: BTreeMap<String, Vec<u16>>,
}

impl MaterialCorpusManifest {
    /// Parses the checked-in corpus that is shared by every algorithm prompt.
    pub fn bundled() -> Result<Self, serde_json::Error> {
        serde_json::from_str(MATERIAL_CORPUS_MANIFEST_JSON)
    }
}

impl SyntheticFixtureSpec {
    /// Generates exact integer fixtures; no decoder, clock, thread order, or floating-point
    /// reduction can affect their bytes.
    #[must_use]
    pub fn generate(&self) -> SyntheticFixture {
        let len = usize::try_from(u64::from(self.width) * u64::from(self.height))
            .expect("fixture dimensions are bounded by the manifest");
        let mut base = Vec::with_capacity(len);
        for y in 0..self.height {
            for x in 0..self.width {
                let value = match self.generator {
                    SyntheticGenerator::Structure => {
                        let line_x = self.expected_properties.get("lineX").copied().unwrap_or(16);
                        let line_y = self.expected_properties.get("lineY").copied().unwrap_or(24);
                        if i64::from(x) == line_x || i64::from(y) == line_y {
                            65_535
                        } else {
                            4_096
                        }
                    }
                    SyntheticGenerator::Orientation => {
                        let rise = self.expected_properties.get("rise").copied().unwrap_or(1);
                        let run = self.expected_properties.get("run").copied().unwrap_or(2);
                        let phase = (i64::from(x) * rise - i64::from(y) * run).rem_euclid(16);
                        u16::try_from(phase * 4_096).unwrap_or(u16::MAX)
                    }
                    SyntheticGenerator::Periodicity => {
                        let px = self
                            .expected_properties
                            .get("periodX")
                            .copied()
                            .unwrap_or(8)
                            .max(1);
                        let py = self
                            .expected_properties
                            .get("periodY")
                            .copied()
                            .unwrap_or(8)
                            .max(1);
                        if i64::from(x) % px == 0 || i64::from(y) % py == 0 {
                            57_344
                        } else {
                            8_192
                        }
                    }
                    SyntheticGenerator::Saliency => {
                        let cx = self
                            .expected_properties
                            .get("centerX")
                            .copied()
                            .unwrap_or(32);
                        let cy = self
                            .expected_properties
                            .get("centerY")
                            .copied()
                            .unwrap_or(32);
                        let dx = i64::from(x) - cx;
                        let dy = i64::from(y) - cy;
                        if dx * dx + dy * dy <= 25 {
                            65_535
                        } else {
                            stable_noise(self.seed, x, y)
                        }
                    }
                    SyntheticGenerator::Registration => stable_noise(self.seed, x, y),
                };
                base.push(value);
            }
        }
        let mut planes = BTreeMap::from([(String::from("base_color"), base.clone())]);
        if self.generator == SyntheticGenerator::Registration {
            planes.insert(String::from("height"), base.clone());
            planes.insert(String::from("normal"), base.clone());
            planes.insert(String::from("roughness"), base);
        }
        SyntheticFixture {
            spec: self.clone(),
            planes,
        }
    }
}

fn stable_noise(seed: u64, x: u32, y: u32) -> u16 {
    let mut z = seed ^ (u64::from(x) << 32) ^ u64::from(y);
    z ^= z >> 30;
    z = z.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z ^= z >> 27;
    z = z.wrapping_mul(0x94d0_49bb_1331_11eb);
    u16::try_from((z ^ (z >> 31)) & 0xffff).expect("masked to u16")
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum StageResult {
    Executed {
        algorithm: AlgorithmProvenance,
        settings_hash: ContentDigest,
        diagnostics: Vec<CompilationDiagnostic>,
    },
    PassThrough {
        reason: String,
    },
    SkippedBecauseUnused {
        reason: String,
    },
    FailedWithRecovery {
        reason: CompilationDiagnostic,
        recovery_choices: Vec<RecoveryChoice>,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlgorithmProvenance {
    pub algorithm_id: String,
    pub version: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContentDigest(pub String);

impl ContentDigest {
    #[must_use]
    pub fn sha256(bytes: &[u8]) -> Self {
        Self(format!("{:x}", Sha256::digest(bytes)))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompilationDiagnostic {
    pub code: DiagnosticCode,
    pub stage: Option<u8>,
    pub message: String,
    pub context: BTreeMap<String, String>,
}

impl CompilationDiagnostic {
    #[must_use]
    pub fn unsupported_stage(stage: u8) -> Self {
        Self {
            code: DiagnosticCode::UnsupportedStage,
            stage: Some(stage),
            message: format!("algorithm stage {stage} is not installed"),
            context: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCode {
    UnsupportedStage,
    Cancelled,
    RevisionSuperseded,
    ResourceLimitExceeded,
    MalformedInput,
    InsufficientInput,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryChoice {
    ChooseAnotherSource,
    UseSynthesis,
    LowerTexelDensity,
    IncreaseOutputResolution,
    AdjustSettings,
    DisableEffect,
    ExplicitStretch,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompilerRequestHeader {
    pub contract_version: u16,
    pub source_digests: Vec<ContentDigest>,
    pub settings_hash: ContentDigest,
    pub algorithm_versions: BTreeMap<u8, AlgorithmProvenance>,
    pub template_topology_hash: ContentDigest,
    pub output: OutputSpecHeader,
    pub seed: u64,
    pub revision: u64,
}

impl CompilerRequestHeader {
    /// Returns canonical JSON bytes. Ordered maps and struct fields make serialization stable.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub fn cache_key(&self) -> Result<CacheKey, serde_json::Error> {
        Ok(CacheKey(ContentDigest::sha256(&self.canonical_bytes()?)))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputSpecHeader {
    pub width: u32,
    pub height: u32,
    pub mip_count: u8,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CacheKey(pub ContentDigest);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheArtifactKind {
    PreparedSources,
    MaterialAnalysis,
    MaterialDomain,
    PlacementPlan,
    EffectPlan,
    CompiledSheet,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CachedArtifact {
    pub key: CacheKey,
    pub kind: CacheArtifactKind,
    pub bytes: Arc<[u8]>,
}

pub trait ContentAddressedCache: Send + Sync {
    fn load(
        &self,
        key: &CacheKey,
        kind: CacheArtifactKind,
    ) -> Result<Option<CachedArtifact>, CacheError>;
    /// Implementations must publish atomically only after the complete digest-verified payload is written.
    fn publish_complete(&self, artifact: CachedArtifact) -> Result<(), CacheError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CacheError {
    Io(String),
    DigestMismatch,
    ResourceLimitExceeded,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedSources {
    pub header: ArtifactHeader,
    pub stage_result: StageResult,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacementPlanHeader {
    pub header: ArtifactHeader,
    pub solver: AlgorithmProvenance,
    pub slot_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SamplingPlanHeader {
    pub header: ArtifactHeader,
    pub slot_id: String,
    pub mode: SamplingMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SamplingMode {
    DirectCrop,
    PeriodicTile,
    RepeatX,
    RepeatY,
    TextureSynthesis,
    UniqueContain,
    UniqueCover,
    ThreeSliceCap,
    NineSlicePanel,
    PlanarRadial,
    PolarRadial,
    ExplicitStretch,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectPlanHeader {
    pub header: ArtifactHeader,
    pub compiler: AlgorithmProvenance,
    pub effect_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactHeader {
    pub request_key: CacheKey,
    pub algorithm: AlgorithmProvenance,
    pub seed: u64,
    pub revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompilationReport {
    pub header: CompilationReportHeader,
    pub stages: BTreeMap<u8, StageResult>,
    pub diagnostics: Vec<CompilationDiagnostic>,
}

impl CompilationReport {
    pub fn deterministic_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompilationReportHeader {
    pub contract_version: u16,
    pub request_key: CacheKey,
    pub compiler: AlgorithmProvenance,
    pub seed: u64,
    pub revision: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceLimits {
    pub max_source_edge: u32,
    pub max_output_edge: u32,
    pub max_total_pixels: u64,
    pub max_cache_write_bytes: u64,
    pub max_candidates_per_slot: u32,
    pub max_graph_nodes: u64,
    pub max_iterations: u32,
    pub max_supersample_factor: u8,
    pub max_effect_operations: u32,
}

impl ResourceLimits {
    pub const V1_BOUNDED: Self = Self {
        max_source_edge: 8_192,
        max_output_edge: 8_192,
        max_total_pixels: 67_108_864,
        max_cache_write_bytes: 1_073_741_824,
        max_candidates_per_slot: 64,
        max_graph_nodes: 16_777_216,
        max_iterations: 1_024,
        max_supersample_factor: 8,
        max_effect_operations: 16_384,
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "policy", rename_all = "snake_case")]
pub enum SeedPolicy {
    Fixed { seed: u64 },
    DerivedFromRequest,
}

impl SeedPolicy {
    #[must_use]
    pub fn resolve(self, request_key: &CacheKey) -> u64 {
        match self {
            Self::Fixed { seed } => seed,
            Self::DerivedFromRequest => {
                let bytes = request_key.0.0.as_bytes();
                bytes
                    .iter()
                    .take(16)
                    .fold(0xcbf2_9ce4_8422_2325, |acc, byte| {
                        (acc ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
                    })
            }
        }
    }
}

/// Stable ordering helper: callers supply the complete semantic key, including a unique ID as
/// the final component. It deliberately avoids unstable hash-map or scheduler order.
pub fn stable_tie_break<T, K: Ord, F: FnMut(&T) -> K>(values: &mut [T], key: F) {
    values.sort_by_key(key);
}

#[derive(Clone, Debug, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Clone, Debug)]
pub struct RevisionAuthority(Arc<AtomicU64>);

impl RevisionAuthority {
    #[must_use]
    pub fn new(revision: u64) -> Self {
        Self(Arc::new(AtomicU64::new(revision)))
    }
    pub fn supersede_with(&self, revision: u64) {
        self.0.store(revision, Ordering::Release);
    }
    #[must_use]
    pub fn current(&self) -> u64 {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Clone, Debug)]
pub struct PublicationGuard {
    expected_revision: u64,
    cancellation: CancellationToken,
    revisions: RevisionAuthority,
}

impl PublicationGuard {
    #[must_use]
    pub fn new(
        expected_revision: u64,
        cancellation: CancellationToken,
        revisions: RevisionAuthority,
    ) -> Self {
        Self {
            expected_revision,
            cancellation,
            revisions,
        }
    }

    pub fn authorize_complete_publish(&self) -> Result<(), CompilationDiagnostic> {
        if self.cancellation.is_cancelled() {
            return Err(CompilationDiagnostic {
                code: DiagnosticCode::Cancelled,
                stage: None,
                message: String::from("compilation was cancelled before atomic publication"),
                context: BTreeMap::new(),
            });
        }
        if self.revisions.current() != self.expected_revision {
            return Err(CompilationDiagnostic {
                code: DiagnosticCode::RevisionSuperseded,
                stage: None,
                message: String::from("a newer document revision superseded compilation"),
                context: BTreeMap::new(),
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceabilityMatrix {
    pub schema_version: u16,
    pub stages: Vec<StageTraceability>,
    pub acceptance_invariants: Vec<AcceptanceTraceability>,
}

impl TraceabilityMatrix {
    pub fn bundled() -> Result<Self, serde_json::Error> {
        serde_json::from_str(ALGORITHM_TRACEABILITY_JSON)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageTraceability {
    pub stage: u8,
    pub requirement: String,
    pub owning_prompt: String,
    pub owner: String,
    pub planned_test: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptanceTraceability {
    pub id: String,
    pub section: String,
    pub invariant: String,
    pub owning_prompt: String,
    pub planned_test: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> CompilerRequestHeader {
        CompilerRequestHeader {
            contract_version: ALGORITHM_STACK_CONTRACT_VERSION,
            source_digests: vec![ContentDigest::sha256(b"source-a")],
            settings_hash: ContentDigest::sha256(b"settings"),
            algorithm_versions: BTreeMap::from([(
                1,
                AlgorithmProvenance {
                    algorithm_id: String::from("source-ingestion"),
                    version: String::from("0.0.0-unsupported"),
                },
            )]),
            template_topology_hash: ContentDigest::sha256(b"fixed-template"),
            output: OutputSpecHeader {
                width: 2048,
                height: 2048,
                mip_count: 4,
            },
            seed: 7,
            revision: 3,
        }
    }

    #[test]
    fn algorithm_stack_contract_manifest_traceability_and_determinism() {
        let corpus = MaterialCorpusManifest::bundled().expect("valid corpus");
        assert_eq!(corpus.behavior_classes.len(), 10);
        assert!(corpus.source_conditions.len() >= 8);
        assert_eq!(corpus.semantic_slot_roles.len(), 7);
        for fixture in &corpus.synthetic_fixtures {
            assert_eq!(fixture.generate(), fixture.generate());
            assert!(fixture.provenance.generated_not_captured);
            assert!(!fixture.expected_properties.is_empty());
        }

        let traceability = TraceabilityMatrix::bundled().expect("valid traceability");
        let stages: Vec<_> = traceability
            .stages
            .iter()
            .map(|entry| entry.stage)
            .collect();
        assert_eq!(stages, (1..=20).collect::<Vec<_>>());
        assert!(traceability.acceptance_invariants.len() >= 30);
        assert!(
            traceability
                .stages
                .iter()
                .all(|entry| !entry.planned_test.is_empty())
        );

        let first = request();
        let second = request();
        assert_eq!(
            first.canonical_bytes().unwrap(),
            second.canonical_bytes().unwrap()
        );
        assert_eq!(first.cache_key().unwrap(), second.cache_key().unwrap());
        let key = first.cache_key().unwrap();
        let report = CompilationReport {
            header: CompilationReportHeader {
                contract_version: 1,
                request_key: key,
                compiler: AlgorithmProvenance {
                    algorithm_id: String::from("hot-trimmer"),
                    version: String::from("0.1.0"),
                },
                seed: 7,
                revision: 3,
            },
            stages: BTreeMap::from([(
                1,
                StageResult::FailedWithRecovery {
                    reason: CompilationDiagnostic::unsupported_stage(1),
                    recovery_choices: vec![RecoveryChoice::ChooseAnotherSource],
                },
            )]),
            diagnostics: vec![CompilationDiagnostic::unsupported_stage(1)],
        };
        assert_eq!(
            report.deterministic_bytes().unwrap(),
            report.deterministic_bytes().unwrap()
        );

        let cancellation = CancellationToken::new();
        let revisions = RevisionAuthority::new(3);
        let guard = PublicationGuard::new(3, cancellation.clone(), revisions.clone());
        cancellation.cancel();
        assert_eq!(
            guard.authorize_complete_publish().unwrap_err().code,
            DiagnosticCode::Cancelled
        );
        let guard = PublicationGuard::new(3, CancellationToken::new(), revisions.clone());
        revisions.supersede_with(4);
        assert_eq!(
            guard.authorize_complete_publish().unwrap_err().code,
            DiagnosticCode::RevisionSuperseded
        );
    }
}
