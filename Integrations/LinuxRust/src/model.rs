//! View-model derived from CLI JSON, mirroring Integrations/Linux/Shared/Usage.js rows()/costs().
//!
//! Unknown numeric values are never filled with 0: `pace_expected`/`pace_delta`
//! are `f64::NAN` when no pace is reported (JS `null`), `CostEntry.today` is
//! `f64::NAN` when Usage.js yields `null`, and `reset_credits_available` is
//! `-1` when no reset-credit inventory is reported. Renderers show `—`.

use crate::cli::{CostPayload, ExtraRateWindow, RateWindow, UsagePayload};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuotaWindow {
    pub key: String,
    pub label: String,
    pub minutes: i64,
    pub remaining: i64,
    pub resets_at: String,
    pub reset_text: String,
    pub pace: String,
    pub pace_expected: f64,
    pub pace_delta: f64,
    pub scoped: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub provider: String,
    pub account_label: String,
    pub plan: String,
    pub status: String,
    pub status_level: String,
    pub error: String,
    pub updated_at: String,
    pub credits: Option<f64>,
    pub windows: Vec<QuotaWindow>,
    pub details: Vec<DetailSectionVm>,
    pub extra_usage_text: String,
    pub reset_credits_available: i64,
    pub reset_credits_next_expiry: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetailSectionVm {
    pub title: String,
    pub rows: Vec<(String, String, String)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostEntry {
    pub provider: String,
    pub today: f64,
    pub month: f64,
    pub tokens: i64,
    pub provenance: String,
    pub error: String,
    pub coverage: String,
    pub daily: Vec<(String, f64, i64)>, // (date, cost, tokens)
}

pub fn provider_name(id: &str) -> String {
    crate::descriptors::display_name(id)
}

/// Usage.js `remaining()`: null for synthetic placeholders / non-finite values.
fn remaining(window: &RateWindow) -> Option<i64> {
    if !window.used_percent.is_finite() || window.is_synthetic_placeholder == Some(true) {
        return None;
    }
    Some((100.0 - window.used_percent).clamp(0.0, 100.0).round() as i64)
}

/// Usage.js `measured()`: a usable extraRateWindows entry.
fn measured(extra: &ExtraRateWindow) -> bool {
    let id_ok = extra.id.as_deref().is_some_and(|id| !id.trim().is_empty());
    id_ok && extra.usage_known != Some(false) && extra.window.as_ref().and_then(remaining).is_some()
}

fn has_error(error: &Option<String>) -> bool {
    error.as_deref().is_some_and(|e| !e.is_empty())
}

/// Usage.js `displayText()`: cap at 500 chars, redact emails unless identity shown.
fn display_text(value: &str, show_identity: bool) -> String {
    let text: String = value.chars().take(500).collect();
    if show_identity {
        text
    } else {
        redact_emails(&text)
    }
}

/// Replace `[^\s@]+@[^\s@]+` matches with `[hidden email]` (no regex dep).
fn redact_emails(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '@' && i > 0 && !chars[i - 1].is_whitespace() && chars[i - 1] != '@' {
            // Right side must have at least one non-space, non-@ char.
            let right_ok = chars
                .get(i + 1)
                .is_some_and(|c| !c.is_whitespace() && *c != '@');
            if right_ok {
                // Drop the already-emitted left side of the address.
                while out
                    .chars()
                    .last()
                    .is_some_and(|c| !c.is_whitespace() && c != '@')
                {
                    out.pop();
                }
                out.push_str("[hidden email]");
                i += 1;
                while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '@' {
                    i += 1;
                }
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn fmt_num(v: f64) -> String {
    if v == v.trunc() {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// Usage.js `cadenceLabel()`: 43200 -> "30 day", 300 -> "5 hour".
fn cadence_label(minutes: i64) -> String {
    if minutes >= 1440 {
        format!("{} day", fmt_num(minutes as f64 / 1440.0))
    } else if minutes > 0 {
        format!("{} hour", fmt_num(minutes as f64 / 60.0))
    } else {
        String::new()
    }
}

fn antigravity_family(extra: &ExtraRateWindow) -> &'static str {
    let id = extra.id.as_deref().unwrap_or("");
    if !id.starts_with("antigravity-quota-summary-") {
        return "";
    }
    let lower = id.to_lowercase();
    if lower.contains("gemini") {
        return "gemini";
    }
    if lower.contains("3p") || lower.contains("third-party") {
        return "claude-gpt";
    }
    let title = extra.title.as_deref().unwrap_or("").to_lowercase();
    if title.contains("gemini") {
        "gemini"
    } else if title.contains("claude") || title.contains("gpt") {
        "claude-gpt"
    } else {
        ""
    }
}

fn ordinary_gemini_quota(extra: &ExtraRateWindow) -> bool {
    let id = extra
        .id
        .as_deref()
        .unwrap_or("")
        .to_lowercase()
        .replace('_', "-");
    let Some(rest) = id.strip_prefix("antigravity-quota-summary-gemini-") else {
        return false;
    };
    let rest = rest.strip_suffix(" limit").unwrap_or(rest);
    matches!(
        rest,
        "session" | "5h" | "5-hour" | "five hour" | "five-hour" | "weekly"
    )
}

/// Format a `providerCost` object (Usage.js `providerCost()` amounts) as text.
/// Returns "" when all amounts are unknown. Used once the CLI contract grows
/// a `providerCost` field; see unresolved notes.
pub fn format_extra_usage(
    period: &str,
    currency: &str,
    used: Option<f64>,
    limit: Option<f64>,
    balance: Option<f64>,
) -> String {
    if used.is_none() && limit.is_none() && balance.is_none() {
        return String::new();
    }
    let cur = if currency.is_empty() { "$" } else { currency };
    let mut parts: Vec<String> = Vec::new();
    if let Some(b) = balance {
        parts.push(format!("{cur}{b:.2} balance"));
    }
    if let (Some(u), Some(l)) = (used, limit) {
        parts.push(format!("{cur}{u:.2} of {cur}{l:.2}"));
    } else if let Some(u) = used {
        parts.push(format!("{cur}{u:.2} used"));
    } else if let Some(l) = limit {
        parts.push(format!("{cur}{l:.2} limit"));
    }
    if !period.is_empty() {
        parts.push(period.to_string());
    }
    parts.join(" · ")
}

pub fn derive_entries(usage: &[UsagePayload], show_identity: bool) -> Vec<Entry> {
    let now = Utc::now();
    usage
        .iter()
        .map(|entry| derive_entry(entry, show_identity, now))
        .collect()
}

fn derive_entry(entry: &UsagePayload, show_identity: bool, now: DateTime<Utc>) -> Entry {
    let usage = entry.usage.as_ref();
    let identity = usage.and_then(|u| u.identity.as_ref());
    let mut windows: Vec<QuotaWindow> = Vec::new();

    let extras: Vec<&ExtraRateWindow> = usage
        .map(|u| {
            u.extra_rate_windows
                .iter()
                .filter(|e| measured(e))
                .collect()
        })
        .unwrap_or_default();
    let extra_idx = |e: &ExtraRateWindow| extras.iter().position(|x| std::ptr::eq(*x, e));
    let mut represented: Vec<usize> = Vec::new();

    let positional: [(&str, Option<&RateWindow>, Option<&String>); 3] = [
        (
            "primary",
            usage.and_then(|u| u.primary.as_ref()),
            entry
                .rate_window_labels
                .as_ref()
                .and_then(|l| l.primary.as_ref()),
        ),
        (
            "secondary",
            usage.and_then(|u| u.secondary.as_ref()),
            entry
                .rate_window_labels
                .as_ref()
                .and_then(|l| l.secondary.as_ref()),
        ),
        (
            "tertiary",
            usage.and_then(|u| u.tertiary.as_ref()),
            entry
                .rate_window_labels
                .as_ref()
                .and_then(|l| l.tertiary.as_ref()),
        ),
    ];

    for (index, (key, window, supplied)) in positional.iter().enumerate() {
        let Some(window) = window else { continue };
        let Some(left) = remaining(window) else {
            continue;
        };

        // Antigravity: show the family pool that the positional window copies.
        let family = match *key {
            "primary" => "gemini",
            "secondary" => "claude-gpt",
            _ => "",
        };
        let mut copy: Option<&ExtraRateWindow> = None;
        if entry.provider == "antigravity" && !family.is_empty() {
            let mut candidates: Vec<&ExtraRateWindow> = extras
                .iter()
                .copied()
                .filter(|e| {
                    antigravity_family(e) == family
                        && !extra_idx(e).is_some_and(|i| represented.contains(&i))
                        && e.window.as_ref().is_some_and(|w| {
                            w.used_percent == window.used_percent
                                && w.window_minutes == window.window_minutes
                                && w.resets_at == window.resets_at
                        })
                })
                .collect();
            candidates.sort_by(|a, b| {
                ordinary_gemini_quota(b)
                    .cmp(&ordinary_gemini_quota(a))
                    .then_with(|| {
                        a.title
                            .as_deref()
                            .unwrap_or("")
                            .to_lowercase()
                            .cmp(&b.title.as_deref().unwrap_or("").to_lowercase())
                    })
            });
            copy = candidates.first().copied();
        }
        if let Some(c) = copy {
            if let Some(i) = extra_idx(c) {
                represented.push(i);
            }
        }

        let copy_title = copy
            .and_then(|c| c.title.as_deref())
            .map(|t| display_text(t, false).trim().to_string())
            .unwrap_or_default();
        let safe_label = supplied
            .map(|s| display_text(s, false).trim().to_string())
            .unwrap_or_default();
        let label = [copy_title, safe_label, cadence_label(window.window_minutes)]
            .into_iter()
            .find(|s| !s.is_empty())
            .unwrap_or_else(|| ["Session", "Weekly", "Additional"][index].to_string());

        let pace = entry.pace.as_ref().and_then(|p| p.get(*key));
        let resets_at = window.resets_at.clone().unwrap_or_default();
        windows.push(QuotaWindow {
            key: match copy {
                Some(c) => format!("extra:{}", c.id.as_deref().unwrap_or("")),
                None => key.to_string(),
            },
            label,
            minutes: window.window_minutes,
            remaining: left,
            reset_text: reset_text(&resets_at, now),
            resets_at,
            pace: pace.map(|p| p.summary.clone()).unwrap_or_default(),
            pace_expected: pace.map(|p| p.expected_used_percent).unwrap_or(f64::NAN),
            pace_delta: pace.map(|p| p.delta_percent).unwrap_or(f64::NAN),
            scoped: false,
        });
    }

    // Extras last, max 8, marked scoped (Usage.js rows()).
    for (i, extra) in extras.iter().enumerate() {
        if represented.contains(&i) || windows.iter().filter(|w| w.scoped).count() >= 8 {
            continue;
        }
        let Some(sw) = extra.window.as_ref() else {
            continue;
        };
        let Some(left) = remaining(sw) else { continue };
        let label = [
            display_text(extra.title.as_deref().unwrap_or(""), false)
                .trim()
                .to_string(),
            cadence_label(sw.window_minutes),
        ]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or_else(|| "Additional".to_string());
        let resets_at = sw.resets_at.clone().unwrap_or_default();
        windows.push(QuotaWindow {
            key: format!("extra:{}", extra.id.as_deref().unwrap_or("")),
            label,
            minutes: sw.window_minutes,
            remaining: left,
            reset_text: reset_text(&resets_at, now),
            resets_at,
            pace: String::new(),
            pace_expected: f64::NAN,
            pace_delta: f64::NAN,
            scoped: true,
        });
    }

    let account_label = if show_identity {
        identity
            .and_then(|i| i.account_email.clone())
            .or_else(|| usage.and_then(|u| u.account_email.clone()))
            .unwrap_or_default()
    } else {
        String::new()
    };
    let plan = display_text(
        identity
            .and_then(|i| i.login_method.as_deref())
            .or_else(|| usage.and_then(|u| u.login_method.as_deref()))
            .unwrap_or(""),
        false,
    );
    let (status, status_level) = match &entry.status {
        Some(s) => (
            display_text(
                s.description
                    .as_deref()
                    .filter(|d| !d.is_empty())
                    .unwrap_or(&s.indicator),
                false,
            ),
            if s.indicator.is_empty() {
                "unknown".to_string()
            } else {
                s.indicator.clone()
            },
        ),
        None => (String::new(), "unknown".to_string()),
    };
    let details = usage
        .map(|u| {
            u.details
                .iter()
                .take(8)
                .map(|section| DetailSectionVm {
                    title: section.title.clone().unwrap_or_default(),
                    rows: section
                        .rows
                        .iter()
                        .take(24)
                        .map(|row| {
                            (
                                row.label.clone(),
                                display_text(&row.value, show_identity),
                                display_text(
                                    row.secondary_value.as_deref().unwrap_or(""),
                                    show_identity,
                                ),
                            )
                        })
                        .collect(),
                })
                .collect()
        })
        .unwrap_or_default();

    Entry {
        provider: entry.provider.clone(),
        account_label,
        plan,
        status,
        status_level,
        error: if has_error(&entry.error) {
            "Usage unavailable. Check this provider’s CodexBar login/configuration.".to_string()
        } else if windows.is_empty() {
            "No quota windows reported.".to_string()
        } else {
            String::new()
        },
        updated_at: usage.and_then(|u| u.updated_at.clone()).unwrap_or_default(),
        credits: entry.credits.as_ref().map(|c| c.remaining),
        windows,
        details,
        // `providerCost` is not in the cli.rs contract yet (see unresolved).
        extra_usage_text: String::new(),
        reset_credits_available: entry
            .reset_credits
            .as_ref()
            .map(|r| r.available)
            .unwrap_or(-1),
        reset_credits_next_expiry: entry
            .reset_credits
            .as_ref()
            .and_then(|r| r.next_expires_at.clone())
            .unwrap_or_default(),
    }
}

pub fn derive_costs(cost: &[CostPayload]) -> Vec<CostEntry> {
    derive_costs_at(cost, &chrono::Local::now().format("%Y-%m-%d").to_string())
}

/// Usage.js `costs()`: today = daily row dated `today` (`YYYY-MM-DD`), else 0
/// when history coverage is established and there is no error, else unknown.
pub fn derive_costs_at(cost: &[CostPayload], today: &str) -> Vec<CostEntry> {
    cost.iter()
        .map(|row| {
            let start = row.daily.len().saturating_sub(30);
            let daily = &row.daily[start..];
            let today_cost = match daily.iter().rev().find(|d| d.date == today) {
                Some(day) => day.total_cost,
                None if row.history_coverage_is_established && !has_error(&row.error) => 0.0,
                None => f64::NAN,
            };
            // cli.rs currently fails to parse `last30DaysCostUSD` (serde
            // camelCase yields `...Usd`); fall back to the daily sum, which
            // covers the same rolling 30-day window, until that rename lands.
            let month = if row.last30_days_cost_usd != 0.0 {
                row.last30_days_cost_usd
            } else {
                daily.iter().map(|d| d.total_cost).sum::<f64>() + 0.0 // `+ 0.0` turns an empty sum's -0.0 into 0.0
            };
            let tokens = if row.last30_days_tokens != 0 {
                row.last30_days_tokens
            } else {
                daily.iter().map(|d| d.total_tokens).sum()
            };
            CostEntry {
                provider: row.provider.clone(),
                today: today_cost,
                month,
                tokens,
                provenance: row
                    .provenance
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
                error: if has_error(&row.error) {
                    "Local cost history unavailable".to_string()
                } else {
                    String::new()
                },
                coverage: if row.history_coverage_is_established {
                    "Local history".to_string()
                } else {
                    "History may be incomplete".to_string()
                },
                daily: daily
                    .iter()
                    .map(|d| (d.date.clone(), d.total_cost, d.total_tokens))
                    .collect(),
            }
        })
        .collect()
}

/// Usage.js `resetLabel()`: relative reset countdown text.
pub fn reset_text(resets_at: &str, now: DateTime<Utc>) -> String {
    let parsed = DateTime::parse_from_rfc3339(resets_at)
        .map(|d| d.with_timezone(&Utc))
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(resets_at, "%Y-%m-%dT%H:%M:%S")
                .map(|d| d.and_utc())
        });
    let Ok(ts) = parsed else {
        return "Reset time unavailable".to_string();
    };
    let minutes = ((ts - now).num_seconds() as f64 / 60.0).ceil() as i64;
    if minutes <= 0 {
        "Reset due · refresh to update".to_string()
    } else if minutes < 60 {
        format!("Resets in {minutes}m")
    } else if minutes < 1440 {
        format!("Resets in {}h {}m", minutes / 60, minutes % 60)
    } else {
        format!("Resets in {}d {}h", minutes / 1440, (minutes % 1440) / 60)
    }
}

/// Usage.js `summary()` with default bar options (first 2 providers, leading
/// percentage or `—`, `+N` overflow). `quota_used` flips remaining -> used.
pub fn summary(entries: &[Entry], quota_used: bool) -> String {
    let shown = &entries[..entries.len().min(2)];
    let label = shown
        .iter()
        .map(|e| {
            let tag = match e.provider.as_str() {
                "codex" => "CX".to_string(),
                "claude" => "CL".to_string(),
                other => other.to_string(),
            };
            let text = match e.windows.first() {
                Some(w) => {
                    let v = if quota_used {
                        100 - w.remaining
                    } else {
                        w.remaining
                    };
                    format!("{v}%")
                }
                None => "—".to_string(),
            };
            format!("{tag} {text}")
        })
        .collect::<Vec<_>>()
        .join("  ·  ");
    if entries.len() > shown.len() {
        format!("{label}  +{}", entries.len() - shown.len())
    } else {
        label
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn fixture_entries(show_identity: bool) -> Vec<Entry> {
        let rows =
            crate::cli::parse_usage(include_bytes!("../tests/fixtures/usage-codex.json")).unwrap();
        derive_entries(&rows, show_identity)
    }

    #[test]
    fn fixture_codex_remaining_99() {
        let e = fixture_entries(true);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].provider, "codex");
        assert_eq!(e[0].windows.len(), 1);
        assert_eq!(e[0].windows[0].remaining, 99);
        assert_eq!(e[0].windows[0].key, "primary");
        assert_eq!(e[0].windows[0].label, "Session");
        assert_eq!(e[0].windows[0].minutes, 43200);
        assert_eq!(e[0].plan, "free");
        assert_eq!(e[0].reset_credits_available, 0);
        assert!(!e[0].windows[0].pace.is_empty());
        assert_eq!(e[0].windows[0].pace_expected, 49.0);
        assert_eq!(e[0].windows[0].pace_delta, -48.0);
        assert_eq!(e[0].error, "");
    }

    #[test]
    fn email_hidden_when_identity_off() {
        assert_eq!(fixture_entries(false)[0].account_label, "");
        assert_eq!(fixture_entries(true)[0].account_label, "<email>");
    }

    #[test]
    fn reset_text_variants() {
        let now = Utc.with_ymd_and_hms(2026, 11, 8, 12, 0, 0).unwrap();
        assert_eq!(reset_text("", now), "Reset time unavailable");
        assert_eq!(reset_text("garbage", now), "Reset time unavailable");
        assert_eq!(
            reset_text("2026-11-08T11:00:00Z", now),
            "Reset due · refresh to update"
        );
        assert_eq!(reset_text("2026-11-08T12:30:00Z", now), "Resets in 30m");
        assert_eq!(reset_text("2026-11-08T13:30:00Z", now), "Resets in 1h 30m");
        assert_eq!(reset_text("2026-11-10T13:00:00Z", now), "Resets in 2d 1h");
    }

    #[test]
    fn summary_uses_first_window_and_tag() {
        let e = fixture_entries(true);
        assert_eq!(summary(&e, false), "CX 99%");
        assert_eq!(summary(&e, true), "CX 1%");
        assert_eq!(summary(&[], false), "");
    }

    #[test]
    fn costs_today_and_month() {
        let rows =
            crate::cli::parse_cost(include_bytes!("../tests/fixtures/cost-both.json")).unwrap();
        let costs = derive_costs_at(&rows, "2026-11-08");
        assert_eq!(costs.len(), 2);
        assert_eq!(costs[1].provider, "claude");
        assert!((costs[1].today - 83.20543440000006).abs() < 1e-6);
        assert!((costs[1].month - 698.2726216650002).abs() < 1e-6);
        assert_eq!(costs[1].tokens, 2267283594);
        assert_eq!(costs[1].provenance, "listPriceEstimate");
        assert_eq!(costs[1].coverage, "Local history");
        assert_eq!(costs[1].daily.len(), 14);
        assert_eq!(derive_costs_at(&rows, "2026-11-02")[1].today, 0.0);
        assert_eq!(costs[0].today, 0.0);
    }

    #[test]
    fn extra_windows_scoped_and_capped() {
        let json = r#"[{"provider":"claude","usage":{"updatedAt":"2026-11-08T00:00:00Z",
          "primary":{"usedPercent":50,"windowMinutes":300},
          "extraRateWindows":[
            {"id":"a","title":"Sonnet only","usageKnown":true,"window":{"usedPercent":80,"windowMinutes":10080}},
            {"id":"","title":"bad","window":{"usedPercent":1,"windowMinutes":60}},
            {"id":"b","usageKnown":false,"window":{"usedPercent":1,"windowMinutes":60}}]}}]"#;
        let rows = crate::cli::parse_usage(json.as_bytes()).unwrap();
        let e = derive_entries(&rows, false);
        assert_eq!(e[0].windows.len(), 2);
        assert_eq!(e[0].windows[0].label, "5 hour");
        assert_eq!(e[0].windows[1].key, "extra:a");
        assert_eq!(e[0].windows[1].remaining, 20);
        assert!(e[0].windows[1].scoped);
        assert!(!e[0].windows[0].scoped);
    }

    #[test]
    fn synthetic_placeholder_skipped_and_error_text() {
        let json = r#"[{"provider":"claude","usage":{"primary":{"usedPercent":0,"windowMinutes":300,"isSyntheticPlaceholder":true}}}]"#;
        let rows = crate::cli::parse_usage(json.as_bytes()).unwrap();
        let e = derive_entries(&rows, false);
        assert!(e[0].windows.is_empty());
        assert_eq!(e[0].error, "No quota windows reported.");
        assert_eq!(e[0].status_level, "unknown");
        assert_eq!(e[0].reset_credits_available, -1);
    }

    #[test]
    fn identity_redaction_in_details_and_extra_usage() {
        assert_eq!(
            display_text("mail a@b.com now", false),
            "mail [hidden email] now"
        );
        assert_eq!(display_text("mail a@b.com now", true), "mail a@b.com now");
        assert_eq!(format_extra_usage("", "USD", None, None, None), "");
        assert_eq!(
            format_extra_usage("Nov", "USD", Some(5.0), Some(50.0), None),
            "USD5.00 of USD50.00 · Nov"
        );
    }
}
