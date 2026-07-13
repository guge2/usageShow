use crate::models::{UsageMetric, UsageSnapshot};
use serde_json::Value;
use std::path::PathBuf;

const PROVIDER: &str = "grok";
const DISPLAY_NAME: &str = "Grok";
const BILLING_URL: &str = "https://grok.com/grok_api_v2.GrokBuildBilling/GetGrokCreditsConfig";

struct GrokCreds {
    access_token: String,
    expires_at: Option<i64>,
}

fn auth_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(home.join(".grok").join("auth.json"))
}

/// Prefer SuperGrok OIDC entries (`https://auth.x.ai::<client-id>`), then any
/// other entry that still has a bearer `key`.
fn load_creds(raw: &str) -> Option<GrokCreds> {
    let root: Value = serde_json::from_str(raw).ok()?;
    let obj = root.as_object()?;

    let mut preferred: Option<GrokCreds> = None;
    let mut fallback: Option<GrokCreds> = None;

    for (key, entry) in obj {
        let Some(token) = entry.get("key").and_then(Value::as_str) else {
            continue;
        };
        if token.is_empty() {
            continue;
        }
        let expires_at = parse_expires_at(entry.get("expires_at"));
        let creds = GrokCreds {
            access_token: token.to_string(),
            expires_at,
        };
        if key.starts_with("https://auth.x.ai") {
            preferred = Some(creds);
        } else if fallback.is_none() {
            fallback = Some(creds);
        }
    }

    preferred.or(fallback)
}

fn parse_expires_at(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    if let Some(ts) = value.as_i64() {
        return Some(if ts > 10_000_000_000 { ts / 1000 } else { ts });
    }
    value
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp())
}

fn is_expired(expires_at: Option<i64>) -> bool {
    match expires_at {
        Some(exp) => chrono::Utc::now().timestamp() >= exp - 30,
        None => false,
    }
}

/// Empty gRPC-web framed message: flag=0, length=0.
fn empty_grpc_web_body() -> Vec<u8> {
    vec![0x00, 0x00, 0x00, 0x00, 0x00]
}

fn read_varint(bytes: &[u8], index: &mut usize) -> Option<u64> {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    while *index < bytes.len() && shift < 64 {
        let byte = bytes[*index];
        *index += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
    }
    None
}

/// Collect fixed32 floats and varints with their field paths for a shallow
/// scan of the billing protobuf (same strategy as CodexBar's Grok fetcher).
struct ProtoScan {
    floats: Vec<(Vec<u64>, f32)>,
    varints: Vec<(Vec<u64>, u64)>,
}

fn scan_protobuf(bytes: &[u8], depth: usize, path: &[u64], scan: &mut ProtoScan) {
    let mut index = 0usize;
    while index < bytes.len() {
        let start = index;
        let Some(key) = read_varint(bytes, &mut index) else {
            break;
        };
        if key == 0 {
            index = start + 1;
            continue;
        }
        let field = key >> 3;
        let wire = key & 0x07;
        let mut field_path = path.to_vec();
        field_path.push(field);

        match wire {
            0 => {
                if let Some(v) = read_varint(bytes, &mut index) {
                    scan.varints.push((field_path, v));
                } else {
                    break;
                }
            }
            1 => {
                if index + 8 > bytes.len() {
                    break;
                }
                index += 8;
            }
            2 => {
                let Some(len) = read_varint(bytes, &mut index) else {
                    break;
                };
                let len = len as usize;
                if index + len > bytes.len() {
                    break;
                }
                if depth < 4 {
                    scan_protobuf(&bytes[index..index + len], depth + 1, &field_path, scan);
                }
                index += len;
            }
            5 => {
                if index + 4 > bytes.len() {
                    break;
                }
                let bits = u32::from_le_bytes([
                    bytes[index],
                    bytes[index + 1],
                    bytes[index + 2],
                    bytes[index + 3],
                ]);
                scan.floats.push((field_path, f32::from_bits(bits)));
                index += 4;
            }
            _ => {
                index = start + 1;
            }
        }
    }
}

/// Extract gRPC-web data frames (flag bit 0x80 marks trailers).
fn grpc_web_data_frames(data: &[u8]) -> Vec<&[u8]> {
    let mut frames = Vec::new();
    let mut index = 0usize;
    while index + 5 <= data.len() {
        let flags = data[index];
        let length = ((data[index + 1] as usize) << 24)
            | ((data[index + 2] as usize) << 16)
            | ((data[index + 3] as usize) << 8)
            | (data[index + 4] as usize);
        let start = index + 5;
        let end = start + length;
        if end > data.len() {
            return Vec::new();
        }
        if flags & 0x80 == 0 {
            frames.push(&data[start..end]);
        }
        index = end;
    }
    frames
}

fn looks_like_protobuf(data: &[u8]) -> bool {
    match data.first() {
        Some(&b) => {
            let field = b >> 3;
            let wire = b & 0x07;
            field > 0 && matches!(wire, 0 | 1 | 2 | 5)
        }
        None => false,
    }
}

fn parse_grpc_status(data: &[u8]) -> Result<(), String> {
    let mut index = 0usize;
    while index + 5 <= data.len() {
        let flags = data[index];
        let length = ((data[index + 1] as usize) << 24)
            | ((data[index + 2] as usize) << 16)
            | ((data[index + 3] as usize) << 8)
            | (data[index + 4] as usize);
        let start = index + 5;
        let end = start + length;
        if end > data.len() {
            break;
        }
        if flags & 0x80 != 0 {
            if let Ok(text) = std::str::from_utf8(&data[start..end]) {
                for line in text.lines() {
                    if let Some(status) = line
                        .strip_prefix("grpc-status:")
                        .or_else(|| line.strip_prefix("grpc-status: "))
                    {
                        let status = status.trim();
                        if status != "0" {
                            let message = text
                                .lines()
                                .find_map(|l| {
                                    l.strip_prefix("grpc-message:")
                                        .or_else(|| l.strip_prefix("grpc-message: "))
                                })
                                .unwrap_or("")
                                .trim();
                            return Err(if message.is_empty() {
                                format!("gRPC status {status}")
                            } else {
                                format!("gRPC status {status}: {message}")
                            });
                        }
                    }
                }
            }
        }
        index = end;
    }
    Ok(())
}

struct BillingSnapshot {
    used_percent: f64,
    reset_at: Option<i64>,
    period_start: Option<i64>,
}

fn parse_unix_ts(v: u64) -> Option<i64> {
    let ts = v as i64;
    if (1_700_000_000..=2_100_000_000).contains(&ts) {
        Some(ts)
    } else {
        None
    }
}

fn parse_billing_response(data: &[u8]) -> Result<BillingSnapshot, String> {
    parse_grpc_status(data)?;

    let mut payloads = grpc_web_data_frames(data);
    if payloads.is_empty() && looks_like_protobuf(data) {
        payloads = vec![data];
    }
    if payloads.is_empty() {
        return Err("Empty billing response".to_string());
    }

    let mut scan = ProtoScan {
        floats: Vec::new(),
        varints: Vec::new(),
    };
    for payload in payloads {
        scan_protobuf(payload, 0, &[], &mut scan);
    }

    // Prefer shallowest field-1 float in [0, 100] (credit_usage_percent).
    let mut candidates: Vec<(usize, f32)> = scan
        .floats
        .iter()
        .filter(|(_, v)| v.is_finite() && *v >= 0.0 && *v <= 100.0)
        .filter(|(path, _)| path.last() == Some(&1))
        .map(|(path, v)| (path.len(), *v))
        .collect();
    candidates.sort_by_key(|(depth, _)| *depth);

    let now = chrono::Utc::now().timestamp();
    // Field paths inside the outer message: period_start=[1,4,1], period_end=[1,5,1].
    let mut period_start: Option<i64> = None;
    let mut preferred_reset: Option<i64> = None;
    let mut any_future_reset: Option<i64> = None;
    for (path, v) in &scan.varints {
        let Some(ts) = parse_unix_ts(*v) else {
            continue;
        };
        match path.as_slice() {
            [1, 4, 1] => {
                period_start = Some(match period_start {
                    Some(prev) => prev.min(ts),
                    None => ts,
                });
            }
            [1, 5, 1] if ts > now => {
                preferred_reset = Some(match preferred_reset {
                    Some(prev) => prev.min(ts),
                    None => ts,
                });
            }
            _ if ts > now => {
                any_future_reset = Some(match any_future_reset {
                    Some(prev) => prev.min(ts),
                    None => ts,
                });
            }
            _ => {}
        }
    }
    let reset_at = preferred_reset.or(any_future_reset);

    let used_percent = if let Some((_, pct)) = candidates.first() {
        f64::from(*pct)
    } else if reset_at.is_some() && scan.floats.is_empty() {
        // proto3 omits zero-valued credit_usage_percent.
        0.0
    } else {
        return Err("Could not parse credit usage percent".to_string());
    };

    Ok(BillingSnapshot {
        used_percent,
        reset_at,
        period_start,
    })
}

fn cycle_label(period_start: Option<i64>, reset_at: Option<i64>) -> String {
    let days = match (period_start, reset_at) {
        (Some(start), Some(end)) if end > start => (end - start) as f64 / 86_400.0,
        (_, Some(end)) => {
            let now = chrono::Utc::now().timestamp();
            ((end - now) as f64 / 86_400.0).abs()
        }
        _ => return "Credits".to_string(),
    };
    if (5.0..=9.0).contains(&days) {
        "Weekly limit".to_string()
    } else if (25.0..=35.0).contains(&days) {
        "Monthly limit".to_string()
    } else {
        "Credits".to_string()
    }
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
        return UsageSnapshot::not_connected(PROVIDER, DISPLAY_NAME, "Grok Build login not found");
    };
    let Some(creds) = load_creds(&raw) else {
        return UsageSnapshot::not_connected(
            PROVIDER,
            DISPLAY_NAME,
            "Grok Build is not logged in - run `grok login`",
        );
    };
    if is_expired(creds.expires_at) {
        return UsageSnapshot::error(
            PROVIDER,
            DISPLAY_NAME,
            "Login expired - open Grok Build once to refresh",
        );
    }

    let client = super::http_client();
    let resp = client
        .post(BILLING_URL)
        .header("Authorization", format!("Bearer {}", creds.access_token))
        .header("Content-Type", "application/grpc-web+proto")
        .header("Accept", "application/grpc-web+proto")
        .header("x-grpc-web", "1")
        .header("x-user-agent", "connect-es/2.1.1")
        .header("Origin", "https://grok.com")
        .header("Referer", "https://grok.com/?_s=usage")
        .body(empty_grpc_web_body())
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, format!("Request failed: {e}"))
        }
    };

    if resp.status() == reqwest::StatusCode::UNAUTHORIZED
        || resp.status() == reqwest::StatusCode::FORBIDDEN
    {
        return UsageSnapshot::error(
            PROVIDER,
            DISPLAY_NAME,
            "Login expired - open Grok Build once to refresh",
        );
    }
    if !resp.status().is_success() {
        return UsageSnapshot::error(
            PROVIDER,
            DISPLAY_NAME,
            format!("API returned {}", resp.status()),
        );
    }

    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => {
            return UsageSnapshot::error(
                PROVIDER,
                DISPLAY_NAME,
                format!("Failed to read response: {e}"),
            )
        }
    };

    let billing = match parse_billing_response(&bytes) {
        Ok(b) => b,
        Err(e) => return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, e),
    };

    let percent = billing.used_percent.clamp(0.0, 100.0);
    let metrics = vec![UsageMetric {
        label: cycle_label(billing.period_start, billing.reset_at),
        used: percent,
        limit: Some(100.0),
        percent: Some(percent),
        unit: "percent".to_string(),
        reset_at: billing.reset_at,
    }];

    UsageSnapshot::ok(PROVIDER, DISPLAY_NAME, metrics)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_captured_billing_frame() {
        // Captured 200 response from GetGrokCreditsConfig (21% used, weekly window).
        let hex = "\
00 00 00 00 68 0A 66 0D 00 00 A8 41 12 00 1A 00 22 0C 08 9B D3 B6 D2 06 \
10 D0 A6 E6 90 01 2A 0C 08 9B C8 DB D2 06 10 D0 A6 E6 90 01 3A 07 08 02 \
15 00 00 60 41 3A 07 08 04 15 00 00 80 40 3A 07 08 01 15 00 00 40 40 42 \
1E 08 02 12 0C 08 9B D3 B6 D2 06 10 D0 A6 E6 90 01 1A 0C 08 9B C8 DB D2 \
06 10 D0 A6 E6 90 01 58 01 62 00 68 01 80 00 00 00 0F 67 72 70 63 2D 73 \
74 61 74 75 73 3A 30 0D 0A";
        let bytes: Vec<u8> = hex
            .split_whitespace()
            .map(|b| u8::from_str_radix(b, 16).unwrap())
            .collect();
        let billing = parse_billing_response(&bytes).expect("parse");
        assert!((billing.used_percent - 21.0).abs() < 0.01);
        assert_eq!(billing.period_start, Some(1_783_474_587));
        assert_eq!(billing.reset_at, Some(1_784_079_387));
        assert_eq!(
            cycle_label(billing.period_start, billing.reset_at),
            "Weekly limit"
        );
    }
}
