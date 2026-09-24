//! Window creation, placement, and show/hide behaviour.

use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const MAIN_WINDOW: &str = "main";
pub const SETTINGS_WINDOW: &str = "settings";

const WINDOW_WIDTH: f64 = 360.0;
const WINDOW_HEIGHT: f64 = 480.0;
const SETTINGS_WIDTH: f64 = 380.0;
const SETTINGS_HEIGHT: f64 = 680.0;

/// A toggle arriving this soon after the panel hid itself on blur is the same
/// click that caused the blur (see `toggle`).
const BLUR_TOGGLE_WINDOW: Duration = Duration::from_millis(300);

/// When the panel last hid itself because it lost focus.
static LAST_BLUR_HIDE: Mutex<Option<Instant>> = Mutex::new(None);

fn last_blur_hide() -> std::sync::MutexGuard<'static, Option<Instant>> {
    LAST_BLUR_HIDE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Anchor the panel to the bottom-right corner of the monitor's work area
/// (the part not covered by the taskbar), whatever the taskbar's size or DPI.
fn position_near_tray(window: &WebviewWindow) {
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };
    let scale = monitor.scale_factor();
    let work_area = monitor.work_area();
    let size = work_area.size.to_logical::<f64>(scale);
    let origin = work_area.position.to_logical::<f64>(scale);

    const MARGIN: f64 = 12.0;
    let x = origin.x + size.width - WINDOW_WIDTH - MARGIN;
    let y = origin.y + size.height - WINDOW_HEIGHT - MARGIN;

    let _ = window.set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
}

fn show(window: &WebviewWindow) {
    position_near_tray(window);
    let _ = window.show();
    let _ = window.set_focus();
}

pub fn toggle(window: &WebviewWindow) {
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return;
    }
    // Clicking the tray icon while the panel is open first takes focus away,
    // which hides the panel, and only then delivers the click. Without this
    // check that click would immediately reopen it, so the tray icon could
    // never close the panel.
    let hid_just_now = last_blur_hide()
        .take()
        .is_some_and(|at| at.elapsed() < BLUR_TOGGLE_WINDOW);
    if !hid_just_now {
        show(window);
    }
}

/// Toggle the tray panel, if it exists.
pub fn toggle_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        toggle(&window);
    }
}

/// Show the tray panel unconditionally, e.g. when a second instance is launched.
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        show(&window);
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
            if handle.is_visible().unwrap_or(false) {
                *last_blur_hide() = Some(Instant::now());
                let _ = handle.hide();
            }
        }
    });
}
