mod commands;
mod diagnostics;
mod paths;

use std::sync::{Arc, Mutex};

use commands::{
    PendingProjectPath, ProjectSession, SharedImportJob, SharedPatchPreviewJob,
    SharedProjectSession, StartupState,
};
use paths::AppPaths;
use tauri::{
    Emitter, Manager,
    menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder},
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Starts the native desktop runtime.
///
/// # Panics
///
/// Panics when Tauri cannot initialize or the operating system event loop fails.
pub fn run() {
    diagnostics::initialize();

    let pending: PendingProjectPath = Arc::new(Mutex::new(None));
    let app = tauri::Builder::default()
        .manage(Arc::clone(&pending) as PendingProjectPath)
        .plugin(tauri_plugin_single_instance::init(
            |app, arguments, _working_directory| {
                if let Some(path) = project_argument(&arguments) {
                    if let Ok(mut pending) = app.state::<PendingProjectPath>().lock() {
                        *pending = Some(path.clone());
                    }
                    let _ = app.emit("open-project-requested", path);
                }
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            },
        ))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .on_menu_event(|app, event| {
            let _ = app.emit("menu-action", event.id().as_ref());
        })
        .setup(|app| {
            let paths = AppPaths::resolve(app.handle()).map_err(std::io::Error::other)?;
            let previous_unclean = paths.begin_session().map_err(std::io::Error::other)?;
            let home = app.path().home_dir().ok();
            tracing::info!(
                protocol = hot_trimmer_domain::IPC_PROTOCOL_VERSION,
                "native project shell ready"
            );
            tracing::debug!(
                app_data = %diagnostics::redact_path(&paths.app_data, home.as_deref()),
                "application directories initialized"
            );
            app.manage(Arc::new(Mutex::new(ProjectSession::new(&paths))) as SharedProjectSession);
            app.manage(Arc::new(Mutex::new(None)) as SharedImportJob);
            app.manage(Arc::new(Mutex::new(None)) as SharedPatchPreviewJob);
            app.manage(StartupState {
                previous_shutdown_clean: !previous_unclean,
            });
            app.manage(paths);
            install_native_menu(app)?;
            if let Some(path) = project_argument(&std::env::args().collect::<Vec<_>>())
                && let Ok(mut pending) = app.state::<PendingProjectPath>().lock()
            {
                *pending = Some(path);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::foundation_status,
            commands::startup_status,
            commands::create_project,
            commands::open_project,
            commands::import_source,
            commands::cancel_import,
            commands::remove_source,
            commands::rename_project,
            commands::apply_patch_command,
            commands::undo_patch_command,
            commands::redo_patch_command,
            commands::fit_patch_polygon,
            commands::generate_patch_preview,
            commands::generate_draft_patch_preview,
            commands::cancel_patch_preview,
            commands::save_project,
            commands::save_project_as,
            commands::close_project,
            commands::list_recent_projects,
            commands::list_recovery_candidates,
            commands::recover_project,
            commands::take_pending_project_path
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Hot Trimmer native application");
    app.run(|handle, event| {
        if matches!(event, tauri::RunEvent::Exit)
            && let Some(paths) = handle.try_state::<AppPaths>()
        {
            paths.end_session();
        }
    });
}

fn install_native_menu(app: &mut tauri::App) -> tauri::Result<()> {
    let new_project = MenuItemBuilder::with_id("new_project", "&New Project…")
        .accelerator("Ctrl+N")
        .build(app)?;
    let open_project = MenuItemBuilder::with_id("open_project", "&Open Project…")
        .accelerator("Ctrl+O")
        .build(app)?;
    let open_image = MenuItemBuilder::with_id("open_image", "Open &Image…")
        .accelerator("Ctrl+I")
        .build(app)?;
    let save_project = MenuItemBuilder::with_id("save_project", "&Save")
        .accelerator("Ctrl+S")
        .build(app)?;
    let save_project_as = MenuItemBuilder::with_id("save_project_as", "Save &As…")
        .accelerator("Ctrl+Shift+S")
        .build(app)?;
    let close_project = MenuItemBuilder::with_id("close_project", "&Close Project")
        .accelerator("Ctrl+W")
        .build(app)?;
    let reveal_project =
        MenuItemBuilder::with_id("reveal_project", "Reveal Project in Folder").build(app)?;
    let recovery = MenuItemBuilder::with_id("show_recovery", "Recovery…").build(app)?;
    let file = SubmenuBuilder::new(app, "&File")
        .item(&new_project)
        .item(&open_project)
        .item(&open_image)
        .separator()
        .item(&save_project)
        .item(&save_project_as)
        .item(&close_project)
        .separator()
        .item(&reveal_project)
        .item(&recovery)
        .separator()
        .quit()
        .build()?;
    let menu = MenuBuilder::new(app).item(&file).build()?;
    app.set_menu(menu)?;
    Ok(())
}

fn project_argument(arguments: &[String]) -> Option<String> {
    arguments
        .iter()
        .find(|argument| argument.to_ascii_lowercase().ends_with(".hottrimmer"))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::project_argument;

    #[test]
    fn routes_only_project_file_arguments() {
        let arguments = vec![
            "hot-trimmer.exe".to_owned(),
            "--flag".to_owned(),
            "D:/Art/Brick.hottrimmer".to_owned(),
        ];
        assert_eq!(
            project_argument(&arguments).as_deref(),
            Some("D:/Art/Brick.hottrimmer")
        );
    }
}
