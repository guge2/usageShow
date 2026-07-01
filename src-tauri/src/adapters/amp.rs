use crate::models::{UsageMetric, UsageSnapshot};
use regex::Regex;
use std::path::PathBuf;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const PROVIDER: &str = "amp";
const DISPLAY_NAME: &str = "Amp";

fn amp_binary() -> PathBuf {
    if let Some(home) = dirs::home_dir() {
        let candidate = home.join(".amp").join("bin").join("amp.exe");
        if candidate.exists() {
            return candidate;
        }
    }
    PathBuf::from("amp")
}

pub async fn fetch() -> UsageSnapshot {
    let bin = amp_binary();
    // The app runs under the Windows GUI subsystem (no console). Bun-compiled
    // binaries like amp.exe probe stdin at startup and crash with
    // STATUS_DLL_INIT_FAILED if it inherits an invalid/absent console handle,
    // so give it an explicit null stdin instead of inheriting ours.
    let mut cmd = tokio::process::Command::new(&bin);
    cmd.arg("usage").stdin(std::process::Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let output = cmd.output().await;

    let output = match output {
        Ok(o) => o,
        Err(_) => {
            return UsageSnapshot::not_connected(PROVIDER, DISPLAY_NAME, "Amp CLI not detected")
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = format!("{stdout}\n{stderr}");

    if !output.status.success() || combined.to_lowercase().contains("not logged in") {
        return UsageSnapshot::not_connected(PROVIDER, DISPLAY_NAME, "Not logged in to Amp");
    }

    let mut metrics = Vec::new();

    // e.g. "Amp Free: $5/$5 remaining (replenishes +$0.21/hour) - https://..."
    if let Ok(re) = Regex::new(r"(?m)^(.+?):\s*\$([\d.]+)/\$([\d.]+)\s*remaining") {
        for cap in re.captures_iter(&stdout) {
            let label = cap[1].trim().to_string();
            let remaining: f64 = cap[2].parse().unwrap_or(0.0);
            let limit: f64 = cap[3].parse().unwrap_or(0.0);
            let used = (limit - remaining).max(0.0);
            let percent = if limit > 0.0 {
                (used / limit) * 100.0
            } else {
                0.0
            };
            metrics.push(UsageMetric {
                label,
                used,
                limit: Some(limit),
                percent: Some(percent),
                unit: "usd".to_string(),
                reset_at: None,
            });
        }
    }

    // e.g. "Individual credits: $0 remaining - https://..." (open-ended, no fixed cap)
    if let Ok(re) = Regex::new(r"(?m)^(.+?):\s*\$([\d.]+)\s*remaining") {
        for cap in re.captures_iter(&stdout) {
            let label = cap[1].trim().to_string();
            if metrics.iter().any(|m: &UsageMetric| m.label == label) {
                continue;
            }
            let remaining: f64 = cap[2].parse().unwrap_or(0.0);
            metrics.push(UsageMetric {
                label,
                used: remaining,
                limit: None,
                percent: None,
                unit: "usd".to_string(),
                reset_at: None,
            });
        }
    }

    if metrics.is_empty() {
        return UsageSnapshot::error(PROVIDER, DISPLAY_NAME, "Failed to parse amp usage output");
    }

    UsageSnapshot::ok(PROVIDER, DISPLAY_NAME, metrics)
}
