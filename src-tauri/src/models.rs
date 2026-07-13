use serde::{Deserialize, Serialize};

pub const ALL_PROVIDERS: &[(&str, &str)] = &[
    ("claude", "Claude"),
    ("codex", "Codex"),
    ("cursor", "Cursor"),
    ("amp", "Amp"),
    ("factory", "Factory Droid"),
    ("agy", "AGY"),
    ("grok", "Grok"),
];

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppSettings {
    pub refresh_interval_secs: u64,
    pub enabled_providers: Vec<String>,
    pub autostart: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            refresh_interval_secs: 180,
            enabled_providers: ALL_PROVIDERS.iter().map(|(id, _)| id.to_string()).collect(),
            autostart: false,
        }
    }
}

/// A single measurable quota/limit window for a provider (e.g. "5 hour window",
/// "weekly", "monthly credits"). A provider can report more than one metric.
#[derive(Serialize, Clone, Debug)]
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

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UsageStatus {
    /// Successfully fetched fresh data.
    Ok,
    /// Local credentials for this provider were not found (app not installed / never logged in).
    NotConnected,
    /// Credentials found but expired/invalid, or the request failed.
    Error,
}

#[derive(Serialize, Clone, Debug)]
pub struct UsageSnapshot {
    /// Stable machine id, e.g. "claude"
    pub provider: String,
    /// Display name, e.g. "Claude"
    pub display_name: String,
    pub status: UsageStatus,
    /// Present when `status` is `Error` or `NotConnected`.
    pub message: Option<String>,
    pub metrics: Vec<UsageMetric>,
    pub updated_at: i64,
}

impl UsageSnapshot {
    pub fn not_connected(provider: &str, display_name: &str, message: impl Into<String>) -> Self {
        Self {
            provider: provider.to_string(),
            display_name: display_name.to_string(),
            status: UsageStatus::NotConnected,
            message: Some(message.into()),
            metrics: vec![],
            updated_at: now_unix(),
        }
    }

    pub fn error(provider: &str, display_name: &str, message: impl Into<String>) -> Self {
        Self {
            provider: provider.to_string(),
            display_name: display_name.to_string(),
            status: UsageStatus::Error,
            message: Some(message.into()),
            metrics: vec![],
            updated_at: now_unix(),
        }
    }

    pub fn ok(provider: &str, display_name: &str, metrics: Vec<UsageMetric>) -> Self {
        Self {
            provider: provider.to_string(),
            display_name: display_name.to_string(),
            status: UsageStatus::Ok,
            message: None,
            metrics,
            updated_at: now_unix(),
        }
    }
}

pub fn now_unix() -> i64 {
    chrono::Utc::now().timestamp()
}
