//! System tray icon, its menu, its tooltip, and the global toggle shortcut.

use crate::locale::{self, tr};
use crate::models::{AppSettings, UsageSnapshot};
use crate::quota;
use crate::state::{self, AppState};
use crate::window;
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Manager,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

pub const TOGGLE_SHORTCUT: &str = "alt+c";
const TRAY_ID: &str = "main-tray";

fn title(settings: &AppSettings) -> &'static str {
    tr(locale::use_chinese(settings), "AI Usage Monitor", "AI 用量监控")
}

pub fn setup(app: &App) -> tauri::Result<()> {
    let settings = app
        .try_state::<AppState>()
        .map(|s| s.settings())
        .unwrap_or_default();
    let zh = locale::use_chinese(&settings);

    let show = MenuItemBuilder::with_id("show", tr(zh, "Open Panel", "打开面板"))
        .accelerator(TOGGLE_SHORTCUT)
        .build(app)?;
    let settings_item = MenuItemBuilder::with_id("settings", tr(zh, "Settings", "设置")).build(app)?;
    let refresh = MenuItemBuilder::with_id("refresh", tr(zh, "Refresh Now", "立即刷新")).build(app)?;
    let quit = MenuItemBuilder::with_id("quit", tr(zh, "Quit", "退出")).build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&show, &settings_item, &refresh, &quit])
        .build()?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip(title(&settings))
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

/// Hovering the tray icon shows a one-line-per-provider usage summary.
pub fn update_tooltip(app: &AppHandle, snapshots: &[UsageSnapshot], settings: &AppSettings) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let text = quota::tooltip_summary(title(settings), snapshots, settings.show_remaining);
        let _ = tray.set_tooltip(Some(text));
    }
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
