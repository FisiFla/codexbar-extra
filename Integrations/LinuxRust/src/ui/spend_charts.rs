//! Charts and small view helpers of the Usage & Spend dashboard (mirrors the macOS
//! `SpendDashboardTrendPanel`, `SpendTrendChartModel` and `SpendActivityHeatmap`).
//!
//! Pure data shaping (bucketing, stacking, y ticks, hourly index, activity grid) is unit-tested;
//! `TrendChart` and `HeatmapChart` are thin canvas programs on top of it. Unknown stays unknown:
//! a bucket with no recorded spend draws no bar, and unavailable heatmap days are hatched.

use super::spend_view::{heat_color, heat_level, SpendMsg};
use crate::charts::{bar_frames, hit_test, label_width, place_label, plot_rect, segment_color};
use crate::scan::aggregate::Zone;
use crate::scan::ScanSnapshot;
use crate::state::Message;
use chrono::{Datelike, Local, NaiveDate, TimeZone, Timelike, Utc};
use iced::widget::canvas::{self, Frame, Geometry, Path, Text};
use iced::widget::{container, svg};
use iced::{alignment, border, mouse, Background, Border, Color, Element, Length};
use iced::{Pixels, Point, Rectangle, Renderer, Size, Theme};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// `SpendTrendChartModel` bar width ratio.
pub const BAR_RATIO: f32 = 0.72;

// ---------------------------------------------------------------------------
// Bucketing (Overview = day / week / month, Hour = 24 hourly buckets)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Day,
    Week,
    Month,
}

impl Unit {
    pub fn noun(self) -> &'static str {
        match self {
            Unit::Day => "Daily",
            Unit::Week => "Weekly",
            Unit::Month => "Monthly",
        }
    }
}

/// Mac `overviewUnit`: more than 180 days is monthly, more than 45 weekly, else daily.
pub fn overview_unit(days: i64) -> Unit {
    if days > 180 {
        Unit::Month
    } else if days > 45 {
        Unit::Week
    } else {
        Unit::Day
    }
}

/// First day of the bucket containing `d` (weeks start on Monday).
pub fn bucket_start(d: NaiveDate, unit: Unit) -> NaiveDate {
    match unit {
        Unit::Day => d,
        Unit::Week => d - chrono::Duration::days(d.weekday().num_days_from_monday() as i64),
        Unit::Month => d.with_day(1).unwrap_or(d),
    }
}

fn next_start(d: NaiveDate, unit: Unit) -> NaiveDate {
    match unit {
        Unit::Day => d + chrono::Duration::days(1),
        Unit::Week => d + chrono::Duration::days(7),
        Unit::Month => {
            let (y, m) = if d.month() == 12 {
                (d.year() + 1, 1)
            } else {
                (d.year(), d.month() + 1)
            };
            NaiveDate::from_ymd_opt(y, m, 1).unwrap_or(d + chrono::Duration::days(31))
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Seg {
    pub id: String,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bucket {
    pub start: NaiveDate,
    /// Inclusive last day of the bucket.
    pub end: NaiveDate,
    /// Hour of day for hourly buckets.
    pub hour: Option<u32>,
    /// Per-provider amounts, ascending by provider id (the stack order, baseline first).
    pub segs: Vec<Seg>,
    pub total: f64,
}

impl Bucket {
    pub fn has_data(&self) -> bool {
        !self.segs.is_empty()
    }
}

fn stack(map: BTreeMap<String, f64>, isolate: Option<&str>) -> (Vec<Seg>, f64) {
    let segs: Vec<Seg> = map
        .into_iter()
        .filter(|(id, v)| v.is_finite() && *v > 0.0 && isolate.is_none_or(|i| i == id))
        .map(|(id, value)| Seg { id, value })
        .collect();
    let total = segs.iter().map(|s| s.value).sum();
    (segs, total)
}

/// Continuous buckets covering `range` (inclusive). `points` are (provider, day, cost);
/// segments are sorted by provider id. `isolate` keeps a single provider.
pub fn build_buckets(
    points: &[(String, NaiveDate, f64)],
    unit: Unit,
    range: (NaiveDate, NaiveDate),
    isolate: Option<&str>,
) -> Vec<Bucket> {
    if range.0 > range.1 {
        return Vec::new();
    }
    let mut by: BTreeMap<NaiveDate, BTreeMap<String, f64>> = BTreeMap::new();
    for (id, day, cost) in points {
        if *day < range.0 || *day > range.1 || !cost.is_finite() {
            continue;
        }
        *by.entry(bucket_start(*day, unit))
            .or_default()
            .entry(id.clone())
            .or_insert(0.0) += cost;
    }
    let mut out = Vec::new();
    let mut start = bucket_start(range.0, unit);
    while start <= range.1 {
        let next = next_start(start, unit);
        let (segs, total) = stack(by.remove(&start).unwrap_or_default(), isolate);
        out.push(Bucket {
            start,
            end: next - chrono::Duration::days(1),
            hour: None,
            segs,
            total,
        });
        start = next;
    }
    out
}

/// Index of the highest bucket (first on ties); `None` when nothing was spent.
pub fn peak_bucket(buckets: &[Bucket]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (i, b) in buckets.iter().enumerate() {
        if b.total > 0.0 && best.is_none_or(|j| b.total > buckets[j].total) {
            best = Some(i);
        }
    }
    best
}

/// Mac y domain: `0...max(0.01, peak * 1.15)`.
pub fn y_domain(buckets: &[Bucket]) -> f64 {
    let peak = buckets.iter().map(|b| b.total).fold(0.0, f64::max);
    (peak * 1.15).max(0.01)
}

/// "Automatic, desired count 4" tick values: 0 then multiples of a 1 / 2 / 2.5 / 5 x 10^k step.
pub fn nice_ticks(max: f64, desired: usize) -> Vec<f64> {
    if !max.is_finite() || max <= 0.0 || desired == 0 {
        return Vec::new();
    }
    let raw = max / desired as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0]
        .iter()
        .map(|m| m * mag)
        .find(|s| *s >= raw - 1e-12)
        .unwrap_or(10.0 * mag);
    let mut out = Vec::new();
    let mut i = 0.0;
    while i * step <= max + step * 1e-9 {
        out.push((i * step * 1e9).round() / 1e9);
        i += 1.0;
    }
    out
}

/// Compact y-axis money label: $0, $0.50, $12, $2.5K.
pub fn axis_label(v: f64) -> String {
    if v >= 1000.0 {
        let k = v / 1000.0;
        if (k - k.round()).abs() < 0.05 {
            format!("${:.0}K", k)
        } else {
            format!("${:.1}K", k)
        }
    } else if v.abs() < 1e-9 {
        "$0".into()
    } else if v >= 10.0 || (v - v.round()).abs() < 1e-9 {
        format!("${:.0}", v)
    } else {
        format!("${:.2}", v)
    }
}

/// Date label of a bucket on the x axis.
pub fn bucket_label(b: &Bucket, unit: Unit) -> String {
    match (b.hour, unit) {
        (Some(h), _) => format!("{h:02}:00"),
        (None, Unit::Month) => b.start.format("%b %Y").to_string(),
        (None, _) => b.start.format("%b %-d").to_string(),
    }
}

/// Full range text of a bucket: "Nov 3", "Nov 3 – Nov 9", "Nov 2026", "Nov 3, 14:00".
pub fn bucket_range(b: &Bucket, unit: Unit) -> String {
    match (b.hour, unit) {
        (Some(h), _) => format!("{}, {h:02}:00", b.start.format("%b %-d")),
        (None, Unit::Day) => b.start.format("%b %-d").to_string(),
        (None, Unit::Week) => format!("{} – {}", b.start.format("%b %-d"), b.end.format("%b %-d")),
        (None, Unit::Month) => b.start.format("%B %Y").to_string(),
    }
}

// ---------------------------------------------------------------------------
// Hourly index (from the local session scan, priced requests only)
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct HourlyIndex {
    /// day -> 24 hours -> provider id -> priced cost.
    pub days: BTreeMap<NaiveDate, Vec<BTreeMap<String, f64>>>,
    /// day -> number of scanned requests (priced or not).
    pub requests: BTreeMap<NaiveDate, usize>,
}

fn provider_id(p: crate::scan::Provider) -> String {
    p.label().to_lowercase()
}

fn local_parts(ms: i64, zone: Zone) -> Option<(NaiveDate, u32)> {
    match zone {
        Zone::Utc => Utc.timestamp_millis_opt(ms).single().map(|d| (d.date_naive(), d.hour())),
        Zone::Local => Local
            .timestamp_millis_opt(ms)
            .single()
            .map(|d| (d.date_naive(), d.hour())),
    }
}

pub fn build_hourly(snap: &ScanSnapshot, zone: Zone) -> HourlyIndex {
    let mut idx = HourlyIndex::default();
    for (prov, _sid, req) in &snap.reqs {
        let Some((day, hour)) = local_parts(req.t, zone) else {
            continue;
        };
        *idx.requests.entry(day).or_insert(0) += 1;
        let Some(c) = req.c.filter(|c| c.is_finite() && *c > 0.0) else {
            continue;
        };
        let hours = idx
            .days
            .entry(day)
            .or_insert_with(|| vec![BTreeMap::new(); 24]);
        *hours[hour as usize].entry(provider_id(*prov)).or_insert(0.0) += c;
    }
    idx
}

type HourlyCache = Mutex<Option<(usize, Zone, Arc<HourlyIndex>)>>;
static HOURLY: HourlyCache = Mutex::new(None);

/// Cached [`build_hourly`] keyed by the snapshot identity and zone (the view redraws often).
pub fn hourly_for(snap: &Arc<ScanSnapshot>, zone: Zone) -> Arc<HourlyIndex> {
    let key = Arc::as_ptr(snap) as usize;
    let mut guard = HOURLY.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((k, z, idx)) = guard.as_ref() {
        if *k == key && *z == zone {
            return idx.clone();
        }
    }
    let idx = Arc::new(build_hourly(snap, zone));
    *guard = Some((key, zone, idx.clone()));
    idx
}

/// The 24 buckets of one day. `None` when the day has no hourly data.
pub fn hour_buckets(idx: &HourlyIndex, day: NaiveDate, isolate: Option<&str>) -> Option<Vec<Bucket>> {
    let hours = idx.days.get(&day)?;
    Some(
        hours
            .iter()
            .enumerate()
            .map(|(h, m)| {
                let (segs, total) = stack(m.clone(), isolate);
                Bucket {
                    start: day,
                    end: day,
                    hour: Some(h as u32),
                    segs,
                    total,
                }
            })
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// Provider colours
// ---------------------------------------------------------------------------

pub fn parse_hex(hex: &str) -> Option<Color> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(Color::from_rgb8((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// Provider accent for chart segments; near-black accents are lifted so they read on dark.
pub fn provider_color(id: &str) -> Color {
    let c = crate::descriptors::accent_hex(id)
        .and_then(parse_hex)
        .unwrap_or_else(|| {
            let n: usize = id.bytes().map(|b| b as usize).sum();
            segment_color(n % 6)
        });
    let luma = 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
    if luma < 0.22 {
        Color::from_rgb(
            c.r + (1.0 - c.r) * 0.55,
            c.g + (1.0 - c.g) * 0.55,
            c.b + (1.0 - c.b) * 0.55,
        )
    } else {
        c
    }
}

// ---------------------------------------------------------------------------
// Activity heatmap model (53 weeks x 7 days, Sunday first)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct HeatCell {
    pub col: usize,
    pub row: usize,
    pub date: NaiveDate,
    /// `None` = unavailable (before recorded coverage).
    pub tokens: Option<i64>,
    pub level: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct HeatGrid {
    pub weeks: usize,
    pub cells: Vec<HeatCell>,
    pub months: Vec<(usize, String)>,
    pub total: i64,
}

/// 365-day window ending `today`. Days before the first recorded day are unavailable; recorded
/// days use token quartile levels; later days without a row are "no recorded usage" (level 0).
pub fn activity_grid(days: &[(String, f64, i64)], today: NaiveDate) -> HeatGrid {
    let known: BTreeMap<NaiveDate, i64> = days
        .iter()
        .filter_map(|(d, _, t)| super::spend_view::parse_date(d).map(|d| (d, *t)))
        .collect();
    let Some(first) = known.keys().next().copied() else {
        return HeatGrid::default();
    };
    let start = today - chrono::Duration::days(364);
    let grid_start = start - chrono::Duration::days(start.weekday().num_days_from_sunday() as i64);
    let max = known
        .range(start..=today)
        .map(|(_, t)| *t)
        .max()
        .unwrap_or(0);
    let mut cells = Vec::new();
    let mut total = 0i64;
    let mut day = start;
    while day <= today {
        let offset = (day - grid_start).num_days();
        let tokens = match known.get(&day) {
            Some(t) => Some(*t),
            None if day >= first => Some(0),
            None => None,
        };
        total += tokens.unwrap_or(0);
        cells.push(HeatCell {
            col: (offset / 7) as usize,
            row: (offset % 7) as usize,
            date: day,
            tokens,
            level: heat_level(tokens, max),
        });
        day += chrono::Duration::days(1);
    }
    let weeks = cells.last().map(|c| c.col + 1).unwrap_or(0);
    let mut months: Vec<(usize, String)> = Vec::new();
    let mut last_month = None;
    for c in &cells {
        let m = (c.date.year(), c.date.month());
        if last_month != Some(m) {
            last_month = Some(m);
            let crowded = months.last().is_some_and(|(col, _)| c.col < col + 3);
            if !crowded {
                months.push((c.col, c.date.format("%b").to_string()));
            }
        }
    }
    HeatGrid {
        weeks,
        cells,
        months,
        total,
    }
}

// ---------------------------------------------------------------------------
// Canvas: stacked trend chart
// ---------------------------------------------------------------------------

fn label(content: String, at: Point, color: Color, h: alignment::Horizontal) -> Text {
    Text {
        content,
        position: at,
        color,
        size: Pixels(10.5),
        horizontal_alignment: h,
        vertical_alignment: alignment::Vertical::Center,
        ..Text::default()
    }
}

pub struct TrendChart {
    pub buckets: Vec<Bucket>,
    pub unit: Unit,
    pub hourly: bool,
    pub y_max: f64,
    pub hover: Option<usize>,
    pub selected: Option<usize>,
    /// Message sent when bucket `i` is clicked.
    pub on_click: Vec<SpendMsg>,
}

impl TrendChart {
    fn index_at(&self, p: Point, size: Size) -> Option<usize> {
        let plot = plot_rect(size);
        let frames = bar_frames(plot, self.buckets.len(), BAR_RATIO);
        // Slots, not just bar bodies, so thin bars stay easy to hit.
        let slot = plot.width / self.buckets.len().max(1) as f32;
        let hit = hit_test(p, plot, &frames);
        hit.or_else(|| {
            plot.contains(p)
                .then(|| (((p.x - plot.x) / slot) as usize).min(self.buckets.len() - 1))
        })
    }
}

fn top_rounded(frame: &mut Frame, x: f32, y: f32, w: f32, h: f32, color: Color, round: bool) {
    if round && h >= 4.0 && w >= 6.0 {
        let r = border::Radius {
            top_left: 3.0,
            top_right: 3.0,
            bottom_right: 0.0,
            bottom_left: 0.0,
        };
        frame.fill(&Path::rounded_rectangle(Point::new(x, y), Size::new(w, h), r), color);
    } else {
        frame.fill_rectangle(Point::new(x, y), Size::new(w, h.max(0.5)), color);
    }
}

impl canvas::Program<Message> for TrendChart {
    type State = Option<usize>;

    fn update(
        &self,
        state: &mut Option<usize>,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        if self.buckets.is_empty() {
            return (canvas::event::Status::Ignored, None);
        }
        let idx = cursor
            .position_in(bounds)
            .and_then(|p| self.index_at(p, bounds.size()));
        if let canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
            if let Some(msg) = idx.and_then(|i| self.on_click.get(i)) {
                return (
                    canvas::event::Status::Captured,
                    Some(Message::Spend(msg.clone())),
                );
            }
        }
        if idx != *state {
            *state = idx;
            return (
                canvas::event::Status::Ignored,
                Some(Message::ChartHover {
                    key: "spend-trend".into(),
                    index: idx,
                }),
            );
        }
        (canvas::event::Status::Ignored, None)
    }

    fn mouse_interaction(
        &self,
        _state: &Option<usize>,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        match cursor.position_in(bounds).and_then(|p| self.index_at(p, bounds.size())) {
            Some(_) => mouse::Interaction::Pointer,
            None => mouse::Interaction::default(),
        }
    }

    fn draw(
        &self,
        _state: &Option<usize>,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let size = bounds.size();
        if self.buckets.is_empty() || size.width < 80.0 || size.height < 60.0 {
            return vec![frame.into_geometry()];
        }
        let dim = Color::from_rgba(1.0, 1.0, 1.0, 0.55);
        let plot = plot_rect(size);
        let frames = bar_frames(plot, self.buckets.len(), BAR_RATIO);
        let baseline = plot.y + plot.height;
        let scale = plot.height / self.y_max as f32;
        for tick in nice_ticks(self.y_max, 4) {
            let y = baseline - tick as f32 * scale;
            frame.fill_rectangle(
                Point::new(plot.x, y),
                Size::new(plot.width, 1.0),
                Color::from_rgba(1.0, 1.0, 1.0, if tick == 0.0 { 0.25 } else { 0.10 }),
            );
            frame.fill_text(label(
                axis_label(tick),
                Point::new(crate::charts::Y_AXIS_WIDTH - 6.0, y),
                dim,
                alignment::Horizontal::Right,
            ));
        }
        let slot = plot.width / self.buckets.len() as f32;
        for (band, alpha) in [(self.hover, 0.06), (self.selected, 0.12)] {
            if let Some(i) = band.filter(|i| *i < frames.len()) {
                frame.fill_rectangle(
                    Point::new(plot.x + slot * i as f32, plot.y),
                    Size::new(slot, plot.height),
                    Color::from_rgba(1.0, 1.0, 1.0, alpha),
                );
            }
        }
        for (b, f) in self.buckets.iter().zip(&frames) {
            let mut y = baseline;
            let last = b.segs.len().saturating_sub(1);
            for (n, s) in b.segs.iter().enumerate() {
                let h = (s.value as f32 * scale).max(1.0);
                y -= h;
                top_rounded(&mut frame, f.x, y, f.width.max(1.0), h, provider_color(&s.id), n == last);
            }
        }
        // X axis: 24h ticks in hour mode, otherwise evenly spaced bucket dates.
        if self.hourly {
            for h in (0..=24).step_by(4) {
                let x = plot.x + plot.width * h as f32 / 24.0;
                let text = format!("{h:02}:00");
                let left = place_label(x, label_width(&text), size.width);
                frame.fill_text(label(
                    text,
                    Point::new(left, size.height - crate::charts::X_AXIS_HEIGHT / 2.0),
                    dim,
                    alignment::Horizontal::Left,
                ));
            }
        } else {
            let n = self.buckets.len();
            let want = ((plot.width / 84.0) as usize).clamp(2, 8).min(n);
            let mut last_right = f32::MIN;
            for k in 0..want {
                let i = if want == 1 { 0 } else { k * (n - 1) / (want - 1) };
                let text = bucket_label(&self.buckets[i], self.unit);
                let w = label_width(&text);
                let left = place_label(frames[i].x + frames[i].width / 2.0, w, size.width);
                if left < last_right + 6.0 {
                    continue;
                }
                last_right = left + w;
                frame.fill_text(label(
                    text,
                    Point::new(left, size.height - crate::charts::X_AXIS_HEIGHT / 2.0),
                    dim,
                    alignment::Horizontal::Left,
                ));
            }
        }
        vec![frame.into_geometry()]
    }
}

// ---------------------------------------------------------------------------
// Canvas: activity heatmap
// ---------------------------------------------------------------------------

const HEAT_LABEL_H: f32 = 18.0;
const HEAT_MAX_PITCH: f32 = 13.0;

pub fn heat_height() -> f32 {
    HEAT_LABEL_H + 7.0 * HEAT_MAX_PITCH + 2.0
}

pub struct HeatmapChart {
    pub grid: HeatGrid,
    pub hover: Option<usize>,
}

impl HeatmapChart {
    fn pitch(&self, width: f32) -> f32 {
        (width / self.grid.weeks.max(1) as f32).clamp(4.0, HEAT_MAX_PITCH)
    }

    fn index_at(&self, p: Point, width: f32) -> Option<usize> {
        let pitch = self.pitch(width);
        let col = (p.x / pitch).floor();
        let row = ((p.y - HEAT_LABEL_H) / pitch).floor();
        if col < 0.0 || row < 0.0 || row >= 7.0 {
            return None;
        }
        let (col, row) = (col as usize, row as usize);
        self.grid
            .cells
            .iter()
            .any(|c| c.col == col && c.row == row)
            .then_some(col * 7 + row)
    }
}

fn hatch_color(base: Color, a: f32) -> Color {
    Color { a, ..base }
}

impl canvas::Program<Message> for HeatmapChart {
    type State = Option<usize>;

    fn update(
        &self,
        state: &mut Option<usize>,
        _event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        let idx = cursor
            .position_in(bounds)
            .and_then(|p| self.index_at(p, bounds.width));
        if idx != *state {
            *state = idx;
            return (
                canvas::event::Status::Ignored,
                Some(Message::ChartHover {
                    key: "spend-heat".into(),
                    index: idx,
                }),
            );
        }
        (canvas::event::Status::Ignored, None)
    }

    fn draw(
        &self,
        _state: &Option<usize>,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let pitch = self.pitch(bounds.width);
        let cell = if pitch >= HEAT_MAX_PITCH { pitch - 3.0 } else { pitch * 0.78 };
        let corner = (cell * 0.22).min(2.5);
        let dim = Color::from_rgba(1.0, 1.0, 1.0, 0.75);
        for (col, name) in &self.grid.months {
            frame.fill_text(label(
                name.clone(),
                Point::new(*col as f32 * pitch, HEAT_LABEL_H / 2.0 - 2.0),
                dim,
                alignment::Horizontal::Left,
            ));
        }
        let unavailable_fill = Color::from_rgb8(0x28, 0x2D, 0x34);
        let unavailable_stroke = Color::from_rgb8(0x8A, 0x95, 0x9F);
        for c in &self.grid.cells {
            let x = c.col as f32 * pitch;
            let y = HEAT_LABEL_H + c.row as f32 * pitch;
            let rect = Path::rounded_rectangle(Point::new(x, y), Size::new(cell, cell), corner.into());
            match c.tokens {
                Some(_) => frame.fill(&rect, heat_color(c.level)),
                None => {
                    frame.fill(&rect, unavailable_fill);
                    frame.stroke(
                        &rect,
                        canvas::Stroke::default()
                            .with_color(hatch_color(unavailable_stroke, 0.8))
                            .with_width(0.75),
                    );
                    let inset = (cell * 0.25).min(2.0);
                    let slash = Path::line(
                        Point::new(x + inset, y + cell - inset),
                        Point::new(x + cell - inset, y + inset),
                    );
                    frame.stroke(
                        &slash,
                        canvas::Stroke::default()
                            .with_color(hatch_color(unavailable_stroke, 0.8))
                            .with_width(0.75),
                    );
                }
            }
        }
        if let Some(i) = self.hover {
            if let Some(c) = self.grid.cells.iter().find(|c| c.col * 7 + c.row == i) {
                let tokens = match c.tokens {
                    Some(t) => format!("{} tokens", super::spend_view::count(Some(t))),
                    None => "Unavailable".to_string(),
                };
                let lines = [tokens, c.date.format("%a, %b %-d, %Y").to_string()];
                let w = lines.iter().map(|l| label_width(l)).fold(0.0, f32::max) + 16.0;
                let h = 2.0 * 14.0 + 8.0;
                let x = (c.col as f32 * pitch - w / 2.0).clamp(0.0, (bounds.width - w).max(0.0));
                let y = (HEAT_LABEL_H + c.row as f32 * pitch - h - 4.0).max(0.0);
                let r = Path::rounded_rectangle(Point::new(x, y), Size::new(w, h), 5.0.into());
                frame.fill(&r, Color::from_rgba(0.08, 0.08, 0.1, 0.96));
                frame.stroke(
                    &r,
                    canvas::Stroke::default()
                        .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.25))
                        .with_width(1.0),
                );
                for (n, line) in lines.into_iter().enumerate() {
                    frame.fill_text(label(
                        line,
                        Point::new(x + 8.0, y + 11.0 + n as f32 * 14.0),
                        if n == 0 { Color::WHITE } else { dim },
                        alignment::Horizontal::Left,
                    ));
                }
            }
        }
        vec![frame.into_geometry()]
    }
}

// ---------------------------------------------------------------------------
// Small view helpers: glyphs and the panel look
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Refresh,
    Share,
    Shield,
    Warning,
    ChevronLeft,
    ChevronRight,
    ChevronDown,
    ChevronUp,
    Folder,
    Bubble,
    Calendar,
}

macro_rules! svg_path {
    ($d:expr) => {
        concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#ffffff" d=""##,
            $d,
            r##""/></svg>"##
        )
        .as_bytes()
    };
}

fn glyph_bytes(g: Glyph) -> &'static [u8] {
    match g {
        Glyph::Refresh => svg_path!("M17.65 6.35A7.96 7.96 0 0 0 12 4a8 8 0 1 0 7.75 10h-2.08A6 6 0 1 1 12 6c1.66 0 3.14.69 4.22 1.78L13 11h7V4l-2.35 2.35z"),
        Glyph::Share => svg_path!("M11 2.8 8.4 5.4 7.3 4.3 12 -.4l4.7 4.7-1.1 1.1L13 2.8V14h-2zM6 8h3v1.6H7.6v9.8h8.8V9.6H15V8h3v13H6z"),
        Glyph::Shield => svg_path!("M12 1 3 5v6c0 5.5 3.8 10.7 9 12 5.2-1.3 9-6.5 9-12V5l-9-4z"),
        Glyph::Warning => svg_path!("M1 21h22L12 2 1 21zm12-3h-2v-2h2v2zm0-4h-2v-4h2v4z"),
        Glyph::ChevronLeft => svg_path!("M15.4 7.4 14 6l-6 6 6 6 1.4-1.4L10.8 12z"),
        Glyph::ChevronRight => svg_path!("M8.6 16.6 10 18l6-6-6-6-1.4 1.4 4.6 4.6z"),
        Glyph::ChevronDown => svg_path!("M7.4 8.6 6 10l6 6 6-6-1.4-1.4-4.6 4.6z"),
        Glyph::ChevronUp => svg_path!("M7.4 15.4 6 14l6-6 6 6-1.4 1.4L12 10.8z"),
        Glyph::Folder => svg_path!("M10 4H4c-1.1 0-2 .9-2 2v12c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V8c0-1.1-.9-2-2-2h-8l-2-2z"),
        Glyph::Bubble => svg_path!("M20 2H4c-1.1 0-2 .9-2 2v18l4-4h14c1.1 0 2-.9 2-2V4c0-1.1-.9-2-2-2z"),
        Glyph::Calendar => svg_path!("M19 4h-1V2h-2v2H8V2H6v2H5c-1.1 0-2 .9-2 2v14c0 1.1.9 2 2 2h14c1.1 0 2-.9 2-2V6c0-1.1-.9-2-2-2zm0 16H5V9h14v11z"),
    }
}

/// Monochrome vector glyph tinted `color`.
pub fn glyph<'a, M: 'a>(g: Glyph, size: f32, color: Color) -> Element<'a, M> {
    svg(svg::Handle::from_memory(glyph_bytes(g)))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_, _| svg::Style { color: Some(color) })
        .into()
}

/// Mac dashboard panel: padding 16, corner 12, soft fill, hairline border.
pub fn panel<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(content)
        .padding(16)
        .width(Length::Fill)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.045))),
            border: Border {
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.09),
                width: 1.0,
                radius: 12.0.into(),
            },
            ..Default::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn pts() -> Vec<(String, NaiveDate, f64)> {
        vec![
            ("codex".into(), d("2026-11-03"), 2.0),
            ("claude".into(), d("2026-11-03"), 3.0),
            ("claude".into(), d("2026-11-05"), 1.0),
            ("codex".into(), d("2026-11-09"), 4.0),
        ]
    }

    #[test]
    fn overview_unit_thresholds() {
        assert_eq!(overview_unit(7), Unit::Day);
        assert_eq!(overview_unit(45), Unit::Day);
        assert_eq!(overview_unit(46), Unit::Week);
        assert_eq!(overview_unit(180), Unit::Week);
        assert_eq!(overview_unit(181), Unit::Month);
    }

    #[test]
    fn stacks_sorted_by_provider_with_totals() {
        let b = build_buckets(&pts(), Unit::Day, (d("2026-11-03"), d("2026-11-09")), None);
        assert_eq!(b.len(), 7);
        let first = &b[0];
        assert_eq!(
            first.segs.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["claude", "codex"]
        );
        assert_eq!(first.total, 5.0);
        assert!(!b[1].has_data());
        assert_eq!(b[1].total, 0.0);
    }

    #[test]
    fn isolate_keeps_one_provider() {
        let b = build_buckets(&pts(), Unit::Day, (d("2026-11-03"), d("2026-11-09")), Some("codex"));
        assert_eq!(b[0].segs.len(), 1);
        assert_eq!(b[0].total, 2.0);
        assert!(!b[2].has_data());
    }

    #[test]
    fn weekly_bucketing_starts_on_monday() {
        // 2026-11-03 is a Tuesday -> week of Mon 2026-11-02.
        assert_eq!(bucket_start(d("2026-11-03"), Unit::Week), d("2026-11-02"));
        let b = build_buckets(&pts(), Unit::Week, (d("2026-11-03"), d("2026-11-09")), None);
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].start, d("2026-11-02"));
        assert_eq!(b[0].end, d("2026-11-08"));
        assert_eq!(b[0].total, 6.0);
        assert_eq!(b[1].start, d("2026-11-09"));
        assert_eq!(b[1].total, 4.0);
    }

    #[test]
    fn monthly_bucketing_rolls_over_year() {
        let p = vec![
            ("claude".to_string(), d("2026-12-15"), 1.0),
            ("claude".to_string(), d("2027-01-02"), 2.0),
        ];
        let b = build_buckets(&p, Unit::Month, (d("2026-12-01"), d("2027-01-31")), None);
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].end, d("2026-12-31"));
        assert_eq!(b[1].start, d("2027-01-01"));
        assert_eq!(b[1].total, 2.0);
    }

    #[test]
    fn ignores_points_outside_range_and_non_finite() {
        let p = vec![
            ("claude".to_string(), d("2026-10-01"), 9.0),
            ("claude".to_string(), d("2026-11-03"), f64::NAN),
            ("claude".to_string(), d("2026-11-04"), 1.0),
        ];
        let b = build_buckets(&p, Unit::Day, (d("2026-11-03"), d("2026-11-04")), None);
        assert_eq!(b.len(), 2);
        assert!(!b[0].has_data());
        assert_eq!(b[1].total, 1.0);
    }

    #[test]
    fn highest_bucket_is_first_max() {
        let b = build_buckets(&pts(), Unit::Day, (d("2026-11-03"), d("2026-11-09")), None);
        assert_eq!(peak_bucket(&b), Some(0));
        let b = build_buckets(&pts(), Unit::Week, (d("2026-11-03"), d("2026-11-09")), None);
        assert_eq!(peak_bucket(&b), Some(0));
        assert_eq!(peak_bucket(&[]), None);
        let empty = build_buckets(&[], Unit::Day, (d("2026-11-03"), d("2026-11-04")), None);
        assert_eq!(peak_bucket(&empty), None);
    }

    #[test]
    fn y_domain_adds_headroom_with_floor() {
        let b = build_buckets(&pts(), Unit::Day, (d("2026-11-03"), d("2026-11-09")), None);
        assert!((y_domain(&b) - 5.0 * 1.15).abs() < 1e-9);
        assert_eq!(y_domain(&[]), 0.01);
    }

    #[test]
    fn nice_ticks_match_automatic_four() {
        assert_eq!(nice_ticks(807.0, 4), vec![0.0, 250.0, 500.0, 750.0]);
        assert_eq!(nice_ticks(12.7, 4), vec![0.0, 5.0, 10.0]);
        assert_eq!(nice_ticks(0.0, 4), Vec::<f64>::new());
        assert_eq!(nice_ticks(f64::NAN, 4), Vec::<f64>::new());
        let small = nice_ticks(0.01, 4);
        assert_eq!(small.first(), Some(&0.0));
        assert!(small.len() >= 2);
    }

    #[test]
    fn axis_labels_are_compact() {
        assert_eq!(axis_label(0.0), "$0");
        assert_eq!(axis_label(250.0), "$250");
        assert_eq!(axis_label(2.5), "$2.50");
        assert_eq!(axis_label(0.25), "$0.25");
        assert_eq!(axis_label(1500.0), "$1.5K");
        assert_eq!(axis_label(2000.0), "$2K");
        assert_eq!(axis_label(5.0), "$5");
    }

    #[test]
    fn bucket_labels() {
        let b = build_buckets(&pts(), Unit::Week, (d("2026-11-03"), d("2026-11-09")), None);
        assert_eq!(bucket_range(&b[0], Unit::Week), "Nov 2 – Nov 8");
        assert_eq!(bucket_label(&b[0], Unit::Week), "Nov 2");
        let h = Bucket {
            start: d("2026-11-03"),
            end: d("2026-11-03"),
            hour: Some(7),
            segs: vec![],
            total: 0.0,
        };
        assert_eq!(bucket_label(&h, Unit::Day), "07:00");
    }

    #[test]
    fn hour_buckets_stack_per_provider() {
        let mut idx = HourlyIndex::default();
        let mut hours = vec![BTreeMap::new(); 24];
        hours[9].insert("codex".to_string(), 1.5);
        hours[9].insert("claude".to_string(), 0.5);
        idx.days.insert(d("2026-11-03"), hours);
        let b = hour_buckets(&idx, d("2026-11-03"), None).unwrap();
        assert_eq!(b.len(), 24);
        assert_eq!(b[9].total, 2.0);
        assert_eq!(b[9].segs[0].id, "claude");
        assert!(!b[8].has_data());
        assert!(hour_buckets(&idx, d("2026-11-04"), None).is_none());
    }

    #[test]
    fn activity_grid_is_sunday_first_53_weeks() {
        // 2026-11-09 is a Monday.
        let today = d("2026-11-09");
        let days = vec![
            ("2026-11-01".to_string(), 1.0, 100),
            ("2026-11-09".to_string(), 1.0, 400),
        ];
        let g = activity_grid(&days, today);
        assert!(g.weeks >= 52 && g.weeks <= 54);
        let sun = g.cells.iter().find(|c| c.date == d("2026-11-01")).unwrap();
        assert_eq!(sun.row, 0);
        let mon = g.cells.iter().find(|c| c.date == d("2026-11-09")).unwrap();
        assert_eq!(mon.row, 1);
        assert_eq!(mon.col, g.weeks - 1);
        assert_eq!(g.cells.len(), 365);
        assert_eq!(g.total, 500);
        // Before the first recorded day: unavailable. After it: recorded or level 0.
        let before = g.cells.iter().find(|c| c.date == d("2026-10-31")).unwrap();
        assert_eq!(before.tokens, None);
        assert_eq!(before.level, None);
        let gap = g.cells.iter().find(|c| c.date == d("2026-11-05")).unwrap();
        assert_eq!(gap.tokens, Some(0));
        assert_eq!(gap.level, Some(0));
        assert_eq!(mon.level, Some(4));
        assert_eq!(sun.level, Some(1));
        assert!(!g.months.is_empty());
        assert_eq!(activity_grid(&[], today), HeatGrid::default());
    }

    #[test]
    fn provider_color_lifts_near_black() {
        let c = provider_color("abliteration");
        assert!(c.r > 0.3);
        assert_eq!(parse_hex("#CC7C5E"), Some(Color::from_rgb8(0xCC, 0x7C, 0x5E)));
        assert_eq!(parse_hex("nope"), None);
    }
}
