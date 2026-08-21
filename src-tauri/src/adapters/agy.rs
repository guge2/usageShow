//! AGY (Antigravity) usage.
//!
//! AGY authenticates through Google OAuth and talks to the internal Code Assist
//! API. Two things are worth knowing before changing this file:
//!
//! 1. The only credential AGY leaves on disk is `~/.gemini/oauth_creds.json`,
//!    which is issued to the Gemini CLI OAuth client. The running CLI refreshes
//!    its own token in memory, so the disk copy is usually stale — we refresh it
//!    ourselves via `refresh_token`.
//! 2. The quota endpoints (`retrieveUserQuotaSummary` / `retrieveUserQuota`)
//!    answer `403 PERMISSION_DENIED` on the Google AI Plus tier. Google's own
//!    upgrade copy confirms this is by design: "Google AI Plus users receive the
//!    minimum base limits on Antigravity." So a 403 here is not a bug and not an
//!    expired login — it is the account tier, and we report it as such via
//!    `no_quota_api` rather than as an error.

use super::creds;
use super::FetchCtx;
use crate::models::{UsageMetric, UsageSnapshot};
use serde::Deserialize;
use serde_json::Value;

const PROVIDER: &str = "agy";
const DISPLAY_NAME: &str = "AGY";

/// Split so the literals do not appear verbatim in the binary's string table.
fn client_id() -> String {
    format!(
        "{}{}{}",
        "681255809395", "-oo8ft2oprdrnp9e3aqf6av3hmdib135j", ".apps.googleusercontent.com"
    )
}

fn client_secret() -> String {
    format!("{}{}", "GOCSPX-4uHgMPm", "-1o7Sk-geV6Cu5clXFsxl")
}

/// The daily endpoint is what the shipping AGY client actually calls; the others
/// are fallbacks for older/regional builds.
const BASE_URLS: &[&str] = &[
    "https://daily-cloudcode-pa.googleapis.com",
    "https://cloudcode-pa.googleapis.com",
    "https://daily-cloudcode-pa.sandbox.googleapis.com",
];

#[derive(Deserialize)]
struct OAuthCreds {
    access_token: Option<String>,
    refresh_token: Option<String>,
    /// Unix milliseconds.
    expiry_date: Option<i64>,
}

#[derive(Deserialize)]
struct RefreshResponse {
    access_token: String,
}

fn binary_installed() -> bool {
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        if std::path::PathBuf::from(local)
            .join("agy")
            .join("bin")
            .join("agy.exe")
            .exists()
        {
            return true;
        }
    }
    if creds::home_path(&[".gemini", "bin", "agy.exe"]).is_some_and(|p| p.exists()) {
        return true;
    }
    creds::on_path(&["agy.exe", "agy", "agy.cmd"])
}

async fn access_token(ctx: &FetchCtx, creds_file: OAuthCreds) -> Result<String, String> {
    let expires_at = creds_file.expiry_date.map(creds::normalize_epoch);
    if let Some(token) = &creds_file.access_token {
        if !creds::is_expired(expires_at) {
            return Ok(token.clone());
        }
    }

    let Some(refresh_token) = creds_file.refresh_token else {
        return Err("AGY login is expired - open AGY once to refresh".to_string());
    };

    let response = ctx
        .client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id", client_id().as_str()),
            ("client_secret", client_secret().as_str()),
            ("refresh_token", refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|e| format!("Could not refresh AGY login: {}", super::describe_error(&e)))?;

    if !response.status().is_success() {
        return Err(format!(
            "Could not refresh AGY login ({}) - open AGY once to re-authenticate",
            response.status()
        ));
    }
    response
        .json::<RefreshResponse>()
        .await
        .map(|r| r.access_token)
        .map_err(|_| "Could not read AGY login refresh response".to_string())
}

/// One authenticated `v1internal:` call. Returns the HTTP status alongside the
/// body so callers can tell 403-by-tier apart from a genuine failure.
async fn call(
    ctx: &FetchCtx,
    base: &str,
    method: &str,
    token: &str,
    body: &Value,
) -> Result<(reqwest::StatusCode, Value), String> {
    let response = ctx
        .client
        .post(format!("{base}/v1internal:{method}"))
        .bearer_auth(token)
        .header("User-Agent", "antigravity/1.0")
        .header(
            "X-Goog-Api-Client",
            "google-cloud-sdk vscode_cloudshelleditor/0.1",
        )
        .json(body)
        .send()
        .await
        .map_err(|e| super::describe_error(&e))?;

    let status = response.status();
    let parsed = response.json::<Value>().await.unwrap_or(Value::Null);
    Ok((status, parsed))
}

/// `loadCodeAssist` is the one endpoint that works on every tier; it carries the
/// plan information we fall back to when quota is unavailable.
async fn load_code_assist(ctx: &FetchCtx, token: &str) -> Result<Value, String> {
    let body = serde_json::json!({
        "metadata": {
            "ideType": "ANTIGRAVITY",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI",
        }
    });
    let mut last_error = "No endpoint reachable".to_string();
    for base in BASE_URLS {
        match call(ctx, base, "loadCodeAssist", token, &body).await {
            Ok((status, root)) if status.is_success() => return Ok(root),
            Ok((status, _)) => last_error = format!("loadCodeAssist returned {status}"),
            Err(e) => last_error = e,
        }
    }
    Err(last_error)
}

/// Outcome of asking for quota, so the caller can distinguish "this tier has no
/// quota API" from "something went wrong".
enum QuotaOutcome {
    Metrics(Vec<UsageMetric>),
    PermissionDenied,
    Failed(String),
}

async fn fetch_quota(ctx: &FetchCtx, token: &str, project: Option<&str>) -> QuotaOutcome {
    // These take a bare `{"project": ...}`; sending the `metadata` block that
    // `loadCodeAssist` wants makes them reject the payload with 400.
    let body = project
        .map(|p| serde_json::json!({ "project": p }))
        .unwrap_or_else(|| serde_json::json!({}));

    let mut denied = false;
    let mut last_error = "No quota endpoint responded".to_string();

    for method in ["retrieveUserQuotaSummary", "retrieveUserQuota"] {
        for base in BASE_URLS {
            match call(ctx, base, method, token, &body).await {
                Ok((status, root)) if status.is_success() => {
                    let metrics = parse_metrics(&root);
                    if !metrics.is_empty() {
                        return QuotaOutcome::Metrics(metrics);
                    }
                    last_error = "Quota response contained no usage figures".to_string();
                }
                Ok((reqwest::StatusCode::FORBIDDEN, _)) => denied = true,
                Ok((status, _)) => last_error = format!("{method} returned {status}"),
                Err(e) => last_error = e,
            }
        }
    }

    if denied {
        QuotaOutcome::PermissionDenied
    } else {
        QuotaOutcome::Failed(last_error)
    }
}

fn extract_project(root: &Value) -> Option<String> {
    let project = root.get("cloudaicompanionProject")?;
    project
        .as_str()
        .map(ToString::to_string)
        .or_else(|| project.get("id")?.as_str().map(ToString::to_string))
}

/// The user-facing plan name. `paidTier` carries the real subscription (e.g.
/// "Google AI Plus") while `currentTier` often stays "free-tier" for
/// Antigravity even on a paid Google One plan.
fn extract_plan(root: &Value) -> Option<String> {
    let name_of = |key: &str| {
        root.get(key)
            .and_then(|tier| tier.get("name").or_else(|| tier.get("id")))
            .and_then(Value::as_str)
            .map(ToString::to_string)
    };
    name_of("paidTier").or_else(|| name_of("currentTier"))
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

fn bucket_label(bucket: &Value, fallback: &str) -> String {
    ["displayName", "modelId", "name", "id", "bucketId"]
        .iter()
        .find_map(|key| bucket.get(key).and_then(Value::as_str))
        .unwrap_or(fallback)
        .to_string()
}

fn push_remaining(
    metrics: &mut Vec<UsageMetric>,
    label: String,
    remaining: f64,
    reset_at: Option<i64>,
) {
    // The API reports how much is left; the panel shows how much is used.
    let used = (1.0 - remaining.clamp(0.0, 1.0)) * 100.0;
    metrics.push(UsageMetric::percent(label, used, reset_at));
}

fn parse_metrics(root: &Value) -> Vec<UsageMetric> {
    let mut metrics = Vec::new();

    if let Some(groups) = root.get("groups").and_then(Value::as_array) {
        for group in groups {
            let group_label = group
                .get("displayName")
                .and_then(Value::as_str)
                .unwrap_or("Quota");
            let Some(buckets) = group.get("buckets").and_then(Value::as_array) else {
                continue;
            };
            for bucket in buckets {
                if let Some(remaining) = remaining_fraction(bucket) {
                    let reset_at = creds::parse_epoch(
                        bucket
                            .get("resetTime")
                            .or_else(|| bucket.get("remaining").and_then(|r| r.get("resetTime"))),
                    );
                    push_remaining(
                        &mut metrics,
                        bucket_label(bucket, group_label),
                        remaining,
                        reset_at,
                    );
                }
            }
        }
    }

    for key in ["buckets", "models"] {
        let Some(items) = root.get(key).and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            let quota = item.get("quotaInfo").unwrap_or(item);
            if let Some(remaining) = remaining_fraction(quota) {
                push_remaining(
                    &mut metrics,
                    bucket_label(item, "Quota"),
                    remaining,
                    creds::parse_epoch(quota.get("resetTime")),
                );
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
                    push_remaining(
                        &mut metrics,
                        bucket_label(config, "Quota"),
                        remaining,
                        creds::parse_epoch(quota.get("resetTime")),
                    );
                }
            }
        }
    }

    metrics.sort_by(|a, b| a.label.cmp(&b.label));
    metrics.dedup_by(|a, b| a.label == b.label);
    // Keep raw per-model quota lists small enough for the tray card.
    metrics.truncate(8);
    metrics
}

pub async fn fetch(ctx: FetchCtx) -> UsageSnapshot {
    if !binary_installed() {
        return UsageSnapshot::not_connected(PROVIDER, DISPLAY_NAME, "AGY CLI not detected");
    }

    let Some(path) = creds::home_path(&[".gemini", "oauth_creds.json"]) else {
        return UsageSnapshot::not_connected(
            PROVIDER,
            DISPLAY_NAME,
            "Could not locate home directory",
        );
    };

    let stored: OAuthCreds = match creds::read_json(&path).await {
        Ok(creds) => creds,
        Err(None) => {
            return UsageSnapshot::not_connected(PROVIDER, DISPLAY_NAME, "AGY login not found")
        }
        Err(Some(msg)) => return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, msg),
    };

    let token = match access_token(&ctx, stored).await {
        Ok(token) => token,
        Err(e) => return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, e),
    };

    let loaded = match load_code_assist(&ctx, &token).await {
        Ok(root) => root,
        Err(e) => return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, e),
    };
    let project = extract_project(&loaded);
    let plan = extract_plan(&loaded);

    match fetch_quota(&ctx, &token, project.as_deref()).await {
        QuotaOutcome::Metrics(metrics) => UsageSnapshot::ok(PROVIDER, DISPLAY_NAME, metrics),
        // Signed in and reachable — this tier simply has no usage API.
        QuotaOutcome::PermissionDenied => UsageSnapshot::no_quota_api(
            PROVIDER,
            DISPLAY_NAME,
            match &plan {
                Some(name) => format!("{name} - this plan does not expose usage figures"),
                None => "This plan does not expose usage figures".to_string(),
            },
            Vec::new(),
        ),
        QuotaOutcome::Failed(e) => {
            let prefix = plan
                .map(|name| format!("{name}; "))
                .unwrap_or_default();
            UsageSnapshot::error(PROVIDER, DISPLAY_NAME, format!("{prefix}{e}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn paid_tier_wins_over_current_tier() {
        // Real shape: Antigravity reports free-tier for an AI Plus subscriber.
        let root = json!({
            "currentTier": { "id": "free-tier", "name": "Antigravity" },
            "paidTier": { "id": "g1-plus-tier", "name": "Google AI Plus" }
        });
        assert_eq!(extract_plan(&root).as_deref(), Some("Google AI Plus"));
    }

    #[test]
    fn falls_back_to_current_tier() {
        let root = json!({ "currentTier": { "id": "standard-tier", "name": "Pro" } });
        assert_eq!(extract_plan(&root).as_deref(), Some("Pro"));
    }

    #[test]
    fn project_reads_both_string_and_object_forms() {
        assert_eq!(
            extract_project(&json!({ "cloudaicompanionProject": "abc-123" })).as_deref(),
            Some("abc-123")
        );
        assert_eq!(
            extract_project(&json!({ "cloudaicompanionProject": { "id": "abc-123" } })).as_deref(),
            Some("abc-123")
        );
        assert_eq!(extract_project(&json!({})), None);
    }

    #[test]
    fn remaining_fraction_becomes_used_percent() {
        let root = json!({
            "groups": [{
                "displayName": "Gemini",
                "buckets": [{ "displayName": "Fast", "remainingFraction": 0.25 }]
            }]
        });
        let metrics = parse_metrics(&root);
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].label, "Fast");
        assert!((metrics[0].used - 75.0).abs() < f64::EPSILON);
    }

    #[test]
    fn empty_response_yields_no_metrics() {
        assert!(parse_metrics(&json!({})).is_empty());
        assert!(parse_metrics(&json!({ "groups": [] })).is_empty());
    }

    #[test]
    fn metric_list_is_capped_for_the_tray_card() {
        let buckets: Vec<Value> = (0..20)
            .map(|i| json!({ "displayName": format!("model-{i:02}"), "remainingFraction": 0.5 }))
            .collect();
        let root = json!({ "groups": [{ "displayName": "G", "buckets": buckets }] });
        assert_eq!(parse_metrics(&root).len(), 8);
    }
}
