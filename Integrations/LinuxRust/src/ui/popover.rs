//! Quick View popover (mac `MenuCardView` order / Qt `qml/QuickView.qml` layout):
//! provider switcher, Overview or provider card body, action rows, footer.

use crate::model::{CostEntry, Entry, QuotaWindow};
use crate::settings::Settings;
use crate::state::{App, Message, Page, SettingsPane};
use crate::ui::popover_charts::{self, glyph, Glyph};
use iced::widget::{
    button, column, container, horizontal_rule, progress_bar, row, scrollable, text, Column, Row,
    Space,
};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Theme};

const EM_DASH: &str = "—";
const ERROR_RGB: (u8, u8, u8) = (0xcf, 0x74, 0x43);
const DEFAULT_METER_RGB: (u8, u8, u8) = (0xc9, 0x87, 0x57);

fn rgb(c: (u8, u8, u8)) -> Color {
    Color::from_rgb8(c.0, c.1, c.2)
}

/// Secondary text: the single shared token (see `style::SECONDARY`).
fn muted() -> Color {
    crate::ui::style::SECONDARY
}

fn semibold() -> Font {
    crate::ui::style::font_semibold()
}

fn medium() -> Font {
    crate::ui::style::font_medium()
}

pub(crate) fn quota_display_value(remaining: i64, quota_display: &str) -> i64 {
    let remaining = remaining.clamp(0, 100);
    if quota_display == "used" {
        100 - remaining
    } else {
        remaining
    }
}

pub(crate) fn quota_display_suffix(quota_display: &str) -> &'static str {
    if quota_display == "used" {
        "used"
    } else {
        "left"
    }
}

pub(crate) fn is_warning(warning_colors: bool, remaining: i64, notify_threshold: u32) -> bool {
    warning_colors && remaining <= i64::from(notify_threshold)
}

pub(crate) fn meter_fraction(percent: i64) -> f32 {
    (percent as f32 / 100.0).clamp(0.0, 1.0)
}

/// Tick position for pace-expected marker. `pace_expected` is expected *used* %
/// (`Usage.js` `pace.expectedUsedPercent`); convert into display terms.
/// `0`/non-finite/out-of-range means unknown -> no tick (never render 0 as data).
pub(crate) fn pace_tick_fraction(
    show_pace: bool,
    pace_expected: f64,
    quota_display: &str,
) -> Option<f32> {
    if !show_pace || !pace_expected.is_finite() || pace_expected <= 0.0 || pace_expected > 100.0 {
        return None;
    }
    let display = if quota_display == "used" {
        pace_expected
    } else {
        100.0 - pace_expected
    };
    Some((display as f32 / 100.0).clamp(0.0, 1.0))
}

pub(crate) fn parse_hex_rgb(s: &str) -> Option<(u8, u8, u8)> {
    let hex = s.trim().strip_prefix('#').unwrap_or(s.trim());
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some(((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

pub(crate) fn accent_rgb(provider: &str) -> (u8, u8, u8) {
    crate::descriptors::accent_hex(provider)
        .and_then(parse_hex_rgb)
        .unwrap_or(DEFAULT_METER_RGB)
}

/// Status-page URL from the Swift provider descriptors (`statusPageURL`).
pub(crate) fn status_url(provider: &str) -> Option<&'static str> {
    match provider {
        "codex" | "openai" => Some("https://status.openai.com/"),
        "claude" => Some("https://status.claude.com/"),
        "copilot" => Some("https://www.githubstatus.com/"),
        "cursor" => Some("https://status.cursor.com"),
        _ => None,
    }
}

/// Weekly remaining for the switcher quota strip: the longest window wins.
pub(crate) fn weekly_remaining(entry: &Entry) -> i64 {
    entry
        .windows
        .iter()
        .max_by_key(|w| w.minutes)
        .map(|w| w.remaining.clamp(0, 100))
        .unwrap_or(0)
}

/// Pace stripe colour: `delta = actual - expected` used % (Swift `UsagePace`).
/// At or under pace is green, over pace is red, unknown stays white.
pub(crate) fn pace_stripe_color(delta: f64) -> Color {
    if !delta.is_finite() {
        Color::WHITE
    } else if delta <= 0.0 {
        Color::from_rgb(0.19, 0.82, 0.35)
    } else {
        Color::from_rgb(1.0, 0.42, 0.38)
    }
}

/// Quota-warning marker position in display terms (fraction along the bar).
pub(crate) fn warning_fraction(notify_threshold: u32, quota_display: &str) -> f32 {
    let t = (notify_threshold as f32 / 100.0).clamp(0.0, 1.0);
    if quota_display == "used" {
        1.0 - t
    } else {
        t
    }
}

/// Reset line per `Usage.js resetText()/resetLabel()`: countdown | absolute | both.
pub(crate) fn reset_display_text(
    resets_at: &str,
    fallback: &str,
    mode: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> String {
    let parsed = chrono::DateTime::parse_from_rfc3339(resets_at.trim()).ok();
    let Some(at) = parsed else {
        let fb = fallback.trim();
        return if fb.is_empty() {
            "Reset time unavailable".to_string()
        } else {
            fb.to_string()
        };
    };
    let at_utc = at.with_timezone(&chrono::Utc);
    let countdown = {
        let minutes = ((at_utc - now).num_seconds() as f64 / 60.0).ceil() as i64;
        if minutes <= 0 {
            "Reset due · refresh to update".to_string()
        } else if minutes < 60 {
            format!("Resets in {minutes}m")
        } else if minutes < 1440 {
            format!("Resets in {}h {}m", minutes / 60, minutes % 60)
        } else {
            format!("Resets in {}d {}h", minutes / 1440, (minutes % 1440) / 60)
        }
    };
    let absolute = at_utc
        .with_timezone(&chrono::Local)
        .format("%b %-d, %H:%M")
        .to_string();
    match mode {
        "absolute" => format!("Resets {absolute}"),
        "both" => format!("{countdown} · {absolute}"),
        _ => countdown,
    }
}

pub(crate) fn format_money(v: f64) -> String {
    if !v.is_finite() {
        return EM_DASH.to_string();
    }
    let sign = if v < 0.0 { "-" } else { "" };
    let a = v.abs();
    if a >= 1000.0 {
        format!("{sign}${}", format_count(a.round() as i64))
    } else {
        format!("{sign}${a:.2}")
    }
}

pub(crate) fn format_count(v: i64) -> String {
    let sign = if v < 0 { "-" } else { "" };
    let digits = v.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    format!("{sign}{out}")
}

pub(crate) fn provenance_label(v: &str) -> &str {
    match v {
        "listPriceEstimate" => "List-price estimate",
        "actual" => "Reported cost",
        "unknown" | "" => "Cost source unavailable",
        other => other,
    }
}

fn updated_line(app: &App) -> String {
    if app.busy {
        "Refreshing usage…".to_string()
    } else if app.stale {
        "Usage may be out of date".to_string()
    } else if let Some(at) = app.updated_epoch {
        format!("Updated {}", relative_ago(at, chrono::Utc::now().timestamp()))
    } else if !app.updated.is_empty() {
        format!("Updated {}", app.updated)
    } else {
        "Waiting for usage…".to_string()
    }
}

/// Mac `Date.relativeDescription` as the menu shows it ("just now", "2 min ago").
pub(crate) fn relative_ago(at: i64, now: i64) -> String {
    let secs = (now - at).max(0);
    match secs {
        0..=14 => "just now".into(),
        15..=59 => format!("{secs} sec ago"),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} hr ago", secs / 3600),
        _ => {
            let d = secs / 86_400;
            format!("{d} day{} ago", if d == 1 { "" } else { "s" })
        }
    }
}

/// Mac `MenuCardView.planDisplay`: Codex Pro tiers get their display names; otherwise
/// drop boilerplate words (claude/codex/account/plan) and capitalize a lowercase first letter.
pub(crate) fn plan_display(raw: &str, provider: &str) -> String {
    let raw = raw.trim();
    if provider == "codex" {
        match raw.to_lowercase().as_str() {
            "pro" => return "Pro 20x".into(),
            "prolite" | "pro_lite" | "pro-lite" | "pro lite" => return "Pro 5x".into(),
            _ => {}
        }
    }
    let kept: Vec<&str> = raw
        .split_whitespace()
        .filter(|w| !matches!(w.to_lowercase().as_str(), "claude" | "codex" | "account" | "plan"))
        .collect();
    let cleaned = if kept.is_empty() { raw.to_string() } else { kept.join(" ") };
    if cleaned.eq_ignore_ascii_case("oauth") {
        return "Ollama".into();
    }
    let mut chars = cleaned.chars();
    match chars.next() {
        Some(c) if c.is_lowercase() => c.to_uppercase().chain(chars).collect(),
        _ => cleaned,
    }
}

/// Mac `UsagePaceText.weeklyDetail`: the CLI's " | "-joined pace summary becomes a left label
/// ("54% in reserve") and a right label ("Lasts until reset · 1.5× headroom"); the menu never
/// shows the "Expected N% used" part.
fn pace_detail(summary: &str) -> (String, Option<String>) {
    let mut parts = summary
        .split(" | ")
        .map(str::trim)
        .filter(|p| !p.is_empty() && !p.starts_with("Expected "));
    let left = parts.next().unwrap_or_default().to_string();
    let rest: Vec<&str> = parts.collect();
    (left, (!rest.is_empty()).then(|| rest.join(" · ")))
}

fn format_credits(v: f64) -> String {
    if v.fract() == 0.0 && v.is_finite() && v.abs() <= i64::MAX as f64 {
        format!("{}", v as i64)
    } else {
        format!("{v:.2}")
    }
}

pub fn scroll_id() -> scrollable::Id {
    scrollable::Id::new("quick-view-body")
}

pub fn view(app: &App) -> Element<Message> {
    // Mac menu order: switcher, card + chart rows, action rows (all scrolling), then the fixed
    // Settings / About / Quit rows.
    let content = column![body(app), actions(app)].width(Length::Fill);
    column![
        switcher(app),
        horizontal_rule(1),
        crate::ui::style::scrollable(content)
            .id(scroll_id())
            .width(Length::Fill)
            .height(Length::Fill),
        horizontal_rule(1),
        footer(app),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Hairline divider between card sections (inset by the card padding).
fn rule<'a>() -> Element<'a, Message> {
    horizontal_rule(1).into()
}

/// Canvas usage meter (mac `UsageProgressBar`): capsule track/fill with a
/// punched pace stripe and quota-warning marker drawn over the fill.
#[derive(Debug, Clone, Copy)]
struct UsageMeter {
    fraction: f32,
    fill: Color,
    pace: Option<(f32, Color)>,
    warning: Option<f32>,
}

impl iced::widget::canvas::Program<Message> for UsageMeter {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry<iced::Renderer>> {
        let mut frame = iced::widget::canvas::Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        let r = h / 2.0;
        let track = Color::from_rgba(1.0, 1.0, 1.0, 0.22);
        frame.fill(
            &iced::widget::canvas::Path::rounded_rectangle(
                iced::Point::ORIGIN,
                iced::Size::new(w, h),
                r.into(),
            ),
            track,
        );
        let fill_w = (w * self.fraction.clamp(0.0, 1.0)).max(if self.fraction > 0.0 {
            h
        } else {
            0.0
        });
        if fill_w > 0.0 {
            frame.fill(
                &iced::widget::canvas::Path::rounded_rectangle(
                    iced::Point::ORIGIN,
                    iced::Size::new(fill_w, h),
                    r.into(),
                ),
                self.fill,
            );
        }
        // Punched markers: repaint the track across the slot, then the stripe.
        let punch = |frame: &mut iced::widget::canvas::Frame<iced::Renderer>,
                     at: f32,
                     slot: f32,
                     stripe: f32,
                     color: Color| {
            let x = (w * at.clamp(0.0, 1.0) - slot / 2.0).clamp(0.0, (w - slot).max(0.0));
            frame.fill_rectangle(iced::Point::new(x, 0.0), iced::Size::new(slot, h), track);
            let sx = (w * at.clamp(0.0, 1.0) - stripe / 2.0).clamp(0.0, (w - stripe).max(0.0));
            frame.fill_rectangle(iced::Point::new(sx, 0.0), iced::Size::new(stripe, h), color);
        };
        if let Some(at) = self.warning {
            punch(&mut frame, at, 5.0, 1.0, Color::from_rgba(1.0, 1.0, 1.0, 0.85));
        }
        if let Some((at, color)) = self.pace {
            punch(&mut frame, at, 6.0, 2.0, color);
        }
        vec![frame.into_geometry()]
    }
}

fn usage_meter(
    display_value: i64,
    fill: Color,
    pace: Option<(f32, Color)>,
    warning: Option<f32>,
) -> Element<'static, Message> {
    iced::widget::Canvas::new(UsageMeter {
        fraction: meter_fraction(display_value),
        fill,
        pace,
        warning,
    })
    .width(Length::Fill)
    .height(Length::Fixed(6.0))
    .into()
}

/// Slim capsule meter for switcher/overview strips (no markers).
fn meter_bar(
    display_value: i64,
    fill: Color,
    tick: Option<f32>,
    height: f32,
) -> Element<'static, Message> {
    let _ = tick;
    progress_bar(0.0..=100.0, meter_fraction(display_value) * 100.0)
        .height(height)
        .width(Length::Fill)
        .style(crate::ui::style::meter(fill, height))
        .into()
}

fn tab_style(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, status: button::Status| {
        let (background, text_color) = if selected {
            (
                Some(Background::Color(crate::ui::style::ACCENT)),
                Color::WHITE,
            )
        } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
            (
                Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.06))),
                theme.palette().text,
            )
        } else {
            (None, crate::ui::style::SECONDARY)
        };
        button::Style {
            background,
            text_color,
            border: Border::default().rounded(crate::ui::style::R_SM),
            ..Default::default()
        }
    }
}

/// 2x2 grid icon for the Overview tab, drawn as shapes (no font glyph dependency).
fn overview_glyph<'a>(c: Color) -> Element<'a, Message> {
    let cell = move || {
        container(Space::new(Length::Fixed(6.0), Length::Fixed(6.0))).style(move |_| {
            container::Style {
                background: Some(c.into()),
                border: Border::default().rounded(1.5),
                ..Default::default()
            }
        })
    };
    container(
        column![
            row![cell(), cell()].spacing(2),
            row![cell(), cell()].spacing(2)
        ]
        .spacing(2),
    )
    .center(Length::Fixed(16.0))
    .into()
}

/// One switcher tab (mac `ProviderSwitcherButtons`): stacked icon+name with a
/// 2px weekly-remaining strip; inline icon+name when few segments are shown.
fn tab<'a>(
    selected: bool,
    icon: Element<'a, Message>,
    label: String,
    mini: Element<'a, Message>,
    index: i32,
    inline: bool,
) -> Element<'a, Message> {
    let fg = if selected { Color::WHITE } else { muted() };
    let content: Element<'a, Message> = if inline {
        row![
            icon,
            text(label)
                .size(11)
                .color(fg)
                .wrapping(iced::widget::text::Wrapping::None),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .into()
    } else {
        column![
            icon,
            text(label)
                .size(11)
                .color(fg)
                .wrapping(iced::widget::text::Wrapping::None),
            container(mini).width(Length::Fixed(46.0)),
        ]
        .align_x(Alignment::Center)
        .spacing(2)
        .into()
    };
    crate::ui::style::button(container(content).center_x(Length::Fill))
        .on_press(Message::SelectEntry(index))
        .style(tab_style(selected))
        .padding(if inline { [4, 7] } else { [4, 6] })
        .width(Length::Fill)
        .into()
}

/// Quick View window width (main.rs opens it at 340 wide).
const MENU_WIDTH: f32 = 340.0;

/// Mac `switcherOuterPadding`: 16, else 10, else 6, whichever keeps segments comfortable.
fn switcher_outer_padding(width: f32, count: usize) -> f32 {
    let comfortable = if count >= 5 { 50.0 } else { 54.0 };
    let average = |pad: f32| (width - pad * 2.0 - (count.saturating_sub(1)) as f32) / count as f32;
    [16.0, 10.0]
        .into_iter()
        .find(|&pad| average(pad) >= comfortable)
        .unwrap_or(6.0)
}

/// Mac `switcherRowCount` for stacked (icon over label) segments.
pub(crate) fn switcher_row_count(width: f32, count: usize) -> usize {
    if count <= 1 {
        return 1;
    }
    let max_rows = count.min(4);
    if count >= 15 {
        return max_rows;
    }
    let allowed = |per_row: usize| {
        let pad = switcher_outer_padding(width, per_row);
        (width - pad * 2.0 - (per_row - 1) as f32) / per_row as f32
    };
    if allowed(count) >= 50.0 {
        return 1;
    }
    (2..=max_rows)
        .find(|&rows| allowed(count.div_ceil(rows)) >= 50.0)
        .unwrap_or(max_rows)
}

/// Mac `splitRows`: earlier rows take the remainder, so 7 over 2 rows is 4 + 3.
pub(crate) fn split_rows(count: usize, rows: usize) -> Vec<usize> {
    (0..rows)
        .map(|i| count / rows + usize::from(i < count % rows))
        .collect()
}

fn switcher(app: &App) -> Element<'_, Message> {
    // Mac shows a segment per enabled provider only; the "all" selection also returns every
    // unconfigured provider as an error entry, which would otherwise flood the switcher.
    let shown: Vec<(usize, &Entry)> = app
        .entries
        .iter()
        .enumerate()
        .filter(|(i, e)| {
            app.selected == *i as i32
                || crate::ui::settings_sidebar::provider_enabled(app, &e.provider)
        })
        .collect();
    let count = shown.len() + 1;
    let inline = count <= 3;
    let mut tabs: Vec<Element<'_, Message>> = Vec::with_capacity(count);
    let overview_selected = app.selected == -1;
    let overview_color = if overview_selected {
        Color::WHITE
    } else {
        muted()
    };
    tabs.push(tab(
        overview_selected,
        overview_glyph(overview_color),
        "Overview".to_string(),
        meter_bar(0, Color::TRANSPARENT, None, 2.0),
        -1,
        inline,
    ));
    for (i, entry) in shown {
        let selected = app.selected == i as i32;
        let fg = if selected { Color::WHITE } else { muted() };
        // Mac quota strip: weekly remaining, always in the provider accent.
        let value = weekly_remaining(entry);
        let fill = rgb(accent_rgb(&entry.provider));
        tabs.push(tab(
            selected,
            crate::icons::icon(&entry.provider, 16.0, fg),
            crate::model::provider_name(&entry.provider),
            meter_bar(value, fill, None, 2.0),
            i as i32,
            inline,
        ));
    }
    // Mac wraps segments into up to four rows of uniform width instead of scrolling.
    let rows = if inline { 1 } else { switcher_row_count(MENU_WIDTH, count) };
    let sizes = split_rows(count, rows);
    let columns = sizes.iter().copied().max().unwrap_or(1);
    let pad = switcher_outer_padding(MENU_WIDTH, columns);
    let mut grid = Column::new()
        .spacing(if rows > 1 { 4 } else { 2 })
        .width(Length::Fill);
    let mut iter = tabs.into_iter();
    for size in sizes {
        let mut r = Row::new().spacing(1).width(Length::Fill);
        for t in iter.by_ref().take(size) {
            r = r.push(t);
        }
        for _ in size..columns {
            r = r.push(Space::with_width(Length::Fill));
        }
        // Rows hug their tabs; without this a fill-height child lets them take the window.
        grid = grid.push(r.height(Length::Shrink));
    }
    container(grid)
        .padding(iced::Padding {
            top: 10.0,
            right: pad,
            bottom: 6.0,
            left: pad,
        })
        .width(Length::Fill)
        .into()
}

fn body(app: &App) -> Element<'_, Message> {
    if app.entries.is_empty() {
        let mut col = Column::new().spacing(8);
        col = col.push(
            text(if app.busy {
                "Refreshing usage…".to_string()
            } else {
                "No providers to show. Enable one in Settings.".to_string()
            })
            .size(13)
            .color(muted()),
        );
        if !app.busy && !app.error.is_empty() {
            col = col.push(text(app.error.clone()).size(12).color(rgb(ERROR_RGB)));
        }
        if app.stale {
            col = col.push(text("Usage may be out of date").size(12).color(muted()));
        }
        return container(col).padding(20).width(Length::Fill).into();
    }
    if app.selected == -1 {
        return overview_body(app);
    }
    match app.entries.get(app.selected as usize) {
        Some(entry) => {
            let cost = app.spending.iter().find(|c| c.provider == entry.provider);
            provider_body(app, entry, cost)
        }
        None => overview_body(app),
    }
}

fn overview_body(app: &App) -> Element<'_, Message> {
    // Mac overview: spend summary header, then flat full provider rows (no
    // title, no card chrome); compact metrics hide reset/meta lines.
    let mut col = Column::new().spacing(12);
    if !app.spending.is_empty() {
        let today: f64 = app.spending.iter().map(|c| c.today).sum();
        let month: f64 = app.spending.iter().map(|c| c.month).sum();
        let tokens: i64 = app.spending.iter().map(|c| c.tokens).sum();
        col = col.push(
            Column::new()
                .spacing(2)
                .push(
                    text("Last 30 days")
                        .size(crate::ui::style::TEXT_HEADLINE)
                        .font(semibold())
                        .color(muted()),
                )
                .push(
                    text(format_money(month))
                        .size(crate::ui::style::TEXT_TITLE2)
                        .font(crate::ui::style::font_bold()),
                )
                .push(
                    text(format!(
                        "Today {} · {} tokens · {}",
                        format_money(today),
                        format_count(tokens),
                        updated_line(app)
                    ))
                    .size(crate::ui::style::TEXT_CAPTION)
                    .color(muted()),
                ),
        );
    } else {
        col = col.push(
            text(updated_line(app))
                .size(crate::ui::style::TEXT_CAPTION)
                .color(muted()),
        );
        if app.cost_busy {
            col = col.push(text("Reading local history…").size(12).color(muted()));
        }
    }
    for (i, entry) in app.entries.iter().enumerate() {
        col = col.push(rule());
        let name = crate::model::provider_name(&entry.provider);
        let mut lines = Column::new().spacing(6).width(Length::Fill);
        let mut head = Row::new().spacing(8).align_y(Alignment::Center);
        head = head.push(crate::icons::brand(&entry.provider, 16.0, Color::WHITE));
        head = head.push(
            text(name)
                .size(crate::ui::style::TEXT_HEADLINE)
                .font(semibold())
                .width(Length::Fill),
        );
        if !entry.plan.is_empty() {
            head = head.push(
                text(plan_display(&entry.plan, &entry.provider))
                    .size(crate::ui::style::TEXT_FOOTNOTE)
                    .font(medium())
                    .color(muted()),
            );
        }
        lines = lines.push(head);
        if entry.windows.is_empty() {
            lines = lines.push(text(EM_DASH).size(12).color(muted()));
        } else {
            for w in &entry.windows {
                lines = lines.push(compact_metric(w, &app.settings, &entry.provider, app.stale));
            }
        }
        col = col.push(
            crate::ui::style::button(container(lines).width(Length::Fill))
                .on_press(Message::SelectEntry(i as i32))
                .style(crate::ui::style::plain)
                .padding([4, 0])
                .width(Length::Fill),
        );
    }
    container(col).padding([6, 20]).width(Length::Fill).into()
}

/// Compact overview metric: title + percent, then the bar (no reset/meta).
fn compact_metric(
    window: &QuotaWindow,
    s: &Settings,
    provider: &str,
    stale: bool,
) -> Element<'static, Message> {
    let amount = quota_display_value(window.remaining, &s.quota_display);
    let fill = if stale {
        muted()
    } else {
        rgb(accent_rgb(provider))
    };
    let label = if window.label.is_empty() {
        EM_DASH.to_string()
    } else {
        window.label.clone()
    };
    Column::new()
        .spacing(4)
        .push(
            row![
                text(label)
                    .size(crate::ui::style::TEXT_BODY)
                    .font(medium())
                    .width(Length::Fill),
                text(format!("{amount}%"))
                    .size(crate::ui::style::TEXT_BODY)
                    .font(crate::ui::style::font_numeric()),
            ]
            .align_y(Alignment::Center),
        )
        .push(meter_bar(amount, fill, None, 6.0))
        .into()
}

/// Hover-highlighted, borderless menu row (mac `MenuHighlightStyle`).
fn menu_row_style(theme: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let disabled = matches!(status, button::Status::Disabled);
    button::Style {
        background: hovered.then(|| Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.08))),
        text_color: if disabled {
            muted()
        } else {
            theme.palette().text
        },
        border: Border::default().rounded(6.0),
        ..Default::default()
    }
}

/// Plain-text menu row (Mac NSMenu items carry no icons), optionally with a trailing chevron.
fn menu_row<'a>(
    label: String,
    msg: Option<Message>,
    trailing: Option<Glyph>,
) -> Element<'a, Message> {
    let mut r = Row::new()
        .spacing(10)
        .align_y(Alignment::Center)
        .push(text(label).size(13).width(Length::Fill));
    if let Some(t) = trailing {
        r = r.push(glyph(t, 11.0, muted()));
    }
    crate::ui::style::button(r)
        .on_press_maybe(msg)
        .style(menu_row_style)
        .padding([5, 8])
        .width(Length::Fill)
        .into()
}

/// Mac status submenu (`StatusComponentsMenuView`): dot · name · status per component, groups
/// expand to their children, then the website link. Fetched when the row first opens.
fn status_submenu<'a>(app: &'a App, provider: &str, url: &'static str) -> Element<'a, Message> {
    let key = format!("{provider}:status");
    let open = app.toggled_charts.contains(&key);
    let mut col = Column::new()
        .spacing(1)
        .push(menu_row(
            "Status Page".to_string(),
            Some(Message::ToggleChart(key.clone())),
            Some(if open {
                Glyph::ChevronDown
            } else {
                Glyph::ChevronRight
            }),
        ));
    if !open {
        return col.into();
    }
    let mut list = Column::new().spacing(6).padding([4, 14]);
    match app.status_components.get(provider) {
        None => {
            list = list.push(text("Loading…").size(12).color(muted()));
        }
        Some((_, Err(e))) => {
            list = list.push(text(e.clone()).size(12).color(muted()));
        }
        Some((_, Ok(components))) => {
            for c in components {
                if c.children.is_empty() {
                    list = list.push(status_line(c, None, false));
                } else {
                    let gkey = format!("{key}:{}", c.id);
                    let expanded = app.toggled_charts.contains(&gkey);
                    list = list.push(
                        crate::ui::style::button(status_line(c, Some(expanded), false))
                            .on_press(Message::ToggleChart(gkey))
                            .style(|theme: &Theme, _| button::Style {
                                text_color: theme.palette().text,
                                ..Default::default()
                            })
                            .padding(0)
                            .width(Length::Fill),
                    );
                    if expanded {
                        for child in &c.children {
                            list = list.push(status_line(child, None, true));
                        }
                    }
                }
            }
        }
    }
    col = col.push(list);
    col = col.push(menu_row(
        "Open Status Page".to_string(),
        Some(Message::OpenUrl(url.to_string())),
        None,
    ));
    col.into()
}

fn status_line(
    c: &crate::status_feed::Component,
    expanded: Option<bool>,
    indented: bool,
) -> Element<'static, Message> {
    let dot = crate::status_feed::dot_rgb(&c.status)
        .map(|(r, g, b)| Color::from_rgb(r, g, b))
        .unwrap_or_else(muted);
    let mut r = Row::new()
        .spacing(8)
        .align_y(Alignment::Center)
        .push(Space::with_width(Length::Fixed(if indented { 17.0 } else { 0.0 })))
        .push(
            container(Space::new(Length::Fixed(8.0), Length::Fixed(8.0))).style(
                move |_: &Theme| container::Style {
                    background: Some(Background::Color(dot)),
                    border: Border::default().rounded(4.0),
                    ..Default::default()
                },
            ),
        )
        .push(text(c.name.clone()).size(13));
    if let Some(open) = expanded {
        r = r.push(glyph(
            if open {
                Glyph::ChevronDown
            } else {
                Glyph::ChevronRight
            },
            9.0,
            muted(),
        ));
    }
    r.push(Space::with_width(Length::Fill))
        .push(
            text(crate::status_feed::label(&c.status))
                .size(12)
                .color(muted()),
        )
        .into()
}

fn selected_entry(app: &App) -> Option<&Entry> {
    if app.selected >= 0 {
        app.entries.get(app.selected as usize)
    } else {
        None
    }
}

/// Scrolling action rows (mac "Add Account / Usage Dashboard / Status Page" group).
fn actions(app: &App) -> Element<'_, Message> {
    let mut col = Column::new().spacing(1);
    col = col.push(container(rule()).padding([0.0, 8.0]));
    if let Some(entry) = selected_entry(app) {
        let provider = entry.provider.clone();
        // Add Account: CLI sign-in where supported, else the Providers pane.
        let add_msg = if matches!(provider.as_str(), "codex" | "claude") {
            Message::AccountAction {
                provider: provider.clone(),
                login: true,
            }
        } else {
            Message::OpenSettingsPane(SettingsPane::Providers)
        };
        col = col.push(menu_row(
            "Add Account…".to_string(),
            Some(add_msg),
            None,
        ));
        if let Some(url) = crate::ui::settings_view::dashboard_url(&provider) {
            col = col.push(menu_row(
                "Usage Dashboard".to_string(),
                Some(Message::OpenUrl(url.to_string())),
                None,
            ));
        }
        if let Some(url) = status_url(&provider) {
            if matches!(provider.as_str(), "codex" | "claude") {
                col = col.push(status_submenu(app, &provider, url));
            } else {
                col = col.push(menu_row(
                    "Status Page".to_string(),
                    Some(Message::OpenUrl(url.to_string())),
                    None,
                ));
            }
        }
        if !entry.status.is_empty() {
            col = col.push(
                container(
                    text(entry.status.clone())
                        .size(crate::ui::style::TEXT_CAPTION)
                        .color(muted()),
                )
                .padding([2, 8]),
            );
        }
    }
    col = col.push(menu_row(
        "Usage & Spend…".to_string(),
        Some(Message::ShowPage(Page::Usage)),
        None,
    ));
    col = col.push(menu_row(
        "Agent Sessions…".to_string(),
        Some(Message::ShowPage(Page::Sessions)),
        None,
    ));
    container(col)
        .padding(iced::Padding {
            top: 0.0,
            right: 12.0,
            bottom: 6.0,
            left: 12.0,
        })
        .width(Length::Fill)
        .into()
}

/// Fixed bottom rows (mac order): Refresh / Settings… / About / Quit.
fn footer(app: &App) -> Element<'_, Message> {
    let refresh = if app.busy {
        menu_row("Refreshing…".to_string(), None, None)
    } else {
        menu_row(
            "Refresh".to_string(),
            Some(Message::Refresh),
            None,
        )
    };
    container(
        column![
            refresh,
            menu_row(
                "Settings…".to_string(),
                Some(Message::ShowPage(Page::Settings)),
                None,
            ),
            menu_row(
                format!("About CodexBar (v{})", env!("CARGO_PKG_VERSION")),
                Some(Message::OpenSettingsPane(SettingsPane::About)),
                None,
            ),
            menu_row("Quit".to_string(), Some(Message::Quit), None),
        ]
        .spacing(1),
    )
    .padding(iced::Padding {
        top: 6.0,
        right: 12.0,
        bottom: 8.0,
        left: 12.0,
    })
    .width(Length::Fill)
    .into()
}

/// Card section heading (mac body medium, e.g. Credits/Cost/Extra usage).
fn section_title(label: &str) -> Element<'static, Message> {
    text(label.to_string())
        .size(crate::ui::style::TEXT_BODY)
        .font(medium())
        .into()
}

/// Detail-group heading (mac caption semibold, uppercased, secondary).
fn detail_title(label: &str) -> Element<'static, Message> {
    text(label.to_uppercase())
        .size(crate::ui::style::TEXT_CAPTION)
        .font(semibold())
        .color(muted())
        .into()
}

fn provider_body<'a>(
    app: &'a App,
    entry: &'a Entry,
    cost: Option<&'a CostEntry>,
) -> Element<'a, Message> {
    let s = &app.settings;
    let mut col = Column::new().spacing(12).width(Length::Fill);
    // Card header (mac `UsageMenuCardHeaderView`): headline name + subheadline
    // account; the subtitle row carries updated, or the error when present.
    let mut header = Column::new().spacing(2).width(Length::Fill);
    let mut head1 = Row::new().align_y(Alignment::Center);
    head1 = head1.push(
        text(crate::model::provider_name(&entry.provider))
            .size(crate::ui::style::TEXT_HEADLINE)
            .font(semibold())
            .width(Length::Fill),
    );
    if !entry.account_label.is_empty() {
        head1 = head1.push(
            text(entry.account_label.clone())
                .size(crate::ui::style::TEXT_SUBHEADLINE)
                .color(muted()),
        );
    }
    header = header.push(head1);
    let mut head2 = Row::new().spacing(6).align_y(Alignment::Center);
    if !entry.error.is_empty() {
        head2 = head2.push(
            text(entry.error.clone())
                .size(crate::ui::style::TEXT_CAPTION)
                .color(rgb(ERROR_RGB))
                .width(Length::Fill),
        );
        head2 = head2.push(
            crate::ui::style::button(glyph(Glyph::Copy, 13.0, muted()))
                .on_press(Message::CopyText(entry.error.clone()))
                .style(crate::ui::style::plain)
                .padding([3, 5]),
        );
    } else {
        head2 = head2.push(
            text(updated_line(app))
                .size(crate::ui::style::TEXT_CAPTION)
                .color(muted())
                .width(Length::Fill),
        );
    }
    if !entry.plan.is_empty() {
        head2 = head2.push(
            text(plan_display(&entry.plan, &entry.provider))
                .size(crate::ui::style::TEXT_FOOTNOTE)
                .font(medium())
                .color(muted()),
        );
    }
    header = header.push(head2);
    col = col.push(header);
    col = col.push(rule());
    // Metrics in order
    if entry.windows.is_empty() {
        col = col.push(text(EM_DASH).size(13).color(muted()));
    } else {
        let mut metrics = Column::new().spacing(12).width(Length::Fill);
        for window in &entry.windows {
            metrics = metrics.push(metric_view(window, s, &entry.provider, app.stale));
        }
        col = col.push(metrics);
    }
    // DeepSeek-only price clock
    if entry.provider == "deepseek" {
        col = col.push(deepseek_section());
    }
    // OpenAI-only: Admin API live usage window (mac "Live Usage" card action).
    if entry.provider == "openai" {
        col = col.push(menu_row(
            "Live usage…".to_string(),
            Some(Message::ShowPage(Page::OpenAiLive)),
            None,
        ));
    }
    // Credits. The CLI carries only `remaining` (no limit), so no bar can be
    // drawn yet; the value stays text until the contract grows a limit.
    // Codex always shows the block (mac "Credits · Balance: Unavailable" + "Buy Credits…").
    if entry.credits.is_some() || entry.provider == "codex" {
        col = col.push(rule());
        col = col.push(section_title("Credits"));
        col = col.push(match entry.credits {
            Some(credits) => text(format!("{} remaining", format_credits(credits)))
                .size(crate::ui::style::TEXT_FOOTNOTE)
                .color(muted()),
            None => text("Credits · Balance: Unavailable")
                .size(crate::ui::style::TEXT_FOOTNOTE),
        });
        if entry.provider == "codex" {
            if let Some(url) = crate::ui::settings_view::dashboard_url("codex") {
                col = col.push(menu_row(
                    "Buy Credits…".to_string(),
                    Some(Message::OpenUrl(url.to_string())),
                    None,
                ));
            }
        }
    }
    // Reset credits (hide at 0: model cannot distinguish unknown from none)
    if entry.reset_credits_available > 0 {
        col = col.push(rule());
        col = col.push(section_title("Limit Reset Credits"));
        let mut line = Row::new().align_y(Alignment::Center);
        line = line.push(
            text(format!("{} available", entry.reset_credits_available))
                .size(crate::ui::style::TEXT_FOOTNOTE)
                .font(semibold())
                .width(Length::Fill),
        );
        if !entry.reset_credits_next_expiry.is_empty() {
            line = line.push(
                text(format!(
                    "Next expires {}",
                    entry.reset_credits_next_expiry
                ))
                .size(crate::ui::style::TEXT_CAPTION)
                .color(muted()),
            );
        }
        col = col.push(line);
    }
    // Extra usage / provider cost. Blocked on the CLI `providerCost` field:
    // `extra_usage_text` is still always empty, so this never renders yet.
    if !entry.extra_usage_text.is_empty() {
        col = col.push(rule());
        col = col.push(section_title("Extra usage"));
        col = col.push(
            text(entry.extra_usage_text.clone())
                .size(crate::ui::style::TEXT_FOOTNOTE)
                .color(muted()),
        );
    }
    // Cost (local history), with the cost-history chart inline.
    if s.show_costs
        && (cost.is_some()
            || app.cost_busy
            || !app.cost_error.is_empty()
            || cost_payload(app, &entry.provider).is_some())
    {
        col = col.push(rule());
        col = col.push(cost_section(app, &entry.provider, cost));
    }
    col = col.push(chart_sections(app, &entry.provider));
    // Provider detail sections: label left (secondary), value right (medium),
    // secondary caption under the value (mac `DetailFieldRow`).
    for section in &entry.details {
        let mut sec = Column::new().spacing(6).width(Length::Fill);
        sec = sec.push(rule());
        if !section.title.is_empty() {
            sec = sec.push(detail_title(&section.title));
        }
        for (label, value, secondary) in &section.rows {
            let shown_value = if value.is_empty() {
                EM_DASH
            } else {
                value.as_str()
            };
            if label.is_empty() {
                sec = sec.push(
                    text(shown_value.to_string())
                        .size(crate::ui::style::TEXT_CAPTION)
                        .font(medium()),
                );
            } else {
                let mut val = Column::new()
                    .spacing(0)
                    .align_x(Alignment::End)
                    .push(
                        text(shown_value.to_string())
                            .size(crate::ui::style::TEXT_CAPTION)
                            .font(medium()),
                    );
                if !secondary.is_empty() {
                    val = val.push(
                        text(secondary.clone())
                            .size(crate::ui::style::TEXT_CAPTION2)
                            .color(muted()),
                    );
                }
                sec = sec.push(
                    row![
                        text(label.clone())
                            .size(crate::ui::style::TEXT_CAPTION)
                            .color(muted())
                            .width(Length::Fill),
                        val,
                    ]
                    .spacing(12),
                );
            }
        }
        col = col.push(sec);
    }
    container(col).padding([6, 20]).width(Length::Fill).into()
}

/// Metric row (mac `MenuCardMetricRow`): header `Title N% used | reset`, then
/// the canvas bar (tint kept; warning is a marker, pace a punched stripe),
/// then the pace meta line. Stale data greys the fill instead of recolouring.
fn metric_view(
    window: &QuotaWindow,
    s: &Settings,
    provider: &str,
    stale: bool,
) -> Element<'static, Message> {
    let amount = quota_display_value(window.remaining, &s.quota_display);
    let fill = if stale {
        muted()
    } else {
        rgb(accent_rgb(provider))
    };
    let tick = pace_tick_fraction(s.show_pace, window.pace_expected, &s.quota_display);
    let pace = tick.map(|f| (f, pace_stripe_color(window.pace_delta)));
    let warning = is_warning(s.warning_colors, window.remaining, s.notify_threshold)
        .then(|| warning_fraction(s.notify_threshold, &s.quota_display));
    let label = if window.label.is_empty() {
        EM_DASH.to_string()
    } else {
        window.label.clone()
    };
    let reset = reset_display_text(
        &window.resets_at,
        &window.reset_text,
        &s.reset_display,
        chrono::Utc::now(),
    );
    let mut col = Column::new().spacing(6).width(Length::Fill);
    col = col.push(
        row![
            text(label)
                .size(crate::ui::style::TEXT_BODY)
                .font(medium()),
            text(format!(
                "{amount}% {}",
                quota_display_suffix(&s.quota_display)
            ))
            .size(crate::ui::style::TEXT_BODY)
            .font(crate::ui::style::font_numeric())
            .width(Length::Fill),
            text(reset)
                .size(crate::ui::style::TEXT_FOOTNOTE)
                .font(crate::ui::style::font_numeric())
                .color(muted()),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    );
    col = col.push(usage_meter(amount, fill, pace, warning));
    if s.show_pace && !window.pace.is_empty() {
        let (left, right) = pace_detail(&window.pace);
        let mut line = Row::new().spacing(6).align_y(Alignment::Center);
        line = line.push(
            text(left)
                .size(crate::ui::style::TEXT_CAPTION)
                .color(muted())
                .width(Length::Fill),
        );
        if let Some(right) = right {
            line = line.push(
                text(right)
                    .size(crate::ui::style::TEXT_CAPTION)
                    .color(muted()),
            );
        }
        col = col.push(line);
    }
    col.into()
}

fn deepseek_section() -> Element<'static, Message> {
    let clock = crate::deepseek::price_clock(chrono::Utc::now());
    let badge_color = if clock.is_peak {
        Color::from_rgb(0.91, 0.42, 0.36)
    } else {
        Color::from_rgb(0.22, 0.74, 0.48)
    };
    // Boxed like the Mac card: padded tint surface with a hairline border.
    let mut inner = Column::new().spacing(6).width(Length::Fill);
    inner = inner.push(section_title("Peak Price Clock"));
    let badge = if clock.badge.is_empty() {
        EM_DASH.to_string()
    } else {
        clock.badge.to_uppercase()
    };
    inner = inner.push(
        row![
            container(Space::new(Length::Fixed(7.0), Length::Fixed(7.0))).style(
                move |_: &Theme| container::Style {
                    background: Some(Background::Color(badge_color)),
                    border: Border::default().rounded(3.5),
                    ..Default::default()
                }
            ),
            text(badge)
                .size(crate::ui::style::TEXT_CAPTION2)
                .font(crate::ui::style::font_bold())
                .color(badge_color),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    );
    inner = inner.push(
        text(if clock.countdown.is_empty() {
            EM_DASH.to_string()
        } else {
            clock.countdown.clone()
        })
        .size(24)
        .font(crate::ui::style::font_bold()),
    );
    if !clock.next_transition.is_empty() {
        inner = inner.push(text(clock.next_transition.clone()).size(12).color(muted()));
    }
    for (tag, range, is_peak) in &clock.windows {
        let tag_color = if *is_peak {
            Color::from_rgb(0.91, 0.42, 0.36)
        } else {
            Color::from_rgb(0.22, 0.74, 0.48)
        };
        inner = inner.push(
            row![
                text(tag.clone())
                    .size(12)
                    .font(semibold())
                    .color(tag_color)
                    .width(Length::Fixed(54.0)),
                text(range.clone()).size(11),
            ]
            .align_y(Alignment::Center),
        );
    }
    inner = inner.push(
        text("Excludes Chinese public holidays")
            .size(crate::ui::style::TEXT_MICRO)
            .color(muted()),
    );
    container(inner)
        .padding(10)
        .width(Length::Fill)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.04))),
            border: Border {
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.08),
                width: 1.0,
                radius: crate::ui::style::R_MD.into(),
            },
            ..Default::default()
        })
        .into()
}

fn cost_payload<'a>(app: &'a App, provider: &str) -> Option<&'a crate::cli::CostPayload> {
    app.cost_payloads
        .iter()
        .find(|p| p.provider == provider && p.error.is_none() && !p.daily.is_empty())
}

/// Tokens recorded for `date` in the per-day history, if any.
pub(crate) fn tokens_on(cost: &CostEntry, date: &str) -> Option<i64> {
    cost.daily
        .iter()
        .find(|(d, _, _)| d == date)
        .map(|(_, _, t)| *t)
}

fn cost_line(label: &str, amount: f64, tokens: Option<i64>) -> String {
    match tokens.filter(|t| *t > 0) {
        Some(t) => format!(
            "{label}: {} · {} tokens",
            format_money(amount),
            crate::throughput::count(t as f64)
        ),
        None => format!("{label}: {}", format_money(amount)),
    }
}

/// Collapsible menu-row header (title + chevron); `Show`/`Hide` words are gone like on the Mac.
fn row_header(
    key: String,
    title: &'static str,
    open: bool,
    icon: Option<Glyph>,
    bold: bool,
) -> Element<'static, Message> {
    let mut r = Row::new().spacing(10).align_y(Alignment::Center);
    if let Some(g) = icon {
        r = r.push(glyph(g, 15.0, Color::WHITE));
    }
    let _ = bold;
    let label = text(title)
        .size(crate::ui::style::TEXT_BODY)
        .font(medium());
    r = r.push(label.width(Length::Fill));
    r = r.push(glyph(
        if open {
            Glyph::ChevronDown
        } else {
            Glyph::ChevronRight
        },
        11.0,
        muted(),
    ));
    crate::ui::style::button(r)
        .on_press(Message::ToggleChart(key))
        .style(menu_row_style)
        .padding([5, 8])
        .width(Length::Fill)
        .into()
}

fn detail_text(s: String) -> Element<'static, Message> {
    container(
        text(s)
            .size(crate::ui::style::TEXT_FOOTNOTE)
            .color(muted()),
    )
    .height(Length::Fixed(16.0))
    .into()
}

/// Cost block (mac "Cost ›"): Today / Last 30 days lines and the cost-history chart, open by
/// default. Collapsing it stores `<provider>:cost` in `toggled_charts`.
fn cost_section(app: &App, provider: &str, cost: Option<&CostEntry>) -> Element<'static, Message> {
    let mut sec = Column::new().spacing(5).width(Length::Fill);
    let payload = cost_payload(app, provider);
    let key = format!("{provider}:cost");
    let open = payload.is_some() && !app.toggled_charts.contains(&key);
    sec = if payload.is_some() {
        sec.push(row_header(key, "Cost", open, None, true))
    } else {
        sec.push(section_title("Cost"))
    };
    if app.cost_busy {
        sec = sec.push(text("Reading local history…").size(12).color(muted()));
    } else if !app.cost_error.is_empty() {
        sec = sec.push(text(app.cost_error.clone()).size(12).color(rgb(ERROR_RGB)));
    }
    if let Some(cost) = cost {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        sec = sec.push(
            text(cost_line("Today", cost.today, tokens_on(cost, &today)))
                .size(crate::ui::style::TEXT_FOOTNOTE),
        );
        sec = sec.push(
            text(cost_line("Last 30 days", cost.month, Some(cost.tokens)))
                .size(crate::ui::style::TEXT_FOOTNOTE),
        );
        sec = sec.push(
            text(provenance_label(&cost.provenance).to_string())
                .size(11)
                .color(muted()),
        );
        if !cost.error.is_empty() {
            sec = sec.push(text(cost.error.clone()).size(12).color(muted()));
        }
        if !cost.coverage.is_empty() {
            sec = sec.push(text(cost.coverage.clone()).size(12).color(muted()));
        }
    }
    if let (true, Some(payload)) = (open, payload) {
        let bars = crate::charts::cost_bars(&payload.daily);
        let hover = hover_for(app, &format!("{provider}:cost"));
        let detail = match popover_charts::selected_index(&bars, hover) {
            Some(i) => {
                let date = bars[i].date.clone();
                match payload.daily.iter().find(|d| d.date == date) {
                    Some(d) => popover_charts::cost_detail(&date, d.total_cost, d.total_tokens),
                    None => popover_charts::detail_line(&bars, hover, crate::charts::money),
                }
            }
            None => popover_charts::detail_line(&bars, hover, crate::charts::money),
        };
        let total: f64 = payload.daily.iter().map(|d| d.total_cost).sum();
        sec = sec.push(chart_canvas(crate::charts::BarChart {
            hover,
            key: format!("{provider}:cost"),
            bars,
            color: Color::from_rgb(0.81, 0.56, 0.24),
            peak_cap: true,
            fmt: crate::charts::money,
            axis: crate::charts::axis_money,
            y_max: None,
        }));
        sec = sec.push(detail_text(detail));
        sec = sec.push(detail_text(format!(
            "Est. total (30 days): {}",
            crate::charts::money(total)
        )));
    }
    sec.into()
}

fn hover_for(app: &App, key: &str) -> Option<usize> {
    app.chart_hover
        .as_ref()
        .filter(|(k, _)| k == key)
        .map(|(_, i)| *i)
}

/// Segmented series picker (mac `Picker(.segmented)`); the selected index lives in
/// `App::chart_series` under `key`.
fn series_picker(key: &str, titles: &[String], selected: usize) -> Element<'static, Message> {
    let mut r = Row::new().spacing(6);
    for (i, t) in titles.iter().enumerate() {
        let key = key.to_string();
        r = r.push(
            crate::ui::style::button(text(t.clone()).size(12))
                .on_press(Message::ChartSeries { key, index: i })
                .padding([2, 8])
                .style(crate::ui::style::segment(i == selected)),
        );
    }
    r.into()
}

fn local_clock(unix: i64) -> String {
    chrono::DateTime::from_timestamp(unix, 0)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%a, %b %-d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

fn percent_label(v: f64) -> String {
    format!("{v:.0}%")
}

/// Chart menu rows (mac hover submenus rendered inline): usage breakdown, credits history, plan
/// utilization, quota burndown, tokens/min and storage. Each is hidden when it has no data.
fn chart_sections<'a>(app: &'a App, provider: &str) -> Element<'a, Message> {
    let mut col = Column::new().spacing(5).width(Length::Fill);
    let accent = rgb(accent_rgb(provider));
    let hover_for = |key: &str| hover_for(app, key);
    let selected_series = |key: &str, len: usize| {
        app.chart_series
            .get(key)
            .copied()
            .unwrap_or(0)
            .min(len.saturating_sub(1))
    };
    let tz = chrono::Local::now().offset().local_minus_utc() as i64;
    let now = chrono::Utc::now().timestamp();
    let mut first = true;
    let mut sep = |col: Column<'a, Message>| {
        let col = if first { col.push(rule()) } else { col };
        first = false;
        col
    };
    let section = |key: String, title: &'static str| {
        // Mac shows these behind "›" submenus, so they start collapsed.
        let open = app.toggled_charts.contains(&key);
        (row_header(key, title, open, None, false), open)
    };

    let payload = if app.settings.show_costs {
        cost_payload(app, provider)
    } else {
        None
    };

    // Usage breakdown (stacked per-model bars).
    if let Some(payload) = payload {
        let (bars, _legend) = crate::charts::model_bars(&payload.daily, 5);
        if !bars.is_empty() {
            let key = format!("{provider}:models");
            let (head, open) = section(key.clone(), "Usage breakdown");
            col = sep(col).push(head);
            if open {
                let hover = hover_for(&key);
                let detail = popover_charts::detail_line(&bars, hover, crate::charts::money);
                let rows =
                    popover_charts::detail_rows(&bars, hover, crate::charts::money, 4);
                col = col.push(chart_canvas(crate::charts::BarChart {
                    hover,
                    key,
                    bars,
                    color: accent,
                    peak_cap: false,
                    fmt: crate::charts::money,
                    axis: crate::charts::axis_money,
                    y_max: None,
                }));
                // Mac order: chart, then the fixed-height detail, then the legend.
                col = col.push(detail_text(detail));
                for (color_index, name, value) in rows {
                    let swatch = crate::charts::segment_color(color_index);
                    col = col.push(
                        row![
                            container(Space::new(Length::Fixed(7.0), Length::Fixed(7.0)))
                                .style(move |_: &Theme| container::Style {
                                    background: Some(Background::Color(swatch)),
                                    border: Border::default().rounded(3.5),
                                    ..Default::default()
                                }),
                            text(name).size(12).width(Length::Fill),
                            text(value).size(12).color(muted()),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
                    );
                }
            }
        }
    }

    // Credits history (local balance snapshots; mac scrapes the OpenAI web dashboard instead).
    if let Some(bars) = app.history.credit_bars(provider, tz) {
        let key = format!("{provider}:credits");
        let (head, open) = section(key.clone(), "Credits history");
        col = sep(col).push(head);
        if open {
            let total: f64 = bars.iter().filter_map(|b| b.value).sum();
            let hover = hover_for(&key);
            col = col.push(chart_canvas(crate::charts::BarChart {
                hover,
                key,
                color: Color::from_rgb(0.29, 0.64, 0.69),
                peak_cap: true,
                fmt: credits_label,
                axis: credits_label,
                y_max: None,
                bars: bars.clone(),
            }));
            col = col.push(detail_text(popover_charts::detail_line(
                &bars,
                hover,
                credits_label,
            )));
            col = col.push(detail_text(format!(
                "Credits used (total {})",
                credits_label(total)
            )));
            col = col.push(
                text("Daily drops in the credit balance, sampled hourly on this machine; top-ups are ignored.")
                    .size(11)
                    .color(muted()),
            );
        }
    }

    // Plan Usage (mac submenu): quota burndown (codex/claude), separator, utilization history.
    let series = app.history.series_for(provider);
    if !series.is_empty() {
        let key = format!("{provider}:util");
        let (head, open) = section(key.clone(), "Plan Usage");
        col = sep(col).push(head);
        if open {
            let burns: Vec<(&crate::history::Series, crate::history::Burndown)> =
                if matches!(provider, "codex" | "claude") {
                    series
                        .iter()
                        .filter_map(|s| {
                            crate::history::Burndown::from_series(s, now).map(|b| (*s, b))
                        })
                        .collect()
                } else {
                    Vec::new()
                };
            if !burns.is_empty() {
                let key = format!("{provider}:burn");
                let idx = selected_series(&key, burns.len());
                if burns.len() > 1 {
                    let titles: Vec<String> = burns.iter().map(|b| b.0.label.clone()).collect();
                    col = col.push(series_picker(&key, &titles, idx));
                }
                let burn = burns[idx].1.clone();
                let (start, reset) = (burn.start, burn.reset);
                let last_at = burn.samples.last().map(|s| s.at);
                let remaining = burn.latest_remaining();
                col = col.push(
                    iced::widget::Canvas::new(crate::charts::BurndownChart {
                        burn,
                        color: accent,
                    })
                    .width(Length::Fill)
                    .height(Length::Fixed(130.0)),
                );
                col = col.push(
                    row![
                        text(local_clock(start)).size(11).color(muted()),
                        Space::with_width(Length::Fill),
                        text(local_clock(reset)).size(11).color(muted()),
                    ]
                    .width(Length::Fill),
                );
                if let Some(r) = remaining {
                    col = col.push(detail_text(format!("{r:.0}% usage remaining")));
                }
                if let Some(t) = last_at {
                    col = col.push(
                        text(format!("Last known usage: {}", local_clock(t)))
                            .size(11)
                            .color(muted()),
                    );
                }
                col = col.push(rule());
            }
            let titles: Vec<String> = series.iter().map(|s| s.label.clone()).collect();
            let idx = selected_series(&key, series.len());
            if series.len() > 1 {
                col = col.push(series_picker(&key, &titles, idx));
            }
            let s = series[idx];
            let bars = crate::history::utilization_bars(s, tz);
            let hover = hover_for(&key);
            col = col.push(chart_canvas(crate::charts::BarChart {
                hover,
                key,
                color: accent,
                peak_cap: false,
                fmt: percent_label,
                axis: percent_label,
                y_max: Some(100.0),
                bars: bars.clone(),
            }));
            col = col.push(detail_text(popover_charts::detail_line(
                &bars,
                hover,
                percent_label,
            )));
            let cadence = if s.window_minutes < 1440 {
                "Peak used per day"
            } else {
                "Peak used per reset period"
            };
            col = col.push(
                text(format!(
                    "{cadence} · {} samples recorded locally",
                    s.entries.len()
                ))
                .size(11)
                .color(muted()),
            );
        }
    }

    // Tokens/min from the per-day `cost` payload.
    if let Some(payload) = payload {
        let tpm_bars = crate::throughput::daily_bars(&payload.daily);
        if let Some(sum) = crate::throughput::summary(&tpm_bars) {
            let key = format!("{provider}:tpm");
            let (head, open) = section(key.clone(), "Tokens / min · daily average");
            col = sep(col).push(head);
            if open {
                let hover = hover_for(&key);
                col = col.push(chart_canvas(crate::charts::BarChart {
                    hover,
                    key,
                    color: accent,
                    peak_cap: true,
                    fmt: crate::throughput::per_min,
                    axis: crate::throughput::count,
                    y_max: None,
                    bars: tpm_bars.clone(),
                }));
                col = col.push(detail_text(popover_charts::detail_line(
                    &tpm_bars,
                    hover,
                    crate::throughput::per_min,
                )));
                col = col.push(detail_text(format!(
                    "Latest {} · Peak {} on {} · Avg {}",
                    crate::throughput::count(sum.latest),
                    crate::throughput::count(sum.peak),
                    crate::charts::short_date(&sum.peak_date),
                    crate::throughput::count(sum.average),
                )));
                col = col.push(
                    text("Approximation: each day's tokens ÷ 1,440 min, not a measured per-minute rate.")
                        .size(11)
                        .color(muted()),
                );
            }
        }
    }

    // Storage breakdown (cached scan of the provider's local data dirs).
    if let Some(fp) = app.storage.get(provider).filter(|f| f.has_local_data()) {
        let key = format!("{provider}:storage");
        let (head, open) = section(key, "Storage");
        col = sep(col).push(head);
        if open {
            col = col.push(detail_text(format!(
                "Total: {}",
                crate::storage::bytes_label(fp.total_bytes)
            )));
            let rows = crate::storage::rows(fp);
            // One segmented share bar + dot legend (mac storage layout).
            let total = fp.total_bytes.max(1) as f64;
            let mut share = Row::new().spacing(1).height(Length::Fixed(5.0));
            for (i, (_, bytes)) in rows.iter().enumerate() {
                let portion = ((*bytes as f64 / total) * 1000.0).round().max(1.0) as u16;
                let seg = crate::charts::segment_color(i);
                share = share.push(
                    container(Space::new(Length::Fill, Length::Fixed(5.0)))
                        .width(Length::FillPortion(portion.max(1)))
                        .style(move |_: &Theme| container::Style {
                            background: Some(Background::Color(seg)),
                            border: Border::default().rounded(2.5),
                            ..Default::default()
                        }),
                );
            }
            col = col.push(share);
            for (i, (name, bytes)) in rows.into_iter().enumerate() {
                let dot = crate::charts::segment_color(i);
                let pct = bytes as f64 / total * 100.0;
                col = col.push(
                    row![
                        container(Space::new(Length::Fixed(7.0), Length::Fixed(7.0))).style(
                            move |_: &Theme| container::Style {
                                background: Some(Background::Color(dot)),
                                border: Border::default().rounded(3.5),
                                ..Default::default()
                            }
                        ),
                        text(name).size(12).width(Length::Fill),
                        text(format!(
                            "{} · {pct:.0}%",
                            crate::storage::bytes_label(bytes)
                        ))
                        .size(12)
                        .color(muted()),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                );
            }
            if fp.unreadable > 0 {
                col = col.push(
                    text(format!("{} unreadable item(s) skipped", fp.unreadable))
                        .size(11)
                        .color(muted()),
                );
            }
        }
    }
    col.into()
}

fn credits_label(v: f64) -> String {
    if v >= 100.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

fn chart_canvas(chart: crate::charts::BarChart) -> Element<'static, Message> {
    iced::widget::Canvas::new(chart)
        .width(Length::Fill)
        .height(Length::Fixed(130.0))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pace_detail_splits_like_mac() {
        assert_eq!(
            pace_detail("54% in reserve | Expected 55% used | Lasts until reset | 1.5× headroom"),
            ("54% in reserve".into(), Some("Lasts until reset · 1.5× headroom".into()))
        );
        assert_eq!(pace_detail("On pace | Expected 10% used"), ("On pace".into(), None));
    }

    #[test]
    fn plan_and_relative_time_match_mac() {
        assert_eq!(plan_display("free", "codex"), "Free");
        assert_eq!(plan_display("pro", "codex"), "Pro 20x");
        assert_eq!(plan_display("Claude Max Account", "claude"), "Max");
        assert_eq!(plan_display("AI Pro", "gemini"), "AI Pro");
        assert_eq!(relative_ago(100, 105), "just now");
        assert_eq!(relative_ago(0, 130), "2 min ago");
        assert_eq!(relative_ago(0, 7300), "2 hr ago");
    }

    #[test]
    fn switcher_wraps_like_mac() {
        // Mac menu (310 wide) with Overview + 5 providers: two rows of three.
        assert_eq!(switcher_row_count(310.0, 6), 2);
        assert_eq!(split_rows(6, 2), vec![3, 3]);
        // Few segments fit one row; 15+ always use four rows.
        assert_eq!(switcher_row_count(340.0, 4), 1);
        assert_eq!(switcher_row_count(340.0, 16), 4);
        assert_eq!(split_rows(7, 2), vec![4, 3]);
    }

    #[test]
    fn pace_stripe_color_green_under_pace_red_over_white_unknown() {
        assert_eq!(pace_stripe_color(-48.0), Color::from_rgb(0.19, 0.82, 0.35));
        assert_eq!(pace_stripe_color(0.0), Color::from_rgb(0.19, 0.82, 0.35));
        assert_eq!(pace_stripe_color(12.0), Color::from_rgb(1.0, 0.42, 0.38));
        assert_eq!(pace_stripe_color(f64::NAN), Color::WHITE);
    }

    #[test]
    fn warning_fraction_mirrors_display_mode() {
        assert!((warning_fraction(10, "remaining") - 0.10).abs() < 1e-6);
        assert!((warning_fraction(10, "used") - 0.90).abs() < 1e-6);
        assert!((warning_fraction(25, "bogus") - 0.25).abs() < 1e-6);
    }

    #[test]
    fn weekly_remaining_picks_longest_window() {
        let mut e = Entry {
            provider: "codex".into(),
            plan: String::new(),
            account_label: String::new(),
            error: String::new(),
            updated_at: String::new(),
            windows: vec![],
            credits: None,
            reset_credits_available: 0,
            reset_credits_next_expiry: String::new(),
            extra_usage_text: String::new(),
            details: vec![],
            status: String::new(),
            status_level: String::new(),
        };
        assert_eq!(weekly_remaining(&e), 0);
        e.windows = vec![
            QuotaWindow {
                key: "session".into(),
                scoped: false,
                label: "Session".into(),
                remaining: 80,
                minutes: 300,
                resets_at: String::new(),
                reset_text: String::new(),
                pace: String::new(),
                pace_expected: f64::NAN,
                pace_delta: f64::NAN,
            },
            QuotaWindow {
                key: "weekly".into(),
                scoped: false,
                label: "Weekly".into(),
                remaining: 42,
                minutes: 10080,
                resets_at: String::new(),
                reset_text: String::new(),
                pace: String::new(),
                pace_expected: f64::NAN,
                pace_delta: f64::NAN,
            },
        ];
        assert_eq!(weekly_remaining(&e), 42);
    }

    #[test]
    fn status_url_only_known_providers() {
        assert_eq!(status_url("codex"), Some("https://status.openai.com/"));
        assert_eq!(status_url("claude"), Some("https://status.claude.com/"));
        assert_eq!(status_url("nope"), None);
    }

    #[test]
    fn quota_display_value_remaining_and_used() {
        assert_eq!(quota_display_value(80, "remaining"), 80);
        assert_eq!(quota_display_value(80, "used"), 20);
        assert_eq!(quota_display_value(0, "used"), 100);
        assert_eq!(quota_display_value(100, "used"), 0);
        assert_eq!(quota_display_value(35, "bogus"), 35);
    }

    #[test]
    fn quota_display_value_clamps_out_of_range() {
        assert_eq!(quota_display_value(150, "remaining"), 100);
        assert_eq!(quota_display_value(-10, "remaining"), 0);
        assert_eq!(quota_display_value(150, "used"), 0);
        assert_eq!(quota_display_value(-10, "used"), 100);
    }

    #[test]
    fn quota_suffix_matches_mode() {
        assert_eq!(quota_display_suffix("used"), "used");
        assert_eq!(quota_display_suffix("remaining"), "left");
        assert_eq!(quota_display_suffix("bogus"), "left");
    }

    #[test]
    fn warning_predicate_honours_toggle_and_threshold_boundary() {
        assert!(is_warning(true, 10, 10));
        assert!(is_warning(true, 0, 10));
        assert!(!is_warning(true, 11, 10));
        assert!(!is_warning(false, 0, 10));
        assert!(!is_warning(false, 5, 99));
    }

    #[test]
    fn meter_fraction_clamps_to_unit_interval() {
        assert!((meter_fraction(0) - 0.0).abs() < f32::EPSILON);
        assert!((meter_fraction(50) - 0.5).abs() < f32::EPSILON);
        assert!((meter_fraction(100) - 1.0).abs() < f32::EPSILON);
        assert!((meter_fraction(250) - 1.0).abs() < f32::EPSILON);
        assert!((meter_fraction(-25) - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn pace_tick_fraction_converts_expected_used() {
        let f = pace_tick_fraction(true, 30.0, "used").unwrap();
        assert!((f - 0.30).abs() < 1e-6);
        let f = pace_tick_fraction(true, 30.0, "remaining").unwrap();
        assert!((f - 0.70).abs() < 1e-6);
        assert_eq!(pace_tick_fraction(false, 30.0, "used"), None);
        assert_eq!(pace_tick_fraction(true, 0.0, "used"), None);
        assert_eq!(pace_tick_fraction(true, f64::NAN, "used"), None);
        assert_eq!(pace_tick_fraction(true, 150.0, "used"), None);
    }

    #[test]
    fn parse_hex_rgb_accepts_hash_and_bare() {
        assert_eq!(parse_hex_rgb("#c98757"), Some((0xc9, 0x87, 0x57)));
        assert_eq!(parse_hex_rgb("c98757"), Some((0xc9, 0x87, 0x57)));
        assert_eq!(parse_hex_rgb("#D9874A"), Some((0xd9, 0x87, 0x4a)));
        assert_eq!(parse_hex_rgb("#xyz"), None);
        assert_eq!(parse_hex_rgb(""), None);
    }

    #[test]
    fn accent_rgb_defaults_when_descriptor_has_none() {
        assert_eq!(accent_rgb("no-such-provider"), (0xc9, 0x87, 0x57));
    }

    #[test]
    fn reset_display_countdown_absolute_both() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-24T08:25:42Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let at = "2026-11-24T09:25:42Z";
        assert_eq!(
            reset_display_text(at, "", "countdown", now),
            "Resets in 1h 0m"
        );
        let abs = reset_display_text(at, "", "absolute", now);
        assert!(abs.starts_with("Resets "), "{abs}");
        assert!(!abs.contains("Resets in"), "{abs}");
        let both = reset_display_text(at, "", "both", now);
        assert!(both.starts_with("Resets in 1h 0m · "), "{both}");
        assert_eq!(
            reset_display_text("2026-11-24T08:00:00Z", "", "countdown", now),
            "Reset due · refresh to update"
        );
        assert_eq!(
            reset_display_text("", "Resets soon", "countdown", now),
            "Resets soon"
        );
        assert_eq!(
            reset_display_text("", "", "countdown", now),
            "Reset time unavailable"
        );
        assert_eq!(
            reset_display_text("nonsense", "", "absolute", now),
            "Reset time unavailable"
        );
    }

    #[test]
    fn reset_countdown_minutes_and_days() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-24T08:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(
            reset_display_text("2026-11-24T08:05:30Z", "", "countdown", now),
            "Resets in 6m"
        );
        assert_eq!(
            reset_display_text("2026-11-26T10:00:00Z", "", "countdown", now),
            "Resets in 2d 2h"
        );
    }

    #[test]
    fn cost_line_formats_tokens_when_known() {
        assert_eq!(cost_line("Today", 0.04, Some(15_000)), "Today: $0.04 · 15K tokens");
        assert_eq!(cost_line("Today", 0.0, Some(0)), "Today: $0.00");
        assert_eq!(cost_line("Last 30 days", 254.24, None), "Last 30 days: $254.24");
    }

    #[test]
    fn tokens_on_finds_the_day() {
        let c = CostEntry {
            provider: "x".into(),
            today: 0.0,
            month: 0.0,
            tokens: 0,
            provenance: String::new(),
            error: String::new(),
            coverage: String::new(),
            daily: vec![("2026-11-09".into(), 1.0, 42)],
        };
        assert_eq!(tokens_on(&c, "2026-11-09"), Some(42));
        assert_eq!(tokens_on(&c, "2026-11-08"), None);
    }

    #[test]
    fn money_count_and_provenance_formatting() {
        assert_eq!(format_money(3.5), "$3.50");
        assert_eq!(format_count(1234567), "1,234,567");
        assert_eq!(format_count(0), "0");
        assert_eq!(provenance_label("listPriceEstimate"), "List-price estimate");
        assert_eq!(provenance_label("actual"), "Reported cost");
        assert_eq!(provenance_label("unknown"), "Cost source unavailable");
        assert_eq!(provenance_label(""), "Cost source unavailable");
        assert_eq!(provenance_label("customSource"), "customSource");
    }
}
