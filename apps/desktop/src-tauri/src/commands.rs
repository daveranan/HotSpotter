use hot_trimmer_domain::{FoundationStatusRequest, IPC_PROTOCOL_VERSION, UserFacingError};
use serde::Serialize;
use tauri::State;

use crate::paths::AppPaths;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeDirectories {
    app_data: String,
    cache: String,
    logs: String,
    recovery: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundationStatus {
    protocol_version: u16,
    app_version: &'static str,
    platform: &'static str,
    directories: NativeDirectories,
    capabilities: [&'static str; 4],
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri command state extraction requires an owned State wrapper.
pub fn foundation_status(
    request: FoundationStatusRequest,
    paths: State<'_, AppPaths>,
) -> Result<FoundationStatus, UserFacingError> {
    request.validate().map_err(UserFacingError::from)?;

    Ok(FoundationStatus {
        protocol_version: IPC_PROTOCOL_VERSION,
        app_version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
        directories: NativeDirectories {
            app_data: paths.app_data.display().to_string(),
            cache: paths.cache.display().to_string(),
            logs: paths.logs.display().to_string(),
            recovery: paths.recovery.display().to_string(),
        },
        capabilities: [
            "native_paths",
            "typed_ipc",
            "structured_diagnostics",
            "native_dialog",
        ],
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use hot_trimmer_domain::{FoundationStatusRequest, IPC_PROTOCOL_VERSION};
    use serde_json::{Value, json};

    use super::{FoundationStatus, NativeDirectories};

    #[test]
    fn rust_response_matches_the_cross_language_contract_fixture() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/contracts/foundation-status.json"
        ))
        .expect("valid contract fixture");

        let request = FoundationStatusRequest {
            protocol_version: IPC_PROTOCOL_VERSION,
        };
        assert_eq!(
            serde_json::to_value(request).expect("serializable request"),
            fixture["request"]
        );

        let response = FoundationStatus {
            protocol_version: IPC_PROTOCOL_VERSION,
            app_version: "0.1.0",
            platform: "windows",
            directories: NativeDirectories {
                app_data: PathBuf::from("<app-data>").display().to_string(),
                cache: PathBuf::from("<cache>").display().to_string(),
                logs: PathBuf::from("<logs>").display().to_string(),
                recovery: PathBuf::from("<recovery>").display().to_string(),
            },
            capabilities: [
                "native_paths",
                "typed_ipc",
                "structured_diagnostics",
                "native_dialog",
            ],
        };

        assert_eq!(
            serde_json::to_value(response).expect("serializable response"),
            fixture["response"]
        );
        assert_eq!(fixture["request"], json!({ "protocolVersion": 1 }));
    }
}
