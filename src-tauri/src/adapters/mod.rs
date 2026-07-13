pub mod agy;
pub mod amp;
pub mod claude;
pub mod codex;
pub mod cursor;
pub mod factory;
pub mod grok;

use crate::models::UsageSnapshot;

/// Fetch a fresh snapshot from every supported provider, in parallel.
/// Each adapter is fully isolated: a failure in one never affects the others.
pub async fn fetch_all() -> Vec<UsageSnapshot> {
    let (claude, codex, cursor, amp, factory, agy, grok) = tokio::join!(
        claude::fetch(),
        codex::fetch(),
        cursor::fetch(),
        amp::fetch(),
        factory::fetch(),
        agy::fetch(),
        grok::fetch(),
    );
    vec![claude, codex, cursor, amp, factory, agy, grok]
}

/// Shared reqwest client for all adapters (connection pooling, sane timeout).
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}
