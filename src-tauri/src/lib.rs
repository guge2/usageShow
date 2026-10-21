pub mod adapters;
mod commands;
mod config;
mod locale;
pub mod models;
mod quota;
mod scheduler;
mod state;
mod tray;
mod window;

use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be registered first. A second launch (e.g. autostart plus a
        // manual start) would otherwise add another tray icon and a second
        // polling loop, doubling requests against rate-limited APIs.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            window::show_main(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            commands::get_usage,
            commands::refresh_usage,
            commands::refresh_provider,
            commands::get_settings,
            commands::save_settings,
            commands::open_settings_window,
            commands::list_providers,
            commands::get_proxy_status,
            commands::fit_panel,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            app.manage(AppState::new(
                config::load(&handle),
                config::load_alert_marks(&handle),
            ));

            window::setup_main(&handle);
            tray::setup(app)?;
            scheduler::spawn(handle);

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
