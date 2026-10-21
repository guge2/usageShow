//! Shared application state and the single refresh path through it.

use crate::adapters::{self, http, FetchCtx};
use crate::models::{AppSettings, UsageSnapshot};
use crate::quota::{self, AlertMarks};
use crate::{config, locale, tray};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::Notify;

/// Event name the frontend listens on for pushed updates.
pub const USAGE_UPDATED: &str = "usage-updated";

/// A full refresh requested this soon after another one finished returns the
/// cache instead of hitting every provider again (e.g. the scheduler's first
/// round and a click on Refresh racing at startup; Anthropic rate-limits hard).
const REFRESH_DEDUP_SECS: i64 = 5;

pub struct AppState {
    cache: Mutex<Vec<UsageSnapshot>>,
    settings: Mutex<AppSettings>,
    alert_marks: Mutex<AlertMarks>,
    /// Serialises full refreshes; holds when the last one finished.
    refresh_gate: tokio::sync::Mutex<i64>,
    /// Woken when settings change, so the scheduler re-reads the interval
    /// instead of finishing a long sleep under the old one.
    pub notify: Notify,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // Lock poisoning is recovered from rather than propagated: a panic
    // elsewhere should not permanently break refreshes.
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl AppState {
    pub fn new(settings: AppSettings, alert_marks: AlertMarks) -> Self {
        Self {
            cache: Mutex::new(Vec::new()),
            settings: Mutex::new(settings),
            alert_marks: Mutex::new(alert_marks),
            refresh_gate: tokio::sync::Mutex::new(0),
            notify: Notify::new(),
        }
    }

    pub fn settings(&self) -> AppSettings {
        lock(&self.settings).clone()
    }

    pub fn set_settings(&self, settings: AppSettings) {
        *lock(&self.settings) = settings;
    }

    pub fn cached(&self) -> Vec<UsageSnapshot> {
        lock(&self.cache).clone()
    }

    fn set_cache(&self, snapshots: Vec<UsageSnapshot>) {
        *lock(&self.cache) = snapshots;
    }
}

fn fetch_ctx(settings: &AppSettings) -> FetchCtx {
    let proxy = http::resolve_proxy(&settings.proxy_mode, &settings.proxy_url);
    FetchCtx {
        client: http::client_for(&proxy),
    }
}

/// The one place a full refresh happens: fetch the enabled providers with the
/// currently configured proxy, cache the result, and push it to any open
/// window. Callers arriving while one is in flight wait for it and share its
/// result instead of starting another round of requests.
pub async fn refresh(app: &AppHandle) -> Vec<UsageSnapshot> {
    let Some(state) = app.try_state::<AppState>() else {
        return Vec::new();
    };
    let mut last_done = state.refresh_gate.lock().await;
    if crate::models::now_unix() - *last_done < REFRESH_DEDUP_SECS {
        return state.cached();
    }

    let settings = state.settings();
    let previous = state.cached();
    let fresh: Vec<UsageSnapshot> =
        adapters::fetch_enabled(&settings.enabled_providers, fetch_ctx(&settings))
            .await
            .into_iter()
            .map(|snapshot| {
                let last = previous.iter().find(|p| p.provider == snapshot.provider);
                snapshot.or_previous(last)
            })
            .collect();

    publish(app, &state, fresh.clone(), &settings);
    *last_done = crate::models::now_unix();
    fresh
}

/// Refresh a single provider (the per-card refresh button) and merge it into
/// the cache, leaving the others untouched.
pub async fn refresh_provider(app: &AppHandle, id: &str) -> Result<Vec<UsageSnapshot>, String> {
    let Some(state) = app.try_state::<AppState>() else {
        return Ok(Vec::new());
    };
    let settings = state.settings();
    let snapshot = adapters::fetch_one(id, fetch_ctx(&settings))
        .await
        .ok_or_else(|| format!("Unknown provider: {id}"))?;

    let mut list = state.cached();
    let merged = snapshot.or_previous(list.iter().find(|s| s.provider == id));
    match list.iter_mut().find(|s| s.provider == id) {
        Some(slot) => *slot = merged,
        None => list.push(merged),
    }
    // Keep registry order, in case the provider was not in the cache yet.
    let order = adapters::provider_ids();
    list.sort_by_key(|s| order.iter().position(|p| *p == s.provider));

    publish(app, &state, list.clone(), &settings);
    Ok(list)
}

/// Cache a new set of snapshots, then alert, update the tray tooltip and
/// push it to the windows.
fn publish(app: &AppHandle, state: &AppState, snapshots: Vec<UsageSnapshot>, settings: &AppSettings) {
    state.set_cache(snapshots.clone());
    send_alerts(app, state, &snapshots, settings);
    tray::update_tooltip(app, &snapshots, settings);
    let _ = app.emit(USAGE_UPDATED, &snapshots);
}

fn send_alerts(app: &AppHandle, state: &AppState, snapshots: &[UsageSnapshot], settings: &AppSettings) {
    let (alerts, marks) = {
        let mut marks = lock(&state.alert_marks);
        let alerts = quota::due_alerts(snapshots, &mut marks, settings.alert_threshold);
        (alerts, marks.clone())
    };
    if alerts.is_empty() {
        return;
    }
    // Remembered across restarts, so relaunching doesn't repeat an alert.
    let _ = config::save_alert_marks(app, &marks);

    let zh = locale::use_chinese(settings);
    for alert in alerts {
        let reset = alert
            .reset_at
            .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
            .map(|dt| {
                dt.with_timezone(&chrono::Local)
                    .format("%m-%d %H:%M")
                    .to_string()
            });
        let title = if zh {
            format!("{} {} 已用 {:.0}%", alert.display_name, alert.label, alert.percent)
        } else {
            format!("{} {} at {:.0}%", alert.display_name, alert.label, alert.percent)
        };
        let body = match (zh, reset) {
            (true, Some(r)) => format!("额度将于 {r} 重置"),
            (false, Some(r)) => format!("Resets {r}"),
            (true, None) => "额度即将用尽".to_string(),
            (false, None) => "Almost out of quota".to_string(),
        };
        let _ = app.notification().builder().title(title).body(body).show();
    }
}
