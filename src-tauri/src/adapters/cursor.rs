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
    let mut stmt = conn.prepare("SELECT value FROM ItemTable WHERE key = 'cursorAuth/accessToken'")?;
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        let value: String = row.get(0)?;
        let trimmed = value.trim().trim_matches('"').to_string();
        return Ok(Some(trimmed));
    }
    Ok(None)
}

pub async fn fetch() -> UsageSnapshot {
    let Some(path) = db_path() else {
        return UsageSnapshot::not_connected(PROVIDER, DISPLAY_NAME, "无法定位用户目录");
    };
    if !path.exists() {
        return UsageSnapshot::not_connected(PROVIDER, DISPLAY_NAME, "未检测到 Cursor 安装");
    }

    let path_clone = path.clone();
    let token_result =
        tokio::task::spawn_blocking(move || read_access_token(&path_clone)).await;

    let token = match token_result {
        Ok(Ok(Some(t))) => t,
        Ok(Ok(None)) => {
            return UsageSnapshot::not_connected(PROVIDER, DISPLAY_NAME, "未登录 Cursor 账号")
        }
        Ok(Err(e)) => {
            return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, format!("读取本地数据库失败: {e}"))
        }
        Err(e) => return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, format!("内部错误: {e}")),
    };

    let client = super::http_client();
    let resp = client
        .get("https://api2.cursor.sh/auth/usage")
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, format!("请求失败: {e}")),
    };

    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, "登录状态已过期，请重新登录 Cursor");
    }
    if !resp.status().is_success() {
        return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, format!("接口返回 {}", resp.status()));
    }

    let body: Result<Value, _> = resp.json().await;
    let Ok(root) = body else {
        return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, "响应格式解析失败");
    };

    let Some(map) = root.as_object() else {
        return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, "响应格式解析失败");
    };

    let mut used_total = 0.0f64;
    let mut limit_total = 0.0f64;
    for (key, value) in map.iter() {
        if key == "startOfMonth" {
            continue;
        }
        let Some(obj) = value.as_object() else { continue };
        let used = obj.get("numRequests").and_then(|v| v.as_f64()).unwrap_or(0.0);
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
            "当前账号为按额度计费模式，该接口暂不支持展示余额",
        );
    }

    let percent = (used_total / limit_total) * 100.0;
    let metrics = vec![UsageMetric {
        label: "月度请求额度".to_string(),
        used: used_total,
        limit: Some(limit_total),
        percent: Some(percent),
        unit: "requests".to_string(),
        reset_at: None,
    }];

    UsageSnapshot::ok(PROVIDER, DISPLAY_NAME, metrics)
}
