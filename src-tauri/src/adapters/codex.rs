use super::creds;
use super::FetchCtx;
use crate::models::{UsageMetric, UsageSnapshot};
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

/// Read one `rate_limit.<key>` window: percent used plus its reset time.
fn window(root: &Value, key: &str) -> Option<(f64, Option<i64>)> {
    let window = root.get("rate_limit")?.get(key)?;
    let percent = window.get("used_percent")?.as_f64()?;
    let reset_at = creds::parse_epoch(window.get("reset_at"));
    Some((percent, reset_at))
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

    let mut metrics = Vec::new();
    for (key, label) in [
        ("primary_window", "Primary limit"),
        ("secondary_window", "Secondary limit"),
    ] {
        if let Some((percent, reset_at)) = window(&root, key) {
            metrics.push(UsageMetric::percent(label, percent, reset_at));
        }
    }

    if metrics.is_empty() {
        return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, "No active usage window");
    }
    UsageSnapshot::ok(PROVIDER, DISPLAY_NAME, metrics)
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
        assert_eq!(
            window(&root, "primary_window"),
            Some((42.5, Some(1_700_000_000)))
        );
    }

    #[test]
    fn reset_in_milliseconds_is_normalized() {
        let root = json!({
            "rate_limit": {
                "primary_window": { "used_percent": 1.0, "reset_at": 1_700_000_000_000i64 }
            }
        });
        assert_eq!(
            window(&root, "primary_window"),
            Some((1.0, Some(1_700_000_000)))
        );
    }

    #[test]
    fn a_missing_window_is_none() {
        let root = json!({ "rate_limit": {} });
        assert_eq!(window(&root, "primary_window"), None);
        assert_eq!(window(&json!({}), "primary_window"), None);
    }
}
