//! Shared application state and the single refresh path through it.

use crate::adapters::{self, http, FetchCtx};
use crate::models::{AppSettings, UsageSnapshot};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Notify;

/// Event name the frontend listens on for pushed updates.
pub const USAGE_UPDATED: &str = "usage-updated";

pub struct AppState {
    cache: Mutex<Vec<UsageSnapshot>>,
    settings: Mutex<AppSettings>,
    /// Woken when settings change, so the scheduler re-reads the interval
    /// instead of finishing a long sleep under the old one.
    pub notify: Notify,
}

impl AppState {
    pub fn new(settings: AppSettings) -> Self {
        Self {
            cache: Mutex::new(Vec::new()),
            settings: Mutex::new(settings),
            notify: Notify::new(),
        }
    }

    /// Read the settings. Lock poisoning is recovered from rather than
    /// propagated: a panic elsewhere should not permanently break refreshes.
    pub fn settings(&self) -> AppSettings {
        match self.settings.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    pub fn set_settings(&self, settings: AppSettings) {
        match self.settings.lock() {
            Ok(mut guard) => *guard = settings,
            Err(poisoned) => *poisoned.into_inner() = settings,
        }
    }

    pub fn cached(&self) -> Vec<UsageSnapshot> {
        match self.cache.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    fn set_cache(&self, snapshots: Vec<UsageSnapshot>) {
        match self.cache.lock() {
            Ok(mut guard) => *guard = snapshots,
            Err(poisoned) => *poisoned.into_inner() = snapshots,
        }
    }
}

/// The one place a refresh happens: fetch the enabled providers with the
/// currently configured proxy, cache the result, and push it to any open window.
pub async fn refresh(app: &AppHandle) -> Vec<UsageSnapshot> {
    let Some(state) = app.try_state::<AppState>() else {
        return Vec::new();
    };
    let settings = state.settings();

    let proxy = http::resolve_proxy(&settings.proxy_mode, &settings.proxy_url);
    let ctx = FetchCtx {
        client: http::client_for(&proxy),
    };

    let previous = state.cached();
    let fresh: Vec<UsageSnapshot> = adapters::fetch_enabled(&settings.enabled_providers, ctx)
        .await
        .into_iter()
        .map(|snapshot| {
            let last = previous.iter().find(|p| p.provider == snapshot.provider);
            snapshot.or_previous(last)
        })
        .collect();

    state.set_cache(fresh.clone());
    let _ = app.emit(USAGE_UPDATED, &fresh);
    fresh
}
