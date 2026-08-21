//! Tauri commands — the entire frontend-facing API surface.

use crate::adapters::{self, http};
use crate::config;
use crate::models::{AppSettings, ProviderInfo, ProxyStatus, UsageSnapshot};
use crate::state::{self, AppState};
use crate::window;
use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

#[tauri::command]
pub async fn get_usage(state: tauri::State<'_, AppState>) -> Result<Vec<UsageSnapshot>, String> {
    Ok(state.cached())
}

#[tauri::command]
pub async fn refresh_usage(app: AppHandle) -> Result<Vec<UsageSnapshot>, String> {
    Ok(state::refresh(&app).await)
}

#[tauri::command]
pub async fn get_settings(state: tauri::State<'_, AppState>) -> Result<AppSettings, String> {
    Ok(state.settings())
}

/// The provider list, so the frontend does not maintain its own copy.
#[tauri::command]
pub async fn list_providers() -> Result<Vec<ProviderInfo>, String> {
    Ok(adapters::provider_infos())
}

/// What the proxy setting actually resolved to, shown in Settings so a user can
/// confirm their proxy was detected without digging through logs.
#[tauri::command]
pub async fn get_proxy_status(state: tauri::State<'_, AppState>) -> Result<ProxyStatus, String> {
    let settings = state.settings();
    let resolved = http::resolve_proxy(&settings.proxy_mode, &settings.proxy_url);
    Ok(ProxyStatus {
        description: resolved.describe(),
        active: resolved.url().is_some(),
    })
}

#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    settings: AppSettings,
) -> Result<(), String> {
    // Apply the autostart side effect first: if the OS refuses, nothing is
    // persisted and the UI stays consistent with reality.
    if settings.autostart != state.settings().autostart {
        let autolaunch = app.autolaunch();
        let result = if settings.autostart {
            autolaunch.enable()
        } else {
            autolaunch.disable()
        };
        result.map_err(|e| format!("Failed to set launch-at-startup: {e}"))?;
    }

    config::save(&app, &settings).map_err(|e| e.to_string())?;
    state.set_settings(settings);
    // Wake the scheduler so a changed interval or provider set takes effect now.
    state.notify.notify_one();
    Ok(())
}

#[tauri::command]
pub async fn open_settings_window(app: AppHandle) -> Result<(), String> {
    window::open_settings(&app)
}
