#![doc = "Stages 15, 16, and 18 effect compilation boundary."]

use hot_trimmer_domain::{CompilationDiagnostic, RecoveryChoice, StageResult};

#[must_use]
pub fn unsupported(stage: u8) -> StageResult {
    StageResult::FailedWithRecovery {
        reason: CompilationDiagnostic::unsupported_stage(stage),
        recovery_choices: vec![RecoveryChoice::DisableEffect],
    }
}
