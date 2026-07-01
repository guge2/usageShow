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
        return UsageSnapshot::not_connected(PROVIDER, DISPLAY_NAME, "无法定位用户目录");
    };
    let Ok(raw) = tokio::fs::read_to_string(&path).await else {
        return UsageSnapshot::not_connected(PROVIDER, DISPLAY_NAME, "未找到 Claude Code 登录信息");
    };
    let parsed: Result<CredentialsFile, _> = serde_json::from_str(&raw);
    let Ok(creds) = parsed else {
        return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, "登录信息解析失败");
    };
    let Some(oauth) = creds.claude_ai_oauth else {
        return UsageSnapshot::not_connected(PROVIDER, DISPLAY_NAME, "未使用 OAuth 登录 Claude Code");
    };

    if let Some(expires_at_ms) = oauth.expires_at {
        let now_ms = chrono::Utc::now().timestamp_millis();
        if now_ms > expires_at_ms {
            return UsageSnapshot::error(
                PROVIDER,
                DISPLAY_NAME,
                "登录状态已过期，请打开一次 Claude Code 以刷新",
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
        Err(e) => return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, format!("请求失败: {e}")),
    };

    if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, "请求过于频繁，请稍后再试");
    }
    if !resp.status().is_success() {
        return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, format!("接口返回 {}", resp.status()));
    }

    let body: Result<UsageResponse, _> = resp.json().await;
    let Ok(usage) = body else {
        return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, "响应格式解析失败");
    };

    let mut metrics = Vec::new();
    push_window(&mut metrics, "5 小时限额", usage.five_hour);
    push_window(&mut metrics, "7 天限额", usage.seven_day);
    push_window(&mut metrics, "7 天限额 (Opus)", usage.seven_day_opus);
    push_window(&mut metrics, "7 天限额 (Sonnet)", usage.seven_day_sonnet);
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
        return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, "暂无活跃的用量窗口");
    }

    UsageSnapshot::ok(PROVIDER, DISPLAY_NAME, metrics)
}
