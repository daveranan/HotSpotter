use hot_trimmer_domain::{
    AlgorithmProvenance, CacheKey, CancellationToken, CompilationDiagnostic, CompilationReport,
    CompilationReportHeader, CompilerRequestHeader, DiagnosticCode, RecoveryChoice, StageResult,
};
use thiserror::Error;

use crate::{IntermediateAtlasArtifact, IntermediateAtlasError, IntermediateAtlasRequest, compose_intermediate_atlas};

pub const COMPILER_FACADE_ALGORITHM_ID: &str = "hot-trimmer.algorithm-stack";
pub const COMPILER_FACADE_VERSION: &str = "14.1.0-intermediate";

/// The only facade allowed to produce an authoritative compiled sheet.
///
/// Prompt 00 deliberately installs no route. The old document renderer is not called from here.
#[derive(Clone, Copy, Debug, Default)]
pub struct AlgorithmCompiler;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoritativeCompilation {
    pub request_key: CacheKey,
    pub report: CompilationReport,
    /// Map payloads are added only after their owning stages exist.
    pub maps: Vec<NeverPublishedMap>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NeverPublishedMap;

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum CompilerFacadeError {
    #[error("request header could not be serialized deterministically: {0}")]
    InvalidRequest(String),
    #[error("compilation was cancelled before a complete artifact existed")]
    Cancelled { report: CompilationReport },
    #[error("algorithm stage {stage} is unsupported")]
    UnsupportedStage { stage: u8, report: CompilationReport },
    #[error(transparent)]
    Intermediate(#[from] IntermediateAtlasError),
    #[error("persisted Stage 1-14 pipeline failed: {0}")]
    Pipeline(String),
}

impl AlgorithmCompiler {
    #[must_use]
    pub const fn new() -> Self { Self }

    /// Refuses the first uninstalled stage. It cannot return pixels from a placeholder or legacy path.
    pub fn compile(
        &self,
        request: &CompilerRequestHeader,
        cancellation: &CancellationToken,
    ) -> Result<AuthoritativeCompilation, CompilerFacadeError> {
        let request_key = request
            .cache_key()
            .map_err(|error| CompilerFacadeError::InvalidRequest(error.to_string()))?;
        if cancellation.is_cancelled() {
            let diagnostic = CompilationDiagnostic {
                code: DiagnosticCode::Cancelled,
                stage: None,
                message: String::from("compilation was cancelled before stage execution"),
                context: Default::default(),
            };
            return Err(CompilerFacadeError::Cancelled {
                report: report(request, request_key, diagnostic, None),
            });
        }

        let diagnostic = CompilationDiagnostic::unsupported_stage(1);
        Err(CompilerFacadeError::UnsupportedStage {
            stage: 1,
            report: report(request, request_key, diagnostic, Some(1)),
        })
    }

    /// Executes the explicitly incomplete Stage 14 publication route on the sole compiler facade.
    /// The executable request carries the artifacts which the header-only Prompt 00 contract lacks.
    pub fn compile_intermediate_atlas(
        &self,
        request: &IntermediateAtlasRequest<'_>,
        cancellation: &CancellationToken,
        current_revision: impl Fn() -> u64,
    ) -> Result<IntermediateAtlasArtifact, CompilerFacadeError> {
        compose_intermediate_atlas(request, || cancellation.is_cancelled(), current_revision)
            .map_err(CompilerFacadeError::from)
    }
}

fn report(
    request: &CompilerRequestHeader,
    request_key: CacheKey,
    diagnostic: CompilationDiagnostic,
    stage: Option<u8>,
) -> CompilationReport {
    let mut stages = std::collections::BTreeMap::new();
    if let Some(stage) = stage {
        stages.insert(stage, StageResult::FailedWithRecovery {
            reason: diagnostic.clone(),
            recovery_choices: vec![RecoveryChoice::ChooseAnotherSource],
        });
    }
    CompilationReport {
        header: CompilationReportHeader {
            contract_version: request.contract_version,
            request_key,
            compiler: AlgorithmProvenance {
                algorithm_id: String::from(COMPILER_FACADE_ALGORITHM_ID),
                version: String::from(COMPILER_FACADE_VERSION),
            },
            seed: request.seed,
            revision: request.revision,
        },
        stages,
        diagnostics: vec![diagnostic],
    }
}
