use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidCoordinate,
    ProtocolMismatch,
    InvalidInput,
    ProjectLocked,
    ProjectInvalid,
    ImageImportFailed,
    NoOpenProject,
    DirtyProject,
    RecoveryFailed,
    SourceRegistrationFailed,
    OperationCancelled,
    Internal,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserFacingError {
    pub code: ErrorCode,
    pub message: String,
    pub recovery: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("normalized coordinate must be finite and between zero and one; received {value}")]
    InvalidNormalizedCoordinate { value: f64 },
    #[error("IPC protocol {received} is not supported; expected {expected}")]
    ProtocolMismatch { expected: u16, received: u16 },
}

impl From<DomainError> for UserFacingError {
    fn from(error: DomainError) -> Self {
        match error {
            DomainError::InvalidNormalizedCoordinate { value } => Self {
                code: ErrorCode::InvalidCoordinate,
                message: "A coordinate is outside the editable image area.".into(),
                recovery: "Move the point inside the source image and retry.".into(),
                detail: Some(format!("coordinate={value}")),
            },
            DomainError::ProtocolMismatch { expected, received } => Self {
                code: ErrorCode::ProtocolMismatch,
                message: "The desktop interface and native core are incompatible.".into(),
                recovery: "Restart Hot Trimmer. Reinstall it if the problem continues.".into(),
                detail: Some(format!("expected={expected}, received={received}")),
            },
        }
    }
}
