//! Aggregated spend view-model over `codexbar cost` payloads, mirroring the mac
//! SpendDashboardModel derivations that the CLI JSON supports (daily points,
//! provider/model rankings, daily ledger, activity heatmap).
//! Unknown stays unknown: errored payloads are excluded, days with no data are
//! `None` in the heatmap and `today` is `None` — never fabricated zeroes.

use std::collections::BTreeMap;

#[derive(Default, Clone, Debug, PartialEq)]
pub struct SpendModel {
    pub total_cost: f64,
    pub total_tokens: i64,
    /// (date, cost, tokens) aggregated across providers, ascending by date.
    pub days: Vec<(String, f64, i64)>,
    /// Same per-day aggregates, most recent first (Daily Ledger table order).
    pub ledger: Vec<(String, f64, i64)>,
    /// (provider, cost, tokens, share-of-total-cost 0..=1), ranked by cost desc.
    pub by_provider: Vec<(String, f64, i64, f64)>,
    /// (provider, model, cost, tokens, share-of-total-cost), ranked by cost desc.
    /// Empty when daily `modelBreakdowns` are unavailable.
    pub by_model: Vec<(String, String, f64, i64, f64)>,
    /// (date, intensity 0..=4) for every calendar day in range; `None` = no data.
    pub heatmap: Vec<(String, Option<u8>)>,
    /// Distinct dates with recorded data.
    pub covered_days: usize,
    /// (date, cost, tokens) for `today`, or `None` when today has no recorded data.
    pub today: Option<(String, f64, i64)>,
    /// (cost, tokens) over the trailing 30 days ending today (from daily data).
    pub period30: (f64, i64),
}

fn local_today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn share(part: f64, total: f64) -> f64 {
    if total > 0.0 {
        part / total
    } else {
        0.0
    }
}

/// Heatmap over the full calendar span of `days`: one cell per date,
/// `None` for dates with no data, `Some(0)` for a recorded zero-token day,
/// otherwise 1..=4 by token quartile rank (GitHub-style intensity steps).
pub fn heatmap_intensities(days: &[(String, f64, i64)]) -> Vec<(String, Option<u8>)> {
    if days.is_empty() {
        return Vec::new();
    }
    let by_date: BTreeMap<&str, i64> = days.iter().map(|(d, _, t)| (d.as_str(), *t)).collect();
    let parse = |s: &str| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok();
    let (Some(start), Some(end)) = (
        parse(&days.first().unwrap().0),
        parse(&days.last().unwrap().0),
    ) else {
        return days
            .iter()
            .map(|(d, _, t)| (d.clone(), Some(if *t > 0 { 1 } else { 0 })))
            .collect();
    };
    let mut positives: Vec<i64> = days.iter().map(|d| d.2).filter(|t| *t > 0).collect();
    positives.sort_unstable();
    let intensity = |tokens: i64| -> u8 {
        if tokens <= 0 || positives.is_empty() {
            return 0;
        }
        let rank = positives.partition_point(|t| *t <= tokens) as f64 / positives.len() as f64;
        (rank * 4.0).ceil().clamp(1.0, 4.0) as u8
    };
    let mut out = Vec::new();
    let mut cur = start;
    while cur <= end {
        let key = cur.format("%Y-%m-%d").to_string();
        out.push((
            key.clone(),
            by_date.get(key.as_str()).map(|t| intensity(*t)),
        ));
        cur += chrono::Duration::days(1);
    }
    out
}

fn assemble(
    total_cost: f64,
    total_tokens: i64,
    day_map: BTreeMap<String, (f64, i64)>,
    by_provider_raw: Vec<(String, f64, i64)>,
    by_model_raw: Vec<(String, String, f64, i64)>,
    today: &str,
) -> SpendModel {
    let days: Vec<(String, f64, i64)> = day_map
        .iter()
        .map(|(d, (c, t))| (d.clone(), *c, *t))
        .collect();
    let ledger: Vec<(String, f64, i64)> = days.iter().rev().cloned().collect();
    let mut by_provider: Vec<(String, f64, i64, f64)> = by_provider_raw
        .into_iter()
        .map(|(p, c, t)| (p, c, t, share(c, total_cost)))
        .collect();
    by_provider.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    let mut by_model: Vec<(String, String, f64, i64, f64)> = by_model_raw
        .into_iter()
        .map(|(p, m, c, t)| (p, m, c, t, share(c, total_cost)))
        .collect();
    by_model.sort_by(|a, b| {
        b.2.partial_cmp(&a.2)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
            .then_with(|| a.1.cmp(&b.1))
    });
    let today_row = day_map.get(today).map(|(c, t)| (today.to_string(), *c, *t));
    // Trailing 30 days ending today, inclusive (string compare works for ISO dates).
    let window_start = chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d")
        .ok()
        .map(|d| {
            (d - chrono::Duration::days(29))
                .format("%Y-%m-%d")
                .to_string()
        });
    let period30 = days
        .iter()
        .filter(|(d, _, _)| window_start.as_ref().is_none_or(|s| d >= s) && d.as_str() <= today)
        .fold((0.0, 0i64), |(c, t), (_, dc, dt)| (c + dc, t + dt));
    SpendModel {
        covered_days: days.len(),
        heatmap: heatmap_intensities(&days),
        total_cost,
        total_tokens,
        days,
        ledger,
        by_provider,
        by_model,
        today: today_row,
        period30,
    }
}

pub fn build(c: &[crate::cli::CostPayload]) -> SpendModel {
    build_at(c, &local_today())
}

/// `build` with an explicit "today" (YYYY-MM-DD) for deterministic tests.
/// Per-provider totals prefer daily sums; a payload with no daily rows falls
/// back to its `last30Days*` fields. Errored payloads are excluded entirely.
pub fn build_at(c: &[crate::cli::CostPayload], today: &str) -> SpendModel {
    let mut day_map: BTreeMap<String, (f64, i64)> = BTreeMap::new();
    let mut model_map: BTreeMap<(String, String), (f64, i64)> = BTreeMap::new();
    let mut by_provider_raw = Vec::new();
    let (mut total_cost, mut total_tokens) = (0.0f64, 0i64);
    for p in c.iter().filter(|p| p.error.is_none()) {
        let (pc, pt) = if p.daily.is_empty() {
            (p.last30_days_cost_usd, p.last30_days_tokens)
        } else {
            p.daily.iter().fold((0.0, 0i64), |(c, t), d| {
                (c + d.total_cost, t + d.total_tokens)
            })
        };
        total_cost += pc;
        total_tokens += pt;
        by_provider_raw.push((p.provider.clone(), pc, pt));
        for d in &p.daily {
            let e = day_map.entry(d.date.clone()).or_default();
            e.0 += d.total_cost;
            e.1 += d.total_tokens;
            for m in &d.model_breakdowns {
                let e = model_map
                    .entry((p.provider.clone(), m.model_name.clone()))
                    .or_default();
                e.0 += m.cost;
                e.1 += m.total_tokens;
            }
        }
    }
    let by_model_raw = model_map
        .into_iter()
        .map(|((p, m), (c, t))| (p, m, c, t))
        .collect();
    assemble(
        total_cost,
        total_tokens,
        day_map,
        by_provider_raw,
        by_model_raw,
        today,
    )
}

/// Same aggregate from view-model cost entries. Totals keep the historical
/// contract (sum of `month` / `tokens`); days/heatmap come from `daily`.
/// `by_model` stays empty — `CostEntry` carries no model breakdowns.
pub fn build_from_entries(c: &[crate::model::CostEntry]) -> SpendModel {
    let mut day_map: BTreeMap<String, (f64, i64)> = BTreeMap::new();
    let mut by_provider_raw = Vec::new();
    for e in c {
        by_provider_raw.push((e.provider.clone(), e.month, e.tokens));
        for (date, cost, tokens) in &e.daily {
            let slot = day_map.entry(date.clone()).or_default();
            slot.0 += cost;
            slot.1 += tokens;
        }
    }
    assemble(
        c.iter().map(|x| x.month).sum(),
        c.iter().map(|x| x.tokens).sum(),
        day_map,
        by_provider_raw,
        Vec::new(),
        &local_today(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{CostDay, CostModelBreakdown, CostPayload};

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 0.01, "{a} != {b}");
    }

    fn payload(
        provider: &str,
        last30_cost: f64,
        last30_tokens: i64,
        daily: Vec<CostDay>,
    ) -> CostPayload {
        CostPayload {
            provider: provider.into(),
            error: None,
            currency_code: Some("USD".into()),
            last30_days_tokens: last30_tokens,
            last30_days_cost_usd: last30_cost,
            history_coverage_is_established: true,
            provenance: Some("unknown".into()),
            session_tokens: None,
            history_label: None,
            daily,
        }
    }
    fn day(date: &str, cost: f64, tokens: i64, models: Vec<(&str, f64, i64)>) -> CostDay {
        CostDay {
            date: date.into(),
            total_tokens: tokens,
            total_cost: cost,
            model_breakdowns: models
                .into_iter()
                .map(|(m, c, t)| CostModelBreakdown {
                    model_name: m.into(),
                    cost: c,
                    total_tokens: t,
                })
                .collect(),
        }
    }

    #[test]
    fn fixture_aggregates() {
        let rows =
            crate::cli::parse_cost(include_bytes!("../tests/fixtures/cost-both.json")).unwrap();
        let m = build_at(&rows, "2026-11-08");
        approx(m.total_cost, 698.2726);
        assert_eq!(m.total_tokens, 2_267_283_594);
        assert_eq!(m.days.len(), 14);
        assert_eq!(m.covered_days, 14);
        assert_eq!(m.days[0].0, "2026-10-16");
        assert_eq!(m.days[13].0, "2026-11-08");
        assert!(
            m.days.windows(2).all(|w| w[0].0 < w[1].0),
            "days sorted ascending"
        );
        // ledger is the same aggregates, most recent first
        assert_eq!(m.ledger[0].0, "2026-11-08");
        assert_eq!(m.ledger.len(), m.days.len());
        // by_provider: claude carries everything, codex proven zero
        assert_eq!(m.by_provider[0].0, "claude");
        approx(m.by_provider[0].1, 698.2726);
        approx(m.by_provider[0].3, 1.0);
        // by_model ranked by cost desc
        assert_eq!(m.by_model[0].0, "claude");
        assert_eq!(m.by_model[0].1, "claude-opus-5-5");
        approx(m.by_model[0].2, 406.4203);
        assert_eq!(m.by_model[0].3, 980_281_143);
        assert_eq!(m.by_model.len(), 3);
        // today / period30
        let (d, c, t) = m.today.clone().expect("today present in fixture");
        assert_eq!(d, "2026-11-08");
        approx(c, 83.2054);
        assert_eq!(t, 207_597_456);
        approx(m.period30.0, 698.2726);
        assert_eq!(m.period30.1, 2_267_283_594);
        // heatmap covers the full calendar range, gaps are None (unknown, not 0)
        assert_eq!(m.heatmap.len(), 24); // 2026-10-16 ..= 2026-11-08
        let gap = m.heatmap.iter().find(|(d, _)| d == "2026-10-20").unwrap();
        assert_eq!(gap.1, None);
        let busiest = m.heatmap.iter().find(|(d, _)| d == "2026-10-31").unwrap();
        assert_eq!(busiest.1, Some(4));
        assert!(m.heatmap.iter().all(|(_, i)| i.is_none_or(|v| v <= 4)));
    }

    #[test]
    fn synthetic_overlap_shares_and_error_exclusion() {
        let rows = vec![
            payload(
                "alpha",
                30.0,
                300,
                vec![
                    day("2026-11-07", 10.0, 100, vec![("m1", 10.0, 100)]),
                    day(
                        "2026-11-08",
                        20.0,
                        200,
                        vec![("m1", 15.0, 150), ("m2", 5.0, 50)],
                    ),
                ],
            ),
            payload("beta", 10.0, 50, vec![day("2026-11-08", 10.0, 50, vec![])]),
            // empty daily falls back to last30 totals
            payload("gamma", 5.0, 25, vec![]),
            // errored payload: numbers unknown, must not pollute aggregates
            CostPayload {
                error: Some("boom".into()),
                ..payload(
                    "delta",
                    999.0,
                    999,
                    vec![day("2026-11-08", 999.0, 999, vec![])],
                )
            },
        ];
        let m = build_at(&rows, "2026-11-08");
        approx(m.total_cost, 45.0);
        assert_eq!(m.total_tokens, 375);
        assert_eq!(
            m.days,
            vec![
                ("2026-11-07".into(), 10.0, 100),
                ("2026-11-08".into(), 30.0, 250)
            ]
        );
        assert_eq!(m.by_provider[0].0, "alpha");
        approx(m.by_provider[0].3, 30.0 / 45.0);
        assert!(!m.by_provider.iter().any(|p| p.0 == "delta"));
        assert_eq!(m.by_model[0].1, "m1");
        approx(m.by_model[0].2, 25.0);
        let (_, c, t) = m.today.clone().unwrap();
        approx(c, 30.0);
        assert_eq!(t, 250);
        // a date outside all daily data is unknown, never zero-as-today
        assert!(build_at(&rows, "2026-12-25").today.is_none());
    }

    #[test]
    fn heatmap_quantiles_and_zero() {
        let days = vec![
            ("2026-01-01".into(), 0.0, 0),
            ("2026-01-02".into(), 1.0, 10),
            ("2026-01-04".into(), 1.0, 100),
        ];
        let h = heatmap_intensities(&days);
        assert_eq!(h.len(), 4); // 01..=04 with 03 missing
        assert_eq!(h[0], ("2026-01-01".into(), Some(0)));
        assert_eq!(h[2], ("2026-01-03".into(), None));
        assert_eq!(h[3].1, Some(4));
        assert!(heatmap_intensities(&[]).is_empty());
    }

    #[test]
    fn empty_build_is_unknown_not_fabricated() {
        let m = build(&[]);
        assert_eq!(m.covered_days, 0);
        assert!(m.days.is_empty() && m.heatmap.is_empty() && m.by_model.is_empty());
        assert!(m.today.is_none());
    }

    #[test]
    fn from_entries_keeps_totals_contract_and_fills_days() {
        let entries = vec![crate::model::CostEntry {
            provider: "claude".into(),
            today: 0.0,
            month: 30.0,
            tokens: 300,
            provenance: "unknown".into(),
            error: String::new(),
            coverage: "Local history".into(),
            daily: vec![
                ("2026-11-07".into(), 10.0, 100),
                ("2026-11-08".into(), 20.0, 200),
            ],
        }];
        let m = build_from_entries(&entries);
        approx(m.total_cost, 30.0);
        assert_eq!(m.total_tokens, 300);
        assert_eq!(m.days.len(), 2);
        assert_eq!(m.covered_days, 2);
        assert!(
            m.by_model.is_empty(),
            "CostEntry carries no model breakdowns"
        );
        assert_eq!(m.by_provider[0].0, "claude");
    }
}
