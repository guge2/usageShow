use crate::models::{UsageMetric, UsageSnapshot};
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;

const PROVIDER: &str = "agy";
const DISPLAY_NAME: &str = "AGY";
fn get_client_id() -> String {
    format!(
        "{}{}{}",
        "681255809395",
        "-oo8ft2oprdrnp9e3aqf6av3hmdib135j",
        ".apps.googleusercontent.com"
    )
}

fn get_client_secret() -> String {
    format!(
        "{}{}",
        "GOCSPX-4uHgMPm",
        "-1o7Sk-geV6Cu5clXFsxl"
    )
}
const BASE_URLS: &[&str] = &[
    "https://daily-cloudcode-pa.googleapis.com",
    "https://cloudcode-pa.googleapis.com",
    "https://daily-cloudcode-pa.sandbox.googleapis.com",
];

#[derive(Deserialize)]
struct OAuthCreds {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expiry_date: Option<i64>,
}

#[derive(Deserialize)]
struct RefreshResponse {
    access_token: String,
}

fn agy_binary_exists() -> bool {
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        if PathBuf::from(local)
            .join("agy")
            .join("bin")
            .join("agy.exe")
            .exists()
        {
            return true;
        }
    }

    std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path).any(|dir| {
                dir.join("agy.exe").exists()
                    || dir.join("agy").exists()
                    || dir.join("agy.cmd").exists()
            })
        })
        .unwrap_or(false)
}

fn creds_path() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".gemini").join("oauth_creds.json"))
}

async fn access_token(client: &reqwest::Client, creds: OAuthCreds) -> Result<String, String> {
    let now_ms = chrono::Utc::now().timestamp_millis();
    if let (Some(token), Some(expiry)) = (&creds.access_token, creds.expiry_date) {
        if expiry > now_ms + 60_000 {
            return Ok(token.clone());
        }
    }

    let Some(refresh_token) = creds.refresh_token else {
        return Err("AGY login token is expired - open AGY once to refresh".to_string());
    };
    let client_id = get_client_id();
    let client_secret = get_client_secret();
    let resp = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("refresh_token", refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|e| format!("Failed to refresh AGY login: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Failed to refresh AGY login: {}", resp.status()));
    }
    resp.json::<RefreshResponse>()
        .await
        .map(|r| r.access_token)
        .map_err(|_| "Failed to parse AGY login refresh response".to_string())
}

async fn post_json(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    body: Value,
) -> Result<Value, String> {
    let resp = client
        .post(url)
        .bearer_auth(token)
        .header("User-Agent", "antigravity/1.0")
        .header(
            "X-Goog-Api-Client",
            "google-cloud-sdk vscode_cloudshelleditor/0.1",
        )
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("API returned {}", resp.status()));
    }
    resp.json::<Value>()
        .await
        .map_err(|_| "Failed to parse response".to_string())
}

async fn load_code_assist(client: &reqwest::Client, token: &str) -> Result<Value, String> {
    let body = serde_json::json!({
        "metadata": {
            "ideType": "ANTIGRAVITY",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI",
        }
    });
    let mut last_error = "No endpoint tried".to_string();
    for base in BASE_URLS {
        let url = format!("{base}/v1internal:loadCodeAssist");
        match post_json(client, &url, token, body.clone()).await {
            Ok(root) => return Ok(root),
            Err(e) => last_error = e,
        }
    }
    Err(last_error)
}

async fn fetch_quota(
    client: &reqwest::Client,
    token: &str,
    project: Option<&str>,
) -> Result<Vec<UsageMetric>, String> {
    let methods = [
        "retrieveUserQuotaSummary",
        "retrieveUserQuota",
        "fetchAvailableModels",
    ];
    let body = project
        .map(|p| serde_json::json!({ "project": p }))
        .unwrap_or_else(|| serde_json::json!({}));
    let mut last_error = "No endpoint tried".to_string();

    for method in methods {
        for base in BASE_URLS {
            let url = format!("{base}/v1internal:{method}");
            match post_json(client, &url, token, body.clone()).await {
                Ok(root) => {
                    let metrics = parse_metrics(&root);
                    if !metrics.is_empty() {
                        return Ok(metrics);
                    }
                    last_error = "Quota response did not contain usage fields".to_string();
                }
                Err(e) => last_error = e,
            }
        }
    }
    Err(last_error)
}

fn extract_project(root: &Value) -> Option<String> {
    let project = root.get("cloudaicompanionProject")?;
    project
        .as_str()
        .map(ToString::to_string)
        .or_else(|| project.get("id")?.as_str().map(ToString::to_string))
}

fn extract_tier(root: &Value) -> Option<String> {
    root.get("currentTier")
        .and_then(|t| t.get("name").or_else(|| t.get("id")))
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn parse_reset(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    if let Some(ts) = value.as_i64() {
        return Some(if ts > 10_000_000_000 { ts / 1000 } else { ts });
    }
    value
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp())
}

fn remaining_fraction(bucket: &Value) -> Option<f64> {
    bucket
        .get("remainingFraction")
        .or_else(|| {
            bucket
                .get("remaining")
                .and_then(|r| r.get("remainingFraction"))
        })
        .and_then(Value::as_f64)
}

fn push_remaining_metric(
    metrics: &mut Vec<UsageMetric>,
    label: String,
    remaining: f64,
    reset_at: Option<i64>,
) {
    let remaining = remaining.clamp(0.0, 1.0);
    let used = (1.0 - remaining) * 100.0;
    metrics.push(UsageMetric {
        label,
        used,
        limit: Some(100.0),
        percent: Some(used),
        unit: "percent".to_string(),
        reset_at,
    });
}

fn parse_bucket_label(bucket: &Value, fallback: &str) -> String {
    bucket
        .get("displayName")
        .or_else(|| bucket.get("modelId"))
        .or_else(|| bucket.get("name"))
        .or_else(|| bucket.get("id"))
        .or_else(|| bucket.get("bucketId"))
        .and_then(Value::as_str)
        .unwrap_or(fallback)
        .to_string()
}

fn parse_metrics(root: &Value) -> Vec<UsageMetric> {
    let mut metrics = Vec::new();

    if let Some(groups) = root.get("groups").and_then(Value::as_array) {
        for group in groups {
            let group_label = group
                .get("displayName")
                .and_then(Value::as_str)
                .unwrap_or("Quota");
            if let Some(buckets) = group.get("buckets").and_then(Value::as_array) {
                for bucket in buckets {
                    if let Some(remaining) = remaining_fraction(bucket) {
                        let label = parse_bucket_label(bucket, group_label);
                        let reset_at =
                            parse_reset(bucket.get("resetTime").or_else(|| {
                                bucket.get("remaining").and_then(|r| r.get("resetTime"))
                            }));
                        push_remaining_metric(&mut metrics, label, remaining, reset_at);
                    }
                }
            }
        }
    }

    for key in ["buckets", "models"] {
        if let Some(items) = root.get(key).and_then(Value::as_array) {
            for item in items {
                let quota = item.get("quotaInfo").unwrap_or(item);
                if let Some(remaining) = remaining_fraction(quota) {
                    let label = parse_bucket_label(item, "Quota");
                    push_remaining_metric(
                        &mut metrics,
                        label,
                        remaining,
                        parse_reset(quota.get("resetTime")),
                    );
                }
            }
        }
    }

    if let Some(configs) = root
        .get("userStatus")
        .and_then(|s| s.get("cascadeModelConfigData"))
        .and_then(|d| d.get("clientModelConfigs"))
        .and_then(Value::as_array)
    {
        for config in configs {
            if let Some(quota) = config.get("quotaInfo") {
                if let Some(remaining) = remaining_fraction(quota) {
                    let label = parse_bucket_label(config, "Quota");
                    push_remaining_metric(
                        &mut metrics,
                        label,
                        remaining,
                        parse_reset(quota.get("resetTime")),
                    );
                }
            }
        }
    }

    metrics.sort_by(|a, b| a.label.cmp(&b.label));
    metrics.dedup_by(|a, b| a.label == b.label);
    // ponytail: keep raw model quota lists small enough for the tray card.
    metrics.truncate(8);
    metrics
}

pub async fn fetch() -> UsageSnapshot {
    if !agy_binary_exists() {
        return UsageSnapshot::not_connected(PROVIDER, DISPLAY_NAME, "AGY CLI not detected");
    }

    let Some(path) = creds_path() else {
        return UsageSnapshot::not_connected(
            PROVIDER,
            DISPLAY_NAME,
            "Could not locate home directory",
        );
    };
    let Ok(raw) = tokio::fs::read_to_string(path).await else {
        return UsageSnapshot::not_connected(PROVIDER, DISPLAY_NAME, "AGY login not found");
    };
    let Ok(creds) = serde_json::from_str::<OAuthCreds>(&raw) else {
        return UsageSnapshot::error(
            PROVIDER,
            DISPLAY_NAME,
            "Failed to parse AGY login credentials",
        );
    };

    let client = super::http_client();
    let token = match access_token(&client, creds).await {
        Ok(token) => token,
        Err(e) => return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, e),
    };
    let loaded = match load_code_assist(&client, &token).await {
        Ok(root) => root,
        Err(e) => return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, e),
    };
    let project = extract_project(&loaded);

    match fetch_quota(&client, &token, project.as_deref()).await {
        Ok(metrics) => UsageSnapshot::ok(PROVIDER, DISPLAY_NAME, metrics),
        Err(e) => {
            let prefix = extract_tier(&loaded)
                .map(|tier| format!("Plan detected: {tier}; "))
                .unwrap_or_default();
            UsageSnapshot::error(PROVIDER, DISPLAY_NAME, format!("{prefix}{e}"))
        }
    }
}
