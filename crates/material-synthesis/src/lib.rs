#![doc = "Stage 8 and Stage 14 registered material-synthesis boundary."]

use hot_trimmer_domain::{CompilationDiagnostic, RecoveryChoice, StageResult};

mod material_domain;

pub use material_domain::*;

#[must_use]
pub fn unsupported(stage: u8) -> StageResult {
    StageResult::FailedWithRecovery {
        reason: CompilationDiagnostic::unsupported_stage(stage),
        recovery_choices: vec![RecoveryChoice::ChooseAnotherSource],
    }
}
