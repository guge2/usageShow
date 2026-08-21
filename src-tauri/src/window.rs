//! Window creation, placement, and show/hide behaviour.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const MAIN_WINDOW: &str = "main";
pub const SETTINGS_WINDOW: &str = "settings";

const WINDOW_WIDTH: f64 = 360.0;
const WINDOW_HEIGHT: f64 = 480.0;
const SETTINGS_WIDTH: f64 = 380.0;
const SETTINGS_HEIGHT: f64 = 680.0;

/// Anchor the panel to the bottom-right, above the taskbar, on whichever
/// monitor it currently belongs to.
fn position_near_tray(window: &WebviewWindow) {
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };
    let scale = monitor.scale_factor();
    let size = monitor.size().to_logical::<f64>(scale);
    let origin = monitor.position().to_logical::<f64>(scale);

    const MARGIN: f64 = 12.0;
    const TASKBAR_ALLOWANCE: f64 = 48.0;
    let x = origin.x + size.width - WINDOW_WIDTH - MARGIN;
    let y = origin.y + size.height - WINDOW_HEIGHT - TASKBAR_ALLOWANCE;

    let _ = window.set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
}

pub fn toggle(window: &WebviewWindow) {
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
    } else {
        position_near_tray(window);
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Toggle the tray panel, if it exists.
pub fn toggle_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        toggle(&window);
    }
}

pub fn open_settings(app: &AppHandle) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window(SETTINGS_WINDOW) {
        let _ = existing.show();
        let _ = existing.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(
        app,
        SETTINGS_WINDOW,
        WebviewUrl::App("settings.html".into()),
    )
    .title("Settings")
    .inner_size(SETTINGS_WIDTH, SETTINGS_HEIGHT)
    .resizable(false)
    .minimizable(false)
    .center()
    .build()
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// Start hidden, and hide again whenever focus is lost.
pub fn setup_main(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    let _ = window.hide();
    let handle = window.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::Focused(false) = event {
            let _ = handle.hide();
        }
    });
}
