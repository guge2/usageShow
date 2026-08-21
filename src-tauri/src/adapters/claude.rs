use super::creds;
use super::FetchCtx;
use crate::models::{UsageMetric, UsageSnapshot};
use serde::Deserialize;
use tokio::sync::OnceCell;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const PROVIDER: &str = "claude";
const DISPLAY_NAME: &str = "Claude";
const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";

#[derive(Deserialize)]
struct CredentialsFile {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: Option<OauthBlock>,
}

#[derive(Deserialize)]
struct OauthBlock {
    #[serde(rename = "accessToken")]
    access_token: String,
    /// Unix milliseconds.
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

/// Best-effort detection of the installed Claude Code version, used to build an
/// authentic `claude-code/<version>` User-Agent. Anthropic's usage endpoint
/// rate-limits aggressively without it.
///
/// Cached for the process lifetime: the version does not change under a running
/// tray app, and spawning a subprocess on every refresh cycle is pure overhead.
async fn user_agent() -> &'static str {
    static UA: OnceCell<String> = OnceCell::const_new();
    UA.get_or_init(|| async {
        let mut cmd = tokio::process::Command::new("claude");
        cmd.arg("--version").stdin(std::process::Stdio::null());
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);
        match cmd.output().await {
            Ok(out) if out.status.success() => {
                let text = String::from_utf8_lossy(&out.stdout);
                let version = text.split_whitespace().next().unwrap_or("2.0.0");
                format!("claude-code/{version}")
            }
            _ => "claude-code/2.0.0".to_string(),
        }
    })
    .await
    .as_str()
}

fn push_window(metrics: &mut Vec<UsageMetric>, label: &str, window: Option<UsageWindow>) {
    if let Some(w) = window {
        if let Some(pct) = w.utilization {
            metrics.push(UsageMetric::percent(
                label,
                pct,
                creds::parse_rfc3339(&w.resets_at),
            ));
        }
    }
}

pub async fn fetch(ctx: FetchCtx) -> UsageSnapshot {
    let Some(path) = creds::home_path(&[".claude", ".credentials.json"]) else {
        return UsageSnapshot::not_connected(
            PROVIDER,
            DISPLAY_NAME,
            "Could not locate home directory",
        );
    };

    let file: CredentialsFile = match creds::read_json(&path).await {
        Ok(file) => file,
        // On macOS the OAuth token lives in the Keychain, so a missing file
        // does not necessarily mean "not logged in" — but it does mean there is
        // nothing here for us to read.
        Err(None) => {
            return UsageSnapshot::not_connected(PROVIDER, DISPLAY_NAME, "Claude Code login not found")
        }
        Err(Some(msg)) => return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, msg),
    };

    let Some(oauth) = file.claude_ai_oauth else {
        return UsageSnapshot::not_connected(
            PROVIDER,
            DISPLAY_NAME,
            "Claude Code is not logged in via OAuth",
        );
    };

    let expires_at_secs = oauth.expires_at.map(creds::normalize_epoch);
    if creds::is_expired(expires_at_secs) {
        return UsageSnapshot::error(
            PROVIDER,
            DISPLAY_NAME,
            "Login expired - open Claude Code once to refresh",
        );
    }

    let response = ctx
        .client
        .get(USAGE_URL)
        .bearer_auth(&oauth.access_token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", user_agent().await)
        .header("Content-Type", "application/json")
        .send()
        .await;

    let response = match response {
        Ok(r) => r,
        Err(e) => return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, super::describe_error(&e)),
    };

    match response.status() {
        reqwest::StatusCode::TOO_MANY_REQUESTS => {
            return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, "Rate limited, try again later")
        }
        reqwest::StatusCode::UNAUTHORIZED => {
            return UsageSnapshot::error(
                PROVIDER,
                DISPLAY_NAME,
                "Login expired - open Claude Code once to refresh",
            )
        }
        // Anthropic answers a blocked/unroutable origin with 403 rather than a
        // connection error, which reads as an auth problem but usually is not.
        reqwest::StatusCode::FORBIDDEN => {
            return UsageSnapshot::error(
                PROVIDER,
                DISPLAY_NAME,
                "Request refused (403) - the API may be unreachable from this network; check the proxy setting",
            )
        }
        status if !status.is_success() => {
            return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, format!("API returned {status}"))
        }
        _ => {}
    }

    let Ok(usage) = response.json::<UsageResponse>().await else {
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
