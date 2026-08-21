//! System tray icon, its menu, and the global toggle shortcut.

use crate::state;
use crate::window;
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

pub const TOGGLE_SHORTCUT: &str = "alt+c";

pub fn setup(app: &App) -> tauri::Result<()> {
    let show = MenuItemBuilder::with_id("show", "Open Panel")
        .accelerator(TOGGLE_SHORTCUT)
        .build(app)?;
    let settings = MenuItemBuilder::with_id("settings", "Settings").build(app)?;
    let refresh = MenuItemBuilder::with_id("refresh", "Refresh Now").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&show, &settings, &refresh, &quit])
        .build()?;

    TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("AI Usage Monitor")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu_event)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                window::toggle_main(tray.app_handle());
            }
        })
        .build(app)?;

    register_shortcut(app.handle())?;
    Ok(())
}

fn on_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    match event.id.as_ref() {
        "show" => window::toggle_main(app),
        "settings" => {
            let _ = window::open_settings(app);
        }
        "refresh" => {
            let handle = app.clone();
            tauri::async_runtime::spawn(async move {
                state::refresh(&handle).await;
            });
        }
        "quit" => app.exit(0),
        _ => {}
    }
}

/// A shortcut already claimed by another app is not fatal — the tray icon
/// still works, so we only skip the binding.
fn register_shortcut(app: &AppHandle) -> tauri::Result<()> {
    let handle = app.clone();
    let result = app
        .global_shortcut()
        .on_shortcut(TOGGLE_SHORTCUT, move |_app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                window::toggle_main(&handle);
            }
        });
    if let Err(e) = result {
        eprintln!("Could not register {TOGGLE_SHORTCUT}: {e}");
    }
    Ok(())
}
