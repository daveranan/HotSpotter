#![doc = "Stages 3-7 material analysis boundary. Algorithms are installed by their owning prompts."]

use hot_trimmer_domain::{CompilationDiagnostic, RecoveryChoice, StageResult};

mod delighting;

pub use delighting::*;

#[must_use]
pub fn unsupported(stage: u8) -> StageResult {
    StageResult::FailedWithRecovery {
        reason: CompilationDiagnostic::unsupported_stage(stage),
        recovery_choices: vec![RecoveryChoice::AdjustSettings],
    }
}
