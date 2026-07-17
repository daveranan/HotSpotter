use hot_trimmer_domain::{
    AlgorithmProvenance, CacheKey, CancellationToken, CompilationDiagnostic, CompilationReport,
    CompilationReportHeader, CompilerRequestHeader, DiagnosticCode, RecoveryChoice, StageResult,
};
use thiserror::Error;

pub const COMPILER_FACADE_ALGORITHM_ID: &str = "hot-trimmer.algorithm-stack";
pub const COMPILER_FACADE_VERSION: &str = "0.0.0-skeleton";

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
