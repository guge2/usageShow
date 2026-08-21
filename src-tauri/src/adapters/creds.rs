//! Small helpers shared by the credential-reading adapters.
//!
//! Every provider stores its login somewhere under the home directory and
//! stamps it with an expiry in its own favourite format (seconds, milliseconds,
//! or RFC 3339). Before this module each adapter carried its own copy of that
//! parsing; the copies had drifted, and none of them were tested.

use std::path::PathBuf;

/// Tokens are treated as expired slightly early so a fetch cannot start with a
/// token that dies mid-flight.
const EXPIRY_SKEW_SECS: i64 = 30;

/// Path to `~/<segments...>`, or `None` when the home directory is unknown.
pub fn home_path(segments: &[&str]) -> Option<PathBuf> {
    let mut path = dirs::home_dir()?;
    for segment in segments {
        path.push(segment);
    }
    Some(path)
}

/// Read and deserialize a JSON credential file.
///
/// Returns `Err(None)` when the file simply isn't there (provider not
/// installed / never logged in) and `Err(Some(msg))` when it exists but cannot
/// be parsed — the two cases map to different `UsageSnapshot` states.
pub async fn read_json<T: serde::de::DeserializeOwned>(
    path: &PathBuf,
) -> Result<T, Option<String>> {
    let raw = tokio::fs::read_to_string(path).await.map_err(|_| None)?;
    serde_json::from_str(&raw).map_err(|e| Some(format!("Failed to parse credentials: {e}")))
}

/// Normalise a timestamp that may be Unix seconds, Unix milliseconds, or an
/// RFC 3339 string, into Unix seconds.
pub fn parse_epoch(value: Option<&serde_json::Value>) -> Option<i64> {
    let value = value?;
    if let Some(n) = value.as_i64() {
        return Some(normalize_epoch(n));
    }
    if let Some(n) = value.as_f64() {
        return Some(normalize_epoch(n as i64));
    }
    value
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp())
}

/// Values past ~year 2286 in seconds are really milliseconds.
pub fn normalize_epoch(value: i64) -> i64 {
    if value > 10_000_000_000 {
        value / 1000
    } else {
        value
    }
}

/// Whether a Unix-seconds expiry has passed (with a small safety margin).
/// A credential with no stated expiry is treated as still valid.
pub fn is_expired(expires_at_secs: Option<i64>) -> bool {
    match expires_at_secs {
        Some(exp) => chrono::Utc::now().timestamp() >= exp - EXPIRY_SKEW_SECS,
        None => false,
    }
}

/// Parse an RFC 3339 timestamp into Unix seconds.
pub fn parse_rfc3339(value: &Option<String>) -> Option<i64> {
    value
        .as_ref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp())
}

/// Whether any of `names` resolves to an executable on `PATH`.
pub fn on_path(names: &[&str]) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path)
        .any(|dir| names.iter().any(|name| dir.join(name).exists()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn milliseconds_are_converted_to_seconds() {
        assert_eq!(normalize_epoch(1_779_243_134_572), 1_779_243_134);
    }

    #[test]
    fn seconds_pass_through_unchanged() {
        assert_eq!(normalize_epoch(1_779_243_134), 1_779_243_134);
    }

    #[test]
    fn parses_numeric_and_string_timestamps_alike() {
        assert_eq!(parse_epoch(Some(&json!(1_700_000_000))), Some(1_700_000_000));
        assert_eq!(
            parse_epoch(Some(&json!(1_700_000_000_000i64))),
            Some(1_700_000_000)
        );
        assert_eq!(
            parse_epoch(Some(&json!("2023-11-14T22:13:20Z"))),
            Some(1_700_000_000)
        );
    }

    #[test]
    fn unparseable_timestamps_are_none() {
        assert_eq!(parse_epoch(Some(&json!("not a date"))), None);
        assert_eq!(parse_epoch(Some(&json!(null))), None);
        assert_eq!(parse_epoch(None), None);
    }

    #[test]
    fn expiry_in_the_past_is_expired() {
        assert!(is_expired(Some(1_000_000_000)));
    }

    #[test]
    fn expiry_far_in_the_future_is_not_expired() {
        let future = chrono::Utc::now().timestamp() + 3600;
        assert!(!is_expired(Some(future)));
    }

    #[test]
    fn expiry_inside_the_skew_window_counts_as_expired() {
        let just_about_now = chrono::Utc::now().timestamp() + 5;
        assert!(is_expired(Some(just_about_now)));
    }

    #[test]
    fn missing_expiry_is_never_expired() {
        assert!(!is_expired(None));
    }
}
