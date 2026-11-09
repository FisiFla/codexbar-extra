//! Bar charts for the Quick View chart sections and the spend trend.
//!
//! Mirrors the macOS menu charts (`CostHistoryChartMenuView`, `UsageBreakdownChartMenuView`):
//! calendar-day bars with 0.7 width ratio (`ChartBarHoverSelection`), date labels centred on
//! their bar at the first/last day (`ChartAxisLabelLayout`), y ticks at 0 / mid / max, a yellow
//! cap on the peak day and a 10 % selection band. Data shaping, hit-testing and label layout are
//! pure functions (unit-tested); `BarChart` is the thin Canvas program on top of them.
//! Unknown stays unknown: days with no data are gray ticks, never zero bars.

use crate::cli::CostDay;
use crate::state::Message;
use chrono::NaiveDate;
use iced::widget::canvas::{self, Frame, Geometry, Path, Text};
use iced::{alignment, mouse, Color, Pixels, Point, Rectangle, Renderer, Size, Theme};

/// `ChartBarHoverSelection.barWidthRatio`: shared by drawing and hit-testing.
pub const BAR_WIDTH_RATIO: f32 = 0.7;
pub const Y_AXIS_WIDTH: f32 = 42.0;
pub const X_AXIS_HEIGHT: f32 = 18.0;
pub const TOP_PAD: f32 = 8.0;
const LABEL_CHAR_WIDTH: f32 = 5.6;
const UNKNOWN_GRAY: Color = Color::from_rgb(0.45, 0.45, 0.45);
const PEAK_YELLOW: Color = Color::from_rgb(0.96, 0.8, 0.22);

/// Fixed categorical palette for model segments; the last slot is "Other".
pub const SEGMENT_PALETTE: [(u8, u8, u8); 7] = [
    (0x5b, 0x8d, 0xef),
    (0xe5, 0xa0, 0x4b),
    (0x5c, 0xb8, 0x8a),
    (0xc5, 0x6b, 0xd1),
    (0xe0, 0x6c, 0x75),
    (0x56, 0xb6, 0xc2),
    (0x8a, 0x8f, 0x98),
];

pub fn segment_color(index: usize) -> Color {
    let (r, g, b) = SEGMENT_PALETTE[index.min(SEGMENT_PALETTE.len() - 1)];
    Color::from_rgb8(r, g, b)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub name: String,
    pub color_index: usize,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChartBar {
    /// `YYYY-MM-DD`.
    pub date: String,
    /// `None` = unknown day (no recorded data).
    pub value: Option<f64>,
    /// Stacked parts, largest first; empty for plain bars.
    pub segments: Vec<Segment>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelTotal {
    pub name: String,
    pub color_index: usize,
    pub cost: f64,
}

fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

/// `Oct 16` (mac `.dateTime.month(.abbreviated).day()`).
pub fn short_date(date: &str) -> String {
    parse_date(date)
        .map(|d| d.format("%b %-d").to_string())
        .unwrap_or_else(|| date.to_string())
}

/// One bar per calendar day from the first to the last row, ascending. Gap days are unknown.
/// Rows with an unparseable date are ignored; non-finite costs are unknown.
pub fn calendar_bars<T>(rows: &[(String, T)], value: impl Fn(&T) -> Option<f64>) -> Vec<ChartBar> {
    let mut known: Vec<(NaiveDate, Option<f64>)> = rows
        .iter()
        .filter_map(|(d, t)| parse_date(d).map(|d| (d, value(t).filter(|v| v.is_finite()))))
        .collect();
    known.sort_by_key(|(d, _)| *d);
    known.dedup_by_key(|(d, _)| *d);
    let (Some(first), Some(last)) = (known.first().map(|k| k.0), known.last().map(|k| k.0)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut day = first;
    let mut i = 0;
    while day <= last {
        let v = if known.get(i).map(|k| k.0) == Some(day) {
            i += 1;
            known[i - 1].1
        } else {
            None
        };
        out.push(ChartBar {
            date: day.format("%Y-%m-%d").to_string(),
            value: v,
            segments: Vec::new(),
        });
        day += chrono::Duration::days(1);
    }
    out
}

/// Cost-history bars (mac `CostHistoryChartMenuView`, cost metric).
pub fn cost_bars(days: &[CostDay]) -> Vec<ChartBar> {
    let rows: Vec<(String, f64)> = days
        .iter()
        .map(|d| (d.date.clone(), d.total_cost))
        .collect();
    calendar_bars(&rows, |c| Some(*c))
}

/// Model-stacked bars. The top `max_models` models by total cost get palette slots 0.., the
/// rest fold into one "Other" segment (last palette slot). A day without any model breakdown
/// is unknown rather than zero. Returns no bars when no day has a breakdown.
pub fn model_bars(days: &[CostDay], max_models: usize) -> (Vec<ChartBar>, Vec<ModelTotal>) {
    let max_models = max_models.min(SEGMENT_PALETTE.len() - 1);
    let mut totals: Vec<(String, f64)> = Vec::new();
    for m in days.iter().flat_map(|d| d.model_breakdowns.iter()) {
        if !m.cost.is_finite() {
            continue;
        }
        match totals.iter_mut().find(|t| t.0 == m.model_name) {
            Some(t) => t.1 += m.cost,
            None => totals.push((m.model_name.clone(), m.cost)),
        }
    }
    if totals.is_empty() {
        return (Vec::new(), Vec::new());
    }
    totals.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let other_idx = SEGMENT_PALETTE.len() - 1;
    let slot = |name: &str| {
        totals
            .iter()
            .position(|t| t.0 == name)
            .filter(|p| *p < max_models)
            .unwrap_or(other_idx)
    };
    let rows: Vec<(String, Vec<Segment>)> = days
        .iter()
        .map(|d| {
            let mut segs: Vec<Segment> = Vec::new();
            for m in d.model_breakdowns.iter().filter(|m| m.cost.is_finite()) {
                let ci = slot(&m.model_name);
                let name = if ci == other_idx {
                    "Other".to_string()
                } else {
                    m.model_name.clone()
                };
                match segs
                    .iter_mut()
                    .find(|s| s.color_index == ci && ci == other_idx)
                {
                    Some(s) => s.value += m.cost,
                    None => segs.push(Segment {
                        name,
                        color_index: ci,
                        value: m.cost,
                    }),
                }
            }
            segs.sort_by(|a, b| {
                b.value
                    .total_cmp(&a.value)
                    .then(a.color_index.cmp(&b.color_index))
            });
            (d.date.clone(), segs)
        })
        .collect();
    let mut bars = calendar_bars(&rows, |s| {
        (!s.is_empty()).then(|| s.iter().map(|x| x.value).sum())
    });
    for bar in &mut bars {
        if let Some((_, segs)) = rows.iter().find(|(d, _)| *d == bar.date) {
            if bar.value.is_some() {
                bar.segments = segs.clone();
            }
        }
    }
    let mut legend: Vec<ModelTotal> = totals
        .iter()
        .take(max_models)
        .enumerate()
        .map(|(i, t)| ModelTotal {
            name: t.0.clone(),
            color_index: i,
            cost: t.1,
        })
        .collect();
    let other: f64 = totals.iter().skip(max_models).map(|t| t.1).sum();
    if totals.len() > max_models {
        legend.push(ModelTotal {
            name: "Other".into(),
            color_index: other_idx,
            cost: other,
        });
    }
    (bars, legend)
}

/// Index of the tallest bar (first on ties); `None` when nothing is positive.
pub fn peak_index(bars: &[ChartBar]) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, b) in bars.iter().enumerate() {
        if let Some(v) = b.value.filter(|v| *v > 0.0) {
            if best.is_none_or(|(_, m)| v > m) {
                best = Some((i, v));
            }
        }
    }
    best.map(|b| b.0)
}

pub fn max_value(bars: &[ChartBar]) -> f64 {
    bars.iter()
        .filter_map(|b| b.value)
        .fold(0.0f64, f64::max)
        .max(0.0)
}

/// Mac `yAxisTickValues`: 0 / mid / max from $1 up, 0 / max below, none when flat.
pub fn y_ticks(max: f64) -> Vec<f64> {
    if max.is_nan() || max <= 0.0 || !max.is_finite() {
        Vec::new()
    } else if max < 1.0 {
        vec![0.0, max]
    } else {
        vec![0.0, max / 2.0, max]
    }
}

pub fn axis_money(v: f64) -> String {
    if v >= 1000.0 {
        format!("${:.1}k", v / 1000.0)
    } else if v >= 10.0 {
        format!("${v:.0}")
    } else {
        format!("${v:.2}")
    }
}

pub fn money(v: f64) -> String {
    format!("${v:.2}")
}

// ---------------------------------------------------------------------------
// Geometry: plot frame, bar frames, hit-testing, axis labels
// ---------------------------------------------------------------------------

/// Plot area inside a canvas of `size` (room for y labels left, date labels below).
pub fn plot_rect(size: Size) -> Rectangle {
    Rectangle {
        x: Y_AXIS_WIDTH,
        y: TOP_PAD,
        width: (size.width - Y_AXIS_WIDTH - 4.0).max(0.0),
        height: (size.height - TOP_PAD - X_AXIS_HEIGHT).max(0.0),
    }
}

/// `ChartBarHoverSelection.bars`: `n` equal day slots, each bar `ratio` of its slot, centred.
pub fn bar_frames(plot: Rectangle, n: usize, ratio: f32) -> Vec<Rectangle> {
    if n == 0 || plot.width < 0.0 || plot.height < 0.0 || ratio < 0.0 {
        return Vec::new();
    }
    let slot = plot.width / n as f32;
    (0..n)
        .map(|i| {
            let width = slot * ratio;
            let center = plot.x + slot * (i as f32 + 0.5);
            Rectangle {
                x: center - width / 2.0,
                y: plot.y,
                width,
                height: plot.height,
            }
        })
        .collect()
}

/// `ChartBarHoverSelection.selection`: the point must be inside the plot; a lone bar is always
/// selected, otherwise only the bar frame under the point (the gaps select nothing).
pub fn hit_test(point: Point, plot: Rectangle, frames: &[Rectangle]) -> Option<usize> {
    if !plot.contains(point) {
        return None;
    }
    if frames.len() == 1 {
        return Some(0);
    }
    frames.iter().position(|f| f.contains(point))
}

#[derive(Debug, Clone, PartialEq)]
pub struct AxisLabel {
    pub index: usize,
    pub text: String,
    /// Left edge of the text, clamped so the whole label stays inside `0..total_width`.
    pub x: f32,
    pub width: f32,
}

pub fn label_width(text: &str) -> f32 {
    text.chars().count() as f32 * LABEL_CHAR_WIDTH
}

/// Left edge for a label centred on `center` but kept fully inside `0..total_width`
/// (`ChartAxisLabelLayout`: full dates at both ends without shifting away from their bars).
pub fn place_label(center: f32, width: f32, total_width: f32) -> f32 {
    let left = center - width / 2.0;
    left.min(total_width - width).max(0.0)
}

/// Date labels for the first and last bar (one label when they are the same day).
pub fn axis_labels(bars: &[ChartBar], frames: &[Rectangle], total_width: f32) -> Vec<AxisLabel> {
    let mut idx = Vec::new();
    if !bars.is_empty() && bars.len() == frames.len() {
        idx.push(0);
        if bars.len() > 1 {
            idx.push(bars.len() - 1);
        }
    }
    let mut labels: Vec<AxisLabel> = idx
        .into_iter()
        .map(|i| {
            let text = short_date(&bars[i].date);
            let width = label_width(&text);
            let center = frames[i].x + frames[i].width / 2.0;
            AxisLabel {
                index: i,
                x: place_label(center, width, total_width),
                text,
                width,
            }
        })
        .collect();
    // Two short ranges can collide; drop the last label rather than overlap.
    if labels.len() == 2 && labels[0].x + labels[0].width + 4.0 > labels[1].x {
        labels.pop();
    }
    labels
}

/// Tooltip lines for a bar: date, then either segment rows (max 4) or the value.
pub fn tooltip_lines(bar: &ChartBar, fmt: fn(f64) -> String) -> Vec<String> {
    let mut lines = vec![short_date(&bar.date)];
    match bar.value {
        None => lines.push("No data".to_string()),
        Some(v) if bar.segments.is_empty() => lines.push(fmt(v)),
        Some(v) => {
            for s in bar.segments.iter().take(4) {
                lines.push(format!("{}  {}", s.name, fmt(s.value)));
            }
            if bar.segments.len() > 4 {
                lines.push(format!("+{} more", bar.segments.len() - 4));
            }
            lines.push(format!("Total  {}", fmt(v)));
        }
    }
    lines
}

/// Tooltip box next to the hovered bar: on the side with more room, clamped into the canvas.
pub fn tooltip_rect(bar: Rectangle, size: Size, width: f32, height: f32) -> Rectangle {
    let center = bar.x + bar.width / 2.0;
    let x = if center < size.width / 2.0 {
        bar.x + bar.width + 6.0
    } else {
        bar.x - 6.0 - width
    };
    Rectangle {
        x: x.min(size.width - width).max(0.0),
        y: TOP_PAD.min((size.height - height).max(0.0)),
        width,
        height,
    }
}

// ---------------------------------------------------------------------------
// Canvas program
// ---------------------------------------------------------------------------

pub struct BarChart {
    pub key: String,
    pub bars: Vec<ChartBar>,
    /// Plain-bar colour (segmented bars use the palette).
    pub color: Color,
    pub peak_cap: bool,
    pub hover: Option<usize>,
    pub fmt: fn(f64) -> String,
    /// Y-axis tick label formatter.
    pub axis: fn(f64) -> String,
    /// Fixed y maximum (e.g. 100 for percent); `None` scales to the data.
    pub y_max: Option<f64>,
}

impl BarChart {
    fn index_at(&self, point: Point, size: Size) -> Option<usize> {
        let plot = plot_rect(size);
        hit_test(
            point,
            plot,
            &bar_frames(plot, self.bars.len(), BAR_WIDTH_RATIO),
        )
    }
}

fn text(content: String, at: Point, color: Color, h: alignment::Horizontal) -> Text {
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

impl canvas::Program<Message> for BarChart {
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
            .and_then(|p| self.index_at(p, bounds.size()));
        if idx != *state {
            *state = idx;
            return (
                canvas::event::Status::Ignored,
                Some(Message::ChartHover {
                    key: self.key.clone(),
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
        let size = bounds.size();
        if self.bars.is_empty() || size.width < 60.0 || size.height < 40.0 {
            return vec![frame.into_geometry()];
        }
        let dim = Color::from_rgba(1.0, 1.0, 1.0, 0.55);
        // Axis labels are caption2 tertiary on macOS.
        let axis_dim = Color::from_rgba(1.0, 1.0, 1.0, 0.40);
        let plot = plot_rect(size);
        let frames = bar_frames(plot, self.bars.len(), BAR_WIDTH_RATIO);
        let max = self.y_max.unwrap_or_else(|| max_value(&self.bars));
        let baseline = plot.y + plot.height;
        frame.fill_rectangle(
            Point::new(plot.x, baseline),
            Size::new(plot.width, 1.0),
            Color::from_rgba(1.0, 1.0, 1.0, 0.25),
        );
        for tick in y_ticks(max) {
            let y = baseline - (tick / max) as f32 * plot.height;
            frame.fill_text(text(
                (self.axis)(tick),
                Point::new(Y_AXIS_WIDTH - 6.0, y),
                axis_dim,
                alignment::Horizontal::Right,
            ));
        }
        if let Some(i) = self.hover.filter(|i| *i < frames.len()) {
            let f = frames[i];
            frame.fill_rectangle(
                Point::new(f.x, plot.y),
                Size::new(f.width, plot.height),
                Color::from_rgba(1.0, 1.0, 1.0, 0.1),
            );
        }
        let peak = if self.peak_cap {
            peak_index(&self.bars)
        } else {
            None
        };
        for (i, (bar, f)) in self.bars.iter().zip(&frames).enumerate() {
            let Some(value) = bar.value else {
                frame.fill_rectangle(
                    Point::new(f.x, baseline - 3.0),
                    Size::new(f.width.max(1.0), 3.0),
                    UNKNOWN_GRAY,
                );
                continue;
            };
            let scale = if max > 0.0 {
                plot.height / max as f32
            } else {
                0.0
            };
            if bar.segments.is_empty() {
                let h = (value as f32 * scale).max(1.0);
                frame.fill_rectangle(
                    Point::new(f.x, baseline - h),
                    Size::new(f.width.max(1.0), h),
                    self.color,
                );
                if peak == Some(i) {
                    let cap = (max as f32 * 0.05 * scale).clamp(1.0, h);
                    frame.fill_rectangle(
                        Point::new(f.x, baseline - h),
                        Size::new(f.width.max(1.0), cap),
                        PEAK_YELLOW,
                    );
                }
            } else {
                let mut y = baseline;
                for s in &bar.segments {
                    let h = (s.value as f32 * scale).max(0.0);
                    y -= h;
                    frame.fill_rectangle(
                        Point::new(f.x, y),
                        Size::new(f.width.max(1.0), h),
                        segment_color(s.color_index),
                    );
                }
            }
        }
        for l in axis_labels(&self.bars, &frames, size.width) {
            frame.fill_text(text(
                l.text,
                Point::new(l.x, size.height - X_AXIS_HEIGHT / 2.0),
                axis_dim,
                alignment::Horizontal::Left,
            ));
        }
        if let Some(i) = self.hover.filter(|i| *i < self.bars.len()) {
            let lines = tooltip_lines(&self.bars[i], self.fmt);
            let w = lines.iter().map(|l| label_width(l)).fold(0.0, f32::max) + 16.0;
            let h = lines.len() as f32 * 14.0 + 8.0;
            let r = tooltip_rect(frames[i], size, w, h);
            frame.fill(
                &Path::rounded_rectangle(r.position(), r.size(), 8.0.into()),
                Color::from_rgba(0.08, 0.08, 0.1, 0.96),
            );
            frame.stroke(
                &Path::rounded_rectangle(r.position(), r.size(), 8.0.into()),
                canvas::Stroke::default()
                    .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.12))
                    .with_width(1.0),
            );
            for (n, line) in lines.into_iter().enumerate() {
                let color = if n == 0 { Color::WHITE } else { dim };
                frame.fill_text(text(
                    line,
                    Point::new(r.x + 8.0, r.y + 4.0 + 7.0 + n as f32 * 14.0),
                    color,
                    alignment::Horizontal::Left,
                ));
            }
        }
        vec![frame.into_geometry()]
    }
}

// ---------------------------------------------------------------------------
// Quota burndown line chart
// ---------------------------------------------------------------------------

/// Plot position of a burndown point: x over the window, y as remaining % (0 at the bottom).
pub fn burndown_point(
    plot: Rectangle,
    b: &crate::history::Burndown,
    at: i64,
    remaining: f64,
) -> Point {
    let span = (b.reset - b.start).max(1) as f32;
    let fx = ((at - b.start) as f32 / span).clamp(0.0, 1.0);
    let fy = (remaining as f32 / 100.0).clamp(0.0, 1.0);
    Point::new(plot.x + fx * plot.width, plot.y + (1.0 - fy) * plot.height)
}

/// Canvas program for `QuotaBurndownChartMenuView`: remaining % (solid, with points) against the
/// ideal linear burn (dashed grey) from window start to reset.
pub struct BurndownChart {
    pub burn: crate::history::Burndown,
    pub color: Color,
}

impl canvas::Program<Message> for BurndownChart {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let size = bounds.size();
        if size.width < 60.0 || size.height < 40.0 {
            return vec![frame.into_geometry()];
        }
        let plot = plot_rect(size);
        let baseline = plot.y + plot.height;
        for tick in [0.0, 50.0, 100.0] {
            let y = baseline - (tick / 100.0) as f32 * plot.height;
            frame.fill_rectangle(
                Point::new(plot.x, y),
                Size::new(plot.width, 1.0),
                Color::from_rgba(1.0, 1.0, 1.0, 0.12),
            );
            frame.fill_text(text(
                format!("{tick:.0}%"),
                Point::new(Y_AXIS_WIDTH - 6.0, y),
                Color::from_rgba(1.0, 1.0, 1.0, 0.40),
                alignment::Horizontal::Right,
            ));
        }
        let b = &self.burn;
        let a = burndown_point(plot, b, b.start, b.ideal_at(b.start));
        let z = burndown_point(plot, b, b.reset, b.ideal_at(b.reset));
        let segments = 24;
        for i in (0..segments).step_by(2) {
            let t0 = i as f32 / segments as f32;
            let t1 = (i + 1) as f32 / segments as f32;
            let p0 = Point::new(a.x + (z.x - a.x) * t0, a.y + (z.y - a.y) * t0);
            let p1 = Point::new(a.x + (z.x - a.x) * t1, a.y + (z.y - a.y) * t1);
            frame.stroke(
                &Path::line(p0, p1),
                canvas::Stroke::default()
                    .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.45))
                    .with_width(1.0),
            );
        }
        let pts: Vec<Point> = b
            .samples
            .iter()
            .map(|s| burndown_point(plot, b, s.at, s.remaining))
            .collect();
        if pts.len() > 1 {
            let path = Path::new(|p| {
                p.move_to(pts[0]);
                for q in &pts[1..] {
                    p.line_to(*q);
                }
            });
            frame.stroke(
                &path,
                canvas::Stroke::default()
                    .with_color(self.color)
                    .with_width(2.0),
            );
        }
        if let Some(last) = pts.last() {
            frame.fill(&Path::circle(*last, 3.5), self.color);
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::CostModelBreakdown;

    fn day(date: &str, cost: f64, models: &[(&str, f64)]) -> CostDay {
        CostDay {
            date: date.into(),
            total_tokens: 10,
            total_cost: cost,
            model_breakdowns: models
                .iter()
                .map(|(n, c)| CostModelBreakdown {
                    model_name: (*n).into(),
                    cost: *c,
                    total_tokens: 1,
                })
                .collect(),
        }
    }

    #[test]
    fn calendar_bars_fill_gaps_as_unknown_and_sort() {
        let bars = cost_bars(&[
            day("2026-10-18", 3.0, &[]),
            day("2026-10-16", 1.0, &[]),
            day("2026-10-16", 9.0, &[]),
            day("bad", 5.0, &[]),
        ]);
        let got: Vec<_> = bars.iter().map(|b| (b.date.as_str(), b.value)).collect();
        assert_eq!(
            got,
            [
                ("2026-10-16", Some(1.0)),
                ("2026-10-17", None),
                ("2026-10-18", Some(3.0))
            ]
        );
        assert!(cost_bars(&[]).is_empty());
    }

    #[test]
    fn non_finite_cost_is_unknown_not_zero() {
        let bars = cost_bars(&[day("2026-10-16", f64::NAN, &[])]);
        assert_eq!(bars[0].value, None);
    }

    #[test]
    fn model_bars_rank_fold_other_and_unknown_days() {
        let days = vec![
            day("2026-10-16", 10.0, &[("a", 6.0), ("b", 3.0), ("c", 1.0)]),
            day("2026-10-17", 2.0, &[]),
            day("2026-10-18", 5.0, &[("b", 4.0), ("c", 1.0)]),
        ];
        let (bars, legend) = model_bars(&days, 2);
        assert_eq!(bars.len(), 3);
        assert_eq!(bars[1].value, None, "day without breakdown is unknown");
        let names: Vec<_> = legend.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["b", "a", "Other"]);
        assert_eq!(legend[0].cost, 7.0);
        assert_eq!(legend[2].color_index, SEGMENT_PALETTE.len() - 1);
        let d0: Vec<_> = bars[0]
            .segments
            .iter()
            .map(|s| (s.name.as_str(), s.value))
            .collect();
        assert_eq!(d0, [("a", 6.0), ("b", 3.0), ("Other", 1.0)]);
        assert_eq!(bars[0].value, Some(10.0));
        assert!(model_bars(&[day("2026-10-16", 1.0, &[])], 3).0.is_empty());
    }

    #[test]
    fn peak_and_ticks_follow_mac_rules() {
        let bars = cost_bars(&[
            day("2026-10-16", 1.0, &[]),
            day("2026-10-17", 4.0, &[]),
            day("2026-10-18", 4.0, &[]),
        ]);
        assert_eq!(peak_index(&bars), Some(1));
        assert_eq!(max_value(&bars), 4.0);
        assert_eq!(y_ticks(4.0), [0.0, 2.0, 4.0]);
        assert_eq!(y_ticks(0.5), [0.0, 0.5]);
        assert!(y_ticks(0.0).is_empty());
        assert_eq!(peak_index(&cost_bars(&[day("2026-10-16", 0.0, &[])])), None);
        assert_eq!(axis_money(1234.0), "$1.2k");
        assert_eq!(axis_money(42.4), "$42");
        assert_eq!(axis_money(3.0), "$3.00");
    }

    #[test]
    fn bar_frames_use_slot_ratio_and_center() {
        let plot = Rectangle {
            x: 10.0,
            y: 5.0,
            width: 100.0,
            height: 50.0,
        };
        let f = bar_frames(plot, 4, BAR_WIDTH_RATIO);
        assert_eq!(f.len(), 4);
        assert!((f[0].width - 17.5).abs() < 1e-4);
        assert!((f[0].x + f[0].width / 2.0 - 22.5).abs() < 1e-4);
        assert!((f[3].x + f[3].width / 2.0 - 97.5).abs() < 1e-4);
        assert_eq!(f[2].height, 50.0);
        assert!(bar_frames(plot, 0, 0.7).is_empty());
    }

    #[test]
    fn hit_test_selects_bar_not_gap_and_ignores_outside() {
        let plot = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
        };
        let f = bar_frames(plot, 4, BAR_WIDTH_RATIO); // slot 25, bar 17.5 centred at 12.5, 37.5..
        assert_eq!(hit_test(Point::new(12.5, 10.0), plot, &f), Some(0));
        assert_eq!(hit_test(Point::new(37.5, 49.0), plot, &f), Some(1));
        assert_eq!(hit_test(Point::new(25.0, 10.0), plot, &f), None, "gap");
        assert_eq!(hit_test(Point::new(12.5, 60.0), plot, &f), None, "below");
        assert_eq!(hit_test(Point::new(-1.0, 10.0), plot, &f), None, "left");
        let one = bar_frames(plot, 1, BAR_WIDTH_RATIO);
        assert_eq!(hit_test(Point::new(1.0, 1.0), plot, &one), Some(0));
        assert_eq!(hit_test(Point::new(1.0, 1.0), plot, &[]), None);
    }

    #[test]
    fn labels_stay_inside_and_center_on_bars() {
        assert_eq!(place_label(50.0, 20.0, 200.0), 40.0);
        assert_eq!(place_label(3.0, 30.0, 200.0), 0.0);
        assert_eq!(place_label(198.0, 30.0, 200.0), 170.0);
        let bars = cost_bars(&[day("2026-10-01", 1.0, &[]), day("2026-10-30", 1.0, &[])]);
        let plot = plot_rect(Size::new(400.0, 140.0));
        let frames = bar_frames(plot, bars.len(), BAR_WIDTH_RATIO);
        let labels = axis_labels(&bars, &frames, 400.0);
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[0].text, "Oct 1");
        assert_eq!(labels[1].text, "Oct 30");
        assert!(labels[0].x >= 0.0);
        assert!(labels[1].x + labels[1].width <= 400.0 + 0.01);
        let single = cost_bars(&[day("2026-10-01", 1.0, &[])]);
        let f1 = bar_frames(plot, 1, BAR_WIDTH_RATIO);
        assert_eq!(axis_labels(&single, &f1, 400.0).len(), 1);
        assert!(axis_labels(&[], &[], 400.0).is_empty());
        // Two adjacent days in a narrow chart would overlap: keep only the first.
        let two = cost_bars(&[day("2026-10-01", 1.0, &[]), day("2026-10-02", 1.0, &[])]);
        let p2 = plot_rect(Size::new(100.0, 100.0));
        let f2 = bar_frames(p2, 2, BAR_WIDTH_RATIO);
        assert_eq!(axis_labels(&two, &f2, 100.0).len(), 1);
    }

    #[test]
    fn tooltip_text_and_placement() {
        let bars = cost_bars(&[day("2026-10-16", 79.1, &[]), day("2026-10-18", 1.0, &[])]);
        assert_eq!(tooltip_lines(&bars[0], money), ["Oct 16", "$79.10"]);
        assert_eq!(tooltip_lines(&bars[1], money), ["Oct 17", "No data"]);
        assert_eq!(tooltip_lines(&bars[2], money), ["Oct 18", "$1.00"]);
        let (m, _) = model_bars(&[day("2026-10-16", 6.0, &[("a", 5.0), ("b", 1.0)])], 3);
        assert_eq!(
            tooltip_lines(&m[0], money),
            ["Oct 16", "a  $5.00", "b  $1.00", "Total  $6.00"]
        );
        let size = Size::new(300.0, 140.0);
        let left_bar = Rectangle {
            x: 50.0,
            y: 8.0,
            width: 10.0,
            height: 100.0,
        };
        let r = tooltip_rect(left_bar, size, 80.0, 40.0);
        assert!(r.x > left_bar.x, "bar in left half -> tooltip to its right");
        let right_bar = Rectangle {
            x: 280.0,
            ..left_bar
        };
        let r = tooltip_rect(right_bar, size, 80.0, 40.0);
        assert!(r.x + r.width <= 300.0 && r.x < right_bar.x);
    }

    #[test]
    fn burndown_points_map_window_to_plot() {
        let b = crate::history::Burndown {
            start: 1000,
            reset: 2000,
            samples: vec![],
        };
        let plot = Rectangle {
            x: 10.0,
            y: 8.0,
            width: 100.0,
            height: 80.0,
        };
        assert_eq!(burndown_point(plot, &b, 1000, 100.0), Point::new(10.0, 8.0));
        assert_eq!(burndown_point(plot, &b, 2000, 0.0), Point::new(110.0, 88.0));
        assert_eq!(burndown_point(plot, &b, 1500, 50.0), Point::new(60.0, 48.0));
        // Out-of-window values clamp rather than escape the plot.
        assert_eq!(
            burndown_point(plot, &b, 5000, 150.0),
            Point::new(110.0, 8.0)
        );
    }
}
