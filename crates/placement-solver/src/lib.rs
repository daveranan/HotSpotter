#![doc = "Stages 10-13 placement boundary. Candidate algorithms are not installed in Prompt 00."]

use hot_trimmer_domain::{CompilationDiagnostic, RecoveryChoice, StageResult};

mod candidates;

pub use candidates::*;

#[must_use]
pub fn unsupported(stage: u8) -> StageResult {
    StageResult::FailedWithRecovery {
        reason: CompilationDiagnostic::unsupported_stage(stage),
        recovery_choices: vec![RecoveryChoice::LowerTexelDensity],
    }
}
