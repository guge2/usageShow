//! Background refresh loop.

use crate::state::{self, AppState};
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// Guard against a settings file that asks for an unreasonably tight loop.
const MIN_INTERVAL_SECS: u64 = 30;
const FALLBACK_INTERVAL_SECS: u64 = 180;

pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            state::refresh(&app).await;

            let interval = app
                .try_state::<AppState>()
                .map(|s| s.settings().refresh_interval_secs)
                .unwrap_or(FALLBACK_INTERVAL_SECS)
                .max(MIN_INTERVAL_SECS);

            // Wake early when settings change, so a new interval or a newly
            // enabled provider takes effect without waiting out the old sleep.
            let sleep = tokio::time::sleep(Duration::from_secs(interval));
            let settings_changed = async {
                if let Some(state) = app.try_state::<AppState>() {
                    state.notify.notified().await;
                }
            };
            tokio::select! {
                _ = sleep => {}
                _ = settings_changed => {}
            }
        }
    });
}
