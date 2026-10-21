use super::creds;
use super::FetchCtx;
use crate::models::{title_case, window_label, UsageMetric, UsageSnapshot};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::OnceCell;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const PROVIDER: &str = "codex";
const DISPLAY_NAME: &str = "Codex";
const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

#[derive(Deserialize)]
struct AuthFile {
    tokens: Option<Tokens>,
}

#[derive(Deserialize)]
struct Tokens {
    access_token: Option<String>,
    account_id: Option<String>,
}

/// Cached for the process lifetime — see the note in `claude.rs`.
async fn user_agent() -> &'static str {
    static UA: OnceCell<String> = OnceCell::const_new();
    UA.get_or_init(|| async {
        let mut cmd = tokio::process::Command::new("codex");
        cmd.arg("--version").stdin(std::process::Stdio::null());
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);
        match cmd.output().await {
            Ok(out) if out.status.success() => {
                let text = String::from_utf8_lossy(&out.stdout);
                let version = text.split_whitespace().last().unwrap_or("0.0.0").to_string();
                format!("codex-cli/{version}")
            }
            _ => "codex-cli/0.0.0".to_string(),
        }
    })
    .await
    .as_str()
}

/// Read one `rate_limit.<key>` window. `limit_window_seconds` says what it
/// actually is: "primary" is the 5-hour window on most plans but can be the
/// weekly one, so the label comes from the span, not the slot.
fn window(root: &Value, key: &str) -> Option<UsageMetric> {
    let window = root.get("rate_limit")?.get(key)?;
    let percent = window.get("used_percent")?.as_f64()?;
    let span = window.get("limit_window_seconds").and_then(Value::as_i64);
    let reset_at = creds::parse_epoch(window.get("reset_at")).or_else(|| {
        window
            .get("reset_after_seconds")
            .and_then(Value::as_i64)
            .map(|secs| crate::models::now_unix() + secs)
    });
    let label = match span {
        Some(s) => window_label(s),
        None if key == "primary_window" => "Primary".to_string(),
        None => "Secondary".to_string(),
    };
    Some(UsageMetric::percent(label, percent, reset_at).spanning(span))
}

/// Purchased credits beside the plan's windows, when the account has any.
fn credits(root: &Value) -> Option<UsageMetric> {
    let credits = root.get("credits")?;
    if !credits.get("has_credits")?.as_bool()? {
        return None;
    }
    if credits.get("unlimited").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let balance = credits.get("balance").and_then(|b| {
        b.as_f64()
            .or_else(|| b.as_str().and_then(|s| s.parse::<f64>().ok()))
    })?;
    Some(UsageMetric::amount("Credits", balance, "credits"))
}

pub async fn fetch(ctx: FetchCtx) -> UsageSnapshot {
    let Some(path) = creds::home_path(&[".codex", "auth.json"]) else {
        return UsageSnapshot::not_connected(
            PROVIDER,
            DISPLAY_NAME,
            "Could not locate home directory",
        );
    };

    let auth: AuthFile = match creds::read_json(&path).await {
        Ok(auth) => auth,
        Err(None) => {
            return UsageSnapshot::not_connected(PROVIDER, DISPLAY_NAME, "Codex login not found")
        }
        Err(Some(msg)) => return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, msg),
    };

    let Some(access_token) = auth.tokens.as_ref().and_then(|t| t.access_token.clone()) else {
        return UsageSnapshot::not_connected(
            PROVIDER,
            DISPLAY_NAME,
            "Codex is not logged in with a ChatGPT account",
        );
    };

    let mut request = ctx
        .client
        .get(USAGE_URL)
        .bearer_auth(&access_token)
        .header("User-Agent", user_agent().await);
    if let Some(account_id) = auth.tokens.and_then(|t| t.account_id) {
        request = request.header("chatgpt-account-id", account_id);
    }

    let response = match request.send().await {
        Ok(r) => r,
        Err(e) => return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, super::describe_error(&e)),
    };

    match response.status() {
        reqwest::StatusCode::UNAUTHORIZED => {
            return UsageSnapshot::error(
                PROVIDER,
                DISPLAY_NAME,
                "Login expired - open Codex CLI once to refresh",
            )
        }
        reqwest::StatusCode::TOO_MANY_REQUESTS => {
            return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, "Rate limited, try again later")
        }
        status if !status.is_success() => {
            return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, format!("API returned {status}"))
        }
        _ => {}
    }

    let Ok(root) = response.json::<Value>().await else {
        return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, "Failed to parse response");
    };

    let mut metrics: Vec<UsageMetric> = ["primary_window", "secondary_window"]
        .iter()
        .filter_map(|key| window(&root, key))
        .collect();
    // Shortest window first, so the 5-hour window leads whichever slot it's in.
    metrics.sort_by_key(|m| match (m.reset_at, m.starts_at) {
        (Some(r), Some(s)) => r - s,
        _ => i64::MAX,
    });

    if metrics.is_empty() {
        return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, "No active usage window");
    }
    metrics.extend(credits(&root));

    let text = |key: &str| root.get(key).and_then(Value::as_str);
    UsageSnapshot::ok(PROVIDER, DISPLAY_NAME, metrics)
        .with_plan(text("plan_type").map(title_case))
        .with_account(text("email").map(ToString::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_a_window_with_its_reset_time() {
        let root = json!({
            "rate_limit": {
                "primary_window": { "used_percent": 42.5, "reset_at": 1_700_000_000 }
            }
        });
        let m = window(&root, "primary_window").unwrap();
        assert_eq!((m.used, m.reset_at), (42.5, Some(1_700_000_000)));
    }

    #[test]
    fn reset_in_milliseconds_is_normalized() {
        let root = json!({
            "rate_limit": {
                "primary_window": { "used_percent": 1.0, "reset_at": 1_700_000_000_000i64 }
            }
        });
        assert_eq!(
            window(&root, "primary_window").unwrap().reset_at,
            Some(1_700_000_000)
        );
    }

    #[test]
    fn a_missing_window_is_none() {
        let root = json!({ "rate_limit": {} });
        assert_eq!(window(&root, "primary_window"), None);
        assert_eq!(window(&json!({}), "primary_window"), None);
    }

    #[test]
    fn weekly_primary_window_is_labelled_by_its_span() {
        // Real shape from a Pro account whose only window is the weekly one.
        let root = json!({
            "plan_type": "pro",
            "rate_limit": {
                "primary_window": {
                    "used_percent": 100,
                    "limit_window_seconds": 604800,
                    "reset_after_seconds": 384767,
                    "reset_at": 1791640059
                },
                "secondary_window": null
            },
            "credits": { "has_credits": true, "unlimited": false, "balance": "28263.2358" }
        });
        let m = window(&root, "primary_window").unwrap();
        assert_eq!(m.label, "7d");
        assert_eq!(m.starts_at, Some(1791640059 - 604800));
        assert!(window(&root, "secondary_window").is_none());
        assert!((credits(&root).unwrap().used - 28263.2358).abs() < 1e-6);
    }
}
