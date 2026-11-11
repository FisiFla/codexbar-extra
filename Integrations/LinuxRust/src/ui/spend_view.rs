//! Usage & Spend dashboard, laid out like the Swift `PreferencesSpendDashboardPane`: header and
//! period picker, time zone, failures, summary card, detail panel (Providers / Models / Projects /
//! Independent chats / Sessions), stacked trend chart, daily ledger, token-activity heatmap and
//! data controls. Charts live in `spend_charts`.
//!
//! Unknown discipline: a value that was never measured renders as "—", never as 0. Days before the
//! first recorded day are unavailable (hatched heatmap cells); an empty trend bucket draws no bar.

use super::spend_charts::{self as sc, glyph, panel, Glyph};
use crate::model::{provider_name, CostEntry};
use crate::scan::aggregate::Zone;
use crate::state::{App, Message};
use crate::ui::style::SECONDARY;
use chrono::{Datelike, NaiveDate};
use iced::widget::{
    button, column, container, horizontal_space, row, scrollable, text, toggler, Canvas, Space,
};
use iced::{Color, Element, Length, Theme};
use std::collections::{BTreeMap, BTreeSet};

pub fn scroll_id() -> scrollable::Id {
    scrollable::Id::new("spend-body")
}

pub const EM_DASH: &str = "—";
/// Activity/chart green (not the app accent; see `style::ACCENT`).
pub(crate) const CHART_ACCENT: Color = Color::from_rgb(0.18, 0.65, 0.35);
const CELL: f32 = 11.0;

// ---------------------------------------------------------------------------
// Pure formatting / scaling helpers (unit-tested below)
// ---------------------------------------------------------------------------

fn finite(v: f64) -> Option<f64> {
    v.is_finite().then_some(v)
}

fn grouped(digits: &str) -> String {
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Grouped currency (mac `currencyGrouping`): "$12.30" under $1,000,
/// "$1,235" (0 decimals) at or above; "—" when unknown (never "$0.00").
pub fn money(v: Option<f64>) -> String {
    match v.and_then(finite) {
        Some(x) => {
            let sign = if x < 0.0 { "-" } else { "" };
            let a = x.abs();
            if a >= 1000.0 {
                format!("{sign}${}", grouped(&format!("{:.0}", a)))
            } else {
                let s = format!("{a:.2}");
                let (int, frac) = s.split_once('.').unwrap_or((s.as_str(), "00"));
                format!("{sign}${}.{frac}", grouped(int))
            }
        }
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
    // Mac palette thresholds: <=30% / <=65% / <=90% / above.
    let ratio = t as f64 / max as f64;
    Some(if ratio <= 0.30 {
        1
    } else if ratio <= 0.65 {
        2
    } else if ratio <= 0.90 {
        3
    } else {
        4
    })
}

/// Mac `SpendActivityPalette` (dark): levels 0-4; unavailable days use the slash fill.
pub fn heat_color(level: Option<usize>) -> Color {
    let hex = match level {
        None => 0x282D34,
        Some(0) => 0x30353A,
        Some(1) => 0x347D4E,
        Some(2) => 0x43985D,
        Some(3) => 0x58B872,
        Some(_) => 0x7DDF96,
    };
    Color::from_rgb8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
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

/// Inline share/quota bar: filled track sized by `fraction` (0..=1).
pub(crate) fn share_bar(fraction: f32, color: Color) -> Element<'static, Message> {
    let filled = (fraction.clamp(0.0, 1.0) * 160.0).max(if fraction > 0.0 { 2.0 } else { 0.0 });
    let track = Length::Fixed(160.0);
    let fill = container(iced::widget::Space::new(
        Length::Fixed(filled),
        Length::Fixed(6.0),
    ))
    .style(move |_theme: &Theme| container::Style {
        background: Some(color.into()),
        border: iced::Border::default().rounded(3.0),
        ..Default::default()
    });
    container(fill)
        .width(track)
        .style(|_theme: &Theme| container::Style {
            background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.12).into()),
            border: iced::Border::default().rounded(3.0),
            ..Default::default()
        })
        .into()
}

pub(crate) fn swatch(color: Color) -> Element<'static, Message> {
    container(text(""))
        .width(Length::Fixed(CELL))
        .height(Length::Fixed(CELL))
        .style(move |_theme: &Theme| container::Style {
            background: Some(color.into()),
            ..Default::default()
        })
        .into()
}

// ---------------------------------------------------------------------------
// UI state owned by the dashboard (one field on `App`, one `Message::Spend`)
// ---------------------------------------------------------------------------

/// Interactive state of the trend panel and expandable lists.
#[derive(Debug, Clone)]
pub struct SpendUi {
    /// Trend panel in Hour mode.
    pub hour: bool,
    /// Pinned overview bucket (its first day).
    pub pin: Option<NaiveDate>,
    /// Day shown in Hour mode (latest day with hourly data when `None`).
    pub hour_day: Option<NaiveDate>,
    /// Pinned hour (0..24) in Hour mode.
    pub hour_pin: Option<usize>,
    /// Provider isolated through the legend.
    pub isolate: Option<String>,
    /// Lists expanded with "Show all" (`ledger`, `models`, `projects`, `chats`, `provider-models`).
    pub expanded: BTreeSet<String>,
    /// Data controls disclosure.
    pub data_open: bool,
}

impl Default for SpendUi {
    fn default() -> Self {
        Self {
            hour: false,
            pin: None,
            hour_day: None,
            hour_pin: None,
            isolate: None,
            expanded: BTreeSet::new(),
            data_open: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SpendMsg {
    SetHour(bool),
    PinBucket(NaiveDate),
    PinHour(usize),
    /// Open Hour mode on this day.
    SetHourDay(NaiveDate),
    Isolate(String),
    ToggleExpand(String),
    ToggleDataControls,
}

impl SpendUi {
    pub fn apply(&mut self, msg: SpendMsg) {
        match msg {
            SpendMsg::SetHour(on) => {
                self.hour = on;
                self.hour_pin = None;
            }
            SpendMsg::PinBucket(d) => self.pin = if self.pin == Some(d) { None } else { Some(d) },
            SpendMsg::PinHour(h) => {
                self.hour_pin = if self.hour_pin == Some(h) { None } else { Some(h) }
            }
            SpendMsg::SetHourDay(d) => {
                self.hour = true;
                self.hour_day = Some(d);
                self.hour_pin = None;
            }
            SpendMsg::Isolate(id) => {
                self.isolate = if self.isolate.as_deref() == Some(id.as_str()) {
                    None
                } else {
                    Some(id)
                }
            }
            SpendMsg::ToggleExpand(k) => {
                if !self.expanded.remove(&k) {
                    self.expanded.insert(k);
                }
            }
            SpendMsg::ToggleDataControls => self.data_open = !self.data_open,
        }
    }
}

// ---------------------------------------------------------------------------
// View helpers
// ---------------------------------------------------------------------------

fn white(a: f32) -> Color {
    Color::from_rgba(1.0, 1.0, 1.0, a)
}

fn semibold() -> iced::Font {
    crate::ui::style::font_semibold()
}

fn title(s: impl Into<String>, size: u16) -> Element<'static, Message> {
    text(s.into()).size(size).font(semibold()).into()
}

fn caption(s: impl Into<String>) -> Element<'static, Message> {
    text(s.into())
        .size(crate::ui::style::TEXT_FOOTNOTE)
        .color(SECONDARY)
        .into()
}

fn rule() -> Element<'static, Message> {
    container(Space::new(Length::Fill, Length::Fixed(1.0)))
        .width(Length::Fill)
        .style(|_: &Theme| container::Style {
            background: Some(white(0.09).into()),
            ..Default::default()
        })
        .into()
}

fn dot(color: Color, size: f32) -> Element<'static, Message> {
    container(Space::new(Length::Fixed(size), Length::Fixed(size)))
        .style(move |_: &Theme| container::Style {
            background: Some(color.into()),
            border: iced::Border::default().rounded(size / 2.0),
            ..Default::default()
        })
        .into()
}

/// Rounded segmented picker (selected segment gets the accent fill).
fn segmented(items: Vec<(String, Message, bool)>) -> Element<'static, Message> {
    // Mac segmented controls give every segment the width of the widest label.
    let widest = items.iter().map(|(l, _, _)| l.chars().count()).max().unwrap_or(0);
    let seg_w = (widest as f32 * 7.2 + 16.0).max(56.0);
    let mut r = row![].spacing(2).align_y(iced::Alignment::Center);
    for (label, msg, selected) in items {
        r = r.push(
            crate::ui::style::button(container(text(label).size(13).wrapping(iced::widget::text::Wrapping::None)).center_x(Length::Fill))
                .width(Length::Fixed(seg_w))
                .padding([3, 8])
                .style(crate::ui::style::segment(selected))
                .on_press(msg),
        );
    }
    container(r)
        .padding(2)
        .style(|_: &Theme| container::Style {
            background: Some(white(0.10).into()),
            border: iced::Border::default().rounded(8.0),
            ..Default::default()
        })
        .into()
}

/// Borderless button (disclosure headers, legend entries, "Show all").
fn flat<'a>(content: impl Into<Element<'a, Message>>, msg: Option<Message>) -> Element<'a, Message> {
    button(content)
        .padding([2, 6])
        .style(|_: &Theme, status| iced::widget::button::Style {
            background: matches!(status, iced::widget::button::Status::Hovered)
                .then(|| white(0.07).into()),
            text_color: Color::from_rgb(0.95, 0.95, 0.97),
            border: iced::Border::default().rounded(6.0),
            ..Default::default()
        })
        .on_press_maybe(msg)
        .into()
}

fn spend(m: SpendMsg) -> Message {
    Message::Spend(m)
}

fn expand_button(app: &App, key: &str, total: usize, shown: usize) -> Option<Element<'static, Message>> {
    let open = app.spend_ui.expanded.contains(key);
    if total <= shown && !open {
        return None;
    }
    let label = if open {
        "Show less".to_string()
    } else {
        format!("Show all ({total})")
    };
    Some(flat(
        text(label).size(12).color(crate::ui::style::ACCENT),
        Some(spend(SpendMsg::ToggleExpand(key.into()))),
    ))
}

/// Full-width thin share bar.
fn thin_bar(fraction: f32, color: Color) -> Element<'static, Message> {
    let filled = (fraction.clamp(0.0, 1.0) * 1000.0).round() as u16;
    let mut r = row![container(Space::new(Length::Fill, Length::Fixed(4.0)))
        .width(Length::FillPortion(filled.max(if fraction > 0.0 { 4 } else { 0 })))
        .style(move |_: &Theme| container::Style {
            background: Some(color.into()),
            border: iced::Border::default().rounded(2.0),
            ..Default::default()
        })];
    let rest = 1000u16.saturating_sub(filled);
    if rest > 0 {
        r = r.push(Space::new(Length::FillPortion(rest), Length::Fixed(4.0)));
    }
    container(r)
        .width(Length::Fill)
        .style(|_: &Theme| container::Style {
            background: Some(white(0.10).into()),
            border: iced::Border::default().rounded(2.0),
            ..Default::default()
        })
        .into()
}

fn right(s: impl Into<String>, size: u16, w: f32) -> Element<'static, Message> {
    text(s.into())
        .size(size)
        .width(Length::Fixed(w))
        .align_x(iced::alignment::Horizontal::Right)
        .into()
}

fn period_segments(period: i64, today: NaiveDate) -> Element<'static, Message> {
    let mtd = today.day() as i64;
    let preset = matches!(period, 7 | 30 | 90 | 0);
    segmented(vec![
        ("7d".into(), Message::SetSpendPeriod(7), period == 7),
        ("30d".into(), Message::SetSpendPeriod(30), period == 30),
        ("90d".into(), Message::SetSpendPeriod(90), period == 90),
        (
            "Month to date".into(),
            Message::SetSpendPeriod(mtd),
            period == mtd && !preset,
        ),
        ("All".into(), Message::SetSpendPeriod(0), period <= 0),
    ])
}

fn zone_text(zone: Zone) -> String {
    match zone {
        Zone::Local => format!("UTC{}", chrono::Local::now().format("%:z")),
        Zone::Utc => "UTC".into(),
    }
}

// ---------------------------------------------------------------------------
// Panels
// ---------------------------------------------------------------------------

fn header(app: &App, today: NaiveDate) -> Element<'static, Message> {
    let busy = app.cost_busy;
    let refresh = crate::ui::style::button(
        row![
            glyph(Glyph::Refresh, 12.0, Color::WHITE),
            text(if busy { "Refreshing…" } else { "Refresh" }).size(13)
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center),
    )
    .on_press_maybe((!busy).then_some(Message::RefreshCosts));
    let zone = app.scan_zone;
    column![
        row![
            column![
                title("Usage & Spend", 17),
                caption("Local estimated cost history across supported providers."),
            ]
            .spacing(2),
            horizontal_space(),
            refresh,
        ]
        .align_y(iced::Alignment::Center),
        period_segments(app.spend_period_days, today),
        row![
            caption("Statistics time zone"),
            segmented(vec![
                ("Local".into(), Message::SetScanZone(Zone::Local), zone == Zone::Local),
                ("UTC".into(), Message::SetScanZone(Zone::Utc), zone == Zone::Utc),
            ]),
            caption(zone_text(zone)),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
        caption("Daily totals use the local date; hourly activity and session times follow this zone."),
    ]
    .spacing(8)
    .into()
}

fn failures_banner(app: &App) -> Option<Element<'static, Message>> {
    let failed = app
        .spending
        .iter()
        .filter(|s| !s.error.trim().is_empty())
        .count();
    if app.cost_error.is_empty() && failed == 0 {
        return None;
    }
    let orange = Color::from_rgb(1.0, 0.62, 0.04);
    let mut col = column![row![
        glyph(Glyph::Warning, 14.0, orange),
        text(format!("Refresh failures: {}", failed.max(1))).size(13).color(orange),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center)]
    .spacing(2);
    if !app.cost_error.is_empty() {
        col = col.push(caption(app.cost_error.clone()));
    }
    Some(col.into())
}

fn track_costs_toggle(app: &App) -> Element<'static, Message> {
    let mut next = app.settings.clone();
    next.show_costs = !next.show_costs;
    toggler(app.settings.show_costs)
        .label("Track costs")
        .size(18)
        .text_size(13)
        .spacing(8)
        .style(crate::ui::style::toggler)
        .on_toggle(move |_| Message::SettingsChanged(Box::new(next.clone())))
        .into()
}

fn data_controls(app: &App) -> Element<'static, Message> {
    let open = app.spend_ui.data_open;
    let mut col = column![flat(
        row![
            glyph(Glyph::Shield, 14.0, SECONDARY),
            text("List-price equivalent — not a billing receipt.").size(13),
            horizontal_space(),
            glyph(if open { Glyph::ChevronUp } else { Glyph::ChevronDown }, 12.0, SECONDARY),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
        Some(spend(SpendMsg::ToggleDataControls)),
    )]
    .spacing(10);
    if open {
        col = col.push(track_costs_toggle(app)).push(caption(
            "Estimates come from local session logs priced at list rates. Unknown values show as —, never 0.",
        ));
    }
    col = col.push(
        row![crate::ui::style::button(
            row![
                glyph(Glyph::Share, 12.0, Color::WHITE),
                text("Share Stats…").size(13)
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center)
        )
        .on_press(Message::ShowPage(crate::state::Page::ShareStats))]
        .spacing(8),
    );
    panel(col)
}

/// One stat of the summary card: caption over a semibold value.
fn stat(label: &str, value: String) -> Element<'static, Message> {
    column![caption(label), title(value, 17)]
        .spacing(2)
        .width(Length::FillPortion(1))
        .into()
}

fn summary_panel(
    app: &App,
    total_cost: Option<f64>,
    total_tokens: Option<i64>,
    today_cost: Option<f64>,
    lb: bool,
) -> Element<'static, Message> {
    let sources = app
        .spending
        .iter()
        .filter(|e| e.error.trim().is_empty())
        .count();
    let col = column![row![
        stat("Estimated spend", money_lb(total_cost, lb)),
        stat(
            "Tracked tokens",
            total_tokens.map_or(EM_DASH.to_string(), |t| super::spend_sessions::fmt_compact(t))
        ),
        stat("Today", money_lb(today_cost, lb)),
        stat("Sources", sources.to_string()),
    ]
    .spacing(12)]
    .spacing(12);
    let mut provenance: Vec<String> = app
        .spending
        .iter()
        .filter(|e| e.error.trim().is_empty())
        .map(|e| provenance_label(&e.provenance))
        .collect();
    provenance.sort();
    provenance.dedup();
    let mut meta = row![].spacing(8).align_y(iced::Alignment::Center);
    if lb {
        meta = meta.push(capsule("Lower bound (~)"));
    }
    if !provenance.is_empty() {
        meta = meta.push(caption(provenance.join(" · ")));
    }
    if let Some(d) = app.spend_ui.pin {
        meta = meta.push(capsule(&format!("Selected {}", d.format("%b %-d"))));
    }
    // Mac lays the summary inline on the pane (no card), closed by a hairline.
    col.push(meta).push(rule()).into()
}

fn capsule(s: &str) -> Element<'static, Message> {
    container(text(s.to_string()).size(11))
        .padding([2, 8])
        .style(|_: &Theme| container::Style {
            background: Some(white(0.09).into()),
            border: iced::Border::default().rounded(10.0),
            ..Default::default()
        })
        .into()
}

fn amount_tokens(cost: Option<f64>, tokens: Option<i64>, lb: bool) -> String {
    if cost.map_or(false, |c| c <= 0.0) && tokens.map_or(false, |t| t <= 0) {
        return "No usage in this period".into();
    }
    format!("{} · {} tokens", money_lb(cost, lb), count(tokens))
}

fn providers_section(app: &App, today: NaiveDate, lb: bool) -> Element<'static, Message> {
    let rows = provider_rows(&app.spending, app.spend_period_days, today);
    if rows.is_empty() {
        return caption("No local cost history yet.");
    }
    let open = app.spend_ui.expanded.contains("provider-models");
    let mut any_more = false;
    let mut col = column![].spacing(10);
    let last = rows.len() - 1;
    for (i, (_rank, id, cost, tokens, _share)) in rows.into_iter().enumerate() {
        let mut block = column![row![
            crate::icons::brand(&id, 20.0, Color::WHITE),
            text(provider_name(&id)).size(14).font(semibold()),
            horizontal_space(),
            text(amount_tokens(cost, tokens, lb && cost.is_some())).size(13),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center)]
        .spacing(6);
        if let Some(e) = app.spending.iter().find(|e| e.provider == id) {
            if !e.error.trim().is_empty() {
                block = block.push(caption(e.error.clone()));
            }
        }
        let models: Vec<&(String, String, f64, i64, f64)> = app
            .spend
            .by_model
            .iter()
            .filter(|m| m.0 == id)
            .collect();
        if models.is_empty() {
            block = block.push(caption("Model breakdown unavailable"));
        } else {
            block = block.push(caption("Models"));
            any_more |= models.len() > 6;
            for m in models.iter().take(if open { usize::MAX } else { 6 }) {
                block = block.push(
                    row![
                        crate::icons::icon(&id, 14.0, white(0.76)),
                        text(m.1.clone()).size(13),
                        horizontal_space(),
                        text(format!("{} · {} tokens", money_lb(Some(m.2), lb), count(Some(m.3))))
                            .size(12)
                            .color(SECONDARY),
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
                );
            }
        }
        col = col.push(block);
        if i != last {
            col = col.push(rule());
        }
    }
    if any_more || open {
        if let Some(b) = expand_button(app, "provider-models", 7, 6) {
            col = col.push(b);
        }
    }
    col.into()
}

fn share_text(share: Option<f64>) -> String {
    match share {
        Some(s) if s < 1.0 => format!("{s:.1}%"),
        Some(s) => format!("{s:.0}%"),
        None => EM_DASH.to_string(),
    }
}

fn models_section(app: &App, total_cost: Option<f64>, lb: bool) -> Element<'static, Message> {
    let rows_in: Vec<(String, String, f64, i64)> = app
        .spend
        .by_model
        .iter()
        .map(|m| (m.0.clone(), m.1.clone(), m.2, m.3))
        .collect();
    let models = rank_models(&rows_in, total_cost);
    if models.is_empty() {
        return caption("No model-level history");
    }
    let total = models.len();
    let open = app.spend_ui.expanded.contains("models");
    let shown = if open || total <= 6 { total } else { 5 };
    let mut col = column![row![horizontal_space(), caption("Estimated spend")]].spacing(10);
    for (_rank, provider, model, cost, _tokens, share) in models.into_iter().take(shown) {
        col = col.push(
            column![
                row![
                    crate::icons::icon(&provider, 24.0, white(0.85)),
                    column![
                        text(model).size(13).font(semibold()),
                        caption(provider_name(&provider))
                    ]
                    .spacing(1),
                    horizontal_space(),
                    text(share_text(share)).size(12).color(SECONDARY),
                    right(money_lb(cost, lb), 13, 76.0),
                ]
                .spacing(10)
                .align_y(iced::Alignment::Center),
                thin_bar(
                    share.map(|s| (s / 100.0) as f32).unwrap_or(0.0),
                    provider_color_of(&provider),
                ),
            ]
            .spacing(6),
        );
    }
    if total > 6 {
        if let Some(b) = expand_button(app, "models", total, 5) {
            col = col.push(b);
        }
    }
    col.into()
}

fn provider_color_of(id: &str) -> Color {
    sc::provider_color(id)
}

fn ranked_row(
    rank: usize,
    icon: Glyph,
    name: String,
    sub: Option<String>,
    value: String,
) -> Element<'static, Message> {
    let mut label = column![text(name).size(13).font(semibold())].spacing(1);
    if let Some(s) = sub {
        label = label.push(caption(s));
    }
    row![
        text(format!("#{rank}")).size(12).color(SECONDARY).width(Length::Fixed(28.0)),
        glyph(icon, 14.0, SECONDARY),
        label,
        horizontal_space(),
        text(value).size(12).color(SECONDARY),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center)
    .into()
}

fn projects_section(app: &App) -> Element<'static, Message> {
    use super::spend_sessions::{fmt_compact, fmt_cost};
    let rows: Vec<_> = app
        .scan_view
        .projects
        .iter()
        .filter(|p| p.name != "(unknown)")
        .collect();
    if rows.is_empty() {
        return caption("No project history in this period.");
    }
    let open = app.spend_ui.expanded.contains("projects");
    let mut col = column![].spacing(8);
    for (i, p) in rows.iter().enumerate().take(if open { usize::MAX } else { 8 }) {
        col = col.push(ranked_row(
            i + 1,
            Glyph::Folder,
            p.name.clone(),
            Some(format!("{} sessions · {} requests", p.sessions, count(Some(p.requests as i64)))),
            format!("{} · {} tokens", fmt_cost(&p.cost), fmt_compact(p.tokens.total())),
        ));
    }
    if let Some(b) = expand_button(app, "projects", rows.len(), 8) {
        col = col.push(b);
    }
    col.into()
}

fn chats_section(app: &App) -> Element<'static, Message> {
    use super::spend_sessions::{fmt_compact, fmt_cost, fmt_ts, short_id};
    let rows: Vec<_> = app
        .scan_view
        .sessions
        .iter()
        .filter(|s| s.project.is_none())
        .collect();
    if rows.is_empty() {
        return caption("No independent chats in this period.");
    }
    let open = app.spend_ui.expanded.contains("chats");
    let mut col = column![].spacing(8);
    for (i, s) in rows.iter().enumerate().take(if open { usize::MAX } else { 8 }) {
        col = col.push(ranked_row(
            i + 1,
            Glyph::Bubble,
            format!("{} · {}", s.provider.label(), short_id(&s.id)),
            Some(format!("{} · {}", s.primary_model(), fmt_ts(s.start_ms, app.scan_zone))),
            format!("{} · {} tokens", fmt_cost(&s.cost), fmt_compact(s.tokens.total())),
        ));
    }
    if let Some(b) = expand_button(app, "chats", rows.len(), 8) {
        col = col.push(b);
    }
    col.into()
}

fn detail_panel(app: &App, today: NaiveDate, total_cost: Option<f64>, lb: bool) -> Element<'static, Message> {
    let has_models = !app.spend.by_model.is_empty();
    let scan = app.scan_loaded;
    let has_projects = scan
        && app
            .scan_view
            .projects
            .iter()
            .any(|p| p.name != "(unknown)");
    let has_chats = scan && app.scan_view.sessions.iter().any(|s| s.project.is_none());
    let has_sessions = scan && !app.scan_view.sessions.is_empty();
    let mut sections: Vec<(&str, &str)> = vec![("providers", "Providers")];
    if has_models {
        sections.push(("models", "Models"));
    }
    if has_projects {
        sections.push(("projects", "Projects"));
    }
    if has_chats {
        sections.push(("chats", "Independent chats"));
    }
    if has_sessions {
        sections.push(("sessions", "Sessions"));
    }
    let current = sections
        .iter()
        .find(|(k, _)| *k == app.spend_tab)
        .map(|(k, _)| *k)
        .unwrap_or("providers");
    let mut col = column![].spacing(12);
    if sections.len() > 1 {
        col = col.push(segmented(
            sections
                .iter()
                .map(|(k, l)| {
                    (
                        l.to_string(),
                        Message::SetSpendTab(k.to_string()),
                        *k == current,
                    )
                })
                .collect(),
        ));
    }
    col = col.push(match current {
        "models" => models_section(app, total_cost, lb),
        "projects" => projects_section(app),
        "chats" => chats_section(app),
        "sessions" => sessions_section(app),
        _ => providers_section(app, today, lb),
    });
    panel(col)
}

fn sessions_section(app: &App) -> Element<'static, Message> {
    column![
        super::spend_sessions::sessions_tab(app),
        super::spend_sessions::sections(app)
    ]
    .spacing(16)
    .into()
}

// ----- trend ---------------------------------------------------------------

fn trend_points(app: &App, today: NaiveDate) -> Vec<(String, NaiveDate, f64)> {
    let period = app.spend_period_days;
    app.spending
        .iter()
        .filter(|e| e.error.trim().is_empty())
        .flat_map(|e| {
            e.daily.iter().filter_map(move |(d, c, _)| {
                let date = parse_date(d)?;
                (in_period(date, period, today) && c.is_finite())
                    .then(|| (e.provider.clone(), date, *c))
            })
        })
        .collect()
}

fn inspection(
    b: &sc::Bucket,
    unit: sc::Unit,
    open_hour: Option<NaiveDate>,
    lb: bool,
) -> Element<'static, Message> {
    let mut head = row![text(sc::bucket_range(b, unit)).size(13).font(semibold())]
        .spacing(10)
        .align_y(iced::Alignment::Center);
    if let Some(day) = open_hour {
        head = head.push(
            crate::ui::style::button(text("Hour").size(12))
                .padding([2, 10])
                .on_press(spend(SpendMsg::SetHourDay(day))),
        );
    }
    head = head
        .push(horizontal_space())
        .push(text(money_lb(Some(b.total).filter(|t| *t > 0.0 || b.has_data()), lb)).size(13).font(semibold()));
    let mut col = column![head].spacing(6);
    if b.segs.is_empty() {
        col = col.push(caption("No recorded spend"));
    }
    for s in &b.segs {
        col = col.push(
            row![
                dot(sc::provider_color(&s.id), 8.0),
                crate::icons::brand(&s.id, 16.0, Color::WHITE),
                text(provider_name(&s.id)).size(12),
                horizontal_space(),
                text(money_lb(Some(s.value), lb)).size(12).color(SECONDARY),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        );
    }
    container(col)
        .padding(12)
        .width(Length::Fill)
        .style(|_: &Theme| container::Style {
            background: Some(white(0.04).into()),
            border: iced::Border {
                color: white(0.07),
                width: 1.0,
                radius: 10.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn trend_panel(app: &App, today: NaiveDate, lb: bool) -> Option<Element<'static, Message>> {
    let ui = &app.spend_ui;
    let period = app.spend_period_days;
    let points = trend_points(app, today);
    let hourly = app
        .scan
        .as_ref()
        .map(|s| sc::hourly_for(s, app.scan_zone))
        .filter(|h| !h.days.is_empty());
    if points.is_empty() && hourly.is_none() {
        return None;
    }
    let hour_mode = ui.hour && hourly.is_some();
    let isolate = ui.isolate.as_deref();
    let start = if period > 0 {
        today - chrono::Duration::days(period - 1)
    } else {
        points.iter().map(|p| p.1).min().unwrap_or(today)
    };
    let unit = sc::overview_unit((today - start).num_days() + 1);

    let (all, buckets, day) = match (&hourly, hour_mode) {
        (Some(idx), true) => {
            let day = ui
                .hour_day
                .filter(|d| idx.days.contains_key(d))
                .or_else(|| idx.days.keys().next_back().copied())?;
            let all = sc::hour_buckets(idx, day, None).unwrap_or_default();
            let shown = sc::hour_buckets(idx, day, isolate).unwrap_or_default();
            (all, shown, Some(day))
        }
        _ => {
            let all = sc::build_buckets(&points, unit, (start, today), None);
            let shown = sc::build_buckets(&points, unit, (start, today), isolate);
            (all, shown, None)
        }
    };
    if buckets.is_empty() {
        return None;
    }
    let mut providers: Vec<String> = all
        .iter()
        .flat_map(|b| b.segs.iter().map(|s| s.id.clone()))
        .collect();
    providers.sort();
    providers.dedup();

    // Header: title and the Overview / Hour picker.
    let mut head = row![title(
        if hour_mode { "Hourly estimated spend" } else { "Estimated spend" },
        16
    )]
    .align_y(iced::Alignment::Center);
    head = head.push(horizontal_space());
    if hourly.is_some() {
        head = head.push(segmented(vec![
            ("Overview".into(), spend(SpendMsg::SetHour(false)), !hour_mode),
            ("Hour".into(), spend(SpendMsg::SetHour(true)), hour_mode),
        ]));
    }
    let mut col = column![head].spacing(10);

    // Hour mode: day navigation.
    if let (Some(idx), Some(day)) = (&hourly, day) {
        let prev = idx.days.range(..day).next_back().map(|(d, _)| *d);
        let next = idx
            .days
            .range(day + chrono::Duration::days(1)..)
            .next()
            .map(|(d, _)| *d);
        let nav = |g: Glyph, to: Option<NaiveDate>| -> Element<'static, Message> {
            crate::ui::style::button(glyph(g, 12.0, Color::WHITE))
                .padding([3, 8])
                .on_press_maybe(to.map(|d| spend(SpendMsg::SetHourDay(d))))
                .into()
        };
        col = col.push(
            row![
                nav(Glyph::ChevronLeft, prev),
                glyph(Glyph::Calendar, 13.0, SECONDARY),
                text(day.format("%a, %b %-d, %Y").to_string()).size(13),
                nav(Glyph::ChevronRight, next),
                horizontal_space(),
                caption(zone_text(app.scan_zone)),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        );
    }

    // Summary: total on the left, highest bucket on the right.
    let total: f64 = buckets.iter().map(|b| b.total).sum();
    let peak = sc::peak_bucket(&buckets);
    let lb_total = lb || hour_mode;
    col = col.push(
        row![
            column![
                caption(if hour_mode { "Recorded hourly spend" } else { "Recorded spend" }),
                text(money_lb(Some(total).filter(|_| peak.is_some()), lb_total))
                    .size(20)
                    .font(semibold())
                    .wrapping(iced::widget::text::Wrapping::None),
            ]
            .spacing(2),
            horizontal_space(),
            column![
                caption(match peak {
                    Some(i) => format!("Highest recorded spend {}", sc::bucket_range(&buckets[i], unit)),
                    None => "Highest recorded spend".to_string(),
                }),
                text(money_lb(peak.map(|i| buckets[i].total), lb_total))
                    .size(16)
                    .font(semibold())
                    .wrapping(iced::widget::text::Wrapping::None),
            ]
            .spacing(2)
            .align_x(iced::Alignment::End),
        ]
        .align_y(iced::Alignment::End),
    );
    col = col.push(caption(if hour_mode {
        format!(
            "{} · Hourly",
            day.map(|d| d.format("%a, %b %-d").to_string()).unwrap_or_default()
        )
    } else {
        format!(
            "{} – {} · {}",
            start.format("%b %-d"),
            today.format("%b %-d"),
            unit.noun()
        )
    }));

    // Chart.
    let hover = app
        .chart_hover
        .as_ref()
        .filter(|(k, _)| k == "spend-trend")
        .map(|(_, i)| *i)
        .filter(|i| *i < buckets.len());
    let selected = if hour_mode {
        ui.hour_pin.filter(|i| *i < buckets.len())
    } else {
        ui.pin.and_then(|d| buckets.iter().position(|b| b.start <= d && d <= b.end))
    };
    let on_click: Vec<SpendMsg> = buckets
        .iter()
        .enumerate()
        .map(|(i, b)| {
            if hour_mode {
                SpendMsg::PinHour(i)
            } else {
                SpendMsg::PinBucket(b.start)
            }
        })
        .collect();
    let y_max = sc::y_domain(&buckets);
    let shown = hover.or(selected).or(peak).unwrap_or(0);
    let inspected = buckets[shown].clone();
    let open_hour = (!hour_mode && unit == sc::Unit::Day)
        .then(|| hourly.as_ref().filter(|h| h.days.contains_key(&inspected.start)))
        .flatten()
        .map(|_| inspected.start);
    col = col.push(
        Canvas::new(sc::TrendChart {
            buckets,
            unit,
            hourly: hour_mode,
            y_max,
            hover,
            selected,
            on_click,
        })
        .width(Length::Fill)
        .height(Length::Fixed(190.0)),
    );
    col = col.push(caption(
        "Click a bar to explore. Use the menu below for exact amounts.",
    ));
    col = col.push(inspection(&inspected, unit, open_hour, lb_total));
    if hour_mode {
        col = col.push(caption(
            "Hourly figures come from local session logs; unpriced requests are excluded.",
        ));
    }

    // Legend (click to isolate a source; "All" clears the isolation).
    let mut legend = Vec::new();
    {
        let clear = isolate.map(|cur| spend(SpendMsg::Isolate(cur.to_string())));
        let a = if isolate.is_some() { 0.6 } else { 1.0 };
        legend.push(flat(
            row![
                dot(Color { a, ..Color::WHITE }, 8.0),
                text("All").size(12).color(Color { a, ..Color::WHITE }),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
            clear,
        ));
    }
    for id in &providers {
        let dimmed = isolate.is_some_and(|i| i != id);
        let a = if dimmed { 0.6 } else { 1.0 };
        let c = sc::provider_color(id);
        legend.push(flat(
            row![
                dot(Color { a, ..c }, 8.0),
                crate::icons::brand(id, 14.0, Color { a, ..Color::WHITE }),
                text(provider_name(id)).size(12).color(Color { a, ..Color::WHITE }),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
            Some(spend(SpendMsg::Isolate(id.clone()))),
        ));
    }
    col = col.push(row(legend).spacing(6).wrap());
    Some(panel(col))
}

// ----- ledger ----------------------------------------------------------------

fn ledger_panel(app: &App, today: NaiveDate, lb: bool) -> Option<Element<'static, Message>> {
    let period = app.spend_period_days;
    let range = daily_range(&app.spend.days, if period > 0 { period } else { 0 }, today);
    let rows: Vec<&DailyTotal> = range
        .iter()
        .rev()
        .skip_while(|d| d.cost.is_none() && d.tokens.is_none())
        .collect();
    if rows.is_empty() {
        return None;
    }
    let mut by_day: BTreeMap<NaiveDate, Vec<String>> = BTreeMap::new();
    for e in app.spending.iter().filter(|e| e.error.trim().is_empty()) {
        for (d, c, t) in &e.daily {
            if let Some(date) = parse_date(d) {
                if *c > 0.0 || *t > 0 {
                    by_day.entry(date).or_default().push(e.provider.clone());
                }
            }
        }
    }
    let requests = app
        .scan
        .as_ref()
        .map(|s| sc::hourly_for(s, app.scan_zone));
    let open = app.spend_ui.expanded.contains("ledger");
    let shown = if open { rows.len() } else { rows.len().min(30) };
    let mut col = column![
        title("Daily estimated spend", 16),
        row![
            text("Day").size(12).color(SECONDARY).width(Length::Fixed(100.0)),
            text("Providers").size(12).color(SECONDARY).width(Length::Fill),
            right("Tracked tokens", 12, 110.0),
            right("Requests", 12, 70.0),
            right("Estimated spend", 12, 110.0),
        ]
        .spacing(8),
        rule(),
    ]
    .spacing(8);
    for d in rows.iter().take(shown) {
        let ids = by_day.get(&d.date).cloned().unwrap_or_default();
        let mut ids: Vec<String> = ids;
        ids.sort();
        ids.dedup();
        let mut prov = row![].spacing(4).align_y(iced::Alignment::Center);
        if ids.is_empty() {
            prov = prov.push(caption("No usage yet"));
        } else {
            for id in ids.iter().take(4) {
                prov = prov.push(crate::icons::brand(id, 16.0, Color::WHITE));
            }
            if ids.len() > 4 {
                prov = prov.push(caption(format!("+{}", ids.len() - 4)));
            }
        }
        let req = requests
            .as_ref()
            .and_then(|h| h.requests.get(&d.date))
            .map(|n| *n as i64);
        col = col.push(
            row![
                text(d.date.format("%a, %b %-d").to_string())
                    .size(13)
                    .width(Length::Fixed(100.0)),
                container(prov).width(Length::Fill),
                right(count(d.tokens), 13, 110.0),
                right(count(req), 13, 70.0),
                right(money_lb(d.cost, lb && d.cost.is_some()), 13, 110.0),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        );
    }
    if let Some(b) = expand_button(app, "ledger", rows.len(), 30) {
        col = col.push(b);
    }
    if lb {
        col = col.push(caption(
            "~ marks a lower bound: some history is incomplete or unpriced.",
        ));
    }
    Some(panel(col))
}

// ----- heatmap ---------------------------------------------------------------

fn heatmap_panel(app: &App, today: NaiveDate) -> Element<'static, Message> {
    let grid = sc::activity_grid(&app.spend.days, today);
    let mut col = column![row![
        title("Token activity", 16),
        horizontal_space(),
        caption(if grid.cells.is_empty() {
            "Unavailable".to_string()
        } else {
            format!("{} tokens in the last year", count(Some(grid.total)))
        }),
    ]
    .align_y(iced::Alignment::Center)]
    .spacing(10);
    if grid.cells.is_empty() {
        return panel(col.push(caption("No activity in the last 12 months")));
    }
    let hover = app
        .chart_hover
        .as_ref()
        .filter(|(k, _)| k == "spend-heat")
        .map(|(_, i)| *i);
    col = col.push(
        Canvas::new(sc::HeatmapChart { grid, hover })
            .width(Length::Fill)
            .height(Length::Fixed(sc::heat_height())),
    );
    let mut legend = row![caption("Less")].spacing(4).align_y(iced::Alignment::Center);
    for l in 0..5 {
        legend = legend.push(swatch(heat_color(Some(l))));
    }
    legend = legend.push(caption("More")).push(Space::with_width(12));
    legend = legend.push(
        container(Space::new(Length::Fixed(CELL), Length::Fixed(CELL))).style(|_: &Theme| {
            container::Style {
                background: Some(Color::from_rgb8(0x28, 0x2D, 0x34).into()),
                border: iced::Border {
                    color: Color::from_rgb8(0x8A, 0x95, 0x9F),
                    width: 0.75,
                    radius: 2.0.into(),
                },
                ..Default::default()
            }
        }),
    );
    legend = legend.push(caption("Unavailable"));
    panel(col.push(legend))
}

/// The Usage & Spend window content: the standalone scrolling view of [`dashboard`].
pub fn view(app: &App) -> Element<'_, Message> {
    crate::ui::style::scrollable(dashboard(app))
        .id(scroll_id())
        .into()
}

/// The whole dashboard without an outer scrollable (the Settings pane wraps it in one with
/// [`scroll_id`]). Section order follows `PreferencesSpendDashboardPane`.
pub fn dashboard(app: &App) -> Element<'_, Message> {
    let today = chrono::Local::now().date_naive();
    let period = app.spend_period_days;
    let mut page = column![header(app, today)].spacing(14).padding(16);
    if let Some(b) = failures_banner(app) {
        page = page.push(b);
    }
    if !app.settings.show_costs {
        page = page.push(panel(
            column![
                title("Cost tracking is off", 14),
                caption("Turn on Track costs to build local estimates."),
            ]
            .spacing(4),
        ));
        return page.push(data_controls(app)).into();
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
    let lb = app
        .spending
        .iter()
        .any(|e| is_lower_bound(&e.provenance, &e.coverage, &e.error));
    let has_spend = total_cost.is_some() || covered > 0 || !app.spend.by_model.is_empty();
    if !has_spend {
        page = page.push(panel(
            column![
                title(
                    if app.cost_busy { "Reading local history…" } else { "No local cost history yet" },
                    14
                ),
                caption("Turn on cost tracking or refresh after using a supported provider."),
            ]
            .spacing(4),
        ));
        return page.push(data_controls(app)).into();
    }

    // Coverage caption; the total itself lives in the summary stats below.
    let coverage = if range.is_empty() {
        EM_DASH.to_string()
    } else {
        format!("{covered} of {} days covered", range.len())
    };
    page = page.push(caption(if lb {
        format!("Partial estimate · {coverage}")
    } else {
        format!("Local estimated history · {coverage}")
    }));
    page = page.push(summary_panel(app, total_cost, total_tokens, today_cost, lb));
    page = page.push(detail_panel(app, today, total_cost, lb));
    if let Some(t) = trend_panel(app, today, lb) {
        page = page.push(t);
    }
    if let Some(l) = ledger_panel(app, today, lb) {
        page = page.push(l);
    }
    page = page.push(heatmap_panel(app, today));
    page.push(data_controls(app)).into()
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
    fn spend_money_groups_and_drops_decimals_at_thousand() {
        assert_eq!(money(Some(999.99)), "$999.99");
        assert_eq!(money(Some(1000.0)), "$1,000");
        assert_eq!(money(Some(1234.56)), "$1,235");
        assert_eq!(money(Some(-1_234_567.0)), "-$1,234,567");
    }

    #[test]
    fn spend_heat_level_threshold_boundaries() {
        assert_eq!(heat_level(Some(30), 100), Some(1));
        assert_eq!(heat_level(Some(31), 100), Some(2));
        assert_eq!(heat_level(Some(65), 100), Some(2));
        assert_eq!(heat_level(Some(66), 100), Some(3));
        assert_eq!(heat_level(Some(90), 100), Some(3));
        assert_eq!(heat_level(Some(91), 100), Some(4));
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
        // Mac dark palette: level 0 and 4, and the distinct unavailable fill.
        assert_eq!(heat_color(Some(0)), Color::from_rgb8(0x30, 0x35, 0x3A));
        assert_eq!(heat_color(Some(4)), Color::from_rgb8(0x7D, 0xDF, 0x96));
        assert_eq!(heat_color(None), Color::from_rgb8(0x28, 0x2D, 0x34));
        assert_ne!(heat_color(Some(0)), heat_color(None));
        assert_eq!(heat_color(Some(99)), heat_color(Some(4)));
    }

    #[test]
    fn spend_share_lower_bound_provenance() {
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

    #[test]
    fn spend_ui_pin_toggles_and_hour_day_opens_hour_mode() {
        let mut ui = SpendUi::default();
        assert!(ui.data_open && !ui.hour);
        ui.apply(SpendMsg::PinBucket(day(2026, 11, 3)));
        assert_eq!(ui.pin, Some(day(2026, 11, 3)));
        ui.apply(SpendMsg::PinBucket(day(2026, 11, 3)));
        assert_eq!(ui.pin, None);
        ui.apply(SpendMsg::SetHourDay(day(2026, 11, 4)));
        assert!(ui.hour);
        assert_eq!(ui.hour_day, Some(day(2026, 11, 4)));
        ui.apply(SpendMsg::PinHour(9));
        assert_eq!(ui.hour_pin, Some(9));
        ui.apply(SpendMsg::SetHour(false));
        assert_eq!(ui.hour_pin, None);
    }

    #[test]
    fn spend_ui_isolate_and_expand_toggle() {
        let mut ui = SpendUi::default();
        ui.apply(SpendMsg::Isolate("codex".into()));
        assert_eq!(ui.isolate.as_deref(), Some("codex"));
        ui.apply(SpendMsg::Isolate("claude".into()));
        assert_eq!(ui.isolate.as_deref(), Some("claude"));
        ui.apply(SpendMsg::Isolate("claude".into()));
        assert_eq!(ui.isolate, None);
        ui.apply(SpendMsg::ToggleExpand("ledger".into()));
        assert!(ui.expanded.contains("ledger"));
        ui.apply(SpendMsg::ToggleExpand("ledger".into()));
        assert!(ui.expanded.is_empty());
        ui.apply(SpendMsg::ToggleDataControls);
        assert!(!ui.data_open);
    }

    #[test]
    fn spend_share_text_precision() {
        assert_eq!(share_text(Some(45.4)), "45%");
        assert_eq!(share_text(Some(0.42)), "0.4%");
        assert_eq!(share_text(None), EM_DASH);
    }
}
