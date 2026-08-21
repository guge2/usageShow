//! Run every adapter once and print what it returned.
//!
//! This exercises the real credential files and the real network path without
//! launching the tray app, which makes it the fastest way to check whether a
//! provider is actually readable on this machine:
//!
//! ```text
//! cargo run --example probe              # honour the configured/auto proxy
//! cargo run --example probe -- --no-proxy
//! cargo run --example probe -- --proxy 127.0.0.1:7890
//! ```
//!
//! It prints no tokens — only status, metrics, and messages.

use tauri_app_lib::adapters::{self, http, FetchCtx};
use tauri_app_lib::models::{ProxyMode, UsageStatus};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Trailing bare words select specific providers, e.g. `-- claude codex`.
    let only: Vec<String> = args
        .iter()
        .filter(|a| !a.starts_with("--"))
        .filter(|a| !a.contains(':'))
        .cloned()
        .collect();

    let (mode, manual) = match args.first().map(String::as_str) {
        Some("--no-proxy") => (ProxyMode::Off, String::new()),
        Some("--proxy") => (
            ProxyMode::Manual,
            args.get(1).cloned().unwrap_or_default(),
        ),
        _ => (ProxyMode::Auto, String::new()),
    };

    let proxy = http::resolve_proxy(&mode, &manual);
    println!("Proxy: {}\n", proxy.describe());

    let ctx = FetchCtx {
        client: http::client_for(&proxy),
    };
    let ids: Vec<String> = if only.is_empty() {
        adapters::provider_ids()
    } else {
        only
    };
    println!("Providers: {}\n", ids.join(", "));

    let started = std::time::Instant::now();
    let snapshots = adapters::fetch_enabled(&ids, ctx).await;
    let elapsed = started.elapsed();

    for snapshot in &snapshots {
        let marker = match snapshot.status {
            UsageStatus::Ok => "OK  ",
            UsageStatus::NotConnected => "-   ",
            UsageStatus::NoQuotaApi => "INFO",
            UsageStatus::Error => "FAIL",
        };
        println!("{marker} {:<14}", snapshot.display_name);
        if let Some(message) = &snapshot.message {
            println!("       {message}");
        }
        for metric in &snapshot.metrics {
            let value = match metric.limit {
                Some(limit) => format!("{:.1} / {:.1} {}", metric.used, limit, metric.unit),
                None => format!("{:.1} {}", metric.used, metric.unit),
            };
            println!("       {:<22} {value}", metric.label);
        }
    }

    let ok = snapshots
        .iter()
        .filter(|s| s.status == UsageStatus::Ok)
        .count();
    println!(
        "\n{ok}/{} reporting usage, in {:.1}s",
        snapshots.len(),
        elapsed.as_secs_f64()
    );
}
