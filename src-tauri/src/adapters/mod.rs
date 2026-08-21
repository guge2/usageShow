//! Provider registry.
//!
//! Adding a provider means writing `adapters/<name>.rs` with a
//! `pub async fn fetch(ctx: FetchCtx) -> UsageSnapshot` and adding one line to
//! the `providers!` list below. Nothing else in the codebase — Rust or
//! TypeScript — needs to learn about it: the frontend asks the backend for the
//! provider list at runtime.

pub mod creds;
pub mod http;

pub mod agy;
pub mod amp;
pub mod claude;
pub mod codex;
pub mod cursor;
pub mod factory;
pub mod grok;

use crate::models::{ProviderInfo, UsageSnapshot};
use std::future::Future;
use std::pin::Pin;
/// How long to wait before re-attempting a provider that failed to connect.
const RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(400);

/// Everything an adapter needs from the outside world. Passing this in (rather
/// than letting adapters build their own client) is what makes the proxy
/// setting apply uniformly to every provider.
#[derive(Clone)]
pub struct FetchCtx {
    pub client: reqwest::Client,
}

type BoxedFetch = Pin<Box<dyn Future<Output = UsageSnapshot> + Send>>;

pub struct Provider {
    pub id: &'static str,
    pub display_name: &'static str,
    fetch: fn(FetchCtx) -> BoxedFetch,
}

macro_rules! providers {
    ($(($id:literal, $name:literal, $module:ident)),* $(,)?) => {
        pub static PROVIDERS: &[Provider] = &[
            $(Provider {
                id: $id,
                display_name: $name,
                fetch: |ctx| Box::pin($module::fetch(ctx)),
            }),*
        ];
    };
}

providers![
    ("claude", "Claude", claude),
    ("codex", "Codex", codex),
    ("cursor", "Cursor", cursor),
    ("amp", "Amp", amp),
    ("factory", "Factory Droid", factory),
    ("agy", "AGY", agy),
    ("grok", "Grok", grok),
];

/// Turn a `reqwest` error into something a user can act on.
///
/// A bare "Request failed: error sending request for url (...)" tells the user
/// nothing. On this app's most common failure — an API that is only reachable
/// through a proxy — the actionable part is the proxy setting.
pub fn describe_error(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "Timed out - the API may be unreachable from this network; check the proxy setting"
            .to_string()
    } else if error.is_connect() {
        "Could not connect - check your network and the proxy setting".to_string()
    } else if error.is_decode() {
        "Received an unreadable response".to_string()
    } else {
        format!("Request failed: {error}")
    }
}

/// Whether a failure looks like a transient connection problem worth one retry.
///
/// This matches on the text produced by `describe_error`, so the two must stay
/// in step - `transient_markers_match_describe_error` locks that down.
fn is_transient(snapshot: &UsageSnapshot) -> bool {
    if snapshot.status != crate::models::UsageStatus::Error {
        return false;
    }
    let Some(message) = &snapshot.message else {
        return false;
    };
    ["Could not connect", "Timed out"]
        .iter()
        .any(|marker| message.contains(marker))
}

pub fn provider_ids() -> Vec<String> {
    PROVIDERS.iter().map(|p| p.id.to_string()).collect()
}

pub fn provider_infos() -> Vec<ProviderInfo> {
    PROVIDERS
        .iter()
        .map(|p| ProviderInfo {
            id: p.id.to_string(),
            display_name: p.display_name.to_string(),
        })
        .collect()
}

/// Fetch the enabled providers concurrently, preserving registry order.
///
/// Only enabled providers are contacted. The previous implementation fetched
/// all seven and discarded the disabled ones afterwards, which meant a provider
/// the user had switched off still issued a network request every cycle.
pub async fn fetch_enabled(enabled: &[String], ctx: FetchCtx) -> Vec<UsageSnapshot> {
    let selected: Vec<(usize, &Provider)> = PROVIDERS
        .iter()
        .enumerate()
        .filter(|(_, p)| enabled.iter().any(|e| e == p.id))
        .collect();

    let expected: Vec<(usize, &Provider)> = selected.clone();

    let mut tasks = tokio::task::JoinSet::new();
    for (index, provider) in selected {
        let fetch = provider.fetch;
        let ctx = ctx.clone();
        tasks.spawn(async move {
            let mut snapshot = fetch(ctx.clone()).await;
            // A momentary network blip should not turn into a red card on the
            // panel; one retry absorbs it. Deterministic failures (expired
            // login, 403, rate limit) are not retried.
            if is_transient(&snapshot) {
                tokio::time::sleep(RETRY_DELAY).await;
                snapshot = fetch(ctx).await;
            }
            (index, snapshot)
        });
    }

    let mut results: Vec<(usize, UsageSnapshot)> = Vec::new();
    while let Some(joined) = tasks.join_next().await {
        // A `JoinError` means that adapter panicked. `JoinSet` contains the
        // panic so the other providers still complete, but the failed task
        // cannot tell us which provider it was — so we reconcile below.
        if let Ok(pair) = joined {
            results.push(pair);
        }
    }

    // Give any provider whose task died a visible error row rather than letting
    // it silently disappear from the panel.
    for (index, provider) in expected {
        if !results.iter().any(|(i, _)| *i == index) {
            results.push((
                index,
                UsageSnapshot::error(
                    provider.id,
                    provider.display_name,
                    "Adapter crashed while fetching",
                ),
            ));
        }
    }

    results.sort_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, snapshot)| snapshot).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `is_transient` matches on `describe_error`'s wording; if either changes
    /// without the other, retries silently stop happening.
    #[test]
    fn transient_markers_match_describe_error() {
        for message in [
            "Timed out - the API may be unreachable from this network; check the proxy setting",
            "Could not connect - check your network and the proxy setting",
        ] {
            let snapshot = UsageSnapshot::error("p", "P", message);
            assert!(is_transient(&snapshot), "should retry: {message}");
        }
    }

    #[test]
    fn deterministic_failures_are_not_retried() {
        for message in [
            "Login expired - open Claude Code once to refresh",
            "Rate limited, try again later",
            "API returned 403 Forbidden",
        ] {
            let snapshot = UsageSnapshot::error("p", "P", message);
            assert!(!is_transient(&snapshot), "should not retry: {message}");
        }
    }

    #[test]
    fn non_error_states_are_never_retried() {
        assert!(!is_transient(&UsageSnapshot::ok("p", "P", vec![])));
        assert!(!is_transient(&UsageSnapshot::not_connected("p", "P", "nope")));
        assert!(!is_transient(&UsageSnapshot::no_quota_api("p", "P", "plan", vec![])));
    }

    #[test]
    fn provider_ids_are_unique() {
        let mut ids = provider_ids();
        let before = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), before, "duplicate provider id in the registry");
    }

    #[test]
    fn defaults_enable_every_registered_provider() {
        let settings = crate::models::AppSettings::default();
        assert_eq!(settings.enabled_providers, provider_ids());
    }
}
