//! Tauri commands — the entire frontend-facing API surface.

use crate::adapters::{self, http};
use crate::config;
use crate::models::{AppSettings, ProviderInfo, ProxyStatus, UsageSnapshot};
use crate::state::{self, AppState};
use crate::{tray, window};
use tauri::{AppHandle, Emitter};
use tauri_plugin_autostart::ManagerExt;

/// Pushed to every window after a save, so the panel picks up display
/// settings (language, used/left) without a reload.
pub const SETTINGS_UPDATED: &str = "settings-updated";

#[tauri::command]
pub async fn get_usage(state: tauri::State<'_, AppState>) -> Result<Vec<UsageSnapshot>, String> {
    Ok(state.cached())
}

#[tauri::command]
pub async fn refresh_usage(app: AppHandle) -> Result<Vec<UsageSnapshot>, String> {
    Ok(state::refresh(&app).await)
}

/// Refresh one provider only (the per-card refresh button).
#[tauri::command]
pub async fn refresh_provider(app: AppHandle, provider: String) -> Result<Vec<UsageSnapshot>, String> {
    state::refresh_provider(&app, &provider).await
}

/// The panel reports its content height; the window follows it.
#[tauri::command]
pub async fn fit_panel(app: AppHandle, height: f64) -> Result<(), String> {
    window::fit_panel(&app, height);
    Ok(())
}

/// `autostart` is reported from the OS, not from the settings file: the user
/// can disable the entry elsewhere (e.g. Task Manager's Startup tab), and the
/// checkbox should show what will actually happen at the next login.
#[tauri::command]
pub async fn get_settings(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<AppSettings, String> {
    let mut settings = state.settings();
    if let Ok(enabled) = app.autolaunch().is_enabled() {
        settings.autostart = enabled;
    }
    Ok(settings)
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
    // persisted and the UI stays consistent with reality. Compare against the
    // OS state rather than the stored flag, which can drift from it.
    let autolaunch = app.autolaunch();
    let currently_enabled = autolaunch
        .is_enabled()
        .unwrap_or_else(|_| state.settings().autostart);
    if settings.autostart != currently_enabled {
        let result = if settings.autostart {
            autolaunch.enable()
        } else {
            autolaunch.disable()
        };
        result.map_err(|e| format!("Failed to set launch-at-startup: {e}"))?;
    }

    config::save(&app, &settings).map_err(|e| e.to_string())?;
    let previous = state.settings();
    state.set_settings(settings.clone());
    let _ = app.emit(SETTINGS_UPDATED, &settings);

    // Wake the scheduler only when what or how to fetch changed; display-only
    // settings (language, used/left, alerts) shouldn't hit every API again.
    let fetch_changed = settings.refresh_interval_secs != previous.refresh_interval_secs
        || settings.enabled_providers != previous.enabled_providers
        || settings.proxy_mode != previous.proxy_mode
        || settings.proxy_url != previous.proxy_url;
    if fetch_changed {
        state.notify.notify_one();
    } else {
        tray::update_tooltip(&app, &state.cached(), &settings);
    }
    Ok(())
}

#[tauri::command]
pub async fn open_settings_window(app: AppHandle) -> Result<(), String> {
    window::open_settings(&app)
}
