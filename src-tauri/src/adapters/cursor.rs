use crate::models::{UsageMetric, UsageSnapshot};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::path::PathBuf;

const PROVIDER: &str = "cursor";
const DISPLAY_NAME: &str = "Cursor";

fn db_path() -> Option<PathBuf> {
    // On Windows dirs::config_dir() resolves to %APPDATA% (Roaming).
    let config = dirs::config_dir()?;
    Some(
        config
            .join("Cursor")
            .join("User")
            .join("globalStorage")
            .join("state.vscdb"),
    )
}

fn read_access_token(path: &PathBuf) -> Result<Option<String>, rusqlite::Error> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut stmt =
        conn.prepare("SELECT value FROM ItemTable WHERE key = 'cursorAuth/accessToken'")?;
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        let value: String = row.get(0)?;
        let trimmed = value.trim().trim_matches('"').to_string();
        return Ok(Some(trimmed));
    }
    Ok(None)
}

fn parse_millis(value: Option<&Value>) -> Option<i64> {
    let raw = value
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<i64>().ok())
        .or_else(|| value.and_then(Value::as_i64))?;
    Some(if raw > 10_000_000_000 {
        raw / 1000
    } else {
        raw
    })
}

async fn fetch_dashboard_usage(
    client: &reqwest::Client,
    token: &str,
) -> Result<Option<Vec<UsageMetric>>, String> {
    let resp = client
        .post("https://api2.cursor.sh/aiserver.v1.DashboardService/GetCurrentPeriodUsage")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .header("Connect-Protocol-Version", "1")
        .json(&serde_json::json!({}))
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("Login expired - please sign in to Cursor again".to_string());
    }
    if !resp.status().is_success() {
        return Ok(None);
    }

    let root: Value = resp
        .json()
        .await
        .map_err(|_| "Failed to parse response".to_string())?;
    let Some(usage) = root.get("planUsage") else {
        return Ok(None);
    };
    let Some(percent) = usage.get("totalPercentUsed").and_then(Value::as_f64) else {
        return Ok(None);
    };
    Ok(Some(vec![UsageMetric {
        label: "Monthly included usage".to_string(),
        used: percent,
        limit: Some(100.0),
        percent: Some(percent),
        unit: "percent".to_string(),
        reset_at: parse_millis(root.get("billingCycleEnd")),
    }]))
}

async fn fetch_legacy_usage(client: &reqwest::Client, token: &str) -> UsageSnapshot {
    let resp = client
        .get("https://api2.cursor.sh/auth/usage")
        .header("Authorization", format!("Bearer {token}"))
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
            "Login expired - please sign in to Cursor again",
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

    let Some(map) = root.as_object() else {
        return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, "Failed to parse response");
    };

    let mut used_total = 0.0f64;
    let mut limit_total = 0.0f64;
    for (key, value) in map.iter() {
        if key == "startOfMonth" {
            continue;
        }
        let Some(obj) = value.as_object() else {
            continue;
        };
        let used = obj
            .get("numRequests")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let limit = obj
            .get("maxRequestUsage")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        used_total += used;
        limit_total += limit;
    }

    if limit_total <= 0.0 {
        // Accounts on Cursor's newer usage-based/credit pricing no longer report a
        // fixed per-model request cap on this (legacy) endpoint - every model comes
        // back with `maxRequestUsage: null`. There is currently no confirmed public
        // endpoint for the credit balance itself, so surface this distinctly rather
        // than a generic failure.
        return UsageSnapshot::error(
            PROVIDER,
            DISPLAY_NAME,
            "This account uses credit-based billing; this endpoint doesn't support showing balance yet",
        );
    }

    let percent = (used_total / limit_total) * 100.0;
    let metrics = vec![UsageMetric {
        label: "Monthly request quota".to_string(),
        used: used_total,
        limit: Some(limit_total),
        percent: Some(percent),
        unit: "requests".to_string(),
        reset_at: None,
    }];

    UsageSnapshot::ok(PROVIDER, DISPLAY_NAME, metrics)
}

pub async fn fetch() -> UsageSnapshot {
    let Some(path) = db_path() else {
        return UsageSnapshot::not_connected(
            PROVIDER,
            DISPLAY_NAME,
            "Could not locate home directory",
        );
    };
    if !path.exists() {
        return UsageSnapshot::not_connected(
            PROVIDER,
            DISPLAY_NAME,
            "Cursor installation not detected",
        );
    }

    let path_clone = path.clone();
    let token_result = tokio::task::spawn_blocking(move || read_access_token(&path_clone)).await;

    let token = match token_result {
        Ok(Ok(Some(t))) => t,
        Ok(Ok(None)) => {
            return UsageSnapshot::not_connected(PROVIDER, DISPLAY_NAME, "Not logged in to Cursor")
        }
        Ok(Err(e)) => {
            return UsageSnapshot::error(
                PROVIDER,
                DISPLAY_NAME,
                format!("Failed to read local database: {e}"),
            )
        }
        Err(e) => {
            return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, format!("Internal error: {e}"))
        }
    };

    let client = super::http_client();
    match fetch_dashboard_usage(&client, &token).await {
        Ok(Some(metrics)) => return UsageSnapshot::ok(PROVIDER, DISPLAY_NAME, metrics),
        Ok(None) => {}
        Err(e) => return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, e),
    }

    fetch_legacy_usage(&client, &token).await
}
