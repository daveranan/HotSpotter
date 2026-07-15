mod commands;
mod diagnostics;
mod paths;

use paths::AppPaths;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Starts the native desktop runtime.
///
/// # Panics
///
/// Panics when Tauri cannot initialize or the operating system event loop fails.
pub fn run() {
    diagnostics::initialize();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let paths = AppPaths::resolve(app.handle()).map_err(std::io::Error::other)?;
            let home = app.path().home_dir().ok();
            tracing::info!(
                protocol = hot_trimmer_domain::IPC_PROTOCOL_VERSION,
                "native foundation ready"
            );
            tracing::debug!(
                app_data = %diagnostics::redact_path(&paths.app_data, home.as_deref()),
                "application directories initialized"
            );
            app.manage(paths);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![commands::foundation_status])
        .run(tauri::generate_context!())
        .expect("failed to run Hot Trimmer native application");
}
