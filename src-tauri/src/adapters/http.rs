//! Shared HTTP client for every adapter.
//!
//! Adapters talk to services (Anthropic, OpenAI, Google) that are frequently
//! only reachable through a local proxy. `reqwest` honours the `HTTPS_PROXY` /
//! `HTTP_PROXY` environment variables but — unlike a browser or most CLIs — it
//! does *not* read the Windows system proxy from the registry. A user whose
//! proxy is configured only in Windows' Internet Settings would therefore see
//! every network-backed provider time out, even though their browser and the
//! vendor CLIs work fine. `resolve_proxy` closes that gap.

use crate::models::ProxyMode;
use std::sync::OnceLock;
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// Where the effective proxy came from, for display in Settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxySource {
    None,
    Environment(String),
    SystemRegistry(String),
    Manual(String),
    /// A manual address the HTTP client cannot use. Kept distinct from `None`
    /// so Settings says why the connection is direct instead of implying the
    /// address was accepted.
    Invalid(String),
}

impl ProxySource {
    pub fn url(&self) -> Option<&str> {
        match self {
            ProxySource::None | ProxySource::Invalid(_) => None,
            ProxySource::Environment(u) | ProxySource::SystemRegistry(u) | ProxySource::Manual(u) => {
                Some(u)
            }
        }
    }

    /// Human-readable one-liner shown in the Settings window.
    pub fn describe(&self) -> String {
        match self {
            ProxySource::None => "Direct connection (no proxy)".to_string(),
            ProxySource::Environment(u) => format!("Environment variable: {u}"),
            ProxySource::SystemRegistry(u) => format!("Windows system proxy: {u}"),
            ProxySource::Manual(u) => format!("Manual: {u}"),
            ProxySource::Invalid(u) => {
                format!("Invalid proxy address \"{u}\" - connecting directly")
            }
        }
    }
}

/// Normalise a proxy address into a URL `reqwest` accepts.
///
/// Windows stores `ProxyServer` as a bare `host:port`, and sometimes as a
/// per-scheme list such as `http=1.2.3.4:80;https=1.2.3.4:443`.
pub fn normalize_proxy(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    // Per-scheme list: prefer the https entry, then http, then socks.
    if raw.contains('=') {
        let mut http = None;
        let mut socks = None;
        for part in raw.split(';') {
            let part = part.trim();
            let Some((scheme, addr)) = part.split_once('=') else {
                continue;
            };
            let addr = addr.trim();
            if addr.is_empty() {
                continue;
            }
            match scheme.trim().to_ascii_lowercase().as_str() {
                "https" => return with_scheme(addr, "http"),
                "http" => {
                    http.get_or_insert(addr);
                }
                "socks" => {
                    socks.get_or_insert(addr);
                }
                _ => {}
            }
        }
        // A `socks=` entry is a SOCKS server: speaking HTTP to it cannot work.
        // `socks5h` also resolves DNS through the proxy, which matters when
        // the proxy exists to get around the local network in the first place.
        return http
            .and_then(|addr| with_scheme(addr, "http"))
            .or_else(|| socks.and_then(|addr| with_scheme(addr, "socks5h")));
    }

    with_scheme(raw, "http")
}

/// Prefix `default_scheme://` unless the address already names a scheme.
fn with_scheme(addr: &str, default_scheme: &str) -> Option<String> {
    let addr = addr.trim();
    if addr.is_empty() {
        None
    } else if addr.contains("://") {
        Some(addr.to_string())
    } else {
        Some(format!("{default_scheme}://{addr}"))
    }
}

/// Whether the HTTP client can actually use this proxy URL. `reqwest` only
/// checks the scheme when a request is made, so an unsupported one would
/// otherwise look accepted in Settings and then fail on every fetch.
fn is_usable(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    matches!(
        parsed.scheme(),
        "http" | "https" | "socks4" | "socks4a" | "socks5" | "socks5h"
    ) && parsed.host_str().is_some_and(|host| !host.is_empty())
        && reqwest::Proxy::all(url).is_ok()
}

fn env_proxy() -> Option<String> {
    ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy", "HTTP_PROXY", "http_proxy"]
        .iter()
        .filter_map(|key| std::env::var(key).ok())
        .filter_map(|value| normalize_proxy(&value))
        .find(|url| is_usable(url))
}

/// Read the proxy Windows itself is configured to use (the same setting the
/// Edge/Chrome UI writes). Returns `None` when proxying is disabled.
#[cfg(windows)]
fn system_proxy() -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        .ok()?;
    let enabled: u32 = key.get_value("ProxyEnable").unwrap_or(0);
    if enabled == 0 {
        return None;
    }
    let server: String = key.get_value("ProxyServer").ok()?;
    normalize_proxy(&server).filter(|url| is_usable(url))
}

#[cfg(not(windows))]
fn system_proxy() -> Option<String> {
    None
}

/// Decide which proxy (if any) adapters should use for this fetch cycle.
pub fn resolve_proxy(mode: &ProxyMode, manual_url: &str) -> ProxySource {
    match mode {
        ProxyMode::Off => ProxySource::None,
        ProxyMode::Manual => match normalize_proxy(manual_url) {
            Some(url) if is_usable(&url) => ProxySource::Manual(url),
            Some(_) => ProxySource::Invalid(manual_url.trim().to_string()),
            None => ProxySource::None,
        },
        ProxyMode::Auto => {
            if let Some(url) = env_proxy() {
                ProxySource::Environment(url)
            } else if let Some(url) = system_proxy() {
                ProxySource::SystemRegistry(url)
            } else {
                ProxySource::None
            }
        }
    }
}

fn build_client(source: &ProxySource) -> reqwest::Client {
    let mut builder = reqwest::Client::builder().timeout(REQUEST_TIMEOUT);

    match source.url() {
        Some(url) => {
            // `no_proxy` keeps loopback/intranet hosts direct, matching the
            // behaviour every other tool on the machine has.
            let no_proxy = reqwest::NoProxy::from_env();
            match reqwest::Proxy::all(url) {
                Ok(proxy) => builder = builder.proxy(proxy.no_proxy(no_proxy)),
                Err(_) => builder = builder.no_proxy(),
            }
        }
        // Explicitly disable reqwest's own env-var pickup so "Off" really is off.
        None => builder = builder.no_proxy(),
    }

    builder.build().unwrap_or_else(|_| reqwest::Client::new())
}

/// Cache of the last built client, keyed by the proxy it was built for, so a
/// steady-state refresh loop reuses one connection pool instead of rebuilding
/// TLS state every cycle.
static CACHE: OnceLock<std::sync::Mutex<Option<(ProxySource, reqwest::Client)>>> = OnceLock::new();

/// Shared client for the current proxy configuration.
pub fn client_for(source: &ProxySource) -> reqwest::Client {
    let cell = CACHE.get_or_init(|| std::sync::Mutex::new(None));
    let mut guard = match cell.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };

    if let Some((cached_source, client)) = guard.as_ref() {
        if cached_source == source {
            return client.clone();
        }
    }

    let client = build_client(source);
    *guard = Some((source.clone(), client.clone()));
    client
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_host_port_gets_a_scheme() {
        assert_eq!(
            normalize_proxy("127.0.0.1:7890").as_deref(),
            Some("http://127.0.0.1:7890")
        );
    }

    #[test]
    fn existing_scheme_is_preserved() {
        assert_eq!(
            normalize_proxy("socks5://127.0.0.1:1080").as_deref(),
            Some("socks5://127.0.0.1:1080")
        );
    }

    #[test]
    fn per_scheme_list_prefers_https() {
        assert_eq!(
            normalize_proxy("http=1.1.1.1:80;https=2.2.2.2:443").as_deref(),
            Some("http://2.2.2.2:443")
        );
    }

    #[test]
    fn per_scheme_list_falls_back_to_http() {
        assert_eq!(
            normalize_proxy("ftp=9.9.9.9:21;http=1.1.1.1:80").as_deref(),
            Some("http://1.1.1.1:80")
        );
    }

    #[test]
    fn socks_entry_is_spoken_to_as_socks() {
        assert_eq!(
            normalize_proxy("socks=127.0.0.1:1080").as_deref(),
            Some("socks5h://127.0.0.1:1080")
        );
    }

    #[test]
    fn per_scheme_list_prefers_http_over_socks() {
        assert_eq!(
            normalize_proxy("socks=127.0.0.1:1080;http=127.0.0.1:7890").as_deref(),
            Some("http://127.0.0.1:7890")
        );
    }

    #[test]
    fn manual_socks5_proxy_is_accepted() {
        assert_eq!(
            resolve_proxy(&ProxyMode::Manual, "socks5://127.0.0.1:1080"),
            ProxySource::Manual("socks5://127.0.0.1:1080".to_string())
        );
    }

    #[test]
    fn unusable_manual_address_is_reported_not_hidden() {
        let resolved = resolve_proxy(&ProxyMode::Manual, "bogus://127.0.0.1:1080");
        assert_eq!(resolved, ProxySource::Invalid("bogus://127.0.0.1:1080".to_string()));
        assert_eq!(resolved.url(), None);
    }

    #[test]
    fn blank_is_none() {
        assert_eq!(normalize_proxy("   "), None);
        assert_eq!(normalize_proxy(""), None);
    }

    #[test]
    fn off_mode_never_proxies() {
        assert_eq!(
            resolve_proxy(&ProxyMode::Off, "http://127.0.0.1:7890"),
            ProxySource::None
        );
    }

    #[test]
    fn manual_mode_uses_the_given_url() {
        assert_eq!(
            resolve_proxy(&ProxyMode::Manual, "127.0.0.1:7890"),
            ProxySource::Manual("http://127.0.0.1:7890".to_string())
        );
    }

    #[test]
    fn manual_mode_with_blank_url_is_direct() {
        assert_eq!(resolve_proxy(&ProxyMode::Manual, ""), ProxySource::None);
    }
}
