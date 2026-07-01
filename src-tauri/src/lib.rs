mod adapters;
mod models;

use models::{AppSettings, UsageSnapshot};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use tauri_plugin_autostart::ManagerExt;
use tokio::sync::Notify;

const MAIN_WINDOW: &str = "main";
const SETTINGS_WINDOW: &str = "settings";
const WINDOW_WIDTH: f64 = 360.0;
const WINDOW_HEIGHT: f64 = 480.0;

struct AppState {
    cache: Mutex<Vec<UsageSnapshot>>,
    settings: Mutex<AppSettings>,
    notify: Notify,
}

fn filter_enabled(snapshots: Vec<UsageSnapshot>, enabled: &[String]) -> Vec<UsageSnapshot> {
    snapshots
        .into_iter()
        .filter(|s| enabled.iter().any(|e| e == &s.provider))
        .collect()
}

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("settings.json"))
}

fn load_settings(app: &AppHandle) -> AppSettings {
    settings_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn persist_settings(app: &AppHandle, settings: &AppSettings) -> std::io::Result<()> {
    let Some(path) = settings_path(app) else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(settings).unwrap_or_default())
}

#[tauri::command]
async fn get_usage(state: tauri::State<'_, AppState>) -> Result<Vec<UsageSnapshot>, String> {
    Ok(state.cache.lock().unwrap().clone())
}

#[tauri::command]
async fn refresh_usage(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<UsageSnapshot>, String> {
    let enabled = state.settings.lock().unwrap().enabled_providers.clone();
    let fresh = filter_enabled(adapters::fetch_all().await, &enabled);
    *state.cache.lock().unwrap() = fresh.clone();
    let _ = app.emit("usage-updated", &fresh);
    Ok(fresh)
}

#[tauri::command]
async fn get_settings(state: tauri::State<'_, AppState>) -> Result<AppSettings, String> {
    Ok(state.settings.lock().unwrap().clone())
}

#[tauri::command]
async fn save_settings(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    settings: AppSettings,
) -> Result<(), String> {
    let previous_autostart = state.settings.lock().unwrap().autostart;
    if settings.autostart != previous_autostart {
        let autolaunch = app.autolaunch();
        let result = if settings.autostart {
            autolaunch.enable()
        } else {
            autolaunch.disable()
        };
        if let Err(e) = result {
            return Err(format!("设置开机自启失败: {e}"));
        }
    }

    persist_settings(&app, &settings).map_err(|e| e.to_string())?;
    *state.settings.lock().unwrap() = settings;
    state.notify.notify_one();
    Ok(())
}

#[tauri::command]
async fn open_settings_window(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(SETTINGS_WINDOW) {
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(&app, SETTINGS_WINDOW, WebviewUrl::App("settings.html".into()))
        .title("设置")
        .inner_size(340.0, 480.0)
        .resizable(false)
        .minimizable(false)
        .center()
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn position_window_near_tray(window: &WebviewWindow) {
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };
    let scale = monitor.scale_factor();
    let monitor_size = monitor.size().to_logical::<f64>(scale);
    let monitor_pos = monitor.position().to_logical::<f64>(scale);

    let margin = 12.0;
    let taskbar_allowance = 48.0;
    let x = monitor_pos.x + monitor_size.width - WINDOW_WIDTH - margin;
    let y = monitor_pos.y + monitor_size.height - WINDOW_HEIGHT - taskbar_allowance;

    let _ = window.set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
}

fn toggle_window(window: &WebviewWindow) {
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
    } else {
        position_window_near_tray(window);
        let _ = window.show();
        let _ = window.set_focus();
    }
}

async fn refresh_and_broadcast(app: &AppHandle) {
    let enabled = app
        .try_state::<AppState>()
        .map(|s| s.settings.lock().unwrap().enabled_providers.clone())
        .unwrap_or_default();
    let fresh = filter_enabled(adapters::fetch_all().await, &enabled);
    if let Some(state) = app.try_state::<AppState>() {
        *state.cache.lock().unwrap() = fresh.clone();
    }
    let _ = app.emit("usage-updated", &fresh);
}

fn spawn_scheduler(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            refresh_and_broadcast(&app).await;

            let interval_secs = app
                .try_state::<AppState>()
                .map(|s| s.settings.lock().unwrap().refresh_interval_secs)
                .unwrap_or(180);
            let sleep = tokio::time::sleep(std::time::Duration::from_secs(interval_secs));
            let woken = async {
                if let Some(state) = app.try_state::<AppState>() {
                    state.notify.notified().await;
                }
            };
            tokio::select! {
                _ = sleep => {},
                _ = woken => {},
            }
        }
    });
}

/// Headless diagnostic entry point: fetches every provider once and prints the
/// resulting snapshots as JSON, without starting the GUI/tray. Useful for
/// verifying adapter behaviour against real local credentials.
pub fn dump_usage() {
    let rt = tokio::runtime::Runtime::new().expect("failed to start tokio runtime");
    let snapshots = rt.block_on(adapters::fetch_all());
    println!("{}", serde_json::to_string_pretty(&snapshots).unwrap());
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            get_usage,
            refresh_usage,
            get_settings,
            save_settings,
            open_settings_window
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let settings = load_settings(&handle);

            app.manage(AppState {
                cache: Mutex::new(Vec::new()),
                settings: Mutex::new(settings),
                notify: Notify::new(),
            });

            let force_show = std::env::var("USAGESHOW_FORCE_SHOW").is_ok();
            if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
                if force_show {
                    position_window_near_tray(&window);
                    let _ = window.show();
                } else {
                    let _ = window.hide();
                }
                let w = window.clone();
                window.on_window_event(move |event| {
                    if force_show {
                        return;
                    }
                    if let tauri::WindowEvent::Focused(false) = event {
                        let _ = w.hide();
                    }
                });
            }

            let show_item = MenuItemBuilder::with_id("show", "打开面板").build(app)?;
            let settings_item = MenuItemBuilder::with_id("settings", "设置").build(app)?;
            let refresh_item = MenuItemBuilder::with_id("refresh", "立即刷新").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "退出").build(app)?;
            let menu = MenuBuilder::new(app)
                .items(&[&show_item, &settings_item, &refresh_item, &quit_item])
                .build()?;

            let tray_handle = handle.clone();
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("AI 用量监控")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
                            toggle_window(&window);
                        }
                    }
                    "settings" => {
                        let app_handle = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = open_settings_window(app_handle).await;
                        });
                    }
                    "refresh" => {
                        let app_handle = app.clone();
                        tauri::async_runtime::spawn(async move {
                            refresh_and_broadcast(&app_handle).await;
                        });
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(move |_tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        if let Some(window) = tray_handle.get_webview_window(MAIN_WINDOW) {
                            toggle_window(&window);
                        }
                    }
                })
                .build(app)?;

            spawn_scheduler(handle);

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
