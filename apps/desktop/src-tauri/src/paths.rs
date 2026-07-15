use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
};

use tauri::{AppHandle, Manager};

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub app_data: PathBuf,
    pub cache: PathBuf,
    pub logs: PathBuf,
    pub recovery: PathBuf,
    session_marker: PathBuf,
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
        let session_marker = app_data.join("running.session");

        for directory in [&app_data, &cache, &logs, &recovery] {
            fs::create_dir_all(directory)
                .map_err(|error| format!("failed to initialize application directory: {error}"))?;
        }

        Ok(Self {
            app_data,
            cache,
            logs,
            recovery,
            session_marker,
        })
    }

    pub fn begin_session(&self) -> Result<bool, String> {
        let previous_unclean = self.session_marker.exists();
        let mut marker = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&self.session_marker)
            .map_err(|error| error.to_string())?;
        marker
            .write_all(std::process::id().to_string().as_bytes())
            .map_err(|error| error.to_string())?;
        marker.sync_all().map_err(|error| error.to_string())?;
        Ok(previous_unclean)
    }

    pub fn end_session(&self) {
        let _ = fs::remove_file(&self.session_marker);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use uuid::Uuid;

    use super::AppPaths;

    #[test]
    fn session_marker_distinguishes_clean_and_unclean_shutdown() {
        let root = std::env::temp_dir().join(format!("hot-trimmer-session-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create fixture root");
        let paths = AppPaths {
            app_data: root.clone(),
            cache: root.join("cache"),
            logs: root.join("logs"),
            recovery: root.join("recovery"),
            session_marker: root.join("running.session"),
        };
        assert!(!paths.begin_session().expect("first clean startup"));
        assert!(paths.begin_session().expect("detect unclean startup"));
        paths.end_session();
        assert!(!paths.begin_session().expect("clean startup after shutdown"));
        paths.end_session();
        fs::remove_dir_all(root).expect("remove fixture root");
    }
}
