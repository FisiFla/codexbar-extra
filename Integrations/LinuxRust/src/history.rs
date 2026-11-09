//! Local plan-utilization history: per provider and per rate window, the used % and reset time.
//!
//! Port of the macOS `PlanUtilizationHistoryStore` / `UsageStore+PlanUtilization` sampling and of
//! `QuotaBurndownModel`. Samples are compacted per UTC hour bucket: within an hour only the peak
//! usage of each reset segment survives (at most two entries, one before and one after a reset),
//! exactly like `canonicalPlanUtilizationHourEntries`. Each series keeps at most
//! `MAX_SAMPLES` entries and the whole file at most `MAX_TOTAL_SAMPLES` (oldest dropped first).
//!
//! The file lives at `$XDG_STATE_HOME/codexbar-linux/usage-history.json` (mode 0600, written
//! atomically). It is keyed by provider id only: no emails, account labels or identity strings.
//! Differences from macOS: no per-account buckets (Linux entries are not account-scoped), and the
//! percent is `100 - remaining` from the already-rounded view model (whole percents).

use crate::model::Entry;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const MIN_SAMPLE_INTERVAL_SECS: i64 = 60 * 60;
/// Swift `planUtilizationMaxSamples` (24 * 730) per series.
pub const MAX_SAMPLES: usize = 24 * 730;
/// Hard bound on the whole file (about 30 bytes per sample, so roughly 4 MB).
pub const MAX_TOTAL_SAMPLES: usize = 120_000;
/// Sanity bounds on what a (possibly hand-edited) file may contain.
const MAX_SERIES_PER_PROVIDER: usize = 12;
const MAX_PROVIDERS: usize = 200;
const RESET_TOLERANCE_SECS: i64 = 2 * 60;
const FILE_VERSION: u32 = 1;

/// `[capturedAt (unix s), usedPercent, resetsAt (unix s) | null]`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Sample(pub i64, pub f64, pub Option<i64>);

impl Sample {
    /// `PlanUtilizationHistoryEntry.precedes`.
    fn cmp_order(&self, other: &Self) -> std::cmp::Ordering {
        self.0
            .cmp(&other.0)
            .then(self.1.total_cmp(&other.1))
            .then(self.2.unwrap_or(i64::MIN).cmp(&other.2.unwrap_or(i64::MIN)))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Series {
    /// Window key from the view model (`primary`, `secondary`, `extra:<id>`, ...).
    pub name: String,
    /// Display title (already email-redacted by the view model).
    pub label: String,
    pub window_minutes: i64,
    pub entries: Vec<Sample>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct History {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub providers: BTreeMap<String, Vec<Series>>,
}

/// One observed window to record.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    pub provider: String,
    pub name: String,
    pub label: String,
    pub window_minutes: i64,
    pub used: f64,
    pub reset: Option<i64>,
}

/// `PlanUtilizationSeriesName.canonicalWindowMinutes`: collapse near-5h and near-weekly jitter.
pub fn canonical_window_minutes(minutes: i64) -> i64 {
    match minutes {
        295..=305 => 300,
        10070..=10090 => 10080,
        m => m,
    }
}

fn parse_unix(rfc3339: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .ok()
        .map(|d| d.timestamp())
}

/// Observations from one successful refresh. Entries with an error, and windows without a
/// positive cadence, are skipped (Swift requires `windowMinutes > 0`). Only the first entry per
/// provider counts: history is not account-scoped here.
pub fn observations(entries: &[Entry]) -> Vec<Observation> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for e in entries {
        if !e.error.is_empty() || !seen.insert(e.provider.clone()) {
            continue;
        }
        for w in &e.windows {
            if w.minutes <= 0 || !(0..=100).contains(&w.remaining) {
                continue;
            }
            out.push(Observation {
                provider: e.provider.clone(),
                name: w.key.clone(),
                label: w.label.clone(),
                window_minutes: canonical_window_minutes(w.minutes),
                used: (100 - w.remaining) as f64,
                reset: parse_unix(&w.resets_at),
            });
        }
    }
    out
}

fn hour_bucket(t: i64) -> i64 {
    t.div_euclid(MIN_SAMPLE_INTERVAL_SECS)
}

fn differing_resets(a: Option<i64>, b: Option<i64>) -> bool {
    matches!((a, b), (Some(a), Some(b)) if (a - b).abs() >= RESET_TOLERANCE_SECS)
}

fn segment_peak(existing: Sample, obs: Sample) -> Sample {
    if existing.2.is_none() && obs.2.is_some() {
        return obs;
    }
    let higher = obs.1 > existing.1;
    let tie_newer = obs.1 == existing.1 && obs.0 >= existing.0;
    let src = if higher || tie_newer { obs } else { existing };
    let prefer_obs = obs.0 >= existing.0;
    let reset = if prefer_obs {
        obs.2.or(existing.2)
    } else {
        existing.2.or(obs.2)
    };
    Sample(src.0, src.1, reset)
}

/// `canonicalPlanUtilizationHourEntries`.
fn canonical_hour(existing: &[Sample], incoming: Sample) -> Vec<Sample> {
    let mut obs: Vec<Sample> = existing.iter().copied().chain([incoming]).collect();
    obs.sort_by(Sample::cmp_order);
    let mut peak = obs[0];
    let mut before_reset: Option<Sample> = None;
    for o in obs.into_iter().skip(1) {
        if differing_resets(peak.2, o.2) {
            before_reset.get_or_insert(peak);
            peak = o;
            continue;
        }
        peak = segment_peak(peak, o);
    }
    match before_reset {
        Some(b) => vec![b, peak],
        None => vec![peak],
    }
}

/// `updatedPlanUtilizationEntries`: fold `entry` into its hour bucket. Returns whether the
/// series changed. Entries stay sorted by time and bounded by `MAX_SAMPLES`.
pub fn update_entries(entries: &mut Vec<Sample>, entry: Sample) -> bool {
    let bucket = hour_bucket(entry.0);
    let lo = entries.partition_point(|e| hour_bucket(e.0) < bucket);
    let hi = entries.partition_point(|e| hour_bucket(e.0) <= bucket);
    let canonical = canonical_hour(&entries[lo..hi], entry);
    if canonical == entries[lo..hi] {
        return false;
    }
    entries.splice(lo..hi, canonical);
    if entries.len() > MAX_SAMPLES {
        let extra = entries.len() - MAX_SAMPLES;
        entries.drain(..extra);
    }
    true
}

impl History {
    /// Record observations taken at `now` (unix seconds). Returns whether anything changed.
    pub fn record(&mut self, obs: &[Observation], now: i64) -> bool {
        let mut changed = false;
        for o in obs {
            if !o.used.is_finite() {
                continue;
            }
            let used = o.used.clamp(0.0, 100.0);
            if !self.providers.contains_key(&o.provider) && self.providers.len() >= MAX_PROVIDERS {
                continue;
            }
            let list = self.providers.entry(o.provider.clone()).or_default();
            let idx = match list
                .iter()
                .position(|s| s.name == o.name && s.window_minutes == o.window_minutes)
            {
                Some(i) => i,
                None if list.len() >= MAX_SERIES_PER_PROVIDER => continue,
                None => {
                    list.push(Series {
                        name: o.name.clone(),
                        label: o.label.clone(),
                        window_minutes: o.window_minutes,
                        entries: Vec::new(),
                    });
                    changed = true;
                    list.len() - 1
                }
            };
            let series = &mut list[idx];
            if series.label != o.label && !o.label.is_empty() {
                series.label = o.label.clone();
                changed = true;
            }
            let sample = Sample(now, used, o.reset);
            changed |= update_entries(&mut series.entries, sample);
            list.sort_by(|a, b| {
                a.window_minutes
                    .cmp(&b.window_minutes)
                    .then(a.name.cmp(&b.name))
            });
        }
        if changed {
            self.version = FILE_VERSION;
            self.enforce_total_bound();
        }
        changed
    }

    pub fn total_samples(&self) -> usize {
        self.providers
            .values()
            .flatten()
            .map(|s| s.entries.len())
            .sum()
    }

    /// Drop the oldest samples (from the largest series first) until the file is within bounds.
    pub fn enforce_total_bound(&mut self) {
        let mut total = self.total_samples();
        while total > MAX_TOTAL_SAMPLES {
            let Some(series) = self
                .providers
                .values_mut()
                .flatten()
                .max_by_key(|s| s.entries.len())
            else {
                break;
            };
            let drop = (total - MAX_TOTAL_SAMPLES).min(series.entries.len());
            if drop == 0 {
                break;
            }
            series.entries.drain(..drop);
            total -= drop;
        }
    }

    /// Series with data for a provider, ordered by cadence (session before weekly).
    pub fn series_for(&self, provider: &str) -> Vec<&Series> {
        self.providers
            .get(provider)
            .map(|v| v.iter().filter(|s| !s.entries.is_empty()).collect())
            .unwrap_or_default()
    }

    /// Repair a loaded file: sorted entries, finite values, bounded sizes.
    fn sanitized(mut self) -> Self {
        self.providers.retain(|_, v| !v.is_empty());
        while self.providers.len() > MAX_PROVIDERS {
            let Some(k) = self.providers.keys().next_back().cloned() else {
                break;
            };
            self.providers.remove(&k);
        }
        for list in self.providers.values_mut() {
            list.retain(|s| s.window_minutes > 0);
            list.truncate(MAX_SERIES_PER_PROVIDER);
            for s in list.iter_mut() {
                s.entries.retain(|e| e.1.is_finite());
                s.entries.sort_by(Sample::cmp_order);
                if s.entries.len() > MAX_SAMPLES {
                    let extra = s.entries.len() - MAX_SAMPLES;
                    s.entries.drain(..extra);
                }
            }
        }
        self.enforce_total_bound();
        self
    }
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

/// `$XDG_STATE_HOME/codexbar-linux/usage-history.json` (default `~/.local/state`).
pub fn state_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/state")))?;
    Some(base.join("codexbar-linux").join("usage-history.json"))
}

/// Missing file is an empty history; a corrupt file is moved aside and ignored.
pub fn load(path: &Path) -> History {
    let Ok(bytes) = std::fs::read(path) else {
        return History::default();
    };
    match serde_json::from_slice::<History>(&bytes) {
        Ok(h) => h.sanitized(),
        Err(_) => {
            let _ = std::fs::rename(path, path.with_extension("json.corrupt"));
            History::default()
        }
    }
}

static SAVE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Atomic write: temp file (mode 0600) in the same directory, fsync, rename over the target.
pub fn save(path: &Path, history: &History) -> std::io::Result<()> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let _guard = SAVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    let tmp = path.with_extension(format!("json.tmp{}", std::process::id()));
    let result = (|| {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(&serde_json::to_vec(history)?)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

// ---------------------------------------------------------------------------
// Utilization history (bars)
// ---------------------------------------------------------------------------

/// Max points like `PlanUtilizationHistoryChartMenuView.Layout.maxPoints`.
pub const MAX_BARS: usize = 30;

fn local_date(t: i64, tz_offset_secs: i64) -> String {
    chrono::DateTime::from_timestamp(t + tz_offset_secs, 0)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/// Peak used % per reset period (entries grouped by reset boundary within 2 min), oldest first.
/// Returns `(period end or last sample, peak)`.
fn period_peaks(series: &Series) -> Vec<(i64, f64)> {
    let mut periods: Vec<(Option<i64>, i64, f64)> = Vec::new(); // (reset, anchor time, peak)
    for e in &series.entries {
        match periods.last_mut() {
            Some(p) if !differing_resets(p.0, e.2) => {
                p.2 = p.2.max(e.1);
                p.1 = e.0.max(p.1);
                p.0 = p.0.or(e.2);
            }
            _ => periods.push((e.2, e.0, e.1)),
        }
    }
    periods
        .into_iter()
        .map(|(reset, at, peak)| (reset.unwrap_or(at), peak))
        .collect()
}

/// Bars for the utilization chart. Windows shorter than a day (5 h sessions) get one bar per
/// local day holding the peak of that day; longer windows get one bar per reset period dated by
/// its reset boundary. At most `MAX_BARS`, newest kept. Values are used percent.
pub fn utilization_bars(series: &Series, tz_offset_secs: i64) -> Vec<crate::charts::ChartBar> {
    let mut bars: Vec<crate::charts::ChartBar> = Vec::new();
    if series.window_minutes < 1440 {
        let mut by_day: BTreeMap<String, f64> = BTreeMap::new();
        for e in &series.entries {
            let d = by_day.entry(local_date(e.0, tz_offset_secs)).or_insert(0.0);
            *d = d.max(e.1);
        }
        let rows: Vec<(String, f64)> = by_day.into_iter().collect();
        bars = crate::charts::calendar_bars(&rows, |v| Some(*v));
    } else {
        for (t, peak) in period_peaks(series) {
            bars.push(crate::charts::ChartBar {
                date: local_date(t, tz_offset_secs),
                value: Some(peak),
                segments: Vec::new(),
            });
        }
    }
    if bars.len() > MAX_BARS {
        bars.drain(..bars.len() - MAX_BARS);
    }
    bars
}

// ---------------------------------------------------------------------------
// Quota burndown
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct BurnSample {
    pub at: i64,
    pub remaining: f64,
}

/// Port of `QuotaBurndownModel`: remaining % since the window start vs. the ideal linear burn.
#[derive(Debug, Clone, PartialEq)]
pub struct Burndown {
    pub start: i64,
    pub reset: i64,
    pub samples: Vec<BurnSample>,
}

impl Burndown {
    /// Model for the latest recorded sample of `series`, as the Swift menu view does (`now` is the
    /// latest capture time). `None` when the window is over, unknown, or invalid.
    pub fn from_series(series: &Series, now_unix: i64) -> Option<Self> {
        let latest = series.entries.last()?;
        let reset = latest.2?;
        if latest.0 > now_unix || reset <= now_unix {
            return None;
        }
        Self::new(series, latest.1, reset, latest.0, series.window_minutes)
    }

    pub fn new(
        series: &Series,
        used: f64,
        reset: i64,
        now: i64,
        window_minutes: i64,
    ) -> Option<Self> {
        if !used.is_finite() || window_minutes <= 0 {
            return None;
        }
        let start = reset - window_minutes * 60;
        if !(start <= now && now < reset) {
            return None;
        }
        let mut segment: Vec<(i64, f64)> = Vec::new();
        let historical = series
            .entries
            .iter()
            .filter(|e| {
                e.0 >= start
                    && e.0 <= now
                    && e.1.is_finite()
                    && e.2
                        .is_none_or(|r| (r - reset).abs() <= RESET_TOLERANCE_SECS)
            })
            .map(|e| (e.0, e.1))
            .chain([(now, used)]);
        for s in historical {
            if let Some(last) = segment.last() {
                if s.0 == last.0 {
                    *segment.last_mut().unwrap() = s;
                    continue;
                }
                if s.1 < last.1 {
                    segment.clear();
                }
            }
            segment.push(s);
        }
        Some(Self {
            start,
            reset,
            samples: segment
                .into_iter()
                .map(|(at, u)| BurnSample {
                    at,
                    remaining: (100.0 - u).clamp(0.0, 100.0),
                })
                .collect(),
        })
    }

    /// Ideal remaining % at `t` (100 at the start, 0 at the reset).
    pub fn ideal_at(&self, t: i64) -> f64 {
        let span = (self.reset - self.start) as f64;
        (100.0 * (1.0 - (t - self.start) as f64 / span)).clamp(0.0, 100.0)
    }

    pub fn latest_remaining(&self) -> Option<f64> {
        self.samples.last().map(|s| s.remaining)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: i64 = 3600;

    fn series(entries: Vec<Sample>, minutes: i64) -> Series {
        Series {
            name: "primary".into(),
            label: "Session".into(),
            window_minutes: minutes,
            entries,
        }
    }

    #[test]
    fn same_hour_keeps_only_the_peak() {
        let mut e = Vec::new();
        assert!(update_entries(&mut e, Sample(1000, 10.0, Some(9000))));
        assert!(update_entries(&mut e, Sample(1500, 30.0, Some(9000))));
        assert!(!update_entries(&mut e, Sample(1800, 20.0, Some(9000))));
        assert_eq!(e, vec![Sample(1500, 30.0, Some(9000))]);
    }

    #[test]
    fn new_hour_appends_and_stays_sorted() {
        let mut e = Vec::new();
        update_entries(&mut e, Sample(2 * H + 5, 20.0, None));
        update_entries(&mut e, Sample(5, 10.0, None));
        update_entries(&mut e, Sample(H + 5, 15.0, None));
        let times: Vec<i64> = e.iter().map(|s| s.0).collect();
        assert_eq!(times, vec![5, H + 5, 2 * H + 5]);
    }

    #[test]
    fn reset_inside_an_hour_keeps_pre_and_post_reset_peaks() {
        let mut e = Vec::new();
        update_entries(&mut e, Sample(100, 80.0, Some(5000)));
        update_entries(&mut e, Sample(200, 95.0, Some(5000)));
        update_entries(&mut e, Sample(300, 3.0, Some(5000 + 5 * H)));
        assert_eq!(
            e,
            vec![
                Sample(200, 95.0, Some(5000)),
                Sample(300, 3.0, Some(5000 + 5 * H))
            ]
        );
    }

    #[test]
    fn reset_jitter_under_two_minutes_is_one_segment() {
        let mut e = Vec::new();
        update_entries(&mut e, Sample(100, 10.0, Some(5000)));
        update_entries(&mut e, Sample(200, 12.0, Some(5060)));
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].1, 12.0);
        assert_eq!(e[0].2, Some(5060));
    }

    #[test]
    fn series_is_bounded_oldest_dropped() {
        let mut e: Vec<Sample> = (0..MAX_SAMPLES as i64)
            .map(|i| Sample(i * H, 1.0, None))
            .collect();
        assert!(update_entries(
            &mut e,
            Sample(MAX_SAMPLES as i64 * H, 2.0, None)
        ));
        assert_eq!(e.len(), MAX_SAMPLES);
        assert_eq!(e[0].0, H);
        assert_eq!(e.last().unwrap().0, MAX_SAMPLES as i64 * H);
    }

    #[test]
    fn total_bound_trims_largest_series_first() {
        let mut h = History::default();
        let big: Vec<Sample> = (0..(MAX_TOTAL_SAMPLES as i64 + 50))
            .map(|i| Sample(i, 1.0, None))
            .collect();
        h.providers.insert("codex".into(), vec![series(big, 300)]);
        h.providers.insert(
            "claude".into(),
            vec![series(vec![Sample(1, 1.0, None)], 300)],
        );
        h.enforce_total_bound();
        assert_eq!(h.total_samples(), MAX_TOTAL_SAMPLES);
        assert_eq!(h.series_for("claude")[0].entries.len(), 1);
    }

    #[test]
    fn observations_skip_errors_and_unknown_windows_and_hold_no_identity() {
        use crate::model::{Entry, QuotaWindow};
        let w = |key: &str, minutes, remaining, resets: &str| QuotaWindow {
            key: key.into(),
            label: "Session".into(),
            minutes,
            remaining,
            resets_at: resets.into(),
            reset_text: String::new(),
            pace: String::new(),
            pace_expected: f64::NAN,
            pace_delta: f64::NAN,
            scoped: false,
        };
        let entry = |provider: &str, error: &str| Entry {
            provider: provider.into(),
            account_label: "me@example.com".into(),
            plan: "Pro".into(),
            status: String::new(),
            status_level: String::new(),
            error: error.into(),
            updated_at: String::new(),
            credits: None,
            windows: vec![
                w("primary", 302, 75, "2026-11-09T12:00:00Z"),
                w("secondary", 0, 50, ""),
            ],
            details: vec![],
            extra_usage_text: String::new(),
            reset_credits_available: -1,
            reset_credits_next_expiry: String::new(),
        };
        let obs = observations(&[
            entry("codex", ""),
            entry("claude", "boom"),
            entry("codex", ""),
        ]);
        assert_eq!(obs.len(), 1);
        assert_eq!(obs[0].window_minutes, 300);
        assert_eq!(obs[0].used, 25.0);
        assert_eq!(obs[0].reset, Some(1_794_225_600));
        let mut h = History::default();
        h.record(&obs, 1_794_220_000);
        let json = serde_json::to_string(&h).unwrap();
        assert!(!json.contains('@') && !json.contains("Pro"));
    }

    #[test]
    fn save_load_roundtrip_atomic_and_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("cbx-hist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("codexbar-linux").join("usage-history.json");
        let mut h = History::default();
        h.record(
            &[Observation {
                provider: "codex".into(),
                name: "primary".into(),
                label: "Session".into(),
                window_minutes: 300,
                used: 40.0,
                reset: Some(99_999),
            }],
            50_000,
        );
        save(&path, &h).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(load(&path), h);
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
        std::fs::write(&path, b"not json").unwrap();
        assert_eq!(load(&path), History::default());
        assert!(path.with_extension("json.corrupt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn utilization_bars_peak_per_day_for_session_and_per_period_for_weekly() {
        let day = 86_400;
        // 2026-11-02 00:00 UTC = 1_793_577_600
        let base = 1_793_577_600;
        let s = series(
            vec![
                Sample(base + 3 * H, 20.0, None),
                Sample(base + 9 * H, 60.0, None),
                Sample(base + 2 * day + H, 10.0, None),
            ],
            300,
        );
        let bars = utilization_bars(&s, 0);
        let got: Vec<_> = bars.iter().map(|b| (b.date.as_str(), b.value)).collect();
        assert_eq!(
            got,
            vec![
                ("2026-11-02", Some(60.0)),
                ("2026-11-03", None),
                ("2026-11-04", Some(10.0))
            ]
        );
        let reset1 = base + 7 * day;
        let weekly = series(
            vec![
                Sample(base + day, 30.0, Some(reset1)),
                Sample(base + 3 * day, 55.0, Some(reset1)),
                Sample(reset1 + day, 5.0, Some(reset1 + 7 * day)),
            ],
            10080,
        );
        let bars = utilization_bars(&weekly, 0);
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].value, Some(55.0));
        assert_eq!(bars[0].date, "2026-11-09");
        assert_eq!(bars[1].value, Some(5.0));
    }

    #[test]
    fn utilization_bars_cap_to_thirty_newest() {
        let entries: Vec<Sample> = (0..50)
            .map(|i| {
                Sample(
                    1_793_577_600 + i * 7 * 86_400,
                    i as f64,
                    Some(i * 7 * 86_400 + 1_794_000_000),
                )
            })
            .collect();
        let bars = utilization_bars(&series(entries, 10080), 0);
        assert_eq!(bars.len(), MAX_BARS);
        assert_eq!(bars.last().unwrap().value, Some(49.0));
    }

    #[test]
    fn burndown_uses_current_window_segment_and_linear_ideal() {
        let reset = 100_000;
        let win = 1000; // minutes => 60_000 s, start = 40_000
        let s = series(
            vec![
                Sample(30_000, 90.0, Some(reset - 70_000)), // previous window: out of range
                Sample(45_000, 10.0, Some(reset)),
                Sample(60_000, 30.0, Some(reset + 60)), // within tolerance
                Sample(70_000, 5.0, Some(reset)),       // usage dropped: segment restarts
                Sample(80_000, 20.0, Some(reset + 3600)), // other reset: ignored
            ],
            win,
        );
        let b = Burndown::new(&s, 25.0, reset, 90_000, win).unwrap();
        assert_eq!(b.start, 40_000);
        let pts: Vec<_> = b.samples.iter().map(|s| (s.at, s.remaining)).collect();
        assert_eq!(pts, vec![(70_000, 95.0), (90_000, 75.0)]);
        assert_eq!(b.ideal_at(40_000), 100.0);
        assert_eq!(b.ideal_at(100_000), 0.0);
        assert!((b.ideal_at(70_000) - 50.0).abs() < 1e-9);
        assert_eq!(b.latest_remaining(), Some(75.0));
    }

    #[test]
    fn burndown_rejects_expired_or_unknown_windows() {
        let s = series(vec![Sample(100, 10.0, Some(1000))], 10);
        assert!(Burndown::new(&s, 10.0, 1000, 1000, 10).is_none()); // now == reset
        assert!(Burndown::new(&s, f64::NAN, 1000, 500, 10).is_none());
        assert!(Burndown::new(&s, 10.0, 1000, 500, 0).is_none());
        assert!(Burndown::new(&s, 10.0, 1000, 300, 10).is_none()); // before start (400)
                                                                   // latest sample's reset already passed: no model.
        assert!(Burndown::from_series(&s, 2000).is_none());
        let live = series(vec![Sample(700, 40.0, Some(1000))], 10);
        let b = Burndown::from_series(&live, 800).unwrap();
        assert_eq!(b.latest_remaining(), Some(60.0));
    }

    #[test]
    fn burndown_same_timestamp_replaces() {
        let s = series(vec![Sample(500, 10.0, Some(1000))], 10);
        let b = Burndown::new(&s, 20.0, 1000, 500, 10).unwrap();
        assert_eq!(b.samples.len(), 1);
        assert_eq!(b.samples[0].remaining, 80.0);
    }
}
