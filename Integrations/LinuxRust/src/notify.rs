//! Desktop notifications on quota transitions, a port of
//! `Integrations/Linux/Shared/Notifications.js` `transition()`: threshold
//! crossing, observed reset, and provider status change. Silent on first
//! load, on errors, and for providers reported more than once (ambiguous
//! accounts).

use crate::model::Entry;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub kind: &'static str, // "low" | "reset" | "status"
    pub provider: String,
    pub message: String,
}

#[derive(Clone, Default)]
struct WindowState {
    remaining: i64,
    reset: String,
}

fn failed(entry: &Entry) -> bool {
    // Usage.js `failed` is the CLI-level error, which the Rust model renders
    // as the "Usage unavailable..." display string; "No quota windows
    // reported." is not a failure (and carries no windows to alert on).
    entry.error.starts_with("Usage unavailable")
}

fn provider_counts(entries: &[Entry]) -> HashMap<&str, usize> {
    let mut counts = HashMap::new();
    for row in entries {
        *counts.entry(row.provider.as_str()).or_default() += 1;
    }
    counts
}

fn window_states(entries: &[Entry]) -> HashMap<String, WindowState> {
    let counts = provider_counts(entries);
    let mut states = HashMap::new();
    for row in entries {
        if counts[row.provider.as_str()] != 1 || failed(row) {
            continue;
        }
        for window in &row.windows {
            states.insert(
                format!("{}/{}", row.provider, window.key),
                WindowState {
                    remaining: window.remaining,
                    reset: window.resets_at.clone(),
                },
            );
        }
    }
    states
}

fn status_states(entries: &[Entry]) -> HashMap<String, String> {
    let counts = provider_counts(entries);
    let mut states = HashMap::new();
    for row in entries {
        if counts[row.provider.as_str()] != 1 || failed(row) {
            continue;
        }
        if is_tracked_level(&row.status_level) {
            states.insert(format!("{}/status", row.provider), row.status_level.clone());
        }
    }
    states
}

fn is_tracked_level(level: &str) -> bool {
    ["none", "minor", "major", "critical", "maintenance"].contains(&level)
}

/// Pure transition detector (port of Notifications.js). `prev`/`next` are the
/// last two successful entry lists; an empty `prev` (first load) or a failed
/// `prev` row yields no events for that provider.
pub fn transition_events(prev: &[Entry], next: &[Entry], threshold: u32) -> Vec<Notice> {
    let limit: i64 = if threshold == 0 {
        10
    } else {
        threshold.clamp(1, 99) as i64
    };
    let counts = provider_counts(next);
    let prev_windows = window_states(prev);
    let prev_status = status_states(prev);
    let mut events = Vec::new();
    for row in next {
        if counts[row.provider.as_str()] != 1 || failed(row) {
            continue;
        }
        for window in &row.windows {
            let key = format!("{}/{}", row.provider, window.key);
            let Some(old) = prev_windows.get(&key) else {
                continue;
            };
            if old.remaining > limit && window.remaining <= limit {
                events.push(Notice {
                    kind: "low",
                    provider: row.provider.clone(),
                    message: format!("{}: {}% quota remaining.", window.label, window.remaining),
                });
            } else if !old.reset.is_empty()
                && !window.resets_at.is_empty()
                && old.reset != window.resets_at
                && window.remaining > old.remaining
            {
                events.push(Notice {
                    kind: "reset",
                    provider: row.provider.clone(),
                    message: format!(
                        "{} quota reset · {}% remaining.",
                        window.label, window.remaining
                    ),
                });
            }
        }
        if is_tracked_level(&row.status_level) {
            let key = format!("{}/status", row.provider);
            if let Some(old) = prev_status.get(&key) {
                if *old != row.status_level {
                    let message = if row.status_level == "none" {
                        "Service has recovered.".to_string()
                    } else {
                        format!("Service status changed: {}.", row.status_level)
                    };
                    events.push(Notice {
                        kind: "status",
                        provider: row.provider.clone(),
                        message,
                    });
                }
            }
        }
    }
    events
}

/// Detect transitions and post them via `org.freedesktop.Notifications`
/// (notify-rust), matching the Qt app's 8-second "CodexBar · <provider>".
pub fn maybe_notify(prev: &[Entry], next: &[Entry], threshold: u32) {
    for event in transition_events(prev, next, threshold) {
        let _ = notify_rust::Notification::new()
            .appname("CodexBar")
            .summary(&format!("CodexBar · {}", event.provider))
            .body(&event.message)
            .icon("codexbar")
            .timeout(notify_rust::Timeout::Milliseconds(8000))
            .show();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Entry, QuotaWindow};

    fn win(key: &str, remaining: i64, resets: &str) -> QuotaWindow {
        QuotaWindow {
            key: key.into(),
            label: "Session".into(),
            minutes: 300,
            remaining,
            resets_at: resets.into(),
            reset_text: String::new(),
            pace: String::new(),
            pace_expected: 0.0,
            pace_delta: 0.0,
            scoped: false,
        }
    }
    fn entry(remaining: i64, resets: &str) -> Entry {
        Entry {
            provider: "codex".into(),
            account_label: String::new(),
            plan: String::new(),
            status: String::new(),
            status_level: "none".into(),
            error: String::new(),
            updated_at: String::new(),
            credits: None,
            windows: vec![win("primary", remaining, resets)],
            details: vec![],
            extra_usage_text: String::new(),
            reset_credits_available: 0,
            reset_credits_next_expiry: String::new(),
        }
    }

    #[test]
    fn notify_first_load_silent() {
        assert!(transition_events(&[], &[entry(5, "2026-01-01T00:00:00Z")], 10).is_empty());
    }

    #[test]
    fn notify_threshold_crossing() {
        let prev = vec![entry(50, "2026-01-01T00:00:00Z")];
        let ev = transition_events(&prev, &[entry(10, "2026-01-01T00:00:00Z")], 10);
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].kind, "low");
        assert_eq!(ev[0].provider, "codex");
        assert_eq!(ev[0].message, "Session: 10% quota remaining.");
        // already low -> no repeat
        let ev = transition_events(
            &[entry(10, "2026-01-01T00:00:00Z")],
            &[entry(5, "2026-01-01T00:00:00Z")],
            10,
        );
        assert!(ev.is_empty(), "{ev:?}");
    }

    #[test]
    fn notify_reset_observed() {
        let prev = vec![entry(3, "2026-01-01T00:00:00Z")];
        let ev = transition_events(&prev, &[entry(100, "2026-01-08T00:00:00Z")], 10);
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].kind, "reset");
        assert_eq!(ev[0].message, "Session quota reset · 100% remaining.");
    }

    #[test]
    fn notify_status_transition() {
        let mut next = entry(50, "2026-01-01T00:00:00Z");
        next.status_level = "major".into();
        let ev = transition_events(&[entry(50, "2026-01-01T00:00:00Z")], &[next], 10);
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].kind, "status");
        assert_eq!(ev[0].message, "Service status changed: major.");
        // recovery
        let mut prev = entry(50, "2026-01-01T00:00:00Z");
        prev.status_level = "major".into();
        let ev = transition_events(&[prev], &[entry(50, "2026-01-01T00:00:00Z")], 10);
        assert_eq!(ev[0].message, "Service has recovered.");
        // unknown level never tracked
        let mut unknown = entry(50, "2026-01-01T00:00:00Z");
        unknown.status_level = "unknown".into();
        assert!(transition_events(&[entry(50, "2026-01-01T00:00:00Z")], &[unknown], 10).is_empty());
    }

    #[test]
    fn notify_duplicate_provider_and_errors_silent() {
        let prev = vec![entry(50, "2026-01-01T00:00:00Z")];
        let dup = vec![
            entry(5, "2026-01-01T00:00:00Z"),
            entry(5, "2026-01-01T00:00:00Z"),
        ];
        assert!(transition_events(&prev, &dup, 10).is_empty());
        let mut failed = entry(5, "2026-01-01T00:00:00Z");
        failed.error = "Usage unavailable".into();
        assert!(transition_events(&prev, &[failed], 10).is_empty());
        // after a failed refresh the next good reading is a fresh baseline
        let mut failed_prev = entry(50, "2026-01-01T00:00:00Z");
        failed_prev.error = "Usage unavailable".into();
        assert!(
            transition_events(&[failed_prev], &[entry(5, "2026-01-01T00:00:00Z")], 10).is_empty()
        );
    }
}
