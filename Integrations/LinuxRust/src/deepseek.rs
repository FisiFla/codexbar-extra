//! DeepSeek peak/off-peak price clock, port of
//! `Sources/CodexBarCore/Providers/DeepSeek/DeepSeekPriceSchedule.swift`
//! + `DeepSeekPriceClockPresentation.swift`. Pure compute from a UTC instant:
//! peak Mon-Fri 01:00-04:00 and 06:00-10:00 UTC, off-peak (50% off) otherwise,
//! whole weekend off-peak. Chinese public holidays are not modelled (as upstream).

use chrono::{DateTime, Datelike, Duration, NaiveDate, Timelike, Utc, Weekday};

pub struct PriceClock {
    pub is_peak: bool,
    /// `H:MM:SS` remaining in the current window (Swift `countdownDigitsText`).
    pub countdown: String,
    /// Current window + next 3: (tag "Peak"/"Off-peak", UTC time range, is_peak).
    pub windows: Vec<(String, String, bool)>,
    /// `PEAK PRICING` / `OFF-PEAK PRICING (50% OFF)`.
    pub badge: String,
    /// `→ Peak starts HH:mm UTC`-style next-transition caption.
    pub next_transition: String,
}

fn is_peak_at(t: DateTime<Utc>) -> bool {
    let weekday = matches!(
        t.weekday(),
        Weekday::Mon | Weekday::Tue | Weekday::Wed | Weekday::Thu | Weekday::Fri
    );
    weekday && ((1..4).contains(&t.hour()) || (6..10).contains(&t.hour()))
}

fn at(day: NaiveDate, hour: u32) -> DateTime<Utc> {
    day.and_hms_opt(hour, 0, 0).expect("valid hour").and_utc()
}

/// Sorted (instant, tier-starting-there) transitions around `now`, mirroring
/// Swift `transitions(around:)` (scan -8..=+9 days, adjacent same-tier deduped).
fn transitions(now: DateTime<Utc>) -> Vec<(DateTime<Utc>, bool)> {
    let anchor = now.date_naive();
    let mut out: Vec<(DateTime<Utc>, bool)> = Vec::new();
    for offset in -8..=9 {
        let day = anchor + Duration::days(offset);
        if matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
            continue;
        }
        out.push((at(day, 1), true));
        out.push((at(day, 4), false));
        out.push((at(day, 6), true));
        out.push((at(day, 10), false));
    }
    out.sort_by_key(|(t, _)| *t);
    out.dedup_by(|next, prev| next.1 == prev.1);
    out
}

fn range_text(start: DateTime<Utc>, end: DateTime<Utc>) -> String {
    if start.date_naive() == end.date_naive() {
        format!("{} — {}", start.format("%H:%M"), end.format("%H:%M"))
    } else {
        format!("{} — {}", start.format("%a %H:%M"), end.format("%a %H:%M"))
    }
}

pub fn price_clock(now: DateTime<Utc>) -> PriceClock {
    let trans = transitions(now);
    // Active transition = latest <= now; the ±8/9-day scan always provides one.
    let active = trans.iter().rposition(|(t, _)| *t <= now).unwrap_or(0);
    let is_peak = is_peak_at(now);
    let current_end = trans.get(active + 1).map(|(t, _)| *t).unwrap_or(now);
    let total = (current_end - now).num_seconds().max(0);
    let countdown = format!(
        "{}:{:02}:{:02}",
        total / 3600,
        (total % 3600) / 60,
        total % 60
    );

    let mut windows = Vec::new();
    for i in active..(active + 4).min(trans.len().saturating_sub(1)) {
        let (start, peak) = trans[i];
        let end = trans[i + 1].0;
        windows.push((
            if peak {
                "Peak".to_string()
            } else {
                "Off-peak".to_string()
            },
            range_text(start, end),
            peak,
        ));
    }

    let (next_start, next_peak) = trans
        .get(active + 1)
        .copied()
        .unwrap_or((current_end, !is_peak));
    PriceClock {
        is_peak,
        countdown,
        windows,
        badge: if is_peak {
            "PEAK PRICING"
        } else {
            "OFF-PEAK PRICING (50% OFF)"
        }
        .to_string(),
        next_transition: format!(
            "→ {} starts {} UTC",
            if next_peak { "Peak" } else { "Off-peak" },
            next_start.format("%H:%M")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> PriceClock {
        price_clock(Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap())
    }
    #[test]
    fn monday_peak_windows() {
        // 2026-11-09 is a Monday
        assert!(at(2026, 11, 9, 2, 0).is_peak);
        assert!(at(2026, 11, 9, 7, 30).is_peak);
        assert!(!at(2026, 11, 9, 5, 0).is_peak);
        assert!(!at(2026, 11, 9, 0, 30).is_peak);
        assert!(!at(2026, 11, 9, 10, 0).is_peak);
        assert!(at(2026, 11, 9, 1, 0).is_peak);
    }
    #[test]
    fn weekend_off_peak_and_countdown() {
        // 2026-11-07 Saturday, 2026-11-08 Sunday
        assert!(!at(2026, 11, 7, 2, 0).is_peak);
        assert!(!at(2026, 11, 8, 8, 0).is_peak);
        let c = at(2026, 11, 9, 2, 0); // peak until 04:00 -> 2h left
        assert_eq!(c.countdown, "2:00:00");
        assert_eq!(c.windows.len(), 4);
        assert_eq!(c.windows[0].0, "Peak");
        assert!(c.windows[0].2);
        assert_eq!(c.windows[1].0, "Off-peak");
        // Friday 10:00 -> off-peak until Monday 01:00 (63h)
        assert_eq!(at(2026, 11, 6, 10, 0).countdown, "63:00:00");
    }
}
