//! Pure helpers around usage snapshots, kept free of Tauri so they can be
//! unit tested:
//!
//! - deciding which usage alerts are due (once per window cycle)
//! - the tray tooltip's one-line-per-provider summary

use crate::models::{UsageSnapshot, UsageStatus};
use std::collections::HashMap;

/// Two readings of a window's reset this close together are the same cycle
/// (some providers compute reset as "now + seconds left", which drifts).
const SAME_CYCLE_SLACK_SECS: i64 = 15 * 60;

/// Windows' tooltip text is capped at 128 UTF-16 units including the NUL.
const TOOLTIP_MAX_CHARS: usize = 120;

/// One notification to show.
#[derive(Debug, Clone, PartialEq)]
pub struct Alert {
    pub display_name: String,
    pub label: String,
    pub percent: f64,
    pub reset_at: Option<i64>,
}

/// What was already alerted: `provider|label` -> the reset time of the
/// window cycle it was alerted for.
pub type AlertMarks = HashMap<String, Option<i64>>;

fn same_cycle(a: Option<i64>, b: Option<i64>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => (a - b).abs() <= SAME_CYCLE_SLACK_SECS,
        (None, None) => true,
        _ => false,
    }
}

/// The alerts `snapshots` call for at `threshold` percent (0 = off), with
/// `marks` brought up to date: each window alerts once per cycle, and again
/// only after it drops below the threshold or a new cycle starts. Carried-over
/// (stale) and failed readings neither alert nor clear marks, as nothing new
/// is known.
pub fn due_alerts(snapshots: &[UsageSnapshot], marks: &mut AlertMarks, threshold: u8) -> Vec<Alert> {
    let mut out = Vec::new();
    if threshold == 0 {
        marks.clear();
        return out;
    }
    let threshold = f64::from(threshold.min(100));
    for snap in snapshots {
        if snap.status != UsageStatus::Ok || snap.stale {
            continue;
        }
        for metric in &snap.metrics {
            let (Some(percent), Some(_)) = (metric.percent, metric.limit) else {
                continue;
            };
            let key = format!("{}|{}", snap.provider, metric.label);
            if percent < threshold {
                marks.remove(&key);
                continue;
            }
            if let Some(mark) = marks.get(&key) {
                if same_cycle(*mark, metric.reset_at) {
                    continue;
                }
            }
            marks.insert(key, metric.reset_at);
            out.push(Alert {
                display_name: snap.display_name.clone(),
                label: metric.label.clone(),
                percent,
                reset_at: metric.reset_at,
            });
        }
    }
    out
}

/// "AI Usage Monitor\nClaude 5h 28% · 7d 40%\nCodex 7d 46%", trimmed to fit
/// the tray tooltip.
pub fn tooltip_summary(title: &str, snapshots: &[UsageSnapshot], show_remaining: bool) -> String {
    let mut text = title.to_string();
    for snap in snapshots
        .iter()
        .filter(|s| s.status == UsageStatus::Ok && !s.stale)
    {
        let parts: Vec<String> = snap
            .metrics
            .iter()
            .filter_map(|m| m.percent.map(|p| (m, p)))
            .take(3)
            .map(|(m, p)| {
                let shown = if show_remaining { 100.0 - p } else { p };
                format!("{} {:.0}%", m.label, shown.clamp(0.0, 100.0))
            })
            .collect();
        if parts.is_empty() {
            continue;
        }
        let line = format!("\n{} {}", snap.display_name, parts.join(" · "));
        if text.chars().count() + line.chars().count() > TOOLTIP_MAX_CHARS {
            break;
        }
        text.push_str(&line);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::UsageMetric;

    fn ok(provider: &str, pct: f64, reset: Option<i64>) -> UsageSnapshot {
        UsageSnapshot::ok(provider, provider, vec![UsageMetric::percent("5h", pct, reset)])
    }

    #[test]
    fn alerts_once_per_cycle() {
        let mut marks = AlertMarks::new();
        let snaps = vec![ok("claude", 92.0, Some(5000))];
        assert_eq!(due_alerts(&snaps, &mut marks, 90).len(), 1);
        // same cycle, read again (reset drifted a little)
        let snaps = vec![ok("claude", 95.0, Some(5060))];
        assert!(due_alerts(&snaps, &mut marks, 90).is_empty());
        // new cycle
        let snaps = vec![ok("claude", 91.0, Some(5000 + 18000))];
        assert_eq!(due_alerts(&snaps, &mut marks, 90).len(), 1);
        // dropped below, then crossed again in the same cycle
        let snaps = vec![ok("claude", 10.0, Some(5000 + 18000))];
        assert!(due_alerts(&snaps, &mut marks, 90).is_empty());
        let snaps = vec![ok("claude", 90.0, Some(5000 + 18000))];
        assert_eq!(due_alerts(&snaps, &mut marks, 90).len(), 1);
        // off
        assert!(due_alerts(&snaps, &mut marks, 0).is_empty());
    }

    #[test]
    fn stale_and_failed_readings_do_not_alert() {
        let mut marks = AlertMarks::new();
        let mut stale = ok("claude", 99.0, Some(5000));
        stale.stale = true;
        let failed = UsageSnapshot::error("codex", "Codex", "Rate limited, try again later");
        assert!(due_alerts(&[stale, failed], &mut marks, 90).is_empty());
    }

    #[test]
    fn tooltip_fits() {
        let snaps: Vec<UsageSnapshot> = (0..20)
            .map(|i| ok(&format!("provider{i}"), 12.0, None))
            .collect();
        let tip = tooltip_summary("AI Usage Monitor", &snaps, false);
        assert!(tip.chars().count() <= TOOLTIP_MAX_CHARS);
        assert!(tip.contains("provider0 5h 12%"));
        assert!(tooltip_summary("T", &snaps[..1], true).contains("88%"));
    }
}
