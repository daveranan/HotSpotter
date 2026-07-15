use serde::{Deserialize, Serialize};

use crate::DomainError;

pub const IPC_PROTOCOL_VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundationStatusRequest {
    pub protocol_version: u16,
}

impl FoundationStatusRequest {
    /// Validates that the caller uses the native core's protocol version.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::ProtocolMismatch`] when the versions differ.
    pub fn validate(self) -> Result<Self, DomainError> {
        if self.protocol_version == IPC_PROTOCOL_VERSION {
            Ok(self)
        } else {
            Err(DomainError::ProtocolMismatch {
                expected: IPC_PROTOCOL_VERSION,
                received: self.protocol_version,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FoundationStatusRequest, IPC_PROTOCOL_VERSION};

    #[test]
    fn rejects_unknown_protocol_versions() {
        let request = FoundationStatusRequest {
            protocol_version: IPC_PROTOCOL_VERSION + 1,
        };
        assert!(request.validate().is_err());
    }
}
