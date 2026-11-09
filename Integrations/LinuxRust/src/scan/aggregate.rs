//! Turns a merged `ScanSnapshot` into the view models the Usage & Spend sections render.
//! Pure functions of (snapshot, period, time zone), so period/time-zone changes re-aggregate
//! without touching the disk.

use super::*;
use chrono::{Local, TimeZone, Timelike, Utc};
use std::collections::HashMap;

/// A tool call at or above this duration counts as "slow".
pub const SLOW_TOOL_MS: i64 = 10_000;
/// Percentiles are only reported with at least this many samples.
pub const P95_MIN_SAMPLES: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Local,
    Utc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Filter {
    /// Inclusive lower bound (unix ms); `None` = all history.
    pub since_ms: Option<i64>,
    pub zone: Zone,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TokenMix {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_write: i64,
}

impl TokenMix {
    pub fn total(&self) -> i64 {
        self.input + self.output + self.cache_read + self.cache_write
    }
    fn add(&mut self, r: &Req) {
        self.input += r.i;
        self.output += r.o;
        self.cache_read += r.cr;
        self.cache_write += r.cw;
    }
}

/// Cost with "unknown stays unknown" bookkeeping.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Money {
    pub usd: f64,
    /// Some requests had no price: `usd` is a lower bound.
    pub lower_bound: bool,
    /// Some requests were priced from a same-family fallback rate.
    pub estimated: bool,
    /// Every request was unpriced.
    pub unknown: bool,
}

#[derive(Default)]
struct MoneyAcc {
    usd: f64,
    priced: usize,
    unpriced: usize,
    est: bool,
}

impl MoneyAcc {
    fn add(&mut self, r: &Req) {
        match r.c {
            Some(c) => {
                self.usd += c;
                self.priced += 1;
                self.est |= r.est;
            }
            None => self.unpriced += 1,
        }
    }
    fn finish(&self) -> Money {
        Money {
            usd: self.usd,
            lower_bound: self.unpriced > 0 && self.priced > 0,
            estimated: self.est,
            unknown: self.priced == 0 && self.unpriced > 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelRow {
    pub model: String,
    pub requests: usize,
    pub tokens: TokenMix,
    pub cost: Money,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ToolRow {
    pub name: String,
    pub calls: usize,
    pub errors: usize,
    pub slow: usize,
    pub longest_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PerfGroup {
    pub model: String,
    pub effort: Option<String>,
    pub samples: usize,
    pub median_tps: Option<f64>,
    pub median_ttft_ms: Option<f64>,
    pub median_duration_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Perf {
    pub samples: usize,
    pub median_ttft_ms: Option<f64>,
    pub median_tps: Option<f64>,
    pub median_duration_ms: Option<f64>,
    /// `Some` only with `P95_MIN_SAMPLES` or more samples.
    pub p95_duration_ms: Option<f64>,
    pub cache_hit_pct: Option<f64>,
    pub groups: Vec<PerfGroup>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionRow {
    pub provider: Provider,
    pub id: String,
    pub project: Option<String>,
    pub start_ms: i64,
    pub end_ms: i64,
    pub requests: usize,
    pub tokens: TokenMix,
    pub cost: Money,
    /// Models by descending token volume.
    pub models: Vec<ModelRow>,
    pub tools: Vec<ToolRow>,
    pub tool_calls: usize,
    pub tool_errors: usize,
    pub perf: Perf,
}

impl SessionRow {
    pub fn primary_model(&self) -> &str {
        self.models.first().map(|m| m.model.as_str()).unwrap_or("")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectRow {
    pub name: String,
    pub sessions: usize,
    pub requests: usize,
    pub tokens: TokenMix,
    pub cost: Money,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HourBucket {
    pub requests: usize,
    pub tokens: i64,
    /// Priced requests only (a lower bound when some were unpriced).
    pub cost: f64,
}

#[derive(Debug, Clone, Default)]
pub struct View {
    pub sessions: Vec<SessionRow>,
    pub projects: Vec<ProjectRow>,
    pub token_mix: TokenMix,
    pub hourly: [HourBucket; 24],
    pub tools: Vec<ToolRow>,
    pub perf: Perf,
    pub requests: usize,
    pub cost: Money,
}

pub fn hour_of(ts_ms: i64, zone: Zone) -> usize {
    match zone {
        Zone::Utc => Utc.timestamp_millis_opt(ts_ms).single().map(|d| d.hour()),
        Zone::Local => Local.timestamp_millis_opt(ts_ms).single().map(|d| d.hour()),
    }
    .unwrap_or(0) as usize
}

pub fn median(v: &mut [f64]) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    Some(if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    })
}

/// Nearest-rank percentile.
fn percentile(v: &mut [f64], p: f64) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let rank = ((p * v.len() as f64).ceil() as usize).clamp(1, v.len());
    Some(v[rank - 1])
}

fn tool_rows(tools: &[&Tool]) -> Vec<ToolRow> {
    let mut by: HashMap<&str, ToolRow> = HashMap::new();
    for t in tools {
        let row = by.entry(t.n.as_str()).or_insert_with(|| ToolRow {
            name: t.n.clone(),
            ..Default::default()
        });
        row.calls += 1;
        row.errors += t.e as usize;
        if let Some(d) = t.d {
            row.slow += (d >= SLOW_TOOL_MS) as usize;
            row.longest_ms = Some(row.longest_ms.map_or(d, |l| l.max(d)));
        }
    }
    let mut rows: Vec<ToolRow> = by.into_values().collect();
    rows.sort_by(|a, b| b.calls.cmp(&a.calls).then_with(|| a.name.cmp(&b.name)));
    rows
}

fn tps(t: &Turn) -> Option<f64> {
    if t.o <= 0 {
        return None;
    }
    // Generation speed excludes time to first token when it is known and plausible.
    let gen_ms = match t.f {
        Some(f) if f > 0 && f < t.d => t.d - f,
        _ => t.d,
    };
    (gen_ms > 0).then(|| t.o as f64 / (gen_ms as f64 / 1000.0))
}

pub fn perf(turns: &[&Turn]) -> Perf {
    if turns.is_empty() {
        return Perf::default();
    }
    let mut dur: Vec<f64> = turns.iter().map(|t| t.d as f64).collect();
    let mut ttft: Vec<f64> = turns.iter().filter_map(|t| t.f).map(|f| f as f64).collect();
    let mut rate: Vec<f64> = turns.iter().filter_map(|t| tps(t)).collect();
    let (mut ci, mut cc) = (0i64, 0i64);
    for t in turns {
        if let (Some(i), Some(c)) = (t.i, t.c) {
            if i > 0 && c <= i {
                ci += i;
                cc += c;
            }
        }
    }
    let mut groups: HashMap<(String, Option<String>), Vec<&Turn>> = HashMap::new();
    for t in turns {
        if let Some(m) = &t.m {
            groups.entry((m.clone(), t.e.clone())).or_default().push(t);
        }
    }
    let mut groups: Vec<PerfGroup> = groups
        .into_iter()
        .map(|((model, effort), ts)| PerfGroup {
            model,
            effort,
            samples: ts.len(),
            median_tps: median(&mut ts.iter().filter_map(|t| tps(t)).collect::<Vec<_>>()),
            median_ttft_ms: median(
                &mut ts
                    .iter()
                    .filter_map(|t| t.f)
                    .map(|f| f as f64)
                    .collect::<Vec<_>>(),
            ),
            median_duration_ms: median(&mut ts.iter().map(|t| t.d as f64).collect::<Vec<_>>())
                .unwrap_or(0.0),
        })
        .collect();
    groups.sort_by(|a, b| {
        b.samples
            .cmp(&a.samples)
            .then_with(|| a.model.cmp(&b.model))
            .then_with(|| a.effort.cmp(&b.effort))
    });
    Perf {
        samples: turns.len(),
        median_ttft_ms: median(&mut ttft),
        median_tps: median(&mut rate),
        median_duration_ms: median(&mut dur.clone()),
        p95_duration_ms: if turns.len() >= P95_MIN_SAMPLES {
            percentile(&mut dur, 0.95)
        } else {
            None
        },
        cache_hit_pct: (ci > 0).then(|| cc as f64 / ci as f64 * 100.0),
        groups,
    }
}

pub fn aggregate(snap: &ScanSnapshot, f: Filter) -> View {
    let keep = |t: i64| f.since_ms.is_none_or(|s| t >= s);
    let mut view = View::default();
    let mut total_cost = MoneyAcc::default();

    type Key = (Provider, String);
    struct Acc<'a> {
        reqs: Vec<&'a Req>,
        tools: Vec<&'a Tool>,
        turns: Vec<&'a Turn>,
    }
    let mut sessions: HashMap<Key, Acc> = HashMap::new();
    let entry = |m: &mut HashMap<Key, Acc<'_>>, k: Key| {
        m.entry(k).or_insert_with(|| Acc {
            reqs: vec![],
            tools: vec![],
            turns: vec![],
        });
    };
    for (p, sid, r) in &snap.reqs {
        if !keep(r.t) {
            continue;
        }
        entry(&mut sessions, (*p, sid.clone()));
        sessions.get_mut(&(*p, sid.clone())).unwrap().reqs.push(r);
        view.token_mix.add(r);
        total_cost.add(r);
        view.requests += 1;
        let h = &mut view.hourly[hour_of(r.t, f.zone)];
        h.requests += 1;
        h.tokens += r.total();
        h.cost += r.c.unwrap_or(0.0);
    }
    let mut all_tools: Vec<&Tool> = Vec::new();
    for (p, sid, t) in &snap.tools {
        if !keep(t.t) {
            continue;
        }
        entry(&mut sessions, (*p, sid.clone()));
        sessions.get_mut(&(*p, sid.clone())).unwrap().tools.push(t);
        all_tools.push(t);
    }
    let mut all_turns: Vec<&Turn> = Vec::new();
    for (p, sid, t) in &snap.turns {
        if !keep(t.t) {
            continue;
        }
        entry(&mut sessions, (*p, sid.clone()));
        sessions.get_mut(&(*p, sid.clone())).unwrap().turns.push(t);
        all_turns.push(t);
    }
    view.cost = total_cost.finish();
    view.tools = tool_rows(&all_tools);
    view.perf = perf(&all_turns);

    for ((provider, id), acc) in sessions {
        // Sessions that only have tool/turn rows (no billed requests) still show, with unknown cost.
        let mut tokens = TokenMix::default();
        let mut money = MoneyAcc::default();
        let mut by_model: HashMap<&str, (usize, TokenMix, MoneyAcc)> = HashMap::new();
        for r in &acc.reqs {
            tokens.add(r);
            money.add(r);
            let e = by_model.entry(r.m.as_str()).or_default();
            e.0 += 1;
            e.1.add(r);
            e.2.add(r);
        }
        let mut models: Vec<ModelRow> = by_model
            .into_iter()
            .map(|(m, (n, t, c))| ModelRow {
                model: m.to_string(),
                requests: n,
                tokens: t,
                cost: c.finish(),
            })
            .collect();
        models.sort_by(|a, b| {
            b.tokens
                .total()
                .cmp(&a.tokens.total())
                .then_with(|| a.model.cmp(&b.model))
        });
        let times = acc
            .reqs
            .iter()
            .map(|r| r.t)
            .chain(acc.tools.iter().map(|t| t.t))
            .chain(acc.turns.iter().map(|t| t.t));
        let (start_ms, end_ms) =
            times.fold((i64::MAX, i64::MIN), |(lo, hi), t| (lo.min(t), hi.max(t)));
        let tools = tool_rows(&acc.tools);
        view.sessions.push(SessionRow {
            provider,
            project: snap.projects.get(&(provider, id.clone())).cloned(),
            id,
            start_ms,
            end_ms,
            requests: acc.reqs.len(),
            tokens,
            cost: money.finish(),
            models,
            tool_calls: tools.iter().map(|t| t.calls).sum(),
            tool_errors: tools.iter().map(|t| t.errors).sum(),
            tools,
            perf: perf(&acc.turns),
        });
    }
    view.sessions
        .sort_by(|a, b| b.end_ms.cmp(&a.end_ms).then_with(|| a.id.cmp(&b.id)));

    let mut projects: HashMap<String, (usize, usize, TokenMix, MoneyAcc)> = HashMap::new();
    for s in &view.sessions {
        let name = s.project.clone().unwrap_or_else(|| "(unknown)".into());
        let e = projects.entry(name).or_default();
        e.0 += 1;
        e.1 += s.requests;
        e.2.input += s.tokens.input;
        e.2.output += s.tokens.output;
        e.2.cache_read += s.tokens.cache_read;
        e.2.cache_write += s.tokens.cache_write;
    }
    // Money needs request-level pricing state, so recompute per project from the sessions' rows.
    let mut proj_money: HashMap<String, MoneyAcc> = HashMap::new();
    for (p, sid, r) in &snap.reqs {
        if !keep(r.t) {
            continue;
        }
        let name = snap
            .projects
            .get(&(*p, sid.clone()))
            .cloned()
            .unwrap_or_else(|| "(unknown)".into());
        proj_money.entry(name).or_default().add(r);
    }
    view.projects = projects
        .into_iter()
        .map(|(name, (sessions, requests, tokens, _))| ProjectRow {
            cost: proj_money
                .get(&name)
                .map(MoneyAcc::finish)
                .unwrap_or_default(),
            name,
            sessions,
            requests,
            tokens,
        })
        .collect();
    view.projects.sort_by(|a, b| {
        b.cost
            .usd
            .total_cmp(&a.cost.usd)
            .then_with(|| b.tokens.total().cmp(&a.tokens.total()))
            .then_with(|| a.name.cmp(&b.name))
    });
    view
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(t: i64, m: &str, i: i64, o: i64, c: Option<f64>) -> Req {
        Req {
            k: 0,
            t,
            m: m.into(),
            i,
            o,
            cr: 10,
            cw: 5,
            c,
            ..Default::default()
        }
    }

    fn snap() -> ScanSnapshot {
        let mut s = ScanSnapshot::default();
        let day = 86_400_000i64;
        let base = 1_786_000_000_000i64;
        s.reqs.push((
            Provider::Claude,
            "a".into(),
            req(base, "m1", 100, 50, Some(1.0)),
        ));
        s.reqs.push((
            Provider::Claude,
            "a".into(),
            req(base + 1000, "m2", 10, 5, None),
        ));
        s.reqs.push((
            Provider::Codex,
            "b".into(),
            req(base - 10 * day, "m3", 1, 1, Some(0.25)),
        ));
        s.projects
            .insert((Provider::Claude, "a".into()), "demo".into());
        for (n, e, d) in [
            ("Bash", false, 12_000),
            ("Bash", true, 100),
            ("Read", false, 5),
        ] {
            s.tools.push((
                Provider::Claude,
                "a".into(),
                Tool {
                    t: base,
                    n: n.into(),
                    e,
                    d: Some(d),
                    ..Default::default()
                },
            ));
        }
        for k in 0..25 {
            s.turns.push((
                Provider::Codex,
                "b".into(),
                Turn {
                    t: base - 10 * day + k,
                    m: Some("m3".into()),
                    e: Some("high".into()),
                    d: 1000 + k * 100,
                    f: Some(200),
                    o: 80,
                    i: Some(100),
                    c: Some(50),
                    s: None,
                },
            ));
        }
        s
    }

    #[test]
    fn period_filter_drops_old_rows_and_sessions() {
        let s = snap();
        let all = aggregate(
            &s,
            Filter {
                since_ms: None,
                zone: Zone::Utc,
            },
        );
        assert_eq!(all.sessions.len(), 2);
        let recent = aggregate(
            &s,
            Filter {
                since_ms: Some(1_786_000_000_000 - 1000),
                zone: Zone::Utc,
            },
        );
        assert_eq!(recent.sessions.len(), 1);
        assert_eq!(recent.requests, 2);
        assert_eq!(recent.projects[0].name, "demo");
    }

    #[test]
    fn unknown_prices_make_lower_bounds_not_zero() {
        let v = aggregate(
            &snap(),
            Filter {
                since_ms: None,
                zone: Zone::Utc,
            },
        );
        let a = v.sessions.iter().find(|s| s.id == "a").unwrap();
        assert!(a.cost.lower_bound && !a.cost.unknown);
        assert_eq!(a.cost.usd, 1.0);
        assert_eq!(a.models.len(), 2);
        assert!(a.models.iter().any(|m| m.cost.unknown));
        assert!(v.cost.lower_bound);
    }

    #[test]
    fn token_mix_and_session_ranking() {
        let v = aggregate(
            &snap(),
            Filter {
                since_ms: None,
                zone: Zone::Utc,
            },
        );
        assert_eq!(v.token_mix.input, 111);
        assert_eq!(v.token_mix.output, 56);
        assert_eq!(v.token_mix.cache_read, 30);
        assert_eq!(v.token_mix.cache_write, 15);
        assert_eq!(v.sessions[0].id, "a"); // most recent first
        assert_eq!(v.sessions[0].primary_model(), "m1");
    }

    #[test]
    fn tool_summary_counts_errors_slow_and_longest() {
        let v = aggregate(
            &snap(),
            Filter {
                since_ms: None,
                zone: Zone::Utc,
            },
        );
        let bash = v.tools.iter().find(|t| t.name == "Bash").unwrap();
        assert_eq!(
            (bash.calls, bash.errors, bash.slow, bash.longest_ms),
            (2, 1, 1, Some(12_000))
        );
        assert_eq!(v.tools[0].name, "Bash");
    }

    #[test]
    fn perf_medians_p95_threshold_and_cache() {
        let v = aggregate(
            &snap(),
            Filter {
                since_ms: None,
                zone: Zone::Utc,
            },
        );
        let p = &v.perf;
        assert_eq!(p.samples, 25);
        assert_eq!(p.median_ttft_ms, Some(200.0));
        assert_eq!(p.median_duration_ms, Some(2200.0));
        assert_eq!(p.p95_duration_ms, Some(3300.0));
        assert_eq!(p.cache_hit_pct, Some(50.0));
        assert_eq!(p.groups.len(), 1);
        // below the sample floor no percentile is claimed
        assert_eq!(perf(&[]), Perf::default());
        let t = one_turn();
        assert_eq!(perf(&[&t]).p95_duration_ms, None);
        // tok/s excludes time to first token: 80 tokens over (1000-200)ms = 100/s
        assert!(
            (tps(&Turn {
                d: 1000,
                f: Some(200),
                o: 80,
                ..Default::default()
            })
            .unwrap()
                - 100.0)
                .abs()
                < 1e-9
        );
    }

    fn one_turn() -> Turn {
        Turn {
            t: 0,
            d: 1000,
            o: 10,
            ..Default::default()
        }
    }

    #[test]
    fn hourly_buckets_follow_the_selected_zone() {
        // 2026-01-01T23:30:00Z
        let t = 1_767_310_200_000i64;
        assert_eq!(hour_of(t, Zone::Utc), 23);
        let mut s = ScanSnapshot::default();
        s.reqs
            .push((Provider::Claude, "a".into(), req(t, "m", 1, 1, None)));
        let v = aggregate(
            &s,
            Filter {
                since_ms: None,
                zone: Zone::Utc,
            },
        );
        assert_eq!(v.hourly[23].requests, 1);
        assert_eq!(v.hourly.iter().map(|h| h.requests).sum::<usize>(), 1);
    }

    #[test]
    fn median_and_percentile_helpers() {
        assert_eq!(median(&mut [3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&mut [4.0, 1.0, 2.0, 3.0]), Some(2.5));
        assert_eq!(median(&mut []), None);
        let mut v: Vec<f64> = (1..=100).map(|x| x as f64).collect();
        assert_eq!(percentile(&mut v, 0.95), Some(95.0));
    }
}
