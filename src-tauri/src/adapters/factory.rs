use super::FetchCtx;
use super::creds;
use crate::models::{UsageMetric, UsageSnapshot};
use aes_gcm::aead::consts::U16;
use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::aes::Aes256;
use aes_gcm::{AesGcm, Key};
use base64::Engine;
use serde::Deserialize;
use serde_json::Value;

const PROVIDER: &str = "factory";
const DISPLAY_NAME: &str = "Factory Droid";

/// The Droid CLI's `auth.v2.file` is AES-256-GCM encrypted with a 16-byte
/// nonce (not the more common 12-byte one), key stored separately in
/// `auth.v2.key`. Format: `<iv_b64>:<tag_b64>:<ciphertext_b64>`.
type Aes256Gcm16 = AesGcm<Aes256, U16>;

/// WorkOS client id embedded in Droid's own access token claims - used to
/// call WorkOS's standard (publicly documented) refresh-token endpoint.
const WORKOS_CLIENT_ID: &str = "client_01HNM792M5G5G1A2THWPXKFMXB";

#[derive(Deserialize)]
struct FactoryCreds {
    access_token: String,
    refresh_token: String,
    #[serde(default)]
    active_organization_id: Option<String>,
}

#[derive(Deserialize)]
struct WorkosRefreshResponse {
    access_token: String,
}

fn b64_decode(s: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .ok()
}

/// Newer Droid CLI releases write credentials as plain JSON to
/// `auth.encrypted` (despite the name) instead of the legacy AES-GCM
/// `auth.v2.file`/`auth.v2.key` pair. Support both, preferring whichever
/// was written most recently.
fn creds_from_plain_file(path: &std::path::Path) -> Option<FactoryCreds> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn decrypt_creds_v2(dir: &std::path::Path) -> Option<FactoryCreds> {
    let key_raw = std::fs::read_to_string(dir.join("auth.v2.key")).ok()?;
    let key_bytes = b64_decode(&key_raw)?;
    if key_bytes.len() != 32 {
        return None;
    }

    let file_raw = std::fs::read_to_string(dir.join("auth.v2.file")).ok()?;
    let parts: Vec<&str> = file_raw.trim().split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let iv = b64_decode(parts[0])?;
    let tag = b64_decode(parts[1])?;
    let mut combined = b64_decode(parts[2])?;
    if iv.len() != 16 {
        return None;
    }
    combined.extend_from_slice(&tag);

    let key = Key::<Aes256Gcm16>::from_slice(&key_bytes);
    let cipher = Aes256Gcm16::new(key);
    let nonce = GenericArray::<u8, U16>::from_slice(&iv);
    let plaintext = cipher
        .decrypt(
            nonce,
            Payload {
                msg: &combined,
                aad: &[],
            },
        )
        .ok()?;
    serde_json::from_slice(&plaintext).ok()
}

fn mtime(path: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn decrypt_creds() -> Option<FactoryCreds> {
    let home = dirs::home_dir()?;
    let dir = home.join(".factory");
    let plain_path = dir.join("auth.encrypted");
    let v2_path = dir.join("auth.v2.file");

    let prefer_plain = match (mtime(&plain_path), mtime(&v2_path)) {
        (Some(p), Some(v)) => p >= v,
        (Some(_), None) => true,
        _ => false,
    };

    if prefer_plain {
        creds_from_plain_file(&plain_path).or_else(|| decrypt_creds_v2(&dir))
    } else {
        decrypt_creds_v2(&dir).or_else(|| creds_from_plain_file(&plain_path))
    }
}

/// Decodes a JWT's payload segment without verifying the signature - only
/// used locally to read claims for refresh/org-id decisions.
fn jwt_claims(token: &str) -> Option<Value> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let mut seg = parts[1].replace('-', "+").replace('_', "/");
    while seg.len() % 4 != 0 {
        seg.push('=');
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&seg)
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn jwt_exp(token: &str) -> Option<i64> {
    jwt_claims(token)?.get("exp")?.as_i64()
}

/// Plain-JSON `auth.encrypted` credentials don't carry
/// `active_organization_id`; fall back to the `external_org_id` claim
/// embedded in the access token itself. Note: the token's `org_id` claim
/// is the WorkOS org id, which the Factory API rejects (403 "Requested
/// active organization is not accessible by this user") - the API expects
/// `external_org_id` in the `X-Factory-Org-Id` header instead.
fn jwt_org_id(token: &str) -> Option<String> {
    jwt_claims(token)?
        .get("external_org_id")?
        .as_str()
        .map(|s| s.to_string())
}

/// WorkOS's documented refresh-token grant. Never writes the refreshed
/// token back to Factory's own credential files - kept in memory only.
async fn refresh_access_token(client: &reqwest::Client, refresh_token: &str) -> Option<String> {
    let body = serde_json::json!({
        "grant_type": "refresh_token",
        "client_id": WORKOS_CLIENT_ID,
        "refresh_token": refresh_token,
    });
    let resp = client
        .post("https://api.workos.com/user_management/authenticate")
        .json(&body)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json::<WorkosRefreshResponse>()
        .await
        .ok()
        .map(|r| r.access_token)
}

const API_BASE: &str = "https://api.factory.ai";
const RELOGIN: &str = "Login expired - please sign in to Droid CLI again";

/// Rolling windows reported by `GET /api/billing/limits` (what `droid
/// /status` shows): key in the response, label, and nominal length.
const LIMIT_WINDOWS: &[(&str, &str, i64)] = &[
    ("fiveHour", "5h", 5 * 3600),
    ("weekly", "7d", 7 * 86400),
    ("monthly", "30d", 30 * 86400),
];

/// A window's end: an ISO time, or epoch seconds/milliseconds.
fn parse_window_end(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    if let Some(s) = value.as_str() {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
            return Some(dt.timestamp());
        }
        return s
            .parse::<i64>()
            .ok()
            .filter(|n| *n > 0)
            .map(creds::normalize_epoch);
    }
    creds::parse_epoch(Some(value)).filter(|n| *n > 0)
}

/// One pool's windows (standard, or Droid Core). A pool that is entirely
/// idle (nothing used, no window running) is skipped when `skip_idle`.
fn push_pool(
    metrics: &mut Vec<UsageMetric>,
    pool: Option<&Value>,
    prefix: &str,
    skip_idle: bool,
    now: i64,
) {
    let Some(pool) = pool else {
        return;
    };
    let mut windows = Vec::new();
    for (key, label, span) in LIMIT_WINDOWS {
        let Some(w) = pool.get(*key).filter(|w| w.is_object()) else {
            continue;
        };
        let used = w.get("usedPercent").and_then(Value::as_f64).unwrap_or(0.0);
        let reset_at = parse_window_end(w.get("windowEnd")).or_else(|| {
            w.get("secondsRemaining")
                .and_then(Value::as_f64)
                .map(|secs| now + secs as i64)
        });
        windows.push(
            UsageMetric::percent(
                format!("{prefix}{label}"),
                used.clamp(0.0, 100.0),
                reset_at,
            )
            .spanning(Some(*span)),
        );
    }
    let idle = windows
        .iter()
        .all(|m| m.used <= 0.0 && m.reset_at.is_none());
    if skip_idle && idle {
        return;
    }
    metrics.extend(windows);
}

/// Parse `/api/billing/limits`. `None` when the account isn't on rate-limit
/// billing (then the legacy token allowance applies).
fn parse_limits(root: &Value, now: i64) -> Option<Vec<UsageMetric>> {
    if root.get("usesTokenRateLimitsBilling").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    let limits = root.get("limits")?;
    let mut metrics = Vec::new();
    push_pool(&mut metrics, limits.get("standard"), "", false, now);
    push_pool(&mut metrics, limits.get("core"), "Core · ", true, now);
    if metrics.is_empty() {
        return None;
    }
    let extra_allowed = root
        .get("extraUsageAllowed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let extra_cents = root
        .get("extraUsageBalanceCents")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    if extra_allowed && extra_cents > 0.0 {
        metrics.push(UsageMetric::amount("Extra usage", extra_cents / 100.0, "usd"));
    }
    Some(metrics)
}

/// Legacy token-allowance billing (`/api/organization/subscription/usage`).
/// `usedRatio` exceeds 1 once overage kicks in, so the used amount is shown
/// as-is and the percentage is capped for the gauge.
fn push_bucket(
    metrics: &mut Vec<UsageMetric>,
    label: &str,
    bucket: &Value,
    starts_at: Option<i64>,
    reset_at: Option<i64>,
) {
    let used = bucket.get("orgTotalTokensUsed").and_then(Value::as_f64);
    let limit = bucket.get("totalAllowance").and_then(Value::as_f64);
    if let (Some(used), Some(limit)) = (used, limit) {
        if limit > 0.0 {
            let ratio = bucket
                .get("usedRatio")
                .and_then(Value::as_f64)
                .unwrap_or(used / limit);
            metrics.push(UsageMetric {
                label: label.to_string(),
                used,
                limit: Some(limit),
                percent: Some((ratio * 100.0).clamp(0.0, 100.0)),
                unit: "tokens".to_string(),
                reset_at,
                starts_at,
            });
        }
    }
}

fn parse_legacy_usage(root: &Value) -> Vec<UsageMetric> {
    let usage = root.get("usage");
    let reset_at = creds::parse_epoch(usage.and_then(|u| u.get("endDate")));
    let starts_at = creds::parse_epoch(usage.and_then(|u| u.get("startDate")));

    let mut metrics = Vec::new();
    if let Some(standard) = usage.and_then(|u| u.get("standard")) {
        push_bucket(&mut metrics, "Standard", standard, starts_at, reset_at);
    }
    if let Some(premium) = usage.and_then(|u| u.get("premium")) {
        push_bucket(&mut metrics, "Premium", premium, starts_at, reset_at);
    }
    metrics
}

enum ApiError {
    Unauthorized,
    Other(String),
}

async fn get_json(
    client: &reqwest::Client,
    path: &str,
    token: &str,
    org_id: &str,
) -> Result<Value, ApiError> {
    let resp = client
        .get(format!("{API_BASE}{path}"))
        .bearer_auth(token)
        .header("X-Factory-Org-Id", org_id)
        .send()
        .await
        .map_err(|e| ApiError::Other(super::describe_error(&e)))?;
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(ApiError::Unauthorized);
    }
    if !resp.status().is_success() {
        return Err(ApiError::Other(format!("API returned {}", resp.status())));
    }
    resp.json::<Value>()
        .await
        .map_err(|_| ApiError::Other("Failed to parse response".to_string()))
}

pub async fn fetch(ctx: FetchCtx) -> UsageSnapshot {
    let Some(creds) = decrypt_creds() else {
        return UsageSnapshot::not_connected(
            PROVIDER,
            DISPLAY_NAME,
            "Factory Droid login not found",
        );
    };

    let now = chrono::Utc::now().timestamp();
    let mut access_token = creds.access_token.clone();
    let needs_refresh = jwt_exp(&access_token)
        .map(|exp| exp <= now + 30)
        .unwrap_or(true);
    if needs_refresh {
        match refresh_access_token(&ctx.client, &creds.refresh_token).await {
            Some(new_token) => access_token = new_token,
            None => {
                return UsageSnapshot::error(
                    PROVIDER,
                    DISPLAY_NAME,
                    "Login expired - open Droid CLI once to refresh",
                )
            }
        }
    }

    let org_id = creds
        .active_organization_id
        .clone()
        .or_else(|| jwt_org_id(&creds.access_token))
        .unwrap_or_default();
    let email = jwt_claims(&access_token)
        .or_else(|| jwt_claims(&creds.access_token))
        .and_then(|c| {
            c.get("email")
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
        });

    // Current billing: rolling 5h / 7d / 30d windows, as `droid /status` shows.
    // The legacy endpoint below still answers for these accounts, but with a
    // token allowance that no longer governs them (it read 271% used for an
    // account that had used 21% of its 5-hour window).
    let limits_error =
        match get_json(&ctx.client, "/api/billing/limits", &access_token, &org_id).await {
            Ok(root) => match parse_limits(&root, now) {
                Some(metrics) => {
                    return UsageSnapshot::ok(PROVIDER, DISPLAY_NAME, metrics).with_account(email)
                }
                None => None,
            },
            Err(ApiError::Unauthorized) => {
                return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, RELOGIN)
            }
            Err(ApiError::Other(e)) => Some(e),
        };

    // Legacy token-allowance billing.
    match get_json(
        &ctx.client,
        "/api/organization/subscription/usage?useCache=true",
        &access_token,
        &org_id,
    )
    .await
    {
        Ok(root) => {
            let metrics = parse_legacy_usage(&root);
            if metrics.is_empty() {
                let msg = limits_error.unwrap_or_else(|| "No usage data available".to_string());
                return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, msg);
            }
            UsageSnapshot::ok(PROVIDER, DISPLAY_NAME, metrics).with_account(email)
        }
        Err(ApiError::Unauthorized) => UsageSnapshot::error(PROVIDER, DISPLAY_NAME, RELOGIN),
        Err(ApiError::Other(e)) => {
            UsageSnapshot::error(PROVIDER, DISPLAY_NAME, limits_error.unwrap_or(e))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Captured /api/billing/limits response (account on rate-limit billing).
    const LIMITS: &str = r#"{"usesTokenRateLimitsBilling":true,"limits":{"standard":{"fiveHour":{"usedPercent":21,"windowEnd":"2026-10-21T18:22:04.067Z","secondsRemaining":14832},"weekly":{"usedPercent":35,"windowEnd":"2026-10-22T09:55:24.441Z","secondsRemaining":70833},"monthly":{"usedPercent":24,"windowEnd":"2026-10-23T07:13:03.130Z","secondsRemaining":147491}},"core":{"fiveHour":{"usedPercent":0,"windowEnd":null,"secondsRemaining":null},"weekly":{"usedPercent":0,"windowEnd":null,"secondsRemaining":null},"monthly":{"usedPercent":0,"windowEnd":null,"secondsRemaining":null}}},"overagePreference":"extraUsage","canManageOverage":true,"extraUsageBalanceCents":0,"extraUsageAllowed":true}"#;

    /// Captured legacy usage response for the same account: 271% of a token
    /// allowance that no longer governs it.
    const LEGACY: &str = r#"{"usage":{"startDate":1783977208000,"endDate":1801728000000,"standard":{"userTokens":74499656,"orgTotalTokensUsed":54260954,"orgOverageUsed":34260954,"basicAllowance":20000000,"totalAllowance":20000000,"orgOverageLimit":0,"usedRatio":2.7130477},"premium":{"orgTotalTokensUsed":0,"totalAllowance":0,"usedRatio":0}}}"#;

    #[test]
    fn parses_rate_limit_windows() {
        let root: Value = serde_json::from_str(LIMITS).unwrap();
        let m = parse_limits(&root, 1_792_600_000).expect("windows");
        let labels: Vec<&str> = m.iter().map(|x| x.label.as_str()).collect();
        assert_eq!(
            labels,
            ["5h", "7d", "30d"],
            "idle Core pool and zero extra usage are hidden"
        );
        assert_eq!(m[0].percent, Some(21.0));
        let reset = chrono::DateTime::parse_from_rfc3339("2026-10-21T18:22:04.067Z")
            .unwrap()
            .timestamp();
        assert_eq!(m[0].reset_at, Some(reset));
        assert_eq!(m[0].starts_at, Some(reset - 5 * 3600));
        assert_eq!(m[1].percent, Some(35.0));
        assert_eq!(m[2].percent, Some(24.0));
    }

    #[test]
    fn active_core_pool_and_extra_usage_are_shown() {
        let mut root: Value = serde_json::from_str(LIMITS).unwrap();
        root["limits"]["core"]["fiveHour"]["usedPercent"] = serde_json::json!(5);
        root["extraUsageBalanceCents"] = serde_json::json!(1234);
        let m = parse_limits(&root, 0).unwrap();
        let labels: Vec<&str> = m.iter().map(|x| x.label.as_str()).collect();
        assert_eq!(
            labels,
            ["5h", "7d", "30d", "Core · 5h", "Core · 7d", "Core · 30d", "Extra usage"]
        );
        assert!((m[6].used - 12.34).abs() < 1e-9);
    }

    #[test]
    fn legacy_billing_falls_back_and_caps_overage() {
        let limits = serde_json::json!({ "usesTokenRateLimitsBilling": false, "limits": {} });
        assert!(parse_limits(&limits, 0).is_none());
        let m = parse_legacy_usage(&serde_json::from_str(LEGACY).unwrap());
        assert_eq!(m.len(), 1, "premium without allowance is skipped");
        assert_eq!(m[0].percent, Some(100.0));
        assert!((m[0].used - 54_260_954.0).abs() < 1.0);
    }

    #[test]
    fn window_end_formats() {
        let ms = serde_json::json!(1_700_000_000_000i64);
        assert_eq!(parse_window_end(Some(&ms)), Some(1_700_000_000));
        let s = serde_json::json!("1700000000000");
        assert_eq!(parse_window_end(Some(&s)), Some(1_700_000_000));
        assert_eq!(parse_window_end(Some(&Value::Null)), None);
    }
}
