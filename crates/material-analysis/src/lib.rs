#![doc = "Stages 3-7 material analysis boundary. Algorithms are installed by their owning prompts."]

use hot_trimmer_domain::{CompilationDiagnostic, RecoveryChoice, StageResult};

mod delighting;
mod feature_fields;
mod quality_classification;
mod scale_orientation;

pub use delighting::*;
pub use feature_fields::*;
pub use quality_classification::*;
pub use scale_orientation::*;

#[must_use]
pub fn unsupported(stage: u8) -> StageResult {
    StageResult::FailedWithRecovery {
        reason: CompilationDiagnostic::unsupported_stage(stage),
        recovery_choices: vec![RecoveryChoice::AdjustSettings],
    }
}
