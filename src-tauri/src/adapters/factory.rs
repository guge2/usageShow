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
async fn refresh_access_token(refresh_token: &str) -> Option<String> {
    let client = super::http_client();
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

fn push_bucket(metrics: &mut Vec<UsageMetric>, label: &str, bucket: &Value, reset_at: Option<i64>) {
    let used = bucket.get("orgTotalTokensUsed").and_then(|v| v.as_f64());
    let limit = bucket.get("totalAllowance").and_then(|v| v.as_f64());
    let ratio = bucket.get("usedRatio").and_then(|v| v.as_f64());
    if let (Some(used), Some(limit)) = (used, limit) {
        if limit > 0.0 {
            metrics.push(UsageMetric {
                label: label.to_string(),
                used,
                limit: Some(limit),
                percent: Some(ratio.unwrap_or(used / limit) * 100.0),
                unit: "tokens".to_string(),
                reset_at,
            });
        }
    }
}

pub async fn fetch() -> UsageSnapshot {
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
        match refresh_access_token(&creds.refresh_token).await {
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
    let client = super::http_client();
    let resp = client
        .get("https://api.factory.ai/api/organization/subscription/usage?useCache=true")
        .header("Authorization", format!("Bearer {access_token}"))
        .header("X-Factory-Org-Id", &org_id)
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, format!("Request failed: {e}"))
        }
    };
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return UsageSnapshot::error(
            PROVIDER,
            DISPLAY_NAME,
            "Login expired - please sign in to Droid CLI again",
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

    let usage = root.get("usage");
    let reset_at = usage
        .and_then(|u| u.get("endDate"))
        .and_then(|v| v.as_i64())
        .map(|ms| ms / 1000);

    let mut metrics = Vec::new();
    if let Some(standard) = usage.and_then(|u| u.get("standard")) {
        push_bucket(&mut metrics, "Standard Usage (30d)", standard, reset_at);
    }
    if let Some(premium) = usage.and_then(|u| u.get("premium")) {
        let has_allowance = premium
            .get("totalAllowance")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
            > 0.0;
        if has_allowance {
            push_bucket(&mut metrics, "Premium Usage (30d)", premium, reset_at);
        }
    }

    if metrics.is_empty() {
        return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, "No usage data available");
    }

    UsageSnapshot::ok(PROVIDER, DISPLAY_NAME, metrics)
}
