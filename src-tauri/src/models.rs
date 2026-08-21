use serde::{Deserialize, Serialize};

/// How adapters should reach the network.
///
/// `Auto` follows the machine: `HTTPS_PROXY`/`HTTP_PROXY` first, then the
/// Windows system proxy. This is the default because the vendor CLIs and
/// browsers already behave this way, and a user who needs a proxy to reach
/// these APIs at all should not have to configure it twice.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    #[default]
    Auto,
    Off,
    Manual,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppSettings {
    pub refresh_interval_secs: u64,
    pub enabled_providers: Vec<String>,
    pub autostart: bool,
    /// Absent in configs written before proxy support existed.
    #[serde(default)]
    pub proxy_mode: ProxyMode,
    #[serde(default)]
    pub proxy_url: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            refresh_interval_secs: 180,
            enabled_providers: crate::adapters::provider_ids(),
            autostart: false,
            proxy_mode: ProxyMode::Auto,
            proxy_url: String::new(),
        }
    }
}

/// Provider identity as sent to the frontend, so the UI never has to keep its
/// own hand-maintained copy of the provider list.
#[derive(Serialize, Clone, Debug)]
pub struct ProviderInfo {
    pub id: String,
    pub display_name: String,
}

/// What the app resolved the proxy to, surfaced in Settings so a user can see
/// whether their proxy was picked up without reading logs.
#[derive(Serialize, Clone, Debug)]
pub struct ProxyStatus {
    pub description: String,
    pub active: bool,
}

/// A single measurable quota/limit window for a provider (e.g. "5 hour window",
/// "weekly", "monthly credits"). A provider can report more than one metric.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct UsageMetric {
    /// Short human label, e.g. "5h limit" / "Weekly limit" / "Amp Free"
    pub label: String,
    /// Amount already used, in `unit`.
    pub used: f64,
    /// Total allowance, in `unit`. `None` when the provider doesn't expose a hard cap.
    pub limit: Option<f64>,
    /// Percentage already used (0-100), when known directly from the provider.
    pub percent: Option<f64>,
    /// Unit of `used`/`limit`: "percent" | "usd" | "requests" | "tokens"
    pub unit: String,
    /// Unix seconds when this window resets, if known.
    pub reset_at: Option<i64>,
}

impl UsageMetric {
    /// A 0-100 percentage metric, the shape most providers report.
    pub fn percent(label: impl Into<String>, used: f64, reset_at: Option<i64>) -> Self {
        Self {
            label: label.into(),
            used,
            limit: Some(100.0),
            percent: Some(used),
            unit: "percent".to_string(),
            reset_at,
        }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UsageStatus {
    /// Successfully fetched fresh data.
    Ok,
    /// Local credentials for this provider were not found (app not installed / never logged in).
    NotConnected,
    /// Credentials found but expired/invalid, or the request failed.
    Error,
    /// Connected and healthy, but this account tier exposes no usage figures.
    /// Distinct from `Error`: nothing is broken and retrying will not help.
    NoQuotaApi,
}

/// How long carried-over metrics stay on screen before a failure is shown bare.
/// Beyond this the numbers are old enough to mislead.
const STALE_GRACE_SECS: i64 = 30 * 60;

#[derive(Serialize, Clone, Debug)]
pub struct UsageSnapshot {
    /// Stable machine id, e.g. "claude"
    pub provider: String,
    /// Display name, e.g. "Claude"
    pub display_name: String,
    pub status: UsageStatus,
    /// Present when `status` is anything other than `Ok`.
    pub message: Option<String>,
    pub metrics: Vec<UsageMetric>,
    pub updated_at: i64,
    /// True when `metrics` are carried over from an earlier successful fetch
    /// because this one failed. Lets the panel keep showing real numbers
    /// through a transient hiccup instead of blanking out.
    pub stale: bool,
}

impl UsageSnapshot {
    fn new(
        provider: &str,
        display_name: &str,
        status: UsageStatus,
        message: Option<String>,
        metrics: Vec<UsageMetric>,
    ) -> Self {
        Self {
            provider: provider.to_string(),
            display_name: display_name.to_string(),
            status,
            message,
            metrics,
            updated_at: now_unix(),
            stale: false,
        }
    }

    pub fn not_connected(provider: &str, display_name: &str, message: impl Into<String>) -> Self {
        Self::new(
            provider,
            display_name,
            UsageStatus::NotConnected,
            Some(message.into()),
            vec![],
        )
    }

    pub fn error(provider: &str, display_name: &str, message: impl Into<String>) -> Self {
        Self::new(
            provider,
            display_name,
            UsageStatus::Error,
            Some(message.into()),
            vec![],
        )
    }

    pub fn ok(provider: &str, display_name: &str, metrics: Vec<UsageMetric>) -> Self {
        Self::new(provider, display_name, UsageStatus::Ok, None, metrics)
    }

    /// Logged in and reachable, but the account tier has no usage API.
    /// `metrics` may still carry whatever partial info is available.
    pub fn no_quota_api(
        provider: &str,
        display_name: &str,
        message: impl Into<String>,
        metrics: Vec<UsageMetric>,
    ) -> Self {
        Self::new(
            provider,
            display_name,
            UsageStatus::NoQuotaApi,
            Some(message.into()),
            metrics,
        )
    }

    /// Combine a failed fetch with the previous good one for the same provider.
    ///
    /// A rate-limit or a blip should not wipe the panel: keep the last known
    /// metrics, flag them stale, and let the message explain why. Returns the
    /// failure untouched when there is nothing usable to fall back to.
    pub fn or_previous(self, previous: Option<&UsageSnapshot>) -> Self {
        // Nothing to repair: it succeeded, or it already carries its own data
        // (e.g. a `no_quota_api` result with partial info).
        if self.status == UsageStatus::Ok || !self.metrics.is_empty() {
            return self;
        }
        let Some(previous) = previous else {
            return self;
        };
        if previous.status != UsageStatus::Ok || previous.metrics.is_empty() {
            return self;
        }
        if now_unix() - previous.updated_at > STALE_GRACE_SECS {
            return self;
        }
        Self {
            metrics: previous.metrics.clone(),
            updated_at: previous.updated_at,
            stale: true,
            ..self
        }
    }
}

pub fn now_unix() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_snapshot(age_secs: i64) -> UsageSnapshot {
        let mut snapshot = UsageSnapshot::ok(
            "claude",
            "Claude",
            vec![UsageMetric::percent("5h limit", 39.0, None)],
        );
        snapshot.updated_at = now_unix() - age_secs;
        snapshot
    }

    #[test]
    fn rate_limit_keeps_the_last_good_numbers() {
        let previous = ok_snapshot(60);
        let failed = UsageSnapshot::error("claude", "Claude", "Rate limited, try again later");

        let merged = failed.or_previous(Some(&previous));

        assert!(merged.stale);
        assert_eq!(merged.status, UsageStatus::Error);
        assert_eq!(merged.metrics, previous.metrics);
        // The reason for the failure is still shown to the user.
        assert_eq!(
            merged.message.as_deref(),
            Some("Rate limited, try again later")
        );
        // And the timestamp stays honest about when the data was actually read.
        assert_eq!(merged.updated_at, previous.updated_at);
    }

    #[test]
    fn data_past_the_grace_window_is_not_resurrected() {
        let previous = ok_snapshot(STALE_GRACE_SECS + 60);
        let failed = UsageSnapshot::error("claude", "Claude", "Rate limited");

        let merged = failed.or_previous(Some(&previous));

        assert!(!merged.stale);
        assert!(merged.metrics.is_empty());
    }

    #[test]
    fn a_previous_failure_is_not_used_as_a_fallback() {
        let previous = UsageSnapshot::error("claude", "Claude", "Older failure");
        let failed = UsageSnapshot::error("claude", "Claude", "Newer failure");

        assert!(!failed.or_previous(Some(&previous)).stale);
    }

    #[test]
    fn first_ever_failure_has_nothing_to_fall_back_on() {
        let failed = UsageSnapshot::error("claude", "Claude", "Rate limited");
        assert!(!failed.or_previous(None).stale);
    }

    #[test]
    fn a_successful_fetch_is_never_marked_stale() {
        let previous = ok_snapshot(60);
        let fresh = UsageSnapshot::ok(
            "claude",
            "Claude",
            vec![UsageMetric::percent("5h limit", 41.0, None)],
        );

        let merged = fresh.or_previous(Some(&previous));

        assert!(!merged.stale);
        assert_eq!(merged.metrics[0].used, 41.0);
    }

    #[test]
    fn no_quota_api_keeps_its_own_message_and_is_not_backfilled() {
        let previous = ok_snapshot(60);
        let info = UsageSnapshot::no_quota_api("agy", "AGY", "Google AI Plus", Vec::new());

        let merged = info.or_previous(Some(&previous));

        // It has no metrics, so the previous ones are reused - but the status
        // and its explanation survive.
        assert_eq!(merged.status, UsageStatus::NoQuotaApi);
        assert_eq!(merged.message.as_deref(), Some("Google AI Plus"));
    }
}
