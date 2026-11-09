//! Tokens/min throughput.
//!
//! macOS derives per-minute tokens from the OpenAI Admin usage API (`OpenAILiveUsage`), which
//! needs an admin key and is not available to this app yet. The honest Linux substitute is the
//! per-day token totals from `codexbar cost`: each day's total divided by the 1,440 minutes in a
//! day. That is an average over the whole day, not a measured per-minute rate, and the UI says so.

use crate::charts::{calendar_bars, ChartBar};
use crate::cli::CostDay;

pub const MINUTES_PER_DAY: f64 = 1440.0;

/// Bars of average tokens/min per calendar day (days without data are unknown).
pub fn daily_bars(days: &[CostDay]) -> Vec<ChartBar> {
    let rows: Vec<(String, f64)> = days
        .iter()
        .filter(|d| d.total_tokens > 0)
        .map(|d| (d.date.clone(), d.total_tokens as f64 / MINUTES_PER_DAY))
        .collect();
    calendar_bars(&rows, |v| Some(*v))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    /// Last day in the data.
    pub latest: f64,
    pub latest_date: String,
    pub peak: f64,
    pub peak_date: String,
    /// Mean over days that have data.
    pub average: f64,
}

pub fn summary(bars: &[ChartBar]) -> Option<Summary> {
    let known: Vec<(&ChartBar, f64)> = bars
        .iter()
        .filter_map(|b| b.value.filter(|v| *v > 0.0).map(|v| (b, v)))
        .collect();
    let last = known.last()?;
    let peak = known
        .iter()
        .fold(known[0], |a, b| if b.1 > a.1 { *b } else { a });
    Some(Summary {
        latest: last.1,
        latest_date: last.0.date.clone(),
        peak: peak.1,
        peak_date: peak.0.date.clone(),
        average: known.iter().map(|k| k.1).sum::<f64>() / known.len() as f64,
    })
}

/// Compact count like `UsageFormatter.tokenCountString`: 842, 12K, 3.2M.
pub fn count(v: f64) -> String {
    if v >= 1_000_000.0 {
        format!("{:.1}M", v / 1_000_000.0)
    } else if v >= 10_000.0 {
        format!("{:.0}K", v / 1000.0)
    } else if v >= 1000.0 {
        format!("{:.1}K", v / 1000.0)
    } else if v >= 10.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

/// Tooltip value: `1.2K/min`.
pub fn per_min(v: f64) -> String {
    format!("{}/min", count(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(date: &str, tokens: i64) -> CostDay {
        CostDay {
            date: date.into(),
            total_tokens: tokens,
            total_cost: 0.0,
            model_breakdowns: vec![],
        }
    }

    #[test]
    fn divides_daily_tokens_by_minutes_per_day_and_marks_gaps_unknown() {
        let bars = daily_bars(&[
            day("2026-11-01", 1_440_000),
            day("2026-11-03", 720_000),
            day("2026-11-04", 0),
        ]);
        let got: Vec<_> = bars.iter().map(|b| b.value).collect();
        assert_eq!(got, vec![Some(1000.0), None, Some(500.0)]);
    }

    #[test]
    fn summary_reports_latest_peak_and_average() {
        let bars = daily_bars(&[
            day("2026-11-01", 1_440_000),
            day("2026-11-02", 4_320_000),
            day("2026-11-03", 720_000),
        ]);
        let s = summary(&bars).unwrap();
        assert_eq!(s.latest, 500.0);
        assert_eq!(s.latest_date, "2026-11-03");
        assert_eq!(s.peak, 3000.0);
        assert_eq!(s.peak_date, "2026-11-02");
        assert!((s.average - 1500.0).abs() < 1e-9);
        assert!(summary(&daily_bars(&[day("2026-11-01", 0)])).is_none());
    }

    #[test]
    fn count_format() {
        assert_eq!(count(3.0), "3.0");
        assert_eq!(count(842.0), "842");
        assert_eq!(count(1234.0), "1.2K");
        assert_eq!(count(12_500.0), "12K");
        assert_eq!(count(3_200_000.0), "3.2M");
    }
}
