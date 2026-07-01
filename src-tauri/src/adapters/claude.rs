use crate::models::{UsageMetric, UsageSnapshot};
use serde::Deserialize;
use std::path::PathBuf;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const PROVIDER: &str = "claude";
const DISPLAY_NAME: &str = "Claude";

#[derive(Deserialize)]
struct CredentialsFile {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: Option<OauthBlock>,
}

#[derive(Deserialize)]
struct OauthBlock {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "expiresAt")]
    expires_at: Option<i64>,
}

#[derive(Deserialize)]
struct UsageWindow {
    utilization: Option<f64>,
    resets_at: Option<String>,
}

#[derive(Deserialize)]
struct ExtraUsage {
    is_enabled: Option<bool>,
    monthly_limit: Option<f64>,
    used_credits: Option<f64>,
    utilization: Option<f64>,
}

#[derive(Deserialize)]
struct UsageResponse {
    five_hour: Option<UsageWindow>,
    seven_day: Option<UsageWindow>,
    seven_day_opus: Option<UsageWindow>,
    seven_day_sonnet: Option<UsageWindow>,
    extra_usage: Option<ExtraUsage>,
}

fn credentials_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(home.join(".claude").join(".credentials.json"))
}

fn parse_reset_at(iso: &Option<String>) -> Option<i64> {
    iso.as_ref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp())
}

fn push_window(metrics: &mut Vec<UsageMetric>, label: &str, window: Option<UsageWindow>) {
    if let Some(w) = window {
        if let Some(pct) = w.utilization {
            metrics.push(UsageMetric {
                label: label.to_string(),
                used: pct,
                limit: Some(100.0),
                percent: Some(pct),
                unit: "percent".to_string(),
                reset_at: parse_reset_at(&w.resets_at),
            });
        }
    }
}

/// Best-effort detection of the locally installed Claude Code version, used to
/// build an authentic `claude-code/<version>` User-Agent header. Anthropic's
/// usage endpoint rate-limits requests aggressively unless this prefix is present.
async fn detect_user_agent() -> String {
    let fallback = "claude-code/2.0.0".to_string();
    let mut cmd = tokio::process::Command::new("claude");
    cmd.arg("--version").stdin(std::process::Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let output = cmd.output().await;
    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let version = text.split_whitespace().next().unwrap_or("2.0.0");
            format!("claude-code/{version}")
        }
        _ => fallback,
    }
}

pub async fn fetch() -> UsageSnapshot {
    let Some(path) = credentials_path() else {
        return UsageSnapshot::not_connected(PROVIDER, DISPLAY_NAME, "Could not locate home directory");
    };
    let Ok(raw) = tokio::fs::read_to_string(&path).await else {
        return UsageSnapshot::not_connected(PROVIDER, DISPLAY_NAME, "Claude Code login not found");
    };
    let parsed: Result<CredentialsFile, _> = serde_json::from_str(&raw);
    let Ok(creds) = parsed else {
        return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, "Failed to parse login credentials");
    };
    let Some(oauth) = creds.claude_ai_oauth else {
        return UsageSnapshot::not_connected(PROVIDER, DISPLAY_NAME, "Claude Code is not logged in via OAuth");
    };

    if let Some(expires_at_ms) = oauth.expires_at {
        let now_ms = chrono::Utc::now().timestamp_millis();
        if now_ms > expires_at_ms {
            return UsageSnapshot::error(
                PROVIDER,
                DISPLAY_NAME,
                "Login expired - open Claude Code once to refresh",
            );
        }
    }

    let user_agent = detect_user_agent().await;
    let client = super::http_client();
    let resp = client
        .get("https://api.anthropic.com/api/oauth/usage")
        .header("Authorization", format!("Bearer {}", oauth.access_token))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", user_agent)
        .header("Content-Type", "application/json")
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, format!("Request failed: {e}")),
    };

    if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, "Rate limited, please try again later");
    }
    if !resp.status().is_success() {
        return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, format!("API returned {}", resp.status()));
    }

    let body: Result<UsageResponse, _> = resp.json().await;
    let Ok(usage) = body else {
        return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, "Failed to parse response");
    };

    let mut metrics = Vec::new();
    push_window(&mut metrics, "5h limit", usage.five_hour);
    push_window(&mut metrics, "7d limit", usage.seven_day);
    push_window(&mut metrics, "7d limit (Opus)", usage.seven_day_opus);
    push_window(&mut metrics, "7d limit (Sonnet)", usage.seven_day_sonnet);
    if let Some(extra) = usage.extra_usage {
        if extra.is_enabled.unwrap_or(false) {
            metrics.push(UsageMetric {
                label: "Extra Usage".to_string(),
                used: extra.used_credits.unwrap_or(0.0),
                limit: extra.monthly_limit,
                percent: extra.utilization,
                unit: "usd".to_string(),
                reset_at: None,
            });
        }
    }

    if metrics.is_empty() {
        return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, "No active usage window");
    }

    UsageSnapshot::ok(PROVIDER, DISPLAY_NAME, metrics)
}
