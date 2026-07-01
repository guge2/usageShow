use crate::models::{UsageMetric, UsageSnapshot};
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const PROVIDER: &str = "codex";
const DISPLAY_NAME: &str = "Codex";

#[derive(Deserialize)]
struct AuthFile {
    tokens: Option<Tokens>,
}

#[derive(Deserialize)]
struct Tokens {
    access_token: Option<String>,
    account_id: Option<String>,
}

fn auth_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(home.join(".codex").join("auth.json"))
}

async fn detect_user_agent() -> String {
    let fallback = "codex-cli/0.0.0".to_string();
    let mut cmd = tokio::process::Command::new("codex");
    cmd.arg("--version").stdin(std::process::Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let output = cmd.output().await;
    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let version = text
                .split_whitespace()
                .last()
                .unwrap_or("0.0.0")
                .to_string();
            format!("codex-cli/{version}")
        }
        _ => fallback,
    }
}

fn extract_window(root: &Value, key: &str) -> Option<(f64, Option<i64>)> {
    let window = root.get("rate_limit")?.get(key)?;
    let percent = window.get("used_percent")?.as_f64()?;
    let reset_at = window.get("reset_at").and_then(|v| v.as_i64());
    Some((percent, reset_at))
}

pub async fn fetch() -> UsageSnapshot {
    let Some(path) = auth_path() else {
        return UsageSnapshot::not_connected(
            PROVIDER,
            DISPLAY_NAME,
            "Could not locate home directory",
        );
    };
    let Ok(raw) = tokio::fs::read_to_string(&path).await else {
        return UsageSnapshot::not_connected(PROVIDER, DISPLAY_NAME, "Codex login not found");
    };
    let parsed: Result<AuthFile, _> = serde_json::from_str(&raw);
    let Ok(auth) = parsed else {
        return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, "Failed to parse login credentials");
    };
    let Some(tokens) = auth.tokens else {
        return UsageSnapshot::not_connected(
            PROVIDER,
            DISPLAY_NAME,
            "Codex is not logged in with a ChatGPT account",
        );
    };
    let Some(access_token) = tokens.access_token else {
        return UsageSnapshot::not_connected(
            PROVIDER,
            DISPLAY_NAME,
            "Codex is not logged in with a ChatGPT account",
        );
    };

    let user_agent = detect_user_agent().await;
    let client = super::http_client();
    let mut req = client
        .get("https://chatgpt.com/backend-api/wham/usage")
        .header("Authorization", format!("Bearer {access_token}"))
        .header("User-Agent", user_agent);
    if let Some(account_id) = &tokens.account_id {
        req = req.header("chatgpt-account-id", account_id);
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, format!("Request failed: {e}"))
        }
    };

    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return UsageSnapshot::error(
            PROVIDER,
            DISPLAY_NAME,
            "Login expired - open Codex CLI once to refresh",
        );
    }
    if !resp.status().is_success() {
        return UsageSnapshot::error(
            PROVIDER,
            DISPLAY_NAME,
            format!("API returned {}", resp.status()),
        );
    }

    let body: Result<Value, _> = resp.json().await;
    let Ok(root) = body else {
        return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, "Failed to parse response");
    };

    let mut metrics = Vec::new();
    if let Some((percent, reset_at)) = extract_window(&root, "primary_window") {
        metrics.push(UsageMetric {
            label: "Primary limit".to_string(),
            used: percent,
            limit: Some(100.0),
            percent: Some(percent),
            unit: "percent".to_string(),
            reset_at,
        });
    }
    if let Some((percent, reset_at)) = extract_window(&root, "secondary_window") {
        metrics.push(UsageMetric {
            label: "Secondary limit".to_string(),
            used: percent,
            limit: Some(100.0),
            percent: Some(percent),
            unit: "percent".to_string(),
            reset_at,
        });
    }

    if metrics.is_empty() {
        return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, "No active usage window");
    }

    UsageSnapshot::ok(PROVIDER, DISPLAY_NAME, metrics)
}
