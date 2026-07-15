use std::{fs, path::PathBuf};

use tauri::{AppHandle, Manager};

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub app_data: PathBuf,
    pub cache: PathBuf,
    pub logs: PathBuf,
    pub recovery: PathBuf,
}

impl AppPaths {
    pub fn resolve(app: &AppHandle) -> Result<Self, String> {
        let app_data = app
            .path()
            .app_data_dir()
            .map_err(|error| error.to_string())?;
        let cache = app
            .path()
            .app_cache_dir()
            .map_err(|error| error.to_string())?;
        let logs = app
            .path()
            .app_log_dir()
            .map_err(|error| error.to_string())?;
        let recovery = app_data.join("recovery");

        for directory in [&app_data, &cache, &logs, &recovery] {
            fs::create_dir_all(directory)
                .map_err(|error| format!("failed to initialize application directory: {error}"))?;
        }

        Ok(Self {
            app_data,
            cache,
            logs,
            recovery,
        })
    }
}
