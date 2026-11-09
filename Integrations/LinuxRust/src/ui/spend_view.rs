//! Usage & Spend window: per-provider usage cards on top, then the spend
//! dashboard (summary, daily trend, activity heatmap, provider/model/ledger
//! tabs). Mirrors Integrations/Linux/qml/Dashboard.qml + UsageCard.qml and
//! the Swift SpendDashboard pane (audit 3-dashboard, synthesis screen 7).
//!
//! Unknown discipline: a value that was never measured renders as "—",
//! never as 0. Days with no recorded data are unknown (gray heatmap cells,
//! gap ticks in the trend, em-dash ledger rows), distinct from proven zero.

use crate::model::{provider_name, CostEntry, Entry};
use crate::spend::SpendModel;
use crate::state::{App, Message};
use chrono::{Datelike, NaiveDate};
use iced::widget::canvas::{self, Canvas, Frame, Geometry};
use iced::widget::{
    button, column, container, horizontal_space, progress_bar, row, scrollable, text,
};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};
use std::collections::BTreeMap;

pub fn scroll_id() -> scrollable::Id {
    scrollable::Id::new("spend-body")
}

pub const EM_DASH: &str = "—";
const ACCENT: Color = Color::from_rgb(0.18, 0.65, 0.35);
const UNKNOWN_GRAY: Color = Color::from_rgb(0.45, 0.45, 0.45);

// ---------------------------------------------------------------------------
// Pure formatting / scaling helpers (unit-tested below)
// ---------------------------------------------------------------------------

fn finite(v: f64) -> Option<f64> {
    v.is_finite().then_some(v)
}

/// "$12.30" for a measured cost, "—" when unknown (never "$0.00").
pub fn money(v: Option<f64>) -> String {
    match v.and_then(finite) {
        Some(x) if x < 0.0 => format!("-${:.2}", -x),
        Some(x) => format!("${x:.2}"),
        None => EM_DASH.to_string(),
    }
}

/// Like `money`, prefixed "~" when the figure is a lower bound (some of the
/// underlying history is incomplete/unpriced), per the Swift ledger.
pub fn money_lb(v: Option<f64>, lower_bound: bool) -> String {
    match (v.and_then(finite), lower_bound) {
        (Some(_), true) => format!("~{}", money(v)),
        _ => money(v),
    }
}

/// "1,234,567" for a measured count, "—" when unknown (Usage.js count()).
pub fn count(v: Option<i64>) -> String {
    let Some(n) = v else {
        return EM_DASH.to_string();
    };
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

/// Fraction of `max` in 0.0..=1.0 for bar scaling; 0 for unusable input.
pub fn bar_fraction(value: f64, max: f64) -> f32 {
    if !value.is_finite() || !max.is_finite() || max <= 0.0 {
        return 0.0;
    }
    (value / max).clamp(0.0, 1.0) as f32
}

/// 5-step heatmap level (0..=4) for a day's tokens; None stays unknown.
pub fn heat_level(tokens: Option<i64>, max: i64) -> Option<usize> {
    let t = tokens?;
    if t <= 0 || max <= 0 {
        return Some(0);
    }
    let ratio = t as f64 / max as f64;
    Some((ratio * 4.0).ceil().clamp(1.0, 4.0) as usize)
}

/// GitHub-style dark green palette; unknown is gray (hatch approximation),
/// proven zero is the darkest cell.
pub fn heat_color(level: Option<usize>) -> Color {
    match level {
        None => UNKNOWN_GRAY,
        Some(0) => Color::from_rgb(0.09, 0.11, 0.13),
        Some(1) => Color::from_rgb(0.05, 0.27, 0.16),
        Some(2) => Color::from_rgb(0.0, 0.43, 0.20),
        Some(3) => Color::from_rgb(0.15, 0.65, 0.25),
        Some(_) => Color::from_rgb(0.22, 0.83, 0.33),
    }
}

pub fn share_percent(part: Option<f64>, total: Option<f64>) -> Option<f64> {
    let (p, t) = (part.and_then(finite)?, total.and_then(finite)?);
    (t > 0.0).then(|| p / t * 100.0)
}

/// A spend figure is only a lower bound when history is incomplete, the
/// fetch errored, or the provenance says partial (audit 3: inputSummary).
pub fn is_lower_bound(provenance: &str, coverage: &str, error: &str) -> bool {
    !error.trim().is_empty()
        || coverage.to_lowercase().contains("incomplete")
        || provenance.to_lowercase().contains("partial")
        || provenance.to_lowercase().contains("incomplete")
}

/// Provenance chip text, mirroring Usage.js provenance().
pub fn provenance_label(p: &str) -> String {
    match p {
        "listPriceEstimate" => "List-price estimate".to_string(),
        "actual" | "vendorMetered" => "Reported cost".to_string(),
        "mixed" => "Mixed".to_string(),
        "unknown" | "" => "Cost source unavailable".to_string(),
        other => other.to_string(),
    }
}

/// Displayed quota respecting the used/remaining preference (Usage.js).
pub fn quota_value(remaining: i64, mode: &str) -> i64 {
    let r = remaining.clamp(0, 100);
    if mode == "used" {
        100 - r
    } else {
        r
    }
}

/// Period button label: 0 (and negatives) mean "All" for SetSpendPeriod.
pub fn period_label(days: i64) -> String {
    if days <= 0 {
        "All".to_string()
    } else {
        format!("{days}d")
    }
}

pub fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()
}

fn in_period(date: NaiveDate, period_days: i64, today: NaiveDate) -> bool {
    if date > today {
        return false;
    }
    period_days <= 0 || date > today - chrono::Duration::days(period_days)
}

// ---------------------------------------------------------------------------
// Pure aggregation over SpendModel days / CostEntry rows
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct DailyTotal {
    pub date: NaiveDate,
    pub cost: Option<f64>,
    pub tokens: Option<i64>,
}

/// One row per calendar day in the period, ascending. Days absent from
/// `days` stay None (unknown), never zero-filled.
pub fn daily_range(
    days: &[(String, f64, i64)],
    period_days: i64,
    today: NaiveDate,
) -> Vec<DailyTotal> {
    let known: BTreeMap<NaiveDate, (f64, i64)> = days
        .iter()
        .filter_map(|(d, c, t)| parse_date(d).map(|date| (date, (*c, *t))))
        .collect();
    let start = if period_days > 0 {
        today - chrono::Duration::days(period_days - 1)
    } else {
        match known.keys().next() {
            Some(first) => *first,
            None => return Vec::new(),
        }
    };
    let mut out = Vec::new();
    let mut day = start;
    while day <= today {
        let (cost, tokens) = match known.get(&day) {
            Some((c, t)) => (finite(*c), Some(*t)),
            None => (None, None),
        };
        out.push(DailyTotal {
            date: day,
            cost,
            tokens,
        });
        day += chrono::Duration::days(1);
    }
    out
}

/// (total cost, total tokens, covered days, today's cost) over a range.
/// Totals are None when no day in the range was measured.
pub fn range_totals(range: &[DailyTotal]) -> (Option<f64>, Option<i64>, usize, Option<f64>) {
    let mut cost: Option<f64> = None;
    let mut tokens: Option<i64> = None;
    let mut covered = 0usize;
    for d in range {
        if d.cost.is_some() || d.tokens.is_some() {
            covered += 1;
        }
        if let Some(c) = d.cost {
            cost = Some(cost.unwrap_or(0.0) + c);
        }
        if let Some(t) = d.tokens {
            tokens = Some(tokens.unwrap_or(0) + t);
        }
    }
    let today_cost = range.last().and_then(|d| d.cost);
    (cost, tokens, covered, today_cost)
}

/// Period cost for one provider row. An errored fetch is unknown, not 0.
/// Falls back to the last-30-days figure when no daily rows exist but the
/// provider's history is established (Qt costs(): month), for 30d+/All.
pub fn entry_period_cost(e: &CostEntry, period_days: i64, today: NaiveDate) -> Option<f64> {
    if !e.error.trim().is_empty() {
        return None;
    }
    let mut sum: Option<f64> = None;
    for (d, c, _) in &e.daily {
        if let Some(date) = parse_date(d) {
            if in_period(date, period_days, today) {
                if let Some(v) = finite(*c) {
                    sum = Some(sum.unwrap_or(0.0) + v);
                }
            }
        }
    }
    if sum.is_some() {
        return sum;
    }
    if e.daily.is_empty() && (period_days <= 0 || period_days >= 30) {
        return finite(e.month);
    }
    None
}

pub fn entry_period_tokens(e: &CostEntry, period_days: i64, today: NaiveDate) -> Option<i64> {
    if !e.error.trim().is_empty() {
        return None;
    }
    let mut sum: Option<i64> = None;
    for (d, _, t) in &e.daily {
        if let Some(date) = parse_date(d) {
            if in_period(date, period_days, today) {
                sum = Some(sum.unwrap_or(0) + t);
            }
        }
    }
    if sum.is_some() {
        return sum;
    }
    if e.daily.is_empty() && (period_days <= 0 || period_days >= 30) {
        return Some(e.tokens);
    }
    None
}

/// (rank, provider id, cost, tokens, share %) ranked by cost desc;
/// unknown-cost providers sort last (by id), never as $0.
pub type ProviderRow = (usize, String, Option<f64>, Option<i64>, Option<f64>);

pub fn provider_rows(
    entries: &[CostEntry],
    period_days: i64,
    today: NaiveDate,
) -> Vec<ProviderRow> {
    let mut rows: Vec<(String, Option<f64>, Option<i64>)> = entries
        .iter()
        .map(|e| {
            (
                e.provider.clone(),
                entry_period_cost(e, period_days, today),
                entry_period_tokens(e, period_days, today),
            )
        })
        .collect();
    rows.sort_by(|a, b| match (a.1, b.1) {
        (Some(x), Some(y)) => y
            .partial_cmp(&x)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.0.cmp(&b.0),
    });
    let total: Option<f64> = {
        let known: Vec<f64> = rows.iter().filter_map(|r| r.1).collect();
        (!known.is_empty()).then(|| known.iter().sum())
    };
    rows.into_iter()
        .enumerate()
        .map(|(i, (p, c, t))| (i + 1, p, c, t, share_percent(c, total)))
        .collect()
}

/// (rank, provider, model, cost, tokens, share %) from SpendModel.by_model
/// shaped rows (provider, model, cost, tokens), ranked by cost desc.
pub type ModelRow = (usize, String, String, Option<f64>, Option<i64>, Option<f64>);

pub fn rank_models(rows: &[(String, String, f64, i64)], total: Option<f64>) -> Vec<ModelRow> {
    let mut sorted: Vec<&(String, String, f64, i64)> = rows.iter().collect();
    sorted.sort_by(|a, b| {
        b.2.partial_cmp(&a.2)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.cmp(&b.1))
    });
    sorted
        .into_iter()
        .enumerate()
        .map(|(i, r)| {
            (
                i + 1,
                r.0.clone(),
                r.1.clone(),
                finite(r.2),
                Some(r.3),
                share_percent(finite(r.2), total),
            )
        })
        .collect()
}

/// Weeks x 7 grid placement (Monday = row 0). Returns (week count, cells as
/// (column, row, level)); padding before the first day emits no cells.
pub fn heatmap_cells(
    days: &[(NaiveDate, Option<usize>)],
) -> (usize, Vec<(usize, usize, Option<usize>)>) {
    if days.is_empty() {
        return (0, Vec::new());
    }
    let mut sorted: Vec<(NaiveDate, Option<usize>)> = days.to_vec();
    sorted.sort_by_key(|(d, _)| *d);
    let first = sorted[0].0;
    let first_monday =
        first - chrono::Duration::days(first.weekday().num_days_from_monday() as i64);
    let mut cells = Vec::with_capacity(sorted.len());
    let mut weeks = 0usize;
    for (date, level) in sorted {
        let offset = (date - first_monday).num_days();
        let (col, row) = ((offset / 7) as usize, (offset % 7) as usize);
        weeks = weeks.max(col + 1);
        cells.push((col, row, level));
    }
    (weeks, cells)
}

/// Heatmap input: prefer SpendModel.heatmap intensities (unknown = None);
/// otherwise derive levels from daily tokens so the grid still renders.
pub fn heatmap_input(spend: &SpendModel, today: NaiveDate) -> Vec<(NaiveDate, Option<usize>)> {
    let from_model: Vec<(NaiveDate, Option<usize>)> = spend
        .heatmap
        .iter()
        .filter_map(|(d, level)| {
            parse_date(d).map(|date| (date, level.map(|v| v as usize).map(|v| v.min(4))))
        })
        .collect();
    if !from_model.is_empty() {
        return from_model;
    }
    let range = daily_range(&spend.days, 0, today);
    let max = range.iter().filter_map(|d| d.tokens).max().unwrap_or(0);
    range
        .iter()
        .map(|d| (d.date, heat_level(d.tokens, max)))
        .collect()
}

// ---------------------------------------------------------------------------
// Canvas programs: activity heatmap (trend bars live in crate::charts)
// ---------------------------------------------------------------------------

const CELL: f32 = 11.0;
const CELL_GAP: f32 = 3.0;

struct HeatmapChart {
    cells: Vec<(usize, usize, Option<usize>)>,
}

impl canvas::Program<Message> for HeatmapChart {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        for (col, row, level) in &self.cells {
            let x = *col as f32 * (CELL + CELL_GAP);
            let y = *row as f32 * (CELL + CELL_GAP);
            frame.fill_rectangle(Point::new(x, y), Size::new(CELL, CELL), heat_color(*level));
        }
        vec![frame.into_geometry()]
    }
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

fn period_button(label: &str, days: i64, current: i64) -> Element<'static, Message> {
    let b = button(text(label.to_string())).on_press(Message::SetSpendPeriod(days));
    if days == current {
        b.style(button::primary).into()
    } else {
        b.style(button::secondary).into()
    }
}

fn tab_button(label: &str, tab: &str, current: &str) -> Element<'static, Message> {
    let b = button(text(label.to_string())).on_press(Message::SetSpendTab(tab.to_string()));
    if tab == current {
        b.style(button::primary).into()
    } else {
        b.style(button::secondary).into()
    }
}

fn summary_card(label: &str, value: String) -> Element<'static, Message> {
    container(column![text(label.to_string()).size(12), text(value).size(20)].spacing(2))
        .padding(10)
        .style(container::bordered_box)
        .into()
}

/// Inline share/quota bar: filled track sized by `fraction` (0..=1).
fn share_bar(fraction: f32, color: Color) -> Element<'static, Message> {
    let filled = (fraction.clamp(0.0, 1.0) * 160.0).max(if fraction > 0.0 { 2.0 } else { 0.0 });
    row![
        container(text(""))
            .width(Length::Fixed(filled))
            .height(Length::Fixed(10.0))
            .style(move |_theme: &Theme| container::Style {
                background: Some(color.into()),
                ..Default::default()
            }),
        container(text(""))
            .width(Length::Fixed(160.0 - filled))
            .height(Length::Fixed(10.0))
            .style(|_theme: &Theme| container::Style {
                background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.12).into()),
                ..Default::default()
            }),
    ]
    .into()
}

fn swatch(color: Color) -> Element<'static, Message> {
    container(text(""))
        .width(Length::Fixed(CELL))
        .height(Length::Fixed(CELL))
        .style(move |_theme: &Theme| container::Style {
            background: Some(color.into()),
            ..Default::default()
        })
        .into()
}

fn usage_card(app: &App, e: &Entry) -> Element<'static, Message> {
    let used = app.settings.quota_display == "used";
    let mut col = column![row![
        text(provider_name(&e.provider)).size(18),
        horizontal_space(),
        text(e.plan.clone()).size(13),
    ]]
    .spacing(6);
    if !e.account_label.is_empty() {
        col = col.push(text(e.account_label.clone()).size(13));
    }
    if !e.status.is_empty() {
        col = col.push(text(format!("Status: {}", e.status)).size(13));
    }
    if !e.error.is_empty() {
        col = col.push(
            text(e.error.clone())
                .size(13)
                .color(Color::from_rgb(0.92, 0.45, 0.35)),
        );
    }
    for w in &e.windows {
        let qv = quota_value(w.remaining, &app.settings.quota_display);
        col = col.push(
            row![
                text(w.label.clone()),
                horizontal_space(),
                text(format!("{qv}% {}", if used { "used" } else { "left" })),
            ]
            .spacing(8),
        );
        col = col.push(progress_bar(0.0..=100.0, qv as f32));
        if !w.reset_text.is_empty() {
            col = col.push(text(w.reset_text.clone()).size(12));
        }
        if app.settings.show_pace && !w.pace.is_empty() {
            col = col.push(text(w.pace.clone()).size(12));
        }
    }
    if e.windows.is_empty() && e.error.is_empty() {
        col = col.push(text("No quota windows reported.").size(13));
    }
    if let Some(credits) = e.credits.and_then(finite) {
        col = col.push(text(format!("Credits: {credits}")));
    }
    if e.reset_credits_available > 0 {
        let mut s = format!("Reset credits: {}", e.reset_credits_available);
        if !e.reset_credits_next_expiry.is_empty() {
            s.push_str(&format!(" · next expiry {}", e.reset_credits_next_expiry));
        }
        col = col.push(text(s).size(13));
    }
    if !e.extra_usage_text.is_empty() {
        col = col.push(text(e.extra_usage_text.clone()).size(13));
    }
    for section in &e.details {
        if !section.title.is_empty() {
            col = col.push(text(section.title.clone()).size(14));
        }
        for (label, value, secondary) in &section.rows {
            let mut line = format!("{label}: {value}");
            if !secondary.is_empty() {
                line.push_str(&format!(" · {secondary}"));
            }
            col = col.push(text(line).size(13));
        }
    }
    container(col)
        .padding(12)
        .style(container::bordered_box)
        .width(Length::Fill)
        .into()
}

pub fn view(app: &App) -> Element<'_, Message> {
    let today = chrono::Local::now().date_naive();
    let period = app.spend_period_days;
    let mut page = column![
        row![
            text("Usage & Spend").size(24),
            horizontal_space(),
            period_button("7d", 7, period),
            period_button("30d", 30, period),
            period_button("90d", 90, period),
            period_button("All", 0, period),
            button(text(if app.cost_busy { "Refreshing…" } else { "Refresh" }))
                .on_press_maybe((!app.cost_busy).then_some(Message::RefreshCosts)),
            button(text("Refresh usage")).on_press_maybe((!app.busy).then_some(Message::Refresh)),
            button(text("Share…")).on_press(Message::ShowPage(crate::state::Page::ShareStats)),
        ]
        .spacing(6),
        text(format!(
            "Days are grouped by local date (UTC{}). Estimated cost of local sessions, across accounts.",
            chrono::Local::now().format("%:z")
        ))
        .size(12),
    ]
    .spacing(10)
    .padding(16);

    // Usage status line + per-entry cards (Qt Dashboard Usage tab).
    let usage_status = if !app.error.is_empty() {
        app.error.clone()
    } else if app.busy {
        "Refreshing usage…".to_string()
    } else if app.stale {
        "Usage is out of date".to_string()
    } else if !app.updated.is_empty() {
        format!("Updated {}", app.updated)
    } else {
        "Waiting for usage…".to_string()
    };
    page = page.push(text(usage_status).size(13));
    // Usage cards go below the spend dashboard; failing providers collapse into one line
    // so dozens of unconfigured providers don't bury the dashboard.
    let mut usage = column![text("Usage").size(16)].spacing(10);
    if app.entries.is_empty() {
        usage = usage.push(text("No usage reported yet.").size(13));
    }
    let unavailable = app
        .entries
        .iter()
        .filter(|e| !e.error.trim().is_empty())
        .count();
    for e in app.entries.iter().filter(|e| e.error.trim().is_empty()) {
        usage = usage.push(usage_card(app, e));
    }
    if unavailable > 0 {
        usage = usage.push(
            text(format!(
                "{unavailable} provider(s) unavailable — configure or disable them in Settings › Providers."
            ))
            .size(12),
        );
    }

    // Failures banner.
    let failed = app
        .spending
        .iter()
        .filter(|s| !s.error.trim().is_empty())
        .count();
    if !app.cost_error.is_empty() {
        page = page.push(
            text(format!("Cost refresh failed: {}", app.cost_error))
                .size(13)
                .color(Color::from_rgb(0.92, 0.45, 0.35)),
        );
    } else if failed > 0 {
        page = page.push(
            text(format!("{failed} provider(s) failed to refresh spend data"))
                .size(13)
                .color(Color::from_rgb(0.92, 0.65, 0.35)),
        );
    }

    if !app.settings.show_costs {
        page = page.push(text("Enable local spending in Settings."));
        return scrollable(page.push(usage)).into();
    }

    let range = daily_range(&app.spend.days, period, today);
    let (mut total_cost, mut total_tokens, covered, mut today_cost) = range_totals(&range);
    // Daily series may be absent while per-provider totals exist (month fallback).
    if total_cost.is_none() {
        let known: Vec<f64> = app
            .spending
            .iter()
            .filter_map(|e| entry_period_cost(e, period, today))
            .collect();
        if !known.is_empty() {
            total_cost = Some(known.iter().sum());
        }
    }
    if total_tokens.is_none() {
        let known: Vec<i64> = app
            .spending
            .iter()
            .filter_map(|e| entry_period_tokens(e, period, today))
            .collect();
        if !known.is_empty() {
            total_tokens = Some(known.iter().sum());
        }
    }
    if today_cost.is_none() {
        let known: Vec<f64> = app
            .spending
            .iter()
            .filter(|e| e.error.trim().is_empty())
            .filter_map(|e| finite(e.today))
            .collect();
        if !known.is_empty() {
            today_cost = Some(known.iter().sum());
        }
    }
    let any_lower_bound = app
        .spending
        .iter()
        .any(|e| is_lower_bound(&e.provenance, &e.coverage, &e.error));
    let has_spend = total_cost.is_some() || covered > 0 || !app.spend.by_model.is_empty();

    if !has_spend {
        page = page.push(text(if app.cost_busy {
            "Reading local history…"
        } else {
            "No local cost history yet."
        }));
        return scrollable(page.push(usage)).into();
    }

    // Summary cards.
    let covered_label = if range.is_empty() {
        EM_DASH.to_string()
    } else {
        format!("{covered} of {} days", range.len())
    };
    page = page.push(
        row![
            summary_card("Total cost", money_lb(total_cost, any_lower_bound)),
            summary_card("Tokens", count(total_tokens)),
            summary_card("Covered days", covered_label),
            summary_card("Today", money_lb(today_cost, any_lower_bound)),
        ]
        .spacing(8),
    );
    // Provenance chips per provider (audit: provenance + coverage chips).
    for e in &app.spending {
        if !e.error.trim().is_empty() {
            page = page.push(text(format!("{}: {}", provider_name(&e.provider), e.error)).size(12));
        } else {
            page = page.push(
                text(format!(
                    "{} · {} · {}",
                    provider_name(&e.provider),
                    provenance_label(&e.provenance),
                    e.coverage
                ))
                .size(12),
            );
        }
    }

    // Trend: daily estimated spend bars with text labels (no hover needed).
    if !range.is_empty() {
        let bars: Vec<crate::charts::ChartBar> = range
            .iter()
            .map(|d| crate::charts::ChartBar {
                date: d.date.format("%Y-%m-%d").to_string(),
                value: d.cost,
                segments: Vec::new(),
            })
            .collect();
        let chart: Element<Message> = Canvas::new(crate::charts::BarChart {
            key: "spend-trend".into(),
            bars,
            color: ACCENT,
            peak_cap: false,
            hover: app
                .chart_hover
                .as_ref()
                .filter(|(k, _)| k == "spend-trend")
                .map(|(_, i)| *i),
            fmt: crate::charts::money,
        })
        .width(Length::Fill)
        .height(Length::Fixed(140.0))
        .into();
        page = page.push(
            text(format!(
                "Trend — daily estimated spend · {}",
                period_label(period)
            ))
            .size(16),
        );
        page = page.push(chart);
        page = page
            .push(text("Gray ticks are days with no recorded data (unknown, not $0).").size(12));
    }

    // Activity heatmap (weeks x 7, most recent ~53 weeks).
    let heat_days: Vec<(NaiveDate, Option<usize>)> = heatmap_input(&app.spend, today)
        .into_iter()
        .filter(|(d, _)| {
            *d > today - chrono::Duration::days(371)
                && (period <= 0 || *d > today - chrono::Duration::days(period))
        })
        .collect();
    if !heat_days.is_empty() {
        let (weeks, cells) = heatmap_cells(&heat_days);
        let width = weeks as f32 * (CELL + CELL_GAP);
        let chart: Element<Message> = Canvas::new(HeatmapChart { cells })
            .width(Length::Fixed(width.max(CELL)))
            .height(Length::Fixed(7.0 * (CELL + CELL_GAP)))
            .into();
        page = page.push(text("Activity — tokens per day (rows Mon–Sun)").size(16));
        page = page.push(chart);
        page = page.push(
            row![
                text("Less").size(12),
                swatch(heat_color(Some(0))),
                swatch(heat_color(Some(1))),
                swatch(heat_color(Some(2))),
                swatch(heat_color(Some(3))),
                swatch(heat_color(Some(4))),
                text("More").size(12),
                swatch(heat_color(None)),
                text("no data").size(12),
            ]
            .spacing(4),
        );
    }

    // Detail tabs.
    page = page.push(
        row![
            tab_button("Providers", "providers", &app.spend_tab),
            tab_button("Models", "models", &app.spend_tab),
            tab_button("Ledger", "ledger", &app.spend_tab),
        ]
        .spacing(6),
    );
    match app.spend_tab.as_str() {
        "models" => {
            let rows_in: Vec<(String, String, f64, i64)> = app
                .spend
                .by_model
                .iter()
                .map(|m| (m.0.clone(), m.1.clone(), m.2, m.3))
                .collect();
            let models = rank_models(&rows_in, total_cost);
            if models.is_empty() {
                page = page.push(
                    text("Model breakdown unavailable — no per-model cost rows in local history.")
                        .size(13),
                );
            }
            for (rank, provider, model, cost, tokens, share) in models {
                page = page.push(
                    column![
                        row![
                            text(format!("#{rank} {model}")),
                            horizontal_space(),
                            text(format!(
                                "{} · {} · {} tokens · {}",
                                provider_name(&provider),
                                money_lb(cost, any_lower_bound),
                                count(tokens),
                                share
                                    .map(|s| format!("{s:.1}%"))
                                    .unwrap_or_else(|| EM_DASH.to_string())
                            )),
                        ],
                        share_bar(
                            bar_fraction(cost.unwrap_or(0.0), total_cost.unwrap_or(0.0)),
                            ACCENT
                        ),
                    ]
                    .spacing(4),
                );
            }
        }
        "ledger" => {
            page = page.push(
                row![
                    text("Day").width(Length::Fixed(110.0)),
                    text("Tokens").width(Length::Fixed(140.0)),
                    text("Estimated spend"),
                ]
                .spacing(8),
            );
            let ledger_range = daily_range(
                &app.spend.days,
                if period > 0 { period.min(30) } else { 30 },
                today,
            );
            let mut shown = 0usize;
            for d in ledger_range.iter().rev() {
                // Ledger starts at the first covered day; gaps after it are
                // shown as unknown rows rather than dropped silently.
                if shown == 0 && d.cost.is_none() && d.tokens.is_none() {
                    continue;
                }
                if shown >= 30 {
                    break;
                }
                shown += 1;
                page = page.push(
                    row![
                        text(d.date.to_string()).width(Length::Fixed(110.0)),
                        text(count(d.tokens)).width(Length::Fixed(140.0)),
                        text(money_lb(d.cost, any_lower_bound && d.cost.is_some())),
                    ]
                    .spacing(8),
                );
            }
            if shown == 0 {
                page = page.push(text("No local cost history yet.").size(13));
            } else if any_lower_bound {
                page = page.push(
                    text("~ marks a lower bound: some history is incomplete or unpriced.").size(12),
                );
            }
        }
        _ => {
            let rows = provider_rows(&app.spending, period, today);
            if rows.is_empty() {
                page = page.push(text("No local cost history yet.").size(13));
            }
            for (rank, provider, cost, tokens, share) in rows {
                page = page.push(
                    column![
                        row![
                            text(format!("#{rank} {}", provider_name(&provider))),
                            horizontal_space(),
                            text(format!(
                                "{} · {} tokens · {}",
                                money_lb(cost, any_lower_bound),
                                count(tokens),
                                share
                                    .map(|s| format!("{s:.1}%"))
                                    .unwrap_or_else(|| EM_DASH.to_string())
                            )),
                        ],
                        share_bar(
                            bar_fraction(cost.unwrap_or(0.0), total_cost.unwrap_or(0.0)),
                            ACCENT
                        ),
                    ]
                    .spacing(4),
                );
            }
        }
    }

    scrollable(page.push(usage)).id(scroll_id()).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CostEntry;

    fn entry(provider: &str, daily: Vec<(&str, f64, i64)>) -> CostEntry {
        CostEntry {
            provider: provider.into(),
            today: 0.0,
            month: 0.0,
            tokens: 0,
            provenance: "listPriceEstimate".into(),
            error: String::new(),
            coverage: "Local history".into(),
            daily: daily
                .into_iter()
                .map(|(d, c, t)| (d.to_string(), c, t))
                .collect(),
        }
    }
    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn spend_money_formats_and_unknown_is_em_dash() {
        assert_eq!(money(Some(12.3)), "$12.30");
        assert_eq!(money(Some(0.0)), "$0.00");
        assert_eq!(money(Some(-1.5)), "-$1.50");
        assert_eq!(money(None), "—");
        assert_eq!(money(Some(f64::NAN)), "—");
        assert_eq!(money(Some(f64::INFINITY)), "—");
        assert_eq!(money_lb(Some(1.0), true), "~$1.00");
        assert_eq!(money_lb(Some(1.0), false), "$1.00");
        assert_eq!(money_lb(None, true), "—");
    }

    #[test]
    fn spend_count_separators_and_unknown() {
        assert_eq!(count(Some(1234567)), "1,234,567");
        assert_eq!(count(Some(0)), "0");
        assert_eq!(count(Some(-1234)), "-1,234");
        assert_eq!(count(None), "—");
    }

    #[test]
    fn spend_bar_fraction_scales_and_clamps() {
        assert_eq!(bar_fraction(5.0, 10.0), 0.5);
        assert_eq!(bar_fraction(10.0, 10.0), 1.0);
        assert_eq!(bar_fraction(20.0, 10.0), 1.0);
        assert_eq!(bar_fraction(-1.0, 10.0), 0.0);
        assert_eq!(bar_fraction(5.0, 0.0), 0.0);
        assert_eq!(bar_fraction(f64::NAN, 10.0), 0.0);
    }

    #[test]
    fn spend_heat_level_and_colors() {
        assert_eq!(heat_level(None, 100), None);
        assert_eq!(heat_level(Some(0), 100), Some(0));
        assert_eq!(heat_level(Some(1), 100), Some(1));
        assert_eq!(heat_level(Some(50), 100), Some(2));
        assert_eq!(heat_level(Some(100), 100), Some(4));
        assert_eq!(heat_level(Some(10), 0), Some(0));
        let unknown = heat_color(None);
        assert!(
            unknown.r == unknown.g && unknown.g == unknown.b,
            "unknown must be gray"
        );
        assert_ne!(heat_color(Some(0)), unknown);
        assert!(heat_color(Some(4)).g > heat_color(Some(4)).r);
        assert_eq!(heat_color(Some(99)), heat_color(Some(4)));
    }

    #[test]
    fn spend_share_lower_bound_provenance_quota() {
        assert_eq!(share_percent(Some(25.0), Some(100.0)), Some(25.0));
        assert_eq!(share_percent(None, Some(100.0)), None);
        assert_eq!(share_percent(Some(1.0), Some(0.0)), None);
        assert!(is_lower_bound(
            "listPriceEstimate",
            "History may be incomplete",
            ""
        ));
        assert!(is_lower_bound("listPriceEstimate", "Local history", "boom"));
        assert!(is_lower_bound("partial", "Local history", ""));
        assert!(!is_lower_bound("actual", "Local history", ""));
        assert_eq!(provenance_label("listPriceEstimate"), "List-price estimate");
        assert_eq!(provenance_label("vendorMetered"), "Reported cost");
        assert_eq!(provenance_label("unknown"), "Cost source unavailable");
        assert_eq!(quota_value(80, "used"), 20);
        assert_eq!(quota_value(80, "remaining"), 80);
        assert_eq!(quota_value(150, "remaining"), 100);
        assert_eq!(period_label(0), "All");
        assert_eq!(period_label(-5), "All");
        assert_eq!(period_label(30), "30d");
    }

    #[test]
    fn spend_daily_range_keeps_unknown_days() {
        let today = day(2026, 11, 8);
        let days = vec![
            ("2026-11-08".to_string(), 2.0, 200),
            ("2026-11-06".to_string(), 1.0, 100),
        ];
        let range = daily_range(&days, 3, today);
        assert_eq!(range.len(), 3);
        assert_eq!(range[0].date, day(2026, 11, 6));
        assert_eq!(range[0].cost, Some(1.0));
        assert_eq!(range[1].cost, None, "gap day must stay unknown, not zero");
        assert_eq!(range[1].tokens, None);
        assert_eq!(range[2].cost, Some(2.0));
        let (cost, tokens, covered, today_cost) = range_totals(&range);
        assert_eq!(cost, Some(3.0));
        assert_eq!(tokens, Some(300));
        assert_eq!(covered, 2);
        assert_eq!(today_cost, Some(2.0));
        // All-period starts at the first recorded day; empty input is empty.
        assert_eq!(daily_range(&days, 0, today).len(), 3);
        assert!(daily_range(&[], 0, today).is_empty());
        let (c, t, cov, tc) = range_totals(&daily_range(&[], 7, today));
        assert_eq!((c, t, cov, tc), (None, None, 0, None));
    }

    #[test]
    fn spend_period_filter_and_errored_entry_unknown() {
        let today = day(2026, 11, 8);
        let mut bad = entry("claude", vec![("2026-11-08", 9.0, 900)]);
        bad.error = "Local cost history unavailable".into();
        bad.month = 42.0;
        assert_eq!(entry_period_cost(&bad, 30, today), None);
        assert_eq!(entry_period_tokens(&bad, 30, today), None);
        let e = entry(
            "codex",
            vec![("2026-10-01", 5.0, 500), ("2026-11-08", 1.0, 10)],
        );
        assert_eq!(entry_period_cost(&e, 7, today), Some(1.0));
        assert_eq!(entry_period_cost(&e, 0, today), Some(6.0));
        // established history without daily rows falls back to month for 30d+/All only
        let mut m = entry("gemini", vec![]);
        m.month = 7.5;
        m.tokens = 70;
        assert_eq!(entry_period_cost(&m, 30, today), Some(7.5));
        assert_eq!(entry_period_cost(&m, 7, today), None);
        assert_eq!(entry_period_tokens(&m, 0, today), Some(70));
    }

    #[test]
    fn spend_heatmap_grid_aligns_monday() {
        // 2026-11-02 is a Monday; 2026-11-08 a Sunday.
        let start = day(2026, 11, 2);
        let days: Vec<_> = (0..7)
            .map(|i| (start + chrono::Duration::days(i), Some(i as usize)))
            .collect();
        let (weeks, cells) = heatmap_cells(&days);
        assert_eq!(weeks, 1);
        assert_eq!(cells.len(), 7);
        assert_eq!(cells[0], (0, 0, Some(0)));
        assert_eq!(cells[6], (0, 6, Some(6)));
        // starting Wednesday: first cell lands in row 2, no padding cells
        let days2: Vec<_> = (0..3)
            .map(|i| (start + chrono::Duration::days(2 + i), None))
            .collect();
        let (_, cells2) = heatmap_cells(&days2);
        assert_eq!(cells2[0], (0, 2, None));
        assert_eq!(heatmap_cells(&[]), (0, Vec::new()));
    }

    #[test]
    fn spend_heatmap_input_prefers_model_intensities() {
        let today = day(2026, 11, 8);
        let mut m = SpendModel::default();
        m.heatmap = vec![
            ("2026-11-07".into(), None),
            ("2026-11-08".into(), Some(4)),
            ("not-a-date".into(), Some(1)),
        ];
        let got = heatmap_input(&m, today);
        assert_eq!(
            got,
            vec![(day(2026, 11, 7), None), (day(2026, 11, 8), Some(4))]
        );
        // fallback: derive from daily tokens
        let mut m2 = SpendModel::default();
        m2.days = vec![("2026-11-08".into(), 1.0, 100)];
        let got2 = heatmap_input(&m2, today);
        assert_eq!(got2, vec![(day(2026, 11, 8), Some(4))]);
    }

    #[test]
    fn spend_provider_ranking_sorted_unknown_last() {
        let today = day(2026, 11, 8);
        let mut bad = entry("claude", vec![]);
        bad.error = "x".into();
        let entries = vec![
            entry("codex", vec![("2026-11-08", 1.0, 10)]),
            entry("gemini", vec![("2026-11-08", 3.0, 30)]),
            bad,
        ];
        let rows = provider_rows(&entries, 30, today);
        assert_eq!(rows[0].0, 1);
        assert_eq!(rows[0].1, "gemini");
        assert_eq!(rows[0].4, Some(75.0));
        assert_eq!(rows[2].1, "claude");
        assert_eq!(rows[2].2, None);
        assert_eq!(rows[2].4, None);
    }

    #[test]
    fn spend_model_ranking_by_cost() {
        let rows = vec![
            ("claude".to_string(), "sonnet".to_string(), 10.0, 100),
            ("claude".to_string(), "opus".to_string(), 30.0, 300),
        ];
        let ranked = rank_models(&rows, Some(40.0));
        assert_eq!(ranked[0].2, "opus");
        assert_eq!(ranked[0].5, Some(75.0));
        assert_eq!(ranked[1].0, 2);
        assert!(rank_models(&[], None).is_empty());
    }
}
